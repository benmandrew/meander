// Steps the river off the page's thread, so stepping runs while the page
// draws. Each request steps it and replies with a snapshot (see
// `snapshot.js`) for the page to draw from.
//
// A request is { id, reset, steps, budget, capture }. `reset`, if given,
// is { bounds, seed, noise } and restarts the river first. The river then
// takes up to `steps` steps, stopping between steps once `budget` ms have
// passed. With `capture`, it steps in runs of `capture` steps and records
// the centreline after each, with its time. The reply is
// { id, stepped, captures, snap }, or { id, error }.

import init, { River } from './pkg/meander_wasm.js';
import { pack } from './snapshot.js';

let river, memory;
// The window of the last reset, and the walls' version last packed.
let view = [0, 0];
let walls = null;

const ready = init().then((wasm) => {
  memory = wasm.memory;
  river = new River();
});

function centreline() {
  river.sync_line();
  return new Float32Array(memory.buffer, river.line_ptr(), river.line_len()).slice();
}

function serve({ id, reset, steps = 0, budget = Infinity, capture = 0 }) {
  if (reset) {
    river.reset(...reset.bounds, reset.seed, reset.noise, 0);
    view = [river.view_x(), reset.bounds[0]];
  }
  const t0 = performance.now();
  const captures = [];
  let stepped = 0;
  while (stepped < steps && performance.now() - t0 < budget) {
    const n = capture ? Math.min(capture, steps - stepped) : 1;
    stepped += river.advance(n * river.get_param('dt'));
    if (capture) captures.push({ t: river.time(), line: centreline() });
  }
  const { snap, transfer } = pack(river, memory, view, Boolean(reset), reset ? null : walls);
  walls = snap.scalars.wall_version;
  for (const c of captures) transfer.push(c.line.buffer);
  postMessage({ id, stepped, captures, snap }, transfer);
}

self.onmessage = async ({ data }) => {
  try {
    await ready;
    serve(data);
  } catch (e) {
    postMessage({ id: data.id, error: String(e?.stack ?? e) });
  }
};
