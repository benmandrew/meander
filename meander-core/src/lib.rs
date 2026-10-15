//! Meandering river centreline simulation.
//!
//! The river is a polyline flowing from index 0 (upstream) to the last index
//! (downstream). Each step resamples it, migrates every interior point along
//! its normal at a rate given by the Howard–Knutson (1984) upstream-weighted
//! curvature kernel, smooths, and cuts off any loop whose neck has closed.

pub mod field;
pub mod geom;
pub mod ips;
pub mod rng;

use geom::{P, curvature, dist, resample, right_normal};
use field::{Erodibility, Ground};
use rng::Rng;

/// Point spacing as a fraction of the channel width.
pub const SPACING: f64 = 0.5;

#[derive(Clone, Debug)]
pub struct Params {
    /// Channel width `w`, in world units. Sets the point spacing, the cutoff
    /// distance and the rendered width.
    pub width: f64,
    /// Erodibility `E`: migration speed, in channel widths per unit time per
    /// unit of dimensionless rate.
    pub erodibility: f64,
    /// Weight `Ω` on local curvature (negative in Howard–Knutson).
    pub omega: f64,
    /// Weight `Γ` on the upstream-averaged curvature.
    pub gamma: f64,
    /// Upstream decay rate `α`, per channel width.
    pub alpha: f64,
    /// Bend radius, in channel widths, at which migration is fastest; tighter
    /// bends migrate more slowly. 0 disables the saturation.
    pub peak_radius: f64,
    pub dt: f64,
    /// Laplacian smoothing strength per step.
    pub smoothing: f64,
    /// Cutoff distance `d_cut`, in channel widths.
    pub cutoff_dist: f64,
    /// Minimum index separation `K` for a cutoff pair.
    pub cutoff_min_sep: usize,
    /// Per-step displacement limit, as a fraction of the point spacing.
    pub max_step: f64,
    /// Simulation time an oxbow lake takes to fill with no sediment from the
    /// channel. Floods fill lakes near the channel faster.
    pub oxbow_lifetime: f64,
    /// Fraction of a lake a flood of magnitude 1 fills with bedload through
    /// an entrance still at the channel, at a diversion angle of 0. It
    /// falls with the cosine of the angle and with the entrance's distance
    /// from the channel over `levee_length`. 0 disables.
    pub bedload_fill: f64,
    /// Simulation time a filled oxbow stays as a scar before it is dropped.
    pub scar_lifetime: f64,
    /// Chance that the largest flood breaches the levee at a sharp outer
    /// bend and spreads a crevasse splay over the floodplain. It falls to 0
    /// for floods of magnitude [`SPLAY_MIN_FLOOD`]. 0 disables splays.
    pub splay_chance: f64,
    /// Simulation time a splay takes to vegetate over, after which it is
    /// dropped.
    pub splay_lifetime: f64,
    /// Simulation time a scroll bar, the ridge a flood leaves along the
    /// inner bank, lasts before it is dropped. 0 disables scroll bars.
    pub scroll_lifetime: f64,
    /// Random lateral perturbation added each step, in channel widths per
    /// square root of time. Bends drift downstream and leave the domain, so
    /// without this the river eventually straightens.
    pub jitter: f64,
    /// Correlation length of the jitter along the river, in channel widths.
    pub jitter_scale: f64,
    /// Width of the valley the river may migrate within, centred on the
    /// domain, in channel widths. 0 allows the whole domain.
    pub valley_width: f64,
    /// Width of the band along each domain edge over which erodibility
    /// tapers to zero, in channel widths.
    pub edge_margin: f64,
    /// Valley width at each of [`Sim::pinches`], in channel widths. The
    /// river crosses a pinch near the valley axis, so a renderer can put the
    /// window edges there and no bend straddles them. 0 disables.
    pub pinch_width: f64,
    /// Distance either side of a pinch over which the valley widens back to
    /// `valley_width`, in channel widths.
    pub pinch_length: f64,
    /// Strength of the erodibility noise: 0 is uniform, 1 ranges from
    /// near zero to double.
    pub heterogeneity: f64,
    /// Erodibility multiplier inside filled oxbows (clay plugs); 1 disables.
    pub clay_factor: f64,
    /// Erodibility multiplier inside an oxbow lake when it is cut off. The
    /// bank breaks through quickly into open water, and the multiplier moves
    /// to `clay_factor` as the lake fills over `oxbow_lifetime`.
    pub lake_factor: f64,
    /// Erodibility multiplier on ground the channel has just left, as bare
    /// point-bar sand. It moves to 1 as vegetation binds the sand over
    /// `vegetation_time`; 1 disables.
    pub bar_factor: f64,
    /// Simulation time vegetation takes to bind a point bar.
    pub vegetation_time: f64,
    /// Expected floods per unit time. A flood can breach necks wider than
    /// `cutoff_dist` and cut chutes across point bars. 0 disables floods.
    pub flood_rate: f64,
    /// Widest neck, in channel widths, that the largest flood can breach.
    /// A flood of magnitude `u` in [0, 1) breaches necks narrower than
    /// `cutoff_dist + u·(breach_dist − cutoff_dist)`.
    pub breach_dist: f64,
    /// A flood of magnitude `u` cuts a chute across a bend whose arc length
    /// is at least `chute_ratio / u` times the chord, for chords up to
    /// [`CHUTE_CHORD`] widths. 0 disables chute cutoffs.
    pub chute_ratio: f64,
    /// Overbank deposit a flood of magnitude 1 lays at the bank, in channel
    /// depths, before it thins towards the flood level. 0 disables.
    pub overbank: f64,
    /// Distance over which overbank deposits thin by a factor of e away
    /// from the bank, in channel widths.
    pub levee_length: f64,
    /// Flood level above fresh point-bar sand, in channel depths: the
    /// highest ground overbank deposits can build. A bank raised by h
    /// erodes 1 / (1 + h) as fast.
    pub levee_height: f64,
    /// Derive `omega`, `gamma` and `alpha` from the hydraulic inputs below
    /// with the Ikeda–Parker–Sawai model (see [`ips`]).
    pub physics: bool,
    /// Friction coefficient `C_f`.
    pub friction: f64,
    /// Width to depth ratio `w/H`.
    pub aspect: f64,
    /// Froude number `U/√(gH)`.
    pub froude: f64,
    /// Scour factor `A`: transverse bed slope per unit curvature.
    pub scour: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            width: 8.0,
            erodibility: 1.0,
            omega: -1.0,
            gamma: 2.5,
            alpha: 0.6,
            peak_radius: 2.5,
            dt: 0.1,
            smoothing: 0.1,
            cutoff_dist: 1.0,
            cutoff_min_sep: 20,
            max_step: 0.15,
            oxbow_lifetime: OXBOW_LIFETIME,
            bedload_fill: BEDLOAD_FILL,
            scar_lifetime: 1600.0,
            scroll_lifetime: SCROLL_LIFETIME,
            splay_chance: SPLAY_CHANCE,
            splay_lifetime: SPLAY_LIFETIME,
            jitter: 0.05,
            jitter_scale: 3.0,
            valley_width: 45.0,
            edge_margin: 8.0,
            pinch_width: 6.0,
            pinch_length: 20.0,
            heterogeneity: 0.5,
            clay_factor: 0.35,
            lake_factor: 3.0,
            bar_factor: BAR_FACTOR,
            vegetation_time: VEGETATION_TIME,
            flood_rate: FLOOD_RATE,
            breach_dist: BREACH_DIST,
            chute_ratio: CHUTE_RATIO,
            overbank: OVERBANK,
            levee_length: LEVEE_LENGTH,
            levee_height: LEVEE_HEIGHT,
            physics: false,
            friction: 0.01,
            aspect: 30.0,
            froude: 0.3,
            scour: 2.89,
        }
    }
}

impl Params {
    /// Names accepted by [`Params::get`] and [`Params::set`].
    pub const NAMES: &[&str] = &[
        "width",
        "erodibility",
        "omega",
        "gamma",
        "alpha",
        "peak_radius",
        "dt",
        "smoothing",
        "cutoff_dist",
        "cutoff_min_sep",
        "max_step",
        "oxbow_lifetime",
        "bedload_fill",
        "scar_lifetime",
        "scroll_lifetime",
        "splay_chance",
        "splay_lifetime",
        "jitter",
        "jitter_scale",
        "valley_width",
        "edge_margin",
        "pinch_width",
        "pinch_length",
        "heterogeneity",
        "clay_factor",
        "lake_factor",
        "bar_factor",
        "vegetation_time",
        "flood_rate",
        "breach_dist",
        "chute_ratio",
        "overbank",
        "levee_length",
        "levee_height",
        "physics",
        "friction",
        "aspect",
        "froude",
        "scour",
    ];

