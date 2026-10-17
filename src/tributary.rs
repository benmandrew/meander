//! Tributaries and their fans. A combe deep enough carries a stream that
//! drains the upland behind it. Where the stream leaves the combe it drops
//! its gravel as a fan, a low cone of coarse ground that the river erodes
//! slowly and that builds back over what the river trims. The stream's
//! bedload feeds the river's bars for a reach below the confluence, so the
//! bends there migrate faster.
//!
//! Across the floodplain a stream keeps to low ground: old channels, lake
//! beds and the back swamp behind the levees, leaning downstream with the
//! valley. Its course stays where it is until water meets it, when it ends
//! there, or leaves it, when it runs on from its end to the nearest water
//! by the same rule. A stream that ends in a lake builds a delta out into
//! it, which fills the lake and pushes the stream's end on across it, and
//! its bedload stays in the lake. Now and then a stream leaves its course
//! on its fan for a new one.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::field::{LAKE_DEPTH, lattice};
use crate::geom::{self, P};
use crate::{
    Cells, FAN_HEIGHT, FAN_MAX_DEPTH, FAN_MIN_DEPTH, GROWTH_EVERY, JUNCTION_EVERY, Sim,
    TRIBUTARY_REACH,
};

/// Spacing of the points of a stream's course, in channel widths.
const STREAM_SPACING: f64 = 0.5;
/// Side of the blocks of erodibility cells a stream's route is found on:
/// one channel width.
const STREAM_BLOCK: usize = 2;
/// Distance up or down the valley from its start a route may reach, in
/// channel widths.
const STREAM_REACH: f64 = 30.0;
/// Cost of a route per width of ground at the level of a fresh lake bed,
/// the rise in cost per channel depth of ground above that, and the most
/// the ground's noise adds, which bends routes across level ground.
const STREAM_BASE: f32 = 0.3;
const STREAM_RISE: f32 = 1.0;
const STREAM_NOISE: f32 = 0.8;
/// Most a block's own relief adds to its cost, below the scale of the
/// ground's noise, so a route wanders as a small stream does.
const STREAM_JITTER: f32 = 0.6;
/// Seed of that relief, mixed with the ground's.
const JITTER_SEED: u64 = 0x3d1f_9a07;
/// Points of a course either side of the join with a new route that are
/// smoothed into it.
const STREAM_JOIN: usize = 6;
/// Factor on the cost of a step up the valley, against its slope.
const STREAM_UPSTREAM: f32 = 1.6;
/// Factor on the cost of the ground under a course the stream moves
/// off: the old course has built up its bed.
const STREAM_ABANDON: f32 = 4.0;
/// Smoothing passes over a route.
const STREAM_SMOOTH: usize = 16;
/// Longest course, in valley widths; a longer one is routed again from the
/// mouth.
const STREAM_LONGEST: f64 = 3.0;
/// Radius of a tributary's delta, in channel widths.
const DELTA_RADIUS: f64 = 2.5;
/// A lake is open water to a stream until it is this full.
const LAKE_OPEN: f64 = 0.8;
/// Fraction of its length a lake loses from each end as it fills, as
/// drawn.
const LAKE_PLUG: f64 = 0.4;
/// Silt over the lake's mean, as a fraction of its unfilled depth, at
/// which a delta has built its bed out of the water.
const DELTA_DRY: f64 = 0.5;

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
    /// Its course from the mouth to the first water it meets, at about
    /// [`STREAM_SPACING`] widths.
    pub course: Vec<P>,
    /// Centreline point nearest the end of its course, refreshed every
    /// [`JUNCTION_EVERY`] steps.
    pub at: usize,
    /// Whether its course ends in a lake, which takes its bedload.
    pub lake: bool,
    /// Counts changes to `course`.
    pub version: u64,
}

/// Water a stream can end in.
#[derive(Clone, Copy)]
enum Wet {
    /// The channel, at a centreline point.
    Channel(usize),
    Lake,
}

