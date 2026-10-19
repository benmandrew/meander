//! Spatially varying erodibility: smooth noise, ground the channel has
//! swept, which is loose point-bar sand until vegetation binds it, and oxbow
//! footprints that erode fast while open water and stiffen to clay plugs as
//! they fill. The same noise, with a finer layer added, gives the height of
//! the land. Overbank deposits build up on top, thickest beside the
//! channel as natural levees, and a higher bank erodes more slowly.
//! Tributary fans and slumps from the valley walls leave coarse ground,
//! gravel and colluvium, which erodes more slowly still.

use crate::geom::P;

/// Seeded 2D value noise, summed over octaves; returns roughly [0, 1].
pub fn fbm(seed: u64, x: f64, y: f64, octaves: u32) -> f64 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * value_noise(seed.wrapping_add(o as u64 * 0x9E37), x * freq, y * freq);
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

/// Seed offset of the fine layer of [`Erodibility::height`].
const HEIGHT_SEED: u64 = 0x4E1F_7A2B;

/// Depth of a fresh oxbow lake below fresh point-bar sand, in channel
/// depths: the bed of the channel it was cut from.
pub const LAKE_DEPTH: f64 = 1.0;

/// Distance from the bank, in levee lengths, beyond which no overbank
/// deposit settles; exp(−4) is under 2% of the deposit at the bank.
const LEVEE_REACH: f64 = 4.0;

/// Growth rates of vegetation, as multiples of the rate the vegetation
/// and forest times assume: on fresh point-bar sand, which high water covers
/// often, and on ground raised by the growth height or more, which it
/// seldom reaches.
const BARE_GROWTH: f32 = 0.6;
const HIGH_GROWTH: f32 = 1.6;

pub(crate) fn lattice(seed: u64, x: i64, y: i64) -> f64 {
    let mut h = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

pub(crate) fn value_noise(seed: u64, x: f64, y: f64) -> f64 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i64, y0 as i64);
    let a = lattice(seed, ix, iy);
    let b = lattice(seed, ix + 1, iy);
    let c = lattice(seed, ix, iy + 1);
    let d = lattice(seed, ix + 1, iy + 1);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// How erodibility depends on the history of the ground.
pub struct Ground {
    /// Current simulation time.
    pub time: f64,
    /// Strength of the noise: 0 is uniform, 1 ranges from near zero to
    /// double.
    pub heterogeneity: f64,
    /// Time a lake takes to fill.
    pub lake_life: f64,
    /// Multiplier when the lake is first cut off, as open water.
    pub lake: f64,
    /// Multiplier once the lake has filled, as a clay plug.
    pub clay: f64,
    /// Multiplier on ground the channel has just left, as bare point-bar
    /// sand. It moves to `shrub` as growth reaches `vegetation`.
    pub bar: f64,
    /// Growth at which vegetation binds a point bar.
    pub vegetation: f64,
    /// Multiplier once shrub has bound a point bar. It moves to 1, the
    /// mature forest of the old floodplain, over a further `forest` of
    /// growth.
    pub shrub: f64,
    /// Growth shrub needs to become mature forest.
    pub forest: f64,
    /// Multiplier on wholly coarse ground; see [`Erodibility::coarse`].
    pub coarse: f64,
}

/// Overbank deposit beside a channel; see [`Erodibility::lay_overbank`].
/// Lengths are in world units.
pub struct Overbank<'a> {
    /// Half the channel width, which each point's local width multiplies.
    pub half: f64,
    /// Deposit at the bank, in channel depths.
    pub amount: f64,
    /// Distance over which the deposit thins by a factor of e.
    pub length: f64,
    /// Flood level above fresh point-bar sand, in channel depths.
    pub stage: f64,
    /// Factor on the deposit beside each channel point, as a mill pond
    /// raises it, or empty for 1 everywhere.
    pub boost: &'a [f64],
}

