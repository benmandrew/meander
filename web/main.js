// SVG view in three stacked layers, each cached by the browser apart from
// the others. The bottom layer holds past centrelines as point-bar sand,
// and changes only when a band of them stops growing or is dropped. The
// ground layer holds the growing band, the soil patches and the floodplain
// features floods leave, and changes when a centreline is kept or a
// feature changes. The top layer holds the oxbow lakes, their scars and the
// channel, redrawn whenever the river moves.

import { startApp } from './ui.js';

const NS = 'http://www.w3.org/2000/svg';
const WATER = [61, 126, 166];
// Water colour of a lake once cut off: the silt settles out of still
// water, so it is clearer and darker than the river.
const CLEAR = [34, 84, 116];
// Fill over which a new lake turns from the river's colour to `CLEAR`. A
// lake left to fill on its own takes 1200 units of simulated time, so this
// is 12 units, two seconds on screen at the default speed.
const CLEARING = 0.01;
// Water colour of a lake near the end of its life, turbid and weedy.
const MURKY = [80, 120, 116];
const REEDS = 'rgb(104, 126, 84)';
const MARSH = 'rgb(118, 134, 96)';

// Floodplain colours. Loam is the background; patches range from clay and
// silt where erosion is slow to sand and gravel where it is fast.
const SOIL = {
  peat: [128, 124, 92],
  clay: [160, 158, 120],
  silt: [182, 182, 140],
  loam: [196, 196, 152],
  sand: [226, 214, 166],
  gravel: [206, 204, 192],
};
// Silt of the natural levees that floods build beside the channel.
const LEVEE = [216, 210, 166];
// Levee contours, as fractions of the highest ground floods can build.
const LEVEE_LEVELS = [0.3, 0.7];
// Simulated time between levee redraws when no flood has come.
const LEVEE_REFRESH = 10;
// Older, higher ground beyond the valley walls, which the river cannot
// reach, and the darker scarp at its edge, `SCARP` widths wide. The upland
// is translucent, so the soil patches under it still show. The wall
// wanders up to `WALL_WANDER` widths outwards from the valley edge, as
// seeded value noise with knots every `WALL_KNOTS` widths, and never
// inwards, where the channel may be.
const UPLAND = [164, 164, 118];
const SCARP_SHADE = [128, 128, 88];
const SCARP = 1.5;
const WALL_WANDER = 4;
const WALL_KNOTS = [30, 8];
// Vegetation on the floodplain, drawn as nested translucent regions that
// darken with age: shrub once it binds a point bar after
// `vegetation_time`, then young and mature forest at these fractions of
// `forest_time` after that. Ground the channel never reached is mature
// forest.
const FOREST = [
  { at: 0, colour: [150, 170, 104] },
  { at: 0.35, colour: [112, 140, 84] },
  { at: 0.85, colour: [84, 116, 70] },
];
const FOREST_OPACITY = 0.16;
// Opacity of the marsh that stays on the clay plug of an old oxbow after
// its lake and scar have gone, until the channel sweeps it away.
const PLUG_MARSH = 0.25;
// Scroll bars: ridges of fresh sand that vegetation colonises first, so
// they darken against the bar as they age. They are kept in this many age
// bands, each one path element.
const RIDGE = [238, 230, 196];
const OLD_RIDGE = [146, 158, 108];
const SCROLL_BANDS = 6;
// The channel's edge: a thin line of bank on both sides, `BANK_EDGE` times
// the local width from the centreline, and the wet sand of the point bar
// on the inside of a bend, up to `BAR_RIM` widths more at a bend of radius
// `BAR_RADIUS` widths. The outer bank is cut, so vegetation meets the
// water there.
const BANK_EDGE = 0.53;
const BAR_RIM = 0.45;
const BAR_RADIUS = 3;
const WET_SAND = [190, 170, 120];
// Depth in the channel. Pools lie against the outer bank of each bend,
// `POOL_LAG` widths downstream of the apex, in nested crescents reaching
// `POOL_LEVELS` of the width in from the bank at a bend of radius
// `BAR_RADIUS` widths or tighter. Riffles are shallow lenses where the flow
// crosses between two bends, `RIFFLE_WIDTH` of the width across and
// fading over `RIFFLE_LENGTH` widths either side of the crossing; both
// bends must be tighter than `RIFFLE_RADIUS` widths.
const POOL_LAG = 1;
const POOL_LEVELS = [0.5, 0.25];
const POOL_COLOURS = [[50, 106, 146], [38, 88, 124]];
const RIFFLE_WIDTH = 0.75;
const RIFFLE_LENGTH = 2;
const RIFFLE_RADIUS = 8;
const SHALLOW = [96, 154, 182];
// Where the channel is wider than `MID_BAR_FROM` widths, a sand bar splits
// the flow, growing to `MID_BAR` of the width across at `MID_BAR_FULL`.
const MID_BAR_FROM = 1.5;
const MID_BAR_FULL = 1.8;
const MID_BAR = 0.4;
// Fresh point-bar sand, and the grassed-over colour old bars fade towards.
const BAR = [233, 222, 186];
const OLD_BAR = [200, 202, 156];
// Point bars are kept in this many age bands, each one group of paths.
const BANDS = 24;
// Points on each rounded end of a lake.
const CAP_STEPS = 8;
// Fraction of its length a lake loses from each end over its lifetime.
const PLUG = 0.4;
// Widest local channel width, as in the core's `MAX_WIDTH`.
const MAX_WIDTH = 2;
// Centreline point spacing in widths, as in the core's `SPACING`.
const SPACING = 0.5;
// Length, as a multiple of the lake's half-width, that a plugged end's cap
// grows by over the lake's lifetime, so plugged ends stay blunt.
const PLUG_TIP = 0.5;
// Shortest lake piece drawn, in channel widths. The channel leaves short
// pieces where it cuts across a lake, which would show as scattered dashes.
const MIN_LAKE = 3;
// A lake point beside the channel, with less than `ALONGSIDE_GAP` widths of
// bank between their water and running within about 45° of parallel to
// it, is drawn dry. A channel that has migrated along an old lake would
// otherwise show it as a second bank beside its own. Points within
// `ALONGSIDE_END` widths of either end of a piece keep their water, so a
// lake still joins the channel at a cut. Each test ramps over the band
// `ALONGSIDE_RAMP` either side of its threshold, as a fraction of it, and
// the lake there shoals, so the channel's jitter from step to step moves
// the lake's outline a little and does not split it on and off.
const ALONGSIDE_GAP = 0.5;
const ALONGSIDE_COS = 0.7;
const ALONGSIDE_END = 2;
const ALONGSIDE_RAMP = 0.5;
const ALONGSIDE_SPAN = 3;
// A centreline is kept once some part of it has moved this many widths from
// the last kept one, so the swept area has no gaps.
const BAR_STEP = 0.5;
// Segments in each run of a long polyline drawn as one path element. The
// browser rasterises the view in tiles and skips elements whose bounds miss
// a tile, so short runs spare each tile from stroking the whole line.
const RUN = 24;
// Change in a lake's fill that redraws its outline. A lake fills over about
// a thousand units of simulated time, so its outline is rebuilt every few
// units, not every step, and moves less than a pixel at each rebuild.
const LAKE_STEP = 1e-3;

function el(name, attrs, parent) {
  const e = document.createElementNS(NS, name);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  parent?.append(e);
  return e;
}

// Set an attribute only when its value changes, so the browser does not
// repaint an element that looks the same.
function set(e, name, value) {
  const last = e.attrs ??= {};
  if (last[name] === value) return;
  last[name] = value;
  e.setAttribute(name, value);
}

// `v` to one decimal place, without the trailing zero `toFixed` keeps,
// and several times faster.
function dp1(v) {
  return '' + Math.round(10 * v) / 10;
}

