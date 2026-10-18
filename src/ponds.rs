//! Fishponds: the *stews* in which manors and monasteries kept carp,
//! bream and pike for the many fast days of the medieval year. They were
//! dug where the ground held water, often in an old channel, and set in a
//! chain along it, each pond held by an earth dam from the next.
//!
//! A farmstead whose plan is complete digs a chain of up to
//! [`POND_CHAIN`] ponds, with chance [`POND_CHANCE`] at a reorganisation,
//! into the clay plug of a filled oxbow within [`POND_REACH`] of its seat.
//! Each pond is a rectangle laid along the old channel, so the chain bends
//! with it. Ponds stand on the plug, which no holding takes, so a pond
//! belongs to the holding that dug it until that holding is gone or its
//! seat moves beyond [`POND_KEEP`]. An abandoned pond silts to marsh over
//! [`POND_RUIN_TIME`], and the channel takes any pond it sweeps. Ponds
//! draw from a stream of their own and change nothing else.

use crate::farm::{Rect, dist2, on_stream, overlap};
use crate::field::{Erodibility, LAKE_DEPTH};
use crate::geom::P;
use crate::land::{Land, NONE, Settings};

/// Size of a pond, along the old channel by across it, and the length of
/// the dam between two ponds of a chain, in channel widths.
pub const POND: [f64; 2] = [1.3, 0.5];
const POND_DAM: f64 = 0.3;
/// Most ponds in a chain, and the chance a complete farmstead digs one.
const POND_CHAIN: usize = 4;
const POND_CHANCE: f64 = 0.5;
/// Farthest from its seat a holding digs a pond, and the farthest its seat
/// can move from the pond and keep it, in channel widths.
const POND_REACH: f64 = 12.0;
const POND_KEEP: f64 = 16.0;
/// Scar points tried as a chain's first pond, nearest the seat first.
const POND_TRIES: usize = 6;
/// Simulated time over which an abandoned pond silts up: 100 units, 250
/// years.
pub const POND_RUIN_TIME: f64 = 100.0;
/// Least clearance between a pond and a building, in channel widths.
const POND_CLEAR: f64 = 0.15;
/// Overbank deposit, in channel depths, above which an oxbow's ground is
/// firm enough to dig: no longer the open water of `tracks.rs`.
const PLUG_FIRM: f32 = -0.5 * LAKE_DEPTH as f32;

pub struct Pond {
    /// Centre, in world units.
    pub pos: P,
    /// Unit vector along the old channel.
    pub dir: P,
    /// Holding that keeps it, or [`NONE`] once abandoned.
    pub holding: u32,
    /// Time it was abandoned, or infinity while it is kept.
    pub ruined: f64,
}

impl Pond {
    pub fn kept(&self) -> bool {
        self.ruined.is_infinite()
    }

    fn rect(&self, w: f64) -> Rect {
        (self.pos, self.dir, [0.5 * POND[0] * w, 0.5 * POND[1] * w])
    }
}