/// Erodibility multiplier on a grid. Built from noise at reset. Cells the
/// channel crosses are reset to bare sand with no growth, and their
/// multiplier moves from the bar factor to the shrub factor as vegetation
/// binds the sand, then to 1 as forest grows.
/// Oxbow footprints are stamped with their cutoff time, and their multiplier
/// moves from the lake factor to the clay factor as the lake fills.
pub struct Erodibility {
    pub cell: f64,
    pub cols: usize,
    pub rows: usize,
    /// Noise in [0, 1], before the heterogeneity strength is applied.
    pub noise: Vec<f32>,
    /// Time each cell was cut off as part of an oxbow, or infinity.
    pub cut_at: Vec<f64>,
    /// Vegetation on each cell, as the simulated time it would have taken
    /// to grow at the rate the vegetation and forest times assume: 0 where
    /// the channel has just swept, infinity where it never has. It grows
    /// faster on higher ground (see [`Erodibility::grow`]).
    pub growth: Vec<f32>,
    /// Height of overbank deposits above fresh point-bar sand, in channel
    /// depths. The channel resets it to 0 where it sweeps, and a cutoff
    /// sets it to −`LAKE_DEPTH` over the oxbow footprint.
    pub deposit: Vec<f32>,
    /// Share of coarse material in each cell, in [0, 1]: gravel on a
    /// tributary fan, colluvium slumped from a valley wall. The channel
    /// halves it where it sweeps, as it reworks the gravel into its bars.
    pub coarse: Vec<f32>,
    /// Scratch: squared distance from each cell of a band of rows to the
    /// channel, as overbank deposit is laid.
    near: Vec<f32>,
    /// Scratch: the channel point nearest each cell of the band.
    nearest: Vec<u32>,
    /// Scratch for the nearest-point search: the points in order of x, the
    /// points within reach of each row, from `row_start[y]` in
    /// `row_points`, and the lower envelope of their distances along one
    /// row.
    order: Vec<u32>,
    row_start: Vec<u32>,
    row_points: Vec<u32>,
    hull: Vec<(f64, f64, u32, f64)>,
    pub(crate) seed: u64,
    scale: f64,
}

impl Erodibility {
    /// `scale` is the noise feature size in world units.
    pub fn new(bounds: P, cell: f64, scale: f64, seed: u64) -> Self {
        let cols = (bounds[0] / cell).ceil().max(1.0) as usize;
        let rows = (bounds[1] / cell).ceil().max(1.0) as usize;
        let mut noise = Vec::with_capacity(cols * rows);
        for y in 0..rows {
            for x in 0..cols {
                let (wx, wy) = ((x as f64 + 0.5) * cell, (y as f64 + 0.5) * cell);
                noise.push(fbm(seed, wx / scale, wy / scale, 4) as f32);
            }
        }
        let cells = cols * rows;
        Self {
            cell,
            cols,
            rows,
            noise,
            cut_at: vec![f64::INFINITY; cells],
            growth: vec![f32::INFINITY; cells],
            deposit: vec![0.0; cells],
            coarse: vec![0.0; cells],
            near: Vec::new(),
            nearest: Vec::new(),
            order: Vec::new(),
            row_start: Vec::new(),
            row_points: Vec::new(),
            hull: Vec::new(),
            seed,
            scale,
        }
    }

    fn index(&self, p: P) -> usize {
        let x = ((p[0] / self.cell) as usize).min(self.cols - 1);
        let y = ((p[1] / self.cell) as usize).min(self.rows - 1);
        y * self.cols + x
    }

    /// Noise at `p`, in [0, 1].
    pub fn noise_at(&self, p: P) -> f64 {
        self.noise[self.index(p)] as f64
    }

    /// Height of the land at `p`, in [0, 1] and near 0.5 on average. Low,
    /// slow-eroding ground lies low, so clay hollows are deeper; a layer at
    /// a quarter of the noise scale adds relief along a single oxbow.
    pub fn height(&self, p: P) -> f64 {
        let (x, y) = (p[0] / self.scale, p[1] / self.scale);
        0.5 * fbm(self.seed, x, y, 4) + 0.5 * fbm(self.seed ^ HEIGHT_SEED, 4.0 * x, 4.0 * y, 3)
    }

    /// Multiplier at `p`: noise of strength `g.heterogeneity` around 1,
    /// times a factor for the ground's history. Inside an oxbow footprint it
    /// moves linearly from `g.lake` to `g.clay` over `g.lake_life`, or
    /// faster as silt fills the lake (see [`Erodibility::silt_at`]); on a
    /// point bar, from `g.bar` to `g.shrub` as growth reaches
    /// `g.vegetation`, then to 1 over a further `g.forest`. A bank raised by
    /// overbank deposits of height h, in channel depths, holds 1 + h times
    /// the material of a bank at bar level, so it erodes 1 / (1 + h) as
    /// fast. Coarse ground moves the multiplier towards `g.coarse`.
    pub fn at(&self, p: P, g: &Ground) -> f64 {
        let i = self.index(p);
        let n = 1.0 + g.heterogeneity * 2.0 * (self.noise[i] as f64 - 0.5);
        let since = |t: f64, life: f64| ((g.time - t) / life.max(1e-9)).clamp(0.0, 1.0);
        let cut = self.cut_at[i];
        let c = if cut.is_finite() {
            let silt = (1.0 + self.deposit[i] as f64 / LAKE_DEPTH).clamp(0.0, 1.0);
            g.lake + (g.clay - g.lake) * since(cut, g.lake_life).max(silt)
        } else {
            // Never-swept ground has infinite growth and a multiplier of 1.
            let growth = self.growth[i] as f64;
            let shrub =
                g.bar + (g.shrub - g.bar) * (growth / g.vegetation.max(1e-9)).clamp(0.0, 1.0);
            shrub + (1.0 - shrub) * ((growth - g.vegetation) / g.forest.max(1e-9)).clamp(0.0, 1.0)
        };
        let coarse = 1.0 - (1.0 - g.coarse) * self.coarse[i] as f64;
        n.max(0.05) * c * coarse / (1.0 + (self.deposit[i] as f64).max(0.0))
    }