/// Open water in lakes: points along the lakes with the radius of water
/// about them.
type LakeWater = Vec<(P, f64)>;

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
                    course: Vec::new(),
                    at: 0,
                    lake: false,
                    version: 0,
                });
            }
        }
        for t in &found {
            self.erodibility
                .grow_cone(t.mouth, t.radius, FAN_HEIGHT * t.scale, t.scale, 1.0);
        }
        self.tributaries = found;
        for k in 0..self.tributaries.len() {
            self.join_tributary(k);
        }
    }

    /// Walk tributary `k`'s course from its mouth and end it at the first
    /// water it meets. A course that ends dry runs on from its end along a
    /// new route. A stream in a lake builds its delta.
    fn join_tributary(&mut self, k: usize) {
        let w = self.params.width;
        let g = self.build_grid(w);
        let mut course = std::mem::take(&mut self.tributaries[k].course);
        let mouth = self.tributaries[k].mouth;
        let longest = STREAM_LONGEST * self.params.valley_width / STREAM_SPACING;
        if course.is_empty() || course.len() as f64 > longest {
            course = vec![mouth];
            self.tributaries[k].version += 1;
        }
        let (lo, hi) = course
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                (a.min(q[0]), b.max(q[0]))
            });
        let reach = STREAM_REACH * w;
        let lakes = self.lake_water(lo - reach, hi + reach);
        let mut wet = None;
        for (i, &q) in course.iter().enumerate() {
            if let Some(x) = self.wet_at(q, g, &lakes) {
                if i + 1 < course.len() {
                    course.truncate(i + 1);
                    self.tributaries[k].version += 1;
                }
                wet = Some(x);
                break;
            }
        }
        if wet.is_none() {
            let tip = *course.last().unwrap();
            if let Some((route, x)) = self.stream_route(tip, &lakes, &[]) {
                join(&mut course, &route);
                self.tributaries[k].version += 1;
                wet = Some(x);
            }
        }
        let end = *course.last().unwrap();
        let p = &self.params;
        let amount = JUNCTION_EVERY as f64 * p.dt * self.supply() * p.delta_rate * LAKE_DEPTH;
        let t = &mut self.tributaries[k];
        t.course = course;
        match wet {
            Some(Wet::Channel(j)) => {
                t.at = j;
                t.lake = false;
            }
            Some(Wet::Lake) => {
                t.lake = true;
                self.erodibility
                    .delta(end, DELTA_RADIUS * w, amount * t.scale);
            }
            // Out of reach of water for now: it keeps its last confluence.
            None => {}
        }
    }

    /// Whether `q` lies in the channel, within half the local width of a
    /// centreline point, or in the open water of a lake in `lakes`.
    fn wet_at(&self, q: P, g: Cells, lakes: &LakeWater) -> Option<Wet> {
        let (w, pts) = (self.params.width, &self.centerline);
        let (cx, cy) = g.cell(q);
        let mut best: Option<(f64, usize)> = None;
        for y in cy - 1..=cy + 1 {
            for part in self.row(g, cx, y) {
                for &j in part {
                    let j = j as usize;
                    let (d, half) = (geom::dist2(q, pts[j]), 0.5 * self.widths[j] * w);
                    if d < half * half && best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, j));
                    }
                }
            }
        }
        if let Some((_, j)) = best {
            return Some(Wet::Channel(j));
        }
        lakes
            .iter()
            .any(|&(c, r)| geom::dist2(q, c) < r * r)
            .then_some(Wet::Lake)
    }

    /// The points of open lake water between `x0` and `x1`, each with the
    /// radius of water about it. A lake is open until it is [`LAKE_OPEN`]
    /// full; it narrows as it fills, plugs from its ends as drawn, and is
    /// dry where a delta has built its bed well above the lake's mean.
    fn lake_water(&self, x0: f64, x1: f64) -> LakeWater {
        let (w, field) = (self.params.width, &self.erodibility);
        let mut out = Vec::new();
        for o in self
            .oxbows
            .iter()
            .filter(|o| o.is_lake() && o.fill < LAKE_OPEN)
        {
            if !o.points.iter().any(|q| (x0..=x1).contains(&q[0])) {
                continue;
            }
            let mean = field.lake_silt(&o.points);
            let plug = LAKE_PLUG * o.fill * o.length;
            let open = 1.0 - o.fill;
            let mut s = o.offset;
            for (i, &q) in o.points.iter().enumerate() {
                if i > 0 {
                    s += geom::dist(o.points[i - 1], q);
                }
                if s < plug || s > o.length - plug || !(x0..=x1).contains(&q[0]) {
                    continue;
                }
                let Some(silt) = field.lake_silt_at(q) else {
                    continue;
                };
                if silt - mean > DELTA_DRY * open {
                    continue;
                }
                out.push((q, 0.4 * o.widths[i] * w * open));
            }
        }
        out
    }

    /// Dijkstra's search over blocks of the erodibility grid from `from` to
    /// the nearest water, by the cost of the ground: low ground is cheap,
    /// the noise of the ground bends routes across level ground, and steps
    /// up the valley cost more. Blocks under the points `avoid` cost
    /// [`STREAM_ABANDON`] times as much. The search keeps to the valley
    /// floor and reaches [`STREAM_REACH`] widths up or down the valley.
    /// Returns the route, smoothed and resampled, from `from` to the water
    /// point it reaches, and the water there.
    fn stream_route(&self, from: P, lakes: &LakeWater, avoid: &[P]) -> Option<(Vec<P>, Wet)> {
        let (w, field) = (self.params.width, &self.erodibility);
        let cell = STREAM_BLOCK as f64 * field.cell;
        let (cols, rows) = (
            field.cols.div_ceil(STREAM_BLOCK),
            field.rows.div_ceil(STREAM_BLOCK),
        );
        let to_col = |x: f64| ((x / cell).max(0.0) as usize).min(cols - 1);
        let to_row = |y: f64| ((y / cell).max(0.0) as usize).min(rows - 1);
        let reach = STREAM_REACH * w;
        let (x0, x1) = (to_col(from[0] - reach), to_col(from[0] + reach));
        let (bw, bh) = (x1 - x0 + 1, rows);
        let local = |x: usize, y: usize| y * bw + (x - x0);
        let inside = |q: P| (x0..=x1).contains(&to_col(q[0]));
        let centre = |l: usize| {
            [
                (x0 + l % bw) as f64 * cell + 0.5 * cell,
                (l / bw) as f64 * cell + 0.5 * cell,
            ]
        };
        // Ground cost: each block as low as its lowest cell, above a fresh
        // lake bed, and its noise.
        let mut ground = vec![0.0f32; bw * bh];
        for (l, c) in ground.iter_mut().enumerate() {
            let (bx, by) = ((x0 + l % bw) * STREAM_BLOCK, (l / bw) * STREAM_BLOCK);
            let (mut low, mut noise) = (f32::INFINITY, 0.0f32);
            for y in by..(by + STREAM_BLOCK).min(field.rows) {
                for x in bx..(bx + STREAM_BLOCK).min(field.cols) {
                    let i = y * field.cols + x;
                    low = low.min(field.deposit[i]);
                    noise += field.noise[i] / (STREAM_BLOCK * STREAM_BLOCK) as f32;
                }
            }
            let jitter = lattice(
                field.seed ^ JITTER_SEED,
                (x0 + l % bw) as i64,
                (l / bw) as i64,
            ) as f32;
            *c = STREAM_BASE
                + STREAM_RISE * (low + LAKE_DEPTH as f32).max(0.0)
                + STREAM_NOISE * noise
                + STREAM_JITTER * jitter;
        }
        for &q in avoid.iter().filter(|&&q| inside(q)) {
            let l = local(to_col(q[0]), to_row(q[1]));
            ground[l] = STREAM_ABANDON * ground[l].max(STREAM_BASE);
        }
        // Each block is open, off the valley floor, or holds water: a
        // channel point or a lake's water point.
        const OPEN: u32 = u32::MAX;
        const SHUT: u32 = u32::MAX - 1;
        const LAKE: u32 = u32::MAX - 2;
        let mut label = vec![OPEN; bw * bh];
        for x in x0..=x1 {
            let (lo, hi) = self.floor((x as f64 + 0.5) * cell, false);
            for y in 0..bh {
                let cy = (y as f64 + 0.5) * cell;
                if cy < lo || cy > hi {
                    label[local(x, y)] = SHUT;
                }
            }
        }
        let mut lake_at = Vec::new();
        for &(q, _) in lakes.iter().filter(|(q, _)| inside(*q)) {
            let l = local(to_col(q[0]), to_row(q[1]));
            label[l] = LAKE;
            lake_at.push(q);
        }
        for (j, &q) in self
            .centerline
            .iter()
            .enumerate()
            .filter(|(_, q)| inside(**q))
        {
            let l = local(to_col(q[0]), to_row(q[1]));
            if label[l] >= LAKE || (label[l] as usize) > j {
                label[l] = j as u32;
            }
        }
        let s = local(to_col(from[0]), to_row(from[1]));
        let mut cost = vec![f32::INFINITY; bw * bh];
        let mut prev = vec![u32::MAX; bw * bh];
        let mut heap = BinaryHeap::new();
        cost[s] = 0.0;
        heap.push(Reverse((0u32, s as u32)));
        let mut reached = None;
        while let Some(Reverse((bits, l))) = heap.pop() {
            let l = l as usize;
            if f32::from_bits(bits) > cost[l] {
                continue;
            }
            if label[l] != OPEN && label[l] != SHUT {
                reached = Some(l);
                break;
            }
            let (x, y) = ((l % bw) as isize, (l / bw) as isize);
            for (dx, dy, step) in NEIGHBOURS {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= bw as isize || ny >= bh as isize {
                    continue;
                }
                let m = ny as usize * bw + nx as usize;
                if label[m] == SHUT {
                    continue;
                }
                let up = if dx < 0 { STREAM_UPSTREAM } else { 1.0 };
                let next = cost[l] + step * up * 0.5 * (ground[l] + ground[m]);
                if next < cost[m] {
                    cost[m] = next;
                    prev[m] = l as u32;
                    heap.push(Reverse((next.to_bits(), m as u32)));
                }
            }
        }
        let l = reached?;
        // End on the water point itself, which the walk finds wet.
        let (end, wet) = if label[l] == LAKE {
            let c = centre(l);
            let q = lake_at
                .iter()
                .copied()
                .min_by(|a, b| geom::dist2(*a, c).total_cmp(&geom::dist2(*b, c)))?;
            (q, Wet::Lake)
        } else {
            let j = label[l] as usize;
            (self.centerline[j], Wet::Channel(j))
        };
        let mut route = vec![end];
        let mut at = prev[l];
        while at != u32::MAX && at as usize != s {
            route.push(centre(at as usize));
            at = prev[at as usize];
        }
        route.push(from);
        route.reverse();
        let last = route.len() - 1;
        for _ in 0..STREAM_SMOOTH {
            for k in 1..last {
                let mid = geom::lerp(route[k - 1], route[k + 1], 0.5);
                route[k] = geom::lerp(route[k], mid, 0.5);
            }
        }
        let mut route = geom::resample(&route, STREAM_SPACING * w);
        // Resampling can stop short of the end.
        if route.last().is_none_or(|&q| geom::dist2(q, end) > 1e-12) {
            route.push(end);
        }
        Some((route, wet))
    }

    /// The tributaries' share of this step's work: those whose turn it is
    /// walk their courses, and build their fans back by the fraction of
    /// what the channel has trimmed that `fan_time` allows, quicker as the
    /// supply rises.
    pub(crate) fn feed_tributaries(&mut self) {
        let fan_time = self.params.fan_time;
        let frac = GROWTH_EVERY as f64 * self.params.dt * self.supply() / fan_time.max(1e-9);
        for k in 0..self.tributaries.len() {
            if (self.steps + k as u64).is_multiple_of(JUNCTION_EVERY) {
                self.join_tributary(k);
            }
            if fan_time > 0.0 && (self.steps + k as u64).is_multiple_of(GROWTH_EVERY) {
                let t = &self.tributaries[k];
                self.erodibility
                    .grow_cone(t.mouth, t.radius, FAN_HEIGHT * t.scale, t.scale, frac);
            }
        }
    }

    /// Each tributary leaves its course as a Poisson process at
    /// `stream_shift_rate`, approximated per step, at a point on its fan,
    /// for a new route through low ground that keeps off the old course
    /// below that point. A point near the apex is likelier, where the fan
    /// is steepest.
    pub(crate) fn shift_streams(&mut self) {
        let chance = self.params.stream_shift_rate * self.params.dt;
        if chance <= 0.0 {
            return;
        }
        let w = self.params.width;
        for k in 0..self.tributaries.len() {
            if self.streams.uniform() >= chance {
                continue;
            }
            let pick = self.streams.uniform();
            let t = &self.tributaries[k];
            let on_fan = t
                .course
                .iter()
                .take_while(|&&q| geom::dist2(q, t.mouth) < t.radius * t.radius)
                .count();
            if on_fan < 2 {
                continue;
            }
            let node = (pick * pick * (on_fan - 1) as f64) as usize;
            let (from, old) = (t.course[node], t.course[node + 1..].to_vec());
            let reach = STREAM_REACH * w;
            let lakes = self.lake_water(from[0] - reach, from[0] + reach);
            let Some((route, wet)) = self.stream_route(from, &lakes, &old) else {
                continue;
            };
            let t = &mut self.tributaries[k];
            t.course.truncate(node + 1);
            join(&mut t.course, &route);
            t.version += 1;
            match wet {
                Wet::Channel(j) => {
                    t.at = j;
                    t.lake = false;
                }
                Wet::Lake => t.lake = true,
            }
        }
    }

    /// Speed migration below each confluence with the channel by
    /// `tributary_boost` times the tributary's scale, falling to nothing
    /// [`TRIBUTARY_REACH`] widths downstream. A stream that ends in a lake
    /// leaves its bedload there.
    pub(crate) fn feed_bars(&mut self) {
        let boost = self.params.tributary_boost;
        let (n, r) = (self.rate.len(), self.reach_points(TRIBUTARY_REACH));
        if boost <= 0.0 {
            return;
        }
        for t in self.tributaries.iter().filter(|t| !t.lake) {
            for k in t.at..(t.at + r).min(n) {
                self.rate[k] *= 1.0 + boost * t.scale * (1.0 - (k - t.at) as f64 / r as f64);
            }
        }
    }
}

