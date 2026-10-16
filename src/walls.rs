//! The valley walls, where the floodplain ends on each side. The foot of
//! each wall wanders outwards from the edge of a valley `valley_width`
//! wide, as seeded value noise, and dry side valleys (combes) cut back into
//! it. A wall never comes inwards of that edge, so the floor is never
//! narrower than `valley_width`. Every length here is in channel widths, so
//! the walls follow later changes to `width` and `valley_width`.

use crate::rng::Rng;
use crate::smoothstep;

/// Greatest distance the foot of a wall wanders outwards.
const WANDER: f64 = 2.5;
/// Knot spacings of the wander's octaves along the valley, each weighted
/// 1 / (m + 1) for octave m.
const KNOTS: [f64; 2] = [30.0, 8.0];
/// Mean number of combes along 100 widths of each wall.
const COMBES: f64 = 3.0;
/// Ranges of a combe's depth at the foot, and of its half-width there.
const COMBE_DEPTH: [f64; 2] = [2.0, 4.5];
const COMBE_SPAN: [f64; 2] = [2.5, 5.0];
/// Spacing of the stored samples along the valley.
const STEP: f64 = 0.5;

/// A combe cut back into a wall, centred `x` along the valley.
#[derive(Clone, Copy, Debug)]
pub struct Combe {
    pub x: f64,
    pub depth: f64,
    pub span: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Walls {
    /// Distance of each wall's foot outwards of the valley edge, at samples
    /// `STEP` apart: the low-y wall, then the high-y wall.
    out: [Vec<f64>; 2],
    pub combes: [Vec<Combe>; 2],
}

impl Walls {
    /// Walls along a valley `length` long, drawn from their own stream of
    /// `seed`, so making them leaves the river's draws unchanged.
    pub fn new(length: f64, seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ 0xba1e_5ca1);
        let n = (length / STEP).ceil() as usize + 1;
        let total: f64 = (0..KNOTS.len()).map(|m| 1.0 / (m + 1) as f64).sum();
        let mut wall = || {
            let octaves: Vec<Vec<f64>> = KNOTS
                .iter()
                .map(|len| (0..(length / len).ceil() as usize + 2).map(|_| rng.uniform()).collect())
                .collect();
            let count = (COMBES * length / 100.0 + rng.uniform() - 0.5).round().max(0.0) as usize;
            let between = |[a, b]: [f64; 2], u: f64| a + (b - a) * u;
            let combes: Vec<Combe> = (0..count)
                .map(|_| Combe {
                    x: rng.uniform() * length,
                    depth: between(COMBE_DEPTH, rng.uniform()),
                    span: between(COMBE_SPAN, rng.uniform()),
                })
                .collect();
            let out = (0..n)
                .map(|k| {
                    let x = k as f64 * STEP;
                    let mut u = 0.0;
                    for (m, (len, knots)) in KNOTS.iter().zip(&octaves).enumerate() {
                        let t = x / len;
                        let j = t as usize;
                        u += (knots[j] + (knots[j + 1] - knots[j]) * smoothstep(t - j as f64)) / (m + 1) as f64;
                    }
                    WANDER * u / total + combe_cut(&combes, x, 1.0)
                })
                .collect();
            (out, combes)
        };
        let (low, low_combes) = wall();
        let (high, high_combes) = wall();
        Self { out: [low, high], combes: [low_combes, high_combes] }
    }

    /// Distance of the foot of wall `side` (0 low y, 1 high y) outwards of
    /// the valley edge at `x`.
    pub fn out(&self, side: usize, x: f64) -> f64 {
        let out = &self.out[side];
        if out.len() < 2 {
            return 0.0;
        }
        let t = (x / STEP).clamp(0.0, (out.len() - 1) as f64);
        let k = (t as usize).min(out.len() - 2);
        out[k] + (out[k + 1] - out[k]) * (t - k as f64)
    }
}

/// Depth the `combes` cut back at `x`, with their depths and spans scaled
/// by `scale`. Each is a smooth bump falling to 0 at its span, of the
/// profile (1 − t)²(1 + 2t). The renderer draws the brow with the same
/// profile at a larger scale.
pub fn combe_cut(combes: &[Combe], x: f64, scale: f64) -> f64 {
    combes.iter().fold(0.0, |m, c| {
        let t = (x - c.x).abs() / (scale * c.span);
        if t < 1.0 { m.max(scale * c.depth * (1.0 - t) * (1.0 - t) * (1.0 + 2.0 * t)) } else { m }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walls_wander_outwards_within_their_bounds() {
        let walls = Walls::new(300.0, 7);
        for side in 0..2 {
            let deepest = walls.combes[side].iter().fold(0.0f64, |m, c| m.max(c.depth));
            for k in 0..=600 {
                let out = walls.out(side, 0.5 * k as f64);
                assert!(out >= 0.0 && out <= WANDER + deepest + 1e-9, "side {side} at {k}: {out}");
            }
        }
        assert!(walls.combes.iter().map(Vec::len).sum::<usize>() > 0);
    }
}
