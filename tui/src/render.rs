//! The river as a grid of pixels, two to a terminal cell: the top one in
//! the foreground colour of an upper half block, the bottom one in the
//! background. Ground is sampled from the core's grids at each pixel's
//! centre, water and tracks are stamped over it, and buildings and mills
//! take whole cells as glyphs. Colours follow the page's (`web/main.js`).
//!
//! Ground and water are painted `SUPERSAMPLE` times finer along each axis,
//! and each pixel resolves its samples as an edge between two colours, so
//! edges are antialiased over less than a pixel.

use meander_core::farm::Kind;
use meander_core::geom::{P, dist, seg_dist2};
use meander_core::land::NONE;
use meander_core::{FAN_MAX_DEPTH, Sim, Tributary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

type Rgb = [f32; 3];

/// Samples along each axis of a pixel.
const SUPERSAMPLE: usize = 3;
/// Factor by which a pixel's share of each colour at an edge is pushed
/// towards 0 or 1. At 1 an edge ramps over a whole pixel, which reads as
/// blur at a terminal's pixel size; at 2 it ramps over half of one.
const SHARPEN: f32 = 2.0;
/// Width of tracks, streams and the parish boundary, and of hedges, in
/// pixels. Thinner lines straddling two pixels fade to half strength in
/// both.
const LINE: f64 = 1.2;
const HEDGE_WIDTH: f64 = 1.25;

const SOIL: [Rgb; 4] = [
    [160.0, 158.0, 120.0],
    [182.0, 182.0, 140.0],
    [196.0, 196.0, 152.0],
    [226.0, 214.0, 166.0],
];
const LOAM: Rgb = SOIL[2];
const GRAVEL: Rgb = [200.0, 194.0, 172.0];
const LEVEE: Rgb = [216.0, 210.0, 166.0];
const BAR: Rgb = [233.0, 222.0, 186.0];
const FOREST: [Rgb; 3] = [
    [150.0, 170.0, 104.0],
    [112.0, 140.0, 84.0],
    [84.0, 116.0, 70.0],
];
/// Share of `forest_time` at which young and mature forest are reached.
const FOREST_AT: [f64; 2] = [0.35, 0.85];
const UPLAND: Rgb = [190.0, 182.0, 132.0];
const SCARP_SHADE: Rgb = [96.0, 100.0, 66.0];
const SCARP_LIGHT: Rgb = [244.0, 236.0, 200.0];
/// Depth of the shaded scarp above each wall foot, in channel widths.
const SCARP: f64 = 3.0;
const SCAR_WET: Rgb = [92.0, 114.0, 80.0];
const MEADOWS: [Rgb; 2] = [[146.0, 168.0, 104.0], [184.0, 188.0, 128.0]];
const ARABLE: [Rgb; 3] = [
    [168.0, 182.0, 110.0],
    [212.0, 198.0, 138.0],
    [166.0, 146.0, 108.0],
];
/// Flood risk from which a holding's ground is meadow, as on the page.
const MEADOW_RISK: f32 = 0.35;
/// Width of a holding's field strips, in channel widths.
const STRIP: f64 = 7.0;
const HEDGE: Rgb = [84.0, 116.0, 70.0];
const TRACK: Rgb = [206.0, 186.0, 138.0];
const BOUNDARY: Rgb = [86.0, 70.0, 92.0];
const WATER: Rgb = [61.0, 126.0, 166.0];
const CLEAR: Rgb = [34.0, 84.0, 116.0];
const MURKY: Rgb = [80.0, 120.0, 116.0];
/// Share of its width a lake loses as it fills, and of its length it loses
/// from each end, as on the page.
const LAKE_SHRINK: f64 = 0.6;
const LAKE_PLUG: f64 = 0.4;
const REEDS: Rgb = [104.0, 126.0, 84.0];
const POND_WATER: Rgb = [72.0, 116.0, 124.0];
const OUTSIDE: Rgb = [24.0, 26.0, 28.0];
const HOUSE: Rgb = [120.0, 58.0, 40.0];
const OUTBUILDING: Rgb = [92.0, 76.0, 56.0];
const CHURCH: Rgb = [48.0, 46.0, 52.0];
const RUIN: Rgb = [118.0, 106.0, 88.0];
const FOAM: Rgb = [236.0, 240.0, 236.0];

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0) as f32;
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The colour at `t` in [0, 1] along evenly spaced `stops`.
fn gradient(stops: &[Rgb], t: f64) -> Rgb {
    let x = t.clamp(0.0, 1.0) * (stops.len() - 1) as f64;
    let k = (x as usize).min(stops.len() - 2);
    mix(stops[k], stops[k + 1], x - k as f64)
}