// Unit left normal and signed curvature at each point of a flat x, y array,
// positive where the river turns towards the left normal.
function channelFrame(pts) {
  const n = pts.length / 2;
  const nx = new Float64Array(n), ny = new Float64Array(n), curv = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const [a, b] = [Math.max(0, i - 1), Math.min(n - 1, i + 1)];
    const tx = pts[2 * b] - pts[2 * a], ty = pts[2 * b + 1] - pts[2 * a + 1];
    const len = Math.sqrt(tx * tx + ty * ty) || 1;
    nx[i] = -ty / len;
    ny[i] = tx / len;
    if (a === i || b === i) continue;
    const ax = pts[2 * i] - pts[2 * a], ay = pts[2 * i + 1] - pts[2 * a + 1];
    const bx = pts[2 * b] - pts[2 * i], by = pts[2 * b + 1] - pts[2 * i + 1];
    const denom = Math.sqrt((ax * ax + ay * ay) * (bx * bx + by * by)) * len;
    curv[i] = denom > 0 ? 2 * (ax * by - ay * bx) / denom : 0;
  }
  return { nx, ny, curv };
}

// Closed path data for the channel over points [start, end), out to
// `scale` times the local width either side, plus `bar[i]` on the inside of
// the bend if `bar` is given: one bank forwards, the other back. At a bend
// tighter than that, the inner bank stops just short of the centre of the
// bend, so it cannot fold over itself.
function channelData(pts, widths, frame, scale, bar, start, end) {
  if (end - start < 2) return '';
  const { nx, ny, curv } = frame;
  const left = [], right = [];
  for (let i = start; i < end; i++) {
    const h = scale * widths[i], k = curv[i], b = bar ? bar[i] : 0;
    const hl = k > 0 ? Math.min(h + b, 0.95 / k) : h;
    const hr = k < 0 ? Math.min(h + b, -0.95 / k) : h;
    const [x, y] = [pts[2 * i], pts[2 * i + 1]];
    left.push(dp1(x + hl * nx[i]) + ' ' + dp1(y + hl * ny[i]));
    right.push(dp1(x - hr * nx[i]) + ' ' + dp1(y - hr * ny[i]));
  }
  return 'M' + left.join(' ') + 'L' + right.reverse().join(' ') + 'Z';
}

// Channel outlines in runs of `RUN` segments, each overlapping its
// neighbours by a segment so no seam shows between them.
function channelRuns(pts, widths, frame, scale, bar = null) {
  const n = pts.length / 2;
  const runs = [];
  for (let s = 0; s < n - 1; s += RUN) {
    runs.push(channelData(pts, widths, frame, scale, bar, Math.max(0, s - 1), Math.min(n, s + RUN + 2)));
  }
  return runs;
}

// Width of the wet sand of the point bar beyond the inner bank at each
// point, in world units: `BAR_RIM` widths at a bend of radius `BAR_RADIUS`
// widths or tighter, falling to nothing on a straight reach. `bend` is the
// curvature smoothed along the river, so the bar does not flicker with the
// point-to-point noise in it.
function barRim(bend, w) {
  return bend.map((c) => BAR_RIM * w * smoothstep(Math.abs(c) * w * BAR_RADIUS));
}

// `values` smoothed along the river by four [1, 2, 1] passes.
function smoothed(values) {
  const n = values.length;
  let v = Float64Array.from(values);
  for (let pass = 0; pass < 4; pass++) {
    const prev = v;
    v = new Float64Array(n);
    for (let i = 0; i < n; i++) v[i] = 0.25 * prev[Math.max(0, i - 1)] + 0.5 * prev[i] + 0.25 * prev[Math.min(n - 1, i + 1)];
  }
  return v;
}

// Closed path data for a band along the river over points [start, end),
// between signed offsets `a[i]` and `b[i]` along the left normal.
function bandData(pts, frame, a, b, start, end) {
  const { nx, ny } = frame;
  const left = [], right = [];
  for (let i = start; i < end; i++) {
    const [x, y] = [pts[2 * i], pts[2 * i + 1]];
    left.push(dp1(x + a[i] * nx[i]) + ' ' + dp1(y + a[i] * ny[i]));
    right.push(dp1(x + b[i] * nx[i]) + ' ' + dp1(y + b[i] * ny[i]));
  }
  return 'M' + left.join(' ') + 'L' + right.reverse().join(' ') + 'Z';
}

// Bands between offsets `a` and `b` over the stretches where they are
// apart, one path data string each, skipping points where they meet.
function bandRuns(pts, frame, a, b) {
  const n = pts.length / 2, runs = [];
  let start = -1;
  for (let i = 0; i <= n; i++) {
    const open = i < n && Math.abs(a[i] - b[i]) > 0.01;
    if (open && start < 0) start = Math.max(0, i - 1);
    if (!open && start >= 0) {
      runs.push(bandData(pts, frame, a, b, start, Math.min(n, i + 1)));
      start = -1;
    }
  }
  return runs;
}

// Pools, riffles and mid-channel bars, as lists of path data: one list per
// pool level, then riffles, then bars. `bend` is the smoothed signed
// curvature, `widths` the local width in world units, `w` the reference
// width.
function channelDepth(pts, widths, frame, bend, w) {
  const n = bend.length;
  const lag = Math.round(POOL_LAG / SPACING);
  const pools = POOL_LEVELS.map((level) => {
    const a = new Float64Array(n), b = new Float64Array(n);
    for (let i = 0; i < n; i++) {
      // The bank on the outside of the bend upstream, and the pool's
      // reach in from it.
      const k = bend[Math.max(0, i - lag)], half = 0.5 * widths[i];
      const out = k > 0 ? -1 : 1;
      const reach = level * widths[i] * smoothstep(Math.abs(k) * w * BAR_RADIUS);
      a[i] = out * half;
      b[i] = out * Math.max(0, half - reach);
    }
    return bandRuns(pts, frame, a, b);
  });
  // Distance in points to the nearest crossing between two bends tighter
  // than `RIFFLE_RADIUS`.
  const span = Math.round(RIFFLE_LENGTH / SPACING), look = 4 * span;
  const tight = 1 / (RIFFLE_RADIUS * w);
  const near = new Float64Array(n).fill(Infinity);
  for (let i = 1; i < n; i++) {
    if ((bend[i - 1] > 0) === (bend[i] > 0)) continue;
    let up = 0, down = 0;
    for (let j = Math.max(0, i - look); j < i; j++) up = Math.max(up, Math.abs(bend[j]));
    for (let j = i; j < Math.min(n, i + look); j++) down = Math.max(down, Math.abs(bend[j]));
    if (up < tight || down < tight) continue;
    for (let j = Math.max(0, i - span); j < Math.min(n, i + span); j++) near[j] = Math.min(near[j], Math.abs(j - i + 0.5));
  }
  const ra = new Float64Array(n), rb = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const h = 0.5 * RIFFLE_WIDTH * widths[i] * smoothstep(1 - near[i] / span);
    [ra[i], rb[i]] = [h, -h];
  }
  // Mid-channel bars, on the width smoothed so they grow and shrink
  // slowly.
  const wide = smoothed(widths);
  const ba = new Float64Array(n), bb = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const h = 0.5 * MID_BAR * wide[i] * smoothstep((wide[i] / w - MID_BAR_FROM) / (MID_BAR_FULL - MID_BAR_FROM));
    [ba[i], bb[i]] = [h, -h];
  }
  return [...pools, bandRuns(pts, frame, ra, rb), bandRuns(pts, frame, ba, bb)];
}

// Path data for points [start, end) of a flat x, y array.
function pathData(pts, start, end) {
  if (end - start < 2) return '';
  const parts = new Array(end - start);
  for (let i = start; i < end; i++) parts[i - start] = dp1(pts[2 * i]) + ' ' + dp1(pts[2 * i + 1]);
  return 'M' + parts[0] + 'L' + parts.slice(1).join(' ');
}

function mixRgb(a, b, t) {
  return a.map((x, i) => x + (b[i] - x) * t);
}