    /// Vegetation growth at `p`; see [`Erodibility::growth`].
    pub fn growth_at(&self, p: P) -> f64 {
        self.growth[self.index(p)] as f64
    }

    /// Vegetation on cell `i` as drawn: its growth, capped at `full`, and 0
    /// on oxbow ground, where the lake and its clay plug hold no wood.
    pub fn vegetation(&self, i: usize, full: f32) -> f32 {
        if self.cut_at[i].is_finite() {
            0.0
        } else {
            self.growth[i].min(full)
        }
    }

    /// Index of the cell under `p`, clamped to the grid.
    pub fn cell_index(&self, p: P) -> usize {
        self.index(p)
    }

    /// Overbank deposit at `p`, in channel depths.
    pub fn deposit_at(&self, p: P) -> f64 {
        self.deposit[self.index(p)] as f64
    }

    /// Fraction of a fresh oxbow lake's depth that overbank silt has
    /// filled at `p`, in [0, 1]: 1 once the bed reaches the level of fresh
    /// point-bar sand.
    pub fn silt_at(&self, p: P) -> f64 {
        (1.0 + self.deposit_at(p) / LAKE_DEPTH).clamp(0.0, 1.0)
    }

    /// [`Erodibility::silt_at`] where `p` lies in a lake footprint, or
    /// `None` where the channel has swept the cell since the cutoff. Its
    /// cells are half a width across, so the channel sweeps cells under
    /// lake points beside it, and their silt is that of fresh bar sand.
    pub fn lake_silt_at(&self, p: P) -> Option<f64> {
        self.cut_at[self.index(p)]
            .is_finite()
            .then(|| self.silt_at(p))
    }

    /// Mean of [`Erodibility::lake_silt_at`] over the interior points of an
    /// oxbow, whose ends lie under the channel at a cut, or 0 if the
    /// channel has swept them all.
    pub fn lake_silt(&self, pts: &[P]) -> f64 {
        let m = pts.len();
        let inner = if m > 2 { &pts[1..m - 1] } else { pts };
        let (sum, count) = inner
            .iter()
            .filter_map(|&q| self.lake_silt_at(q))
            .fold((0.0, 0usize), |(s, c), x| (s + x, c + 1));
        if count > 0 { sum / count as f64 } else { 0.0 }
    }

    /// Lay overbank deposit beside a channel along the points, over the
    /// rows from `y0` up to `y1`, with one search for the point nearest each
    /// cell. `widths` holds the local width at each point; see [`Overbank`].
    /// Each step lays one band of rows, so a cell gains its deposit a little
    /// at a time, at the mean rate of the river's floods.
    ///
    /// A cell at distance d from the bank gains
    /// `amount · exp(−d / length) · (1 − h / stage)`, so deposits thin away
    /// from the channel into natural levees and stop growing as the ground
    /// nears the flood level `stage`. The factor is the depth of flood water
    /// over the ground, so a lake bed `LAKE_DEPTH` below fresh bar level
    /// traps 1 + `LAKE_DEPTH / stage` times as much. Cells under the channel
    /// gain nothing.
    pub fn lay_overbank(&mut self, pts: &[P], widths: &[f64], f: &Overbank, y0: usize, y1: usize) {
        let y1 = y1.min(self.rows);
        if f.amount <= 0.0 || f.length <= 0.0 || f.stage <= 0.0 || y0 >= y1 {
            return;
        }
        let widest = widths.iter().fold(1.0f64, |a, &b| a.max(b));
        self.find_nearest(pts, widest * f.half + LEVEE_REACH * f.length, y0, y1);
        let cells = &mut self.deposit[y0 * self.cols..y1 * self.cols];
        for (h, (&d2, &k)) in cells.iter_mut().zip(self.near.iter().zip(&self.nearest)) {
            let (d, bank) = ((d2 as f64).sqrt(), f.half * widths[k as usize]);
            if !d.is_finite() || d < bank {
                continue;
            }
            let room = (1.0 - *h as f64 / f.stage).max(0.0);
            let boost = f.boost.get(k as usize).copied().unwrap_or(1.0);
            *h += (boost * f.amount * (-(d - bank) / f.length).exp() * room) as f32;
        }
    }

