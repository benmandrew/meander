// Shared page logic: WASM start-up, the control panel, and the animation
// loop. Each page supplies a renderer and its own display settings.

import init, { River } from './pkg/meander_wasm.js';

// The visible window's height is fixed in world units; its width follows
// the view's aspect ratio at each reset, unless a replay link fixes it.
const WORLD_H = 480;

// The warm-up and a replay record the centreline every `CAPTURE_STEPS`
// steps from the reset, 5 units (12.5 years), so the point bars at a tick
// do not depend on frame timing and a replay draws the same picture each
// time.
const CAPTURE_STEPS = 50;

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

// The river holds still while a floodplain redraw has been due for more
// than `MAX_LAG` of simulated time, 25 years, so a slow machine runs below
// the set speed instead of drawing the floodplain centuries late.
const MAX_LAG = 10;

// The panel shows the JS time of the last `FRAME_WINDOW` frames, about 10 s,
// refreshed every `STATS_EVERY` ms.
const FRAME_WINDOW = 600;
const STATS_EVERY = 250;

// `renderer` provides draw(river, memory, state), reset(river, bounds, seed),
// capture(river, memory, state), which is called during warm-up in place of
// drawing, idle(river, memory, state), which is called on frames that draw
// nothing, to carry on with drawing work spread over frames, and returns
// whether it did any, and
// settle(river, memory, state), which finishes that work at once when the
// river stops, and behind(river), the simulated time that work has waited.
// `display` holds the renderer's own settings.
export async function startApp({ view, renderer, display }) {
  // The overlay reads "Loading…" from the page itself until the warm-up
  // takes it over.
  let wasm;
  try {
    wasm = await init();
  } catch (e) {
    document.getElementById('overlay').textContent = 'Could not load the simulation.';
    throw e;
  }
  const local = { ...SETTINGS, ...display };
  const state = { bounds: [1, 1], local };
  const river = new River();
  let running = true;
  // JS time of recent frames, in ms, as a ring.
  const frameTimes = new Float64Array(FRAME_WINDOW);
  let frames = 0;
  let statsAt = -Infinity;
  // Simulated time owed to the loop but not yet stepped.
  let owed = 0;
  // Steps of the warm-up or replay still to run, spread over frames so the
  // page stays live, and its total.
  let warming = 0;
  let warmTotal = 0;
  // Whether the run under way is a replay, which pauses at its tick.
  let replaying = false;
  // Whether the view needs redrawing although the river has not moved.
  let dirty = true;
  // Whether the river has stopped, on a pause or a single step, and the
  // drawing work spread over frames should finish on the next frame.
  let settle = false;
  // Whether a frame is scheduled. The loop sleeps while the river is paused
  // and has nothing left to draw, and `wake` starts it again.
  let scheduled = false;
  let lastFrame;
  const $ = (id) => document.getElementById(id);

  function wake() {
    delete document.documentElement.dataset.idle;
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(frame);
  }

  // Steps from the reset to the end of the warm-up, shown as year 0.
  const epoch = () => Math.round(local.warmup / river.get_param('dt'));

  // Restart the river, and run the warm-up, or replay to step `target` and
  // pause there.
  function reset(target) {
    const r = view.getBoundingClientRect();
    state.bounds = [local.view ?? WORLD_H * r.width / r.height, WORLD_H];
    const seed = Number($('seed').value) >>> 0;
    river.reset(...state.bounds, seed, local.noise, 0);
    owed = 0;
    replaying = target !== undefined;
    warming = warmTotal = replaying ? target : epoch();
    dirty = true;
    $('overlay').hidden = warming <= 0;
    if (replaying && warming <= 0) pause();
    renderer.reset(river, state.bounds, seed);
    wake();
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
        wake();
      };
      input.addEventListener('input', apply);
      apply();
      root.append(row);
    }
  }

  function frame(now) {
    scheduled = false;
    // A frame that throws still schedules the next, so one failed draw
    // does not stop the loop.
    let busy = true;
    try {
      busy = tick(now);
    } finally {
      // A sleeping loop wakes with no time owed for the pause. The page is
      // marked idle while it sleeps, for screenshots to wait on.
      if (busy) wake();
      else {
        lastFrame = undefined;
        document.documentElement.dataset.idle = '';
      }
    }
  }

  // Run one frame. Returns whether the loop must run another.
  function tick(now) {
    // Real seconds since the last frame, capped so a background tab does
    // not return to a huge backlog.
    const elapsed = Math.min(0.1, (now - (lastFrame ?? now)) / 1000);
    lastFrame = now;
    if (warming > 0) {
      const t0 = performance.now();
      const dt = river.get_param('dt');
      while (warming > 0 && performance.now() - t0 < 12) {
        const n = Math.min(warming, CAPTURE_STEPS);
        river.advance(n * dt);
        warming -= n;
        renderer.capture(river, wasm.memory, state);
      }
      if (warming <= 0 && replaying) pause();
      $('overlay').hidden = warming <= 0;
      const done = Math.round(100 * (1 - warming / warmTotal));
      $('overlay').textContent = replaying ? `Replaying to tick ${warmTotal}… ${done}%` : `Developing river… ${done}%`;
      return true;
    }
    const t0 = performance.now();
    const busy = update(elapsed);
    frameTimes[frames++ % FRAME_WINDOW] = performance.now() - t0;
    if (now - statsAt >= STATS_EVERY) {
      statsAt = now;
      showStats();
    }
    return busy;
  }

  // Step the river and draw it, after the warm-up. Returns whether the loop
  // must run another frame.
  function update(elapsed) {
    const dt = river.get_param('dt');
    // A held frame is not owed later, so the river does not lurch on.
    if (running && renderer.behind(river) <= MAX_LAG) {
      owed += local.speed * elapsed;
      const n = Math.floor(owed / dt);
      if (n > 0) {
        owed -= dt * river.advance(n * dt);
        dirty = true;
      }
    }
    // One long frame while the river stands still does not show as stutter,
    // and the last changes then land together instead of over a second.
    if (settle) {
      settle = false;
      renderer.settle(river, wasm.memory, state);
    }
    // Draw only when something has changed, so a paused or slow river costs
    // nothing between steps beyond the channel and drawing work spread over
    // frames.
    const drew = dirty;
    if (dirty) {
      dirty = false;
      renderer.draw(river, wasm.memory, state);
      const years = (river.steps() - epoch()) * dt * YEARS_PER_UNIT;
      const year = `Year ${Math.round(years).toLocaleString('en-GB')}`;
      if ($('year').textContent !== year) $('year').textContent = year;
      // The step count from the reset, which with the seed and the view's
      // width fixes the river's state. Not while it is being edited.
      const tick = String(river.steps());
      if ($('tick').value !== tick && document.activeElement !== $('tick')) $('tick').value = tick;
    }
    // A running channel trails the river by a step, so it moves on every
    // frame; a stopped one shows the river as it stands.
    const moved = renderer.tween(river, running ? river.time() + owed - dt : Infinity);
    return drew || renderer.idle(river, wasm.memory, state) || moved || running;
  }

  // The mean, 95th and 99th percentile of the recent frame times.
  function showStats() {
    const times = frameTimes.slice(0, Math.min(frames, FRAME_WINDOW)).sort();
    if (!times.length) return;
    const at = (p) => times[Math.min(times.length - 1, Math.floor(p * times.length))].toFixed(1);
    const mean = times.reduce((a, b) => a + b, 0) / times.length;
    const text = `ms/frame   mean ${mean.toFixed(1)}   p95 ${at(0.95)}   p99 ${at(0.99)}`;
    if ($('stats').textContent !== text) $('stats').textContent = text;
  }

  function pause() {
    running = false;
    settle = true;
    $('play').textContent = 'Play';
  }

  function togglePlay() {
    running = !running;
    // The stopped channel shows the river as it stands, so a full step
    // owed starts the trailing channel from there.
    if (running) owed = river.get_param('dt');
    settle = !running;
    $('play').textContent = running ? 'Pause' : 'Play';
    wake();
  }

  $('play').addEventListener('click', togglePlay);
  // Advance one second's worth of simulated time.
  const step = () => {
    river.advance(local.speed);
    dirty = true;
    settle = !running;
    wake();
  };
  $('step').addEventListener('click', step);
  $('reset').addEventListener('click', () => reset());
  $('reseed').addEventListener('click', () => {
    $('seed').value = Math.floor(Math.random() * 1e6);
    reset();
  });
  $('seed').addEventListener('change', () => reset());
  $('tick').addEventListener('change', () => {
    const target = Math.round(Number($('tick').value));
    if (Number.isFinite(target) && target >= 0) reset(target);
  });
  // Put a link that replays to the current tick in the address bar and on
  // the clipboard. The view's width is written in full, since a river in a
  // window of a slightly different width soon diverges.
  $('link').addEventListener('click', () => {
    const url = new URL(location.href);
    url.searchParams.set('seed', String(Number($('seed').value) >>> 0));
    url.searchParams.set('tick', String(river.steps()));
    url.searchParams.set('view', String(state.bounds[0]));
    history.replaceState(null, '', url);
    navigator.clipboard?.writeText(url.href).then(() => {
      $('link').textContent = 'Copied';
      setTimeout(() => { $('link').textContent = 'Link'; }, 1500);
    }, () => {});
  });
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
  // ?warmup=T overrides the warm-up. ?seed=S&tick=N&view=W, from the Link
  // button, replays seed S to tick N in a view W world units wide, which
  // later resets keep.
  const params = new URLSearchParams(location.search);
  if (params.has('warmup')) local.warmup = Number(params.get('warmup'));
  if (params.has('seed')) $('seed').value = Number(params.get('seed')) >>> 0;
  if (params.has('view')) local.view = Number(params.get('view'));
  const target = Math.round(Number(params.get('tick') ?? NaN));
  reset(Number.isFinite(target) && target >= 0 ? target : undefined);
}
