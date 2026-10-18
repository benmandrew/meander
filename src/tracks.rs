//! Tracks between the farmsteads, and the fords where they cross the
//! channel. No bridge stands for long on a river that moves this far, so
//! a track crosses at a ford, and when the channel moves the track follows
//! it to a new ford or finds another way.
//!
//! The network is found again and again, a search at a time, spread over
//! the steps. One search goes out from every farmstead at once over the
//! ground, costed by flood risk and with a heavy cost for wading the
//! channel. Each pair of farmsteads whose searches meet could be joined by
//! a lane through the place they meet. The cheapest are laid first, and a
//! lane is laid only if the tracks already laid make the trip more than
//! [`DETOUR`] times as long, a *greedy spanner*. Ground on the last
//! network's tracks costs less, so ways stay in use and the network
//! changes only where the land or the river has.
//!
//! The churches are destinations too, so the network gathers on them.
//!
//! A lane in use for centuries wears into a *hollow way*, sunk below the
//! fields by feet, hooves and wheels and by the rain that runs down it.
//! Each cell keeps the time it has carried a track, and a track is a
//! full hollow way after [`HOLLOW_TIME`]. When the network leaves a lane
//! worn for over [`HOLLOW_MIN`], the lane stays as an earthwork that fills
//! at [`HOLLOW_FILL`] of the rate it wore, until the channel sweeps it.
//!
//! Tracks read the land and change nothing else.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use crate::farm::Kind;
use crate::field::{Erodibility, LAKE_DEPTH};
use crate::geom::P;
use crate::land::{Land, NONE};

/// Most cells the search settles in one step.
const TRACK_BUDGET: usize = 128;
/// Bands of rows over which the places the searches meet are read, one
/// band a step.
const MEET_BANDS: usize = 8;
/// Extra cost of crossing ground at flood risk 1, as a multiple of its
/// length: tracks keep to the dry ground.
const TRACK_RISK: f32 = 2.0;
/// Cost of ground on the last network's tracks, as a fraction of its cost.
const TRACK_KEEP: f32 = 0.5;
/// Extra cost of wading the channel, as a multiple of the length waded.
const FORD_COST: f32 = 20.0;
/// A lane is laid between two farmsteads whose searches meet when the
/// tracks already laid make the trip more than this many times as long.
const DETOUR: f32 = 2.0;
/// Overbank deposit, in channel depths, below which an oxbow's ground is
/// open water that no track crosses.
const LAKE_OPEN: f32 = -0.5 * LAKE_DEPTH as f32;
/// Simulated time a lane takes to wear into a full hollow way: 400 units,
/// 1,000 years; the least wear that leaves an abandoned lane as an
/// earthwork: 40 units, 100 years; and the rate an abandoned hollow way
/// fills, as a fraction of the rate it wore.
const HOLLOW_TIME: f32 = 400.0;
const HOLLOW_MIN: f32 = 40.0;
const HOLLOW_FILL: f32 = 0.25;

enum Phase {
    /// Start a search from every farmstead.
    Start,
    /// A search under way.
    Search,
    /// Reading the band of rows where the searches meet.
    Meet(usize),
    /// Lay the lanes.
    Build,
}

