// Shared page logic: WASM start-up, the control panel, and the animation
// loop. Each page supplies a renderer and its own display settings.

import init, { River } from './pkg/meander_wasm.js';

// The visible window's height is fixed in world units; its width follows
// the view's aspect ratio at each reset.
const WORLD_H = 480;

// The panel's sliders, all page settings. The simulation runs on its own
// defaults, which come from measured river hydraulics.
const CONTROLS = [
  { key: 'speed', label: 'Speed (sim time per second)', min: 0.5, max: 60, step: 0.5, value: 2 },
];

// Page settings the panel does not show: the warm-up on reset, in
// simulated time, and the initial noise, in channel widths.
const SETTINGS = { warmup: 500, noise: 0.5 };

// Years per unit of simulated time. At this scale the median bank migrates
// about 1% of a channel width per year and the fastest 10% of the channel
// 5% or more, within the range measured on real meandering rivers.
const YEARS_PER_UNIT = 2.5;

// `renderer` provides draw(river, memory, state), reset(river, bounds, seed),
// capture(river, memory, state), which is called during warm-up in place of
// drawing, and idle(river, memory), which is called on frames that draw
// nothing, to carry on with drawing work spread over frames. `display` holds the renderer's own settings.
export async function startApp({ view, renderer, display }) {
  const wasm = await init();
  const local = { ...SETTINGS, ...display };
  const state = { bounds: [1, 1], local };
  const river = new River();
  let running = true;
  let frameMs = 0;
  // Simulated time owed to the loop but not yet stepped.
  let owed = 0;
  // Warm-up time still to run, spread over frames so the page stays live.
  let warming = 0;
  // Simulated time at the end of the warm-up, shown as year 0.
  let epoch = 0;
  // Whether the view needs redrawing although the river has not moved.
  let dirty = true;
  let lastFrame;
  const $ = (id) => document.getElementById(id);

  function reset() {
    const r = view.getBoundingClientRect();
    state.bounds = [WORLD_H * r.width / r.height, WORLD_H];
    const seed = Number($('seed').value) >>> 0;
    river.reset(...state.bounds, seed, local.noise, 0);
    owed = 0;
    warming = local.warmup;
    epoch = river.time();
    dirty = true;
    renderer.reset(river, state.bounds, seed);
  }

  function buildControls() {
    const root = $('sliders');
    for (const s of CONTROLS) {
      const row = document.createElement('div');
      row.className = 'slider';
      row.innerHTML = `<label for="p-${s.key}">${s.label}</label><output></output>
        <input id="p-${s.key}" type="range" min="${s.min}" max="${s.max}" step="${s.step}">`;
      const input = row.querySelector('input');
      const out = row.querySelector('output');
      input.value = s.value;
      const apply = () => {
        local[s.key] = Number(input.value);
        out.textContent = String(local[s.key]);
        dirty = true;
      };
      input.addEventListener('input', apply);
      apply();
      root.append(row);
    }
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
      if (warming <= 0) epoch = river.time();
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
      if (n > 0) {
        owed -= dt * river.advance(n * dt);
        dirty = true;
      }
      frameMs = 0.9 * frameMs + 0.1 * (performance.now() - t0);
    }
    // Draw only when something has changed, so a paused or slow river costs
    // nothing between steps beyond drawing work spread over frames.
    if (!dirty) {
      renderer.idle(river, wasm.memory);
      requestAnimationFrame(frame);
      return;
    }
    dirty = false;
    renderer.draw(river, wasm.memory, state);
    const year = `Year ${Math.round((river.time() - epoch) * YEARS_PER_UNIT).toLocaleString('en-GB')}`;
    if ($('year').textContent !== year) $('year').textContent = year;
    $('stats').textContent =
      `t ${river.time().toFixed(1)}   sinuosity ${river.sinuosity().toFixed(2)}\n` +
      `points ${river.point_count()}   cutoffs ${river.cutoffs()}   oxbows and scars ${river.oxbow_count()}\n` +
      `floods ${river.floods()}, which made ${river.flood_cutoffs()} cutoffs\n` +
      `${frameMs.toFixed(1)} ms/frame (sim)`;
    requestAnimationFrame(frame);
  }

  function togglePlay() {
    running = !running;
    $('play').textContent = running ? 'Pause' : 'Play';
  }

  $('play').addEventListener('click', togglePlay);
  // Advance one second's worth of simulated time.
  const step = () => {
    river.advance(local.speed);
    dirty = true;
  };
  $('step').addEventListener('click', step);
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
    else if (e.key === 's') step();
  });

  buildControls();
  // ?warmup=T overrides the warm-up for the first reset.
  const warmup = new URLSearchParams(location.search).get('warmup');
  if (warmup !== null) local.warmup = Number(warmup);
  reset();
  requestAnimationFrame(frame);
}
