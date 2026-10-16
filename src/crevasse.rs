//! Crevasse splays and avulsion. In a flood, the water stands highest on
//! the levee crest along an outer bank. Where the floodplain beyond falls
//! away steeply enough, the flood breaks through the levee and spreads a
//! lobe of sand over the ground below, burying its vegetation. In a large
//! flood the breach can capture the river: if the low ground beyond leads
//! back to the channel downstream by a route much shorter than the
//! channel, the river takes it and leaves its old course as an oxbow.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::field::{LAKE_DEPTH, Splay};
use crate::geom::{self, P, right_normal};
use crate::{
    AVULSION_BASE, AVULSION_JOIN, AVULSION_REACH, AVULSION_RISE, AVULSION_SMOOTH, AVULSION_WIDTH,
    CUT_COOLDOWN, JAM_CREVASSE, PINCH_CLEAR, SPLAY_LIFE, SPLAY_PROBE, SPLAY_SAND, SPLAY_SPREAD,
    Sim, edge_taper,
};

/// Spacing of the points tested for a breach, in centreline points.
const BREACH_EVERY: usize = 4;
/// Most splays kept for drawing.
const MAX_SPLAYS: usize = 24;
/// Distance upstream of a breach the avulsion search may reach, in channel
/// widths; it runs mostly downstream.
const AVULSION_BACK: f64 = 10.0;
/// Least wall taper a cell of an avulsion's route may have, so the route
/// keeps off the valley walls.
const AVULSION_TAPER: f64 = 0.3;
/// Side of the blocks of erodibility cells an avulsion's search runs on.
const SEARCH_BLOCK: usize = 2;

/// A crevasse splay a flood laid, kept for drawing while vegetation grows
/// over it.
#[derive(Clone, Debug)]
pub struct SplayMark {
    pub apex: P,
    /// Unit vector down the lobe.
    pub dir: P,
    /// Length of the lobe, in world units.
    pub length: f64,
    pub born: f64,
}

/// A breach in the levee: the point it is beside, where the lobe starts,
/// and the direction it runs.
struct Breach {
    at: usize,
    apex: P,
    dir: P,
}

impl Sim {
    /// Drop splays past [`SPLAY_LIFE`].
    pub(crate) fn age_splays(&mut self) {
        let (time, before) = (self.time, self.splays.len());
        self.splays.retain(|s| time - s.born < SPLAY_LIFE);
        if self.splays.len() != before {
            self.splay_version += 1;
        }
    }

    /// One flood of magnitude `u` breaks the levee at the outer-bank point
    /// whose crest stands highest over the ground [`SPLAY_PROBE`] widths
    /// beyond, against the fall `crevasse_drop / u` it needs, and lays a
    /// splay there. A jam nearby backs the flood up and lowers the fall
    /// needed. In a flood of `avulsion_flood` or more, the river may then
    /// avulse through the breach. Returns whether it did.
    pub(crate) fn crevasse(&mut self, u: f64) -> bool {
        let p = &self.params;
        if p.crevasse_drop <= 0.0 || u <= 0.0 {
            return false;
        }
        self.migration_rate();
        let Some(b) = self.find_breach(u) else {
            return false;
        };
        let p = &self.params;
        let splay = Splay {
            apex: b.apex,
            dir: b.dir,
            spread: SPLAY_SPREAD,
            length: p.splay_length * u * p.width,
            sand: SPLAY_SAND * u * self.supply(),
            stage: p.levee_height,
            mature: p.vegetation_time + p.forest_time,
        };
        self.erodibility.splay(&splay);
        self.splays.push(SplayMark {
            apex: b.apex,
            dir: b.dir,
            length: splay.length,
            born: self.time,
        });
        if self.splays.len() > MAX_SPLAYS {
            self.splays.remove(0);
        }
        self.splay_version += 1;
        let p = &self.params;
        u >= p.avulsion_flood && p.avulsion_ratio > 0.0 && self.avulse(&b)
    }