pub struct Tracks {
    /// Each track's cell centres, from one farmstead to another.
    pub tracks: Vec<Vec<P>>,
    /// Where each track crosses the channel, from bank to bank.
    pub fords: Vec<[P; 2]>,
    /// Each track's depth as a hollow way at each of its cells, in [0, 1].
    pub depths: Vec<Vec<f32>>,
    /// The abandoned hollow ways: each one's cell centres, and its depth
    /// at each.
    pub hollows: Vec<Vec<P>>,
    pub hollow_depths: Vec<Vec<f32>>,
    /// Count changes to the tracks, their depths and the hollow ways, and
    /// to `fords`.
    pub version: u64,
    pub ford_version: u64,
    phase: Phase,
    /// The search that set each cell's cost, label and parent; a cell of
    /// an older search is unreached.
    search: u32,
    stamp: Vec<u32>,
    cost: Vec<f32>,
    /// Farmstead whose search reached each cell first, and the cell it
    /// came from, or [`NONE`] at a farmstead.
    label: Vec<u32>,
    from: Vec<u32>,
    /// Whether each cell is on a track.
    on: Vec<bool>,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
    /// Farmsteads at the start of the search, by label.
    seats: Vec<P>,
    /// Cheapest place each pair of searches met: its cost and the cell on
    /// each side.
    meets: BTreeMap<(u32, u32), (f32, u32, u32)>,
    /// Each track's cells.
    cells: Vec<Vec<u32>>,
    /// The fords as last found, kept to find them again into.
    spare: Vec<[P; 2]>,
    /// Time each cell has carried a track, and the time each cell of an
    /// abandoned hollow way left the network.
    wear: Vec<f32>,
    left: Vec<f32>,
    /// The abandoned hollow ways' cells, and whether each cell is in one.
    relicts: Vec<Vec<u32>>,
    relict: Vec<bool>,
    /// Time the last network was laid.
    built_at: f64,
}

impl Default for Tracks {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            fords: Vec::new(),
            version: 0,
            ford_version: 0,
            phase: Phase::Start,
            search: 0,
            stamp: Vec::new(),
            cost: Vec::new(),
            label: Vec::new(),
            from: Vec::new(),
            on: Vec::new(),
            heap: BinaryHeap::new(),
            seats: Vec::new(),
            meets: BTreeMap::new(),
            cells: Vec::new(),
            spare: Vec::new(),
            depths: Vec::new(),
            hollows: Vec::new(),
            hollow_depths: Vec::new(),
            wear: Vec::new(),
            left: Vec::new(),
            relicts: Vec::new(),
            relict: Vec::new(),
            built_at: f64::NAN,
        }
    }
}

impl Tracks {
    /// Drop the network, for a new river.
    pub fn clear(&mut self) {
        *self = Self {
            version: self.version + 1,
            ford_version: self.ford_version + 1,
            ..Self::default()
        };
    }

    /// One step's share of the work. `fresh` is set in the step after the
    /// holdings read the channel, when the fords are found again on the
    /// tracks as they run.
    pub fn tick(&mut self, land: &Land, field: &Erodibility, width: f64, time: f64, fresh: bool) {
        let n = land.owner.len();
        if self.stamp.len() != n {
            *self = Self {
                stamp: vec![0; n],
                cost: vec![f32::INFINITY; n],
                label: vec![NONE; n],
                from: vec![NONE; n],
                on: vec![false; n],
                wear: vec![0.0; n],
                left: vec![0.0; n],
                relict: vec![false; n],
                version: self.version + 1,
                ford_version: self.ford_version + 1,
                ..Self::default()
            };
        }
        if fresh {
            if self.find_fords(land) {
                self.ford_version += 1;
            }
            self.wash(land, time);
        }
        let cw = (land.cell / width) as f32;
        match self.phase {
            Phase::Start => self.start(land),
            Phase::Search => {
                if self.settle(land, field, cw) {
                    self.phase = Phase::Meet(0);
                }
            }
            Phase::Meet(b) => {
                self.meet(land, field, cw, b);
                self.phase = if b + 1 < MEET_BANDS {
                    Phase::Meet(b + 1)
                } else {
                    Phase::Build
                };
            }
            Phase::Build => {
                self.build(land, time);
                self.phase = Phase::Start;
            }
        }
    }

    fn reached(&self, i: usize) -> bool {
        self.stamp[i] == self.search
    }