    fn field(&mut self, name: &str) -> Option<&mut f64> {
        Some(match name {
            "width" => &mut self.width,
            "erodibility" => &mut self.erodibility,
            "omega" => &mut self.omega,
            "gamma" => &mut self.gamma,
            "alpha" => &mut self.alpha,
            "peak_radius" => &mut self.peak_radius,
            "dt" => &mut self.dt,
            "smoothing" => &mut self.smoothing,
            "cutoff_dist" => &mut self.cutoff_dist,
            "max_step" => &mut self.max_step,
            "oxbow_lifetime" => &mut self.oxbow_lifetime,
            "bedload_fill" => &mut self.bedload_fill,
            "scar_lifetime" => &mut self.scar_lifetime,
            "scroll_lifetime" => &mut self.scroll_lifetime,
            "splay_chance" => &mut self.splay_chance,
            "splay_lifetime" => &mut self.splay_lifetime,
            "jitter" => &mut self.jitter,
            "jitter_scale" => &mut self.jitter_scale,
            "valley_width" => &mut self.valley_width,
            "edge_margin" => &mut self.edge_margin,
            "pinch_width" => &mut self.pinch_width,
            "pinch_length" => &mut self.pinch_length,
            "heterogeneity" => &mut self.heterogeneity,
            "clay_factor" => &mut self.clay_factor,
            "lake_factor" => &mut self.lake_factor,
            "bar_factor" => &mut self.bar_factor,
            "vegetation_time" => &mut self.vegetation_time,
            "flood_rate" => &mut self.flood_rate,
            "breach_dist" => &mut self.breach_dist,
            "chute_ratio" => &mut self.chute_ratio,
            "overbank" => &mut self.overbank,
            "levee_length" => &mut self.levee_length,
            "levee_height" => &mut self.levee_height,
            "friction" => &mut self.friction,
            "aspect" => &mut self.aspect,
            "froude" => &mut self.froude,
            "scour" => &mut self.scour,
            _ => return None,
        })
    }

    /// Flags read as 0 or 1.
    pub fn get(&self, name: &str) -> Option<f64> {
        match name {
            "cutoff_min_sep" => Some(self.cutoff_min_sep as f64),
            "physics" => Some(if self.physics { 1.0 } else { 0.0 }),
            _ => self.clone().field(name).map(|v| *v),
        }
    }