/// Run a course on along a route from its last point, smoothing the
/// [`STREAM_JOIN`] points either side of the join so it shows no corner.
fn join(course: &mut Vec<P>, route: &[P]) {
    let a = course.len().saturating_sub(STREAM_JOIN + 1);
    let mut tail = course.split_off(a);
    tail.extend_from_slice(&route[1..]);
    let last = (2 * STREAM_JOIN + 1).min(tail.len() - 1);
    for _ in 0..STREAM_SMOOTH {
        for k in 1..last {
            let mid = geom::lerp(tail[k - 1], tail[k + 1], 0.5);
            tail[k] = geom::lerp(tail[k], mid, 0.5);
        }
    }
    course.extend(tail);
}

/// Steps to the eight neighbouring blocks, with their lengths in blocks.
const NEIGHBOURS: [(isize, isize, f32); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, std::f32::consts::SQRT_2),
    (1, -1, std::f32::consts::SQRT_2),
    (-1, 1, std::f32::consts::SQRT_2),
    (-1, -1, std::f32::consts::SQRT_2),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Oxbow, Params};

    #[test]
    fn a_stream_keeps_its_course_and_ends_in_a_lake_across_it() {
        let mut sim = Sim::new(Params::default(), [400.0 * 8.0, 60.0 * 8.0], 1, 0.5);
        let w = sim.params.width;
        let k = (0..sim.tributaries.len())
            .max_by_key(|&k| sim.tributaries[k].course.len())
            .unwrap();
        let course = sim.tributaries[k].course.clone();
        assert!(course.len() >= 12, "a course of {} points", course.len());
        assert!(!sim.tributaries[k].lake);
        // Nothing has moved, so walking it again leaves it as it is.
        let version = sim.tributaries[k].version;
        sim.join_tributary(k);
        assert_eq!(sim.tributaries[k].version, version);
        assert_eq!(sim.tributaries[k].course, course);
        // A lake across the middle of the course.
        let c = course[course.len() / 2];
        let lake: Vec<P> = (0..41)
            .map(|i| [c[0] + (i as f64 - 20.0) * 0.5 * w, c[1]])
            .collect();
        let widths = vec![1.0; lake.len()];
        sim.erodibility
            .stamp_oxbow(&lake, &widths, 0.5 * w, sim.time);
        sim.oxbows.push(Oxbow::lake(lake, widths, sim.time, 0.0));
        sim.join_tributary(k);
        let t = &sim.tributaries[k];
        assert!(t.lake, "ends in the lake");
        assert!(
            t.course.len() <= course.len() / 2 + 2,
            "cut at the lake: {} of {}",
            t.course.len(),
            course.len()
        );
        assert_eq!(
            t.course[..],
            course[..t.course.len()],
            "the course above the lake stays put"
        );
        let end = *t.course.last().unwrap();
        let silt = sim.erodibility.silt_at(end);
        for _ in 0..50 {
            sim.join_tributary(k);
        }
        assert!(
            sim.erodibility.silt_at(end) > silt + 0.01,
            "the delta builds: {silt} to {}",
            sim.erodibility.silt_at(end)
        );
        // Its bedload stays in the lake.
        let fed = sim.migration_rate().to_vec();
        sim.tributaries.remove(k);
        assert_eq!(fed, sim.migration_rate().to_vec());
    }
}
