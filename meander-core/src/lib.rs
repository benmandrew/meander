//! Meandering river centreline simulation.
//!
//! The river is a polyline flowing from index 0 (upstream) to the last index
//! (downstream). Each step resamples it, migrates every interior point along
//! its normal at a rate given by the Howard–Knutson (1984) upstream-weighted
//! curvature kernel, smooths, and cuts off any loop whose neck has closed.

pub mod field;
pub mod geom;
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
    /// Oxbows are dropped after this much simulation time.
    pub oxbow_lifetime: f64,
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
    /// Strength of the erodibility noise: 0 is uniform, 1 ranges from
    /// near zero to double.
    pub heterogeneity: f64,
    /// Erodibility multiplier inside filled oxbows (clay plugs); 1 disables.
    pub clay_factor: f64,
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
            jitter: 0.05,
            jitter_scale: 3.0,
            valley_width: 45.0,
            edge_margin: 8.0,
            heterogeneity: 0.5,
            clay_factor: 0.35,
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
        "jitter",
        "jitter_scale",
        "valley_width",
        "edge_margin",
        "heterogeneity",
        "clay_factor",
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
            "jitter" => &mut self.jitter,
            "jitter_scale" => &mut self.jitter_scale,
            "valley_width" => &mut self.valley_width,
            "edge_margin" => &mut self.edge_margin,
            "heterogeneity" => &mut self.heterogeneity,
            "clay_factor" => &mut self.clay_factor,
            _ => return None,
        })
    }

    pub fn get(&self, name: &str) -> Option<f64> {
        if name == "cutoff_min_sep" {
            return Some(self.cutoff_min_sep as f64);
        }
        self.clone().field(name).map(|v| *v)
    }

    /// Set a parameter by name. Returns false for an unknown name.
    pub fn set(&mut self, name: &str, value: f64) -> bool {
        if name == "cutoff_min_sep" {
            self.cutoff_min_sep = value.max(2.0) as usize;
            return true;
        }
        match self.field(name) {
            Some(v) => {
                *v = value;
                true
            }
            None => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Oxbow {
    pub points: Vec<P>,
    pub born: f64,
}

pub struct Sim {
    pub params: Params,
    /// Domain is `[0, bounds[0]] × [0, bounds[1]]`.
    pub bounds: P,
    pub centerline: Vec<P>,
    pub oxbows: Vec<Oxbow>,
    pub erodibility: Erodibility,
    pub time: f64,
    pub steps: u64,
    pub cutoffs: u64,
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

/// Erodibility multiplier that falls smoothly from 1 to 0 within `margin` of
/// the domain edges, and within `margin` of the valley walls when
/// `valley` (the valley width) is non-zero.
fn edge_taper(p: P, bounds: P, margin: f64, valley: f64) -> f64 {
    if margin <= 0.0 {
        return 1.0;
    }
    let ramp = |d: f64| {
        let t = (d / margin).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let (mut y0, mut y1) = (0.0, bounds[1]);
    if valley > 0.0 {
        let mid = 0.5 * bounds[1];
        y0 = (mid - 0.5 * valley).max(0.0);
        y1 = (mid + 0.5 * valley).min(bounds[1]);
    }
    ramp(p[0]) * ramp(bounds[0] - p[0]) * ramp(p[1] - y0) * ramp(y1 - p[1])
}

/// Feature size of the erodibility noise, in channel widths at reset.
const NOISE_SCALE: f64 = 25.0;

/// Steps during which a cut suppresses further cutoffs around it.
const CUT_COOLDOWN: u64 = 100;

impl Sim {
    pub fn new(params: Params, bounds: P, seed: u64, noise: f64) -> Self {
        let mut sim = Self {
            params,
            bounds,
            centerline: Vec::new(),
            oxbows: Vec::new(),
            erodibility: Erodibility::new([1.0, 1.0], 1.0, 1.0, 0),
            time: 0.0,
            steps: 0,
            cutoffs: 0,
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
        self.time += self.params.dt;
        self.steps += 1;
        let (time, life) = (self.time, self.params.oxbow_lifetime);
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
            let e = edge_taper(pts[i], self.bounds, p.edge_margin * p.width, p.valley_width * p.width)
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

    /// Find one neck cutoff and apply it. Returns whether a cut was made.
    fn cutoff(&mut self) -> bool {
        let d_cut = self.params.cutoff_dist * self.params.width;
        let k_sep = self.params.cutoff_min_sep;
        let pts = &self.centerline;
        let n = pts.len();
        if n < k_sep + 2 || d_cut <= 0.0 {
            return false;
        }

        // Spatial hash: grid cells of size d_cut, hashed into a table of
        // about 2n buckets and filled by counting sort. Collisions only add
        // candidates, which the distance test rejects.
        let buckets = (2 * n).next_power_of_two();
        let mask = buckets - 1;
        let cell = |p: P| -> (i64, i64) { ((p[0] / d_cut).floor() as i64, (p[1] / d_cut).floor() as i64) };
        let bucket = |cx: i64, cy: i64| -> usize {
            ((cx.wrapping_mul(73_856_093) ^ cy.wrapping_mul(19_349_663)) as usize) & mask
        };
        self.grid_start.clear();
        self.grid_start.resize(buckets + 1, 0);
        self.cell_of.clear();
        for &p in pts {
            let (cx, cy) = cell(p);
            let b = bucket(cx, cy);
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

        let d_cut2 = d_cut * d_cut;
        let cooldown_r = 0.5 * k_sep as f64 * self.ds();
        let near_recent = |p: P| self.recent_cuts.iter().any(|&(c, _)| dist(c, p) < cooldown_r);

        // Smallest upstream index first, then the smallest loop from it.
        let mut found = None;
        'outer: for i in 0..n {
            let (cx, cy) = cell(pts[i]);
            let mut best: Option<usize> = None;
            for y in cy - 1..=cy + 1 {
                for x in cx - 1..=cx + 1 {
                    let b = bucket(x, y);
                    let (s, e) = (self.grid_start[b] as usize, self.grid_start[b + 1] as usize);
                    for &j in &self.grid_items[s..e] {
                        let j = j as usize;
                        if j > i + k_sep
                            && best.is_none_or(|b| j < b)
                            && geom::dist2(pts[i], pts[j]) < d_cut2
                        {
                            best = Some(j);
                        }
                    }
                }
            }
            if let Some(j) = best {
                if near_recent(pts[i]) || near_recent(pts[j]) {
                    continue;
                }
                found = Some((i, j));
                break 'outer;
            }
        }

        let Some((i, j)) = found else { return false };
        let loop_pts = self.centerline[i..=j].to_vec();
        let mid = geom::lerp(self.centerline[i], self.centerline[j], 0.5);
        self.centerline.drain(i + 1..j);
        self.erodibility.stamp_clay(&loop_pts, 0.5 * self.params.width);
        self.oxbows.push(Oxbow { points: loop_pts, born: self.time });
        self.recent_cuts.push((mid, self.steps + CUT_COOLDOWN));
        self.cutoffs += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Straight channel along x with a sine perturbation, cutoffs off.
    fn sine_sim(amp_w: f64, wavelength_w: f64) -> Sim {
        let params = Params { cutoff_dist: 0.0, jitter: 0.0, heterogeneity: 0.0, ..Params::default() };
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
}
