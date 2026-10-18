//! Landholdings on the floodplain, after the farming of medieval Europe.
//! Each holding is worked from a farmstead and holds the ground it reaches
//! most cheaply: a geodesic Voronoi diagram over the erodibility grid, in
//! which the channel and oxbow ground are barriers and ground near the
//! water costs more to cross. Holdings take up ground beside them once
//! vegetation binds it, so a holding grows onto the point bar its bank
//! builds, as land law gives a riparian owner the ground a river accretes.
//! A holding keeps ground the river cuts it off from, as the law keeps a
//! boundary on the old course after an avulsion, until a neighbour can
//! reach that ground more cheaply.
//!
//! Boundaries move only when a holding reorganises, which each does at
//! random, as a Poisson process with mean interval `reorganise_time`, so
//! holdings change one at a time. At its reorganisation a holding moves a
//! farmstead that the water threatens, is absorbed by its neighbours if it
//! has shrunk too far, takes ground it now reaches more cheaply than the
//! holding that has it, by a margin, and divides between heirs if it has
//! grown too large. A holding whose farmstead the river reaches reorganises
//! at once.
//!
//! The work is spread evenly over the steps. One survey of the ground runs
//! over [`SURVEY_STEPS`] steps, a band of rows at a time, and the searches
//! that move boundaries settle at most [`BUDGET`] cells a step, carrying on
//! at the next. One holding reorganises at a time.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use crate::farm::{Building, HAMLET_GAP, Reach};
use crate::field::Erodibility;
use crate::geom::P;
use crate::ponds::Pond;
use crate::rng::Rng;
use crate::smoothstep;

/// Owner of a cell that no holding has.
pub const NONE: u32 = u32::MAX;

/// Steps over which one survey of the ground is spread: the forward pass of
/// its distance transform in `BANDS` bands of rows, the backward pass in as
/// many, then the accretion of the open ground it found.
pub const SURVEY_STEPS: u64 = 40;
pub(crate) const BANDS: usize = 19;
/// Most cells a search settles in one step.
pub const BUDGET: usize = 128;
/// Distance, in channel widths, beyond the columns the holdings can take
/// to which the survey reads the water, so flood risk near their edges
/// counts the channel just beyond. The valley narrows to `pinch_width`
/// at the edges of the page's window, so the channel is never far.
const SURVEY_PAD: f64 = 8.0;

/// Distance from the water, in channel widths, at which flood risk falls
/// to a half.
const RISK_LENGTH: f64 = 6.0;
/// Most that high ground lowers flood risk, as a fraction of the risk
/// from distance: on ground whose overbank deposit has reached the flood
/// level, as on a levee crest or an old scroll ridge. Fresh bar sand and
/// silting lakes lie low and keep the full risk, so farmsteads keep to the
/// ridges beside the river, as on the stream ridges of the Rhine–Meuse
/// delta, and leave the low ground to meadow.
const RIDGE_RELIEF: f64 = 0.6;
/// Distance from the water, in channel widths, over which the relief of
/// high ground comes in: the bank itself stays at full risk, since the
/// channel undercuts it however high it stands.
const RIDGE_CLEAR: f64 = 1.5;
/// Distance beyond the valley wall, in channel widths, over which flood
/// risk falls to 0 on the upland.
const WALL_FADE: f64 = 2.0;
/// Wet ground: the low backswamps of the floodplain, which the levees'
/// silt has not reached, where water stands after a flood and drains
/// slowly. Ground the channel has reworked is wet as it has lain low for
/// long: its overbank deposit under `WET_DEPTH` of the flood level, fully so
/// `WET_RAMP` under it, after
/// `WET_AGE` of growth, fully so at twice that. Ground the channel has
/// never reached stands as old floodplain and stays dry. It keeps
/// `WET_NEAR` widths from the water, fully wet only `WET_FAR` from it, and
/// off the oxbow scars, which have marsh of their own. Its flood risk is at
/// least `WET_RISK` times its wetness, so farmsteads keep off it and its
/// fields are meadow.
const WET_DEPTH: f64 = 0.7;
const WET_RAMP: f64 = 0.2;
const WET_AGE: f64 = 100.0;
const WET_NEAR: f64 = 2.0;
const WET_FAR: f64 = 4.0;
const WET_RISK: f64 = 0.6;
/// Extra cost of crossing ground at flood risk 1, as a multiple of its
/// length: a holding reaches round wet ground before it reaches across it.
const RISK_COST: f32 = 2.0;
/// Range of the cost of crossing ground with the erodibility noise, as a
/// fraction either side of 1, at least 0.2. Boundaries then bend with the soils, as old
/// boundaries follow changes of ground, and are not held to the eight
/// directions a search steps in.
const ROUGHNESS: f32 = 1.0;
/// Highest flood risk at which a farmstead is founded, or moved to if the
/// holding has such ground, and the risk at which it is moved. The gap
/// between them keeps a farmstead from moving at every reorganisation.
pub(crate) const SEAT_RISK: f32 = 0.25;
pub(crate) const FLEE_RISK: f32 = 0.5;
/// A reorganising holding takes a neighbour's cell when it reaches it for
/// less than the neighbour's own cost, less this fraction and `SLACK`
/// widths. Without the margin, boundaries would move at every
/// reorganisation as the river changed the costs.
const MARGIN: f32 = 0.15;
const SLACK: f32 = 1.0;
/// Farthest a reorganising holding reaches from its farmstead, as a
/// multiple of the side of a square holding of `holding_area`.
const REACH: f64 = 2.0;
/// Areas, as multiples of `holding_area`, above which a holding divides
/// between heirs and below which its neighbours absorb it.
const DIVIDE: f64 = 2.0;
const ABSORB: f64 = 0.3;
/// Least distance between the first farmsteads, as a multiple of the side
/// of a square holding of `holding_area`.
const FOUNDING_GAP: f64 = 0.8;
/// Extra cost of crossing the parish boundary, in channel widths: half the
/// side of a square holding of the default `holding_area`. Holdings
/// keep to their own parish where one on the other side can reach the
/// ground, so field boundaries settle on the parish boundary. Ground the
/// river has moved to the far side of it, out of reach of its own
/// parish's holdings, is still taken from across it.
const BOUNDARY_COST: f32 = 8.0;
/// Cosine of the angle through which a holding's long axis must turn
/// before its fields are laid out again.
const AXIS_KEEP: f64 = 0.9;

/// What the holdings read from the simulation.
pub struct Settings {
    pub time: f64,
    /// Channel width in world units.
    pub width: f64,
    /// Area a holding settles to, in square channel widths.
    pub holding_area: f64,
    /// Mean time between one holding's reorganisations.
    pub reorganise_time: f64,
    /// Growth at which grass takes a point bar, after which a holding can
    /// take the ground.
    pub grazing_time: f64,
    /// Flood level above fresh bar sand, in channel depths: the overbank
    /// deposit at which ground stands clear of all but the highest water.
    pub levee_height: f64,
    /// Whether holdings raise farmsteads; see [`crate::farm`].
    pub farmsteads: bool,
    /// Chance that an heir's farmstead goes up beside the parent's, making
    /// a hamlet, when farmsteads are on.
    pub hamlet_chance: f64,
}

