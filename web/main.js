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
// Scroll bars: ridges of fresh sand that vegetation colonises first, so
// they darken against the bar as they age. They are kept in this many age
// bands, each one path element.
const RIDGE = [238, 230, 196];
const OLD_RIDGE = [146, 158, 108];
const SCROLL_BANDS = 6;
// Crevasse splays: a lobe of sand that grasses over, crossed by small
// distributary channels. The lobe spreads 35° either side of its axis.
const SPLAY = [246, 236, 196];
const SPLAY_RIM = [214, 198, 150];
const SPLAY_RILL = [196, 182, 138];
// Simulated time a new splay takes to spread out from the breach.
const SPLAY_GROW = 60;
// Fresh point-bar sand, and the grassed-over colour old bars fade towards.
const BAR = [233, 222, 186];
const OLD_BAR = [200, 202, 156];
// Point bars are kept in this many age bands, each one group of paths.
const BANDS = 24;
// Points on each rounded end of a lake.
const CAP_STEPS = 8;
// Fraction of its length a lake loses from each end over its lifetime.
const PLUG = 0.4;
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
// Simulated time between redraws of a growing crevasse splay.
const SPLAY_STEP = 0.5;

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

// Path data for points [start, end) of a flat x, y array in runs of `RUN`
// segments, each run sharing its last point with the next.
function runData(pts, start, end) {
  const runs = [];
  for (let s = start; s < end - 1; s += RUN) runs.push(pathData(pts, s, Math.min(end, s + RUN + 1)));
  return runs;
}