function mix(a, b, t) {
  return `rgb(${mixRgb(a, b, t).map(Math.round)})`;
}

function clamp01(t) {
  return t < 0 ? 0 : t > 1 ? 1 : t;
}

function smoothstep(t) {
  t = clamp01(t);
  return t * t * (3 - 2 * t);
}

// Seeded generator of uniform values in [0, 1) (mulberry32).
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Outline of a soil patch: an oval with radii `rx`, `ry` at a random angle,
// its radius jittered at a few vertices and smoothed with a closed
// Catmull–Rom spline written as cubic Béziers.
function blobData(rand, cx, cy, rx, ry) {
  const n = 7 + Math.floor(4 * rand());
  const turn = Math.PI * rand();
  const [c, s] = [Math.cos(turn), Math.sin(turn)];
  const pts = [];
  for (let k = 0; k < n; k++) {
    const a = 2 * Math.PI * (k + 0.3 * (rand() - 0.5)) / n;
    const r = 0.85 + 0.3 * rand();
    const x = rx * r * Math.cos(a), y = ry * r * Math.sin(a);
    pts.push([cx + c * x - s * y, cy + s * x + c * y]);
  }
  const f = (v) => v.toFixed(1);
  let d = `M${f(pts[0][0])} ${f(pts[0][1])}`;
  for (let k = 0; k < n; k++) {
    const [p0, p1, p2, p3] = [-1, 0, 1, 2].map((j) => pts[(k + j + n) % n]);
    d += `C${f(p1[0] + (p2[0] - p0[0]) / 6)} ${f(p1[1] + (p2[1] - p0[1]) / 6)} `
      + `${f(p2[0] - (p3[0] - p1[0]) / 6)} ${f(p2[1] - (p3[1] - p1[1]) / 6)} ${f(p2[0])} ${f(p2[1])}`;
  }
  return d + 'Z';
}

// Outline of an oxbow lake along points [start, end) of a flat x, y array,
// covering arc lengths [from, to] along them. The bed is a parabola across
// the old channel, so the lake's half-width is `half` × √depth, capped at
// `half` by the old banks. The depth is `level` less the bed's rise, which
// is `relief` × 4 × (height − 0.5) for the land heights in `heights`, plus
// the flood silt in `silts` above its mean over the lake's interior points;
// the mean is part of the fill that sets `level`. Each point's depth moves
// by its fraction in `dry`, if given, towards `level` above the water.
// Where the bed rises above the water the lake
// splits into pools, one subpath each, and the parabola gives each pool a
// round tip there. A plugged end is a round cap when young, lengthening by
// `PLUG_TIP` times the half-width with `age` in [0, 1]. An end cut by the
// channel stays a round cap, hidden under the water.
function lakeData(pts, heights, silts, widths, start, end, from, to, half, level, relief, age, dry) {
  const m = end - start;
  if (m < 2 || level <= 0) return '';
  let mean = 0;
  for (let k = 0; k < m; k++) mean += widths[start + k] / m;
  const wide = half * mean;
  const s = new Float64Array(m), tx = new Float64Array(m), ty = new Float64Array(m);
  const depth = new Float64Array(m);
  let silt = 0;
  for (let k = 1; k < m - 1; k++) silt += silts[start + k] / Math.max(1, m - 2);
  // The end points lie under the channel at a cut, swept clear of silt.
  for (let k = 0; k < m; k++) {
    const extra = k > 0 && k < m - 1 ? silts[start + k] - silt : 0;
    const wet = level - 4 * relief * (heights[start + k] - 0.5) - extra;
    depth[k] = wet - (dry ? dry[k] * (wet + level) : 0);
  }
  for (let k = 1; k < m; k++) {
    const i = 2 * (start + k);
    s[k] = s[k - 1] + Math.hypot(pts[i] - pts[i - 2], pts[i + 1] - pts[i - 1]);
  }
  const [a, b] = [Math.max(0, from), Math.min(s[m - 1], to)];
  if (b <= a) return '';
  // Unit tangents at the points, by central differences.
  for (let k = 0; k < m; k++) {
    const p = 2 * (start + Math.max(0, k - 1)), q = 2 * (start + Math.min(m - 1, k + 1));
    const dx = pts[q] - pts[p], dy = pts[q + 1] - pts[p + 1];
    const l = Math.hypot(dx, dy) || 1;
    tx[k] = dx / l;
    ty[k] = dy / l;
  }
  // Segment and fraction along it at arc length `at`, walking forwards from
  // segment `j`.
  const locate = (at, j) => {
    while (j < m - 2 && s[j + 1] < at) j++;
    return [j, (at - s[j]) / (s[j + 1] - s[j] || 1)];
  };
  const depthAt = (j, u) => depth[j] + (depth[j + 1] - depth[j]) * u;
  // Half-width at fraction `u` along segment `j`, at depth `d`: `half`
  // times the width of the channel the lake was cut from.
  const width = (j, u, d) => {
    const b = widths[start + j] + (widths[start + j + 1] - widths[start + j]) * u;
    return half * b * Math.sqrt(Math.min(1, Math.max(0, d)));
  };

  // Wet runs [u, v] of [a, b]. The depth is linear between points, so each
  // crossing is exact.
  const runs = [];
  let [j, u] = locate(a, 0);
  let [prevS, prevD] = [a, depthAt(j, u)];
  let wet = prevD > 0 ? a : null;
  for (let k = 0; k <= m; k++) {
    if (k < m && (s[k] <= a || s[k] >= b)) continue;
    let [at, d] = [s[k], depth[k]];
    if (k === m) {
      [j, u] = locate(b, j);
      [at, d] = [b, depthAt(j, u)];
    }
    if ((prevD > 0) !== (d > 0)) {
      const x = prevS + (at - prevS) * prevD / (prevD - d);
      if (wet === null) {
        wet = x;
      } else {
        runs.push([wet, x]);
        wet = null;
      }
    }
    [prevS, prevD] = [at, d];
  }
  if (wet !== null) runs.push([wet, b]);

  let path = '';
  j = 0;
  for (const [u0, v0] of runs) {
    // A pool shorter than the half-width narrows with its length, so it
    // shrinks away and does not vanish at once.
    const thin = smoothstep((v0 - u0) / wide);
    if (thin < 0.02) continue;
    // Each end: a round cap of the local width at a plugged or cut end, or
    // a parabolic tip where the bed rises above the water, sampled densely
    // near the tip.
    const end = (at, plugged) => {
      if (at !== a && at !== b) return { len: wide, cap: false };
      const [jj, uu] = locate(at, 0);
      const r = width(jj, uu, depthAt(jj, uu));
      return { len: plugged ? r * (1 + PLUG_TIP * age) : r, cap: true };
    };
    const [ea, eb] = [end(u0, from > 0), end(v0, to < s[m - 1])];
    const fit = Math.min(1, (v0 - u0) / (ea.len + eb.len));
    ea.len *= fit;
    eb.len *= fit;
    const left = [], right = [];
    // Bank points at arc length `at`, `shape` × the local half-width either
    // side of the centreline, with the position, normal and depth
    // interpolated along the segment.
    const add = (at, shape) => {
      let f;
      [j, f] = locate(at, j);
      const i = 2 * (start + j);
      const x = pts[i] + (pts[i + 2] - pts[i]) * f, y = pts[i + 1] + (pts[i + 3] - pts[i + 1]) * f;
      const nx = -(ty[j] + (ty[j + 1] - ty[j]) * f), ny = tx[j] + (tx[j + 1] - tx[j]) * f;
      const h = thin * shape * width(j, f, depthAt(j, f)) / (Math.hypot(nx, ny) || 1);
      left.push((x + nx * h).toFixed(1) + ' ' + (y + ny * h).toFixed(1));
      right.push((x - nx * h).toFixed(1) + ' ' + (y - ny * h).toFixed(1));
    };
    // Offset into an end region, and the bank shape there, for step `q`.
    const step = (e, q) => {
      if (!e.cap) return [e.len * (q / CAP_STEPS) ** 2, 1];
      const t = 0.5 * Math.PI * q / CAP_STEPS;
      return [e.len * (1 - Math.cos(t)), Math.sin(t)];
    };
    for (let q = 0; q <= CAP_STEPS; q++) {
      const [o, shape] = step(ea, q);
      add(u0 + o, shape);
    }
    for (let k = 0; k < m; k++) if (s[k] > u0 + ea.len && s[k] < v0 - eb.len) add(s[k], 1);
    for (let q = CAP_STEPS; q >= 0; q--) {
      const [o, shape] = step(eb, q);
      add(v0 - o, shape);
    }
    // Both banks meet at the tips; keep each tip once.
    path += 'M' + left.join('L') + 'L' + right.reverse().slice(1, -1).join('L') + 'Z';
  }
  return path;
}