    /// Fill the scratch `near` with the squared distance from each cell of
    /// the rows from `y0` up to `y1` to the nearest of the points within
    /// `reach`, or infinity, and `nearest` with that point's index. The
    /// scratch holds the band alone, from row `y0`.
    ///
    /// Row by row: over one row, the squared distance to point k is the
    /// parabola (x − x_k)² + g_k, with g_k the squared distance from the row
    /// to the point. The lower envelope of the parabolas of the points
    /// within `reach` of the row gives the nearest point to every cell in
    /// one sweep along it, as in Felzenszwalb and Huttenlocher's distance
    /// transform. Searching a box around each point instead visited every
    /// cell once for each point within reach of it, about 40 times.
    fn find_nearest(&mut self, pts: &[P], reach: f64, y0: usize, y1: usize) {
        let (c, r2, cols) = (self.cell, reach * reach, self.cols);
        let band = y1 - y0;
        self.near.clear();
        self.near.resize(band * cols, f32::INFINITY);
        self.nearest.clear();
        self.nearest.resize(band * cols, 0);
        // The rows of the band within `reach` of a point, from 0 at `y0`.
        let rows_of = |p: P| {
            let lo = ((p[1] - reach) / c).floor().max(y0 as f64) as usize - y0;
            let hi = ((((p[1] + reach) / c).max(0.0) as usize) + 1).min(y1);
            lo..hi.saturating_sub(y0).max(lo)
        };
        // Only the points that reach the band, in order of x.
        self.order.clear();
        self.order
            .extend((0..pts.len() as u32).filter(|&k| !rows_of(pts[k as usize]).is_empty()));
        self.order.sort_unstable_by(|&a, &b| {
            pts[a as usize][0]
                .total_cmp(&pts[b as usize][0])
                .then(a.cmp(&b))
        });
        // The points within `reach` of each row: count them into
        // `row_start`, sum the counts, then place each point by counting
        // down, which leaves `row_start[y]` at the start of row y. Placing
        // them in falling order of x leaves each row in rising order.
        self.row_start.clear();
        self.row_start.resize(band + 1, 0);
        for &k in &self.order {
            for y in rows_of(pts[k as usize]) {
                self.row_start[y] += 1;
            }
        }
        for y in 1..=band {
            self.row_start[y] += self.row_start[y - 1];
        }
        self.row_points.clear();
        self.row_points.resize(self.row_start[band] as usize, 0);
        for &k in self.order.iter().rev() {
            for y in rows_of(pts[k as usize]) {
                self.row_start[y] -= 1;
                self.row_points[self.row_start[y] as usize] = k;
            }
        }
        for y in 0..band {
            let row = &self.row_points[self.row_start[y] as usize..self.row_start[y + 1] as usize];
            if row.is_empty() {
                continue;
            }
            // The envelope, as (vertex x, g, point, x from which it is the
            // lowest), built in order of x.
            let cy = ((y0 + y) as f64 + 0.5) * c;
            let hull = &mut self.hull;
            hull.clear();
            for &k in row.iter() {
                let [x, py] = pts[k as usize];
                let g = (cy - py) * (cy - py);
                while let Some(&(px, pg, _, from)) = hull.last() {
                    if x == px {
                        // Of two points above one another, the nearer wins.
                        if g < pg {
                            hull.pop();
                            continue;
                        }
                        break;
                    }
                    let meet = ((g + x * x) - (pg + px * px)) / (2.0 * (x - px));
                    if meet <= from {
                        hull.pop();
                        continue;
                    }
                    hull.push((x, g, k, meet));
                    break;
                }
                if hull.is_empty() {
                    hull.push((x, g, k, f64::NEG_INFINITY));
                }
            }
            let mut h = 0;
            for x in 0..cols {
                let cx = (x as f64 + 0.5) * c;
                while h + 1 < hull.len() && hull[h + 1].3 <= cx {
                    h += 1;
                }
                let (px, g, k, _) = hull[h];
                let d2 = (cx - px) * (cx - px) + g;
                if d2 < r2 {
                    self.near[y * cols + x] = d2 as f32;
                    self.nearest[y * cols + x] = k;
                }
            }
        }
    }