pub struct Holding {
    /// Farmstead, in world units.
    pub seat: P,
    /// Unit vector along the holding's longest extent when its fields were
    /// last laid out.
    pub axis: P,
    /// Seed of the field layout, under 2^24 so it is exact as an `f32`. It
    /// changes whenever the fields are laid out again.
    pub seed: u32,
    /// Time of the next reorganisation.
    pub next: f64,
    /// Cells held.
    pub area: usize,
    /// Columns and rows, inclusive, of a box holding every cell held. It
    /// grows as the holding takes ground and shrinks to fit when the
    /// fields are laid out, so the holding's cells can be found without a
    /// pass over the whole grid.
    pub bounds: [usize; 4],
}

/// Work queued for the holdings, done in order. A search runs over as
/// many steps as it needs.
enum Task {
    /// Let holdings take the open ground in `Land::free`.
    Accrete,
    /// Search out from a holding's farmstead; see [`Land::contest`].
    Contest {
        h: usize,
        margin: f32,
        slack: f32,
        only: Option<u32>,
    },
    /// Divide a holding between heirs if it has grown too large.
    Divide(usize),
    /// Fit a holding's box, and lay out its fields again if forced or its
    /// shape has turned.
    LayOut(usize, bool),
    /// Make one change to a holding's farmstead.
    Build(usize),
    /// The end of a reorganisation, which a renderer should draw.
    Done,
}

/// Holdings on a grid of the same layout as [`Erodibility`].
pub struct Land {
    pub cols: usize,
    pub rows: usize,
    pub cell: f64,
    /// Index into `holdings` of each cell's holding, or [`NONE`].
    pub owner: Vec<u32>,
    /// Cost, in channel widths, of reaching each held cell from its
    /// holding's farmstead, as of the cell's last search by that holding.
    pub cost: Vec<f32>,
    /// Flood risk of each cell in [0, 1]: 1 at the water's edge, falling
    /// with distance from the channel and oxbow ground and with the height
    /// of the overbank deposit, and 0 on the upland.
    pub risk: Vec<f32>,
    /// Wetness of each cell in [0, 1]: 1 on old low floodplain ground away
    /// from the water, 0 near it, on high ground, fresh bars, oxbow scars
    /// and the upland. See [`WET_DEPTH`].
    pub wet: Vec<f32>,
    /// Holdings by index; a dissolved holding leaves its slot empty for the
    /// next to be founded.
    pub holdings: Vec<Option<Holding>>,
    /// Counts finished reorganisations and accretions, which follow each
    /// survey of the ground the channel took and left, so a renderer can
    /// skip redrawing the fields between them.
    pub version: u64,
    /// Reorganisations begun so far.
    pub reorganisations: u64,
    /// The farmsteads' buildings and the ruins of those abandoned.
    pub buildings: Vec<Building>,
    /// Counts changes to `buildings`.
    pub building_version: u64,
    /// A point on the leat of each working mill, which the holding that
    /// has the ground there counts as its own.
    pub mills: Vec<P>,
    /// The fishponds, kept and silting; their changes count in
    /// `building_version`.
    pub ponds: Vec<Pond>,
    /// The courses of the oxbows whose lakes have filled, back to back,
    /// and the start of each in `scar_points`, with an end marker.
    pub(crate) scar_points: Vec<P>,
    pub(crate) scar_starts: Vec<usize>,
    /// The tributaries' courses, which no building stands across.
    pub(crate) streams: Vec<Reach>,
    /// Simulated time of the work under way.
    pub(crate) now: f64,
    /// Cells a holding can take: off the water, grassed over, and in
    /// `span`.
    pub(crate) open: Vec<bool>,
    /// Columns, from the first to one past the last, that holdings can
    /// take ground in, and that the survey reads, which reach
    /// [`SURVEY_PAD`] beyond them; see [`Land::confine`].
    span: [usize; 2],
    surveyed: [usize; 2],
    /// Cost of crossing each cell per unit length before flood risk, from
    /// the erodibility noise; see `ROUGHNESS`.
    pub(crate) rough: Vec<f32>,
    /// Side of the parish boundary each cell lies on, or empty before it is
    /// set; see [`Land::set_boundary`].
    side: Vec<bool>,
    /// Distance of each cell from the water, in cells, during a survey.
    pub(crate) dist: Vec<f32>,
    /// Cells under the channel when the current survey began, in order,
    /// and whether each cell is one of them.
    channel: Vec<u32>,
    under: Vec<bool>,
    /// Open cells no holding had when last surveyed or freed since.
    free: Vec<u32>,
    tasks: VecDeque<Task>,
    /// Whether the task at the front has a search under way.
    searching: bool,
    // Scratch for the searches, reset after each to infinity, with the
    // holding offering each cell its best cost.
    best: Vec<f32>,
    from: Vec<u32>,
    touched: Vec<usize>,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
    rng: Rng,
    /// Draws for the farmsteads, apart from the holdings' own, and for
    /// the churches and fishponds, apart from both, since the farmsteads'
    /// stream also decides the hamlets, which shape the holdings.
    pub(crate) farm_rng: Rng,
    pub(crate) works_rng: Rng,
    founded: bool,
}

impl Land {
    pub fn new(field: &Erodibility, seed: u64) -> Self {
        let n = field.cols * field.rows;
        Self {
            cols: field.cols,
            rows: field.rows,
            cell: field.cell,
            owner: vec![NONE; n],
            cost: vec![0.0; n],
            risk: vec![0.0; n],
            wet: vec![0.0; n],
            holdings: Vec::new(),
            version: 0,
            reorganisations: 0,
            buildings: Vec::new(),
            building_version: 0,
            mills: Vec::new(),
            ponds: Vec::new(),
            scar_points: Vec::new(),
            scar_starts: Vec::new(),
            streams: Vec::new(),
            now: 0.0,
            open: vec![false; n],
            span: [0, field.cols],
            surveyed: [0, field.cols],
            rough: field
                .noise
                .iter()
                .map(|&v| (1.0 + ROUGHNESS * (2.0 * v - 1.0)).max(0.2))
                .collect(),
            side: Vec::new(),
            dist: vec![f32::INFINITY; n],
            channel: Vec::new(),
            under: vec![false; n],
            free: Vec::new(),
            tasks: VecDeque::new(),
            searching: false,
            best: vec![f32::INFINITY; n],
            from: vec![NONE; n],
            touched: Vec::new(),
            heap: BinaryHeap::new(),
            rng: Rng::new(seed ^ 0x1A4D_5EED),
            farm_rng: Rng::new(seed ^ 0x0FA2_3570),
            works_rng: Rng::new(seed ^ 0x5EC7_0C4E),
            founded: false,
        }
    }

    /// Keep the holdings to the columns between `x0` and `x1`, so the
    /// farmsteads, tracks and works on them do too, and the survey to
    /// within [`SURVEY_PAD`] of them, for channel width `width`.
    pub fn confine(&mut self, x0: f64, x1: f64, width: f64) {
        let col = |x: f64| (x.max(0.0) as usize).min(self.cols);
        let pad = SURVEY_PAD * width;
        self.span = [col(x0 / self.cell), col((x1 / self.cell).ceil())];
        self.surveyed = [
            col((x0 - pad) / self.cell),
            col(((x1 + pad) / self.cell).ceil()),
        ];
    }

