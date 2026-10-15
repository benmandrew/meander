// SVG view in two stacked layers. The ground layer holds past centrelines
// as point-bar sand under the soil patches, and changes only when a
// centreline is kept, so the browser can cache it. The top layer holds the
// oxbow lakes, their scars and the channel, redrawn every frame.

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
// Fresh point-bar sand, and the grassed-over colour old bars fade towards.
const BAR = [233, 222, 186];
const OLD_BAR = [200, 202, 156];
// Point bars are kept in this many age bands, each one path element.
const BANDS = 24;
// Points on each rounded end of a lake.
const CAP_STEPS = 8;
// Fraction of its length a lake loses from each end over its lifetime.
const PLUG = 0.4;
// A centreline is kept once some part of it has moved this many widths from
// the last kept one, so the swept area has no gaps.
const BAR_STEP = 0.5;

function el(name, attrs, parent) {
  const e = document.createElementNS(NS, name);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  parent?.append(e);
  return e;
}

// Path data for points [start, end) of a flat x, y array.
function pathData(pts, start, end) {
  if (end - start < 2) return '';
  const parts = new Array(end - start);
  for (let i = start; i < end; i++) {
    parts[i - start] = pts[2 * i].toFixed(1) + ' ' + pts[2 * i + 1].toFixed(1);
  }
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
// is `relief` × 4 × (height − 0.5) for the land heights in `heights`. Where
// the bed rises above the water the lake splits into pools, one subpath
// each, and the parabola gives each pool a round tip there. A plugged end
// is a round cap when young, lengthening with `age` in [0, 1] to a crescent
// tip. An end cut by the channel stays a round cap, hidden under the water.
function lakeData(pts, heights, start, end, from, to, half, level, relief, age) {
  const m = end - start;
  if (m < 2 || level <= 0) return '';
  const s = new Float64Array(m), tx = new Float64Array(m), ty = new Float64Array(m);
  const depth = new Float64Array(m);
  for (let k = 0; k < m; k++) depth[k] = level - 4 * relief * (heights[start + k] - 0.5);
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
class Grid {
  constructor(pts, size) {
    this.pts = pts;
    this.size = size;
    this.cells = new Map();
    for (let i = 0; i < pts.length; i += 2) {
      const k = this.key(Math.floor(pts[i] / size), Math.floor(pts[i + 1] / size));
      const cell = this.cells.get(k);
      if (cell) cell.push(i);
      else this.cells.set(k, [i]);
    }
  }

  key(cx, cy) {
    return cx * 65536 + cy;
  }

  // Whether any point lies within `r` (at most `size`) of (x, y).
  near(x, y, r) {
    const cx = Math.floor(x / this.size), cy = Math.floor(y / this.size);
    for (let i = -1; i <= 1; i++) {
      for (let j = -1; j <= 1; j++) {
        for (const k of this.cells.get(this.key(cx + i, cy + j)) ?? []) {
          const dx = this.pts[k] - x, dy = this.pts[k + 1] - y;
          if (dx * dx + dy * dy <= r * r) return true;
        }
      }
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
  constructor(ground, svg) {
    this.ground = ground;
    this.svg = svg;
    for (const s of [ground, svg]) s.setAttribute('preserveAspectRatio', 'xMidYMid meet');
    const round = { fill: 'none', 'stroke-linejoin': 'round', 'stroke-linecap': 'round' };
    this.background = el('rect', { y: 0, fill: `rgb(${SOIL.loam})` }, ground);
    // Point bars: each age band is one opaque path, newest on top, so each
    // spot shows the newest band over it and bands never build up. Old bands
    // fade to the background colour, and the translucent soil patches lie
    // over them.
    this.barLayer = el('g', round, ground);
    this.soilLayer = el('g', {}, ground);
    this.scarLayer = el('g', { ...round, stroke: MARSH }, svg);
    this.lakeLayer = el('g', { 'stroke-linejoin': 'round', stroke: REEDS }, svg);
    this.bank = el('path', { ...round, stroke: '#b8a678' }, svg);
    this.water = el('path', { ...round, stroke: `rgb(${WATER})` }, svg);
    this.lakes = [];
    this.scars = [];
    this.bands = [];
    this.lastBar = null;
    this.window = [0, 0];
  }

  // Show only the visible window; the hidden reaches fall outside it.
  reset(river, [w, h], seed) {
    const x = river.view_x();
    for (const s of [this.ground, this.svg]) s.setAttribute('viewBox', `${x} 0 ${w} ${h}`);
    this.background.setAttribute('x', x);
    this.background.setAttribute('width', w);
    this.background.setAttribute('height', h);
    this.barLayer.replaceChildren();
    this.bands = [];
    this.lastBar = null;
    this.window = [x, w];
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
    river.sync();
    // Views must be rebuilt after every step: memory growth detaches them.
    const buf = memory.buffer;
    const n = river.oxbow_count();
    const line = new Float32Array(buf, river.line_ptr(), river.line_len());
    const oxPts = new Float32Array(buf, river.oxbow_line_ptr(), river.oxbow_line_len());
    const oxOff = new Uint32Array(buf, river.oxbow_offsets_ptr(), n + 1);
    const oxAge = new Float32Array(buf, river.oxbow_ages_ptr(), n);
    const oxLen = new Float32Array(buf, river.oxbow_lengths_ptr(), n);
    const oxAlong = new Float32Array(buf, river.oxbow_offsets_along_ptr(), n);
    const oxHeight = new Float32Array(buf, river.oxbow_heights_ptr(), oxPts.length / 2);
    const w = river.get_param('width');
    const life = Math.max(1e-6, river.get_param('oxbow_lifetime'));
    const scarLife = Math.max(1e-6, river.get_param('scar_lifetime'));
    const d = pathData(line, 0, line.length / 2);

    this.record(line, river.time(), w, local);
    if (this.barLayer.style.display !== (local.showBars ? '' : 'none')) {
      this.barLayer.style.display = local.showBars ? '' : 'none';
    }

    // Oxbows: a lake that plugs from the ends, narrows far more slowly from
    // the banks and turns murky, fading only over the last fifth of its
    // life, then a marshy scar that fades out. Both plugs and banks advance
    // steadily with age, and the water drains first from where the land
    // is high, splitting the lake into pools. The reed fringe grows
    // inwards, so it does not hide the narrowing.
    resize(this.scars, n, this.scarLayer);
    resize(this.lakes, n, this.lakeLayer);
    this.scarLayer.setAttribute('stroke-width', w);
    for (let k = 0; k < n; k++) {
      const age = oxAge[k] / life;
      const scar = this.scars[k], lake = this.lakes[k];
      scar.setAttribute('d', pathData(oxPts, oxOff[k], oxOff[k + 1]));
      const fade = 1 - smoothstep((oxAge[k] - life) / scarLife);
      scar.setAttribute('stroke-opacity', (0.45 * smoothstep(age) * fade).toFixed(2));
      if (age >= 1) {
        lake.setAttribute('d', '');
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
      lake.setAttribute('d', lakeData(oxPts, oxHeight, oxOff[k], oxOff[k + 1], from, to,
        0.5 * w, level, local.oxbowRelief, age));
      lake.setAttribute('fill', mix(WATER, MURKY, smoothstep(1.5 * age)));
      lake.setAttribute('opacity', clamp01(5 * (1 - age)).toFixed(2));
      lake.setAttribute('stroke-width', reeds.toFixed(2));
    }

    this.bank.setAttribute('d', d);
    this.bank.setAttribute('stroke-width', w * 1.3);
    this.water.setAttribute('d', d);
    this.water.setAttribute('stroke-width', w);
  }

  // Record the centreline without drawing, during warm-up.
  capture(river, memory, { local }) {
    river.sync();
    const line = new Float32Array(memory.buffer, river.line_ptr(), river.line_len());
    this.record(line, river.time(), river.get_param('width'), local);
  }

  // Keep the visible part of the centreline as point-bar sand once it has
  // moved `BAR_STEP` widths from the last kept one, into the newest band.
  // A band spans `barMemory / BANDS` of simulated time, and is dropped once
  // all its centrelines are older than `barMemory`.
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
      band = { t0: t, t1: t, d: '', path: el('path', {}, this.barLayer) };
      this.bands.push(band);
    }
    band.t1 = t;
    band.d += d;
    band.path.setAttribute('d', band.d);
    while (this.bands.length && t - this.bands[0].t1 > local.barMemory) this.bands.shift().path.remove();
    this.shadeBars(t, w, local);
  }

  // Shade each band by its age: bare sand when new, then grassed over and
  // fading into the background as it nears `barMemory`. This runs only when
  // a centreline is kept, so the ground layer stays cached in between.
  shadeBars(t, w, local) {
    this.barLayer.setAttribute('stroke-width', w);
    for (const b of this.bands) {
      const fresh = clamp01(1 - (t - 0.5 * (b.t0 + b.t1)) / local.barMemory);
      const sand = mixRgb(OLD_BAR, BAR, fresh * fresh);
      b.path.setAttribute('stroke', mix(SOIL.loam, sand, local.barOpacity * fresh));
    }
  }

  // Serialise the current frame, both layers, as a standalone SVG file.
  save() {
    const copy = this.svg.cloneNode(true);
    copy.prepend(...[...this.ground.childNodes].map((n) => n.cloneNode(true)));
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
const renderer = new SvgRenderer(document.getElementById('ground'), svg);
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
  ],
});
