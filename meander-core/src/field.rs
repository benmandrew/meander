//! Spatially varying erodibility: smooth noise, ground the channel has
//! swept, which is loose point-bar sand until vegetation binds it, and oxbow
//! footprints that erode fast while open water and stiffen to clay plugs as
//! they fill. The same noise, with a finer layer added, gives the height of
//! the land. Floods lay overbank deposits on top, thickest beside the
//! channel as natural levees, and a higher bank erodes more slowly.

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

/// Distance from the bank, in levee lengths, beyond which a flood lays no
/// deposit; exp(−4) is under 2% of the deposit at the bank.
const LEVEE_REACH: f64 = 4.0;

fn lattice(seed: u64, x: i64, y: i64) -> f64 {
    let mut h = seed ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

fn value_noise(seed: u64, x: f64, y: f64) -> f64 {
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
    /// sand. It moves to 1, the old floodplain, over `vegetation`.
    pub bar: f64,
    /// Time vegetation takes to bind a point bar.
    pub vegetation: f64,
}

/// Erodibility multiplier on a grid. Built from noise at reset. Cells the
/// channel crosses are stamped with the time it last swept them, and their
/// multiplier moves from the bar factor to 1 as vegetation binds the sand.
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
    /// Time the channel last swept each cell, or minus infinity.
    pub swept_at: Vec<f64>,
    /// Height of overbank deposits above fresh point-bar sand, in channel
    /// depths. The channel resets it to 0 where it sweeps, and a cutoff
    /// sets it to −`LAKE_DEPTH` over the oxbow footprint.
    pub deposit: Vec<f32>,
    /// Scratch: squared distance from each cell to the channel during a
    /// flood.
    near: Vec<f32>,
    seed: u64,
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
            swept_at: vec![f64::NEG_INFINITY; cells],
            deposit: vec![0.0; cells],
            near: Vec::new(),
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
    /// faster as floods silt the lake up (see [`Erodibility::silt_at`]); on a
    /// point bar, from `g.bar` to 1 over `g.vegetation`. A bank raised by
    /// overbank deposits of height h, in channel depths, holds 1 + h times
    /// the material of a bank at bar level, so it erodes 1 / (1 + h) as
    /// fast.
    pub fn at(&self, p: P, g: &Ground) -> f64 {
        let i = self.index(p);
        let n = 1.0 + g.heterogeneity * 2.0 * (self.noise[i] as f64 - 0.5);
        let since = |t: f64, life: f64| ((g.time - t) / life.max(1e-9)).clamp(0.0, 1.0);
        let (cut, swept) = (self.cut_at[i], self.swept_at[i]);
        let c = if cut.is_finite() {
            let silt = (1.0 + self.deposit[i] as f64 / LAKE_DEPTH).clamp(0.0, 1.0);
            g.lake + (g.clay - g.lake) * since(cut, g.lake_life).max(silt)
        } else if swept.is_finite() {
            g.bar + (1.0 - g.bar) * since(swept, g.vegetation)
        } else {
            1.0
        };
        n.max(0.05) * c / (1.0 + (self.deposit[i] as f64).max(0.0))
    }

    /// Overbank deposit at `p`, in channel depths.
    pub fn deposit_at(&self, p: P) -> f64 {
        self.deposit[self.index(p)] as f64
    }

    /// Fraction of a fresh oxbow lake's depth that floods have filled with
    /// silt at `p`, in [0, 1]: 1 once the bed reaches the level of fresh
    /// point-bar sand.
    pub fn silt_at(&self, p: P) -> f64 {
        (1.0 + self.deposit_at(p) / LAKE_DEPTH).clamp(0.0, 1.0)
    }

    /// Lay one flood's overbank deposit beside a channel of half-width
    /// `half` along the points. A cell at distance d from the bank gains
    /// `amount · exp(−d / length) · (1 − h / stage)`, so deposits thin away
    /// from the channel into natural levees and stop growing as the ground
    /// nears the flood level `stage`. The factor is the depth of flood water
    /// over the ground, so a lake bed `LAKE_DEPTH` below fresh bar level
    /// traps 1 + `LAKE_DEPTH / stage` times as much. Cells under the channel
    /// gain nothing.
    pub fn flood(&mut self, pts: &[P], half: f64, amount: f64, length: f64, stage: f64) {
        if amount <= 0.0 || length <= 0.0 || stage <= 0.0 {
            return;
        }
        let c = self.cell;
        let reach = half + LEVEE_REACH * length;
        let r2 = (reach * reach) as f32;
        self.near.clear();
        self.near.resize(self.deposit.len(), f32::INFINITY);
        for p in pts {
            let x0 = ((p[0] - reach) / c).floor().max(0.0) as usize;
            let y0 = ((p[1] - reach) / c).floor().max(0.0) as usize;
            let x1 = (((p[0] + reach) / c) as usize).min(self.cols - 1);
            let y1 = (((p[1] + reach) / c) as usize).min(self.rows - 1);
            for y in y0..=y1 {
                let dy = ((y as f64 + 0.5) * c - p[1]) as f32;
                let row = y * self.cols;
                for x in x0..=x1 {
                    let dx = ((x as f64 + 0.5) * c - p[0]) as f32;
                    let d2 = dx * dx + dy * dy;
                    let n = &mut self.near[row + x];
                    if d2 < *n && d2 < r2 {
                        *n = d2;
                    }
                }
            }
        }
        for (h, &d2) in self.deposit.iter_mut().zip(&self.near) {
            let d = (d2 as f64).sqrt();
            if d.is_finite() && d >= half {
                let room = (1.0 - *h as f64 / stage).max(0.0);
                *h += (amount * (-(d - half) / length).exp() * room) as f32;
            }
        }
    }

    /// Mark the cell under each point as swept by the channel at `time`. The
    /// channel replaces any oxbow fill there with fresh point-bar sand.
    pub fn sweep(&mut self, pts: &[P], time: f64) {
        for &p in pts {
            let i = self.index(p);
            self.swept_at[i] = time;
            self.cut_at[i] = f64::INFINITY;
            self.deposit[i] = 0.0;
        }
    }

    /// Lay a crevasse splay: a fan of sand from `apex` along the unit vector
    /// `dir`, out to `length`, spreading `spread` across per unit along. It
    /// is `thickness` channel depths deep at the apex and thins to nothing
    /// at its rim. Dry ground it covers becomes fresh sand swept at `time`;
    /// in a lake the sand fills the bed instead.
    pub fn stamp_splay(&mut self, apex: P, dir: P, length: f64, spread: f64, thickness: f64, time: f64) {
        let c = self.cell;
        let x0 = ((apex[0] - length) / c).floor().max(0.0) as usize;
        let y0 = ((apex[1] - length) / c).floor().max(0.0) as usize;
        let x1 = (((apex[0] + length).max(0.0) / c) as usize).min(self.cols - 1);
        let y1 = (((apex[1] + length).max(0.0) / c) as usize).min(self.rows - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let v = [(x as f64 + 0.5) * c - apex[0], (y as f64 + 0.5) * c - apex[1]];
                let along = v[0] * dir[0] + v[1] * dir[1];
                let across = (v[0] * dir[1] - v[1] * dir[0]).abs();
                let r2 = v[0] * v[0] + v[1] * v[1];
                if along <= 0.0 || across > spread * along || r2 >= length * length {
                    continue;
                }
                let i = y * self.cols + x;
                self.deposit[i] += (thickness * (1.0 - r2.sqrt() / length)) as f32;
                if self.cut_at[i].is_infinite() {
                    self.swept_at[i] = time;
                }
            }
        }
    }

    /// Mark cells within `radius` of the points as cut off at `time`, with
    /// the bed `LAKE_DEPTH` below fresh bar level.
    pub fn stamp_oxbow(&mut self, pts: &[P], radius: f64, time: f64) {
        let c = self.cell;
        let r2 = radius * radius;
        for p in pts {
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
        let h: Vec<f64> = (0..400).map(|k| e.height([k as f64 * 0.25, 50.0])).collect();
        assert!(h.iter().all(|v| (0.0..=1.0).contains(v)));
        let (lo, hi) = h.iter().fold((1.0f64, 0.0f64), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(hi - lo > 0.1, "range {lo}..{hi}");
    }

    fn ground(time: f64) -> Ground {
        Ground { time, heterogeneity: 0.0, lake_life: 100.0, lake: 3.0, clay: 0.3, bar: 2.0, vegetation: 50.0 }
    }

    #[test]
    fn oxbow_erodes_fast_then_stiffens_to_clay() {
        let mut e = Erodibility::new([10.0, 10.0], 1.0, 5.0, 0);
        let at = |e: &Erodibility, time| e.at([5.0, 5.0], &ground(time));
        assert_eq!(at(&e, 10.0), 1.0);
        e.stamp_oxbow(&[[5.0, 5.0]], 1.0, 10.0);
        assert!((at(&e, 10.0) - 3.0).abs() < 1e-12);
        assert!((at(&e, 60.0) - 1.65).abs() < 1e-12);
        assert!((at(&e, 110.0) - 0.3).abs() < 1e-12);
        assert!((at(&e, 1000.0) - 0.3).abs() < 1e-12);
        assert_eq!(e.at([1.0, 1.0], &ground(10.0)), 1.0);
    }

    #[test]
    fn floods_build_levees_that_slow_erosion() {
        let mut e = Erodibility::new([40.0, 40.0], 0.5, 5.0, 0);
        let channel: Vec<P> = (0..80).map(|k| [k as f64 * 0.5, 20.0]).collect();
        let h = |e: &Erodibility, y: f64| e.deposit_at([20.0, y]);
        e.flood(&channel, 1.0, 0.3, 2.0, 1.0);
        assert_eq!(h(&e, 20.25), 0.0, "nothing settles under the channel");
        assert!(h(&e, 21.25) > 0.25, "bank {}", h(&e, 21.25));
        assert!(h(&e, 21.25) > 2.0 * h(&e, 23.25) && h(&e, 23.25) > 2.0 * h(&e, 25.25));
        assert_eq!(h(&e, 38.0), 0.0, "beyond the reach");
        for _ in 0..200 {
            e.flood(&channel, 1.0, 0.3, 2.0, 1.0);
        }
        assert!(h(&e, 21.25) <= 1.0 && h(&e, 21.25) > 0.95, "levee {}", h(&e, 21.25));
        let at = |e: &Erodibility, y| e.at([20.0, y], &ground(0.0));
        assert!((at(&e, 21.25) - 1.0 / (1.0 + h(&e, 21.25))).abs() < 1e-9);
        e.sweep(&[[20.0, 21.25]], 0.0);
        assert_eq!(h(&e, 21.25), 0.0, "the channel clears the levee it crosses");
    }

    #[test]
    fn floods_silt_up_lakes() {
        let mut e = Erodibility::new([40.0, 40.0], 0.5, 5.0, 0);
        let channel: Vec<P> = (0..80).map(|k| [k as f64 * 0.5, 20.0]).collect();
        e.stamp_oxbow(&[[20.0, 22.25]], 0.25, 0.0);
        assert_eq!(e.silt_at([20.0, 22.25]), 0.0);
        let at = |e: &Erodibility| e.at([20.0, 22.25], &ground(0.0));
        assert!((at(&e) - 3.0).abs() < 1e-9, "open water");
        e.flood(&channel, 1.0, 0.3, 2.0, 0.25);
        let silt = e.silt_at([20.0, 22.25]);
        assert!(silt > 0.3 && silt < 1.0, "silt {silt}");
        assert!((at(&e) - (3.0 - 2.7 * silt)).abs() < 1e-6, "stiffens with the silt");
        for _ in 0..50 {
            e.flood(&channel, 1.0, 0.3, 2.0, 0.25);
        }
        assert_eq!(e.silt_at([20.0, 22.25]), 1.0);
        assert!(e.deposit_at([20.0, 22.25]) <= 0.25, "fills no higher than the flood level");
    }

    #[test]
    fn a_splay_spreads_thinning_sand_in_a_fan() {
        let mut e = Erodibility::new([40.0, 40.0], 0.5, 5.0, 0);
        e.stamp_oxbow(&[[20.0, 32.0]], 1.0, 0.0);
        e.stamp_splay([20.0, 20.0], [0.0, 1.0], 15.0, 0.7, 0.25, 5.0);
        let h = |p: P| e.deposit_at(p);
        assert!(h([20.0, 22.0]) > h([20.0, 28.0]) && h([20.0, 28.0]) > 0.0, "thins outwards");
        assert!(h([24.0, 28.0]) > 0.0 && h([28.0, 24.0]) == 0.0, "only inside the fan");
        assert_eq!(h([20.0, 18.0]), 0.0, "nothing behind the apex");
        assert!((e.at([20.0, 22.0], &ground(5.0)) - 2.0 / (1.0 + h([20.0, 22.0]))).abs() < 1e-6, "loose sand");
        assert!(e.silt_at([20.0, 32.0]) > 0.0 && e.cut_at[e.index([20.0, 32.0])] == 0.0, "a lake silts up");
    }

    #[test]
    fn swept_ground_is_loose_until_vegetated() {
        let mut e = Erodibility::new([10.0, 10.0], 1.0, 5.0, 0);
        e.stamp_oxbow(&[[5.0, 5.0]], 1.0, 10.0);
        e.sweep(&[[5.5, 5.5]], 10.0);
        let at = |time| e.at([5.5, 5.5], &ground(time));
        assert!((at(10.0) - 2.0).abs() < 1e-12, "the channel replaces the oxbow fill with sand");
        assert!((at(35.0) - 1.5).abs() < 1e-12);
        assert!((at(100.0) - 1.0).abs() < 1e-12);
        assert!((e.at([4.5, 5.5], &ground(10.0)) - 3.0).abs() < 1e-12, "only the swept cell changes");
    }
}
