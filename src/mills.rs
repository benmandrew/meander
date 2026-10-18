//! Water mills. A holding builds a mill on a straight reach beside its
//! ground: a weir across the channel and a leat along the bank that
//! carries water to the wheel and back. The weir's masonry holds the banks
//! beside it, and the pond it holds back slows the reach above it, widens
//! it, and drops more silt on its banks. A mill is left as a ruin when the
//! channel cuts away from its weir or breaks into its leat.

use crate::geom::{self, P, right_normal};
use crate::{
    LEAT_DOWN, LEAT_OFFSET, LEAT_UP, MILL_RUIN, MILL_SNAP, MILL_STRAIGHT, OUTFLANK_COS, POND_SILT,
    POND_WIDEN, Sim, WEIR_HOLD, land, smoothstep,
};

/// Distance over which a leat leaves the bank at either end, in channel
/// widths.
const LEAT_RAMP: f64 = 2.0;
/// Share of a leat's length that must run on a holding's ground; its ends
/// may cross the bar at the bank.
const LEAT_OWNED: f64 = 0.75;
/// Centreline points on either side of a leat point's last foot searched
/// for its new one.
const FOOT_SEARCH: usize = 3;
/// Share of the way a working leat's points move each step along the
/// bank to where it now puts them. A sharp bend is drawn in a few points,
/// which resampling shifts each step, so the bank at it can swing by half
/// a width; eased, the leat's end goes nine tenths of the way in 8 steps,
/// 2 years. Across the bank they move the whole way, so the ends keep to
/// it.
const LEAT_EASE: f64 = 0.25;

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
    /// The centreline segment nearest each course point, as an offset in
    /// points from [`Mill::at`]. Kept from step to step, so a point's bank
    /// stays on its own reach where a bend brings another one as near.
    pub feet: Vec<isize>,
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
    /// widths, it turns through more than the angle whose cosine is
    /// [`OUTFLANK_COS`] over a channel
    /// width. The river would otherwise fold against the reach the weir
    /// holds, and draw a corner at it.
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
            a[0] * b[0] + a[1] * b[1] < OUTFLANK_COS
        })
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
        let side = if self.events.uniform() < 0.5 {
            -1.0
        } else {
            1.0
        };
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
        if self
            .mills
            .iter()
            .any(|m| m.working() && geom::dist2(m.weir, q) < gap)
        {
            return;
        }
        let nrm = right_normal(&self.centerline, i);
        let mut mill = Mill {
            weir: q,
            across: [side * nrm[0], side * nrm[1]],
            leat: Vec::new(),
            course: self.course(i - up, i + down, side),
            feet: Vec::new(),
            born: self.time,
            ruined_at: f64::INFINITY,
            at: i,
        };
        self.rejoin(&mut mill);
        let (leat, field) = (&mill.leat, &self.erodibility);
        let owned = |c: P| {
            self.land
                .owner
                .get(field.cell_index(c))
                .is_some_and(|&o| o != land::NONE)
        };
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
    /// too near either end of the channel to reach both banks. Once laid,
    /// it moves [`LEAT_EASE`] of the way along the bank to its new line
    /// each call. Returns whether the bank has reached the course where it
    /// runs at full offset, so the river has broken into the leat.
    pub(crate) fn rejoin(&self, m: &mut Mill) -> bool {
        let (w, n, len) = (self.params.width, self.centerline.len(), m.course.len());
        let up = self.reach_points(LEAT_UP);
        if len <= up || m.at < up || m.at - up + len > n {
            return false;
        }
        if m.feet.len() != len {
            m.feet = (0..len).map(|j| j as isize - up as isize).collect();
        }
        let nrm = right_normal(&self.centerline, m.at);
        let side = (m.across[0] * nrm[0] + m.across[1] * nrm[1]).signum();
        let ramp = self.reach_points(LEAT_RAMP).max(1) as f64;
        let mut breached = false;
        let ease = if m.leat.len() == len { LEAT_EASE } else { 1.0 };
        m.leat.resize(len, [0.0; 2]);
        for (j, &c) in m.course.iter().enumerate() {
            let k = (m.at as isize + m.feet[j]).max(0) as usize;
            let (seg, q, nrm, width) = self.foot(c, k);
            m.feet[j] = seg as isize - m.at as isize;
            let off = side * 0.5 * width * w;
            let bank = [q[0] + off * nrm[0], q[1] + off * nrm[1]];
            let t = smoothstep(j.min(len - 1 - j) as f64 / ramp);
            breached |=
                t >= 1.0 && side * ((c[0] - bank[0]) * nrm[0] + (c[1] - bank[1]) * nrm[1]) < 0.0;
            let to = [
                bank[0] + t * (c[0] - bank[0]),
                bank[1] + t * (c[1] - bank[1]),
            ];
            let p = &mut m.leat[j];
            let d = [to[0] - p[0], to[1] - p[1]];
            let across = d[0] * nrm[0] + d[1] * nrm[1];
            let along = [d[0] - across * nrm[0], d[1] - across * nrm[1]];
            *p = [
                p[0] + across * nrm[0] + ease * along[0],
                p[1] + across * nrm[1] + ease * along[1],
            ];
        }
        breached
    }

    /// The segment of the centreline within [`FOOT_SEARCH`] points of
    /// point `k` nearest `c`, the point on it nearest `c`, and the right
    /// normal and width there, each interpolated between the segment's
    /// ends. Resampling slides the points along the line each step, so a
    /// bank laid at a point's index would jump along the bank by up to a
    /// spacing; the nearest point moves only with the channel. The search
    /// runs on the line smoothed over two points on either side, since a
    /// kink in it would swing the foot of a course point a few widths off
    /// along the bank, and its normal with it. The leat lies under the
    /// water, so an end a little inside the bank is hidden.
    fn foot(&self, c: P, k: usize) -> (usize, P, P, f64) {
        let pts = &self.centerline;
        let last = pts.len().saturating_sub(5).max(3);
        let lo = k.saturating_sub(FOOT_SEARCH).clamp(3, last);
        let hi = (k + FOOT_SEARCH).clamp(lo, last);
        // Binomial weights 1, 4, 6, 4, 1 over points i − 2 to i + 2, for
        // points lo − 1 to hi + 2.
        let mut sm = [[0.0; 2]; 2 * FOOT_SEARCH + 4];
        for (j, s) in sm.iter_mut().enumerate().take(hi - lo + 4) {
            let i = lo - 1 + j;
            for d in 0..2 {
                s[d] = (pts[i - 2][d]
                    + 4.0 * (pts[i - 1][d] + pts[i + 1][d])
                    + 6.0 * pts[i][d]
                    + pts[i + 2][d])
                    / 16.0;
            }
        }
        let (mut best, mut seg, mut at) = (f64::INFINITY, lo, 0.0);
        for i in lo..=hi {
            let (p, q) = (sm[i - lo + 1], sm[i - lo + 2]);
            let d = [q[0] - p[0], q[1] - p[1]];
            let len2 = d[0] * d[0] + d[1] * d[1];
            let u = if len2 > 0.0 {
                (((c[0] - p[0]) * d[0] + (c[1] - p[1]) * d[1]) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let e = geom::dist2(c, [p[0] + u * d[0], p[1] + u * d[1]]);
            if e < best {
                (best, seg, at) = (e, i, u);
            }
        }
        let j = seg - lo + 1;
        let (p, q) = (sm[j], sm[j + 1]);
        let (n0, n1) = (right_normal(&sm, j), right_normal(&sm, j + 1));
        let nrm = [n0[0] + at * (n1[0] - n0[0]), n0[1] + at * (n1[1] - n0[1])];
        let l = (nrm[0] * nrm[0] + nrm[1] * nrm[1]).sqrt().max(1e-12);
        (
            seg,
            [p[0] + at * (q[0] - p[0]), p[1] + at * (q[1] - p[1])],
            [nrm[0] / l, nrm[1] / l],
            self.widths[seg] + at * (self.widths[seg + 1] - self.widths[seg]),
        )
    }

    /// Put the point at each working weir back on it. Resampling cuts the
    /// corners of the line a little each step, which would otherwise carry
    /// the channel off a weir that holds it.
    pub(crate) fn pin_weirs(&mut self) {
        let n = self.centerline.len();
        for m in self
            .mills
            .iter()
            .filter(|m| m.working() && m.at > 0 && m.at + 1 < n)
        {
            self.centerline[m.at] = m.weir;
        }
    }

    /// Apply the working weirs to this step's migration: hold the points
    /// within [`WEIR_HOLD`] widths of each, freeing them over as far
    /// again, and slow the reach above it,
    /// widen it and raise the overbank silt laid there, each falling to
    /// nothing at `pond_length`.
    pub(crate) fn pond_weirs(&mut self) {
        let n = self.rate.len();
        for v in [&mut self.held, &mut self.target, &mut self.boost] {
            v.clear();
            v.resize(n, 1.0);
        }
        let (hold, pond) = (
            self.reach_points(WEIR_HOLD),
            self.reach_points(self.params.pond_length).max(1),
        );
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