    /// Set a parameter by name; a flag is set by any non-zero value. With
    /// `physics` on, `omega`, `gamma` and `alpha` are re-derived after every
    /// set, so setting them directly has no effect. Returns false for an
    /// unknown name.
    pub fn set(&mut self, name: &str, value: f64) -> bool {
        match name {
            "cutoff_min_sep" => self.cutoff_min_sep = value.max(2.0) as usize,
            "physics" => self.physics = value != 0.0,
            _ => match self.field(name) {
                Some(v) => *v = value,
                None => return false,
            },
        }
        if self.physics {
            (self.omega, self.gamma, self.alpha) = ips::coefficients(self.friction, self.aspect, self.froude, self.scour);
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct Oxbow {
    pub points: Vec<P>,
    pub born: f64,
    /// Arc length when the oxbow was cut off. Pieces split from it keep
    /// it, so the drawn lake plugs at a rate that does not jump when the
    /// channel trims the oxbow.
    pub length: f64,
    /// Arc length from the first point of the oxbow as cut off to the
    /// first point of this piece, so only the original ends plug.
    pub offset: f64,
    /// Fraction of the lake filled, in [0, 1].
    pub fill: f64,
    /// Time the lake filled and became a scar, or infinity.
    pub filled_at: f64,
    /// Cosine of the diversion angle between the new channel and the
    /// oxbow's entrance at the cut, or 0 past a right angle. Bedload enters
    /// a lake cut at a small angle (Constantine et al. 2010).
    pub entry: f64,
}

impl Oxbow {
    /// A fresh lake along the points, cut off at `born`.
    pub fn lake(points: Vec<P>, born: f64, entry: f64) -> Self {
        let length = geom::length(&points);
        Self { points, born, length, offset: 0.0, fill: 0.0, filled_at: f64::INFINITY, entry }
    }

    pub fn is_lake(&self) -> bool {
        self.filled_at.is_infinite()
    }
}

/// A scroll bar: the ridge a flood leaves along the inner bank of a
/// migrating bend, as a polyline.
#[derive(Clone, Debug)]
pub struct Scroll {
    pub points: Vec<P>,
    pub born: f64,
}

/// A crevasse splay: a fan of sand a flood spreads through a breach in
/// the levee, from `apex` on the outer bank along the unit vector `dir`.
#[derive(Clone, Debug)]
pub struct Splay {
    pub apex: P,
    pub dir: P,
    pub length: f64,
    pub born: f64,
    /// [`SPLAY_RAYS`] rays fanning across the splay, each a unit direction
    /// and the distances from the apex, inner then outer, between which
    /// its sand remains. The channel erodes the sand from either end.
    pub rays: Vec<(P, [f64; 2])>,
}

impl Splay {
    /// Whether the channel has eroded all of the splay.
    pub fn is_gone(&self) -> bool {
        self.rays.iter().all(|r| r.1[0] >= r.1[1])
    }
}

pub struct Sim {
    pub params: Params,
    /// Domain is `[0, bounds[0]] × [0, bounds[1]]`.
    pub bounds: P,
    /// Positions along x where the valley narrows to `pinch_width`.
    pub pinches: Vec<f64>,
    pub centerline: Vec<P>,
    pub oxbows: Vec<Oxbow>,
    /// Counts changes to the oxbow polylines, but not to their fill, so a
    /// renderer can skip rebuilding what depends only on their shape.
    pub oxbow_version: u64,
    pub scrolls: Vec<Scroll>,
    /// Counts changes to `scrolls`, so a renderer can skip redrawing them.
    pub scroll_version: u64,
    pub splays: Vec<Splay>,
    /// Counts changes to `splays`, so a renderer can skip redrawing them.
    pub splay_version: u64,
    /// Crevasse splays so far.
    pub crevasses: u64,
    pub erodibility: Erodibility,
    pub time: f64,
    pub steps: u64,
    pub cutoffs: u64,
    /// Floods so far, and the cutoffs they made (included in `cutoffs`).
    pub floods: u64,
    pub flood_cutoffs: u64,
    rng: Rng,
    /// Recent cut points and the step count until which they suppress new
    /// cutoffs nearby.
    recent_cuts: Vec<(P, u64)>,
    // Scratch buffers, kept to avoid per-step allocation.
    curv: Vec<f64>,
    rate: Vec<f64>,
    noise: Vec<f64>,
    tmp: Vec<P>,
    grid_start: Vec<u32>,
    grid_items: Vec<u32>,
    grid_fill: Vec<u32>,
    cell_of: Vec<u32>,
    /// Counts changes to the centreline within a step, so the grid built
    /// for one pass can serve the next if the centreline has not moved.
    line_version: u64,
    /// The grid's layout, and the `line_version` it was built for.
    grid_for: Option<(Cells, u64)>,
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Valley width at `x`: `valley` (the whole domain height when 0), narrowing
/// smoothly to `pinch` at each of `pinches` over `length` either side.
fn valley_at(x: f64, height: f64, valley: f64, pinches: &[f64], pinch: f64, length: f64) -> f64 {
    let full = if valley > 0.0 { valley.min(height) } else { height };
    if pinch <= 0.0 || pinch >= full || length <= 0.0 {
        return full;
    }
    let d = pinches.iter().fold(f64::INFINITY, |d, &px| d.min((x - px).abs()));
    pinch + (full - pinch) * smoothstep(d / length)
}

/// Erodibility multiplier that falls smoothly from 1 to 0 within `margin` of
/// the domain edges and of the valley walls, for a valley of width `valley`
/// centred on the domain.
fn edge_taper(p: P, bounds: P, margin: f64, valley: f64) -> f64 {
    if margin <= 0.0 {
        return 1.0;
    }
    let ramp = |d: f64| smoothstep(d / margin);
    let mid = 0.5 * bounds[1];
    let y0 = (mid - 0.5 * valley).max(0.0);
    let y1 = (mid + 0.5 * valley).min(bounds[1]);
    ramp(p[0]) * ramp(bounds[0] - p[0]) * ramp(p[1] - y0) * ramp(y1 - p[1])
}

/// Feature size of the erodibility noise, in channel widths at reset.
const NOISE_SCALE: f64 = 25.0;

/// Cell size of the erodibility grid, in channel widths at reset.
const FIELD_CELL: f64 = 0.5;

/// Distance from the centreline at which a point reads the erodibility of
/// the bank it moves into, in channel widths. It lies past the half-width
/// bank, so the read misses the cells the channel itself has just swept.
const BANK_READ: f64 = 0.75;

/// Default point-bar parameters; see [`Params`].
const BAR_FACTOR: f64 = 1.3;
const VEGETATION_TIME: f64 = 100.0;

/// Steps during which a cut suppresses further cutoffs around it.
const CUT_COOLDOWN: u64 = 100;

/// Default flood parameters; see [`Params`].
const FLOOD_RATE: f64 = 0.02;
const BREACH_DIST: f64 = 2.5;
const CHUTE_RATIO: f64 = 4.0;
const OVERBANK: f64 = 0.3;
const OXBOW_LIFETIME: f64 = 1200.0;
const BEDLOAD_FILL: f64 = 0.2;
const SCROLL_LIFETIME: f64 = 1000.0;

/// Distance of a scroll bar from the centreline, in channel widths: just
/// inside the inner bank.
const SCROLL_OFFSET: f64 = 0.6;

/// Least migration rate, in the dimensionless units of
/// [`Sim::migration_rate`], at which a bank counts as accreting a scroll bar.
const SCROLL_MIN_RATE: f64 = 0.05;

/// Scroll bars and pieces with fewer points than this are dropped.
const MIN_SCROLL_POINTS: usize = 6;

const SPLAY_CHANCE: f64 = 1.0;
const SPLAY_LIFETIME: f64 = 2000.0;

/// Smallest flood magnitude that can breach a levee.
pub const SPLAY_MIN_FLOOD: f64 = 0.6;

/// Random points tried for a breach; the sharpest bend among them wins.
const SPLAY_TRIES: usize = 20;

/// Gentlest bend, as curvature in inverse channel widths, that a flood
/// can breach: a radius of 5 widths.
const SPLAY_MIN_CURVATURE: f64 = 0.2;

/// Length of a splay in channel widths: this much for the weakest flood
/// that breaches, plus [`SPLAY_EXTRA`] times the magnitude.
const SPLAY_BASE: f64 = 3.0;
const SPLAY_EXTRA: f64 = 5.0;

/// Spread of a splay across per unit along it: tan 35°.
pub const SPLAY_SPREAD: f64 = 0.7;

/// Rays across a splay that track how much of it the channel has eroded.
pub const SPLAY_RAYS: usize = 9;

/// Steps between checks for scroll bars under the channel. They do not
/// affect the dynamics, so the check need not run every step.
const SCROLL_REWORK_EVERY: u64 = 10;
const LEVEE_LENGTH: f64 = 2.0;
const LEVEE_HEIGHT: f64 = 0.25;

/// Longest chute a flood can cut, in channel widths.
pub const CHUTE_CHORD: f64 = 5.0;

/// Oxbow pieces with fewer points than this are dropped.
const MIN_OXBOW_POINTS: usize = 3;

/// Distance from the channel centreline, in channel widths, within which
/// the channel removes an oxbow point. The lakes lie under the water, so a
/// point nearer than half a width is already hidden on the channel side; a
/// smaller radius lets a channel that runs alongside a lake cover it a
/// little at a time, not a whole reach at once. It is above half the
/// largest step, `max_step` × `SPACING` = 0.075, so a channel moving across
/// a point comes within it.
const REWORK_RADIUS: f64 = 0.1;

/// Most cutoffs a single flood can make.
const MAX_FLOOD_CUTS: usize = 4;

/// Layout of the hashed grid of centreline points: the cell size, the
/// bucket mask, and the step in buckets from one row of cells to the next.
#[derive(Clone, Copy)]
struct Cells {
    size: f64,
    mask: usize,
    stride: i64,
}

impl Cells {
    fn cell(&self, p: P) -> (i64, i64) {
        ((p[0] / self.size).floor() as i64, (p[1] / self.size).floor() as i64)
    }

    /// Cells along a row hash to consecutive buckets, so the three cells
    /// around a point in a row are one run of the table.
    fn bucket(&self, cx: i64, cy: i64) -> usize {
        cy.wrapping_mul(self.stride).wrapping_add(cx) as usize & self.mask
    }
}

/// Whether segments `ab` and `cd` cross at a point interior to both.
fn segments_cross(a: P, b: P, c: P, d: P) -> bool {
    fn orient(a: P, b: P, c: P) -> f64 {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }
    orient(a, b, c) * orient(a, b, d) < 0.0 && orient(c, d, a) * orient(c, d, b) < 0.0
}

impl Sim {
    pub fn new(params: Params, bounds: P, seed: u64, noise: f64) -> Self {
        let mut sim = Self {
            params,
            bounds,
            pinches: Vec::new(),
            centerline: Vec::new(),
            oxbows: Vec::new(),
            oxbow_version: 0,
            scrolls: Vec::new(),
            scroll_version: 0,
            splays: Vec::new(),
            splay_version: 0,
            crevasses: 0,
            erodibility: Erodibility::new([1.0, 1.0], 1.0, 1.0, 0),
            time: 0.0,
            steps: 0,
            cutoffs: 0,
            floods: 0,
            flood_cutoffs: 0,
            rng: Rng::new(seed),
            recent_cuts: Vec::new(),
            curv: Vec::new(),
            rate: Vec::new(),
            noise: Vec::new(),
            tmp: Vec::new(),
            grid_start: Vec::new(),
            grid_items: Vec::new(),
            grid_fill: Vec::new(),
            cell_of: Vec::new(),
            line_version: 0,
            grid_for: None,
        };
        sim.reset(seed, noise);
        sim
    }

    /// Restart from a straight line across the domain with lateral noise of
    /// amplitude `noise` channel widths.
    pub fn reset(&mut self, seed: u64, noise: f64) {
        self.rng = Rng::new(seed);
        let [w, h] = self.bounds;
        let line = vec![[0.0, h / 2.0], [w, h / 2.0]];
        let mut pts = resample(&line, self.ds());
        let amp = noise * self.params.width;
        let n = pts.len();
        // White noise, then a few smoothing passes so the perturbation has
        // energy at meander scales and not only at the point spacing.
        let mut off: Vec<f64> = (0..n).map(|_| self.rng.normal() * amp).collect();
        for _ in 0..8 {
            let prev = off.clone();
            for i in 1..n - 1 {
                off[i] = 0.25 * prev[i - 1] + 0.5 * prev[i] + 0.25 * prev[i + 1];
            }
        }
        off[0] = 0.0;
        off[n - 1] = 0.0;
        for (p, o) in pts.iter_mut().zip(&off) {
            p[1] += o;
        }
        self.centerline = pts;
        let w = self.params.width;
        self.erodibility = Erodibility::new(self.bounds, FIELD_CELL * w, NOISE_SCALE * w, seed);
        self.oxbows.clear();
        self.oxbow_version += 1;
        self.line_version += 1;
        self.scrolls.clear();
        self.scroll_version += 1;
        self.splays.clear();
        self.splay_version += 1;
        self.crevasses = 0;
        self.recent_cuts.clear();
        self.time = 0.0;
        self.steps = 0;
        self.cutoffs = 0;
        self.floods = 0;
        self.flood_cutoffs = 0;
    }

    /// Target point spacing Δs, in world units.
    pub fn ds(&self) -> f64 {
        SPACING * self.params.width
    }

    /// Arc length over straight-line distance between the endpoints.
    pub fn sinuosity(&self) -> f64 {
        let pts = &self.centerline;
        let chord = dist(pts[0], pts[pts.len() - 1]);
        if chord > 0.0 { geom::length(pts) / chord } else { 1.0 }
    }

    pub fn step_n(&mut self, n: usize) {
        for _ in 0..n {
            self.step();
        }
    }

    pub fn step(&mut self) {
        let ds = self.ds();
        self.resample(ds);
        self.migrate();
        self.smooth();
        self.clamp_to_domain();
        self.line_version += 1;
        if self.cutoff() {
            self.resample(ds);
        }
        // Floods arrive as a Poisson process, approximated per step.
        if self.params.flood_rate > 0.0 && self.rng.uniform() < self.params.flood_rate * self.params.dt {
            self.floods += 1;
            let magnitude = self.rng.uniform();
            for _ in 0..MAX_FLOOD_CUTS {
                if !self.flood_cutoff(magnitude) {
                    break;
                }
                self.resample(ds);
            }
            let p = &self.params;
            self.erodibility.flood(&self.centerline, 0.5 * p.width, magnitude * p.overbank, p.levee_length * p.width, p.levee_height);
            self.fill_lakes(magnitude);
            self.add_scrolls();
            self.crevasse(magnitude);
        }
        self.erodibility.sweep(&self.centerline, self.time);
        self.time += self.params.dt;
        self.steps += 1;
        self.rework_oxbows();
        let (time, rate) = (self.time, self.params.dt / self.params.oxbow_lifetime.max(1e-9));
        for o in self.oxbows.iter_mut().filter(|o| o.is_lake()) {
            o.fill = (o.fill + rate).min(1.0);
            if o.fill >= 1.0 {
                o.filled_at = time;
            }
        }
        let (scar, before) = (self.params.scar_lifetime, self.oxbows.len());
        self.oxbows.retain(|o| time - o.filled_at < scar);
        if self.oxbows.len() != before {
            self.oxbow_version += 1;
        }
        if self.steps % SCROLL_REWORK_EVERY == 0 {
            self.rework_scrolls();
            self.erode_splays();
        }
        let steps = self.steps;
        self.recent_cuts.retain(|&(_, until)| until > steps);
    }

    /// Resample the centreline to spacing close to `ds`.
    fn resample(&mut self, ds: f64) {
        geom::resample_into(&self.centerline, ds, &mut self.tmp);
        std::mem::swap(&mut self.centerline, &mut self.tmp);
        self.line_version += 1;
    }

    /// Howard–Knutson migration rate at each point, in dimensionless units
    /// (curvature measured in inverse channel widths).
    pub fn migration_rate(&mut self) -> &[f64] {
        let p = &self.params;
        let pts = &self.centerline;
        let n = pts.len();
        curvature(pts, &mut self.curv);
        // Hickin–Nanson saturation: migration grows with w/r for gentle
        // bends, peaks at r = peak_radius·w, and falls for tighter bends.
        // c / (1 + (c/c0)²) peaks at c0 = 1/peak_radius.
        let c0 = if p.peak_radius > 0.0 { 1.0 / p.peak_radius } else { f64::INFINITY };
        for c in &mut self.curv {
            let cw = *c * p.width;
            *c = cw / (1.0 + (cw / c0).powi(2));
        }
        // Actual spacing after resampling, in channel widths.
        let ds_w = if n > 1 {
            geom::length(pts) / (n - 1) as f64 / p.width
        } else {
            SPACING
        };
        let decay = (-p.alpha * ds_w).exp();
        self.rate.clear();
        self.rate.resize(n, 0.0);
        let (mut acc, mut norm) = (0.0, 0.0);
        for i in 0..n {
            if i > 0 {
                acc = decay * (acc + self.curv[i - 1] * ds_w);
                norm = decay * (norm + ds_w);
            }
            let upstream = if norm > 0.0 { acc / norm } else { 0.0 };
            self.rate[i] = p.omega * self.curv[i] + p.gamma * upstream;
        }
        &self.rate
    }

    fn migrate(&mut self) {
        self.migration_rate();
        let p = &self.params;
        let pts = &self.centerline;
        let n = pts.len();
        let scale = p.dt * p.erodibility * p.width;
        let limit = p.max_step * SPACING * p.width;
        let ground = Ground {
            time: self.time,
            heterogeneity: p.heterogeneity,
            lake_life: p.oxbow_lifetime,
            lake: p.lake_factor,
            clay: p.clay_factor,
            bar: p.bar_factor,
            vegetation: p.vegetation_time,
        };
        let read = BANK_READ * p.width;

        // Noise smoothed along the river by a forward and a backward
        // exponential filter, with correlation length `jitter_scale`, then
        // rescaled to the target RMS. Short-correlated noise only adds kinks.
        let amp = p.jitter * p.width * p.dt.sqrt();
        self.noise.clear();
        self.noise.extend((0..n).map(|_| self.rng.approx_normal()));
        let a = (-SPACING / p.jitter_scale.max(SPACING)).exp();
        for i in 1..n {
            self.noise[i] = a * self.noise[i - 1] + (1.0 - a) * self.noise[i];
        }
        for i in (0..n.saturating_sub(1)).rev() {
            self.noise[i] = a * self.noise[i + 1] + (1.0 - a) * self.noise[i];
        }
        let rms = (self.noise.iter().map(|x| x * x).sum::<f64>() / n.max(1) as f64).sqrt();
        if rms > 0.0 {
            for x in &mut self.noise {
                *x *= amp / rms;
            }
        }

        self.tmp.clear();
        self.tmp.extend_from_slice(pts);
        for i in 1..n.saturating_sub(1) {
            let nrm = right_normal(pts, i);
            let valley = valley_at(
                pts[i][0],
                self.bounds[1],
                p.valley_width * p.width,
                &self.pinches,
                p.pinch_width * p.width,
                p.pinch_length * p.width,
            );
            // Positive rate moves toward the outside of a left bend, i.e. to
            // the right, so bends grow.
            // The bank is on the side the rate pushes towards. Choosing it
            // by the jitter as well would damp outward and amplify inward
            // noise wherever the banks differ, a net drift inwards.
            let side = if self.rate[i] < 0.0 { -read } else { read };
            let bank = [pts[i][0] + side * nrm[0], pts[i][1] + side * nrm[1]];
            let e = edge_taper(pts[i], self.bounds, p.edge_margin * p.width, valley) * self.erodibility.at(bank, &ground);
            let d = (e * (scale * self.rate[i] + self.noise[i])).clamp(-limit, limit);
            self.tmp[i][0] += d * nrm[0];
            self.tmp[i][1] += d * nrm[1];
        }
        std::mem::swap(&mut self.centerline, &mut self.tmp);
    }

    fn smooth(&mut self) {
        let k = self.params.smoothing;
        if k <= 0.0 {
            return;
        }
        let n = self.centerline.len();
        self.tmp.clear();
        self.tmp.extend_from_slice(&self.centerline);
        let src = &self.tmp;
        for i in 1..n.saturating_sub(1) {
            for d in 0..2 {
                self.centerline[i][d] += k * (src[i - 1][d] + src[i + 1][d] - 2.0 * src[i][d]);
            }
        }
    }

    fn clamp_to_domain(&mut self) {
        let [w, h] = self.bounds;
        for p in &mut self.centerline {
            p[0] = p[0].clamp(0.0, w);
            p[1] = p[1].clamp(0.0, h);
        }
    }

    /// Fill the hashed grid with the centreline points, for cells of size
    /// `size`, unless it already holds them. The table has about 4n buckets
    /// and is filled by counting sort, so each bucket lists its points in
    /// increasing order; collisions only add candidates, which callers
    /// reject by distance.
    fn build_grid(&mut self, size: f64) -> Cells {
        if let Some((g, v)) = self.grid_for
            && g.size == size
            && v == self.line_version
        {
            return g;
        }
        let pts = &self.centerline;
        let n = pts.len();
        let buckets = (4 * n).next_power_of_two();
        let mask = buckets - 1;
        let g = Cells { size, mask, stride: (self.bounds[0] / size).ceil() as i64 + 3 };
        self.grid_start.clear();
        self.grid_start.resize(buckets + 1, 0);
        self.cell_of.clear();
        for &p in pts {
            let (cx, cy) = g.cell(p);
            let b = g.bucket(cx, cy);
            self.cell_of.push(b as u32);
            self.grid_start[b + 1] += 1;
        }
        for b in 0..buckets {
            self.grid_start[b + 1] += self.grid_start[b];
        }
        self.grid_items.clear();
        self.grid_items.resize(n, 0);
        self.grid_fill.clear();
        self.grid_fill.extend_from_slice(&self.grid_start[..buckets]);
        for (i, &b) in self.cell_of.iter().enumerate() {
            let slot = &mut self.grid_fill[b as usize];
            self.grid_items[*slot as usize] = i as u32;
            *slot += 1;
        }
        self.grid_for = Some((g, self.line_version));
        g
    }

    /// Point indices in cells `cx − 1` to `cx + 1` of row `cy`, as one run
    /// of buckets, split in two where it wraps round the table.
    fn row(&self, g: Cells, cx: i64, cy: i64) -> [&[u32]; 2] {
        let (b, buckets) = (g.bucket(cx - 1, cy), g.mask + 1);
        let items = |a: usize, e: usize| &self.grid_items[self.grid_start[a] as usize..self.grid_start[e] as usize];
        if b + 3 <= buckets { [items(b, b + 3), &[]] } else { [items(b, buckets), items(0, b + 3 - buckets)] }
    }

    /// Call `f` with every point index above `after` in the 3 × 3 block of
    /// grid cells around `p`, which covers all points within the cell size
    /// of it, row by row and cell by cell, in increasing order in each.
    fn for_neighbours_after(&self, p: P, g: Cells, after: usize, mut f: impl FnMut(usize)) {
        let (cx, cy) = g.cell(p);
        for y in cy - 1..=cy + 1 {
            for part in self.row(g, cx, y) {
                for &j in part {
                    if j as usize > after {
                        f(j as usize);
                    }
                }
            }
        }
    }

    /// Whether any centreline point lies within `r`, at most the cell size,
    /// of `p`.
    fn near_channel(&self, p: P, r: f64, g: Cells) -> bool {
        let (cx, cy) = g.cell(p);
        let pts = &self.centerline;
        (cy - 1..=cy + 1).any(|y| {
            self.row(g, cx, y).iter().any(|part| part.iter().any(|&j| geom::dist2(p, pts[j as usize]) < r * r))
        })
    }

    /// Call `f` with each centreline segment that has an end among the
    /// points in the 3 × 3 block of grid cells around `p`, until it returns
    /// true; returns whether it did. A segment may be passed twice.
    fn any_segment_near(&self, p: P, g: Cells, mut f: impl FnMut(P, P) -> bool) -> bool {
        let (cx, cy) = g.cell(p);
        let pts = &self.centerline;
        let n = pts.len();
        (cy - 1..=cy + 1).any(|y| {
            self.row(g, cx, y).iter().any(|part| {
                part.iter().any(|&j| {
                    let j = j as usize;
                    (j > 0 && f(pts[j - 1], pts[j])) || (j + 1 < n && f(pts[j], pts[j + 1]))
                })
            })
        })
    }

    /// Whether a recent cut suppresses a cutoff at `p`.
    fn near_recent(&self, p: P) -> bool {
        let r = 0.5 * self.params.cutoff_min_sep as f64 * self.ds();
        self.recent_cuts.iter().any(|&(c, _)| dist(c, p) < r)
    }

    /// Find one neck cutoff and apply it. Returns whether a cut was made.
    fn cutoff(&mut self) -> bool {
        let d_cut = self.params.cutoff_dist * self.params.width;
        let k_sep = self.params.cutoff_min_sep;
        let n = self.centerline.len();
        if n < k_sep + 2 || d_cut <= 0.0 {
            return false;
        }
        let g = self.build_grid(d_cut);
        let d_cut2 = d_cut * d_cut;
        let pts = &self.centerline;

        // Smallest upstream index first, then the smallest loop from it.
        for i in 0..n {
            let mut best: Option<usize> = None;
            self.for_neighbours_after(pts[i], g, i + k_sep, |j| {
                if best.is_none_or(|b| j < b) && geom::dist2(pts[i], pts[j]) < d_cut2 {
                    best = Some(j);
                }
            });
            if let Some(j) = best
                && !self.near_recent(pts[i])
                && !self.near_recent(pts[j])
            {
                self.apply_cut(i, j);
                return true;
            }
        }
        false
    }

    /// One cut during a flood of magnitude `u` in [0, 1): a neck breach or a
    /// chute across a point bar. Among the pairs that qualify, the one with
    /// the largest ratio of arc length to chord is cut, provided the chord
    /// does not cross the channel. Returns whether a cut was made.
    fn flood_cutoff(&mut self, u: f64) -> bool {
        let p = &self.params;
        let w = p.width;
        let neck = (p.cutoff_dist + u * (p.breach_dist - p.cutoff_dist).max(0.0)) * w;
        let min_ratio = if p.chute_ratio > 0.0 && u > 0.0 { p.chute_ratio / u } else { f64::INFINITY };
        let reach = if min_ratio.is_finite() { neck.max(CHUTE_CHORD * w) } else { neck };
        let k_sep = p.cutoff_min_sep;
        let n = self.centerline.len();
        if n < k_sep + 2 || reach <= 0.0 {
            return false;
        }
        let ds = geom::length(&self.centerline) / (n - 1) as f64;
        let g = self.build_grid(reach);
        let (neck2, reach2) = (neck * neck, reach * reach);
        let pts = &self.centerline;

        // Best pair from each upstream point, as (ratio, i, j).
        let mut found: Vec<(f64, usize, usize)> = Vec::new();
        for i in 0..n {
            let mut best: Option<(f64, usize)> = None;
            self.for_neighbours_after(pts[i], g, i + k_sep, |j| {
                let d2 = geom::dist2(pts[i], pts[j]);
                if d2 >= reach2 {
                    return;
                }
                let ratio = (j - i) as f64 * ds / d2.sqrt().max(1e-9);
                if (d2 < neck2 || ratio >= min_ratio) && best.is_none_or(|(r, _)| ratio > r) {
                    best = Some((ratio, j));
                }
            });
            if let Some((ratio, j)) = best {
                found.push((ratio, i, j));
            }
        }
        found.sort_by(|a, b| b.0.total_cmp(&a.0));

        for &(_, i, j) in &found {
            let (a, b) = (pts[i], pts[j]);
            if self.near_recent(a) || self.near_recent(b) {
                continue;
            }
            let crosses = (0..n - 1)
                .filter(|&k| k + 1 < i || k > j || (k > i && k + 1 < j))
                .any(|k| segments_cross(a, b, pts[k], pts[k + 1]));
            if !crosses {
                self.apply_cut(i, j);
                self.flood_cutoffs += 1;
                return true;
            }
        }
        false
    }

    /// Remove the parts of oxbows the channel has reached, splitting an
    /// oxbow where the channel crosses it, and drop pieces too short to
    /// draw. The channel cuts an oxbow where its centreline crosses one of
    /// the oxbow's segments, or comes within `REWORK_RADIUS` of one of its
    /// points. A point it comes that close to ends the pieces on both sides
    /// of it, and the two points of a crossed segment each end the piece on
    /// their side, so the lake runs in beneath the channel and the cut end
    /// stays hidden as the channel advances. A piece keeps its parent's
    /// birth time and length.
    fn rework_oxbows(&mut self) {
        if self.oxbows.is_empty() {
            return;
        }
        let w = self.params.width;
        let r2 = (REWORK_RADIUS * w).powi(2);
        let g = self.build_grid(w);
        let oxbows = std::mem::take(&mut self.oxbows);
        let mut kept = Vec::with_capacity(oxbows.len());
        // Pieces of the current oxbow, as ranges of point indices.
        let mut pieces: Vec<(usize, usize)> = Vec::new();
        let (mut near, mut hit) = (Vec::new(), Vec::new());
        let mut changed = false;
        for o in oxbows {
            let pts = &o.points;
            let n = pts.len();
            // Points spaced half a width apart, so a segment within reach of
            // a point, or crossing a segment, has an end within a width.
            near.clear();
            near.extend(pts.iter().map(|&q| self.near_channel(q, w, g)));
            if !near.contains(&true) {
                kept.push(o);
                continue;
            }
            hit.clear();
            hit.extend(pts.iter().zip(&near).map(|(&q, &close)| {
                close && self.any_segment_near(q, g, |a, b| geom::seg_dist2(q, a, b) < r2)
            }));
            // A channel point on the segment, as well as a crossing, cuts it.
            let crossed = |k: usize| {
                let (p, q) = (pts[k], pts[k + 1]);
                self.any_segment_near(geom::lerp(p, q, 0.5), g, |a, b| {
                    segments_cross(p, q, a, b) || geom::seg_dist2(a, p, q) < r2 || geom::seg_dist2(b, p, q) < r2
                })
            };
            pieces.clear();
            let mut start = 0;
            for k in 0..n - 1 {
                // A hit point already ends the pieces either side of it.
                if (near[k] || near[k + 1]) && !hit[k] && !hit[k + 1] && crossed(k) {
                    pieces.push((start, k + 1));
                    start = k + 1;
                }
                if hit[k + 1] {
                    pieces.push((start, k + 2));
                    start = k + 1;
                }
            }
            pieces.push((start, n));
            // A hit only at an end, which lies under the channel at the
            // cut, keeps the whole oxbow as it was.
            pieces.retain(|&(a, b)| b - a >= MIN_OXBOW_POINTS);
            if pieces[..] == [(0, n)] {
                kept.push(o);
                continue;
            }
            changed = true;
            for &(a, b) in &pieces {
                let offset = o.offset + geom::length(&pts[..=a]);
                kept.push(Oxbow { points: pts[a..b].to_vec(), offset, ..o });
            }
        }
        self.oxbows = kept;
        if changed {
            self.oxbow_version += 1;
        }
    }

    /// Fill each lake by one flood of magnitude `u`: bedload through an
    /// entrance near the channel, by [`Params::bedload_fill`], and the silt
    /// the flood laid on its bed, whichever fills it further.
    fn fill_lakes(&mut self, u: f64) {
        let p = &self.params;
        let (half, length) = (0.5 * p.width, p.levee_length * p.width);
        for o in self.oxbows.iter_mut().filter(|o| o.is_lake()) {
            // Only the piece holding the original upstream end has the entrance.
            let mut inflow = 0.0;
            if o.offset == 0.0 && o.entry > 0.0 && length > 0.0 {
                let gap = self.centerline.iter().map(|&c| geom::dist2(o.points[0], c)).fold(f64::INFINITY, f64::min);
                inflow = u * p.bedload_fill * o.entry * (-(gap.sqrt() - half).max(0.0) / length).exp();
            }
            let silt = self.erodibility.lake_silt(&o.points);
            o.fill = (o.fill + inflow).max(silt).min(1.0);
            if o.fill >= 1.0 {
                o.filled_at = self.time;
            }
        }
    }

    /// Leave a scroll bar along the inner bank of each run of points whose
    /// bank is migrating, as a flood does on the point bar it builds.
    fn add_scrolls(&mut self) {
        if self.params.scroll_lifetime <= 0.0 {
            return;
        }
        self.migration_rate();
        let off = SCROLL_OFFSET * self.params.width;
        let pts = &self.centerline;
        let n = pts.len();
        let mut run: Vec<P> = Vec::new();
        let mut side = 0.0;
        let mut added = Vec::new();
        for i in 1..n {
            let r = if i < n - 1 { self.rate[i] } else { 0.0 };
            let s = if r > SCROLL_MIN_RATE {
                1.0
            } else if r < -SCROLL_MIN_RATE {
                -1.0
            } else {
                0.0
            };
            if s != side {
                if run.len() >= MIN_SCROLL_POINTS {
                    added.push(Scroll { points: std::mem::take(&mut run), born: self.time });
                }
                run.clear();
                side = s;
            }
            if s != 0.0 {
                // A positive rate moves the point along its right normal,
                // so the inner bank lies along the left.
                let nrm = right_normal(pts, i);
                run.push([pts[i][0] - s * off * nrm[0], pts[i][1] - s * off * nrm[1]]);
            }
        }
        if !added.is_empty() {
            self.scrolls.extend(added);
            self.scroll_version += 1;
        }
    }

    /// During a flood of magnitude `u`, perhaps breach the levee at the
    /// sharpest of a few random bends and spread a splay of sand from the
    /// outer bank, as deep as the flood level at its apex. Returns whether
    /// a splay formed.
    fn crevasse(&mut self, u: f64) -> bool {
        let p = &self.params;
        if p.splay_chance <= 0.0 || u <= SPLAY_MIN_FLOOD {
            return false;
        }
        let chance = p.splay_chance * (u - SPLAY_MIN_FLOOD) / (1.0 - SPLAY_MIN_FLOOD);
        let n = self.centerline.len();
        if n < 3 || self.rng.uniform() >= chance {
            return false;
        }
        curvature(&self.centerline, &mut self.curv);
        let w = self.params.width;
        let mut best = (0, 0.0f64);
        for _ in 0..SPLAY_TRIES {
            let i = (1 + (self.rng.uniform() * (n - 2) as f64) as usize).min(n - 2);
            let c = self.curv[i] * w;
            if c.abs() > best.1.abs() {
                best = (i, c);
            }
        }
        let (i, c) = best;
        if c.abs() < SPLAY_MIN_CURVATURE {
            return false;
        }
        // A left bend (positive curvature) has its outer bank on the right.
        let nrm = right_normal(&self.centerline, i);
        let s = c.signum();
        let dir = [s * nrm[0], s * nrm[1]];
        let q = self.centerline[i];
        let apex = [q[0] + 0.5 * w * dir[0], q[1] + 0.5 * w * dir[1]];
        let length = (SPLAY_BASE + SPLAY_EXTRA * u) * w;
        let thickness = u * self.params.levee_height;
        self.erodibility.stamp_splay(apex, dir, length, SPLAY_SPREAD, thickness, self.time);
        // The rim is rounded, shorter at the flanks, and a little uneven.
        let rays = (0..SPLAY_RAYS)
            .map(|k| {
                let f = 2.0 * k as f64 / (SPLAY_RAYS - 1) as f64 - 1.0;
                let v = [dir[0] - f * SPLAY_SPREAD * dir[1], dir[1] + f * SPLAY_SPREAD * dir[0]];
                let m = (v[0] * v[0] + v[1] * v[1]).sqrt();
                let rim = length * (1.0 - 0.25 * f * f) * (0.8 + 0.2 * self.rng.uniform());
                ([v[0] / m, v[1] / m], [0.0, rim])
            })
            .collect();
        self.splays.push(Splay { apex, dir, length, born: self.time, rays });
        self.splay_version += 1;
        self.crevasses += 1;
        true
    }

    /// Trim each splay's rays back from either end past the ground the
    /// channel has crossed since the splay formed, and drop splays that are
    /// vegetated over or wholly eroded.
    fn erode_splays(&mut self) {
        let (time, life) = (self.time, self.params.splay_lifetime);
        let step = self.erodibility.cell;
        let field = &self.erodibility;
        let mut changed = false;
        for s in &mut self.splays {
            for (v, ends) in &mut s.rays {
                let at = |r: f64| [s.apex[0] + r * v[0], s.apex[1] + r * v[1]];
                let [mut a, mut b] = *ends;
                while a < b && field.swept_since(at(a), s.born) {
                    a += step;
                }
                while a < b && field.swept_since(at(b), s.born) {
                    b -= step;
                }
                if [a, b] != *ends {
                    *ends = [a, b];
                    changed = true;
                }
            }
        }
        let before = self.splays.len();
        self.splays.retain(|s| time - s.born < life && !s.is_gone());
        if changed || self.splays.len() != before {
            self.splay_version += 1;
        }
    }

    /// Drop scroll bars past their lifetime and the parts of the rest
    /// within half a width of the channel, which has eroded them.
    fn rework_scrolls(&mut self) {
        let (time, life) = (self.time, self.params.scroll_lifetime);
        let before = self.scrolls.len();
        self.scrolls.retain(|s| time - s.born < life);
        let mut changed = self.scrolls.len() != before;
        if !self.scrolls.is_empty() {
            let w = self.params.width;
            let g = self.build_grid(w);
            let scrolls = std::mem::take(&mut self.scrolls);
            let mut kept = Vec::with_capacity(scrolls.len());
            for sc in scrolls {
                let wet = |q: P| self.near_channel(q, 0.5 * w, g);
                if !sc.points.iter().any(|&q| wet(q)) {
                    kept.push(sc);
                    continue;
                }
                changed = true;
                for piece in sc.points.split(|&q| wet(q)) {
                    if piece.len() >= MIN_SCROLL_POINTS {
                        kept.push(Scroll { points: piece.to_vec(), born: sc.born });
                    }
                }
            }
            self.scrolls = kept;
        }
        if changed {
            self.scroll_version += 1;
        }
    }

    /// Join point `i` to point `j` and abandon the reach between them as an
    /// oxbow.
    fn apply_cut(&mut self, i: usize, j: usize) {
        let loop_pts = self.centerline[i..=j].to_vec();
        // Diversion angle at the upstream end, between the new channel,
        // straight across to `j`, and the oxbow's first segment.
        let (a, b, c) = (loop_pts[0], loop_pts[1], self.centerline[j]);
        let (main, branch) = ([c[0] - a[0], c[1] - a[1]], [b[0] - a[0], b[1] - a[1]]);
        let norms = (geom::dist2(a, c) * geom::dist2(a, b)).sqrt();
        let entry = if norms > 0.0 { ((main[0] * branch[0] + main[1] * branch[1]) / norms).max(0.0) } else { 0.0 };
        let mid = geom::lerp(self.centerline[i], self.centerline[j], 0.5);
        self.centerline.drain(i + 1..j);
        self.line_version += 1;
        self.oxbow_version += 1;
        self.erodibility.stamp_oxbow(&loop_pts, 0.5 * self.params.width, self.time);
        self.oxbows.push(Oxbow::lake(loop_pts, self.time, entry));
        self.recent_cuts.push((mid, self.steps + CUT_COOLDOWN));
        self.cutoffs += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Straight channel along x with a sine perturbation, cutoffs off.
    fn sine_sim(amp_w: f64, wavelength_w: f64) -> Sim {
        let params =
            Params { cutoff_dist: 0.0, flood_rate: 0.0, jitter: 0.0, heterogeneity: 0.0, ..Params::default() };
        let w = params.width;
        let mut sim = Sim::new(params, [400.0 * w, 100.0 * w], 1, 0.0);
        let k = std::f64::consts::TAU / (wavelength_w * w);
        for p in &mut sim.centerline {
            p[1] += amp_w * w * (k * p[0]).sin();
        }
        sim
    }

    /// Amplitude and phase of the fundamental over the middle of the domain,
    /// from a projection onto sin and cos at the given wavelength.
    fn fundamental(sim: &Sim, wavelength_w: f64) -> (f64, f64) {
        let w = sim.params.width;
        let k = std::f64::consts::TAU / (wavelength_w * w);
        let (x0, x1) = (0.25 * sim.bounds[0], 0.75 * sim.bounds[0]);
        let mid = sim.bounds[1] / 2.0;
        let (mut s, mut c) = (0.0, 0.0);
        for win in sim.centerline.windows(2) {
            let x = win[0][0];
            if x < x0 || x > x1 {
                continue;
            }
            let dx = win[1][0] - x;
            s += (win[0][1] - mid) * (k * x).sin() * dx;
            c += (win[0][1] - mid) * (k * x).cos() * dx;
        }
        // y ≈ A sin(k(x - shift)) = A cos(k shift) sin(kx) - A sin(k shift) cos(kx).
        let amp = s.hypot(c) * 2.0 / (x1 - x0);
        let shift = (-c).atan2(s) / k;
        (amp, shift)
    }

    #[test]
    fn sine_perturbation_grows_and_drifts_downstream() {
        let wl = 15.0;
        let mut sim = sine_sim(0.2, wl);
        let (a0, _) = fundamental(&sim, wl);
        // Sum small phase increments so the shift is not wrapped to ±λ/2.
        let mut drift = 0.0;
        let (mut prev, half) = (fundamental(&sim, wl).1, 0.5 * wl * sim.params.width);
        for _ in 0..40 {
            sim.step_n(5);
            let s = fundamental(&sim, wl).1;
            drift += (s - prev + half).rem_euclid(2.0 * half) - half;
            prev = s;
        }
        let (a1, _) = fundamental(&sim, wl);
        eprintln!("amplitude {a0} -> {a1}, drift {drift}");
        assert!(a1 > 1.2 * a0, "amplitude {a0} -> {a1} should grow");
        assert!(drift > 0.5 * sim.params.width, "drift {drift} should be downstream (+x)");
    }

    #[test]
    fn local_only_does_not_drift() {
        let wl = 15.0;
        let mut sim = sine_sim(0.2, wl);
        sim.params.omega = 1.0;
        sim.params.gamma = 0.0;
        let (a0, s0) = fundamental(&sim, wl);
        sim.step_n(200);
        let (a1, s1) = fundamental(&sim, wl);
        assert!(a1 > a0);
        assert!((s1 - s0).abs() < 0.1 * wl * sim.params.width, "shift {s0} -> {s1}, amplitude {a0} -> {a1}");
    }

    #[test]
    fn noise_meanders_cuts_off_and_saturates() {
        let mut sim = Sim::new(Params::default(), [800.0, 480.0], 7, 0.5);
        let mut max_sin: f64 = 1.0;
        for _ in 0..200 {
            sim.step_n(100);
            max_sin = max_sin.max(sim.sinuosity());
            assert!(!geom::self_intersects(&sim.centerline), "self-intersection at step {}", sim.steps);
        }
        assert!(sim.cutoffs >= 3, "only {} cutoffs", sim.cutoffs);
        assert!(max_sin < 6.0, "sinuosity reached {max_sin}");
    }

    /// Mean sinuosity over the second half of a long run, and the mean arc
    /// length of the loops cut off over the whole run, in channel widths.
    fn late_sinuosity(params: Params, seed: u64) -> (f64, f64, Sim) {
        let mut sim = Sim::new(params, [1200.0, 480.0], seed, 0.5);
        let (mut sum, mut cut) = (0.0, Vec::new());
        for k in 0..300 {
            let before = sim.time;
            sim.step_n(100);
            if k >= 150 {
                sum += sim.sinuosity();
            }
            // Pieces of one oxbow share its birth time and length.
            for o in sim.oxbows.iter().filter(|o| o.born >= before) {
                if !cut.contains(&(o.born, o.length)) {
                    cut.push((o.born, o.length));
                }
            }
            assert!(!geom::self_intersects(&sim.centerline), "self-intersection at step {}", sim.steps);
        }
        let loop_w = cut.iter().map(|c| c.1).sum::<f64>() / cut.len().max(1) as f64 / sim.params.width;
        (sum / 150.0, loop_w, sim)
    }

    #[test]
    fn floods_cut_off_earlier_and_never_self_intersect() {
        // Loose point bars and overbank deposits change the planform, and
        // deposits come only with floods, so both are off here to isolate
        // the cutoffs. Sinuosity is reported but not compared: with bank
        // reads by the rate's sign, floods no longer lower it on every seed.
        let isolated = Params { bar_factor: 1.0, overbank: 0.0, ..Params::default() };
        let calm = Params { flood_rate: 0.0, ..isolated.clone() };
        let stormy = Params { flood_rate: 0.1, chute_ratio: 3.0, ..isolated };
        for seed in [3, 11] {
            let (s_calm, l_calm, calm_sim) = late_sinuosity(calm.clone(), seed);
            let (s_storm, l_storm, sim) = late_sinuosity(stormy.clone(), seed);
            eprintln!(
                "seed {seed}: sinuosity {s_calm:.2} calm, {s_storm:.2} with {} floods and {} flood cutoffs of {}; \
                 loops cut at {l_calm:.1} widths calm, {l_storm:.1} with floods",
                sim.floods, sim.flood_cutoffs, sim.cutoffs
            );
            assert_eq!(calm_sim.flood_cutoffs, 0);
            assert!(sim.flood_cutoffs >= 3, "only {} flood cutoffs", sim.flood_cutoffs);
            assert!(l_storm < 0.85 * l_calm, "floods should cut loops off shorter");
        }
    }

    #[test]
    fn pinches_hold_one_crossing_near_the_axis() {
        let w = Params::default().width;
        let (up, view) = (60.0 * w, 840.0);
        for seed in [1, 2] {
            let mut sim = Sim::new(Params::default(), [up + view + 20.0 * w, 480.0], seed, 0.5);
            sim.pinches = vec![up, up + view];
            for _ in 0..30 {
                sim.step_n(1000);
                for &px in &sim.pinches {
                    let crossings: Vec<f64> = sim
                        .centerline
                        .windows(2)
                        .filter(|s| (s[0][0] < px) != (s[1][0] < px))
                        .map(|s| s[0][1])
                        .collect();
                    assert_eq!(crossings.len(), 1, "seed {seed}, step {}: crossings at x = {px}: {crossings:?}", sim.steps);
                    let off = (crossings[0] - 240.0).abs();
                    assert!(off < 0.5 * sim.params.pinch_width * w + w, "seed {seed}, step {}: crossing {off} off axis", sim.steps);
                }
            }
        }
    }

    #[test]
    fn physics_sets_the_kernel_weights() {
        let mut p = Params::default();
        let alpha = p.alpha;
        p.set("friction", 0.02);
        assert_eq!(p.alpha, alpha, "hand-tuned weights stay until physics is on");
        p.set("physics", 1.0);
        assert_eq!(p.get("physics"), Some(1.0));
        assert_eq!(p.omega, -1.0);
        assert!((p.gamma - 0.5 * (0.3f64.powi(2) + 2.89 + 3.0)).abs() < 1e-12);
        assert!((p.alpha - 2.0 * 0.02 * 30.0).abs() < 1e-12);
        p.set("gamma", 9.0);
        assert!(p.gamma < 4.0, "derived weights override direct sets");
        p.set("aspect", 10.0);
        assert!((p.alpha - 0.4).abs() < 1e-12);
    }

    #[test]
    fn lakes_near_the_channel_fill_first() {
        let mut sim = sine_sim(0.0, 40.0);
        sim.params.flood_rate = 1.0;
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // Straight lakes beside the channel, near and far, and one whose
        // entrance lies on the channel at a small diversion angle.
        let lake = |x0: f64, y: f64| -> Vec<P> { (0..30).map(|k| [(x0 + 0.5 * k as f64) * w, mid + y * w]).collect() };
        for (pts, entry) in [(lake(100.0, 2.0), 0.0), (lake(150.0, 10.0), 0.0), (lake(200.0, 0.0), 0.9)] {
            sim.erodibility.stamp_oxbow(&pts, 0.5 * w, 0.0);
            sim.oxbows.push(Oxbow::lake(pts, 0.0, entry));
        }
        // The third lake runs along the channel; keep only its entrance there.
        sim.oxbows[2].points.iter_mut().skip(1).for_each(|q| q[1] += 3.0 * w);
        let fills = |sim: &Sim| sim.oxbows.iter().map(|o| o.fill).collect::<Vec<_>>();
        sim.step_n(1000);
        let f = fills(&sim);
        eprintln!("fills after {} floods: {f:?}", sim.floods);
        let quiet = sim.time / sim.params.oxbow_lifetime;
        assert!((f[1] - quiet).abs() < 0.05, "a far lake fills only by the clock: {} against {quiet}", f[1]);
        assert!(f[0] > f[1] + 0.2, "a near lake silts up faster");
        assert!(f[2] > f[1] + 0.2, "a lake with an open entrance takes in bedload");
    }

    #[test]
    fn floods_leave_scroll_bars_inside_migrating_bends() {
        let mut sim = sine_sim(2.0, 30.0);
        sim.params.flood_rate = 0.0;
        sim.step_n(50);
        sim.add_scrolls();
        assert!(!sim.scrolls.is_empty());
        let w = sim.params.width;
        let mid = sim.bounds[1] / 2.0;
        let line = sim.centerline.clone();
        let gap = |q: P| line.iter().map(|&c| dist(q, c)).fold(f64::INFINITY, f64::min);
        let mut bends = 0;
        for sc in &sim.scrolls {
            assert!(sc.points.len() >= MIN_SCROLL_POINTS);
            for &q in &sc.points {
                assert!(gap(q) > 0.5 * w && gap(q) < 0.7 * w, "a ridge lies just off the bank");
            }
            // Bends grow outwards, so the ridge at a bend lies on its axis
            // side: inside the bend.
            let k = sc.points.len() / 2;
            let c = line.iter().min_by(|a, b| dist(**a, sc.points[k]).total_cmp(&dist(**b, sc.points[k]))).unwrap();
            if (c[1] - mid).abs() > 1.5 * w {
                bends += 1;
                assert!((sc.points[k][1] - mid).abs() < (c[1] - mid).abs(), "ridge at {:?}, channel at {c:?}", sc.points[k]);
            }
        }
        assert!(bends >= 4, "only {bends} ridges at bends");
        // The channel erodes a ridge it crosses.
        let v = sim.scroll_version;
        let across: Vec<P> = (-10..=10).map(|k| [100.0 * w, mid + 0.5 * k as f64 * w]).collect();
        sim.scrolls = vec![Scroll { points: across, born: sim.time }];
        sim.rework_scrolls();
        assert!(sim.scroll_version > v);
        for sc in &sim.scrolls {
            assert!(sc.points.iter().all(|&q| gap(q) >= 0.5 * w));
        }
    }

    #[test]
    fn a_big_flood_breaches_an_outer_bend() {
        // Curvature peaks at 4·(2π/20)² ≈ 0.39 per width, over the 0.2 a breach needs.
        let mut sim = sine_sim(4.0, 20.0);
        sim.params.splay_chance = 1.0;
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        assert!(!sim.crevasse(SPLAY_MIN_FLOOD), "too small a flood");
        assert!(sim.crevasse(1.0));
        let s = &sim.splays[0];
        assert!((s.length - 8.0 * w).abs() < 1e-9);
        // The sharpest bends are the crests, so the splay runs away from the axis.
        assert!((s.apex[1] - mid).abs() > 2.4 * w, "apex {:?}", s.apex);
        assert!((s.apex[1] + s.dir[1] - mid).abs() > (s.apex[1] - mid).abs(), "dir {:?}", s.dir);
        let inside = [s.apex[0] + 0.3 * s.length * s.dir[0], s.apex[1] + 0.3 * s.length * s.dir[1]];
        assert!(sim.erodibility.deposit_at(inside) > 0.1 * sim.params.levee_height);
    }

    #[test]
    fn the_channel_erodes_a_splay_from_the_breach() {
        let mut sim = sine_sim(4.0, 20.0);
        sim.params.splay_chance = 1.0;
        assert!(sim.crevasse(1.0));
        let w = sim.params.width;
        let s = sim.splays[0].clone();
        let perp = [-s.dir[1], s.dir[0]];
        // The bend moves 1.5 widths into the splay.
        sim.time += 1.0;
        let swept: Vec<P> = (0..=6)
            .flat_map(|i| (-40..=40).map(move |j| (0.25 * i as f64 * w, 0.1 * j as f64 * w)))
            .map(|(r, x)| [s.apex[0] + r * s.dir[0] + x * perp[0], s.apex[1] + r * s.dir[1] + x * perp[1]])
            .collect();
        sim.erodibility.sweep(&swept, sim.time);
        let v = sim.splay_version;
        sim.erode_splays();
        assert!(sim.splay_version > v);
        let left = &sim.splays[0];
        let v8 = left.rays[8].0;
        for (k, (_, [a, b])) in left.rays.iter().enumerate() {
            // The channel reaches 0.75 widths past the band, give or take a
            // cell, and flank rays cross it obliquely.
            assert!(*a > 1.5 * w && *a <= 3.5 * w, "ray {k} eroded to {a}");
            assert_eq!(*b, s.rays[k].1[1], "the rim is untouched");
        }
        // Once the channel has crossed all of it, the splay is gone.
        let all: Vec<P> = (0..=4 * 9).flat_map(|i| (-60..=60).map(move |j| (0.25 * i as f64 * w, 0.1 * j as f64 * w)))
            .map(|(r, x)| [s.apex[0] + r * s.dir[0] + x * perp[0], s.apex[1] + r * s.dir[1] + x * perp[1]])
            .collect();
        sim.erodibility.sweep(&all, sim.time);
        sim.erode_splays();
        assert!(sim.splays.is_empty());
    }

    #[test]
    fn channel_reworks_oxbows_it_crosses() {
        let mut sim = sine_sim(0.0, 40.0);
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // A vertical oxbow across the channel, and one well clear of it.
        let across: Vec<P> = (-20..=20).map(|k| [100.0 * w, mid + 0.5 * k as f64 * w]).collect();
        let clear: Vec<P> = (0..20).map(|k| [(150 + k) as f64 * w, mid + 20.0 * w]).collect();
        sim.oxbows.push(Oxbow::lake(across, 0.0, 0.0));
        sim.oxbows.push(Oxbow::lake(clear.clone(), 0.0, 0.0));
        sim.step();
        assert_eq!(sim.oxbows.len(), 3, "the crossed oxbow splits in two");
        assert_eq!(sim.oxbows[2].points, clear);
        // Each piece reaches one point under the water at the cut, and
        // keeps its place along the original oxbow.
        let gap = |q: P| sim.centerline.iter().map(|&c| dist(q, c)).fold(f64::INFINITY, f64::min);
        let (a, b) = (&sim.oxbows[0], &sim.oxbows[1]);
        for o in [a, b] {
            let m = o.points.len();
            assert!(o.points[1..m - 1].iter().all(|&q| gap(q) >= 0.5 * w));
        }
        assert!(gap(a.points[a.points.len() - 1]) < 0.5 * w && gap(b.points[0]) < 0.5 * w);
        assert!((b.offset - dist(b.points[0], [100.0 * w, mid - 10.0 * w])).abs() < 1e-9);
    }

    #[test]
    fn channel_splits_an_oxbow_between_its_points() {
        let mut sim = sine_sim(0.0, 40.0);
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // The channel runs between two points a quarter width either side.
        let across: Vec<P> = (-20..20).map(|k| [100.0 * w, mid + (0.5 * k as f64 + 0.25) * w]).collect();
        sim.oxbows.push(Oxbow::lake(across, 0.0, 0.0));
        sim.step();
        assert_eq!(sim.oxbows.len(), 2, "the crossed oxbow splits in two");
        assert_eq!(sim.oxbows[0].points.len() + sim.oxbows[1].points.len(), 40);
    }

    #[test]
    fn channel_alongside_an_oxbow_leaves_it_whole() {
        let mut sim = sine_sim(0.0, 40.0);
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // Under the water on the channel side, but clear of the centreline.
        let beside: Vec<P> = (0..20).map(|k| [(100.0 + 0.5 * k as f64) * w, mid + 0.3 * w]).collect();
        sim.oxbows.push(Oxbow::lake(beside.clone(), 0.0, 0.0));
        sim.step();
        assert_eq!(sim.oxbows.len(), 1);
        assert_eq!(sim.oxbows[0].points, beside);
    }
}