    /// Raise the ground under each point by `height` channel depths, as the
    /// ridge of a scroll bar does, scaled like overbank deposit by the room
    /// left under the flood level `stage`. Oxbow footprints are left alone.
    pub fn raise(&mut self, pts: &[P], height: f64, stage: f64) {
        if height <= 0.0 || stage <= 0.0 {
            return;
        }
        let mut last = usize::MAX;
        for &p in pts {
            let i = self.index(p);
            if i != last && self.cut_at[i].is_infinite() {
                let h = &mut self.deposit[i];
                *h += (height * (1.0 - *h as f64 / stage).max(0.0)) as f32;
            }
            last = i;
        }
    }

    /// Grow the vegetation on every cell by `dt` of simulated time, times a
    /// rate set by the cell's overbank deposit: `BARE_GROWTH` on fresh bar
    /// sand, rising linearly to `HIGH_GROWTH` at a deposit of `height`
    /// channel depths. A `height` of 0 grows every cell at rate 1.
    pub fn grow(&mut self, dt: f64, height: f64) {
        self.grow_rows(dt, height, 0, self.rows);
    }

    /// [`Erodibility::grow`] on rows [y0, y1) only.
    pub fn grow_rows(&mut self, dt: f64, height: f64, y0: usize, y1: usize) {
        let dt = dt as f32;
        let (lo, hi) = (y0 * self.cols, y1.min(self.rows) * self.cols);
        if height <= 0.0 {
            self.growth[lo..hi].iter_mut().for_each(|g| *g += dt);
            return;
        }
        let k = (HIGH_GROWTH - BARE_GROWTH) / height as f32;
        for (g, &h) in self.growth[lo..hi].iter_mut().zip(&self.deposit[lo..hi]) {
            *g += dt * (BARE_GROWTH + k * h.max(0.0)).min(HIGH_GROWTH);
        }
    }

    /// Mark the cell under each point as swept by the channel, with no
    /// growth. The channel replaces any oxbow fill there with fresh
    /// point-bar sand.
    pub fn sweep(&mut self, pts: &[P]) {
        for &p in pts {
            let i = self.index(p);
            self.growth[i] = 0.0;
            self.cut_at[i] = f64::INFINITY;
            self.deposit[i] = 0.0;
            self.coarse[i] *= 0.5;
        }
    }

    /// Columns and rows, inclusive, of the cells that may lie within
    /// `radius` of `apex`.
    fn disc(&self, apex: P, radius: f64) -> [usize; 4] {
        let c = self.cell;
        let x0 = ((apex[0] - radius) / c).floor().max(0.0) as usize;
        let y0 = ((apex[1] - radius) / c).floor().max(0.0) as usize;
        let x1 = (((apex[0] + radius) / c).max(0.0) as usize).min(self.cols - 1);
        let y1 = (((apex[1] + radius) / c).max(0.0) as usize).min(self.rows - 1);
        [x0, y0, x1, y1]
    }

    /// Build a cone of coarse ground about `apex`, `radius` across: its
    /// surface stands `top` channel depths above fresh bar sand at the
    /// apex and its share of coarse material is `coarse` there, both
    /// falling to nothing at the rim. Each cell moves the fraction `frac`
    /// of the way up to the cone, never down, so a fan the channel has
    /// trimmed builds back over time. Lake footprints are left alone.
    pub fn grow_cone(&mut self, apex: P, radius: f64, top: f64, coarse: f64, frac: f64) {
        if radius <= 0.0 || frac <= 0.0 {
            return;
        }
        let (frac, c) = (frac.min(1.0) as f32, self.cell);
        let [x0, y0, x1, y1] = self.disc(apex, radius);
        for y in y0..=y1 {
            let dy = (y as f64 + 0.5) * c - apex[1];
            for x in x0..=x1 {
                let (dx, i) = ((x as f64 + 0.5) * c - apex[0], y * self.cols + x);
                let d2 = dx * dx + dy * dy;
                if d2 >= radius * radius || self.cut_at[i].is_finite() {
                    continue;
                }
                let t = 1.0 - d2.sqrt() / radius;
                let (h, k) = ((top * t) as f32, (coarse * t.sqrt()) as f32);
                self.deposit[i] += frac * (h - self.deposit[i]).max(0.0);
                self.coarse[i] += frac * (k - self.coarse[i]).max(0.0);
            }
        }
    }

