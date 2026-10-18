// Shared page logic: the worker that steps the river, the control panel,
// and the animation loop. Each page supplies a renderer and its own display
// settings.

import { RiverView } from './snapshot.js';

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
// than `MAX_LAG` of simulated time, 25 years, or `LAG_SECONDS` of the set
// speed if longer, so a slow machine runs below the set speed instead of
// drawing the floodplain centuries late. At high speeds a lag in simulated
// time alone is a few frames, and redraws spread over frames held the
// river twice a second. Before the hold, the river slows to the speed at
// which the redraws under way finish within the limit if they take
// 1 / `PACE_LAG` times as long in real time as each layer's recent
// redraws, which take about as long at any speed. With no margin, the
// river ran 1–13% faster than at 0.7 on a phone at 4× throttling, and
// still never stood still for 3 frames. It slows at once and
// speeds up again over `PACE_SECONDS`, so a machine too slow for the set
// speed runs the river at a steady lower speed instead of in starts and
// stops.
const MAX_LAG = 10;
const LAG_SECONDS = 0.5;
const PACE_LAG = 1;
const PACE_SECONDS = 1;

// The worker steps the river (see `worker.js`) while the page draws, one
// request a frame. A request may take `STEP_SHARE` of the last frame's
// interval, but at least `STEP_BUDGET` ms, so the worker is kept busy and
// its reply still comes before the next frame. Simulated time the budget
// cannot cover is dropped, so a machine too slow for the set speed runs the
// river slower instead of owing more steps to each request. So is time
// owed beyond `MAX_OWED` s at the river's speed, past the request under way
// and the next step, while a reply is late, so the river does not lurch on
// when it comes. The warm-up and a replay step in requests of
// `WARM_BUDGET` ms.
const STEP_SHARE = 0.8;
const STEP_BUDGET = 8;
const WARM_BUDGET = 12;
const MAX_OWED = 0.04;

// The panel shows the JS time of the last `FRAME_WINDOW` frames, about 10 s,
// refreshed every `STATS_EVERY` ms.
const FRAME_WINDOW = 600;
const STATS_EVERY = 250;