// Marching squares cases: for each set of corners above the level (top left
// 8, top right 4, bottom right 2, bottom left 1), the cell edges each
// contour segment joins, as T, R, B, L. Saddles 5 and 10 list the pairing
// for a centre above the level, then for one below it.
const CASES = [
  [], ['LB'], ['BR'], ['LR'], ['TR'], [['LT', 'BR'], ['LB', 'TR']], ['TB'], ['LT'],
  ['LT'], ['TB'], [['TR', 'LB'], ['LT', 'BR']], ['TR'], ['LR'], ['BR'], ['LB'], [],
];

// Region where a grid exceeds `level`, as path data for the even-odd fill
// rule. `values` holds `cols` × `rows` cells of side `cell`, row by row; only
// columns [c0, c1) are traced, and cells outside them count as below the
// level, so every contour closes. The contours run through the cell centres,
// placed by linear interpolation along the cell edges.
function contourData(values, cols, rows, cell, c0, c1, level) {
  // Corner grid over the cell centres, with a one-cell border below the level.
  const W = c1 - c0 + 2, H = rows + 2;
  const v = new Float32Array(W * H).fill(-level);
  for (let j = 1; j <= rows; j++) {
    for (let i = 1; i <= c1 - c0; i++) v[j * W + i] = values[(j - 1) * cols + c0 + i - 1];
  }
  // Edge ids: 2k is the edge from corner k rightwards, 2k + 1 downwards.
  const edge = (i, j, side) => {
    const k = j * W + i;
    return side === 'T' ? 2 * k : side === 'B' ? 2 * (k + W) : side === 'L' ? 2 * k + 1 : 2 * (k + 1) + 1;
  };
  const point = (id) => {
    const k = id >> 1, i = k % W, j = (k - i) / W;
    const k2 = id & 1 ? k + W : k + 1;
    const t = (level - v[k]) / (v[k2] - v[k]);
    const [di, dj] = id & 1 ? [0, t] : [t, 0];
    return ((c0 + i + di - 0.5) * cell).toFixed(1) + ' ' + ((j + dj - 0.5) * cell).toFixed(1);
  };
  // Segments as pairs of edge ids, and up to two segments at each edge.
  const segs = [];
  const at = new Int32Array(4 * W * H).fill(-1);
  const link = (e, s) => { at[2 * e + (at[2 * e] < 0 ? 0 : 1)] = s; };
  for (let j = 0; j < H - 1; j++) {
    for (let i = 0; i < W - 1; i++) {
      const tl = v[j * W + i], tr = v[j * W + i + 1], br = v[(j + 1) * W + i + 1], bl = v[(j + 1) * W + i];
      const c = (tl > level) * 8 + (tr > level) * 4 + (br > level) * 2 + (bl > level);
      let pairs = CASES[c];
      if (c === 5 || c === 10) pairs = pairs[(tl + tr + br + bl) / 4 > level ? 0 : 1];
      for (const [a, b] of pairs) {
        const s = segs.length / 2;
        const [ea, eb] = [edge(i, j, a), edge(i, j, b)];
        segs.push(ea, eb);
        link(ea, s);
        link(eb, s);
      }
    }
  }
  // Chain the segments into closed loops.
  const used = new Uint8Array(segs.length / 2);
  let d = '';
  for (let s0 = 0; s0 < used.length; s0++) {
    if (used[s0]) continue;
    const pts = [point(segs[2 * s0])];
    let [s, e] = [s0, segs[2 * s0 + 1]];
    while (s >= 0) {
      used[s] = 1;
      pts.push(point(e));
      const next = at[2 * e] === s ? at[2 * e + 1] : at[2 * e];
      if (next < 0 || used[next]) break;
      s = next;
      e = segs[2 * s] === e ? segs[2 * s + 1] : segs[2 * s];
    }
    if (pts.length > 3) d += 'M' + pts.join('L') + 'Z';
  }
  return d;
}

// Points of a flat x, y array with x in [x0, x1]: path data with one subpath
// per run inside the range, and the points themselves.
function cropped(line, x0, x1) {
  const runs = [], kept = [];
  let run = [];
  for (let i = 0; i < line.length; i += 2) {
    const x = line[i], y = line[i + 1];
    if (x < x0 || x > x1) {
      if (run.length > 1) runs.push(run);
      run = [];
      continue;
    }
    run.push(x.toFixed(1) + ' ' + y.toFixed(1));
    kept.push(x, y);
  }
  if (run.length > 1) runs.push(run);
  return { d: runs.map((r) => 'M' + r[0] + 'L' + r.slice(1).join(' ')).join(''), pts: kept };
}

// Arc length of points [start, end) of a flat x, y array.
function pieceLength(pts, start, end) {
  let len = 0;
  for (let i = start + 1; i < end; i++) len += Math.hypot(pts[2 * i] - pts[2 * i - 2], pts[2 * i + 1] - pts[2 * i - 1]);
  return len;
}

// How far `v` is past `threshold`, from 0 at `ALONGSIDE_RAMP` × `threshold`
// below it to 1 as far above it, eased at both ends.
function ramp(v, threshold) {
  const band = ALONGSIDE_RAMP * threshold;
  return smoothstep((v - threshold + band) / (2 * band));
}

// How dry each lake point [start, end) of a flat x, y array is drawn for
// lying beside the channel, from 0 to 1 by index into the piece, or null if
// none is, and a key that changes when it does. See `ALONGSIDE_GAP`.
// `oxWidths` is in multiples of the reference width `w` and `widths` in
// world units; `channel` is a grid of the channel points `line`.
function alongside(pts, oxWidths, start, end, line, widths, channel, w) {
  const m = end - start;
  let dry = null, key = 0, s = 0;
  const ends = ALONGSIDE_END * w, total = pieceLength(pts, start, end);
  for (let k = 1; k < m - 1; k++) {
    const i = start + k;
    s += Math.hypot(pts[2 * i] - pts[2 * i - 2], pts[2 * i + 1] - pts[2 * i - 1]);
    const inner = ramp(Math.min(s, total - s), ends);
    if (inner <= 0) continue;
    const [x, y] = [pts[2 * i], pts[2 * i + 1]];
    const j = channel.nearest(x, y, channel.size);
    if (j < 0) continue;
    // Tangents over `ALONGSIDE_SPAN` points either side, which steadies
    // them against the channel's jitter and resampling from step to step.
    const c = j >> 1, nc = line.length / 2;
    const [c0, c1] = [Math.max(0, c - ALONGSIDE_SPAN), Math.min(nc - 1, c + ALONGSIDE_SPAN)];
    const [i0, i1] = [Math.max(start, i - ALONGSIDE_SPAN), Math.min(end - 1, i + ALONGSIDE_SPAN)];
    const ux = line[2 * c1] - line[2 * c0], uy = line[2 * c1 + 1] - line[2 * c0 + 1];
    const tx = pts[2 * i1] - pts[2 * i0], ty = pts[2 * i1 + 1] - pts[2 * i0 + 1];
    const ul = Math.hypot(ux, uy) || 1;
    // The gap to the channel's segments, not its nearest point, so it does
    // not move with the spacing of the channel's points along it.
    let across = Infinity;
    for (let a = c0; a < c1; a++) {
      const [ax, ay] = [line[2 * a], line[2 * a + 1]];
      const [bx, by] = [line[2 * a + 2] - ax, line[2 * a + 3] - ay];
      const t = clamp01(((x - ax) * bx + (y - ay) * by) / (bx * bx + by * by || 1));
      across = Math.min(across, Math.hypot(x - ax - t * bx, y - ay - t * by));
    }
    const gap = across - 0.5 * (widths[c] + w * oxWidths[i]);
    const near = 1 - ramp(gap, ALONGSIDE_GAP * w);
    if (near <= 0) continue;
    const cos = (tx * ux + ty * uy) / (ul * (Math.hypot(tx, ty) || 1));
    const d = inner * near * ramp(Math.abs(cos), ALONGSIDE_COS);
    if (d <= 0) continue;
    dry ??= new Float32Array(m);
    dry[k] = d;
    key = (31 * key + k * 64 + Math.round(63 * d)) | 0;
  }
  return { dry, key };
}