    /// Build a tributary's delta out into a lake: raise the bed of the lake
    /// cells within `radius` of `apex` by up to `amount`, in channel
    /// depths, thinning to nothing at the rim, no higher than fresh bar
    /// sand.
    pub fn delta(&mut self, apex: P, radius: f64, amount: f64) {
        if radius <= 0.0 || amount <= 0.0 {
            return;
        }
        let c = self.cell;
        let [x0, y0, x1, y1] = self.disc(apex, radius);
        for y in y0..=y1 {
            let dy = (y as f64 + 0.5) * c - apex[1];
            for x in x0..=x1 {
                let (dx, i) = ((x as f64 + 0.5) * c - apex[0], y * self.cols + x);
                let d2 = dx * dx + dy * dy;
                if d2 >= radius * radius || self.cut_at[i].is_infinite() || self.deposit[i] >= 0.0 {
                    continue;
                }
                let t = 1.0 - d2.sqrt() / radius;
                self.deposit[i] = (self.deposit[i] + (amount * t) as f32).min(0.0);
            }
        }
    }

    /// Mark cells within `half` times the width at each point (`widths`,
    /// one per point) as cut off at `time`, with the bed `LAKE_DEPTH` below
    /// fresh bar level.
    pub fn stamp_oxbow(&mut self, pts: &[P], widths: &[f64], half: f64, time: f64) {
        let c = self.cell;
        for (p, &b) in pts.iter().zip(widths) {
            let radius = half * b;
            let r2 = radius * radius;
            let x0 = ((p[0] - radius) / c).floor().max(0.0) as usize;
            let y0 = ((p[1] - radius) / c).floor().max(0.0) as usize;
            let x1 = (((p[0] + radius) / c) as usize).min(self.cols - 1);
            let y1 = (((p[1] + radius) / c) as usize).min(self.rows - 1);
            for y in y0..=y1 {
                let dy = (y as f64 + 0.5) * c - p[1];
                for x in x0..=x1 {
                    let dx = (x as f64 + 0.5) * c - p[0];
                    if dx * dx + dy * dy <= r2 {
                        self.cut_at[y * self.cols + x] = time;
                        self.deposit[y * self.cols + x] = -LAKE_DEPTH as f32;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fbm_is_bounded_and_deterministic() {
        for k in 0..1000 {
            let (x, y) = (k as f64 * 0.37, k as f64 * 0.11);
            let v = fbm(5, x, y, 4);
            assert!((0.0..=1.0).contains(&v));
            assert_eq!(v, fbm(5, x, y, 4));
        }
        assert_ne!(fbm(1, 0.5, 0.5, 4), fbm(2, 0.5, 0.5, 4));
    }

    #[test]
    fn height_is_bounded_and_varies_along_an_oxbow() {
        let e = Erodibility::new([100.0, 100.0], 1.0, 25.0, 3);
        let h: Vec<f64> = (0..400)
            .map(|k| e.height([k as f64 * 0.25, 50.0]))
            .collect();
        assert!(h.iter().all(|v| (0.0..=1.0).contains(v)));
        let (lo, hi) = h
            .iter()
            .fold((1.0f64, 0.0f64), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(hi - lo > 0.1, "range {lo}..{hi}");
    }

    /// Overbank deposit up to the flood level `stage`.
    fn deposit(stage: f64) -> Overbank<'static> {
        Overbank {
            half: 1.0,
            amount: 0.3,
            length: 2.0,
            stage,
            boost: &[],
        }
    }

    /// Lay `f` over every row, as a full cycle of bands does.
    fn lay(e: &mut Erodibility, channel: &[P], f: &Overbank) {
        let rows = e.rows;
        e.lay_overbank(channel, &vec![1.0; channel.len()], f, 0, rows);
    }

    fn ground(time: f64) -> Ground {
        Ground {
            time,
            heterogeneity: 0.0,
            lake_life: 100.0,
            lake: 3.0,
            clay: 0.3,
            bar: 2.0,
            vegetation: 50.0,
            shrub: 1.0,
            forest: 1.0,
            coarse: 0.5,
        }
    }

    #[test]
    fn oxbow_erodes_fast_then_stiffens_to_clay() {
        let mut e = Erodibility::new([10.0, 10.0], 1.0, 5.0, 0);
        let at = |e: &Erodibility, time| e.at([5.0, 5.0], &ground(time));
        assert_eq!(at(&e, 10.0), 1.0);
        e.stamp_oxbow(&[[5.0, 5.0]], &[1.0], 1.0, 10.0);
        assert!((at(&e, 10.0) - 3.0).abs() < 1e-12);
        assert!((at(&e, 60.0) - 1.65).abs() < 1e-12);
        assert!((at(&e, 110.0) - 0.3).abs() < 1e-12);
        assert!((at(&e, 1000.0) - 0.3).abs() < 1e-12);
        assert_eq!(e.at([1.0, 1.0], &ground(10.0)), 1.0);
    }

    #[test]
    fn a_point_bar_grows_shrub_then_forest() {
        let mut e = Erodibility::new([10.0, 10.0], 1.0, 5.0, 0);
        let g = Ground {
            shrub: 1.5,
            forest: 200.0,
            ..ground(0.0)
        };
        let at = |e: &Erodibility| e.at([5.0, 5.0], &g);
        assert_eq!(at(&e), 1.0, "never swept");
        e.sweep(&[[5.0, 5.0]]);
        assert!((at(&e) - 2.0).abs() < 1e-12, "bare sand");
        e.grow(50.0, 0.0);
        assert!((at(&e) - 1.5).abs() < 1e-12, "shrub");
        e.grow(100.0, 0.0);
        assert!((at(&e) - 1.25).abs() < 1e-12, "young forest");
        e.grow(100.0, 0.0);
        assert!((at(&e) - 1.0).abs() < 1e-12, "mature forest");
        e.grow(1000.0, 0.0);
        assert!((at(&e) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn raised_ground_vegetates_first() {
        let mut e = Erodibility::new([10.0, 10.0], 1.0, 5.0, 0);
        let (low, high) = ([2.5, 5.5], [7.5, 5.5]);
        e.sweep(&[low, high]);
        e.raise(&[high, high], 0.1, 1.0);
        assert!(
            (e.deposit_at(high) - 0.1).abs() < 1e-6,
            "a ridge raises its cell once"
        );
        e.grow(10.0, 0.1);
        let growth = |p| e.growth[e.index(p)];
        assert!((growth(low) - 10.0 * BARE_GROWTH).abs() < 1e-4);
        assert!((growth(high) - 10.0 * HIGH_GROWTH).abs() < 1e-4);
    }

    #[test]
    fn nearest_search_matches_brute_force() {
        let mut e = Erodibility::new([40.0, 30.0], 0.5, 5.0, 0);
        // A winding channel that doubles back, with points above one
        // another, and some outside the grid.
        let mut pts: Vec<P> = (0..300)
            .map(|k| {
                let t = k as f64 * 0.1;
                [
                    2.0 + 1.2 * t + 6.0 * (0.7 * t).sin(),
                    15.0 + 9.0 * (0.45 * t).cos(),
                ]
            })
            .collect();
        pts.extend([[10.0, 5.0], [10.0, 7.0], [-3.0, 12.0], [45.0, 20.0]]);
        let reach = 4.0;
        // The whole grid, and bands of rows as the steps lay them.
        for (y0, y1) in [(0, e.rows), (0, 7), (7, 23), (e.rows - 3, e.rows)] {
            e.find_nearest(&pts, reach, y0, y1);
            for y in y0..y1 {
                for x in 0..e.cols {
                    let (cx, cy) = ((x as f64 + 0.5) * e.cell, (y as f64 + 0.5) * e.cell);
                    let best = pts
                        .iter()
                        .map(|p| (cx - p[0]).powi(2) + (cy - p[1]).powi(2))
                        .fold(f64::INFINITY, f64::min);
                    let i = (y - y0) * e.cols + x;
                    if best >= reach * reach {
                        assert!(e.near[i].is_infinite(), "cell {x}, {y} is out of reach");
                        continue;
                    }
                    let p = pts[e.nearest[i] as usize];
                    let found = (cx - p[0]).powi(2) + (cy - p[1]).powi(2);
                    assert!(
                        (found - best).abs() < 1e-9 && (e.near[i] as f64 - best).abs() < 1e-4,
                        "cell {x}, {y}: {found} for {best}"
                    );
                }
            }
        }
    }

    #[test]
    fn overbank_deposit_builds_levees_that_slow_erosion() {
        let mut e = Erodibility::new([40.0, 40.0], 0.5, 5.0, 0);
        let channel: Vec<P> = (0..80).map(|k| [k as f64 * 0.5, 20.0]).collect();
        let h = |e: &Erodibility, y: f64| e.deposit_at([20.0, y]);
        lay(&mut e, &channel, &deposit(1.0));
        assert_eq!(h(&e, 20.25), 0.0, "nothing settles under the channel");
        assert!(h(&e, 21.25) > 0.25, "bank {}", h(&e, 21.25));
        assert!(h(&e, 21.25) > 2.0 * h(&e, 23.25) && h(&e, 23.25) > 2.0 * h(&e, 25.25));
        assert_eq!(h(&e, 38.0), 0.0, "beyond the reach");
        for _ in 0..200 {
            lay(&mut e, &channel, &deposit(1.0));
        }
        assert!(
            h(&e, 21.25) <= 1.0 && h(&e, 21.25) > 0.95,
            "levee {}",
            h(&e, 21.25)
        );
        let at = |e: &Erodibility, y| e.at([20.0, y], &ground(0.0));
        assert!((at(&e, 21.25) - 1.0 / (1.0 + h(&e, 21.25))).abs() < 1e-9);
        e.sweep(&[[20.0, 21.25]]);
        assert_eq!(h(&e, 21.25), 0.0, "the channel clears the levee it crosses");
    }

    #[test]
    fn overbank_deposit_silts_up_lakes() {
        let mut e = Erodibility::new([40.0, 40.0], 0.5, 5.0, 0);
        let channel: Vec<P> = (0..80).map(|k| [k as f64 * 0.5, 20.0]).collect();
        e.stamp_oxbow(&[[20.0, 22.25]], &[1.0], 0.25, 0.0);
        assert_eq!(e.silt_at([20.0, 22.25]), 0.0);
        let at = |e: &Erodibility| e.at([20.0, 22.25], &ground(0.0));
        assert!((at(&e) - 3.0).abs() < 1e-9, "open water");
        lay(&mut e, &channel, &deposit(0.25));
        let silt = e.silt_at([20.0, 22.25]);
        assert!(silt > 0.3 && silt < 1.0, "silt {silt}");
        assert!(
            (at(&e) - (3.0 - 2.7 * silt)).abs() < 1e-6,
            "stiffens with the silt"
        );
        for _ in 0..50 {
            lay(&mut e, &channel, &deposit(0.25));
        }
        assert_eq!(e.silt_at([20.0, 22.25]), 1.0);
        assert!(
            e.deposit_at([20.0, 22.25]) <= 0.25,
            "fills no higher than the flood level"
        );
    }

    #[test]
    fn a_fan_builds_back_after_the_channel_trims_it() {
        let mut e = Erodibility::new([40.0, 40.0], 0.5, 5.0, 0);
        let (apex, toe) = ([20.0, 20.0], [23.25, 20.25]);
        e.grow_cone(apex, 5.0, 0.5, 1.0, 1.0);
        let at = |e: &Erodibility, p| e.at(p, &ground(0.0));
        assert!(
            e.coarse[e.index(apex)] > 0.95 && e.deposit_at(apex) > 0.45,
            "full cone at the apex"
        );
        assert!(
            at(&e, toe) < 1.0 && at(&e, toe) > at(&e, apex),
            "coarse ground erodes slowly, the apex slowest"
        );
        assert_eq!(e.coarse[e.index([30.0, 20.0])], 0.0, "beyond the rim");
        let full = e.coarse[e.index(toe)];
        e.sweep(&[toe]);
        assert!(
            (e.coarse[e.index(toe)] - 0.5 * full).abs() < 1e-6,
            "the channel reworks half the gravel"
        );
        e.grow_cone(apex, 5.0, 0.5, 1.0, 0.5);
        let back = e.coarse[e.index(toe)];
        assert!(
            back > 0.5 * full && back < full,
            "builds halfway back: {back} of {full}"
        );
        e.grow_cone(apex, 5.0, 0.0, 0.0, 1.0);
        assert_eq!(
            e.coarse[e.index(toe)],
            back,
            "a cone never lowers the ground"
        );
    }

    #[test]
    fn swept_ground_is_loose_until_vegetated() {
        let mut e = Erodibility::new([10.0, 10.0], 1.0, 5.0, 0);
        e.stamp_oxbow(&[[5.0, 5.0]], &[1.0], 1.0, 10.0);
        e.sweep(&[[5.5, 5.5]]);
        let at = |e: &Erodibility| e.at([5.5, 5.5], &ground(10.0));
        assert!(
            (at(&e) - 2.0).abs() < 1e-12,
            "the channel replaces the oxbow fill with sand"
        );
        e.grow(25.0, 0.0);
        assert!((at(&e) - 1.5).abs() < 1e-12);
        e.grow(75.0, 0.0);
        assert!((at(&e) - 1.0).abs() < 1e-12);
        assert!(
            (e.at([4.5, 5.5], &ground(10.0)) - 3.0).abs() < 1e-12,
            "only the swept cell changes"
        );
    }
}
