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
use field::Erodibility;
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
    /// Simulation time an oxbow lake takes to fill.
    pub oxbow_lifetime: f64,
    /// Simulation time a filled oxbow stays as a scar before it is dropped.
    pub scar_lifetime: f64,
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
            oxbow_lifetime: 400.0,
            scar_lifetime: 1600.0,
            jitter: 0.05,
            jitter_scale: 3.0,
            valley_width: 45.0,
            edge_margin: 8.0,
            pinch_width: 6.0,
            pinch_length: 20.0,
            heterogeneity: 0.5,
            clay_factor: 0.35,
            flood_rate: FLOOD_RATE,
            breach_dist: BREACH_DIST,
            chute_ratio: CHUTE_RATIO,
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
        "scar_lifetime",
        "jitter",
        "jitter_scale",
        "valley_width",
        "edge_margin",
        "pinch_width",
        "pinch_length",
        "heterogeneity",
        "clay_factor",
        "flood_rate",
        "breach_dist",
        "chute_ratio",
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
            "scar_lifetime" => &mut self.scar_lifetime,
            "jitter" => &mut self.jitter,
            "jitter_scale" => &mut self.jitter_scale,
            "valley_width" => &mut self.valley_width,
            "edge_margin" => &mut self.edge_margin,
            "pinch_width" => &mut self.pinch_width,
            "pinch_length" => &mut self.pinch_length,
            "heterogeneity" => &mut self.heterogeneity,
            "clay_factor" => &mut self.clay_factor,
            "flood_rate" => &mut self.flood_rate,
            "breach_dist" => &mut self.breach_dist,
            "chute_ratio" => &mut self.chute_ratio,
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
}