// Points of a flat x, y array hashed into square cells of side `size`.
// Hashed grid of points, filled by counting sort into flat arrays. Cells
// along a row hash to consecutive buckets, so the three cells around a
// point in a row are one run of the table.
class Grid {
  constructor(pts, size) {
    const n = pts.length / 2;
    let buckets = 1;
    while (buckets < 4 * n) buckets *= 2;
    this.pts = pts;
    this.size = size;
    this.mask = buckets - 1;
    this.stride = 4099;
    this.start = new Int32Array(buckets + 1);
    const cell = new Int32Array(n);
    for (let i = 0; i < n; i++) {
      cell[i] = this.bucket(Math.floor(pts[2 * i] / size), Math.floor(pts[2 * i + 1] / size));
      this.start[cell[i] + 1]++;
    }
    for (let b = 0; b < buckets; b++) this.start[b + 1] += this.start[b];
    const fill = this.start.slice(0, buckets);
    this.items = new Int32Array(n);
    for (let i = 0; i < n; i++) this.items[fill[cell[i]]++] = 2 * i;
  }

  bucket(cx, cy) {
    return (cy * this.stride + cx) & this.mask;
  }

  // Whether any point lies within `r` (at most `size`) of (x, y).
  near(x, y, r) {
    const cx = Math.floor(x / this.size), cy = Math.floor(y / this.size);
    const { start, mask } = this, r2 = r * r;
    for (let j = cy - 1; j <= cy + 1; j++) {
      // The run of three buckets, in two parts where it wraps.
      const b = this.bucket(cx - 1, j), e = Math.min(b + 3, mask + 1);
      if (this.scan(start[b], start[e], x, y, r2) || this.scan(0, start[b + 3 - e], x, y, r2)) return true;
    }
    return false;
  }

  // Offset in `pts` of the point nearest (x, y) within `r` (at most
  // `size`), or −1 if there is none.
  nearest(x, y, r) {
    const cx = Math.floor(x / this.size), cy = Math.floor(y / this.size);
    const { start, mask, items, pts } = this;
    let [best, bestD] = [-1, r * r];
    const scan = (lo, hi) => {
      for (let m = lo; m < hi; m++) {
        const k = items[m], dx = pts[k] - x, dy = pts[k + 1] - y, d = dx * dx + dy * dy;
        if (d <= bestD) [best, bestD] = [k, d];
      }
    };
    for (let j = cy - 1; j <= cy + 1; j++) {
      const b = this.bucket(cx - 1, j), e = Math.min(b + 3, mask + 1);
      scan(start[b], start[e]);
      scan(0, start[b + 3 - e]);
    }
    return best;
  }

  // Whether any point in items [lo, hi) lies within √r2 of (x, y).
  scan(lo, hi, x, y, r2) {
    for (let m = lo; m < hi; m++) {
      const k = this.items[m], dx = this.pts[k] - x, dy = this.pts[k + 1] - y;
      if (dx * dx + dy * dy <= r2) return true;
    }
    return false;
  }
}

// Reuse `n` path elements from `list` under `parent`, adding or removing
// elements as needed.
function resize(list, n, parent) {
  while (list.length < n) list.push(el('path', {}, parent));
  while (list.length > n) list.pop().remove();
}

class SvgRenderer {
  constructor(bars, ground, svg) {
    this.layers = [bars, ground, svg];
    this.bars = bars;
    this.ground = ground;
    this.svg = svg;
    for (const s of this.layers) s.setAttribute('preserveAspectRatio', 'xMidYMid meet');
    const round = { fill: 'none', 'stroke-linejoin': 'round', 'stroke-linecap': 'round' };
    // Centrelines turn only a few degrees at each point, where a bevel join
    // looks the same as a round one and is far cheaper to rasterise.
    const line = { ...round, 'stroke-linejoin': 'bevel' };
    this.background = el('rect', { y: 0, fill: `rgb(${SOIL.loam})` }, bars);
    // Soil patches lie under the point bars: ground the channel has swept is
    // its own fresh sand, whatever soil was there before.
    this.soilLayer = el('g', {}, bars);
    // Point bars: each age band is one group of paths in one colour, newest
    // on top. A band is opaque while young, so each spot shows the newest
    // band over it and bands never build up. Old bands fade towards the
    // background colour and turn transparent, so the soil shows through
    // again as it forms on the old bar. Bands that have stopped growing lie
    // in the bottom layer; the growing band lies in the ground layer, so
    // keeping a centreline does not repaint the old ones.
    this.oldBars = el('g', line, bars);
    this.barLayer = el('g', line, ground);
    // Shrub and forest lie over the bars and under the scroll bars, so the
    // ridges show through the trees.
    this.forestLayer = el('g', { 'fill-rule': 'evenodd', 'fill-opacity': FOREST_OPACITY }, ground);
    this.forest = FOREST.map(({ colour }) => el('path', { fill: `rgb(${colour})` }, this.forestLayer));
    this.forestAt = null;
    // Scroll bars: one group for each age band, with one path for each
    // ridge, so trimming a ridge repaints only the ground around it.
    this.scrollLayer = el('g', round, ground);
    this.scrollBands = Array.from({ length: SCROLL_BANDS }, (_, b) => {
      const age = (b + 0.5) / SCROLL_BANDS;
      return el('g', {
        stroke: mix(RIDGE, OLD_RIDGE, smoothstep(2 * age)),
        'stroke-opacity': (0.7 * (1 - smoothstep((age - 0.75) / 0.25))).toFixed(2),
      }, this.scrollLayer);
    });
    // Path element of each ridge drawn, by band and path data.
    this.ridges = new Map();
    this.scrollAt = null;
    // Natural levees: nested translucent silt regions, higher ground inside.
    this.leveeLayer = el('g', { fill: `rgb(${LEVEE})`, 'fill-rule': 'evenodd', 'fill-opacity': 0.35 }, ground);
    this.levees = LEVEE_LEVELS.map(() => el('path', {}, this.leveeLayer));
    this.leveeAt = null;
    // The valley walls lie over the floodplain, which never reaches them.
    this.walls = el('g', {}, ground);
    // Every clay plug the channel has not swept away keeps a faint marsh,
    // so old meander belts stay legible after their scars fade.
    this.plugMarsh = el('path', { fill: MARSH, 'fill-opacity': PLUG_MARSH, 'fill-rule': 'evenodd' }, ground);
    this.plugAt = null;
    this.blurred = null;
    // Oxbow scars lie over the whole ground, under the channel. They change
    // slowly, so they sit at the top of the ground layer, not in the top
    // layer that is redrawn whenever the river moves.
    this.scarLayer = el('g', { ...line, stroke: MARSH }, ground);
    // The bank lies under the lakes, so a lake that meets the channel joins
    // its water with no strip of bank between them.
    // Both are filled outlines at the local width, which a stroke cannot vary.
    // Every lake's reed fringe lies under the water of all of them, so lakes
    // that meet join as one body of water.
    this.bank = el('g', { fill: `rgb(${WET_SAND})` }, svg);
    this.reedLayer = el('g', { fill: 'none', 'stroke-linejoin': 'round', stroke: REEDS }, svg);
    this.lakeLayer = el('g', {}, svg);
    this.water = el('g', { fill: `rgb(${WATER})` }, svg);
    // Pools, riffles and mid-channel bars, over the channel's water.
    this.depth = [...POOL_COLOURS, SHALLOW, BAR].map((c) => ({ g: el('g', { fill: `rgb(${c})` }, svg), paths: [] }));
    this.bankRuns = [];
    this.waterRuns = [];
    this.lakes = [];
    this.reeds = [];
    this.scars = [];
    this.bands = [];
    // Bands dropped since the last draw, still to be removed from the page.
    this.dropped = [];
    // Bar settings the old bands were last shaded for.
    this.shadedFor = null;
    this.lastBar = null;
    this.window = [0, 0];
  }

