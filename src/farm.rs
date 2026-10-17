//! Farmsteads: the buildings each holding works its ground from. The plans
//! follow Historic England's farmstead characterisation, read with R. W.
//! Brunskill as stages of one farmstead: a house gains an outshut, then a
//! barn, then ranges that close a yard. Buildings never grow. A farmstead
//! grows by extensions against a wall and by new buildings, one at each of
//! its holding's reorganisations, so it changes over centuries.
//!
//! Flood risk at the farmstead sets its plan. On safe ground it grows to a
//! courtyard of up to [`COURT_PLAN`]'s buildings, square to the holding's
//! long axis. On a levee crest, between the risk at which a farmstead is
//! founded and the risk that moves one, it keeps to a short row of
//! [`RIDGE_PLAN`]'s buildings along the bank, raised on mounds as the
//! Frisian *terpen* were. Wet meadow carries only a field barn.
//!
//! The farmstead follows the river through the risk. A building whose
//! ground grows too wet, or passes to another holding, is abandoned at a
//! reorganisation, the worst first, and new buildings go where the ground
//! is driest, so a farmstead creeps back from an advancing bank. The bank
//! takes any building it reaches. Abandoned buildings stand as ruins that
//! fade over [`RUIN_TIME`], as the house platforms of deserted villages do.

use crate::geom::P;
use crate::land::{FLEE_RISK, Land, NONE, SEAT_RISK, Settings};

/// Simulated time over which an abandoned building fades away: 60 units,
/// 150 years.
pub const RUIN_TIME: f64 = 60.0;
/// Highest flood risk at which a field barn stands; every other building
/// stands to `FLEE_RISK`.
const BARN_RISK: f32 = 0.8;
/// Least flood risk of meadow, and the share of a holding's ground that
/// must be meadow for it to keep a field barn there.
const MEADOW_RISK: f32 = 0.35;
const MEADOW_SHARE: f64 = 0.15;
/// Buildings in the order a farmstead raises them, on safe ground and on
/// a ridge. A cottage for a miller and a field barn come on top.
const COURT_PLAN: [Kind; 7] = [
    Kind::House,
    Kind::Barn,
    Kind::Outshut,
    Kind::Byre,
    Kind::Shed,
    Kind::Byre,
    Kind::Shed,
];
const RIDGE_PLAN: [Kind; 3] = [Kind::House, Kind::Barn, Kind::Shed];
/// Side of a courtyard's yard, the gap between buildings in a row, and
/// the least clearance between any two buildings, in channel widths.
const YARD: f64 = 1.6;
const ROW_GAP: f64 = 0.35;
const CLEAR: f64 = 0.15;
/// Weight of flood risk in a site's score, in widths of distance from
/// where the plan wants a building; the weight of a courtyard building
/// turned end on to its yard; and the range of the score's random part,
/// which keeps plans from repeating.
const RISK_WEIGHT: f64 = 6.0;
const FACING: f64 = 0.8;
const JITTER: f64 = 0.6;
/// Distance between the farmsteads of a hamlet, in channel widths.
pub(crate) const HAMLET_GAP: f64 = 4.0;
/// Cells sampled from a holding to site its field barn.
const BARN_SAMPLES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    House,
    /// A lean-to against the long wall of a house or barn.
    Outshut,
    Barn,
    Byre,
    Shed,
    /// A miller's cottage, while the holding has a working mill.
    Cottage,
    /// A hay barn out on the meadow.
    FieldBarn,
}

impl Kind {
    /// Footprint, along the ridge by across it, in channel widths. The
    /// house is the mill house's size.
    pub fn size(self) -> [f64; 2] {
        match self {
            Kind::House => [1.1, 0.7],
            Kind::Outshut => [0.9, 0.35],
            Kind::Barn => [1.5, 0.8],
            Kind::Byre => [1.0, 0.55],
            Kind::Shed => [0.8, 0.5],
            Kind::Cottage => [0.8, 0.55],
            Kind::FieldBarn => [0.7, 0.45],
        }
    }

    fn slot(self) -> usize {
        self as usize
    }
}

