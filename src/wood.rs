//! Large wood. Trees fall from mature forest on an eroding outer bank and
//! lodge against it as jams, which push the flow off that bank and onto
//! the other, until they rot or a flood breaks them up. A farmed bank, with
//! only its hedgerow and bankside trees, gives [`FARM_WOOD`] of the wood.

use crate::geom::{self, P, right_normal};
use crate::{BANK_BEYOND, FARM_WOOD, JAM_MERGE, JAM_REACH, MAX_JAMS, Sim, land};

/// Size a single tree adds to a jam; a jam of full size is 1.
const TREE: f64 = 0.35;
/// Fraction of a jam's life over which it fades as it breaks up.
const JAM_FADE: f64 = 0.25;
/// Smoothing passes over the summed push of the jams.
const JAM_SMOOTH: usize = 4;
/// Distance from the centreline within which a jam keeps to the channel,
/// in channel widths; past it the bank has left the jam on the floodplain.
const JAM_HOLD: f64 = 1.5;
/// Fraction of the half-width from the centreline to its bank within which
/// a jam is in mid-channel, where the current lifts it off and carries it
/// out of the reach.
const JAM_STRAND: f64 = 0.5;

/// A jam of large wood against a bank. It stays where it lodged, anchored
/// by its root wads and buried ends, while the channel moves about it.
#[derive(Clone, Debug)]
pub struct Jam {
    /// Where it lodged, on the bank.
    pub pos: P,
    /// Unit vector downstream where it lodged, along which its logs lie.
    pub down: P,
    /// The bank it lies against: +1 along the channel's right normal, −1
    /// along its left.
    pub side: f64,
    /// Size, in (0, 1].
    pub size: f64,
    /// Time it breaks up.
    pub until: f64,
    /// Centreline point it lies beside, kept up to date by [`Sim::track`].
    pub at: usize,
}

impl Jam {
    /// Push the jam gives the flow at `time`: its size, fading out over
    /// the last [`JAM_FADE`] of its life.
    pub fn strength(&self, time: f64, lifetime: f64) -> f64 {
        self.size * ((self.until - time) / (JAM_FADE * lifetime).max(1e-9)).clamp(0.0, 1.0)
    }
}

impl Sim {
    /// Points along the channel either side of a feature over which
    /// `reach` channel widths extend.
    pub(crate) fn reach_points(&self, reach: f64) -> usize {
        (reach / crate::SPACING).round() as usize
    }

    /// Point each jam at the centreline point now nearest it, dropping
    /// those the channel has left on the floodplain and those it has run
    /// over, which the current carries away.
    pub(crate) fn track_jams(&mut self) {
        if self.jams.is_empty() {
            return;
        }
        let w = self.params.width;
        let g = self.build_grid(JAM_HOLD * w);
        let mut jams = std::mem::take(&mut self.jams);
        jams.retain_mut(|j| {
            let Some(i) = self.nearest_point(j.pos, JAM_HOLD * w, g) else {
                return false;
            };
            let n = self.centerline.len();
            if i == 0 || i + 1 >= n {
                return false;
            }
            let (q, nrm) = (self.centerline[i], right_normal(&self.centerline, i));
            // Distance out from the centreline towards the jam's bank.
            let out = j.side * ((j.pos[0] - q[0]) * nrm[0] + (j.pos[1] - q[1]) * nrm[1]);
            j.at = i;
            out > JAM_STRAND * 0.5 * self.widths[i] * w
        });
        self.jams = jams;
    }