  // Show only the visible window; the hidden reaches fall outside it.
  reset(river, [w, h], seed) {
    const x = river.view_x();
    for (const s of this.layers) s.setAttribute('viewBox', `${x} 0 ${w} ${h}`);
    this.background.setAttribute('x', x);
    this.background.setAttribute('width', w);
    this.background.setAttribute('height', h);
    this.oldBars.replaceChildren();
    this.barLayer.replaceChildren();
    this.bands = [];
    this.dropped = [];
    this.shadedFor = null;
    this.lastBar = null;
    this.window = [x, w];
    this.leveeAt = null;
    this.plugAt = null;
    this.forestAt = null;
    this.scrollAt = null;
    for (const scar of this.scars) scar.shape = null;
    for (const lake of this.lakes) lake.inputs = null;
    this.drawSoils(river, x, w, h, seed);
    this.drawWalls(river, x, w, h, seed);
  }

  // Draw the upland beyond each valley wall over the window [x0, x0 + vw),
  // with a scarp along the wall. The core narrows the valley at the window
  // edges to keep bends off them; the walls are drawn at its full width, so
  // that device does not show.
  drawWalls(river, x0, vw, h, seed) {
    this.walls.replaceChildren();
    const w = river.get_param('width'), valley = river.get_param('valley_width') * w;
    if (!(valley > 0 && valley < h)) return;
    const rand = random(seed ^ 0xba1e);
    const step = 0.5 * w, n = Math.ceil(vw / step) + 1;
    for (const side of [-1, 1]) {
      const octaves = WALL_KNOTS.map((len) => Array.from({ length: Math.ceil(vw / (len * w)) + 2 }, rand));
      // Distance of the wall from the top or bottom edge of the window.
      const edge = new Float64Array(n);
      for (let k = 0; k < n; k++) {
        const x = Math.min(k * step, vw) / w;
        let u = 0;
        WALL_KNOTS.forEach((len, m) => {
          const t = x / len, j = Math.floor(t), knots = octaves[m];
          u += (knots[j] + (knots[j + 1] - knots[j]) * smoothstep(t - j)) / (m + 1);
        });
        edge[k] = 0.5 * (h - valley) - WALL_WANDER * w * u / 1.5;
      }
      const at = (k, d) => {
        const x = dp1(x0 + Math.min(k * step, vw));
        return `${x} ${dp1(side < 0 ? d : h - d)}`;
      };
      const wall = Array.from(edge, (d, k) => at(k, d)).join(' ');
      const rim = side < 0 ? 0 : h;
      el('path', {
        d: `M${dp1(x0)} ${rim} L${wall} L${dp1(x0 + vw)} ${rim}Z`,
        fill: `rgb(${UPLAND})`, 'fill-opacity': 0.8,
      }, this.walls);
      el('path', {
        d: 'M' + Array.from(edge, (d, k) => at(k, d - 0.5 * SCARP * w)).join(' '),
        fill: 'none', stroke: `rgb(${SCARP_SHADE})`, 'stroke-opacity': 0.45,
        'stroke-width': SCARP * w, 'stroke-linejoin': 'round',
      }, this.walls);
    }
  }

  // Scatter soil patches over the window: broad patches whose type follows
  // the erodibility noise at their centre, then small ones of gravel on
  // erodible ground and peat in clay hollows.
  drawSoils(river, x0, vw, h, seed) {
    this.soilLayer.replaceChildren();
    const rand = random(seed ^ 0x5eed);
    const w = river.get_param('width');
    const margin = 6 * w;
    const place = (count, rMin, rMax, kind) => {
      for (let k = 0; k < count; k++) {
        const cx = x0 - margin + rand() * (vw + 2 * margin);
        const cy = -margin + rand() * (h + 2 * margin);
        const n = clamp01((river.soil(Math.max(0, cx), Math.max(0, cy)) - 0.35) / 0.3);
        const type = kind(n, rand());
        if (!type) continue;
        const r = (rMin + (rMax - rMin) * rand()) * w;
        const aspect = 0.45 + 0.55 * rand();
        el('path', {
          d: blobData(rand, cx, cy, r, r * aspect),
          fill: `rgb(${SOIL[type]})`,
          'fill-opacity': (0.4 + 0.25 * rand()).toFixed(2),
        }, this.soilLayer);
      }
    };
    const area = (vw + 2 * margin) * (h + 2 * margin) / (w * w);
    place(Math.round(area / 150), 3, 8, (n) => (n < 0.25 ? 'clay' : n < 0.45 ? 'silt' : n < 0.6 ? null : 'sand'));
    place(Math.round(area / 50), 0.8, 2.5, (n, u) => {
      if (n > 0.5 && u < n - 0.2) return 'gravel';
      if (n < 0.25 && u < 0.5 - n) return 'peat';
      return null;
    });
  }