pub struct Building {
    /// Centre, in world units.
    pub pos: P,
    /// Unit vector along the ridge.
    pub dir: P,
    pub kind: Kind,
    /// Holding that keeps it, or [`NONE`] once abandoned.
    pub holding: u32,
    /// Time it was raised, which tells one building from another.
    pub built: f64,
    /// Time it was abandoned, or infinity while it stands.
    pub ruined: f64,
    /// Whether it stands on a mound, raised on ground above `SEAT_RISK`.
    pub mound: bool,
}

impl Building {
    pub fn standing(&self) -> bool {
        self.ruined.is_infinite()
    }

    fn rect(&self, w: f64) -> Rect {
        rect(self.pos, self.dir, self.kind, w)
    }
}

/// A footprint: centre, unit vector along the ridge, and half extents.
type Rect = (P, P, [f64; 2]);

fn rect(pos: P, dir: P, kind: Kind, w: f64) -> Rect {
    let s = kind.size();
    (pos, dir, [0.5 * s[0] * w, 0.5 * s[1] * w])
}

/// Whether two footprints come within `pad` of each other, by separating
/// axes.
fn overlap(a: Rect, b: Rect, pad: f64) -> bool {
    let d = [b.0[0] - a.0[0], b.0[1] - a.0[1]];
    let reach = |(_, u, h): Rect, n: P| {
        h[0] * (u[0] * n[0] + u[1] * n[1]).abs() + h[1] * (u[0] * n[1] - u[1] * n[0]).abs()
    };
    [a.1, [-a.1[1], a.1[0]], b.1, [-b.1[1], b.1[0]]]
        .into_iter()
        .all(|n| (d[0] * n[0] + d[1] * n[1]).abs() < reach(a, n) + reach(b, n) + pad)
}

fn dist2(a: P, b: P) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

impl Land {
    /// Abandon every standing building of holding `h`, or every one but
    /// its field barn if `keep_barn`.
    pub(crate) fn abandon(&mut self, h: usize, keep_barn: bool) {
        let (me, now) = (h as u32, self.now);
        let mut changed = false;
        for b in &mut self.buildings {
            if b.holding == me && !(keep_barn && b.kind == Kind::FieldBarn) {
                b.holding = NONE;
                b.ruined = now;
                changed = true;
            }
        }
        self.building_version += changed as u64;
    }

    /// Remove the buildings and ruins the water has reached, and the ruins
    /// that have faded. Runs at the end of a survey, when `dist` is whole.
    pub(crate) fn weather(&mut self) {
        let (cols, rows, c, now) = (self.cols, self.rows, self.cell, self.now);
        let dist = &self.dist;
        let before = self.buildings.len();
        self.buildings.retain(|b| {
            let x = ((b.pos[0] / c).max(0.0) as usize).min(cols - 1);
            let y = ((b.pos[1] / c).max(0.0) as usize).min(rows - 1);
            dist[y * cols + x] > 0.0 && now - b.ruined < RUIN_TIME
        });
        self.building_version += (self.buildings.len() != before) as u64;
    }