    fn start(&mut self, land: &Land) {
        self.search = self.search.wrapping_add(1);
        self.heap.clear();
        self.seats.clear();
        self.meets.clear();
        let churches = (land.buildings.iter())
            .filter(|b| b.kind == Kind::Church && b.standing())
            .map(|b| b.pos);
        for seat in land
            .holdings
            .iter()
            .flatten()
            .map(|h| h.seat)
            .chain(churches)
        {
            let (i, k) = (land.index(seat), self.seats.len() as u32);
            self.seats.push(seat);
            if !self.reached(i) {
                self.stamp[i] = self.search;
                (self.cost[i], self.label[i], self.from[i]) = (0.0, k, NONE);
                self.heap.push(Reverse((0f32.to_bits(), i as u32)));
            }
        }
        self.phase = Phase::Search;
    }

    /// Cost of stepping from cell `i` to its neighbour `j`, `len` widths
    /// away, or `None` if a track cannot. A diagonal step needs both cells
    /// beside it dry, so no track slips across the channel where its cells
    /// meet at a corner.
    fn step(&self, land: &Land, field: &Erodibility, i: usize, j: usize, len: f32) -> Option<f32> {
        let open_water = |k: usize| field.cut_at[k].is_finite() && field.deposit[k] < LAKE_OPEN;
        let wet = |k: usize| land.in_channel(k) || open_water(k);
        if open_water(j) {
            return None;
        }
        let cols = land.cols;
        if i % cols != j % cols && i / cols != j / cols {
            let (a, b) = (i - i % cols + j % cols, j - j % cols + i % cols);
            if wet(a) || wet(b) {
                return None;
            }
        }
        let mut f = land.rough[j] + TRACK_RISK * land.risk[j];
        if land.in_channel(j) {
            f += FORD_COST;
        }
        if self.on[j] {
            f *= TRACK_KEEP;
        }
        Some(len * f)
    }