/// The point `s` world units along `pts` from point `k`, towards the end
/// if `ahead` and towards the start if not, or `None` past the end.
fn along(pts: &[P], k: usize, ahead: bool, s: f64) -> Option<P> {
    let (mut at, mut left) = (k, s);
    loop {
        let next = if ahead {
            (at + 1 < pts.len()).then_some(at + 1)?
        } else {
            at.checked_sub(1)?
        };
        let (a, b) = (pts[at], pts[next]);
        let l = dist2(a, b).sqrt();
        if l >= left {
            let t = if l > 1e-9 { left / l } else { 0.0 };
            return Some([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
        }
        left -= l;
        at = next;
    }
}

impl Land {
    /// Hand over the courses of the oxbows whose lakes have filled, as
    /// points back to back with the start of each.
    pub(crate) fn set_scars<'a>(&mut self, scars: impl Iterator<Item = &'a [P]>) {
        self.scar_points.clear();
        self.scar_starts.clear();
        for pts in scars {
            self.scar_starts.push(self.scar_points.len());
            self.scar_points.extend_from_slice(pts);
        }
        self.scar_starts.push(self.scar_points.len());
    }

    /// Whether footprint `r` comes within `pad` of a pond still kept or
    /// silting.
    pub(crate) fn on_ponds(&self, r: Rect, pad: f64, w: f64) -> bool {
        self.ponds.iter().any(|p| overlap(p.rect(w), r, pad))
    }

    /// Whether `p` lies on an oxbow's plug firm enough to dig, which the
    /// channel did not cover when the survey began, in the columns the
    /// holdings can take.
    fn diggable(&self, field: &Erodibility, p: P) -> bool {
        let (width, height) = (self.cols as f64 * self.cell, self.rows as f64 * self.cell);
        if p[0] < 0.0 || p[1] < 0.0 || p[0] >= width || p[1] >= height {
            return false;
        }
        let i = self.index(p);
        field.cut_at[i].is_finite()
            && field.deposit[i] >= PLUG_FIRM
            && !self.in_channel(i)
            && self.within(i)
    }

    /// Dig a chain of fishponds for holding `h`, if it keeps none and a
    /// draw falls under [`POND_CHANCE`]: from the scar point nearest its
    /// seat, along the scar either way, as many ponds as fit on the plug up
    /// to a chain's drawn length. Each pond's centre, ends and sides must
    /// lie on diggable plug, clear of buildings, streams and other ponds.
    pub(crate) fn dig_ponds(&mut self, h: usize, s: &Settings, field: &Erodibility) {
        let Some(seat) = self.holdings[h].as_ref().map(|g| g.seat) else {
            return;
        };
        let (me, w) = (h as u32, s.width);
        if self.ponds.iter().any(|p| p.holding == me) || self.works_rng.uniform() >= POND_CHANCE {
            return;
        }
        let chain = 1 + (self.works_rng.uniform() * POND_CHAIN as f64) as usize;
        let reach = (POND_REACH * w).powi(2);
        let mut near: Vec<(f64, usize, usize)> = Vec::new();
        for a in 0..self.scar_starts.len().saturating_sub(1) {
            for k in self.scar_starts[a]..self.scar_starts[a + 1] {
                let d = dist2(self.scar_points[k], seat);
                if d < reach {
                    near.push((d, a, k));
                }
            }
        }
        near.sort_by(|x, y| x.0.total_cmp(&y.0));
        let (step, half) = ((POND[0] + POND_DAM) * w, 0.5 * POND[0] * w);
        let mut tried: Vec<P> = Vec::new();
        for &(_, a, k) in &near {
            if tried.len() >= POND_TRIES {
                break;
            }
            let start = self.scar_points[k];
            if tried.iter().any(|&q| dist2(q, start) < step * step) {
                continue;
            }
            tried.push(start);
            let pts = &self.scar_points[self.scar_starts[a]..self.scar_starts[a + 1]];
            let k = k - self.scar_starts[a];
            for ahead in [true, false] {
                let mut dug: Vec<Pond> = Vec::new();
                for n in 0..chain {
                    let from = n as f64 * step;
                    let ends = (
                        along(pts, k, ahead, from),
                        along(pts, k, ahead, from + POND[0] * w),
                    );
                    let (Some(p0), Some(p1)) = ends else { break };
                    let l = dist2(p0, p1).sqrt();
                    if l < 0.5 * POND[0] * w {
                        break;
                    }
                    let dir = [(p1[0] - p0[0]) / l, (p1[1] - p0[1]) / l];
                    let pos = [0.5 * (p0[0] + p1[0]), 0.5 * (p0[1] + p1[1])];
                    let pond = Pond {
                        pos,
                        dir,
                        holding: me,
                        ruined: f64::INFINITY,
                    };
                    let (side, r) = (0.5 * POND[1] * w, pond.rect(w));
                    let n = [-dir[1], dir[0]];
                    let firm = [
                        (0.0, 0.0),
                        (half, 0.0),
                        (-half, 0.0),
                        (0.0, side),
                        (0.0, -side),
                    ]
                    .iter()
                    .all(|&(u, v)| {
                        let q = [
                            pos[0] + u * dir[0] + v * n[0],
                            pos[1] + u * dir[1] + v * n[1],
                        ];
                        self.diggable(field, q)
                    });
                    let blocked = (self.buildings.iter())
                        .any(|b| b.standing() && overlap(b.rect(w), r, POND_CLEAR * w))
                        || on_stream(&self.streams, r, POND_CLEAR * w)
                        || self.on_ponds(r, POND_CLEAR * w, w);
                    if !firm || blocked {
                        break;
                    }
                    dug.push(pond);
                }
                if !dug.is_empty() {
                    self.ponds.extend(dug);
                    self.building_version += 1;
                    return;
                }
            }
        }
    }

    /// Abandon each pond whose holding is gone or has moved its seat beyond
    /// [`POND_KEEP`], then remove the ponds that have silted up or that the
    /// channel has reached. Runs at the end of a survey.
    pub(crate) fn weather_ponds(&mut self, field: &Erodibility, w: f64) {
        let (now, keep) = (self.now, (POND_KEEP * w).powi(2));
        let mut changed = false;
        for k in 0..self.ponds.len() {
            let p = &self.ponds[k];
            if !p.kept() {
                continue;
            }
            let seat = self
                .holdings
                .get(p.holding as usize)
                .and_then(|g| g.as_ref())
                .map(|g| g.seat);
            if seat.is_none_or(|q| dist2(q, p.pos) > keep) {
                let p = &mut self.ponds[k];
                (p.holding, p.ruined) = (NONE, now);
                changed = true;
            }
        }
        let before = self.ponds.len();
        let mut ponds = std::mem::take(&mut self.ponds);
        ponds.retain(|p| now - p.ruined < POND_RUIN_TIME && self.diggable(field, p.pos));
        self.ponds = ponds;
        self.building_version += (changed || self.ponds.len() != before) as u64;
    }
}