// `renderer` provides draw(river, memory, state), reset(river, bounds, seed),
// capture(river, line, t, state), which is called with the centreline at
// time t during warm-up in place of drawing, idle(river, memory, state),
// which is called on frames that draw nothing, to carry on with drawing
// work spread over frames, and returns whether it did any, and
// settle(river, memory, state), which finishes that work at once when the
// river stops, before a last draw, and behind(river), the simulated time
// that work has waited. `river` is a `RiverView` of the worker's last reply,
// and `memory` its buffers.
// `display` holds the renderer's own settings.
export async function startApp({ view, renderer, display }) {
  const local = { ...SETTINGS, ...display };
  const state = { bounds: [1, 1], local };
  const river = new RiverView();
  // The overlay reads "Loading…" from the page itself until the warm-up
  // takes it over.
  const failed = (e) => {
    document.getElementById('overlay').hidden = false;
    document.getElementById('overlay').textContent = 'Could not load the simulation.';
    console.error(e);
  };
  const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  worker.onerror = (e) => failed(e.message);
  // Handlers of the requests under way, by id. A reset drops them, so the
  // replies to requests made before it are ignored.
  const pending = new Map();
  let nextId = 0;
  function request(msg, handle) {
    const id = ++nextId;
    pending.set(id, handle);
    worker.postMessage({ id, ...msg });
  }
  worker.onmessage = ({ data }) => {
    const handle = pending.get(data.id);
    if (!handle) return;
    pending.delete(data.id);
    if (data.error) return failed(data.error);
    river.update(data.snap);
    handle(data);
    wake();
  };
  // Whether the river has come back from its last reset, and its time step.
  let ready = false;
  let dt = 0;
  // Simulated time the stepping request under way asked for, if any.
  let asked = null;
  let running = true;
  // JS time of recent frames, in ms, as a ring.
  const frameTimes = new Float64Array(FRAME_WINDOW);
  let frames = 0;
  let statsAt = -Infinity;
  // Simulated time owed to the loop but not yet stepped, and the speed the
  // river runs at, at most the set speed `paceFor` it was found for, or null
  // to start at the speed allowed.
  let owed = 0;
  let pace = null;
  let paceFor = null;
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
  // The year and tick last shown, so a draw formats them only on a change.
  let shownYear = null;
  let shownTick = null;
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
  const epoch = () => Math.round(local.warmup / dt);

  // Restart the river, and run the warm-up, or replay to step `target` and
  // pause there.
  function reset(target) {
    const r = view.getBoundingClientRect();
    state.bounds = [local.view ?? WORLD_H * r.width / r.height, WORLD_H];
    const seed = Number($('seed').value) >>> 0;
    pending.clear();
    [ready, asked, owed, warming, pace] = [false, null, 0, 0, null];
    replaying = target !== undefined;
    dirty = true;
    request({ reset: { bounds: state.bounds, seed, noise: local.noise } }, () => {
      ready = true;
      dt = river.get_param('dt');
      renderer.reset(river, state.bounds, seed);
      warming = warmTotal = replaying ? target : epoch();
      $('overlay').hidden = warming <= 0;
      if (replaying && warming <= 0) pause();
      warm();
    });
    wake();
  }

  // Run the rest of the warm-up or replay, recording the centreline every
  // `CAPTURE_STEPS` steps from the reset.
  function warm() {
    if (warming <= 0) return;
    request({ steps: warming, budget: WARM_BUDGET, capture: CAPTURE_STEPS }, ({ stepped, captures }) => {
      for (const { t, line } of captures) renderer.capture(river, line, t, state);
      warming -= stepped;
      if (warming <= 0 && replaying) pause();
      $('overlay').hidden = warming <= 0;
      const done = Math.round(100 * (1 - warming / warmTotal));
      $('overlay').textContent = replaying ? `Replaying to tick ${warmTotal}… ${done}%` : `Developing river… ${done}%`;
      warm();
    });
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
    // The worker's replies drive the warm-up; the loop runs on so the page
    // is not marked idle.
    if (!ready || warming > 0) return true;
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
    // A held frame is not owed later, so the river does not lurch on.
    const lag = Math.max(MAX_LAG, LAG_SECONDS * local.speed), behind = renderer.behind(river);
    state.lagging = behind / lag;
    const allowed = Math.min(local.speed, renderer.allowance(river, lag, 1 / PACE_LAG));
    // Only a pace held below the set speed rises slowly, so a new speed
    // from the slider takes effect at once.
    const held = pace !== null && pace < paceFor;
    pace = held ? Math.min(allowed, pace + (allowed - pace) * Math.min(1, elapsed / PACE_SECONDS)) : allowed;
    paceFor = local.speed;
    if (running && behind <= lag) {
      owed = Math.min(owed + pace * elapsed, (asked ?? 0) + dt + pace * MAX_OWED);
      const steps = Math.floor(owed / dt);
      if (asked === null && steps > 0) {
        asked = steps * dt;
        const budget = Math.max(STEP_BUDGET, STEP_SHARE * 1000 * elapsed);
        request({ steps, budget }, ({ stepped }) => {
          asked = null;
          owed -= stepped * dt;
          if (stepped < steps) owed = Math.min(owed, dt);
          if (stepped > 0) dirty = true;
        });
      }
    }
    // One long frame while the river stands still does not show as stutter,
    // and the last changes then land together instead of over a second.
    // The draw that follows brings the rest of the view up to date.
    if (settle) {
      settle = false;
      renderer.settle(river, river.memory, state);
      dirty = true;
    }
    // Draw only when something has changed, so a paused or slow river costs
    // nothing between steps beyond the channel and drawing work spread over
    // frames.
    const drew = dirty;
    if (dirty) {
      dirty = false;
      renderer.draw(river, river.memory, state);
      const years = Math.round((river.steps() - epoch()) * dt * YEARS_PER_UNIT);
      if (years !== shownYear) {
        shownYear = years;
        $('year').textContent = `Year ${years.toLocaleString('en-GB')}`;
      }
      // The step count from the reset, which with the seed and the view's
      // width fixes the river's state. Not while it is being edited.
      const tick = river.steps();
      if (tick !== shownTick && document.activeElement !== $('tick')) {
        shownTick = tick;
        $('tick').value = String(tick);
      }
    }
    // A running channel trails the river by a step, so it moves on every
    // frame; a stopped one shows the river as it stands. The river's time
    // and the time owed sum to an even clock, since a reply moves time from
    // one to the other; while the worker steps, the channel waits at the
    // newest snapshot. Taking the time asked off as well sent the channel
    // back to the start of its step at each request: 201 times in 602
    // frames, so it moved only as each step came in, and in Safari, where
    // the reply often comes a frame later, it went back and forth.
    const moved = renderer.tween(river, running ? river.time() + owed - dt : Infinity);
    return drew || renderer.idle(river, river.memory, state) || moved || running;
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
    if (running) owed = dt;
    settle = !running;
    $('play').textContent = running ? 'Pause' : 'Play';
    wake();
  }

  $('play').addEventListener('click', togglePlay);
  // Advance one second's worth of simulated time.
  const step = () => {
    if (!ready || warming > 0) return;
    request({ steps: Math.round(local.speed / dt) }, () => {
      dirty = true;
      settle = !running;
    });
  };
  $('step').addEventListener('click', step);
  $('reset').addEventListener('click', () => reset());
  const reseed = () => { $('seed').value = Math.floor(Math.random() * 1e6); };
  $('reseed').addEventListener('click', () => {
    reseed();
    reset();
  });
  $('seed').addEventListener('change', () => reset());
  $('tick').addEventListener('change', () => {
    const target = Math.round(Number($('tick').value));
    if (Number.isFinite(target) && target >= 0) reset(target);
    // The next draw writes the tick back, over an edit that was not a tick.
    shownTick = null;
    dirty = true;
    wake();
  });
  // Put a link that replays to the current tick in the address bar and on
  // the clipboard. The view's width is written in full, since a river in a
  // window of a slightly different width soon diverges.
  $('link').addEventListener('click', () => {
    if (!ready) return;
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
  // later resets keep. Without ?seed the river starts from a random seed.
  const params = new URLSearchParams(location.search);
  if (params.has('warmup')) local.warmup = Number(params.get('warmup'));
  if (params.has('seed')) $('seed').value = Number(params.get('seed')) >>> 0;
  else reseed();
  if (params.has('view')) local.view = Number(params.get('view'));
  const target = Math.round(Number(params.get('tick') ?? NaN));
  reset(Number.isFinite(target) && target >= 0 ? target : undefined);
}