    fn find_breach(&self, u: f64) -> Option<Breach> {
        let p = &self.params;
        let (w, n, k_sep) = (p.width, self.centerline.len(), p.cutoff_min_sep);
        let field = &self.erodibility;
        let margin = p.edge_margin * w;
        let mut best: Option<(f64, Breach)> = None;
        for i in (k_sep..n.saturating_sub(k_sep)).step_by(BREACH_EVERY) {
            let q = self.centerline[i];
            if q[0] < margin || q[0] > self.bounds[0] - margin || self.near_recent(q) {
                continue;
            }
            let side = if self.rate[i] < 0.0 { -1.0 } else { 1.0 };
            let nrm = right_normal(&self.centerline, i);
            let dir = [side * nrm[0], side * nrm[1]];
            let at = |d: f64| {
                let off = (0.5 * self.widths[i] + d) * w;
                [q[0] + off * dir[0], q[1] + off * dir[1]]
            };
            let (crest, probe) = (at(0.5), at(SPLAY_PROBE));
            if edge_taper(probe, self.bounds, margin, self.floor(probe[0], true)) < AVULSION_TAPER {
                continue;
            }
            let fall = field.deposit_at(crest).max(0.0) - field.deposit_at(probe);
            let needed = p.crevasse_drop / u / (1.0 + JAM_CREVASSE * self.jam_near(i));
            let score = fall / needed;
            if score >= 1.0 && best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((
                    score,
                    Breach {
                        at: i,
                        apex: crest,
                        dir,
                    },
                ));
            }
        }
        best.map(|(_, b)| b)
    }

    /// Search the floodplain from breach `b` for routes back to the
    /// channel downstream, where crossing a cell costs more the higher its
    /// ground stands over a fresh lake bed, so routes follow old channels
    /// and lakes. Of the routes whose mean ground stands below the levee
    /// crest at the breach, the river takes the one that shortens it most,
    /// if the channel between the breach and where the route rejoins it is
    /// longer than the route by `avulsion_ratio`, and the route neither
    /// crosses the channel nor turns sharply. Returns whether it did.
    fn avulse(&mut self, b: &Breach) -> bool {
        let Some((j, cells)) = self.avulsion_search(b) else {
            return false;
        };
        let n = self.centerline.len();
        // The route leaves the channel [`AVULSION_JOIN`] points above the
        // breach and rejoins it as far below where the search reached it,
        // so it meets the channel at a slant.
        let (i, j) = (b.at.saturating_sub(AVULSION_JOIN), j + AVULSION_JOIN);
        if i == 0 || j + 1 >= n {
            return false;
        }
        let mut route: Vec<P> = Vec::with_capacity(cells.len() + 2);
        route.push(self.centerline[i]);
        route.extend_from_slice(&cells);
        route.push(self.centerline[j]);
        let last = route.len() - 1;
        for _ in 0..AVULSION_SMOOTH {
            for k in 1..last {
                let mid = geom::lerp(route[k - 1], route[k + 1], 0.5);
                route[k] = geom::lerp(route[k], mid, 0.5);
            }
        }
        let route = geom::resample(&route, self.ds());
        let arc = (j - i) as f64 * geom::length(&self.centerline) / (n - 1) as f64;
        if route.len() < 3 || arc < self.params.avulsion_ratio * geom::length(&route) {
            return false;
        }
        if self.route_turns_sharply(i, j, &route) || self.route_crosses(i, j, &route) {
            return false;
        }
        let old = self.centerline[i..=j].to_vec();
        let old_widths = self.widths[i..=j].to_vec();
        let inner = &route[1..route.len() - 1];
        self.centerline.splice(i + 1..j, inner.iter().copied());
        self.widths
            .splice(i + 1..j, std::iter::repeat_n(AVULSION_WIDTH, inner.len()));
        self.line_version += 1;
        self.abandon(old, old_widths, route[2.min(route.len() - 1)]);
        self.recent_cuts.push((b.apex, self.steps + CUT_COOLDOWN));
        self.avulsions += 1;
        true
    }

    /// Dijkstra's search over the erodibility grid from breach `b`, in a
    /// box reaching [`AVULSION_BACK`] widths upstream and
    /// [`AVULSION_REACH`] downstream. Cells under the channel less than
    /// `cutoff_min_sep` points below the breach, near the valley walls or
    /// near a pinch are closed; cells under the channel beyond are where a
    /// route can end. Returns the channel point of the route that
    /// shortens the river most for its length, among those through ground
    /// below the levee crest at the breach, and the centres of the cells
    /// along it.
    fn avulsion_search(&self, b: &Breach) -> Option<(usize, Vec<P>)> {
        let p = &self.params;
        let (w, n, field) = (p.width, self.centerline.len(), &self.erodibility);
        // The search runs on blocks of SEARCH_BLOCK × SEARCH_BLOCK cells,
        // each as low as its lowest cell, so a lake a width across stays
        // open. The route is smoothed afterwards in any case.
        let cell = SEARCH_BLOCK as f64 * field.cell;
        let (cols, rows) = (
            field.cols.div_ceil(SEARCH_BLOCK),
            field.rows.div_ceil(SEARCH_BLOCK),
        );
        let to_col = |x: f64| ((x / cell).max(0.0) as usize).min(cols - 1);
        let to_row = |y: f64| ((y / cell).max(0.0) as usize).min(rows - 1);
        let (x0, x1) = (
            to_col(b.apex[0] - AVULSION_BACK * w),
            to_col(b.apex[0] + AVULSION_REACH * w),
        );
        let (lo, hi) = self.floor(b.apex[0], false);
        let (y0, y1) = (to_row(lo), to_row(hi));
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        let local = |x: usize, y: usize| (y - y0) * bw + (x - x0);
        let centre = |l: usize| {
            [
                (x0 + l % bw) as f64 * cell + 0.5 * cell,
                (y0 + l / bw) as f64 * cell + 0.5 * cell,
            ]
        };
        let mut low = vec![f32::INFINITY; bw * bh];
        for (l, h) in low.iter_mut().enumerate() {
            let (bx, by) = ((x0 + l % bw) * SEARCH_BLOCK, (y0 + l / bw) * SEARCH_BLOCK);
            for y in by..(by + SEARCH_BLOCK).min(field.rows) {
                for x in bx..(bx + SEARCH_BLOCK).min(field.cols) {
                    *h = h.min(field.deposit[y * field.cols + x]);
                }
            }
            *h = (*h + LAKE_DEPTH as f32).max(0.0);
        }
        // Each cell is open, closed, or under channel point k.
        const OPEN: u32 = u32::MAX;
        const SHUT: u32 = u32::MAX - 1;
        let mut label = vec![OPEN; bw * bh];
        let margin = p.edge_margin * w;
        let clear = PINCH_CLEAR * p.pinch_length * w;
        for x in x0..=x1 {
            let cx = (x as f64 + 0.5) * cell;
            let floor = self.floor(cx, true);
            let pinched = self.pinches.iter().any(|&px| (cx - px).abs() < clear);
            for y in y0..=y1 {
                let c = [cx, (y as f64 + 0.5) * cell];
                if pinched || edge_taper(c, self.bounds, margin, floor) < AVULSION_TAPER {
                    label[local(x, y)] = SHUT;
                }
            }
        }
        let first = b.at + p.cutoff_min_sep;
        for (k, &q) in self.centerline.iter().enumerate() {
            let half = 0.5 * self.widths[k] * w;
            let r = (half / cell).ceil() as isize;
            let (cx, cy) = ((q[0] / cell) as isize, (q[1] / cell) as isize);
            for y in (cy - r).max(y0 as isize)..=(cy + r).min(y1 as isize) {
                for x in (cx - r).max(x0 as isize)..=(cx + r).min(x1 as isize) {
                    let l = local(x as usize, y as usize);
                    if geom::dist2(centre(l), q) > half * half {
                        continue;
                    }
                    label[l] = if k >= first && k + 1 < n && label[l] != SHUT {
                        label[l].min(k as u32)
                    } else {
                        SHUT
                    };
                }
            }
        }
        // Start from the first open cell out through the breach.
        let s = (0..4).find_map(|k| {
            let c = [
                b.apex[0] + k as f64 * cell * b.dir[0],
                b.apex[1] + k as f64 * cell * b.dir[1],
            ];
            let (x, y) = (to_col(c[0]), to_row(c[1]));
            let inside = (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
            (inside && label[local(x, y)] == OPEN).then(|| local(x, y))
        })?;
        let mut cost = vec![f32::INFINITY; bw * bh];
        let mut length = vec![0.0f32; bw * bh];
        let mut prev = vec![u32::MAX; bw * bh];
        let mut heap = BinaryHeap::new();
        cost[s] = 0.0;
        heap.push(Reverse((0u32, s as u32)));
        // A route's mean ground must stand below the crest.
        let crest = field.deposit_at(b.apex).max(0.0) as f32 + LAKE_DEPTH as f32;
        let most = AVULSION_BASE + AVULSION_RISE * crest;
        let ds = geom::length(&self.centerline) / (n - 1) as f64;
        let mut best: Option<(f64, usize, usize)> = None;
        while let Some(Reverse((bits, l))) = heap.pop() {
            let l = l as usize;
            let c = f32::from_bits(bits);
            if c > cost[l] {
                continue;
            }
            if label[l] < SHUT {
                let k = label[l] as usize;
                let len = length[l] as f64 * cell;
                let ratio = (k - b.at) as f64 * ds / len.max(cell);
                if c <= most * length[l] && best.is_none_or(|(r, _, _)| ratio > r) {
                    best = Some((ratio, k, l));
                }
                continue;
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
                let next = c + step * (AVULSION_BASE + AVULSION_RISE * low[m]);
                if next < cost[m] {
                    cost[m] = next;
                    length[m] = length[l] + step;
                    prev[m] = l as u32;
                    heap.push(Reverse((next.to_bits(), m as u32)));
                }
            }
        }
        let (_, k, l) = best?;
        let mut cells = Vec::new();
        let mut at = prev[l];
        while at != u32::MAX {
            cells.push(centre(at as usize));
            at = prev[at as usize];
        }
        cells.push(b.apex);
        cells.reverse();
        Some((k, cells))
    }
}

