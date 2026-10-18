// The river's state as the renderer reads it, packed by the worker that
// steps the river (`pack`) and read on the page through `RiverView`, which
// answers the same calls as `River`. Every buffer the renderer reads goes
// into one ArrayBuffer, which the worker hands over without copying, and
// each `*_ptr` is a byte offset into it, so the renderer builds its views
// on `RiverView.memory.buffer` as it would on the WASM memory.

// Values the river returns, read at each pack.
const SCALARS = [
  'time', 'steps', 'view_x', 'cutoffs', 'supply', 'has_boundary',
  'field_cols', 'field_rows', 'field_cell',
  'line_len', 'oxbow_line_len', 'oxbow_count', 'scroll_line_len', 'scroll_count', 'holding_slots',
  'scroll_version', 'track_version', 'ford_version', 'mill_version', 'building_version',
  'wall_version', 'land_version',
];

// Copies the river returns, passed on as they are.
const VECTORS = [
  'jams', 'tributaries', 'tributary_courses', 'mills', 'mill_leats', 'track_points',
  'track_lengths', 'fords', 'boundary', 'buildings', 'wall_combes',
];

// Buffers in the WASM memory, at `<name>_ptr`, each with its element type
// and its length from the scalars.
const BUFFERS = [
  ['line', Float32Array, (s) => s.line_len],
  ['widths', Float32Array, (s) => s.line_len / 2],
  ['oxbow_line', Float32Array, (s) => s.oxbow_line_len],
  ['oxbow_offsets', Uint32Array, (s) => s.oxbow_count + 1],
  ['oxbow_fills', Float32Array, (s) => s.oxbow_count],
  ['oxbow_scar_ages', Float32Array, (s) => s.oxbow_count],
  ['oxbow_lengths', Float32Array, (s) => s.oxbow_count],
  ['oxbow_offsets_along', Float32Array, (s) => s.oxbow_count],
  ['oxbow_widths', Float32Array, (s) => s.oxbow_line_len / 2],
  ['oxbow_heights', Float32Array, (s) => s.oxbow_line_len / 2],
  ['oxbow_silts', Float32Array, (s) => s.oxbow_line_len / 2],
  ['scroll_line', Float32Array, (s) => s.scroll_line_len],
  ['scroll_offsets', Uint32Array, (s) => s.scroll_count + 1],
  ['scroll_ages', Float32Array, (s) => s.scroll_count],
  ['holdings', Float32Array, (s) => 6 * s.holding_slots],
  ['deposit', Float32Array, (s) => s.field_cols * s.field_rows],
  ['plugs', Float32Array, (s) => s.field_cols * s.field_rows],
  ['forest', Float32Array, (s) => s.field_cols * s.field_rows],
  ['coarse', Float32Array, (s) => s.field_cols * s.field_rows],
  ['land_owner', Uint32Array, (s) => s.field_cols * s.field_rows],
  ['land_risk', Float32Array, (s) => s.field_cols * s.field_rows],
  ['land_wet', Float32Array, (s) => s.field_cols * s.field_rows],
];

// Spacing of the samples of the valley walls' feet, in widths, as the
// renderer reads them.
const WALL_STEP = 0.5;

// Pack the river's state, with the WASM `memory`, for a window `[x0, vw]`.
// `full` adds what changes only at a reset: the soil noise. The walls'
// feet are sampled along the window when `wall_version` differs from
// `walls`, the version last packed. Returns the message and the buffers to
// transfer with it.
export function pack(river, memory, [x0, vw], full, walls) {
  river.sync();
  river.sync_scrolls();
  river.sync_plugs();
  river.sync_forest();
  river.sync_holdings();
  const scalars = {};
  for (const k of SCALARS) scalars[k] = river[k]();
  const vectors = {};
  for (const k of VECTORS) vectors[k] = river[k]();
  const params = {};
  for (const name of river.constructor.param_names()) params[name] = river.get_param(name);
  // The copies above can grow the memory, so its buffer is read after them.
  const at = {};
  let size = 0;
  for (const [name, , len] of BUFFERS) {
    at[name] = [size, len(scalars)];
    size += 4 * at[name][1];
  }
  const buf = new ArrayBuffer(size);
  for (const [name, Type] of BUFFERS) {
    const [offset, n] = at[name];
    new Type(buf, offset, n).set(new Type(memory.buffer, river[`${name}_ptr`](), n));
  }
  const snap = { scalars, vectors, params, at, buf };
  const transfer = [buf];
  if (full) {
    const cells = scalars.field_cols * scalars.field_rows;
    snap.soil = new Float32Array(memory.buffer, river.soil_ptr(), cells).slice();
    transfer.push(snap.soil.buffer);
  }
  if (full || scalars.wall_version !== walls) {
    // The same points the renderer reads, from `x0` in steps to the
    // window's right edge.
    const step = WALL_STEP * params.width, n = Math.ceil(vw / step) + 1;
    const xs = new Float64Array(n), feet = [new Float64Array(n), new Float64Array(n)];
    for (let k = 0; k < n; k++) {
      xs[k] = x0 + Math.min(k * step, vw);
      for (const side of [0, 1]) feet[side][k] = river.wall_foot(side, xs[k]);
    }
    snap.walls = { x0, step, xs, feet };
  }
  return { snap, transfer };
}

// The river as the last snapshot left it, read with the same calls as
// `River`. The syncs are already done.
export class RiverView {
  constructor() {
    this.memory = { buffer: new ArrayBuffer(0) };
    this.snap = null;
    this.soilGrid = null;
    this.walls = null;
    for (const k of SCALARS) this[k] = () => this.snap.scalars[k];
    for (const k of VECTORS) this[k] = () => this.snap.vectors[k];
    for (const [name] of BUFFERS) this[`${name}_ptr`] = () => this.snap.at[name][0];
    for (const k of ['sync', 'sync_line', 'sync_scrolls', 'sync_plugs', 'sync_forest', 'sync_holdings']) {
      this[k] = () => {};
    }
  }

  update(snap) {
    this.snap = snap;
    this.memory.buffer = snap.buf;
    if (snap.soil) this.soilGrid = snap.soil;
    if (snap.walls) this.walls = snap.walls;
  }

  get_param(name) {
    const v = this.snap.params[name];
    if (v === undefined) throw new Error(`unknown parameter ${name}`);
    return v;
  }

  // As `Erodibility::noise_at`: the cell holding the point, clamped to the
  // grid.
  soil(x, y) {
    const { field_cols: cols, field_rows: rows, field_cell: cell } = this.snap.scalars;
    const c = Math.min(Math.max(0, Math.floor(x / cell)), cols - 1);
    const r = Math.min(Math.max(0, Math.floor(y / cell)), rows - 1);
    return this.soilGrid[r * cols + c];
  }

  // Exact at the sampled points, and linear between them.
  wall_foot(side, x) {
    const { x0, step, xs, feet } = this.walls;
    const j = Math.min(Math.max(0, Math.floor((x - x0) / step)), xs.length - 2);
    for (const k of [j, j + 1]) if (xs[k] === x) return feet[side][k];
    const t = (x - xs[j]) / (xs[j + 1] - xs[j] || 1);
    return feet[side][j] + (feet[side][j + 1] - feet[side][j]) * t;
  }
}