    /// Whether cell `i` lies in the columns holdings can take.
    pub(crate) fn within(&self, i: usize) -> bool {
        (self.span[0]..self.span[1]).contains(&(i % self.cols))
    }

    pub(crate) fn index(&self, p: P) -> usize {
        let x = ((p[0] / self.cell).max(0.0) as usize).min(self.cols - 1);
        let y = ((p[1] / self.cell).max(0.0) as usize).min(self.rows - 1);
        y * self.cols + x
    }

    /// Whether cell `i` lay under the channel when the last survey began.
    pub(crate) fn in_channel(&self, i: usize) -> bool {
        self.under[i]
    }

    pub(crate) fn centre(&self, i: usize) -> P {
        [
            ((i % self.cols) as f64 + 0.5) * self.cell,
            ((i / self.cols) as f64 + 0.5) * self.cell,
        ]
    }

    /// Number of live holdings.
    pub fn count(&self) -> usize {
        self.holdings.iter().flatten().count()
    }

    /// Whether the parish boundary has been set.
    pub(crate) fn parish(&self) -> bool {
        !self.side.is_empty()
    }

    /// Set the parish boundary on `line`, which runs from the domain's
    /// upstream edge to its downstream edge, `width` wide; its ends are
    /// carried on straight to the edges. A cell's side is the parity of the
    /// boundary's crossings of its column above it.
    pub fn set_boundary(&mut self, line: &[P], width: f64) {
        let (cols, c) = (self.cols, self.cell);
        let (Some(&first), Some(&last)) = (line.first(), line.last()) else {
            return;
        };
        let ends = [[-1.0, first[1]], first];
        let tail = [last, [width + 1.0, last[1]]];
        let segments = line.windows(2).chain([&ends[..], &tail[..]]);
        let mut cross = vec![Vec::new(); cols];
        for seg in segments {
            let (a, b) = (seg[0], seg[1]);
            let lo = ((a[0].min(b[0]) / c - 0.5).ceil().max(0.0)) as usize;
            let hi = ((a[0].max(b[0]) / c - 0.5).ceil().max(0.0) as usize).min(cols);
            for (x, ys) in cross.iter_mut().enumerate().take(hi).skip(lo) {
                let cx = (x as f64 + 0.5) * c;
                let t = (cx - a[0]) / (b[0] - a[0]);
                ys.push(a[1] + t * (b[1] - a[1]));
            }
        }
        self.side = vec![false; cols * self.rows];
        for (x, ys) in cross.iter_mut().enumerate() {
            ys.sort_by(f64::total_cmp);
            let mut k = 0;
            for y in 0..self.rows {
                let cy = (y as f64 + 0.5) * c;
                while k < ys.len() && ys[k] < cy {
                    k += 1;
                }
                self.side[y * cols + x] = k % 2 == 1;
            }
        }
    }

    /// Run a whole survey and all the work it leads to at once, founding
    /// the first farmsteads if there are none. `valley` holds the valley
    /// floor's y range at each grid column.
    pub fn update(
        &mut self,
        field: &Erodibility,
        line: &[P],
        widths: &[f64],
        valley: &[(f64, f64)],
        s: &Settings,
    ) {
        for phase in 0..SURVEY_STEPS {
            self.survey(field, line, widths, valley, s, phase);
        }
        self.work(field, s, usize::MAX);
    }

    /// One step's share of the work: the survey's work for step `phase` of
    /// its [`SURVEY_STEPS`], then up to [`BUDGET`] cells of searching, or
    /// one other task.
    pub fn tick(
        &mut self,
        field: &Erodibility,
        line: &[P],
        widths: &[f64],
        valley: &[(f64, f64)],
        s: &Settings,
        phase: u64,
    ) {
        self.survey(field, line, widths, valley, s, phase);
        self.work(field, s, BUDGET);
    }

    /// Step `phase` of a survey. The channel is read when it starts, and
    /// each band of rows marks its open cells and runs the forward pass of
    /// a chamfer distance transform from the water. The backward pass then
    /// runs from the last band, setting each row's flood risk, handing back
    /// ground the water has taken, and listing open ground no holding has,
    /// for the accretion that ends the survey.
    fn survey(
        &mut self,
        field: &Erodibility,
        line: &[P],
        widths: &[f64],
        valley: &[(f64, f64)],
        s: &Settings,
        phase: u64,
    ) {
        let p = phase as usize;
        let rows = self.rows;
        let band = |k: usize| (k * rows / BANDS, (k + 1) * rows / BANDS);
        if p == 0 {
            self.read_channel(line, widths, s);
        }
        if p < BANDS {
            let (y0, y1) = band(p);
            self.forward(field, s, y0, y1);
        } else if p < 2 * BANDS {
            let (y0, y1) = band(2 * BANDS - 1 - p);
            self.backward(field, valley, s, y0, y1);
        } else if p == 2 * BANDS {
            self.now = s.time;
            self.weather(s.width);
            self.weather_ponds(field, s.width);
            if self.founded {
                self.tasks.push_back(Task::Accrete);
                if s.farmsteads {
                    self.rehouse(s);
                }
            } else {
                self.founded = true;
                self.found(s);
            }
        }
    }

    /// Queue a house for each holding that has lost its own, to the water
    /// or a stream, or has none since it was founded or divided, so it
    /// rebuilds within a survey, 10 years, instead of at its next
    /// reorganisation, 150 years away on average.
    fn rehouse(&mut self, s: &Settings) {
        for h in 0..self.holdings.len() {
            let queued = (self.tasks.iter()).any(|t| matches!(*t, Task::Build(k) if k == h));
            if !queued && !self.housed(h, s.width) {
                self.tasks.extend([Task::Build(h), Task::Done]);
            }
        }
    }

    /// List the cells under the channel: within half its local width of
    /// each point, and at least the cell under it, so its cells join
    /// diagonally at worst, which the searches do not cross.
    fn read_channel(&mut self, line: &[P], widths: &[f64], s: &Settings) {
        let (cols, rows, c) = (self.cols, self.rows, self.cell);
        for &i in &self.channel {
            self.under[i as usize] = false;
        }
        self.channel.clear();
        for (p, &b) in line.iter().zip(widths) {
            let r = 0.5 * b * s.width;
            let x0 = ((p[0] - r) / c).floor().max(0.0) as usize;
            let y0 = ((p[1] - r) / c).floor().max(0.0) as usize;
            let x1 = (((p[0] + r) / c).max(0.0) as usize).min(cols - 1);
            let y1 = (((p[1] + r) / c).max(0.0) as usize).min(rows - 1);
            for y in y0..=y1 {
                let dy = (y as f64 + 0.5) * c - p[1];
                for x in x0..=x1 {
                    let dx = (x as f64 + 0.5) * c - p[0];
                    if dx * dx + dy * dy <= r * r {
                        self.channel.push((y * cols + x) as u32);
                    }
                }
            }
            self.channel.push(self.index(*p) as u32);
        }
        self.channel.sort_unstable();
        self.channel.dedup();
        for &i in &self.channel {
            self.under[i as usize] = true;
        }
    }