  draw(river, memory, { local }) {
    const w = river.get_param('width');
    // The floodplain layers sync their own data, which can grow memory, so
    // they go before the views below are built.
    const layers = [
      [this.oldBars, local.showBars], [this.barLayer, local.showBars], [this.leveeLayer, local.showLevees],
      [this.scrollLayer, local.showScrolls],
    ];
    for (const [layer, show] of layers) {
      if (layer.style.display !== (show ? '' : 'none')) layer.style.display = show ? '' : 'none';
    }
    if (local.showLevees) this.drawLevees(river, memory);
    this.drawPlugs(river, memory);
    this.drawForest(river, memory);
    if (local.showScrolls) this.drawScrolls(river, memory, w);

    river.sync();
    // Views must be rebuilt after every step: memory growth detaches them.
    const buf = memory.buffer;
    const n = river.oxbow_count();
    const line = new Float32Array(buf, river.line_ptr(), river.line_len());
    const oxPts = new Float32Array(buf, river.oxbow_line_ptr(), river.oxbow_line_len());
    const oxOff = new Uint32Array(buf, river.oxbow_offsets_ptr(), n + 1);
    const oxFill = new Float32Array(buf, river.oxbow_fills_ptr(), n);
    const oxScar = new Float32Array(buf, river.oxbow_scar_ages_ptr(), n);
    const oxLen = new Float32Array(buf, river.oxbow_lengths_ptr(), n);
    const oxAlong = new Float32Array(buf, river.oxbow_offsets_along_ptr(), n);
    const oxHeight = new Float32Array(buf, river.oxbow_heights_ptr(), oxPts.length / 2);
    const oxWidth = new Float32Array(buf, river.oxbow_widths_ptr(), oxPts.length / 2);
    const oxSilt = new Float32Array(buf, river.oxbow_silts_ptr(), oxPts.length / 2);
    const widths = new Float32Array(buf, river.widths_ptr(), line.length / 2);
    const scarLife = Math.max(1e-6, river.get_param('scar_lifetime'));
    const frame = channelFrame(line);
    const waterRuns = channelRuns(line, widths, frame, 0.5);
    const bend = smoothed(frame.curv);
    const bankRuns = channelRuns(line, widths, frame, BANK_EDGE, barRim(bend, w));
    // Channel points, to find lake points beside it.
    const reach = (MAX_WIDTH + ALONGSIDE_GAP) * w;
    const channel = new Grid(line, reach);

    this.record(line, river.time(), w, local);
    this.drawBars(river.time(), w, local);

    // Oxbows: a lake that clears and darkens once cut off, plugs from the
    // ends, narrows far more slowly from the banks and turns murky, fading
    // only over the last fifth of its fill, then a marshy scar that fades
    // out. Both plugs and banks advance steadily as it fills, and the water
    // drains first from where the land is high or floods have laid the most
    // silt, splitting the lake into pools. The reed fringe grows inwards, so
    // it does not hide the narrowing.
    resize(this.scars, n, this.scarLayer);
    resize(this.lakes, n, this.lakeLayer);
    resize(this.reeds, n, this.reedLayer);
    for (let k = 0; k < n; k++) {
      const age = oxFill[k];
      const scar = this.scars[k], lake = this.lakes[k], reed = this.reeds[k];
      // The channel splits an oxbow into pieces of its original polyline,
      // so a piece's ends and length identify its shape.
      const [s0, s1] = [2 * oxOff[k], 2 * oxOff[k + 1]];
      const shape = `${s1 - s0} ${oxPts[s0]} ${oxPts[s0 + 1]} ${oxPts[s1 - 2]} ${oxPts[s1 - 1]}`;
      // Mean width of the channel the oxbow was cut from.
      let wm = 0;
      for (let i = oxOff[k]; i < oxOff[k + 1]; i++) wm += oxWidth[i];
      wm *= w / Math.max(1, oxOff[k + 1] - oxOff[k]);
      if (scar.shape !== shape) {
        scar.shape = shape;
        set(scar, 'd', pathData(oxPts, oxOff[k], oxOff[k + 1]));
      }
      set(scar, 'stroke-width', dp1(wm));
      const sliver = pieceLength(oxPts, oxOff[k], oxOff[k + 1]) < MIN_LAKE * w;
      const fade = sliver ? 0 : 1 - smoothstep(oxScar[k] / scarLife);
      set(scar, 'stroke-opacity', (0.45 * smoothstep(age) * fade).toFixed(2));
      if (age >= 1 || sliver) {
        // Elements are matched to oxbows by index, so a lake can come back
        // to this element when the channel splits or drops an oxbow before
        // it. Forget its inputs, or it would stay blank.
        set(lake, 'd', '');
        set(reed, 'd', '');
        lake.inputs = null;
        continue;
      }
      const reeds = 0.1 * wm + 0.15 * wm * age;
      // Half-width where the bed is at its mean height.
      const hw = 0.5 * wm * (1 - local.oxbowShrink * age) - 0.5 * reeds;
      // The plugs advance from the ends of the oxbow as cut off, which a
      // piece split off by the channel may lack.
      const plug = PLUG * age * oxLen[k];
      const [from, to] = [plug - oxAlong[k], oxLen[k] - plug - oxAlong[k]];
      const level = hw > 0 ? (hw / (0.5 * wm)) ** 2 : 0;
      // The water covers the inner half of the fringe, which is stroked at
      // twice its width, so the outline lies half a fringe inside `hw`.
      const inset = hw > 0 ? Math.max(0, 1 - 0.5 * reeds / hw) : 0;
      // Floods and the channel change the silt without reshaping the oxbow.
      let silt = 0;
      for (let i = oxOff[k]; i < oxOff[k + 1]; i++) silt += oxSilt[i];
      const { dry, key } = alongside(oxPts, oxWidth, oxOff[k], oxOff[k + 1], line, widths, channel, w);
      const inputs = `${shape} ${Math.round(age / LAKE_STEP)} ${silt} ${w} ${local.oxbowShrink} ${local.oxbowRelief} ${key}`;
      if (lake.inputs !== inputs) {
        lake.inputs = inputs;
        const d = lakeData(oxPts, oxHeight, oxSilt, oxWidth, oxOff[k], oxOff[k + 1], from, to,
          0.5 * w * inset, level, local.oxbowRelief, age, dry);
        set(lake, 'd', d);
        set(reed, 'd', d);
      }
      const opacity = clamp01(5 * (1 - age)).toFixed(2);
      const clear = mixRgb(WATER, CLEAR, smoothstep(age / CLEARING));
      set(lake, 'fill', mix(clear, MURKY, smoothstep(1.5 * (age - CLEARING) / (1 - CLEARING))));
      set(lake, 'opacity', opacity);
      set(reed, 'opacity', opacity);
      set(reed, 'stroke-width', (2 * reeds).toFixed(2));
    }

    resize(this.bankRuns, bankRuns.length, this.bank);
    resize(this.waterRuns, waterRuns.length, this.water);
    bankRuns.forEach((d, k) => set(this.bankRuns[k], 'd', d));
    waterRuns.forEach((d, k) => set(this.waterRuns[k], 'd', d));
    channelDepth(line, widths, frame, bend, w).forEach((runs, m) => {
      const { g, paths } = this.depth[m];
      resize(paths, runs.length, g);
      runs.forEach((d, k) => set(paths[k], 'd', d));
    });
  }

  // Contour the overbank deposits over the window after each flood, and
  // every `LEVEE_REFRESH` of simulated time as the channel sweeps them away.
  drawLevees(river, memory) {
    const t = river.time(), floods = river.floods();
    const last = this.leveeAt;
    if (last && last.floods === floods && t - last.t < LEVEE_REFRESH && t >= last.t) return;
    this.leveeAt = { floods, t };
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const deposit = new Float32Array(memory.buffer, river.deposit_ptr(), cols * rows);
    const [x, vw] = this.window;
    const c0 = Math.max(0, Math.floor(x / cell) - 1);
    const c1 = Math.min(cols, Math.ceil((x + vw) / cell) + 1);
    const top = river.get_param('levee_height');
    LEVEE_LEVELS.forEach((f, k) => {
      this.levees[k].setAttribute('d', top > 0 ? contourData(deposit, cols, rows, cell, c0, c1, f * top) : '');
    });
  }

  // Draw the clay plug marsh after each cutoff, and every `LEVEE_REFRESH`
  // of simulated time as the channel sweeps it away. The plug grid is
  // blurred over 3 × 3 cells first, so its outline follows the lake's
  // curve instead of the cell edges.
  drawPlugs(river, memory) {
    const t = river.time(), cutoffs = river.cutoffs();
    const last = this.plugAt;
    if (last && last.cutoffs === cutoffs && t - last.t < LEVEE_REFRESH && t >= last.t) return;
    this.plugAt = { cutoffs, t };
    river.sync_plugs();
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const plugs = new Float32Array(memory.buffer, river.plugs_ptr(), cols * rows);
    const [c0, c1] = this.windowColumns(cols, cell);
    const blur = this.blur(plugs, cols, rows, c0, c1);
    set(this.plugMarsh, 'd', contourData(blur, cols, rows, cell, c0, c1, 0.5));
  }

  // Draw the shrub and forest every `LEVEE_REFRESH` of simulated time, and
  // after each cutoff, which clears an oxbow's footprint. The age grid is
  // blurred like the plug grid.
  drawForest(river, memory) {
    const t = river.time(), cutoffs = river.cutoffs();
    const last = this.forestAt;
    if (last && last.cutoffs === cutoffs && t - last.t < LEVEE_REFRESH && t >= last.t) return;
    this.forestAt = { cutoffs, t };
    river.sync_forest();
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const age = new Float32Array(memory.buffer, river.forest_ptr(), cols * rows);
    const [c0, c1] = this.windowColumns(cols, cell);
    const blur = this.blur(age, cols, rows, c0, c1);
    const [shrub, forest] = [river.get_param('vegetation_time'), river.get_param('forest_time')];
    FOREST.forEach(({ at }, k) => {
      set(this.forest[k], 'd', contourData(blur, cols, rows, cell, c0, c1, shrub + at * forest));
    });
  }