    /// The neighbours of cell `i`, and the length of a step to each in
    /// channel widths.
    fn neighbours(land: &Land, i: usize, cw: f32, out: &mut [(usize, f32); 8]) -> usize {
        let (cols, rows) = (land.cols, land.rows);
        let (x, y) = (i % cols, i / cols);
        let diag = cw * std::f32::consts::SQRT_2;
        let mut n = 0;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if (dx, dy) == (0, 0) || nx < 0 || ny < 0 || nx >= cols as i32 || ny >= rows as i32
                {
                    continue;
                }
                out[n] = (
                    ny as usize * cols + nx as usize,
                    if dx != 0 && dy != 0 { diag } else { cw },
                );
                n += 1;
            }
        }
        n
    }

    /// Settle up to [`TRACK_BUDGET`] cells. Returns whether the search has
    /// finished.
    fn settle(&mut self, land: &Land, field: &Erodibility, cw: f32) -> bool {
        let mut out = [(0, 0.0); 8];
        let mut budget = TRACK_BUDGET;
        while budget > 0 {
            let Some(Reverse((bits, i))) = self.heap.pop() else {
                return true;
            };
            let (i, c) = (i as usize, f32::from_bits(bits));
            if c > self.cost[i] {
                continue;
            }
            budget -= 1;
            let n = Self::neighbours(land, i, cw, &mut out);
            for &(j, len) in &out[..n] {
                let Some(step) = self.step(land, field, i, j, len) else {
                    continue;
                };
                if !self.reached(j) || c + step < self.cost[j] {
                    self.stamp[j] = self.search;
                    (self.cost[j], self.label[j], self.from[j]) =
                        (c + step, self.label[i], i as u32);
                    self.heap.push(Reverse(((c + step).to_bits(), j as u32)));
                }
            }
        }
        false
    }

    /// Read band `b` of the rows for the places two searches meet, keeping
    /// the cheapest for each pair of farmsteads.
    fn meet(&mut self, land: &Land, field: &Erodibility, cw: f32, b: usize) {
        let (cols, rows) = (land.cols, land.rows);
        let mut out = [(0, 0.0); 8];
        for i in (b * rows / MEET_BANDS) * cols..((b + 1) * rows / MEET_BANDS) * cols {
            if !self.reached(i) {
                continue;
            }
            let n = Self::neighbours(land, i, cw, &mut out);
            for &(j, len) in &out[..n] {
                if j < i || !self.reached(j) || self.label[i] == self.label[j] {
                    continue;
                }
                let Some(step) = self.step(land, field, i, j, len) else {
                    continue;
                };
                let (a, z) = (self.label[i], self.label[j]);
                let (key, ends) = if a < z {
                    ((a, z), (i as u32, j as u32))
                } else {
                    ((z, a), (j as u32, i as u32))
                };
                let c = self.cost[i] + step + self.cost[j];
                let best = self.meets.entry(key).or_insert((f32::INFINITY, 0, 0));
                if c < best.0 {
                    *best = (c, ends.0, ends.1);
                }
            }
        }
    }

    /// Cost of the cheapest trip between farmsteads `a` and `b` over the
    /// lanes `graph` holds, if under `limit`.
    fn trip(graph: &[Vec<(usize, f32)>], a: usize, b: usize, limit: f32) -> f32 {
        let mut best = vec![f32::INFINITY; graph.len()];
        let mut heap = BinaryHeap::new();
        best[a] = 0.0;
        heap.push(Reverse((0f32.to_bits(), a)));
        while let Some(Reverse((bits, k))) = heap.pop() {
            let c = f32::from_bits(bits);
            if k == b {
                return c;
            }
            if c > best[k] || c > limit {
                continue;
            }
            for &(m, w) in &graph[k] {
                if c + w < best[m] {
                    best[m] = c + w;
                    heap.push(Reverse(((c + w).to_bits(), m)));
                }
            }
        }
        f32::INFINITY
    }

    /// The cells from farmstead to cell `i`, along the search's parents.
    fn trace(&self, mut i: u32, out: &mut Vec<u32>) {
        let start = out.len();
        while i != NONE {
            out.push(i);
            i = self.from[i as usize];
        }
        out[start..].reverse();
    }

    /// Lay the lanes, cheapest first, and the tracks along them, wearing
    /// the last network's cells by the time it was in use.
    fn build(&mut self, land: &Land, time: f64) {
        let mut meets: Vec<_> = self.meets.iter().map(|(&k, &v)| (k, v)).collect();
        meets.sort_by(|a, b| a.1.0.total_cmp(&b.1.0).then(a.0.cmp(&b.0)));
        let mut graph = vec![Vec::new(); self.seats.len()];
        let mut cells = Vec::new();
        for ((a, b), (c, i, j)) in meets {
            let (a, b) = (a as usize, b as usize);
            if Self::trip(&graph, a, b, DETOUR * c) <= DETOUR * c {
                continue;
            }
            graph[a].push((b, c));
            graph[b].push((a, c));
            let mut path = Vec::new();
            self.trace(i, &mut path);
            let mut back = Vec::new();
            self.trace(j, &mut back);
            path.extend(back.into_iter().rev());
            cells.push(path);
        }
        let used = if self.built_at.is_nan() {
            0.0
        } else {
            (time - self.built_at) as f32
        };
        self.built_at = time;
        // A cell several tracks share wears once.
        for &k in self.cells.iter().flatten() {
            let k = k as usize;
            if self.on[k] {
                self.wear[k] += used;
                self.on[k] = false;
            }
        }
        for &k in cells.iter().flatten() {
            let k = k as usize;
            if self.relict[k] {
                self.wear[k] = self.lasting(k, time);
                self.relict[k] = false;
            }
            self.on[k] = true;
        }
        self.prune(time);
        for t in &self.cells {
            let mut run = Vec::new();
            for &k in t.iter().chain([&NONE]) {
                let i = k as usize;
                if k != NONE && !self.on[i] && !self.relict[i] && self.wear[i] >= HOLLOW_MIN {
                    (self.left[i], self.relict[i]) = (time as f32, true);
                    run.push(k);
                } else if run.len() >= 2 {
                    self.relicts.push(std::mem::take(&mut run));
                } else {
                    for &j in &run {
                        self.relict[j as usize] = false;
                    }
                    run.clear();
                }
            }
        }
        if cells != self.cells {
            self.tracks = (cells.iter())
                .map(|t| t.iter().map(|&k| land.centre(k as usize)).collect())
                .collect();
            self.cells = cells;
            if self.find_fords(land) {
                self.ford_version += 1;
            }
        }
        self.depths = (self.cells.iter())
            .map(|t| t.iter().map(|&k| self.depth(k as usize, time)).collect())
            .collect();
        self.read_hollows(land, time);
        self.version += 1;
    }

    /// Wear left at cell `i` of an abandoned hollow way at `time`.
    fn lasting(&self, i: usize, time: f64) -> f32 {
        (self.wear[i] - HOLLOW_FILL * (time as f32 - self.left[i])).max(0.0)
    }

    /// Depth of cell `i` as a hollow way at `time`, in [0, 1].
    fn depth(&self, i: usize, time: f64) -> f32 {
        let wear = if self.relict[i] {
            self.lasting(i, time)
        } else {
            self.wear[i]
        };
        (wear / HOLLOW_TIME).min(1.0)
    }

    /// Split the abandoned hollow ways at the cells the network has taken
    /// up again or that have filled, and drop the pieces under two cells.
    fn prune(&mut self, time: f64) {
        let mut kept = Vec::with_capacity(self.relicts.len());
        for run in std::mem::take(&mut self.relicts) {
            let mut piece = Vec::new();
            for &k in run.iter().chain([&NONE]) {
                let i = k as usize;
                if k != NONE && self.relict[i] && self.lasting(i, time) > 0.0 {
                    piece.push(k);
                    continue;
                }
                if k != NONE && self.relict[i] {
                    (self.relict[i], self.wear[i]) = (false, 0.0);
                }
                if piece.len() >= 2 {
                    kept.push(std::mem::take(&mut piece));
                } else {
                    for &j in &piece {
                        (self.relict[j as usize], self.wear[j as usize]) = (false, 0.0);
                    }
                }
                piece.clear();
            }
        }
        self.relicts = kept;
    }

    /// Copy the abandoned hollow ways' points and depths for a renderer.
    fn read_hollows(&mut self, land: &Land, time: f64) {
        self.hollows = (self.relicts.iter())
            .map(|r| r.iter().map(|&k| land.centre(k as usize)).collect())
            .collect();
        self.hollow_depths = (self.relicts.iter())
            .map(|r| r.iter().map(|&k| self.depth(k as usize, time)).collect())
            .collect();
    }

    /// Wipe the wear from the cells of the tracks and the hollow ways that
    /// the water has taken or the channel has swept to bare bar since the
    /// last survey, and copy the hollow ways again if any changed.
    fn wash(&mut self, land: &Land, time: f64) {
        for &k in self.cells.iter().flatten() {
            if !land.open[k as usize] {
                self.wear[k as usize] = 0.0;
            }
        }
        let mut swept = false;
        for &k in self.relicts.iter().flatten() {
            let i = k as usize;
            if !land.open[i] && self.wear[i] > 0.0 {
                self.wear[i] = 0.0;
                swept = true;
            }
        }
        if swept {
            self.prune(time);
            self.read_hollows(land, time);
            self.version += 1;
        }
    }

    /// Find where each track runs through the channel, from the dry cell
    /// before to the dry cell after. Returns whether they changed.
    fn find_fords(&mut self, land: &Land) -> bool {
        let mut fords = std::mem::take(&mut self.spare);
        fords.clear();
        for t in &self.cells {
            let mut k = 0;
            while k < t.len() {
                if !land.in_channel(t[k] as usize) {
                    k += 1;
                    continue;
                }
                let a = k.saturating_sub(1);
                while k < t.len() && land.in_channel(t[k] as usize) {
                    k += 1;
                }
                let b = k.min(t.len() - 1);
                fords.push([land.centre(t[a] as usize), land.centre(t[b] as usize)]);
            }
        }
        let changed = fords != self.fords;
        self.spare = std::mem::replace(&mut self.fords, fords);
        changed
    }
}