fn hash(a: u64, b: u64) -> u64 {
    let h = (a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^ (h >> 31)
}

/// Fill `out` with a tributary's course above its mouth: from the edge of
/// the world down to the head of its combe, which lies 0.9 of the combe's
/// depth back from the mouth as on the page. The stream wanders by up to a
/// channel width, less towards the head.
fn upland_course(sim: &Sim, t: &Tributary, out: &mut Vec<P>) {
    let w = sim.params.width;
    let [x, y] = t.mouth;
    let (edge, sign) = if t.side == 1 {
        (sim.bounds[1], 1.0)
    } else {
        (0.0, -1.0)
    };
    let head = y + sign * 0.9 * t.scale * FAN_MAX_DEPTH * w;
    let n = ((edge - head).abs() / (2.0 * w)).ceil().max(1.0) as usize;
    let phase = x / w;
    out.clear();
    out.extend((0..=n).map(|k| {
        let f = k as f64 / n as f64;
        let yk = edge + (head - edge) * f;
        [x + w * (1.0 - f) * (phase + yk / (3.0 * w)).sin(), yk]
    }));
}

/// How the window of the world maps to pixels, or to the samples of a
/// finer view: pixel (i, j) has its centre at `x0 + (i + 0.5) s`,
/// `y0 + (j + 0.5) s`.
pub struct View {
    x0: f64,
    y0: f64,
    /// World units a pixel.
    s: f64,
    w: usize,
    h: usize,
    /// Pixels along each axis of a pixel of the picture, and its size in
    /// world units: 1 and `s` for the picture's own view.
    k: usize,
    px: f64,
}

impl View {
    /// The window of `world` units at the river's window, scaled to fit
    /// `w` by `h` pixels and centred in them.
    pub fn fit(sim: &Sim, world: [f64; 2], w: u16, h: u16) -> Self {
        let (w, h) = (w.max(1) as usize, h.max(1) as usize);
        let s = (world[0] / w as f64).max(world[1] / h as f64);
        View {
            x0: sim.window[0] + 0.5 * (world[0] - w as f64 * s),
            y0: 0.5 * (sim.bounds[1] - h as f64 * s),
            s,
            w,
            h,
            k: 1,
            px: s,
        }
    }

    /// The same window with `k` times the pixels along each axis.
    fn fine(&self, k: usize) -> Self {
        View {
            s: self.s / k as f64,
            w: self.w * k,
            h: self.h * k,
            k: self.k * k,
            ..*self
        }
    }

    fn centre(&self, i: usize, j: usize) -> P {
        [
            self.x0 + (i as f64 + 0.5) * self.s,
            self.y0 + (j as f64 + 0.5) * self.s,
        ]
    }

    /// Pixel range covering world coordinate range `a..b` along x (`axis`
    /// 0) or y (1), clipped to the picture.
    fn span(&self, axis: usize, a: f64, b: f64) -> std::ops::Range<usize> {
        let (o, n) = if axis == 0 {
            (self.x0, self.w)
        } else {
            (self.y0, self.h)
        };
        let lo = ((a - o) / self.s - 0.5).ceil().max(0.0) as usize;
        let hi = (((b - o) / self.s - 0.5).floor() + 1.0).clamp(0.0, n as f64) as usize;
        lo..hi.max(lo)
    }
}

/// A painted frame: the pixels, and the glyph, if any, on each cell.
#[derive(Default)]
pub struct Picture {
    w: usize,
    h: usize,
    pixels: Vec<Rgb>,
    /// Ground and water at `SUPERSAMPLE` samples a pixel along each axis,
    /// row by row.
    samples: Vec<Rgb>,
    glyphs: Vec<Option<(char, Rgb)>>,
}

impl Picture {
    pub fn paint(&mut self, sim: &Sim, view: &View) {
        self.w = view.w;
        self.h = view.h;
        self.glyphs.clear();
        self.glyphs.resize(view.w * view.h.div_ceil(2), None);
        let fine = view.fine(SUPERSAMPLE);
        self.samples.clear();
        self.samples.resize(fine.w * fine.h, OUTSIDE);
        self.ground(sim, &fine);
        self.water(sim, &fine);
        self.resolve();
        self.buildings(sim, view);
    }

    /// Each pixel as an edge between its centre sample's colour and the
    /// sample's colour farthest from it, mixed by the share of samples
    /// nearer the centre's, weighted towards the pixel's centre by a tent
    /// filter. The share is pushed towards 0 or 1 by `SHARPEN`.
    fn resolve(&mut self) {
        let k = SUPERSAMPLE;
        let (w, h, fw) = (self.w, self.h, self.w * k);
        let weight = |d: usize| (1 + d.min(k - 1 - d)) as f32;
        let total = (0..k).map(weight).sum::<f32>().powi(2);
        let dist2 = |a: &Rgb, b: &Rgb| (0..3).map(|q| (a[q] - b[q]).powi(2)).sum::<f32>();
        let (samples, pixels) = (&self.samples, &mut self.pixels);
        let at =
            |i: usize, j: usize, dx: usize, dy: usize| &samples[(j * k + dy) * fw + i * k + dx];
        pixels.clear();
        for c in 0..w * h {
            let (i, j) = (c % w, c / w);
            let centre = *at(i, j, k / 2, k / 2);
            let (mut far, mut far_d) = (centre, 0.0);
            for (dx, dy) in (0..k * k).map(|n| (n % k, n / k)) {
                let s = at(i, j, dx, dy);
                let d = dist2(s, &centre);
                if d > far_d {
                    (far, far_d) = (*s, d);
                }
            }
            if far_d == 0.0 {
                pixels.push(centre);
                continue;
            }
            let near: f32 = (0..k * k)
                .map(|n| (n % k, n / k))
                .filter(|&(dx, dy)| {
                    let s = at(i, j, dx, dy);
                    dist2(s, &centre) <= dist2(s, &far)
                })
                .map(|(dx, dy)| weight(dx) * weight(dy))
                .sum();
            let share = ((near / total - 0.5) * SHARPEN + 0.5).clamp(0.0, 1.0);
            pixels.push(mix(far, centre, share as f64));
        }
    }

    fn ground(&mut self, sim: &Sim, view: &View) {
        let p = &sim.params;
        let (field, land) = (&sim.erodibility, &sim.land);
        let full = (p.vegetation_time + p.forest_time) as f32;
        let plug_life = p.oxbow_lifetime + p.scar_lifetime;
        let cell_at = |q: P| {
            let (cx, cy) = (q[0] / field.cell, q[1] / field.cell);
            (cx >= 0.0 && cy >= 0.0 && (cx as usize) < field.cols && (cy as usize) < field.rows)
                .then(|| cy as usize * field.cols + cx as usize)
        };
        for i in 0..view.w {
            let x = view.centre(i, 0)[0];
            let (lo, hi) = sim.floor(x, false);
            for j in 0..view.h {
                let q = view.centre(i, j);
                let Some(c) = cell_at(q) else { continue };
                let n = ((field.noise[c] as f64 - 0.35) / 0.3).clamp(0.0, 1.0);
                let mut colour = mix(LOAM, gradient(&SOIL, n), 0.5);
                colour = mix(colour, GRAVEL, 0.6 * field.coarse[c] as f64);
                let silt = field.deposit[c] as f64 / p.levee_height.max(1e-9);
                colour = mix(colour, LEVEE, 0.35 * silt);
                let g = field.vegetation(c, full) as f64;
                colour = if g < p.vegetation_time {
                    let bare = mix(BAR, colour, 0.3);
                    mix(
                        bare,
                        mix(colour, FOREST[0], 0.35),
                        smoothstep(g / p.vegetation_time),
                    )
                } else {
                    let t = (g - p.vegetation_time) / p.forest_time.max(1e-9);
                    let tone = if t < FOREST_AT[0] {
                        mix(FOREST[0], FOREST[1], t / FOREST_AT[0])
                    } else {
                        mix(
                            FOREST[1],
                            FOREST[2],
                            (t - FOREST_AT[0]) / (FOREST_AT[1] - FOREST_AT[0]),
                        )
                    };
                    mix(colour, tone, 0.35 + 0.5 * t.min(1.0))
                };
                let cut = field.cut_at[c];
                if cut.is_finite() {
                    let fade = 1.0 - (sim.time - cut) / plug_life;
                    colour = mix(colour, SCAR_WET, 0.6 * fade);
                }
                let owner = land.owner[c];
                if owner != NONE
                    && let Some(Some(h)) = land.holdings.get(owner as usize)
                {
                    let fade = (g - p.grazing_time) / (p.vegetation_time - p.grazing_time);
                    let across = (q[0] - h.seat[0]) * -h.axis[1] + (q[1] - h.seat[1]) * h.axis[0];
                    let strip = (across / (STRIP * p.width)).floor() as i64 as u64;
                    let pick = hash(h.seed as u64, strip);
                    let crop = if land.risk[c] >= MEADOW_RISK {
                        MEADOWS[(pick % 2) as usize]
                    } else {
                        ARABLE[(pick % 3) as usize]
                    };
                    colour = mix(colour, crop, 0.85 * fade);
                    // A hedge where the ground `HEDGE_WIDTH` pixels right
                    // or down has another owner.
                    let d = HEDGE_WIDTH * view.px;
                    let hedge = [[d, 0.0], [0.0, d]].iter().any(|d| {
                        cell_at([q[0] + d[0], q[1] + d[1]]).is_some_and(|k| land.owner[k] != owner)
                    });
                    if hedge {
                        colour = mix(colour, HEDGE, 0.7 * fade);
                    }
                }
                // Upland beyond the wall feet, with the scarp lit on the
                // wall facing the sun and shaded on the other.
                let beyond = (lo - q[1]).max(q[1] - hi);
                if beyond > 0.0 {
                    let (scarp, tone) = if q[1] < lo {
                        (SCARP_SHADE, 0.35)
                    } else {
                        (SCARP_LIGHT, 0.3)
                    };
                    let on_scarp = 1.0 - beyond / (SCARP * p.width);
                    colour = mix(colour, UPLAND, 0.4);
                    colour = mix(colour, scarp, tone * on_scarp.max(0.0));
                }
                self.samples[j * view.w + i] = colour;
            }
        }
    }

    /// Set every sample within `r` of segment `a`–`b` to `colour`, or, with
    /// `dash`, those in every other pixel of the picture.
    fn stroke(&mut self, view: &View, a: P, b: P, r: f64, colour: Rgb, dash: bool) {
        let xs = view.span(0, a[0].min(b[0]) - r, a[0].max(b[0]) + r);
        let ys = view.span(1, a[1].min(b[1]) - r, a[1].max(b[1]) + r);
        let r2 = r * r;
        for j in ys {
            for i in xs.clone() {
                let on = !dash || (i / view.k + j / view.k).is_multiple_of(2);
                if on && seg_dist2(view.centre(i, j), a, b) <= r2 {
                    self.samples[j * view.w + i] = colour;
                }
            }
        }
    }

    /// Stroke a polyline at the radius `r` gives for each segment index.
    fn polyline(
        &mut self,
        view: &View,
        pts: &[P],
        r: impl Fn(usize) -> f64,
        colour: Rgb,
        dash: bool,
    ) {
        for (k, seg) in pts.windows(2).enumerate() {
            self.stroke(view, seg[0], seg[1], r(k), colour, dash);
        }
    }

    fn water(&mut self, sim: &Sim, view: &View) {
        let w = sim.params.width;
        // A line at this radius is `LINE` pixels of the picture wide.
        let thin = 0.5 * LINE * view.px;
        for t in &sim.tracks.tracks {
            self.polyline(view, t, |_| thin, TRACK, false);
        }
        self.polyline(view, &sim.boundary, |_| thin, BOUNDARY, true);
        let mut stream = Vec::new();
        for t in &sim.tributaries {
            upland_course(sim, t, &mut stream);
            stream.extend_from_slice(&t.course);
            self.polyline(view, &stream, |_| thin, WATER, false);
        }
        for o in sim.oxbows.iter().filter(|o| o.is_lake()) {
            // Reeds close over the water as the lake fills, which the page
            // draws as a fringe too narrow for a pixel.
            let reeds = 0.75 * smoothstep(o.fill / 0.9);
            let colour = mix(mix(CLEAR, MURKY, o.fill), REEDS, reeds);
            let r = 0.5 * w * (1.0 - LAKE_SHRINK * o.fill);
            // The plugs advance from the ends of the oxbow as cut off, which
            // a piece split off by the channel may lack.
            let plug = LAKE_PLUG * o.fill * o.length;
            let mut along = o.offset;
            for (k, seg) in o.points.windows(2).enumerate() {
                let next = along + dist(seg[0], seg[1]);
                if along >= plug && next <= o.length - plug {
                    let rk = (r * o.widths[k]).max(0.45 * view.px);
                    self.stroke(view, seg[0], seg[1], rk, colour, false);
                }
                along = next;
            }
        }
        for pond in sim.land.ponds.iter().filter(|p| p.kept()) {
            self.stroke(view, pond.pos, pond.pos, thin, POND_WATER, false);
        }
        for m in sim.mills.iter().filter(|m| m.working()) {
            self.polyline(view, &m.leat, |_| thin, WATER, false);
        }
        let (line, widths) = (&sim.centerline, &sim.widths);
        let r = |k: usize| (0.25 * (widths[k] + widths[k + 1]) * w).max(thin);
        self.polyline(view, line, r, WATER, false);
    }

    /// Put `glyph` on the cell under `p`, over any glyph of lower `rank`.
    fn glyph(&mut self, view: &View, p: P, glyph: char, colour: Rgb, rank: u8, ranks: &mut [u8]) {
        let (i, j) = ((p[0] - view.x0) / view.s, (p[1] - view.y0) / view.s);
        if i < 0.0 || j < 0.0 || i >= view.w as f64 || j >= view.h as f64 {
            return;
        }
        let c = (j as usize / 2) * view.w + i as usize;
        if rank > ranks[c] {
            ranks[c] = rank;
            self.glyphs[c] = Some((glyph, colour));
        }
    }

    fn buildings(&mut self, sim: &Sim, view: &View) {
        let mut ranks = vec![0u8; self.glyphs.len()];
        for b in &sim.land.buildings {
            let (glyph, colour, rank) = if !b.standing() {
                ('·', RUIN, 1)
            } else {
                match b.kind {
                    Kind::Church => ('†', CHURCH, 5),
                    Kind::House | Kind::Cottage => ('⌂', HOUSE, 4),
                    _ => ('▪', OUTBUILDING, 2),
                }
            };
            self.glyph(view, b.pos, glyph, colour, rank, &mut ranks);
        }
        for m in sim.mills.iter().filter(|m| m.working()) {
            self.glyph(view, m.weir, '≈', FOAM, 3, &mut ranks);
        }
    }
}

fn colour(c: Rgb) -> Color {
    Color::Rgb(c[0] as u8, c[1] as u8, c[2] as u8)
}

impl Widget for &Picture {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let rows = self.h.div_ceil(2);
        for (row, y) in (area.top()..area.bottom()).take(rows).enumerate() {
            for (i, x) in (area.left()..area.right()).take(self.w).enumerate() {
                let top = self.pixels[2 * row * self.w + i];
                let bottom = self
                    .pixels
                    .get((2 * row + 1) * self.w + i)
                    .copied()
                    .unwrap_or(OUTSIDE);
                let cell = &mut buf[(x, y)];
                match self.glyphs[row * self.w + i] {
                    Some((g, fg)) => {
                        cell.set_char(g)
                            .set_fg(colour(fg))
                            .set_bg(colour(mix(top, bottom, 0.5)));
                    }
                    None => {
                        cell.set_char('▀')
                            .set_fg(colour(top))
                            .set_bg(colour(bottom));
                    }
                }
            }
        }
    }
}