    fn forward(&mut self, field: &Erodibility, s: &Settings, y0: usize, y1: usize) {
        let cols = self.cols;
        let bound = s.grazing_time as f32;
        let (lo, hi) = (y0 * cols, y1 * cols);
        let ([x0, x1], span) = (self.surveyed, self.span[0]..self.span[1]);
        for y in y0..y1 {
            for i in y * cols + x0..y * cols + x1 {
                let water = field.cut_at[i].is_finite();
                self.open[i] = !water && field.growth[i] >= bound && span.contains(&(i % cols));
                self.dist[i] = if water { 0.0 } else { f32::INFINITY };
            }
        }
        // Cells outside the survey's columns are never read, so the
        // channel's may be marked there too.
        let a = self.channel.partition_point(|&i| (i as usize) < lo);
        let b = self.channel.partition_point(|&i| (i as usize) < hi);
        for &i in &self.channel[a..b] {
            self.open[i as usize] = false;
            self.dist[i as usize] = 0.0;
        }
        let (d, diag) = (&mut self.dist, std::f32::consts::SQRT_2);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * cols + x;
                let mut v = d[i];
                if x > x0 {
                    v = v.min(d[i - 1] + 1.0);
                }
                if y > 0 {
                    v = v.min(d[i - cols] + 1.0);
                    if x > x0 {
                        v = v.min(d[i - cols - 1] + diag);
                    }
                    if x + 1 < x1 {
                        v = v.min(d[i - cols + 1] + diag);
                    }
                }
                d[i] = v;
            }
        }
    }

    fn backward(
        &mut self,
        field: &Erodibility,
        valley: &[(f64, f64)],
        s: &Settings,
        y0: usize,
        y1: usize,
    ) {
        let (cols, rows, c) = (self.cols, self.rows, self.cell);
        let (per_cell, fade, diag) = (
            c / (RISK_LENGTH * s.width),
            WALL_FADE * s.width,
            std::f32::consts::SQRT_2,
        );
        let (clear, stage) = (c / (RIDGE_CLEAR * s.width), s.levee_height);
        let (near, far) = (WET_NEAR * s.width / c, (WET_FAR - WET_NEAR) * s.width / c);
        let [x0, x1] = self.surveyed;
        for y in (y0..y1).rev() {
            let wy = (y as f64 + 0.5) * c;
            for x in (x0..x1).rev() {
                let i = y * cols + x;
                let d = &mut self.dist;
                let mut v = d[i];
                if x + 1 < x1 {
                    v = v.min(d[i + 1] + 1.0);
                }
                if y + 1 < rows {
                    v = v.min(d[i + cols] + 1.0);
                    if x + 1 < x1 {
                        v = v.min(d[i + cols + 1] + diag);
                    }
                    if x > x0 {
                        v = v.min(d[i + cols - 1] + diag);
                    }
                }
                d[i] = v;
                let (lo, hi) = valley[x];
                let inside = (((wy - lo).min(hi - wy)) / fade + 1.0).clamp(0.0, 1.0);
                let u = v as f64 * per_cell;
                let high = if stage > 0.0 {
                    (field.deposit[i] as f64 / stage).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let relief = RIDGE_RELIEF * high * (v as f64 * clear).min(1.0);
                // On the floor itself, fading in over a width from the wall.
                let floor = (((wy - lo).min(hi - wy)) / s.width).clamp(0.0, 1.0);
                let low = smoothstep((WET_DEPTH - high) / WET_RAMP);
                let growth = field.growth[i] as f64;
                let old = smoothstep(growth / WET_AGE - 1.0);
                let wet = if field.cut_at[i].is_finite() || growth.is_infinite() {
                    0.0
                } else {
                    floor * low * old * smoothstep((v as f64 - near) / far)
                };
                self.wet[i] = wet as f32;
                let risk = (inside * (1.0 - relief) / (1.0 + u * u)).max(WET_RISK * wet);
                self.risk[i] = risk as f32;
                let o = self.owner[i];
                if o != NONE && !self.open[i] {
                    self.owner[i] = NONE;
                    if let Some(h) = self.holdings[o as usize].as_mut() {
                        h.area -= 1;
                    }
                } else if o == NONE && self.open[i] {
                    self.free.push(i as u32);
                }
            }
        }
    }

    /// Do queued work until `budget` cells have been searched, starting the
    /// next due reorganisation whenever the queue empties. Each task other
    /// than a search or a farmstead's change costs a quarter of a step's
    /// budget.
    fn work(&mut self, field: &Erodibility, s: &Settings, mut budget: usize) {
        self.now = s.time;
        let cw = (self.cell / s.width) as f32;
        let limit = (REACH * self.target(s).1) as f32;
        while budget > 0 {
            if self.tasks.is_empty() && !self.begin(s) {
                return;
            }
            let done = match *self.tasks.front().unwrap() {
                Task::Accrete => {
                    if !self.searching {
                        self.seed_accrete();
                    }
                    let done = self.accrete(cw, &mut budget);
                    self.version += done as u64;
                    done
                }
                Task::Contest {
                    h,
                    margin,
                    slack,
                    only,
                } => {
                    if !self.searching {
                        self.seed_contest(h);
                    }
                    self.contest(h, margin, slack, only, limit, cw, &mut budget)
                }
                Task::Divide(h) => {
                    self.tasks.pop_front();
                    self.divide(h, s);
                    budget = budget.saturating_sub(BUDGET / 4);
                    continue;
                }
                Task::LayOut(h, force) => {
                    self.lay_out(h, force);
                    budget = budget.saturating_sub(BUDGET / 4);
                    true
                }
                // A farmstead's change is small next to a search, and
                // costs nothing from the budget, so the holdings run as
                // they would without farmsteads.
                Task::Build(h) => {
                    self.build(h, s, field);
                    true
                }
                Task::Done => {
                    self.version += 1;
                    true
                }
            };
            if done {
                self.tasks.pop_front();
                self.clear_search();
            } else {
                self.searching = true;
            }
        }
    }

    /// Start the reorganisation of the holding most overdue, if any is: its
    /// farmstead is gone or threatened, or its time has come.
    fn begin(&mut self, s: &Settings) -> bool {
        let mut due: Option<(f64, usize)> = None;
        for (k, slot) in self.holdings.iter().enumerate() {
            if let Some(h) = slot {
                let i = self.index(h.seat);
                let when = if self.owner[i] != k as u32 || self.risk[i] > FLEE_RISK {
                    f64::NEG_INFINITY
                } else {
                    h.next
                };
                if when <= s.time && due.is_none_or(|(t, _)| when < t) {
                    due = Some((when, k));
                }
            }
        }
        let Some((_, h)) = due else { return false };
        self.reorganisations += 1;
        let next = self.wait(s);
        let seat = self.holdings[h].as_ref().map_or([0.0; 2], |x| x.seat);
        let si = self.index(seat);
        let mut moved = false;
        if self.owner[si] != h as u32 || self.risk[si] > FLEE_RISK {
            match self.refuge(h, seat) {
                Some(i) => {
                    let p = self.centre(i);
                    if let Some(x) = self.holdings[h].as_mut() {
                        x.seat = p;
                    }
                    moved = true;
                }
                None => {
                    self.dissolve(h);
                    return true;
                }
            }
        }
        let area = self.holdings[h].as_ref().map_or(0, |x| x.area);
        if (area as f64) < ABSORB * self.target(s).0 {
            self.dissolve(h);
            return true;
        }
        if let Some(x) = self.holdings[h].as_mut() {
            x.next = next;
        }
        self.tasks.extend([
            Task::Contest {
                h,
                margin: MARGIN,
                slack: SLACK,
                only: None,
            },
            Task::Divide(h),
            Task::LayOut(h, moved),
        ]);
        if s.farmsteads {
            self.tasks.push_back(Task::Build(h));
        }
        self.tasks.push_back(Task::Done);
        true
    }

    /// Cells one step from `i` that a search can enter, with the cost of
    /// the step: its length in channel widths, `cw` for a side step, times
    /// the roughness of the cell entered, raised by its flood risk. A diagonal step needs both
    /// cells beside it open, so a search never slips between two water
    /// cells that meet at a corner.
    fn steps(&self, i: usize, cw: f32, out: &mut [(usize, f32); 8]) -> usize {
        let (cols, rows, open) = (self.cols, self.rows, &self.open);
        let (x, y) = (i % cols, i / cols);
        let (l, r, u, d) = (
            x > 0 && open[i - 1],
            x + 1 < cols && open[i + 1],
            y > 0 && open[i - cols],
            y + 1 < rows && open[i + cols],
        );
        let diag = cw * std::f32::consts::SQRT_2;
        let mut n = 0;
        let mut add = |ok: bool, j: usize, len: f32| {
            if ok {
                let parish = match self.side.get(j) {
                    Some(&side) if side != self.side[i] => BOUNDARY_COST,
                    _ => 0.0,
                };
                out[n] = (j, len * (self.rough[j] + RISK_COST * self.risk[j]) + parish);
                n += 1;
            }
        };
        if l {
            add(true, i - 1, cw);
        }
        if r {
            add(true, i + 1, cw);
        }
        if u {
            add(true, i - cols, cw);
            add(l && open[i - cols - 1], i.wrapping_sub(cols + 1), diag);
            add(r && open[i - cols + 1], i - cols + 1, diag);
        }
        if d {
            add(true, i + cols, cw);
            add(l && open[i + cols - 1], i + cols - 1, diag);
            add(r && open[i + cols + 1], i + cols + 1, diag);
        }
        n
    }

    fn push(&mut self, i: usize, cost: f32, from: u32) {
        if self.best[i].is_infinite() {
            self.touched.push(i);
        }
        self.best[i] = cost;
        self.from[i] = from;
        self.heap.push(Reverse((cost.to_bits(), i as u32)));
    }

    fn clear_search(&mut self) {
        for &i in &self.touched {
            self.best[i] = f32::INFINITY;
            self.from[i] = NONE;
        }
        self.touched.clear();
        self.heap.clear();
        self.searching = false;
    }

    /// Give cell `i` to holding `h`, from whichever holding had it.
    fn claim(&mut self, i: usize, h: u32, cost: f32) {
        if let Some(g) = self
            .holdings
            .get_mut(self.owner[i] as usize)
            .and_then(Option::as_mut)
        {
            g.area -= 1;
        }
        self.owner[i] = h;
        self.cost[i] = cost;
        let (x, y) = (i % self.cols, i / self.cols);
        if let Some(g) = self.holdings[h as usize].as_mut() {
            g.area += 1;
            let b = &mut g.bounds;
            *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    }

    /// Cells inside holding `h`'s box.
    pub(crate) fn boxed(&self, h: usize) -> impl Iterator<Item = usize> + '_ {
        let [x0, y0, x1, y1] = self.holdings[h].as_ref().map_or([1, 1, 0, 0], |g| g.bounds);
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| y * self.cols + x))
    }

    /// Start an accretion from every held cell beside the free ground, at
    /// the cost it is held at.
    fn seed_accrete(&mut self) {
        let free = std::mem::take(&mut self.free);
        let (cols, n) = (self.cols, self.owner.len());
        for &f in &free {
            let f = f as usize;
            if !self.open[f] || self.owner[f] != NONE {
                continue;
            }
            let (x, y) = (f % cols, f / cols);
            for (ok, j) in [
                (x > 0, f.wrapping_sub(1)),
                (x + 1 < cols, f + 1),
                (y > 0, f.wrapping_sub(cols)),
                (f + cols < n, f + cols),
            ] {
                if ok && self.owner[j] != NONE && self.cost[j] < self.best[j] {
                    self.push(j, self.cost[j], self.owner[j]);
                }
            }
        }
        self.free = free;
        self.free.clear();
    }

    /// Carry on an accretion: each holding takes the free open ground it
    /// reaches first from its own. Returns whether it has finished.
    fn accrete(&mut self, cw: f32, budget: &mut usize) -> bool {
        let mut out = [(0, 0.0); 8];
        while *budget > 0 {
            let Some(Reverse((bits, i))) = self.heap.pop() else {
                return true;
            };
            let (i, c) = (i as usize, f32::from_bits(bits));
            if c > self.best[i] || !self.open[i] {
                continue;
            }
            *budget -= 1;
            let (o, now) = (self.from[i], self.owner[i]);
            if now == NONE {
                self.claim(i, o, c);
            } else if now != o {
                continue;
            }
            let n = self.steps(i, cw, &mut out);
            for &(j, len) in &out[..n] {
                if self.owner[j] == NONE && c + len < self.best[j] {
                    self.push(j, c + len, o);
                }
            }
        }
        false
    }

    fn seed_contest(&mut self, h: usize) {
        if let Some(seat) = self.holdings[h].as_ref().map(|x| x.seat) {
            let i = self.index(seat);
            if self.open[i] {
                self.push(i, 0.0, h as u32);
            }
        }
    }

    /// Carry on a search out from holding `h`'s farmstead over open ground
    /// within `limit` widths, through its own ground and the ground it
    /// takes. It takes ground no holding has, and a cell of another holding
    /// (of `only`, if given) when it reaches it for less than that
    /// holding's cost by the fraction `margin` and `slack` widths. Each cell
    /// it holds is costed afresh. Returns whether it has finished.
    #[allow(clippy::too_many_arguments)]
    fn contest(
        &mut self,
        h: usize,
        margin: f32,
        slack: f32,
        only: Option<u32>,
        limit: f32,
        cw: f32,
        budget: &mut usize,
    ) -> bool {
        if self.holdings[h].is_none() {
            return true;
        }
        let me = h as u32;
        let mut out = [(0, 0.0); 8];
        while *budget > 0 {
            let Some(Reverse((bits, i))) = self.heap.pop() else {
                return true;
            };
            let (i, c) = (i as usize, f32::from_bits(bits));
            if c > self.best[i] || c > limit || !self.open[i] {
                continue;
            }
            *budget -= 1;
            let o = self.owner[i];
            let take = o == me
                || o == NONE
                || (only.is_none_or(|g| g == o) && c * (1.0 + margin) + slack < self.cost[i]);
            if !take {
                continue;
            }
            self.claim(i, me, c);
            let n = self.steps(i, cw, &mut out);
            for &(j, len) in &out[..n] {
                if c + len < self.best[j] {
                    self.push(j, c + len, me);
                }
            }
        }
        false
    }

    fn wait(&mut self, s: &Settings) -> f64 {
        s.time - (1.0 - self.rng.uniform()).ln() * s.reorganise_time.max(1e-9)
    }

    fn add(&mut self, seat: P, s: &Settings) -> usize {
        let (x, y) = (
            (seat[0] / self.cell) as usize,
            (seat[1] / self.cell) as usize,
        );
        let holding = Holding {
            seat,
            axis: [1.0, 0.0],
            seed: (self.rng.next_u64() >> 40) as u32,
            next: self.wait(s),
            area: 0,
            bounds: [x, y, x, y],
        };
        match self.holdings.iter().position(Option::is_none) {
            Some(k) => {
                self.holdings[k] = Some(holding);
                k
            }
            None => {
                self.holdings.push(Some(holding));
                self.holdings.len() - 1
            }
        }
    }

    /// Area a holding settles to, in cells, and the side of a square
    /// holding of that area in channel widths.
    pub(crate) fn target(&self, s: &Settings) -> (f64, f64) {
        let cw = self.cell / s.width;
        (s.holding_area / (cw * cw), s.holding_area.sqrt())
    }

    /// Found farmsteads at random on open ground away from the water, no
    /// two nearer than `FOUNDING_GAP` of a holding's side, about one for
    /// each `holding_area` of open ground, and queue the sharing of the
    /// ground between them.
    fn found(&mut self, s: &Settings) {
        let (cells, side) = self.target(s);
        let open = self.open.iter().filter(|&&o| o).count();
        let want = (open as f64 / cells).round().max(1.0) as usize;
        let gap = FOUNDING_GAP * side * s.width;
        let mut seats: Vec<P> = Vec::new();
        let [x0, x1] = self.span;
        let across = (x1 - x0).max(1);
        for _ in 0..30 * want {
            if seats.len() >= want {
                break;
            }
            let k = (self.rng.uniform() * (across * self.rows) as f64) as usize;
            let i = (k / across) * self.cols + x0 + k % across;
            if !self.open[i] || self.risk[i] > SEAT_RISK {
                continue;
            }
            let p = self.centre(i);
            if seats
                .iter()
                .all(|q| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) >= gap * gap)
            {
                seats.push(p);
            }
        }
        for p in seats {
            let k = self.add(p, s);
            let i = self.index(p);
            self.claim(i, k as u32, 0.0);
            self.tasks.push_back(Task::LayOut(k, true));
            if s.farmsteads {
                self.tasks.push_back(Task::Build(k));
            }
        }
        self.tasks.push_front(Task::Accrete);
        self.tasks.push_back(Task::Done);
    }

    /// Where holding `h` moves a farmstead from `seat`: its nearest cell
    /// safe enough to found one on, or failing that its least threatened
    /// cell below the risk that moves one, if it has either.
    fn refuge(&self, h: usize, seat: P) -> Option<usize> {
        let me = h as u32;
        let key = |i: usize| {
            let (p, r) = (self.centre(i), self.risk[i]);
            let d2 = (p[0] - seat[0]).powi(2) + (p[1] - seat[1]).powi(2);
            if r <= SEAT_RISK {
                (0, d2)
            } else {
                (1, r as f64)
            }
        };
        self.boxed(h)
            .filter(|&i| self.owner[i] == me && self.risk[i] < FLEE_RISK)
            .min_by(|&a, &b| {
                let (ka, kb) = (key(a), key(b));
                ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
            })
    }

    /// Give holding `h`'s ground up, and queue its neighbours taking it.
    fn dissolve(&mut self, h: usize) {
        let me = h as u32;
        let cells: Vec<usize> = self.boxed(h).filter(|&i| self.owner[i] == me).collect();
        for i in cells {
            self.owner[i] = NONE;
            self.free.push(i as u32);
        }
        self.abandon(h, false);
        self.holdings[h] = None;
        self.tasks.extend([Task::Accrete, Task::Done]);
    }

    /// Divide holding `h` between two heirs if it has grown too large: a
    /// new farmstead on the safe cell of `h` farthest from its own takes
    /// the ground it reaches first. With farmsteads on, the heir builds
    /// beside the parent instead with chance `hamlet_chance`, on the
    /// nearest safe cell at least [`HAMLET_GAP`] widths from its seat, as
    /// heirs did under partible inheritance. The heir's tasks go to the
    /// front of the queue.
    fn divide(&mut self, h: usize, s: &Settings) {
        let me = h as u32;
        let area = self.holdings[h].as_ref().map_or(0, |x| x.area);
        if (area as f64) <= DIVIDE * self.target(s).0 {
            return;
        }
        let safe = |i: &usize| self.owner[*i] == me && self.risk[*i] <= SEAT_RISK;
        let hamlet =
            s.farmsteads && s.hamlet_chance > 0.0 && self.farm_rng.uniform() < s.hamlet_chance;
        let seat = self.holdings[h].as_ref().map_or([0.0; 2], |x| x.seat);
        let gap = (HAMLET_GAP * s.width).powi(2);
        let d2 = |i: usize| {
            let p = self.centre(i);
            (p[0] - seat[0]).powi(2) + (p[1] - seat[1]).powi(2)
        };
        let near = hamlet
            .then(|| {
                self.boxed(h)
                    .filter(safe)
                    .filter(|&i| d2(i) >= gap)
                    .min_by(|&a, &b| d2(a).total_cmp(&d2(b)))
            })
            .flatten();
        let far = near.or_else(|| {
            self.boxed(h)
                .filter(safe)
                .max_by(|&a, &b| self.cost[a].total_cmp(&self.cost[b]))
        });
        let Some(i) = far else { return };
        let k = self.add(self.centre(i), s);
        if s.farmsteads {
            self.tasks.push_front(Task::Build(k));
        }
        self.tasks.push_front(Task::LayOut(k, true));
        self.tasks.push_front(Task::Contest {
            h: k,
            margin: 0.0,
            slack: 0.0,
            only: Some(me),
        });
    }

    /// Fit holding `h`'s box to its cells, then set its axis to its longest
    /// extent, the major axis of its cells' second moments, and draw a new
    /// field seed, when `force` is set or the extent has turned past
    /// `AXIS_KEEP` from the axis. A holding left with no ground is removed.
    fn lay_out(&mut self, h: usize, force: bool) {
        let me = h as u32;
        let cols = self.cols;
        let (mut n, mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let mut b = [usize::MAX, usize::MAX, 0, 0];
        for i in self.boxed(h).filter(|&i| self.owner[i] == me) {
            let (x, y) = (i % cols, i / cols);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            let (x, y) = (x as f64, y as f64);
            n += 1.0;
            sx += x;
            sy += y;
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
        }
        if n == 0.0 {
            self.abandon(h, false);
            self.holdings[h] = None;
            return;
        }
        let (mx, my) = (sx / n, sy / n);
        let (a, d, c) = (sxx / n - mx * mx, syy / n - my * my, sxy / n - mx * my);
        let lambda = 0.5 * (a + d) + (0.25 * (a - d) * (a - d) + c * c).sqrt();
        let v = if c.abs() > 1e-9 {
            [lambda - d, c]
        } else if a >= d {
            [1.0, 0.0]
        } else {
            [0.0, 1.0]
        };
        let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let axis = [v[0] / len, v[1] / len];
        let seed = (self.rng.next_u64() >> 40) as u32;
        if let Some(x) = self.holdings[h].as_mut() {
            x.bounds = b;
            if force || (axis[0] * x.axis[0] + axis[1] * x.axis[1]).abs() < AXIS_KEEP {
                x.axis = axis;
                x.seed = seed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::farm::Kind;

    type Valley = (Erodibility, Vec<P>, Vec<f64>, Vec<(f64, f64)>);

    /// A valley 120 by 60 widths of width 1, whose channel runs straight
    /// along y = `y`, swept so the ground under it is bare.
    fn valley(y: f64) -> Valley {
        let mut field = Erodibility::new([120.0, 60.0], 0.5, 25.0, 1);
        let line: Vec<P> = (0..=240).map(|k| [k as f64 * 0.5, y]).collect();
        field.sweep(&line);
        let widths = vec![1.0; line.len()];
        let walls = vec![(5.0, 55.0); field.cols];
        (field, line, widths, walls)
    }

    fn settings(time: f64) -> Settings {
        Settings {
            time,
            width: 1.0,
            holding_area: 200.0,
            reorganise_time: 60.0,
            grazing_time: 100.0,
            levee_height: 0.25,
            farmsteads: true,
            hamlet_chance: 0.5,
        }
    }

    fn seat_y(land: &Land, o: u32) -> f64 {
        land.holdings[o as usize].as_ref().unwrap().seat[1]
    }

    #[test]
    fn holdings_keep_to_their_side_of_the_parish_boundary() {
        let (field, line, widths, walls) = valley(30.0);
        // An old course that weaves across the channel, so holdings stand
        // on both sides of it on each bank.
        let curve = |x: f64| 30.0 + 20.0 * (x * std::f64::consts::TAU / 80.0).sin();
        let parish: Vec<P> = (0..=240)
            .map(|k| [k as f64 * 0.5, curve(k as f64 * 0.5)])
            .collect();
        let straddling = |bounded: bool| {
            let mut land = Land::new(&field, 1);
            if bounded {
                land.set_boundary(&parish, 120.0);
                for i in 0..land.owner.len() {
                    let [x, y] = land.centre(i);
                    if (y - curve(x)).abs() > 1.0 {
                        assert_eq!(land.side[i], y > curve(x), "cell at ({x}, {y})");
                    }
                }
            }
            land.update(&field, &line, &widths, &walls, &settings(0.0));
            // Cells held across the boundary from their holding's farmstead.
            let mut across = 0;
            for (i, &o) in land.owner.iter().enumerate() {
                let [x, y] = land.centre(i);
                if o != NONE && (y - curve(x)).abs() > 1.0 {
                    let seat = land.holdings[o as usize].as_ref().unwrap().seat;
                    across += usize::from((y > curve(x)) != (seat[1] > curve(seat[0])));
                }
            }
            across
        };
        let (free, bounded) = (straddling(false), straddling(true));
        assert!(
            2 * bounded < free,
            "{bounded} cells held across the boundary, against {free} without it"
        );
    }

    #[test]
    fn holdings_take_the_land_and_stop_at_the_channel() {
        let (field, line, widths, walls) = valley(30.0);
        let mut land = Land::new(&field, 1);
        land.update(&field, &line, &widths, &walls, &settings(0.0));
        let count = land.count();
        assert!((15..=45).contains(&count), "{count} holdings");
        for i in 0..land.owner.len() {
            let (o, y) = (land.owner[i], (i / land.cols) as f64 * 0.5 + 0.25);
            assert_eq!(o == NONE, !land.open[i], "every open cell is held");
            if o != NONE {
                assert_eq!(
                    y < 30.0,
                    seat_y(&land, o) < 30.0,
                    "a holding stays on its own side"
                );
            }
        }
        for h in land.holdings.iter().flatten() {
            assert!(land.risk[land.index(h.seat)] <= SEAT_RISK);
        }
    }

    #[test]
    fn high_ground_lowers_flood_risk_away_from_the_bank() {
        // Silt to the flood level on the far side of the channel only.
        let (mut field, line, widths, walls) = valley(30.0);
        let cols = field.cols;
        for y in 0..field.rows {
            if (y as f64 + 0.5) * field.cell > 30.0 {
                field.deposit[y * cols..(y + 1) * cols].fill(0.25);
            }
        }
        let mut land = Land::new(&field, 1);
        land.update(&field, &line, &widths, &walls, &settings(0.0));
        let risk = |y: f64| land.risk[land.index([60.0, y])];
        for d in [3.0, 6.0, 10.0] {
            let (low, high) = (risk(30.0 - d), risk(30.0 + d));
            assert!(
                high < (1.0 - 0.9 * RIDGE_RELIEF as f32) * low,
                "{d} widths out: risk {high:.2} on high ground, {low:.2} on low"
            );
        }
        let bank = (risk(29.25), risk(30.75));
        assert!(
            (bank.0 - bank.1).abs() < 0.25 * bank.0,
            "the bank keeps its risk: {bank:?}"
        );
    }

    #[test]
    fn low_ground_far_from_the_water_is_wet() {
        // Silt to the flood level on the far side of the channel only.
        let (mut field, line, widths, walls) = valley(30.0);
        let cols = field.cols;
        for y in 0..field.rows {
            if (y as f64 + 0.5) * field.cell > 30.0 {
                field.deposit[y * cols..(y + 1) * cols].fill(0.25);
            }
        }
        // Ground the channel left long ago, bar a fresh strip.
        field.growth.fill(1e3);
        let mut land = Land::new(&field, 1);
        let fresh = land.index([80.0, 16.0]);
        field.growth[fresh] = 0.0;
        land.update(&field, &line, &widths, &walls, &settings(0.0));
        assert_eq!(land.wet[fresh], 0.0, "on fresh bar");
        let at = |y: f64| {
            let i = land.index([60.0, y]);
            (land.wet[i], land.risk[i])
        };
        assert_eq!(at(29.0).0, 0.0, "near the water");
        assert_eq!(at(16.0).0, 1.0, "far from it, on old low ground");
        assert!(at(16.0).1 >= WET_RISK as f32, "kept to meadow");
        assert_eq!(at(44.0).0, 0.0, "on silt to the flood level");
        assert_eq!(at(2.0).0, 0.0, "on the upland");
    }

    #[test]
    fn holdings_reorganise_one_at_a_time() {
        let (field, line, widths, walls) = valley(30.0);
        let mut land = Land::new(&field, 2);
        land.update(&field, &line, &widths, &walls, &settings(0.0));
        let count = land.count() as f64;
        let (dt, steps) = (0.1, 12000u64);
        let mut busy = 0;
        for k in 1..=steps {
            let before = land.reorganisations;
            land.tick(
                &field,
                &line,
                &widths,
                &walls,
                &settings(k as f64 * dt),
                k % SURVEY_STEPS,
            );
            assert!(
                land.reorganisations - before <= 1,
                "one reorganisation begins in a step at most"
            );
            busy += !land.tasks.is_empty() as u32;
        }
        let expected = count * steps as f64 * dt / 60.0;
        let ratio = land.reorganisations as f64 / expected;
        assert!(
            (0.8..1.25).contains(&ratio),
            "{} reorganisations, {expected:.0} expected",
            land.reorganisations
        );
        assert!(
            (busy as f64) < 0.8 * steps as f64,
            "the queue keeps up: busy {busy} of {steps} steps"
        );
    }

    #[test]
    fn a_holding_takes_the_bar_its_bank_builds() {
        let (mut field, line, widths, walls) = valley(30.0);
        let mut land = Land::new(&field, 3);
        land.update(&field, &line, &widths, &walls, &settings(0.0));
        // The channel moves to y = 34 over the north bank, and the bar it
        // leaves behind grows over.
        let moved: Vec<P> = line.iter().map(|p| [p[0], 34.0]).collect();
        for y in 0..=8 {
            field.sweep(
                &line
                    .iter()
                    .map(|p| [p[0], 30.0 + 0.5 * y as f64])
                    .collect::<Vec<_>>(),
            );
        }
        land.update(&field, &moved, &widths, &walls, &settings(1.0));
        for g in field.growth.iter_mut().filter(|g| **g == 0.0) {
            *g = 1000.0;
        }
        field.sweep(&moved);
        land.update(&field, &moved, &widths, &walls, &settings(2.0));
        for x in 0..land.cols {
            let o = land.owner[land.index([x as f64 * 0.5 + 0.25, 32.25])];
            assert_ne!(o, NONE);
            assert!(
                seat_y(&land, o) < 30.0,
                "the south bank's holdings take the bar"
            );
        }
    }

    #[test]
    fn ground_cut_off_by_water_keeps_its_holding() {
        let (mut field, line, widths, walls) = valley(30.0);
        let mut land = Land::new(&field, 4);
        land.update(&field, &line, &widths, &walls, &settings(0.0));
        let inside = land.index([60.25, 20.25]);
        let before = land.owner[inside];
        // An oxbow rings the cell, out of reach of its farmstead.
        let ring: Vec<P> = (0..400)
            .map(|k| {
                let a = k as f64 * std::f64::consts::TAU / 400.0;
                [60.25 + 3.0 * a.cos(), 20.25 + 3.0 * a.sin()]
            })
            .collect();
        field.stamp_oxbow(&ring, &vec![1.0; ring.len()], 0.5, 0.0);
        let still = Settings {
            reorganise_time: 1e9,
            ..settings(1.0)
        };
        land.update(&field, &line, &widths, &walls, &still);
        assert_eq!(land.owner[inside], before);
    }

    /// Run holdings over a valley for `steps` steps of 0.1, with the
    /// farmsteads on, from settings `s` at time 0 and `later` after.
    fn farmed(seed: u64, steps: u64, s: Settings, later: Settings) -> Land {
        let (field, line, widths, walls) = valley(30.0);
        let mut land = Land::new(&field, seed);
        land.update(&field, &line, &widths, &walls, &s);
        for k in 1..=steps {
            let now = Settings {
                time: k as f64 * 0.1,
                ..later
            };
            land.tick(&field, &line, &widths, &walls, &now, k % SURVEY_STEPS);
        }
        land
    }

    #[test]
    fn farmsteads_keep_to_dry_ground_and_grow_with_their_holdings() {
        let land = farmed(5, 6000, settings(0.0), settings(0.0));
        let standing: Vec<&Building> = land.buildings.iter().filter(|b| b.standing()).collect();
        assert!(
            standing.len() >= 3 * land.count(),
            "{} buildings on {} holdings",
            standing.len(),
            land.count()
        );
        let mut owned = 0;
        for b in &standing {
            let i = land.index(b.pos);
            let limit = if b.kind == Kind::FieldBarn {
                0.8
            } else {
                FLEE_RISK
            };
            assert!(land.open[i], "{:?} stands on open ground", b.kind);
            assert!(
                land.risk[i] <= limit,
                "{:?} at risk {}",
                b.kind,
                land.risk[i]
            );
            owned += (land.owner[i] == b.holding) as usize;
        }
        assert!(
            owned as f64 >= 0.9 * standing.len() as f64,
            "{owned} of {} buildings on their own holding's ground",
            standing.len()
        );
        // The plan's buildings follow the holding's area on safe ground.
        let cells = land.target(&settings(0.0)).0;
        let mut rows: Vec<(f64, usize)> = Vec::new();
        for (h, g) in land.holdings.iter().enumerate() {
            let Some(g) = g else { continue };
            if land.risk[land.index(g.seat)] > SEAT_RISK {
                continue;
            }
            let n = (standing.iter())
                .filter(|b| b.holding == h as u32)
                .filter(|b| !matches!(b.kind, Kind::FieldBarn | Kind::Cottage))
                .count();
            rows.push((g.area as f64 / cells, n));
        }
        let close = (rows.iter())
            .filter(|&&(a, n)| (n as f64 - (1.0 + 4.0 * a).round().clamp(1.0, 7.0)).abs() <= 1.0)
            .count();
        assert!(
            close as f64 >= 0.75 * rows.len() as f64,
            "{close} of {} farmsteads within one building of their target",
            rows.len()
        );
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mean = |r: &[(f64, usize)]| r.iter().map(|x| x.1 as f64).sum::<f64>() / r.len() as f64;
        let (small, large) = rows.split_at(rows.len() / 2);
        eprintln!(
            "{} buildings, {close} of {} farmsteads near target, means {:.2} and {:.2}",
            standing.len(),
            rows.len(),
            mean(small),
            mean(large)
        );
        assert!(
            mean(large) > mean(small),
            "larger holdings have more buildings: {:.2} against {:.2}",
            mean(large),
            mean(small)
        );
    }

    #[test]
    fn heirs_build_hamlets_beside_their_parents() {
        // Holdings founded at 200 square widths divide once they settle to
        // 70, so most of them divide at least once.
        let hamlets = |chance: f64| {
            let (s, later) = (
                Settings {
                    hamlet_chance: chance,
                    ..settings(0.0)
                },
                Settings {
                    hamlet_chance: chance,
                    holding_area: 70.0,
                    ..settings(0.0)
                },
            );
            let land = farmed(6, 3000, s, later);
            let seats: Vec<P> = land.holdings.iter().flatten().map(|g| g.seat).collect();
            let near = (HAMLET_GAP + 1.5).powi(2);
            (seats.iter().enumerate())
                .filter(|&(k, p)| {
                    (seats.iter().enumerate()).any(|(j, q)| {
                        j != k && (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) < near
                    })
                })
                .count()
        };
        let (apart, together) = (hamlets(0.0), hamlets(1.0));
        eprintln!("{together} farmsteads in hamlets, {apart} without the hamlet rule");
        assert!(
            together >= 4 && together > 2 * apart,
            "{together} farmsteads in hamlets, {apart} without the hamlet rule"
        );
    }
}