// `v` to one decimal place, without the trailing zero `toFixed` keeps,
// and several times faster.
function dp1(v) {
  return '' + Math.round(10 * v) / 10;
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

// Closed path through the points as a Catmull-Rom spline, with
// coordinates formatted by `f`.
function closedCurve(pts, f) {
  const m = pts.length;
  let d = `M${f(pts[0][0])} ${f(pts[0][1])}`;
  for (let q = 0; q < m; q++) {
    const [p0, p1, p2, p3] = [-1, 0, 1, 2].map((j) => pts[(q + j + m) % m]);
    d += `C${f(p1[0] + (p2[0] - p0[0]) / 6)} ${f(p1[1] + (p2[1] - p0[1]) / 6)} `
      + `${f(p2[0] - (p3[0] - p1[0]) / 6)} ${f(p2[1] - (p3[1] - p1[1]) / 6)} ${f(p2[0])} ${f(p2[1])}`;
  }
  return d + 'Z';
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
// the mean is part of the fill that sets `level`. Where
// the bed rises above the water the lake splits into pools, one subpath
// each, and the parabola gives each pool a round tip there. A plugged end
// is a round cap when young, lengthening with `age` in [0, 1] to a crescent
// tip. An end cut by the channel stays a round cap, hidden under the water.
function lakeData(pts, heights, silts, start, end, from, to, half, level, relief, age) {
  const m = end - start;
  if (m < 2 || level <= 0) return '';
  const s = new Float64Array(m), tx = new Float64Array(m), ty = new Float64Array(m);
  const depth = new Float64Array(m);
  let silt = 0;
  for (let k = 1; k < m - 1; k++) silt += silts[start + k] / Math.max(1, m - 2);
  // The end points lie under the channel at a cut, swept clear of silt.
  for (let k = 0; k < m; k++) {
    const extra = k > 0 && k < m - 1 ? silts[start + k] - silt : 0;
    depth[k] = level - 4 * relief * (heights[start + k] - 0.5) - extra;
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
  const width = (d) => half * Math.sqrt(Math.min(1, Math.max(0, d)));

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
    if (v0 - u0 < half) continue;
    // Each end: a round cap of the local width at a plugged or cut end, or
    // a parabolic tip where the bed rises above the water, sampled densely
    // near the tip.
    const end = (at, plugged) => {
      if (at !== a && at !== b) return { len: half, cap: false };
      const [jj, uu] = locate(at, 0);
      const r = width(depthAt(jj, uu));
      return { len: plugged ? r * (1 + 3 * age) : r, cap: true };
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
      const h = shape * width(depthAt(j, f)) / (Math.hypot(nx, ny) || 1);
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
    // Point bars: each age band is one group of opaque paths in one colour,
    // newest on top, so each spot shows the newest band over it and bands
    // never build up. Old bands fade to the background colour, and the
    // translucent soil patches lie over them. Bands that have stopped
    // growing lie in the bottom layer; the growing band lies in the ground
    // layer, so keeping a centreline does not repaint the old ones.
    this.oldBars = el('g', line, bars);
    this.barLayer = el('g', line, ground);
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
    this.soilLayer = el('g', {}, ground);
    // Splays bury the soil and levees beneath them in sand.
    this.splayLayer = el('g', round, ground);
    this.splayAt = null;
    // Oxbow scars lie over the whole ground, under the channel. They change
    // slowly, so they sit at the top of the ground layer, not in the top
    // layer that is redrawn whenever the river moves.
    this.scarLayer = el('g', { ...line, stroke: MARSH }, ground);
    // The bank lies under the lakes, so a lake that meets the channel joins
    // its water with no strip of bank between them.
    this.bank = el('g', { ...line, stroke: '#b8a678' }, svg);
    this.lakeLayer = el('g', { 'stroke-linejoin': 'round', stroke: REEDS }, svg);
    this.water = el('g', { ...line, stroke: `rgb(${WATER})` }, svg);
    this.bankRuns = [];
    this.waterRuns = [];
    this.lakes = [];
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
    this.scrollAt = null;
    this.splayAt = null;
    for (const scar of this.scars) scar.shape = null;
    for (const lake of this.lakes) lake.inputs = null;
    this.drawSoils(river, x, w, h, seed);
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
      [this.scrollLayer, local.showScrolls], [this.splayLayer, local.showSplays],
    ];
    for (const [layer, show] of layers) {
      if (layer.style.display !== (show ? '' : 'none')) layer.style.display = show ? '' : 'none';
    }
    if (local.showLevees) this.drawLevees(river, memory);
    if (local.showScrolls) this.drawScrolls(river, memory, w);
    if (local.showSplays) this.drawSplays(river, memory, w);

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
    const oxSilt = new Float32Array(buf, river.oxbow_silts_ptr(), oxPts.length / 2);
    const scarLife = Math.max(1e-6, river.get_param('scar_lifetime'));
    const runs = runData(line, 0, line.length / 2);

    this.record(line, river.time(), w, local);
    this.drawBars(river.time(), w, local);

    // Oxbows: a lake that plugs from the ends, narrows far more slowly from
    // the banks and turns murky, fading only over the last fifth of its
    // fill, then a marshy scar that fades out. Both plugs and banks advance
    // steadily as it fills, and the water drains first from where the land
    // is high or floods have laid the most silt, splitting the lake into
    // pools. The reed fringe grows inwards, so it does not hide the
    // narrowing.
    resize(this.scars, n, this.scarLayer);
    resize(this.lakes, n, this.lakeLayer);
    set(this.scarLayer, 'stroke-width', w);
    for (let k = 0; k < n; k++) {
      const age = oxFill[k];
      const scar = this.scars[k], lake = this.lakes[k];
      // The channel splits an oxbow into pieces of its original polyline,
      // so a piece's ends and length identify its shape.
      const [s0, s1] = [2 * oxOff[k], 2 * oxOff[k + 1]];
      const shape = `${s1 - s0} ${oxPts[s0]} ${oxPts[s0 + 1]} ${oxPts[s1 - 2]} ${oxPts[s1 - 1]}`;
      if (scar.shape !== shape) {
        scar.shape = shape;
        set(scar, 'd', pathData(oxPts, oxOff[k], oxOff[k + 1]));
      }
      const fade = 1 - smoothstep(oxScar[k] / scarLife);
      set(scar, 'stroke-opacity', (0.45 * smoothstep(age) * fade).toFixed(2));
      if (age >= 1) {
        // Elements are matched to oxbows by index, so a lake can come back
        // to this element when the channel splits or drops an oxbow before
        // it. Forget its inputs, or it would stay blank.
        set(lake, 'd', '');
        lake.inputs = null;
        continue;
      }
      const reeds = 0.1 * w + 0.15 * w * age;
      // Half-width where the bed is at its mean height.
      const hw = 0.5 * w * (1 - local.oxbowShrink * age) - 0.5 * reeds;
      // The plugs advance from the ends of the oxbow as cut off, which a
      // piece split off by the channel may lack.
      const plug = PLUG * age * oxLen[k];
      const [from, to] = [plug - oxAlong[k], oxLen[k] - plug - oxAlong[k]];
      const level = hw > 0 ? (hw / (0.5 * w)) ** 2 : 0;
      // Floods and the channel change the silt without reshaping the oxbow.
      let silt = 0;
      for (let i = oxOff[k]; i < oxOff[k + 1]; i++) silt += oxSilt[i];
      const inputs = `${shape} ${Math.round(age / LAKE_STEP)} ${silt} ${w} ${local.oxbowShrink} ${local.oxbowRelief}`;
      if (lake.inputs !== inputs) {
        lake.inputs = inputs;
        set(lake, 'd', lakeData(oxPts, oxHeight, oxSilt, oxOff[k], oxOff[k + 1], from, to,
          0.5 * w, level, local.oxbowRelief, age));
      }
      set(lake, 'fill', mix(WATER, MURKY, smoothstep(1.5 * age)));
      set(lake, 'opacity', clamp01(5 * (1 - age)).toFixed(2));
      set(lake, 'stroke-width', reeds.toFixed(2));
    }

    resize(this.bankRuns, runs.length, this.bank);
    resize(this.waterRuns, runs.length, this.water);
    runs.forEach((d, k) => {
      set(this.bankRuns[k], 'd', d);
      set(this.waterRuns[k], 'd', d);
    });
    set(this.bank, 'stroke-width', w * 1.3);
    set(this.water, 'stroke-width', w);
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

  // Draw each crevasse splay as a smooth lobe of sand with a few rills
  // from the breach. A new splay spreads out from the breach over
  // `SPLAY_GROW` of simulated time, redrawn every `SPLAY_STEP`; after that
  // it is redrawn when the channel erodes it, and every `LEVEE_REFRESH` as
  // it grasses over.
  drawSplays(river, memory, w) {
    const t = river.time(), n = river.splay_count(), version = river.splay_version();
    const stride = river.splay_stride(), rays = (stride - 6) / 4;
    const last = this.splayAt;
    if (last && last.version === version && t >= last.t
      && t - last.t < (t < last.growing ? SPLAY_STEP : LEVEE_REFRESH)) return;
    river.sync_splays();
    // Views must be built after syncing: it can grow memory.
    const data = new Float32Array(memory.buffer, river.splay_data_ptr(), stride * n);
    // Time until which some splay is still growing.
    let growing = -Infinity;
    for (let k = 0; k < n; k++) growing = Math.max(growing, data[stride * k + 5] + SPLAY_GROW);
    this.splayAt = { version, t, growing };
    const life = Math.max(1e-6, river.get_param('splay_lifetime'));
    this.splayLayer.replaceChildren();
    const f = (v) => v.toFixed(1);
    for (let k = 0; k < n; k++) {
      const o = stride * k;
      const [ax, ay, , , len, born] = data.subarray(o, o + 6);
      const age = clamp01((t - born) / life);
      const grown = 1 - (1 - clamp01((t - born) / SPLAY_GROW)) ** 2;
      const rand = random(Math.round(born * 10) ^ 0x5b1a);
      // Each ray's remaining sand, cut back to the spreading front.
      const ray = (j) => {
        const [vx, vy, a, b] = data.subarray(o + 6 + 4 * j, o + 10 + 4 * j);
        return { vx, vy, a, b: Math.min(b, grown * len) };
      };
      const span = Array.from({ length: rays }, (_, j) => ray(j));
      const live = span.map((r) => r.b > r.a + 0.25 * w);
      const at = (r, d) => [ax + d * r.vx, ay + d * r.vy];
      const fade = 1 - smoothstep((age - 0.6) / 0.4);
      // Each run of adjacent rays with sand left is one lobe: out along
      // the outer ends and back along the inner ones.
      let d = '';
      for (let j = 0; j < rays;) {
        if (!live[j]) { j++; continue; }
        let e = j;
        while (e + 1 < rays && live[e + 1]) e++;
        if (e > j) {
          const run = span.slice(j, e + 1);
          d += closedCurve([...run.map((r) => at(r, r.b)), ...run.reverse().map((r) => at(r, r.a))], f);
        }
        j = e + 1;
      }
      if (!d) continue;
      el('path', {
        d, fill: mix(SPLAY, OLD_BAR, smoothstep(1.5 * age)), 'fill-opacity': (0.9 * fade).toFixed(2),
        stroke: `rgb(${SPLAY_RIM})`, 'stroke-width': (0.1 * w).toFixed(2), 'stroke-opacity': (0.7 * fade).toFixed(2),
      }, this.splayLayer);
      // Rills: gentle curves outwards between two rays, over what is left
      // of the sand.
      let rills = '';
      for (let r = 0; r < 3; r++) {
        const u = (rays - 1) * (0.2 + 0.3 * r + 0.1 * (rand() - 0.5));
        const j = Math.floor(u), s = u - j;
        const wobble = (rand() - 0.5) * w, reach = (0.55 + 0.2 * rand()) * len;
        if (!live[j] || !live[j + 1]) continue;
        const [p, q] = [span[j], span[j + 1]];
        const lerp = (x, y) => x + s * (y - x);
        const v = { vx: lerp(p.vx, q.vx), vy: lerp(p.vy, q.vy) };
        const [from, to] = [Math.max(lerp(p.a, q.a), 0), Math.min(lerp(p.b, q.b), reach)];
        if (to <= from) continue;
        const [a, b] = [at(v, from), at(v, to)];
        const c = at(v, (from + to) / 2);
        rills += `M${f(a[0])} ${f(a[1])}Q${f(c[0] - wobble * v.vy)} ${f(c[1] + wobble * v.vx)} ${f(b[0])} ${f(b[1])}`;
      }
      if (rills) {
        el('path', {
          d: rills, fill: 'none', stroke: `rgb(${SPLAY_RILL})`, 'stroke-width': (0.12 * w).toFixed(2),
          'stroke-opacity': (0.8 * fade * (1 - age)).toFixed(2),
        }, this.splayLayer);
      }
    }
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
  // new, then grassed over and fading into the background as it nears
  // `barMemory`. The old bands are shaded only when the bottom layer
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
  display: [
    { key: 'oxbowShrink', label: 'Oxbow narrowing', min: 0, max: 0.95, step: 0.05, local: 0.6 },
    { key: 'oxbowRelief', label: 'Oxbow bed relief', min: 0, max: 2, step: 0.05, local: 1 },
    { key: 'barMemory', label: 'Point bar memory (sim time)', min: 50, max: 5000, step: 50, local: 600 },
    { key: 'barOpacity', label: 'Point bar strength', min: 0.1, max: 1, step: 0.05, local: 0.75 },
    { key: 'showBars', label: 'Show point bars (past centrelines)', toggle: true, local: true },
    { key: 'showLevees', label: 'Show natural levees', toggle: true, local: true },
    { key: 'showScrolls', label: 'Show scroll bars', toggle: true, local: true },
    { key: 'showSplays', label: 'Show crevasse splays', toggle: true, local: true },
  ],
});