pub struct Sim {
    pub params: Params,
    /// Domain is `[0, bounds[0]] × [0, bounds[1]]`.
    pub bounds: P,
    /// Positions along x where the valley narrows to `pinch_width`.
    pub pinches: Vec<f64>,
    pub centerline: Vec<P>,
    pub oxbows: Vec<Oxbow>,
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

/// Steps during which a cut suppresses further cutoffs around it.
const CUT_COOLDOWN: u64 = 100;

/// Default flood parameters; see [`Params`].
const FLOOD_RATE: f64 = 0.02;
const BREACH_DIST: f64 = 2.5;
const CHUTE_RATIO: f64 = 4.0;

/// Longest chute a flood can cut, in channel widths.
pub const CHUTE_CHORD: f64 = 5.0;

/// Oxbow pieces with fewer points than this are dropped.
const MIN_OXBOW_POINTS: usize = 3;

/// Most cutoffs a single flood can make.
const MAX_FLOOD_CUTS: usize = 4;

/// Grid cell of a point for a hashed grid of cell size `size`.
fn grid_cell(p: P, size: f64) -> (i64, i64) {
    ((p[0] / size).floor() as i64, (p[1] / size).floor() as i64)
}

/// Bucket of a grid cell in a hash table of `mask + 1` buckets.
fn grid_bucket(cx: i64, cy: i64, mask: usize) -> usize {
    ((cx.wrapping_mul(73_856_093) ^ cy.wrapping_mul(19_349_663)) as usize) & mask
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
        self.erodibility = Erodibility::new(self.bounds, w, NOISE_SCALE * w, seed);
        self.oxbows.clear();
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
        self.centerline = resample(&self.centerline, ds);
        self.migrate();
        self.smooth();
        self.clamp_to_domain();
        if self.cutoff() {
            self.centerline = resample(&self.centerline, ds);
        }
        // Floods arrive as a Poisson process, approximated per step.
        if self.params.flood_rate > 0.0 && self.rng.uniform() < self.params.flood_rate * self.params.dt {
            self.floods += 1;
            let magnitude = self.rng.uniform();
            for _ in 0..MAX_FLOOD_CUTS {
                if !self.flood_cutoff(magnitude) {
                    break;
                }
                self.centerline = resample(&self.centerline, ds);
            }
        }
        self.time += self.params.dt;
        self.steps += 1;
        self.rework_oxbows();
        let (time, life) = (self.time, self.params.oxbow_lifetime + self.params.scar_lifetime);
        self.oxbows.retain(|o| time - o.born < life);
        let steps = self.steps;
        self.recent_cuts.retain(|&(_, until)| until > steps);
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
            let e = edge_taper(pts[i], self.bounds, p.edge_margin * p.width, valley)
                * self.erodibility.at(pts[i], p.heterogeneity, p.clay_factor);
            // Positive rate moves toward the outside of a left bend, i.e. to
            // the right, so bends grow.
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
    /// `size`. The table has about 2n buckets and is filled by counting
    /// sort; collisions only add candidates, which callers reject by
    /// distance. Returns the bucket mask.
    fn build_grid(&mut self, size: f64) -> usize {
        let pts = &self.centerline;
        let n = pts.len();
        let buckets = (2 * n).next_power_of_two();
        let mask = buckets - 1;
        self.grid_start.clear();
        self.grid_start.resize(buckets + 1, 0);
        self.cell_of.clear();
        for &p in pts {
            let (cx, cy) = grid_cell(p, size);
            let b = grid_bucket(cx, cy, mask);
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
        mask
    }

    /// Call `f` with every point index in the 3 × 3 block of grid cells
    /// around `p`, which covers all points within `size` of it.
    fn for_neighbours(&self, p: P, size: f64, mask: usize, mut f: impl FnMut(usize)) {
        let (cx, cy) = grid_cell(p, size);
        for y in cy - 1..=cy + 1 {
            for x in cx - 1..=cx + 1 {
                let b = grid_bucket(x, y, mask);
                let (s, e) = (self.grid_start[b] as usize, self.grid_start[b + 1] as usize);
                for &j in &self.grid_items[s..e] {
                    f(j as usize);
                }
            }
        }
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
        let mask = self.build_grid(d_cut);
        let d_cut2 = d_cut * d_cut;
        let pts = &self.centerline;

        // Smallest upstream index first, then the smallest loop from it.
        for i in 0..n {
            let mut best: Option<usize> = None;
            self.for_neighbours(pts[i], d_cut, mask, |j| {
                if j > i + k_sep && best.is_none_or(|b| j < b) && geom::dist2(pts[i], pts[j]) < d_cut2 {
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
        let mask = self.build_grid(reach);
        let (neck2, reach2) = (neck * neck, reach * reach);
        let pts = &self.centerline;

        // Best pair from each upstream point, as (ratio, i, j).
        let mut found: Vec<(f64, usize, usize)> = Vec::new();
        for i in 0..n {
            let mut best: Option<(f64, usize)> = None;
            self.for_neighbours(pts[i], reach, mask, |j| {
                if j <= i + k_sep {
                    return;
                }
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

    /// Remove the parts of oxbows that the channel now overlaps, splitting
    /// an oxbow where the channel crosses it, and drop pieces too short to
    /// draw. A piece keeps its parent's birth time.
    fn rework_oxbows(&mut self) {
        if self.oxbows.is_empty() {
            return;
        }
        let w = self.params.width;
        let mask = self.build_grid(w);
        let oxbows = std::mem::take(&mut self.oxbows);
        let mut kept = Vec::with_capacity(oxbows.len());
        let pts = &self.centerline;
        let mut hit = Vec::new();
        for o in oxbows {
            hit.clear();
            hit.extend(o.points.iter().map(|&q| {
                let mut near = false;
                self.for_neighbours(q, w, mask, |j| near |= geom::dist2(q, pts[j]) < w * w);
                near
            }));
            if !hit.contains(&true) {
                kept.push(o);
                continue;
            }
            let mut start = 0;
            for k in 0..=hit.len() {
                if k == hit.len() || hit[k] {
                    if k - start >= MIN_OXBOW_POINTS {
                        kept.push(Oxbow { points: o.points[start..k].to_vec(), ..o });
                    }
                    start = k + 1;
                }
            }
        }
        self.oxbows = kept;
    }

    /// Join point `i` to point `j` and abandon the reach between them as an
    /// oxbow.
    fn apply_cut(&mut self, i: usize, j: usize) {
        let loop_pts = self.centerline[i..=j].to_vec();
        let mid = geom::lerp(self.centerline[i], self.centerline[j], 0.5);
        self.centerline.drain(i + 1..j);
        self.erodibility.stamp_clay(&loop_pts, 0.5 * self.params.width);
        let length = geom::length(&loop_pts);
        self.oxbows.push(Oxbow { points: loop_pts, born: self.time, length });
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

    /// Mean sinuosity over the second half of a long run.
    fn late_sinuosity(params: Params, seed: u64) -> (f64, Sim) {
        let mut sim = Sim::new(params, [1200.0, 480.0], seed, 0.5);
        let mut sum = 0.0;
        for k in 0..300 {
            sim.step_n(100);
            if k >= 150 {
                sum += sim.sinuosity();
            }
            assert!(!geom::self_intersects(&sim.centerline), "self-intersection at step {}", sim.steps);
        }
        (sum / 150.0, sim)
    }

    #[test]
    fn floods_cut_off_earlier_and_never_self_intersect() {
        let calm = Params { flood_rate: 0.0, ..Params::default() };
        let stormy = Params { flood_rate: 0.1, chute_ratio: 3.0, ..Params::default() };
        for seed in [3, 11] {
            let (s_calm, calm_sim) = late_sinuosity(calm.clone(), seed);
            let (s_storm, sim) = late_sinuosity(stormy.clone(), seed);
            eprintln!(
                "seed {seed}: sinuosity {s_calm:.2} calm, {s_storm:.2} with {} floods and {} flood cutoffs of {}",
                sim.floods, sim.flood_cutoffs, sim.cutoffs
            );
            assert_eq!(calm_sim.flood_cutoffs, 0);
            assert!(sim.flood_cutoffs >= 3, "only {} flood cutoffs", sim.flood_cutoffs);
            assert!(s_storm < s_calm, "floods should keep the river straighter");
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
    fn channel_reworks_oxbows_it_crosses() {
        let mut sim = sine_sim(0.0, 40.0);
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        // A vertical oxbow across the channel, and one well clear of it.
        let across: Vec<P> = (-20..=20).map(|k| [100.0 * w, mid + 0.5 * k as f64 * w]).collect();
        let clear: Vec<P> = (0..20).map(|k| [(150 + k) as f64 * w, mid + 20.0 * w]).collect();
        sim.oxbows.push(Oxbow { points: across, born: 0.0, length: 20.0 * w });
        sim.oxbows.push(Oxbow { points: clear.clone(), born: 0.0, length: 9.5 * w });
        sim.step();
        assert_eq!(sim.oxbows.len(), 3, "the crossed oxbow splits in two");
        assert_eq!(sim.oxbows[2].points, clear);
        for o in &sim.oxbows {
            for &q in &o.points {
                assert!(sim.centerline.iter().all(|&c| dist(q, c) >= w));
            }
        }
    }
}
