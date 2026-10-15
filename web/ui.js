// Shared page logic: WASM start-up, the control panel, and the animation
// loop. Each page supplies a renderer and its own display controls.

import init, { River } from './pkg/meander_wasm.js';

// The visible window's height is fixed in world units; its width follows
// the view's aspect ratio at each reset.
const WORLD_H = 480;

// Sliders with `local` set are page settings; the rest are simulation
// parameters, read from and written to the WASM side by name. Toggles
// without `local` are simulation flags, set as 0 or 1. Sliders marked
// `derived` are computed by the simulation while the physics flag is on.
const SIM_CONTROLS = [
  { group: 'Time' },
  { key: 'speed', label: 'Speed (sim time per second)', min: 0.5, max: 60, step: 0.5, local: 6 },
  { key: 'warmup', label: 'Warm-up on reset (sim time)', min: 0, max: 3000, step: 100, local: 1000 },
  { group: 'Dynamics' },
  { key: 'erodibility', label: 'Erodibility E', min: 0, max: 3, step: 0.05 },
  { key: 'omega', label: 'Local weight Ω', min: -3, max: 1, step: 0.05, derived: true },
  { key: 'gamma', label: 'Upstream weight Γ', min: 0, max: 5, step: 0.05, derived: true },
  { key: 'alpha', label: 'Upstream decay α (per width)', min: 0.05, max: 2, step: 0.01, derived: true },
  { key: 'peak_radius', label: 'Fastest bend radius (widths, 0 off)', min: 0, max: 6, step: 0.1 },
  { key: 'dt', label: 'Timestep dt', min: 0.01, max: 0.3, step: 0.01 },
  { key: 'smoothing', label: 'Smoothing k', min: 0, max: 0.25, step: 0.01 },
  { key: 'max_step', label: 'Step limit (× spacing)', min: 0.05, max: 0.3, step: 0.01 },
  { group: 'Channel' },
  { key: 'width', label: 'Channel width w', min: 2, max: 20, step: 0.5 },
  { key: 'valley_width', label: 'Valley width (widths, 0 off)', min: 0, max: 60, step: 1 },
  { key: 'edge_margin', label: 'Edge margin (widths)', min: 0, max: 30, step: 1 },
  { group: 'Cutoffs' },
  { key: 'cutoff_dist', label: 'Cutoff distance (widths)', min: 0.3, max: 3, step: 0.05 },
  { key: 'cutoff_min_sep', label: 'Min separation K (points)', min: 6, max: 100, step: 1 },
  { key: 'oxbow_lifetime', label: 'Oxbow lifetime', min: 0, max: 2000, step: 10 },
  { group: 'Floods' },
  { key: 'flood_rate', label: 'Floods per unit time (0 off)', min: 0, max: 0.2, step: 0.005 },
  { key: 'breach_dist', label: 'Widest neck breached (widths)', min: 1, max: 5, step: 0.1 },
  { key: 'chute_ratio', label: 'Chute arc / chord (0 off)', min: 0, max: 10, step: 0.5 },
  { group: 'Physics (Ikeda–Parker–Sawai)' },
  { key: 'physics', label: 'Derive Ω, Γ, α from hydraulics', toggle: true },
  { key: 'friction', label: 'Friction coefficient Cf', min: 0.002, max: 0.03, step: 0.001 },
  { key: 'aspect', label: 'Width / depth', min: 5, max: 100, step: 1 },
  { key: 'froude', label: 'Froude number', min: 0.05, max: 0.9, step: 0.05 },
  { key: 'scour', label: 'Scour factor A', min: 1, max: 6, step: 0.1 },
  { group: 'Noise' },
  { key: 'jitter', label: 'Jitter', min: 0, max: 0.3, step: 0.01 },
  { key: 'jitter_scale', label: 'Jitter length (widths)', min: 0.5, max: 10, step: 0.5 },
  { key: 'noise', label: 'Initial noise (widths, on reset)', min: 0, max: 2, step: 0.05, local: 0.5 },
  { group: 'Erodibility' },
  { key: 'heterogeneity', label: 'Heterogeneity', min: 0, max: 1, step: 0.05 },
  { key: 'clay_factor', label: 'Oxbow clay plugs (× E)', min: 0.05, max: 1, step: 0.05 },
];

