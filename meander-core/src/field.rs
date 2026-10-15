//! Spatially varying erodibility: smooth noise, ground the channel has
//! swept, which is loose point-bar sand until vegetation binds it, and oxbow
//! footprints that erode fast while open water and stiffen to clay plugs as
//! they fill. The same noise, with a finer layer added, gives the height of
//! the land.

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
    /// moves linearly from `g.lake` to `g.clay` over `g.lake_life`; on a
    /// point bar, from `g.bar` to 1 over `g.vegetation`.
    pub fn at(&self, p: P, g: &Ground) -> f64 {
        let i = self.index(p);
        let n = 1.0 + g.heterogeneity * 2.0 * (self.noise[i] as f64 - 0.5);
        let ramp = |from: f64, to: f64, since: f64, life: f64| {
            from + (to - from) * ((g.time - since) / life.max(1e-9)).clamp(0.0, 1.0)
        };
        let (cut, swept) = (self.cut_at[i], self.swept_at[i]);
        let c = if cut.is_finite() {
            ramp(g.lake, g.clay, cut, g.lake_life)
        } else if swept.is_finite() {
            ramp(g.bar, 1.0, swept, g.vegetation)
        } else {
            1.0
        };
        n.max(0.05) * c
    }

    /// Mark the cell under each point as swept by the channel at `time`. The
    /// channel replaces any oxbow fill there with fresh point-bar sand.
    pub fn sweep(&mut self, pts: &[P], time: f64) {
        for &p in pts {
            let i = self.index(p);
            self.swept_at[i] = time;
            self.cut_at[i] = f64::INFINITY;
        }
    }

    /// Mark cells within `radius` of the points as cut off at `time`.
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
