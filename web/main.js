// SVG view: the channel, oxbows and past centrelines as stroked paths,
// rewritten every frame.

import { startApp } from './ui.js';

const NS = 'http://www.w3.org/2000/svg';
const WATER = [61, 126, 166];
const SILT = [128, 150, 110];

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

function mix(a, b, t) {
  return `rgb(${a.map((x, i) => Math.round(x + (b[i] - x) * t))})`;
}

class SvgRenderer {
  constructor(svg) {
    this.svg = svg;
    svg.setAttribute('preserveAspectRatio', 'xMidYMid meet');
    const round = { fill: 'none', 'stroke-linejoin': 'round', 'stroke-linecap': 'round' };
    this.background = el('rect', { y: 0, fill: '#cfc6a4' }, svg);
    this.historyLayer = el('g', { ...round, stroke: '#ddd0a6' }, svg);
    this.oxbowLayer = el('g', round, svg);
    this.bank = el('path', { ...round, stroke: '#b8a678' }, svg);
    this.water = el('path', { ...round, stroke: `rgb(${WATER})` }, svg);
    this.sheen = el('path', { ...round, stroke: '#cfe3ea', 'stroke-opacity': 0.55 }, svg);
    this.oxbows = [];
    this.history = [];
    this.lastCapture = 0;
  }

  // Show only the visible window; the hidden reaches fall outside it.
  reset(river, [w, h]) {
    const x = river.view_x();
    this.svg.setAttribute('viewBox', `${x} 0 ${w} ${h}`);
    this.background.setAttribute('x', x);
    this.background.setAttribute('width', w);
    this.background.setAttribute('height', h);
    this.historyLayer.replaceChildren();
    this.history = [];
    this.lastCapture = -Infinity;
  }

  draw(river, memory, { local, clock }) {
    river.sync();
    // Views must be rebuilt after every step: memory growth detaches them.
    const buf = memory.buffer;
    const n = river.oxbow_count();
    const line = new Float32Array(buf, river.line_ptr(), river.line_len());
    const oxPts = new Float32Array(buf, river.oxbow_line_ptr(), river.oxbow_line_len());
    const oxOff = new Uint32Array(buf, river.oxbow_offsets_ptr(), n + 1);
    const oxAge = new Float32Array(buf, river.oxbow_ages_ptr(), n);
    const w = river.get_param('width');
    const d = pathData(line, 0, line.length / 2);

    this.historyLayer.style.display = local.showHistory ? '' : 'none';
    this.historyLayer.setAttribute('stroke-width', w);
    this.record(d, river.time(), local);

    // Oxbows, reusing path elements.
    while (this.oxbows.length < n) this.oxbows.push(el('path', {}, this.oxbowLayer));
    while (this.oxbows.length > n) this.oxbows.pop().remove();
    for (let k = 0; k < n; k++) {
      const age = oxAge[k];
      const p = this.oxbows[k];
      p.setAttribute('d', pathData(oxPts, oxOff[k], oxOff[k + 1]));
      p.setAttribute('stroke', mix(WATER, SILT, Math.min(1, 1.5 * age)));
      p.setAttribute('stroke-opacity', (1 - 0.9 * age).toFixed(2));
      p.setAttribute('stroke-width', (w * (1 - local.oxbowShrink * age)).toFixed(2));
    }

    this.bank.setAttribute('d', d);
    this.bank.setAttribute('stroke-width', w * 1.3);
    this.water.setAttribute('d', d);
    this.water.setAttribute('stroke-width', w);
    // Dashes along the centreline, shifted downstream over time.
    this.sheen.style.display = local.flow > 0 ? '' : 'none';
    this.sheen.setAttribute('d', d);
    this.sheen.setAttribute('stroke-width', w * 0.18);
    this.sheen.setAttribute('stroke-dasharray', `${w * 0.6} ${w * 2.4}`);
    this.sheen.setAttribute('stroke-dashoffset', (-clock * local.flow * w * 3).toFixed(1));
  }

  // Record the centreline without drawing, during warm-up.
  capture(river, memory, { local }) {
    river.sync();
    const line = new Float32Array(memory.buffer, river.line_ptr(), river.line_len());
    this.record(pathData(line, 0, line.length / 2), river.time(), local);
  }

  // Past centrelines, kept every `historyEvery` units of simulated time in a
  // ring of `historyLength` paths, older ones fainter.
  record(d, t, local) {
    if (t - this.lastCapture < local.historyEvery) return;
    this.lastCapture = t;
    this.history.push(el('path', { d }, this.historyLayer));
    while (this.history.length > local.historyLength) this.history.shift().remove();
    const k = this.history.length;
    this.history.forEach((p, i) => p.setAttribute('stroke-opacity', (0.08 + 0.32 * (i + 1) / k).toFixed(2)));
  }

  // Serialise the current frame as a standalone SVG file.
  save() {
    const copy = this.svg.cloneNode(true);
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
const renderer = new SvgRenderer(svg);
document.getElementById('save').addEventListener('click', () => renderer.save());

startApp({
  view: svg,
  renderer,
  display: [
    { key: 'flow', label: 'Flow dash speed', min: 0, max: 3, step: 0.1, local: 1 },
    { key: 'oxbowShrink', label: 'Oxbow narrowing', min: 0, max: 0.95, step: 0.05, local: 0.6 },
    { key: 'historyEvery', label: 'History interval (sim time)', min: 2, max: 200, step: 2, local: 20 },
    { key: 'historyLength', label: 'History length (paths)', min: 1, max: 200, step: 1, local: 60 },
    { key: 'showHistory', label: 'Show past centrelines', toggle: true, local: true },
  ],
});
