//! Tributaries and their fans. A combe deep enough carries a stream that
//! drains the upland behind it. Where the stream leaves the combe it drops
//! its gravel as a fan, a low cone of coarse ground that the river erodes
//! slowly and that builds back over what the river trims. The stream's
//! bedload feeds the river's bars for a reach below the confluence, so the
//! bends there migrate faster.

use crate::geom::{self, P};
use crate::{FAN_HEIGHT, FAN_MAX_DEPTH, FAN_MIN_DEPTH, GROWTH_EVERY, JUNCTION_EVERY, Sim, TRIBUTARY_REACH};

/// A tributary entering from a combe.
#[derive(Clone, Debug)]
pub struct Tributary {
    /// The wall it comes down: 0 the low-y wall, 1 the high-y wall.
    pub side: usize,
    /// Where it leaves the combe: the apex of its fan.
    pub mouth: P,
    /// Radius of its fan, in world units.
    pub radius: f64,
    /// Its combe's depth over the deepest a combe can be, in (0, 1]: the
    /// share of the largest tributary's flow and sediment it carries.
    pub scale: f64,
    /// Centreline point it joins, refreshed every [`JUNCTION_EVERY`] steps.
    pub at: usize,
}

impl Sim {
    /// Set out a tributary in each combe at least [`FAN_MIN_DEPTH`] deep,
    /// with its fan already built: the fans are older than the river's
    /// present course.
    pub(crate) fn place_tributaries(&mut self) {
        self.tributaries.clear();
        let p = &self.params;
        if p.fan_radius <= 0.0 || p.valley_width <= 0.0 {
            return;
        }
        let w = p.width;
        let mut found = Vec::new();
        for side in 0..2 {
            for c in &self.walls.combes[side] {
                if c.depth < FAN_MIN_DEPTH {
                    continue;
                }
                let x = c.x * w;
                if x <= 0.0 || x >= self.bounds[0] {
                    continue;
                }
                let floor = self.floor(x, false);
                let scale = c.depth / FAN_MAX_DEPTH;
                found.push(Tributary {
                    side,
                    mouth: [x, if side == 0 { floor.0 } else { floor.1 }],
                    radius: (c.depth + p.fan_radius * scale) * w,
                    scale,
                    at: 0,
                });
            }
        }
        for t in &found {
            self.erodibility.grow_cone(t.mouth, t.radius, FAN_HEIGHT * t.scale, t.scale, 1.0);
        }
        self.tributaries = found;
        for k in 0..self.tributaries.len() {
            self.join_tributary(k);
        }
    }

    /// Find the centreline point nearest tributary `k`'s mouth.
    fn join_tributary(&mut self, k: usize) {
        let m = self.tributaries[k].mouth;
        let (at, _) = self.centerline.iter().enumerate().fold((0, f64::INFINITY), |best, (i, &q)| {
            let d = geom::dist2(q, m);
            if d < best.1 { (i, d) } else { best }
        });
        self.tributaries[k].at = at;
    }

    /// The tributaries' share of this step's work: those whose turn it is
    /// rejoin the channel, and build their fans back by the fraction of
    /// what the channel has trimmed that `fan_time` allows, quicker as the
    /// supply rises.
    pub(crate) fn feed_tributaries(&mut self) {
        let fan_time = self.params.fan_time;
        let frac = GROWTH_EVERY as f64 * self.params.dt * self.supply() / fan_time.max(1e-9);
        for k in 0..self.tributaries.len() {
            if (self.steps + k as u64) % JUNCTION_EVERY == 0 {
                self.join_tributary(k);
            }
            if fan_time > 0.0 && (self.steps + k as u64) % GROWTH_EVERY == 0 {
                let t = &self.tributaries[k];
                self.erodibility.grow_cone(t.mouth, t.radius, FAN_HEIGHT * t.scale, t.scale, frac);
            }
        }
    }

    /// Speed migration below each confluence by `tributary_boost` times
    /// the tributary's scale, falling to nothing [`TRIBUTARY_REACH`] widths
    /// downstream.
    pub(crate) fn feed_bars(&mut self) {
        let boost = self.params.tributary_boost;
        let (n, r) = (self.rate.len(), self.reach_points(TRIBUTARY_REACH));
        if boost <= 0.0 {
            return;
        }
        for t in &self.tributaries {
            for k in t.at..(t.at + r).min(n) {
                self.rate[k] *= 1.0 + boost * t.scale * (1.0 - (k - t.at) as f64 / r as f64);
            }
        }
    }
}
