//! Water mills. A holding builds a mill on a straight reach beside its
//! ground: a weir across the channel and a leat along the bank that
//! carries water to the wheel and back. The weir's masonry holds the banks
//! beside it, and the pond it holds back slows the reach above it, widens
//! it, and drops more silt on the banks in a flood. A mill is left as a
//! ruin when the channel cuts away from its weir or breaks into its leat,
//! or a great flood sweeps the weir away.

use crate::geom::{self, P, right_normal};
use crate::{
    LEAT_DOWN, LEAT_OFFSET, LEAT_UP, MILL_FLOOD, MILL_RUIN, MILL_SNAP, MILL_STRAIGHT, POND_SILT, POND_WIDEN, Sim,
    WEIR_HOLD, land, smoothstep,
};

/// Distance over which a leat leaves the bank at either end, in channel
/// widths.
const LEAT_RAMP: f64 = 2.0;
/// Share of a leat's length that must run on a holding's ground; its ends
/// may cross the bar at the bank.
const LEAT_OWNED: f64 = 0.75;

/// A mill, its weir and its leat.
#[derive(Clone, Debug)]
pub struct Mill {
    /// The weir, on the centreline.
    pub weir: P,
    /// Unit vector across the channel at the weir, towards the mill.
    pub across: P,
    /// The leat, from where it leaves the channel to where it returns.
    /// Its ends follow the bank while the mill works; see [`Sim::rejoin`].
    pub leat: Vec<P>,
    /// The course the leat was dug along, [`LEAT_OFFSET`] widths beyond
    /// the bank as it ran when the mill was founded. It stays where it was
    /// dug while the channel migrates.
    pub course: Vec<P>,
    pub born: f64,
    /// Time the mill fell into ruin, or infinity while it works.
    pub ruined_at: f64,
    /// Centreline point at the weir, kept up to date by [`Sim::track`].
    pub at: usize,
}

impl Mill {
    pub fn working(&self) -> bool {
        self.ruined_at.is_infinite()
    }
}

impl Sim {
    /// Find each working mill's weir on the channel and lay its leat to the
    /// bank, and leave as a ruin each mill the channel has cut away from or
    /// broken into the leat of. Drop ruins past [`MILL_RUIN`].
    pub(crate) fn track_mills(&mut self) {
        if self.mills.is_empty() {
            return;
        }
        let (time, w) = (self.time, self.params.width);
        let before = self.mills.len();
        self.mills.retain(|m| time - m.ruined_at < MILL_RUIN);
        let mut changed = self.mills.len() != before;
        let g = self.build_grid(MILL_SNAP * w);
        let mut mills = std::mem::take(&mut self.mills);
        for m in mills.iter_mut().filter(|m| m.working()) {
            let held = match self.nearest_point(m.weir, MILL_SNAP * w, g) {
                Some(i) if !self.outflanked(i) => {
                    m.at = i;
                    !self.rejoin(m)
                }
                _ => false,
            };
            if !held {
                m.ruined_at = time;
                changed = true;
            }
        }
        self.mills = mills;
        if changed {
            self.mill_version += 1;
        }
    }