  // Erodibility grid columns over the window, with a column to spare each
  // side.
  windowColumns(cols, cell) {
    const [x, vw] = this.window;
    return [Math.max(0, Math.floor(x / cell) - 1), Math.min(cols, Math.ceil((x + vw) / cell) + 1)];
  }

  // `values` on the erodibility grid averaged over 3 × 3 cells, in columns
  // [c0, c1), in a buffer reused between calls.
  blur(values, cols, rows, c0, c1) {
    if (this.blurred?.length !== cols * rows) this.blurred = new Float32Array(cols * rows);
    const out = this.blurred;
    for (let j = 0; j < rows; j++) {
      for (let i = c0; i < c1; i++) {
        let sum = 0, count = 0;
        for (let jj = Math.max(0, j - 1); jj <= Math.min(rows - 1, j + 1); jj++) {
          for (let ii = Math.max(0, i - 1); ii <= Math.min(cols - 1, i + 1); ii++) {
            sum += values[jj * cols + ii];
            count++;
          }
        }
        out[j * cols + i] = sum / count;
      }
    }
    return out;
  }

  // Draw the scroll bars in age bands when they change, and every
  // `LEVEE_REFRESH` of simulated time as they age. A band's ridges darken
  // as vegetation takes them, and fade over the last quarter of their life.
  drawScrolls(river, memory, w) {
    const t = river.time(), version = river.scroll_version();
    const last = this.scrollAt;
    if (last && last.version === version && t - last.t < LEVEE_REFRESH && t >= last.t) return;
    this.scrollAt = { version, t };
    river.sync_scrolls();
    // Views must be built after syncing: it can grow memory.
    const buf = memory.buffer;
    const n = river.scroll_count();
    const pts = new Float32Array(buf, river.scroll_line_ptr(), river.scroll_line_len());
    const off = new Uint32Array(buf, river.scroll_offsets_ptr(), n + 1);
    const ages = new Float32Array(buf, river.scroll_ages_ptr(), n);
    const life = Math.max(1e-6, river.get_param('scroll_lifetime'));
    const [x, vw] = this.window;
    // Keep the path of each ridge already drawn in its band, and add the
    // rest; what is left in `this.ridges` afterwards is gone.
    const ridges = new Map();
    for (let k = 0; k < n; k++) {
      // Skip ridges wholly outside the window.
      let inside = false;
      for (let i = off[k]; i < off[k + 1] && !inside; i++) inside = pts[2 * i] >= x - w && pts[2 * i] <= x + vw + w;
      if (!inside) continue;
      const band = Math.min(SCROLL_BANDS - 1, Math.floor(SCROLL_BANDS * ages[k] / life));
      const d = pathData(pts, off[k], off[k + 1]);
      let key = `${band}${d}`;
      if (ridges.has(key)) key += `#${k}`;
      const path = this.ridges.get(key) ?? el('path', { d }, this.scrollBands[band]);
      this.ridges.delete(key);
      ridges.set(key, path);
    }
    for (const path of this.ridges.values()) path.remove();
    this.ridges = ridges;
    set(this.scrollLayer, 'stroke-width', (0.15 * w).toFixed(2));
  }

  // Record the centreline without drawing, during warm-up.
  capture(river, memory, { local }) {
    river.sync_line();
    const line = new Float32Array(memory.buffer, river.line_ptr(), river.line_len());
    this.record(line, river.time(), river.get_param('width'), local);
  }

  // Keep the visible part of the centreline as point-bar sand once it has
  // moved `BAR_STEP` widths from the last kept one, into the newest band.
  // A band spans `barMemory / BANDS` of simulated time, and is dropped once
  // all its centrelines are older than `barMemory`. This only records the
  // bands; `drawBars` puts them on the page.
  record(line, t, w, local) {
    const [x, vw] = this.window;
    const [x0, x1] = [x - w, x + vw + w];
    const last = this.lastBar;
    // Kept points are half a width apart, so a point further than this from
    // all of them is at least `BAR_STEP` widths from the line through them.
    const r = w * Math.hypot(BAR_STEP, 0.25);
    if (last && last.size === w) {
      let moved = false;
      for (let i = 0; i < line.length && !moved; i += 2) {
        moved = line[i] >= x0 && line[i] <= x1 && !last.near(line[i], line[i + 1], r);
      }
      if (!moved) return;
    }
    const { d, pts } = cropped(line, x0, x1);
    this.lastBar = new Grid(pts, w);
    let band = this.bands.at(-1);
    if (!band || t - band.t0 >= local.barMemory / BANDS) {
      // Path data still to be added to the page, and the band's group there.
      band = { t0: t, t1: t, pending: [], g: null };
      this.bands.push(band);
    }
    band.t1 = t;
    band.pending.push(d);
    while (this.bands.length && t - this.bands[0].t1 > local.barMemory) this.dropped.push(this.bands.shift());
  }

  // Bring the bands on the page up to date: add the centrelines kept since
  // the last draw, move bands that have stopped growing to the bottom layer
  // and remove dropped ones. Shade each band by its age: bare sand when
  // new, then grassed over, fading into the background and turning
  // transparent over the soil beneath as it nears `barMemory`. The old bands are shaded only when the bottom layer
  // changes anyway, or the bar settings do, so it stays cached in between.
  drawBars(t, w, local) {
    let changed = false;
    for (const b of this.dropped) {
      changed ||= b.g?.parentNode === this.oldBars;
      b.g?.remove();
    }
    this.dropped = [];
    const newest = this.bands.at(-1);
    for (const b of this.bands) {
      const layer = b === newest ? this.barLayer : this.oldBars;
      if (!b.g) b.g = el('g', {}, layer);
      else if (b.g.parentNode !== layer) layer.append(b.g);
      else if (!b.pending.length) continue;
      changed ||= layer === this.oldBars;
      for (const d of b.pending) el('path', { d }, b.g);
      b.pending = [];
    }
    const key = `${w} ${local.barMemory} ${local.barOpacity}`;
    if (key !== this.shadedFor) changed = true;
    this.shadedFor = key;
    set(this.oldBars, 'stroke-width', w);
    set(this.barLayer, 'stroke-width', w);
    for (const b of changed ? this.bands : this.bands.slice(-1)) {
      const fresh = clamp01(1 - (t - 0.5 * (b.t0 + b.t1)) / local.barMemory);
      const sand = mixRgb(OLD_BAR, BAR, fresh * fresh);
      set(b.g, 'stroke', mix(SOIL.loam, sand, local.barOpacity * fresh));
      // Group opacity, so a band's own paths do not build up where they
      // overlap.
      set(b.g, 'opacity', smoothstep(2 * fresh).toFixed(2));
    }
  }

  // Serialise the current frame, both layers, as a standalone SVG file.
  save() {
    const copy = this.svg.cloneNode(true);
    copy.prepend(...[...this.bars.childNodes, ...this.ground.childNodes].map((n) => n.cloneNode(true)));
    copy.setAttribute('xmlns', NS);
    copy.removeAttribute('id');
    const blob = new Blob([new XMLSerializer().serializeToString(copy)], { type: 'image/svg+xml' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'river.svg';
    a.click();
    URL.revokeObjectURL(a.href);
  }
}

const svg = document.getElementById('view');
const renderer = new SvgRenderer(document.getElementById('bars'), document.getElementById('ground'), svg);
document.getElementById('save').addEventListener('click', () => renderer.save());

startApp({
  view: svg,
  renderer,
  display: {
    oxbowShrink: 0.6,
    oxbowRelief: 1,
    barMemory: 600,
    barOpacity: 0.75,
    showBars: true,
    showLevees: true,
    showScrolls: true,
  },
});