/// Steps to the eight neighbouring cells, with their lengths in cells.
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

    /// A straight channel across a domain 400 widths long, with levees of
    /// full height along it.
    fn levee_sim() -> Sim {
        let params = Params {
            flood_rate: 0.0,
            jitter: 0.0,
            heterogeneity: 0.0,
            wood_rate: 0.0,
            ..Params::default()
        };
        let w = params.width;
        let mut sim = Sim::new(params, [400.0 * w, 100.0 * w], 1, 0.0);
        sim.erodibility.deposit.fill(sim.params.levee_height as f32);
        sim
    }

    /// Lower ground to a fresh lake bed along `pts`, half a width either side.
    fn trench(sim: &mut Sim, pts: &[P]) {
        let widths = vec![1.0; pts.len()];
        let w = sim.params.width;
        sim.erodibility.stamp_oxbow(pts, &widths, 0.5 * w, sim.time);
    }

    #[test]
    fn a_flood_breaks_the_levee_towards_low_ground() {
        let mut sim = levee_sim();
        let (w, i) = (sim.params.width, sim.centerline.len() / 2);
        let (q, nrm) = (sim.centerline[i], right_normal(&sim.centerline, i));
        let out = |d: f64, x: f64| [q[0] + x * w + d * w * nrm[0], q[1] + d * w * nrm[1]];
        assert!(!sim.crevasse(1.0), "no breach above level ground");
        assert!(sim.splays.is_empty());
        let lake: Vec<P> = (0..20).map(|k| out(4.5, 0.5 * k as f64 - 5.0)).collect();
        trench(&mut sim, &lake);
        sim.oxbows.push(Oxbow::lake(lake, vec![1.0; 20], 0.0, 0.0));
        let growth = sim.erodibility.growth_at(out(1.5, 0.0));
        sim.crevasse(1.0);
        assert_eq!(sim.splays.len(), 1, "a breach towards the lake");
        let s = &sim.splays[0];
        let toward = s.dir[0] * nrm[0] + s.dir[1] * nrm[1];
        assert!(
            (s.apex[0] - q[0]).abs() < 6.0 * w && toward > 0.9,
            "beside the lake, out towards it: {s:?}"
        );
        let inside = [s.apex[0] + w * s.dir[0], s.apex[1] + w * s.dir[1]];
        assert!(
            sim.erodibility.growth_at(inside) < growth,
            "sand buries the forest"
        );
        // A shallow fall holds against a flood unless a jam on the far bank
        // backs it up and turns the flow onto this one.
        let mut sim = levee_sim();
        let dip: Vec<P> = (-10..10)
            .flat_map(|x| (6..12).map(move |d| out(0.5 * d as f64, 0.5 * x as f64)))
            .collect();
        for &p in &dip {
            let k = sim.erodibility.cell_index(p);
            sim.erodibility.deposit[k] = 0.15;
        }
        assert!(!sim.crevasse(1.0), "a fall of 0.1 holds");
        let pos = out(-0.5, 0.0);
        sim.jams.push(crate::Jam {
            pos,
            down: [1.0, 0.0],
            side: -1.0,
            size: 1.0,
            until: 1e9,
            at: i,
        });
        assert!(
            sim.crevasse(1.0) || !sim.splays.is_empty(),
            "a jam breaks it"
        );
        assert_eq!(sim.splays.len(), 1);
    }

    #[test]
    fn a_large_flood_avulses_down_an_old_channel() {
        let mut sim = levee_sim();
        sim.params.avulsion_ratio = 1.5;
        let (w, mid) = (sim.params.width, sim.bounds[1] / 2.0);
        let at = |x: f64, y: f64| [x * w, mid + y * w];
        // A wide bend up from the line, and an old channel across its foot.
        let mut pts: Vec<P> = (0..=220).map(|k| at(0.5 * k as f64, 0.0)).collect();
        pts.extend((1..=24).map(|k| at(110.0, 0.5 * k as f64)));
        pts.extend((1..=48).map(|k| at(110.0 + 0.5 * k as f64, 12.0)));
        pts.extend((1..=24).map(|k| at(134.0, 12.0 - 0.5 * k as f64)));
        pts.extend((1..=532).map(|k| at(134.0 + 0.5 * k as f64, 0.0)));
        sim.centerline = pts;
        sim.widths = vec![1.0; sim.centerline.len()];
        sim.line_version += 1;
        let old: Vec<P> = (0..=40).map(|k| at(112.0 + 0.5 * k as f64, 2.0)).collect();
        trench(&mut sim, &old);
        let i = 220 + 4;
        let b = Breach {
            at: i,
            apex: at(111.0, 2.0),
            dir: [1.0, 0.0],
        };
        let (cutoffs, n) = (sim.cutoffs, sim.centerline.len());
        assert!(sim.avulse(&b), "the river takes the old channel");
        assert_eq!((sim.avulsions, sim.cutoffs), (1, cutoffs + 1));
        assert!(sim.centerline.len() < n - 40, "a shorter course");
        assert!(
            sim.centerline.iter().all(|q| q[1] < mid + 6.0 * w),
            "the bend is left"
        );
        assert!(!geom::self_intersects(&sim.centerline));
        assert!(
            sim.oxbows
                .iter()
                .any(|o| o.points.iter().any(|q| q[1] > mid + 11.0 * w)),
            "as an oxbow"
        );
    }
}
