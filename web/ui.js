// Shared page logic: WASM start-up, the control panel, and the animation
// loop. Each page supplies a renderer and its own display controls.

import init, { River } from './pkg/meander_wasm.js';

// World height is fixed; world width follows the view's aspect ratio at
// each reset.
const WORLD_H = 700;

// Sliders with `local` set are page settings; the rest are simulation
// parameters, read from and written to the WASM side by name.
const SIM_CONTROLS = [
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
  { group: 'Erodibility' },
  { key: 'heterogeneity', label: 'Heterogeneity', min: 0, max: 1, step: 0.05 },
  { key: 'clay_factor', label: 'Oxbow clay plugs (× E)', min: 0.05, max: 1, step: 0.05 },
];

// `renderer` provides resize() and draw(river, memory, state), and may
// provide reset(bounds). `display` lists extra page-specific controls.
export async function startApp({ view, renderer, display }) {
  const wasm = await init();
  const controls = [...SIM_CONTROLS, { group: 'Display' }, ...display];
  const local = {};
  const state = { bounds: [1, 1], local, clock: 0, steps: 0 };
  let river;
  let running = true;
  let frameMs = 0;
  const $ = (id) => document.getElementById(id);

  function reset() {
    const r = view.getBoundingClientRect();
    state.bounds = [WORLD_H * r.width / r.height, WORLD_H];
    const seed = Number($('seed').value) >>> 0;
    if (river) river.reset(...state.bounds, seed, local.noise);
    else river = new River(...state.bounds, seed, local.noise);
    for (const s of controls) {
      if (s.key && !s.toggle && s.local === undefined) river.set_param(s.key, Number(s.input.value));
    }
    state.steps = 0;
    renderer.reset?.(state.bounds);
  }

  function buildControls() {
    const root = $('sliders');
    const defaults = new River(100, 100, 0, 0);
    for (const s of controls) {
      if (s.group) {
        const h = document.createElement('h2');
        h.textContent = s.group;
        root.append(h);
        continue;
      }
      if (s.toggle) {
        const row = document.createElement('label');
        row.className = 'toggle';
        row.innerHTML = `<input type="checkbox"> ${s.label}`;
        const box = row.querySelector('input');
        box.checked = s.local;
        local[s.key] = s.local;
        box.addEventListener('change', () => { local[s.key] = box.checked; });
        root.append(row);
        continue;
      }
      const row = document.createElement('div');
      row.className = 'slider';
      row.innerHTML = `<label for="p-${s.key}">${s.label}</label><output></output>
        <input id="p-${s.key}" type="range" min="${s.min}" max="${s.max}" step="${s.step}">`;
      s.input = row.querySelector('input');
      const out = row.querySelector('output');
      s.input.value = s.local ?? defaults.get_param(s.key);
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

  function step(n) {
    river.step(n);
    state.steps += n;
  }

  function frame(now) {
    if (running) {
      const t0 = performance.now();
      step(local.stepsPerFrame);
      frameMs = 0.9 * frameMs + 0.1 * (performance.now() - t0);
    }
    state.clock = now / 1000;
    renderer.draw(river, wasm.memory, state);
    $('stats').textContent =
      `t ${river.time().toFixed(1)}   sinuosity ${river.sinuosity().toFixed(2)}\n` +
      `points ${river.point_count()}   cutoffs ${river.cutoffs()}   oxbows ${river.oxbow_count()}\n` +
      `${frameMs.toFixed(1)} ms/frame (sim)`;
    requestAnimationFrame(frame);
  }

  function togglePlay() {
    running = !running;
    $('play').textContent = running ? 'Pause' : 'Play';
  }

  $('play').addEventListener('click', togglePlay);
  $('step').addEventListener('click', () => step(local.stepsPerFrame));
  $('reset').addEventListener('click', reset);
  $('reseed').addEventListener('click', () => {
    $('seed').value = Math.floor(Math.random() * 1e6);
    reset();
  });
  $('seed').addEventListener('change', reset);
  $('collapse').addEventListener('click', () => {
    $('panel').hidden = true;
    $('expand').hidden = false;
  });
  $('expand').addEventListener('click', () => {
    $('panel').hidden = false;
    $('expand').hidden = true;
  });
  window.addEventListener('keydown', (e) => {
    if (e.target instanceof HTMLInputElement && e.target.type !== 'range') return;
    if (e.key === ' ') { e.preventDefault(); togglePlay(); }
    else if (e.key === 'r') reset();
    else if (e.key === 's') step(local.stepsPerFrame);
  });
  window.addEventListener('resize', () => renderer.resize());

  renderer.resize();
  buildControls();
  reset();
  // ?warmup=N runs N steps before the first frame, to open on a mature river.
  step(Number(new URLSearchParams(location.search).get('warmup')) || 0);
  requestAnimationFrame(frame);
}