    /// Drop trees from mature forest on the banks the last migration
    /// eroded, as a Poisson draw on the eroded length weighted by the
    /// forest's maturity, and let jams past their time break up.
    pub(crate) fn recruit_wood(&mut self) {
        let time = self.time;
        self.jams.retain(|j| j.until > time);
        let p = &self.params;
        let (w, n) = (p.width, self.centerline.len());
        if p.wood_rate <= 0.0 || p.forest_time <= 0.0 || self.shift.len() != n {
            return;
        }
        let field = &self.erodibility;
        let mut total = 0.0;
        self.weights.clear();
        for i in 0..n {
            let d = self.shift[i];
            let mut weight = 0.0;
            if d != 0.0 && i > 0 && i + 1 < n {
                let (q, nrm) = (self.centerline[i], right_normal(&self.centerline, i));
                let off = d.signum() * (0.5 * self.widths[i] + BANK_BEYOND) * w;
                let bank = [q[0] + off * nrm[0], q[1] + off * nrm[1]];
                let maturity =
                    ((field.growth_at(bank) - p.vegetation_time) / p.forest_time).clamp(0.0, 1.0);
                if maturity > 0.0 {
                    let farmed = self
                        .land
                        .owner
                        .get(field.cell_index(bank))
                        .is_some_and(|&o| o != land::NONE);
                    weight = maturity * if farmed { FARM_WOOD } else { 1.0 } * d.abs() / w;
                }
            }
            total += weight;
            self.weights.push(total);
        }
        let expected = p.wood_rate * total;
        if expected <= 0.0 || self.events.uniform() >= expected {
            return;
        }
        let pick = self.events.uniform() * total;
        let i = self
            .weights
            .partition_point(|&c| c < pick)
            .min(n - 2)
            .max(1);
        let side = self.shift[i].signum();
        let (q, nrm) = (self.centerline[i], right_normal(&self.centerline, i));
        let off = side * 0.5 * self.widths[i] * w;
        let pos = [q[0] + off * nrm[0], q[1] + off * nrm[1]];
        let life = p.jam_lifetime;
        // A tree joins a jam already near it on either bank: jams pushing
        // from both banks close together would kink the channel.
        let merge = (JAM_MERGE * w).powi(2);
        if let Some(j) = self
            .jams
            .iter_mut()
            .find(|j| geom::dist2(j.pos, pos) < merge)
        {
            j.size = (j.size + TREE).min(1.0);
            j.until = j.until.max(time + life * j.size);
        } else if self.jams.len() < MAX_JAMS {
            let down = [-nrm[1], nrm[0]];
            self.jams.push(Jam {
                pos,
                down,
                side,
                size: TREE,
                until: time + life * TREE,
                at: i,
            });
        }
    }

    /// Let a flood of magnitude `u` flush each jam with probability `u`.
    pub(crate) fn flush_jams(&mut self, u: f64) {
        let mut jams = std::mem::take(&mut self.jams);
        jams.retain(|_| self.events.uniform() >= u);
        self.jams = jams;
    }

    /// Add each jam's push to the migration rate: away from its bank,
    /// falling to 0 [`JAM_REACH`] widths along the channel either side.
    /// Jams on one reach push no harder together than one jam of full
    /// size, so a cluster of them cannot kink the channel into a loop.
    pub(crate) fn push_jams(&mut self) {
        let (n, r) = (self.rate.len(), self.reach_points(JAM_REACH));
        let (push, life) = (self.params.jam_push, self.params.jam_lifetime);
        if self.jams.is_empty() || push <= 0.0 {
            return;
        }
        self.weights.clear();
        self.weights.resize(n, 0.0);
        for j in &self.jams {
            if j.at >= n {
                continue;
            }
            let s = j.side * j.strength(self.time, life);
            for k in j.at.saturating_sub(r)..=(j.at + r).min(n - 1) {
                let t = 1.0 - k.abs_diff(j.at) as f64 / (r + 1) as f64;
                self.weights[k] += s * t * t * (3.0 - 2.0 * t);
            }
        }
        // Smooth the push along the channel, so jams near one another
        // change it gently.
        for _ in 0..JAM_SMOOTH {
            let mut last = self.weights[0];
            for k in 1..n.saturating_sub(1) {
                let here = self.weights[k];
                self.weights[k] = 0.25 * last + 0.5 * here + 0.25 * self.weights[k + 1];
                last = here;
            }
        }
        for (rate, s) in self.rate.iter_mut().zip(&self.weights) {
            *rate -= push * s.clamp(-1.0, 1.0);
        }
    }

    /// Strength of the strongest jam within [`JAM_REACH`] widths along the
    /// channel of point `i`.
    pub(crate) fn jam_near(&self, i: usize) -> f64 {
        let (r, life) = (self.reach_points(JAM_REACH), self.params.jam_lifetime);
        self.jams
            .iter()
            .filter(|j| j.at.abs_diff(i) <= r)
            .fold(0.0, |m, j| m.max(j.strength(self.time, life)))
    }
}
