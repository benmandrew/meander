import init, { River } from './pkg/meander_wasm.js';

// World height is fixed; world width follows the window's aspect ratio at
// each reset.
const WORLD_H = 700;

const SLIDERS = [
  { group: 'Dynamics' },
  { key: 'erodibility', label: 'Erodibility E', min: 0, max: 3, step: 0.05 },
  { key: 'omega', label: 'Local weight Ω', min: -3, max: 1, step: 0.05 },
  { key: 'gamma', label: 'Upstream weight Γ', min: 0, max: 5, step: 0.05 },
  { key: 'alpha', label: 'Upstream decay α (per width)', min: 0.05, max: 2, step: 0.01 },
  { key: 'dt', label: 'Timestep dt', min: 0.01, max: 0.3, step: 0.01 },
  { key: 'stepsPerFrame', label: 'Steps per frame', min: 1, max: 100, step: 1, local: 20 },
  { key: 'smoothing', label: 'Smoothing k', min: 0, max: 0.25, step: 0.01 },
  { key: 'max_step', label: 'Step limit (× spacing)', min: 0.05, max: 0.3, step: 0.01 },
  { group: 'Channel' },
  { key: 'width', label: 'Channel width w', min: 2, max: 20, step: 0.5 },
  { key: 'edge_margin', label: 'Edge margin (widths)', min: 0, max: 30, step: 1 },
  { group: 'Cutoffs' },
  { key: 'cutoff_dist', label: 'Cutoff distance (widths)', min: 0.3, max: 3, step: 0.05 },
  { key: 'cutoff_min_sep', label: 'Min separation K (points)', min: 6, max: 100, step: 1 },
  { key: 'oxbow_lifetime', label: 'Oxbow lifetime', min: 0, max: 2000, step: 10 },
  { group: 'Noise' },
  { key: 'jitter', label: 'Jitter per step', min: 0, max: 0.5, step: 0.01 },
  { key: 'noise', label: 'Initial noise (widths, on reset)', min: 0, max: 2, step: 0.05, local: 0.5 },
];

const COLOURS = {
  floodplain: '#cfc6a4',
  bank: '#b8a678',
  water: [61, 126, 166],
  silt: [128, 150, 110],
};

const wasm = await init();
const canvas = document.getElementById('view');
const ctx = canvas.getContext('2d');
const local = {};
let river;
let running = true;
let frameMs = 0;

function worldSize() {
  return [WORLD_H * canvas.clientWidth / canvas.clientHeight, WORLD_H];
}

function seed() {
  return Number(document.getElementById('seed').value) >>> 0;
}

function reset() {
  const [w, h] = worldSize();
  if (river) {
    river.reset(w, h, seed(), local.noise);
  } else {
    river = new River(w, h, seed(), local.noise);
  }
  for (const s of SLIDERS) {
    if (s.key && s.local === undefined) river.set_param(s.key, Number(s.input.value));
  }
}

function buildSliders() {
  const root = document.getElementById('sliders');
  const defaults = new River(100, 100, 0, 0);
  for (const s of SLIDERS) {
    if (s.group) {
      const h = document.createElement('h2');
      h.textContent = s.group;
      root.append(h);
      continue;
    }
    const value = s.local ?? defaults.get_param(s.key);
    const row = document.createElement('div');
    row.className = 'slider';
    row.innerHTML = `<label for="p-${s.key}">${s.label}</label><output></output>
      <input id="p-${s.key}" type="range" min="${s.min}" max="${s.max}" step="${s.step}">`;
    s.input = row.querySelector('input');
    const out = row.querySelector('output');
    s.input.value = value;
    const apply = () => {
      const v = Number(s.input.value);
      out.textContent = String(+v.toFixed(3));
      if (s.local !== undefined) local[s.key] = v;
      else river?.set_param(s.key, v);
    };
    s.input.addEventListener('input', apply);
    apply();
    root.append(row);
  }
  defaults.free();
}

function resize() {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.round(canvas.clientWidth * dpr);
  canvas.height = Math.round(canvas.clientHeight * dpr);
}