    /// Make one change to holding `h`'s farmstead at its reorganisation. A
    /// farmstead that has moved leaves its buildings and raises a house at
    /// its new seat. Otherwise the worst threatened building is abandoned,
    /// then one building is raised towards the plan's target, or one
    /// beyond it abandoned.
    pub(crate) fn build(&mut self, h: usize, s: &Settings) {
        let Some(g) = self.holdings[h].as_ref() else {
            return;
        };
        let (seat, axis, me, w) = (g.seat, g.axis, h as u32, s.width);
        let mine = |b: &Building| b.holding == me;
        let housed = self
            .buildings
            .iter()
            .any(|b| mine(b) && b.kind == Kind::House && dist2(b.pos, seat) < 0.0625 * w * w);
        if !housed {
            self.abandon(h, true);
            let dir = self.orientation(seat, axis, s);
            self.raise(Kind::House, seat, dir, me);
            return;
        }
        let threat = |l: &Land, b: &Building| {
            let i = l.index(b.pos);
            let limit = if b.kind == Kind::FieldBarn {
                BARN_RISK
            } else {
                FLEE_RISK
            };
            if l.owner[i] != me || !l.open[i] {
                Some(2.0)
            } else {
                (l.risk[i] > limit).then_some(l.risk[i])
            }
        };
        let worst = (0..self.buildings.len())
            .filter(|&k| mine(&self.buildings[k]) && self.buildings[k].kind != Kind::House)
            .filter_map(|k| threat(self, &self.buildings[k]).map(|r| (k, r)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, _)) = worst {
            self.ruin(k);
        }
        let want = self.wanted(h, s);
        let mut have = [0usize; 7];
        for b in self.buildings.iter().filter(|b| mine(b)) {
            have[b.kind.slot()] += 1;
        }
        if worst.is_none() {
            let surplus = (0..self.buildings.len())
                .filter(|&k| {
                    let b = &self.buildings[k];
                    mine(b) && b.kind != Kind::House && have[b.kind.slot()] > want[b.kind.slot()]
                })
                .max_by(|&a, &b| self.buildings[a].built.total_cmp(&self.buildings[b].built));
            if let Some(k) = surplus {
                self.ruin(k);
                return;
            }
        }
        let ridge = self.risk[self.index(seat)] > SEAT_RISK;
        let plan: &[Kind] = if ridge { &RIDGE_PLAN } else { &COURT_PLAN };
        let mut tally = [0usize; 7];
        let mut next = None;
        for &k in plan.iter().chain(&[Kind::Cottage, Kind::FieldBarn]) {
            tally[k.slot()] += 1;
            if tally[k.slot()] <= want[k.slot()] && tally[k.slot()] > have[k.slot()] {
                next = Some(k);
                break;
            }
        }
        let Some(kind) = next else { return };
        let site = if kind == Kind::FieldBarn {
            self.barn_site(h, axis, s)
        } else {
            let dir = self.orientation(seat, axis, s);
            self.site(kind, me, seat, dir, ridge, s)
        };
        if let Some((p, d)) = site {
            self.raise(kind, p, d, me);
        }
    }

    /// How many of each kind of building holding `h` wants: the first of
    /// its plan in proportion to its area, a cottage while it has a working
    /// mill, and a field barn while enough of its ground is meadow.
    fn wanted(&self, h: usize, s: &Settings) -> [usize; 7] {
        let g = self.holdings[h].as_ref().unwrap();
        let me = h as u32;
        let a = g.area as f64 / self.target(s).0;
        let (plan, n): (&[Kind], f64) = if self.risk[self.index(g.seat)] > SEAT_RISK {
            (&RIDGE_PLAN, 1.0 + 1.5 * a)
        } else {
            (&COURT_PLAN, 1.0 + 4.0 * a)
        };
        let n = (n.round() as usize).clamp(1, plan.len());
        let mut want = [0usize; 7];
        for k in &plan[..n] {
            want[k.slot()] += 1;
        }
        let mill = self.mills.iter().any(|&p| self.owner[self.index(p)] == me);
        want[Kind::Cottage.slot()] = mill as usize;
        let meadow = self
            .boxed(h)
            .filter(|&i| self.owner[i] == me && self.risk[i] >= MEADOW_RISK)
            .count();
        want[Kind::FieldBarn.slot()] = (meadow as f64 >= MEADOW_SHARE * g.area as f64) as usize;
        want
    }

    fn raise(&mut self, kind: Kind, pos: P, dir: P, me: u32) {
        let mound = self.risk[self.index(pos)] > SEAT_RISK;
        self.buildings.push(Building {
            pos,
            dir,
            kind,
            holding: me,
            built: self.now,
            ruined: f64::INFINITY,
            mound,
        });
        self.building_version += 1;
    }

    fn ruin(&mut self, k: usize) {
        let b = &mut self.buildings[k];
        b.holding = NONE;
        b.ruined = self.now;
        self.building_version += 1;
    }