    /// Whether the channel bends round the weir at point `i` so sharply
    /// that it has outflanked it: anywhere within twice [`WEIR_HOLD`]
    /// widths, it turns through more than 60° over a channel width. The
    /// river would otherwise fold against the reach the weir holds.
    fn outflanked(&self, i: usize) -> bool {
        let pts = &self.centerline;
        let n = pts.len();
        let r = 2 * self.reach_points(WEIR_HOLD);
        let unit = |k: usize| {
            let d = [pts[k + 1][0] - pts[k][0], pts[k + 1][1] - pts[k][1]];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-12);
            [d[0] / l, d[1] / l]
        };
        (i.saturating_sub(r)..(i + r).min(n.saturating_sub(3))).any(|k| {
            let (a, b) = (unit(k), unit(k + 2));
            a[0] * b[0] + a[1] * b[1] < 0.5
        })
    }

    /// Let a flood of magnitude `u` above [`MILL_FLOOD`] sweep away each
    /// weir with a chance that rises to 1 at magnitude 1.
    pub(crate) fn flood_mills(&mut self, u: f64) {
        if u <= MILL_FLOOD {
            return;
        }
        let chance = (u - MILL_FLOOD) / (1.0 - MILL_FLOOD);
        let time = self.time;
        for m in self.mills.iter_mut().filter(|m| m.working()) {
            if self.events.uniform() < chance {
                m.ruined_at = time;
                self.mill_version += 1;
            }
        }
    }

    /// With chance `mill_rate` per unit time, try to found a mill at a
    /// random point: the reach from [`LEAT_UP`] widths above to
    /// [`LEAT_DOWN`] below must be straight, with a chord at least
    /// [`MILL_STRAIGHT`] of its arc, most of the ground its leat runs on
    /// held by a holding, and no other working mill nearer than
    /// `mill_gap`.
    pub(crate) fn found_mill(&mut self) {
        let p = &self.params;
        let (w, n) = (p.width, self.centerline.len());
        if p.mill_rate <= 0.0 || self.events.uniform() >= p.mill_rate * p.dt {
            return;
        }
        let (up, down) = (self.reach_points(LEAT_UP), self.reach_points(LEAT_DOWN));
        if n < up + down + 3 {
            return;
        }
        let i = up + 1 + (self.events.uniform() * (n - up - down - 2) as f64) as usize;
        let side = if self.events.uniform() < 0.5 { -1.0 } else { 1.0 };
        let q = self.centerline[i];
        let margin = p.edge_margin * w;
        if q[0] < margin || q[0] > self.bounds[0] - margin {
            return;
        }
        let arc = geom::length(&self.centerline[i - up..=i + down]);
        if geom::dist(self.centerline[i - up], self.centerline[i + down]) < MILL_STRAIGHT * arc {
            return;
        }
        let gap = (p.mill_gap * w).powi(2);
        if self.mills.iter().any(|m| m.working() && geom::dist2(m.weir, q) < gap) {
            return;
        }
        let nrm = right_normal(&self.centerline, i);
        let mut mill = Mill {
            weir: q,
            across: [side * nrm[0], side * nrm[1]],
            leat: Vec::new(),
            course: self.course(i - up, i + down, side),
            born: self.time,
            ruined_at: f64::INFINITY,
            at: i,
        };
        self.rejoin(&mut mill);
        let (leat, field) = (&mill.leat, &self.erodibility);
        let owned = |c: P| self.land.owner.get(field.cell_index(c)).is_some_and(|&o| o != land::NONE);
        if (leat.iter().filter(|&&c| owned(c)).count() as f64) < LEAT_OWNED * leat.len() as f64 {
            return;
        }
        self.mills.push(mill);
        self.mill_version += 1;
    }

    /// The course a leat is dug along beside points `a` to `b`, on the
    /// side `side` of the right normal, [`LEAT_OFFSET`] widths beyond the
    /// bank.
    pub(crate) fn course(&self, a: usize, b: usize, side: f64) -> Vec<P> {
        let w = self.params.width;
        (a..=b)
            .map(|k| {
                let (q, nrm) = (self.centerline[k], right_normal(&self.centerline, k));
                let off = side * (0.5 * self.widths[k] + LEAT_OFFSET) * w;
                [q[0] + off * nrm[0], q[1] + off * nrm[1]]
            })
            .collect()
    }

    /// Lay mill `m`'s leat along its course, its ends turning off it over
    /// [`LEAT_RAMP`] widths to the bank as it now runs, from [`LEAT_UP`]
    /// widths above the weir to [`LEAT_DOWN`] below. Over a mill's
    /// centuries the reach above the weir migrates, and the miller digs the
    /// intake on to meet it. The leat is left as it was when the weir is
    /// too near either end of the channel to reach both banks. Returns
    /// whether the bank has reached the course where it runs at full
    /// offset, so the river has broken into the leat.
    pub(crate) fn rejoin(&self, m: &mut Mill) -> bool {
        let (w, n, len) = (self.params.width, self.centerline.len(), m.course.len());
        let up = self.reach_points(LEAT_UP);
        if len <= up || m.at < up || m.at - up + len > n {
            return false;
        }
        let a = m.at - up;
        let nrm = right_normal(&self.centerline, m.at);
        let side = (m.across[0] * nrm[0] + m.across[1] * nrm[1]).signum();
        let ramp = self.reach_points(LEAT_RAMP).max(1) as f64;
        let mut breached = false;
        m.leat.clear();
        m.leat.extend(m.course.iter().enumerate().map(|(j, &c)| {
            let k = a + j;
            let (q, nrm) = (self.centerline[k], right_normal(&self.centerline, k));
            let off = side * 0.5 * self.widths[k] * w;
            let bank = [q[0] + off * nrm[0], q[1] + off * nrm[1]];
            let t = smoothstep(j.min(len - 1 - j) as f64 / ramp);
            breached |= t >= 1.0 && side * ((c[0] - bank[0]) * nrm[0] + (c[1] - bank[1]) * nrm[1]) < 0.0;
            [bank[0] + t * (c[0] - bank[0]), bank[1] + t * (c[1] - bank[1])]
        }));
        breached
    }

    /// Put the point at each working weir back on it. Resampling cuts the
    /// corners of the line a little each step, which would otherwise carry
    /// the channel off a weir that holds it.
    pub(crate) fn pin_weirs(&mut self) {
        let n = self.centerline.len();
        for m in self.mills.iter().filter(|m| m.working() && m.at > 0 && m.at + 1 < n) {
            self.centerline[m.at] = m.weir;
        }
    }

    /// Apply the working weirs to this step's migration: hold the points
    /// within [`WEIR_HOLD`] widths of each, freeing them over as far
    /// again, and slow the reach above it,
    /// widen it and raise the silt a flood lays there, each falling to
    /// nothing at `pond_length`.
    pub(crate) fn pond_weirs(&mut self) {
        let n = self.rate.len();
        for v in [&mut self.held, &mut self.target, &mut self.boost] {
            v.clear();
            v.resize(n, 1.0);
        }
        let (hold, pond) = (self.reach_points(WEIR_HOLD), self.reach_points(self.params.pond_length).max(1));
        let backwater = self.params.weir_backwater;
        for m in self.mills.iter().filter(|m| m.working() && m.at < n) {
            for k in m.at.saturating_sub(pond)..m.at {
                let t = 1.0 - (m.at - k) as f64 / pond as f64;
                self.rate[k] *= 1.0 - backwater * t;
                self.target[k] = self.target[k].max(1.0 + POND_WIDEN * t);
                self.boost[k] = self.boost[k].max(1.0 + POND_SILT * t);
            }
            // Held within WEIR_HOLD, freed linearly over as far again, so
            // the hold leaves no kink at its ends.
            for k in m.at.saturating_sub(2 * hold)..=(m.at + 2 * hold).min(n - 1) {
                let free = (k.abs_diff(m.at) as f64 / hold.max(1) as f64 - 1.0).clamp(0.0, 1.0);
                self.held[k] = self.held[k].min(free);
            }
        }
    }
}