// `renderer` provides draw(river, memory, state), reset(river, bounds), and
// capture(river, memory, state), which is called during warm-up in place of
// drawing. `display` lists extra page-specific controls.
export async function startApp({ view, renderer, display }) {
  const wasm = await init();
  const controls = [...SIM_CONTROLS, { group: 'Display' }, ...display];
  const local = {};
  const state = { bounds: [1, 1], local };
  const river = new River();
  let running = true;
  let frameMs = 0;
  // Simulated time owed to the loop but not yet stepped.
  let owed = 0;
  // Warm-up time still to run, spread over frames so the page stays live.
  let warming = 0;
  let lastFrame;
  const $ = (id) => document.getElementById(id);

  function reset() {
    const r = view.getBoundingClientRect();
    state.bounds = [WORLD_H * r.width / r.height, WORLD_H];
    const seed = Number($('seed').value) >>> 0;
    for (const s of controls) {
      if (!s.key || s.local !== undefined) continue;
      river.set_param(s.key, s.toggle ? Number(s.box.checked) : Number(s.input.value));
    }
    river.reset(...state.bounds, seed, local.noise, 0);
    refresh();
    owed = 0;
    warming = local.warmup;
    renderer.reset(river, state.bounds);
  }

  // Show the simulation's current parameter values, which differ from the
  // sliders where the physics flag derives them.
  function refresh() {
    const physics = river.get_param('physics') !== 0;
    for (const s of controls) {
      if (!s.input || s.local !== undefined) continue;
      const v = river.get_param(s.key);
      s.input.value = v;
      s.out.textContent = String(+v.toFixed(3));
      s.input.disabled = Boolean(s.derived && physics);
      s.input.parentElement.classList.toggle('derived', s.input.disabled);
    }
  }

  function buildControls() {
    const root = $('sliders');
    const defaults = new River();
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
        const box = s.box = row.querySelector('input');
        if (s.local !== undefined) {
          box.checked = s.local;
          local[s.key] = s.local;
          box.addEventListener('change', () => { local[s.key] = box.checked; });
        } else {
          box.checked = defaults.get_param(s.key) !== 0;
          box.addEventListener('change', () => {
            river.set_param(s.key, Number(box.checked));
            refresh();
          });
        }
        root.append(row);
        continue;
      }
      const row = document.createElement('div');
      row.className = 'slider';
      row.innerHTML = `<label for="p-${s.key}">${s.label}</label><output></output>
        <input id="p-${s.key}" type="range" min="${s.min}" max="${s.max}" step="${s.step}">`;
      s.input = row.querySelector('input');
      const out = s.out = row.querySelector('output');
      s.input.value = s.local ?? defaults.get_param(s.key);
      const apply = () => {
        const v = Number(s.input.value);
        out.textContent = String(+v.toFixed(3));
        if (s.local !== undefined) local[s.key] = v;
        else if (river.set_param(s.key, v) && river.get_param('physics') !== 0) refresh();
      };
      s.input.addEventListener('input', apply);
      apply();
      root.append(row);
    }
    defaults.free();
  }

  function frame(now) {
    // Real seconds since the last frame, capped so a background tab does
    // not return to a huge backlog.
    const elapsed = Math.min(0.1, (now - (lastFrame ?? now)) / 1000);
    lastFrame = now;
    if (warming > 0) {
      const t0 = performance.now();
      while (warming > 0 && performance.now() - t0 < 12) {
        const chunk = Math.min(warming, 5);
        river.advance(chunk);
        warming -= chunk;
        renderer.capture(river, wasm.memory, state);
      }
      $('overlay').hidden = warming <= 0;
      $('overlay').textContent = `Developing river… ${Math.round(100 * (1 - warming / local.warmup))}%`;
      requestAnimationFrame(frame);
      return;
    }
    if (running) {
      const t0 = performance.now();
      const dt = river.get_param('dt');
      owed += local.speed * elapsed;
      const n = Math.floor(owed / dt);
      if (n > 0) owed -= dt * river.advance(n * dt);
      frameMs = 0.9 * frameMs + 0.1 * (performance.now() - t0);
    }
    renderer.draw(river, wasm.memory, state);
    $('stats').textContent =
      `t ${river.time().toFixed(1)}   sinuosity ${river.sinuosity().toFixed(2)}\n` +
      `points ${river.point_count()}   cutoffs ${river.cutoffs()}   oxbows ${river.oxbow_count()}\n` +
      `floods ${river.floods()}, which made ${river.flood_cutoffs()} cutoffs\n` +
      `${frameMs.toFixed(1)} ms/frame (sim)`;
    requestAnimationFrame(frame);
  }

  function togglePlay() {
    running = !running;
    $('play').textContent = running ? 'Pause' : 'Play';
  }

  $('play').addEventListener('click', togglePlay);
  $('step').addEventListener('click', () => river.advance(local.speed));
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
    else if (e.key === 's') river.advance(local.speed);
  });

  buildControls();
  // ?warmup=T overrides the warm-up slider for the first reset.
  const warmup = new URLSearchParams(location.search).get('warmup');
  if (warmup !== null) local.warmup = Number(warmup);
  reset();
  requestAnimationFrame(frame);
}