    /// The direction a farmstead at `seat` is laid along: the holding's
    /// long axis on safe ground, and along the bank on a ridge, square to
    /// the rise of the flood risk.
    fn orientation(&self, seat: P, axis: P, s: &Settings) -> P {
        let i = self.index(seat);
        let (cols, k) = (self.cols, 2);
        let (x, y) = (i % cols, i / cols);
        if self.risk[i] <= SEAT_RISK || x < k || y < k || x + k >= cols || y + k >= self.rows {
            return axis;
        }
        let r = &self.risk;
        let (gx, gy) = (
            (r[i + k] - r[i - k]) as f64,
            (r[i + k * cols] - r[i - k * cols]) as f64,
        );
        let l = (gx * gx + gy * gy).sqrt();
        if l * s.width < 1e-4 {
            axis
        } else {
            [-gy / l, gx / l]
        }
    }

    /// The best site for a new building of `kind` on holding `me`: against
    /// one of the farmstead's buildings, end on in a row, or across a yard
    /// or square to it in a courtyard, or on a side of the courtyard's yard;
    /// an outshut against the long wall of a house or barn. A site must lie
    /// on the holding's open ground below `FLEE_RISK` and keep clear of
    /// every standing building. The score adds the flood risk, a random
    /// part and the distance from where the plan wants the building: in a
    /// row, from the seat; in a courtyard, from a side of the yard, which
    /// lies on the drier side of the house, with its long wall to the yard.
    fn site(
        &mut self,
        kind: Kind,
        me: u32,
        seat: P,
        dir: P,
        ridge: bool,
        s: &Settings,
    ) -> Option<(P, P)> {
        let w = s.width;
        let farm: Vec<(Kind, Rect)> = self
            .buildings
            .iter()
            .filter(|b| b.holding == me && b.kind != Kind::FieldBarn)
            .map(|b| (b.kind, b.rect(w)))
            .collect();
        // Each site, with the footprint it is built against, if any.
        let mut sites: Vec<(P, P, Option<Rect>)> = Vec::new();
        let half = rect(seat, dir, kind, w).2;
        let yard = (!ridge)
            .then(|| farm.iter().find(|f| f.0 == Kind::House))
            .flatten()
            .map(|&(_, (c, u, hb))| {
                let n = [-u[1], u[0]];
                let reach = hb[1] + 0.5 * YARD * w;
                let side = |t: f64| [c[0] + t * reach * n[0], c[1] + t * reach * n[1]];
                let y = if self.risk[self.index(side(1.0))] <= self.risk[self.index(side(-1.0))] {
                    side(1.0)
                } else {
                    side(-1.0)
                };
                (y, u, n)
            });
        if let Some((y, u, n)) = yard {
            let ring = 0.5 * YARD * w + half[1];
            for (a, d) in [(u, n), (n, u)] {
                for t in [-1.0, 1.0] {
                    for shift in [-0.5, 0.0, 0.5] {
                        let q = [
                            y[0] + t * ring * a[0] + shift * half[0] * d[0],
                            y[1] + t * ring * a[1] + shift * half[0] * d[1],
                        ];
                        sites.push((q, d, None));
                    }
                }
            }
        }
        for &(k, own) in &farm {
            let (c, u, hb) = own;
            let n = [-u[1], u[0]];
            let at = |a: P, t: f64| [c[0] + t * a[0], c[1] + t * a[1]];
            if kind == Kind::Outshut {
                if matches!(k, Kind::House | Kind::Barn) {
                    for side in [-1.0, 1.0] {
                        sites.push((at(n, side * (hb[1] + half[1])), u, Some(own)));
                    }
                }
                continue;
            }
            let dirs: &[P] = if ridge { &[u] } else { &[u, n] };
            for &d in dirs {
                let (along_u, along_n) = if d == u {
                    (half[0], half[1])
                } else {
                    (half[1], half[0])
                };
                for side in [-1.0, 1.0] {
                    sites.push((at(u, side * (hb[0] + ROW_GAP * w + along_u)), d, None));
                    if !ridge {
                        sites.push((at(u, side * (hb[0] + along_u)), d, Some(own)));
                        sites.push((at(n, side * (hb[1] + YARD * w + along_n)), d, None));
                    }
                }
            }
        }
        let (width, height) = (self.cols as f64 * self.cell, self.rows as f64 * self.cell);
        let mut best: Option<(f64, (P, P))> = None;
        for (p, d, against) in sites {
            let r = rect(p, d, kind, w);
            let (u, h) = (r.1, r.2);
            let n = [-u[1], u[0]];
            let mut risk = 0.0f32;
            let mut ok = true;
            for (a, b) in [
                (0.0, 0.0),
                (1.0, 1.0),
                (1.0, -1.0),
                (-1.0, 1.0),
                (-1.0, -1.0),
            ] {
                let q = [
                    p[0] + a * h[0] * u[0] + b * h[1] * n[0],
                    p[1] + a * h[0] * u[1] + b * h[1] * n[1],
                ];
                if q[0] < 0.0 || q[1] < 0.0 || q[0] >= width || q[1] >= height {
                    ok = false;
                    break;
                }
                let i = self.index(q);
                if self.owner[i] != me || !self.open[i] || self.risk[i] > FLEE_RISK {
                    ok = false;
                    break;
                }
                risk = risk.max(self.risk[i]);
            }
            let blocked = self.buildings.iter().any(|b| {
                let o = b.rect(w);
                let pad = if against == Some(o) {
                    -0.01 * w
                } else {
                    CLEAR * w
                };
                b.standing() && overlap(o, r, pad)
            });
            if !ok || blocked {
                continue;
            }
            let off = match yard {
                Some((y, ..)) => {
                    let (dx, dy) = (p[0] - y[0], p[1] - y[1]);
                    let r = (dx * dx + dy * dy).sqrt().max(1e-9);
                    let facing = ((dx * d[0] + dy * d[1]) / r).abs();
                    (r - 0.5 * YARD * w - half[1]).abs() / w + FACING * facing
                }
                None => dist2(p, seat).sqrt() / w,
            };
            let score = off + RISK_WEIGHT * risk as f64 + JITTER * self.farm_rng.uniform();
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, (p, d)));
            }
        }
        best.map(|(_, site)| site)
    }

    /// A site for holding `h`'s field barn: of cells sampled from its
    /// meadow, the one whose risk is nearest halfway between meadow and
    /// `BARN_RISK`, laid along the holding's axis.
    fn barn_site(&mut self, h: usize, axis: P, s: &Settings) -> Option<(P, P)> {
        let me = h as u32;
        let [x0, y0, x1, y1] = self.holdings[h].as_ref()?.bounds;
        let mid = 0.5 * (MEADOW_RISK + BARN_RISK);
        let mut best: Option<(f32, usize)> = None;
        for _ in 0..BARN_SAMPLES {
            let x = x0 + (self.farm_rng.uniform() * (x1 - x0 + 1) as f64) as usize;
            let y = y0 + (self.farm_rng.uniform() * (y1 - y0 + 1) as f64) as usize;
            let i = y.min(y1) * self.cols + x.min(x1);
            let r = self.risk[i];
            if self.owner[i] != me || !self.open[i] || !(MEADOW_RISK..=BARN_RISK).contains(&r) {
                continue;
            }
            if best.is_none_or(|(b, _)| (r - mid).abs() < b) {
                best = Some(((r - mid).abs(), i));
            }
        }
        let p = self.centre(best?.1);
        let r = rect(p, axis, Kind::FieldBarn, s.width);
        let clear = !self
            .buildings
            .iter()
            .any(|b| b.standing() && overlap(b.rect(s.width), r, CLEAR * s.width));
        clear.then_some((p, axis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprints_overlap_only_when_they_meet() {
        let x = [1.0, 0.0];
        let a = rect([0.0, 0.0], x, Kind::House, 1.0);
        assert!(overlap(a, rect([1.0, 0.0], x, Kind::House, 1.0), 0.0));
        assert!(!overlap(a, rect([1.2, 0.0], x, Kind::House, 1.0), 0.0));
        assert!(overlap(a, rect([1.2, 0.0], x, Kind::House, 1.0), 0.15));
        // Turned square, a house reaches 0.35 along x.
        assert!(!overlap(
            a,
            rect([0.95, 0.0], [0.0, 1.0], Kind::House, 1.0),
            0.0
        ));
        assert!(overlap(
            a,
            rect([0.85, 0.0], [0.0, 1.0], Kind::House, 1.0),
            0.0
        ));
    }
}