function strokePath(pts, start, end) {
  ctx.beginPath();
  ctx.moveTo(pts[2 * start], pts[2 * start + 1]);
  for (let i = start + 1; i < end; i++) ctx.lineTo(pts[2 * i], pts[2 * i + 1]);
  ctx.stroke();
}

function mix(a, b, t) {
  return a.map((x, i) => Math.round(x + (b[i] - x) * t));
}

function draw() {
  river.sync();
  // Views must be rebuilt after every step: memory growth detaches them.
  const buf = wasm.memory.buffer;
  const line = new Float32Array(buf, river.line_ptr(), river.line_len());
  const nOx = river.oxbow_count();
  const oxPts = new Float32Array(buf, river.oxbow_points_ptr(), river.oxbow_points_len());
  const oxOff = new Uint32Array(buf, river.oxbow_offsets_ptr(), nOx + 1);
  const oxAge = new Float32Array(buf, river.oxbow_ages_ptr(), nOx);

  const dpr = window.devicePixelRatio || 1;
  const [ww, wh] = worldSize();
  const scale = Math.min(canvas.width / ww, canvas.height / wh);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = COLOURS.floodplain;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.setTransform(scale, 0, 0, scale,
    (canvas.width - ww * scale) / 2, (canvas.height - wh * scale) / 2);
  ctx.lineJoin = 'round';
  ctx.lineCap = 'round';

  const w = river.get_param('width');
  for (let k = 0; k < nOx; k++) {
    const age = oxAge[k];
    const [r, g, b] = mix(COLOURS.water, COLOURS.silt, Math.min(1, age * 1.5));
    ctx.strokeStyle = `rgba(${r},${g},${b},${1 - 0.9 * age})`;
    ctx.lineWidth = w * (1 - 0.65 * age);
    strokePath(oxPts, oxOff[k], oxOff[k + 1]);
  }

  const n = line.length / 2;
  ctx.strokeStyle = COLOURS.bank;
  ctx.lineWidth = w + 1.5 / scale * dpr;
  strokePath(line, 0, n);
  ctx.strokeStyle = `rgb(${COLOURS.water})`;
  ctx.lineWidth = w;
  strokePath(line, 0, n);

  document.getElementById('stats').textContent =
    `t ${river.time().toFixed(1)}   sinuosity ${river.sinuosity().toFixed(2)}\n` +
    `points ${river.point_count()}   cutoffs ${river.cutoffs()}   oxbows ${nOx}\n` +
    `${frameMs.toFixed(1)} ms/frame (sim)`;
}

function frame() {
  if (running) {
    const t0 = performance.now();
    river.step(local.stepsPerFrame);
    frameMs = 0.9 * frameMs + 0.1 * (performance.now() - t0);
  }
  draw();
  requestAnimationFrame(frame);
}

function togglePlay() {
  running = !running;
  document.getElementById('play').textContent = running ? 'Pause' : 'Play';
}

document.getElementById('play').addEventListener('click', togglePlay);
document.getElementById('step').addEventListener('click', () => river.step(local.stepsPerFrame));
document.getElementById('reset').addEventListener('click', reset);
document.getElementById('reseed').addEventListener('click', () => {
  document.getElementById('seed').value = Math.floor(Math.random() * 1e6);
  reset();
});
document.getElementById('seed').addEventListener('change', reset);
document.getElementById('collapse').addEventListener('click', () => {
  document.getElementById('panel').hidden = true;
  document.getElementById('expand').hidden = false;
});
document.getElementById('expand').addEventListener('click', () => {
  document.getElementById('panel').hidden = false;
  document.getElementById('expand').hidden = true;
});
window.addEventListener('keydown', (e) => {
  if (e.target instanceof HTMLInputElement && e.target.type !== 'range') return;
  if (e.key === ' ') { e.preventDefault(); togglePlay(); }
  else if (e.key === 'r') reset();
  else if (e.key === 's') river.step(local.stepsPerFrame);
});
window.addEventListener('resize', resize);

resize();
buildSliders();
reset();
// ?warmup=N runs N steps before the first frame, to open on a mature river.
river.step(Number(new URLSearchParams(location.search).get('warmup')) || 0);
requestAnimationFrame(frame);
