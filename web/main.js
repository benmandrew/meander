// SVG view in three stacked layers, each cached by the browser apart from
// the others. The bottom layer holds past centrelines as point-bar sand,
// and changes only when a band of them stops growing or is dropped. The
// ground layer holds the growing band, the soil patches and the floodplain
// features the river leaves, and changes when a centreline is kept or a
// feature changes. The top layer holds the oxbow lakes, their scars and the
// channel, redrawn whenever the river moves.

import { startApp } from './ui.js';

const NS = 'http://www.w3.org/2000/svg';
const WATER = [61, 126, 166];
// Water colour of a lake once cut off: the silt settles out of still
// water, so it is clearer and darker than the river.
const CLEAR = [34, 84, 116];
// Fill over which a new lake turns from the river's colour to `CLEAR`. A
// lake left to fill on its own takes 1200 units of simulated time, so this
// is 12 units, two seconds on screen at the default speed.
const CLEARING = 0.01;
// Water colour of a lake near the end of its life, turbid and weedy.
const MURKY = [80, 120, 116];
const REEDS = 'rgb(104, 126, 84)';
const MARSH = 'rgb(118, 134, 96)';
// Oxbow scars: damp ground across the old channel, a wetter core where the
// lake last stood, and marsh tufts on the core, each with its opacity.
const SCAR_DAMP = [126, 140, 98];
const SCAR_WET = [92, 114, 80];
const SCAR_TUFT = [66, 88, 60];
const SCAR_OPACITY = [0.3, 0.35, 0.6];
// Half-widths of the damp ground and the wet core, as fractions of the old
// channel's width. The core is the lake's width when it filled, 1 less
// `oxbowShrink`, times 0.5.
const SCAR_DAMP_HALF = 0.55;
// Spacing of the marsh tufts along a scar, and the width of each, in
// channel widths.
const TUFT_STEP = 1.2;
const TUFT_SIZE = 0.5;
// Fraction of `scar_lifetime` over which the core and its tufts fade. The
// damp ground fades over the whole of it.
const SCAR_WET_LIFE = 0.5;

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
// Silt of the natural levees that build up beside the channel.
const LEVEE = [216, 210, 166];
// Levee contours, as fractions of the highest ground overbank silt can build.
const LEVEE_LEVELS = [0.3, 0.7];
// Simulated time between levee redraws.
const LEVEE_REFRESH = 10;
// Milliseconds a frame for floodplain redraws, and on a frame that draws
// nothing else. Work beyond it carries on in the frames that follow.
const FLOODPLAIN_BUDGET = 4;
const FLOODPLAIN_IDLE_BUDGET = 10;
// Valley sides. Each wall is a scarp from its foot, where the floodplain
// ends, up to its brow, where the older upland begins. The core sets the
// foot, with the combes cut back into it (see `walls.rs`). The scarp is
// `SCARP` widths wide, varying with its own noise, and at a combe the brow
// cuts back `COMBE_BROW` times as deep and wide as the foot, so the
// contours bend into a V.
// Relief is drawn as on old survey maps: hachures run down the slope over
// the fields, in two staggered rows `HACHURE_GAP` widths apart, heavier
// where the slope is steeper. The light comes from the upper left, as for
// the trees, so the wall at the top faces away from it and its hachures
// are darker. Where `HANGER` noise passes its threshold, favoured on steep
// stretches and in combes, the slope is a hanger wood in place of fields,
// tapering at each end. The upland's drier soil tints the ground beyond
// the foot, under its fields.
const UPLAND = [190, 182, 132];
const UPLAND_OPACITY = 0.3;
const SCARP_SHADE = [96, 100, 66];
const SCARP = [1.2, 4.5];
const COMBE_BROW = 1.6;
const HANGER = { knots: 14, threshold: 0.62 };
const HACHURE_GAP = 0.45;
const HACHURE_WIDTH = 0.11;
// Vegetation on the floodplain, drawn as nested translucent regions that
// darken with age: shrub once it binds a point bar after
// `vegetation_time`, then young and mature forest at these fractions of
// `forest_time` after that. Ground the channel never reached is mature
// forest. Each level fades in over `FOREST_STEPS` nested regions across
// the second half of the age gap before it, which reach `FOREST_OPACITY`
// together. With one region, ground the channel swept within a short time
// would cross the level at once and darken in a single frame.
const FOREST = [
  { at: 0, colour: [150, 170, 104] },
  { at: 0.35, colour: [112, 140, 84] },
  { at: 0.85, colour: [84, 116, 70] },
];
const FOREST_OPACITY = 0.16;
const FOREST_STEPS = 6;
// Trees: crowns on a grid `TREE_GAP` widths apart, jittered by up to half
// a gap. Each crown appears once the growth of the ground under it passes
// its own seeded threshold between the young and mature forest levels,
// then widens to its full radius, up to `TREE_RADIUS` widths, over
// `TREE_GROW` of `forest_time`. Crowns smaller than `TREE_MIN` widths are
// not drawn. A crown is `TREE_LOBES` overlapping circles around a central
// one, at seeded angles. It casts a shadow down and to the right and
// catches light up and to the left.
const TREE_GAP = 0.9;
const TREE_RADIUS = 0.5;
const TREE_LOBES = 4;
const TREE_GROW = 0.4;
const TREE_MIN = 0.06;
const TREE_TONES = [[104, 136, 78], [90, 124, 68], [76, 110, 62]];
const TREE_SHADOW = [48, 66, 40];
const TREE_LIGHT = [150, 176, 108];
// Crowns are drawn in strips `TREE_STRIP` widths wide, one path per strip
// for each kind of mark, so the browser rasterises only the strips a tile
// meets and a strip whose trees have not changed keeps its path data.
const TREE_STRIP = 8;
// Farmland: the core shares the ground between landholdings, each worked
// from a farmstead (see `land.rs`), and each holding is laid out in
// fields. Fields are drawn only where grass has taken the ground, after
// `grazing_time`, which is where holdings take it up, so the channel cuts
// them as it migrates and they follow it onto the bars it leaves. Most fields on
// the young ground of the meander belt flood too often to plough and are
// meadow (see `MEADOW_RISK`). A holding's fields lie
// in strips along its long axis, measured from its farmstead,
// `FIELD_STRIP` widths across, each cut into fields `FIELD_LENGTH` widths
// long whose ends lean by up to `FIELD_LEAN` of the strip's width. The
// strips bend by up to `FIELD_BEND` widths either way over a wavelength of
// `FIELD_WAVE` widths, under half the narrowest strip so edges never cross.
// The layout stays put until the holding reorganises and its shape turns,
// when the core draws it a new seed. Fields smaller than `MIN_FIELD`
// square widths join a neighbour in the same holding.
const FIELD_STRIP = [5, 11];
const FIELD_LENGTH = [4, 14];
const FIELD_LEAN = 0.35;
const FIELD_WAVE = [15, 35];
const FIELD_BEND = 1.5;
const MIN_FIELD = 2;
// Boundaries are traced along the edges of the core's cells, simplified to
// within `FIELD_SIMPLIFY` cells and rounded by two passes of Chaikin's
// corner cutting. Fields are redrawn when a reorganisation finishes, at
// most every `FIELD_REFRESH` of simulated time. A redraw shares the
// floodplain's `FLOODPLAIN_BUDGET` a frame, so its 10 to 25 ms spread over
// several frames.
const FIELD_SIMPLIFY = 0.7;
const FIELD_REFRESH = 5;
// Fields fade in as the sward thickens, over `FIELD_FADE` nested regions
// from `grazing_time`, when a holding takes the ground, to
// `vegetation_time`, when it is as thick as old meadow: 50 to 250 years.
// The regions are redrawn with the forest, every `LEVEE_REFRESH`, so each
// redraw moves the fade by about one region, not the fields' whole edge.
const FIELD_FADE = 8;
// Land use follows flood risk, as on a medieval floodplain: meadow and
// pasture on ground at risk `MEADOW_RISK` or more, which floods too often
// to plough, and arable on the drier ground. `WOODLAND` of the fields stay
// as woods.
const MEADOWS = [[146, 168, 104], [184, 188, 128]];
const ARABLE = [[168, 182, 110], [212, 198, 138], [166, 146, 108]];
const MEADOW_RISK = 0.35;
// A field changes use only once its risk passes `MEADOW_RISK` by
// `USE_MARGIN`, so a field near the threshold keeps its colour while the
// channel moves.
const USE_MARGIN = 0.05;
const CROP_OPACITY = 0.5;
const WOODLAND = 0.12;
const WOODS_OPACITY = 0.35;
// Hedges along `HEDGE_EDGE` of the boundaries between holdings and
// `HEDGE_SIDE` of those between fields of one holding, `HEDGE_WIDTH`
// widths thick. Crowns beside a hedge stand in it as hedgerow trees.
const HEDGE = [84, 108, 66];
const HEDGE_EDGE = 0.9;
const HEDGE_SIDE = 0.55;
const HEDGE_WIDTH = 0.18;
// Marsh on the clay plug of an old oxbow: its opacity while young, and
// the fraction of its weight left once it has faded over `scar_lifetime`
// after the lake fills, as the scar does. What is left stays until the
// channel sweeps it away, so old meander belts stay faintly legible. The
// blurred weight is contoured at `PLUG_LEVELS`, nested regions that soften
// its edge and let the faded marsh show only at the lowest.
const PLUG_MARSH = 0.22;
const PLUG_RESIDUAL = 0.3;
const PLUG_LEVELS = [0.2, 0.45, 0.7];
// Scroll bars: ridges of fresh sand that vegetation colonises first, so
// they darken against the bar as they age. They are kept in this many age
// bands, each one path element.
const RIDGE = [238, 230, 196];
const OLD_RIDGE = [146, 158, 108];
const SCROLL_BANDS = 6;
// Coarse ground, the gravel of the tributary fans and the colluvium below
// the bluffs: a tint where its share of coarse material passes the first
// level, and a stipple of pebbles where it passes the second. It changes
// slowly, so it is redrawn every `COARSE_REFRESH` of simulated time, 20
// units or 50 years.
const GRAVEL = [200, 194, 172];
const PEBBLE = [126, 118, 100];
const COARSE_LEVELS = [0.15, 0.45];
const COARSE_REFRESH = 20;
// Valley walls are redrawn after the bluffs slump, at most every
// `WALL_REFRESH` of simulated time: 20 units, 50 years.
const WALL_REFRESH = 20;
// Tributary streams: their width at the largest tributary, in channel
// widths, and the size of the bends they wind in along their courses, in
// channel widths: how far they swing either side, and their length, which
// drifts by up to half either way along the stream.
const STREAM_WIDTH = 0.3;
const STREAM_SWING = 0.18;
const STREAM_BEND = 3;
// Large wood: logs lodged at the bank, leaning downstream into the water,
// from 0.5 to 0.9 channel widths long.
const LOG = [84, 64, 44];
const LOG_LENGTH = [0.5, 0.9];
// Mills: the weir's stone and the white water below it, the leat run dry
// once the mill is a ruin, and the mill house's roof and walls, `HOUSE`
// channel widths long and deep. The pond above a working weir lies darker
// over `pond_length`.
const STONE = [166, 158, 134];
const FOAM = [206, 222, 222];
const DRY_LEAT = [150, 140, 108];
const ROOF = [164, 128, 100];
const WALLING = [118, 106, 88];
const HOUSE = [1.1, 0.7];
// Points of a leat below its weir: `LEAT_DOWN` (3 widths) in the core.
const LEAT_TAIL = 6;
// Farmsteads: each building a roof at the core's footprint for its kind,
// in channel widths along and across the ridge, in the order of `Kind`:
// house, outshut, barn, byre, shed, cottage, field barn. Houses and
// cottages are tiled, the farm buildings thatched, and an outshut is a
// lean-to with no ridge. A building on a mound has a rim of raised ground
// `MOUND_RIM` widths wide, and a farmstead's yard is trodden earth
// `YARD_RIM` widths beyond its buildings. An abandoned building fades out
// as a house platform over the core's `RUIN_TIME` (150 years); the
// farmsteads are redrawn at least every `BUILDING_REFRESH` (50 years) as
// the platforms fade.
const FOOTPRINTS = [[1.1, 0.7], [0.9, 0.35], [1.5, 0.8], [1.0, 0.55], [0.8, 0.5], [0.8, 0.55], [0.7, 0.45]];
const THATCH = [146, 128, 96];
const YARD_EARTH = [204, 190, 152];
const MOUND = [180, 170, 126];
const PLATFORM = [150, 140, 108];
const MOUND_RIM = 0.3;
const YARD_RIM = 0.35;
const BUILDING_REFRESH = 20;
// The channel's edge: a thin line of bank on both sides, `BANK_EDGE` times
// the local width from the centreline, and the wet sand of the point bar
// on the inside of a bend, up to `BAR_RIM` widths more at a bend of radius
// `BAR_RADIUS` widths. The outer bank is cut, so vegetation meets the
// water there.
const BANK_EDGE = 0.53;
const BAR_RIM = 0.45;
const BAR_RADIUS = 3;
const WET_SAND = [190, 170, 120];
// Depth in the channel. Pools lie against the outer bank of each bend,
// `POOL_LAG` widths downstream of the apex, in nested crescents reaching
// `POOL_LEVELS` of the width in from the bank at a bend of radius
// `BAR_RADIUS` widths or tighter. Riffles are shallow lenses, up to
// `RIFFLE_WIDTH` of the width across, where the flow crosses between bends
// of opposite sign within `RIFFLE_LOOK` widths upstream and downstream: over
// the reach whose curvature is under `RIFFLE_CROSSING` of the weaker of the
// two bends'. They grow in as both bends tighten past `RIFFLE_RADIUS`
// widths, over a band of `RIFFLE_RAMP` of its curvature either side. Every
// step is continuous in the curvature, so the channel's jitter moves a
// riffle a little and does not switch it on and off.
const POOL_LAG = 1;
const POOL_LEVELS = [0.5, 0.25];
const POOL_COLOURS = [[50, 106, 146], [38, 88, 124]];
const RIFFLE_WIDTH = 0.75;
const RIFFLE_LOOK = 8;
const RIFFLE_CROSSING = 0.5;
const RIFFLE_RADIUS = 8;
const RIFFLE_RAMP = 0.25;
const SHALLOW = [96, 154, 182];
// Where the channel is wider than `MID_BAR_FROM` widths, a sand bar splits
// the flow, growing to `MID_BAR` of the width across at `MID_BAR_FULL`.
const MID_BAR_FROM = 1.5;
const MID_BAR_FULL = 1.8;
const MID_BAR = 0.4;
// Fresh point-bar sand, and the grassed-over colour old bars fade towards.
const BAR = [233, 222, 186];
const OLD_BAR = [200, 202, 156];
// Point bars are kept in this many age bands, each one group of paths.
const BANDS = 24;
// Ridge and swale: each kept centreline is laid in one of `BAR_TONES`
// tones, from the bar's sand on the ridges to the darker, damper silt of
// `SWALE` in the hollows, `BAR_CONTRAST` of the way at the darkest. The
// tone follows seeded noise in time with knots every `BAR_GRAIN` units of
// simulated time (5 years) and every 5 times that, so the whole river lays
// the same tone at once and each bar shows arcs of its growth, wide where
// it grew fast and crowded where slowly. The contrast falls to
// `BAR_RELICT` of its first value as the bar ages, so old bars keep faint
// ridges under the grass.
const SWALE = [176, 176, 134];
const BAR_TONES = 4;
const BAR_CONTRAST = 0.55;
const BAR_GRAIN = 2;
const BAR_RELICT = 0.35;
// Points on each rounded end of a lake.
const CAP_STEPS = 8;
// Fraction of its length a lake loses from each end over its lifetime.
const PLUG = 0.4;
// Widest local channel width, as in the core's `MAX_WIDTH`.
const MAX_WIDTH = 2;
// Centreline point spacing in widths, as in the core's `SPACING`.
const SPACING = 0.5;
// Length, as a multiple of the lake's half-width, that a plugged end's cap
// grows by over the lake's lifetime, so plugged ends stay blunt.
const PLUG_TIP = 0.5;
// Shortest lake piece drawn, in channel widths. The channel leaves short
// pieces where it cuts across a lake, which would show as scattered dashes.
const MIN_LAKE = 3;
// A lake point beside the channel, with less than `ALONGSIDE_GAP` widths of
// bank between their water and running within about 45° of parallel to
// it, is drawn dry. A channel that has migrated along an old lake would
// otherwise show it as a second bank beside its own. Points within
// `ALONGSIDE_END` widths of either end of a piece keep their water, so a
// lake still joins the channel at a cut. Each test ramps over the band
// `ALONGSIDE_RAMP` either side of its threshold, as a fraction of it, and
// the lake there shoals, so the channel's jitter from step to step moves
// the lake's outline a little and does not split it on and off.
const ALONGSIDE_GAP = 0.5;
const ALONGSIDE_COS = 0.7;
const ALONGSIDE_END = 2;
const ALONGSIDE_RAMP = 0.5;
const ALONGSIDE_SPAN = 3;
// A centreline is kept once some part of it has moved this many widths from
// the last kept one, so the swept area has no gaps.
const BAR_STEP = 0.5;
// Segments in each run of a long polyline drawn as one path element. The
// browser rasterises the view in tiles and skips elements whose bounds miss
// a tile, so short runs spare each tile from stroking the whole line.
const RUN = 24;
// Change in a lake's fill that redraws its outline. A lake fills over about
// a thousand units of simulated time, so its outline is rebuilt every few
// units, not every step, and moves less than a pixel at each rebuild.
const LAKE_STEP = 1e-3;

// Whether a layer last drawn at `last`, a `{ key, t }`, is due again: once
// `key` has changed or `every` of simulated time has passed, or with
// `both` only once both have.
function isDue(last, key, t, every, both = false) {
  if (!last || t < last.t) return true;
  const changed = last.key !== key, aged = t - last.t >= every;
  return both ? changed && aged : changed || aged;
}

function el(name, attrs, parent) {
  const e = document.createElementNS(NS, name);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  parent?.append(e);
  return e;
}

// Set an attribute only when its value changes, so the browser does not
// repaint an element that looks the same.
function set(e, name, value) {
  const last = e.attrs ??= {};
  if (last[name] === value) return;
  last[name] = value;
  e.setAttribute(name, value);
}

// `v` to one decimal place, without the trailing zero `toFixed` keeps,
// and several times faster.
function dp1(v) {
  return '' + Math.round(10 * v) / 10;
}

// Unit left normal and signed curvature at each point of a flat x, y array,
// positive where the river turns towards the left normal.
function channelFrame(pts) {
  const n = pts.length / 2;
  const nx = new Float64Array(n), ny = new Float64Array(n), curv = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const [a, b] = [Math.max(0, i - 1), Math.min(n - 1, i + 1)];
    const tx = pts[2 * b] - pts[2 * a], ty = pts[2 * b + 1] - pts[2 * a + 1];
    const len = Math.sqrt(tx * tx + ty * ty) || 1;
    nx[i] = -ty / len;
    ny[i] = tx / len;
    if (a === i || b === i) continue;
    const ax = pts[2 * i] - pts[2 * a], ay = pts[2 * i + 1] - pts[2 * a + 1];
    const bx = pts[2 * b] - pts[2 * i], by = pts[2 * b + 1] - pts[2 * i + 1];
    const denom = Math.sqrt((ax * ax + ay * ay) * (bx * bx + by * by)) * len;
    curv[i] = denom > 0 ? 2 * (ax * by - ay * bx) / denom : 0;
  }
  return { nx, ny, curv };
}

// Closed path data for the channel over points [start, end), out to
// `scale` times the local width either side, plus `bar[i]` on the side of
// the left normal its sign gives, if `bar` is given: one bank forwards, the
// other back. At a bend
// tighter than that, the inner bank stops just short of the centre of the
// bend, so it cannot fold over itself.
function channelData(pts, widths, frame, scale, bar, start, end) {
  if (end - start < 2) return '';
  const { nx, ny, curv } = frame;
  const left = [], right = [];
  for (let i = start; i < end; i++) {
    const h = scale * widths[i], k = curv[i], b = bar ? bar[i] : 0;
    const [bl, br] = [h + Math.max(0, b), h + Math.max(0, -b)];
    const hl = k > 0 ? Math.min(bl, 0.95 / k) : bl;
    const hr = k < 0 ? Math.min(br, -0.95 / k) : br;
    const [x, y] = [pts[2 * i], pts[2 * i + 1]];
    left.push(dp1(x + hl * nx[i]) + ' ' + dp1(y + hl * ny[i]));
    right.push(dp1(x - hr * nx[i]) + ' ' + dp1(y - hr * ny[i]));
  }
  return 'M' + left.join(' ') + 'L' + right.reverse().join(' ') + 'Z';
}

// Channel outlines in runs of `RUN` segments, each overlapping its
// neighbours by a segment so no seam shows between them.
function channelRuns(pts, widths, frame, scale, bar = null) {
  const n = pts.length / 2;
  const runs = [];
  for (let s = 0; s < n - 1; s += RUN) {
    runs.push(channelData(pts, widths, frame, scale, bar, Math.max(0, s - 1), Math.min(n, s + RUN + 2)));
  }
  return runs;
}

// Width of the wet sand of the point bar beyond the inner bank at each
// point, in world units, signed by the side of the left normal it lies on:
// `BAR_RIM` widths at a bend of radius `BAR_RADIUS` widths or tighter,
// falling to nothing on a straight reach. `bend` is the curvature smoothed
// along the river and sets both the size and the side, so the bar does not
// flicker with the point-to-point noise in it.
function barRim(bend, w) {
  return bend.map((c) => Math.sign(c) * BAR_RIM * w * smoothstep(Math.abs(c) * w * BAR_RADIUS));
}

// `values` smoothed along the river by four [1, 2, 1] passes.
function smoothed(values) {
  const n = values.length;
  let v = Float64Array.from(values);
  for (let pass = 0; pass < 4; pass++) {
    const prev = v;
    v = new Float64Array(n);
    for (let i = 0; i < n; i++) v[i] = 0.25 * prev[Math.max(0, i - 1)] + 0.5 * prev[i] + 0.25 * prev[Math.min(n - 1, i + 1)];
  }
  return v;
}

// Closed path data for a band along the river over points [start, end),
// between signed offsets `a[i]` and `b[i]` along the left normal.
function bandData(pts, frame, a, b, start, end) {
  const { nx, ny } = frame;
  const left = [], right = [];
  for (let i = start; i < end; i++) {
    const [x, y] = [pts[2 * i], pts[2 * i + 1]];
    left.push(dp1(x + a[i] * nx[i]) + ' ' + dp1(y + a[i] * ny[i]));
    right.push(dp1(x + b[i] * nx[i]) + ' ' + dp1(y + b[i] * ny[i]));
  }
  return 'M' + left.join(' ') + 'L' + right.reverse().join(' ') + 'Z';
}

// Bands between offsets `a` and `b` over the stretches where they are
// apart, one path data string each, skipping points where they meet.
function bandRuns(pts, frame, a, b) {
  const n = pts.length / 2, runs = [];
  let start = -1;
  for (let i = 0; i <= n; i++) {
    const open = i < n && Math.abs(a[i] - b[i]) > 0.01;
    if (open && start < 0) start = Math.max(0, i - 1);
    if (!open && start >= 0) {
      runs.push(bandData(pts, frame, a, b, start, Math.min(n, i + 1)));
      start = -1;
    }
  }
  return runs;
}

// Pools, riffles and mid-channel bars, as lists of path data: one list per
// pool level, then riffles, then bars. `bend` is the smoothed signed
// curvature, `widths` the local width in world units, `w` the reference
// width.
function channelDepth(pts, widths, frame, bend, w) {
  const n = bend.length;
  const lag = Math.round(POOL_LAG / SPACING);
  const pools = POOL_LEVELS.map((level) => {
    const a = new Float64Array(n), b = new Float64Array(n);
    for (let i = 0; i < n; i++) {
      // The bank on the outside of the bend upstream, and the pool's
      // reach in from it.
      const k = bend[Math.max(0, i - lag)], half = 0.5 * widths[i];
      const out = k > 0 ? -1 : 1;
      const reach = level * widths[i] * smoothstep(Math.abs(k) * w * BAR_RADIUS);
      a[i] = out * half;
      b[i] = out * Math.max(0, half - reach);
    }
    return bandRuns(pts, frame, a, b);
  });
  // Strength of the riffle at each point: the weaker of the strongest
  // bends of opposite sign upstream and downstream, ramped about
  // `RIFFLE_RADIUS`, where the curvature here is small beside it.
  const look = Math.round(RIFFLE_LOOK / SPACING), tight = 1 / (RIFFLE_RADIUS * w);
  const strength = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    let [upL, upR, downL, downR] = [0, 0, 0, 0];
    for (let j = Math.max(0, i - look); j <= i; j++) [upL, upR] = [Math.max(upL, bend[j]), Math.max(upR, -bend[j])];
    for (let j = i; j < Math.min(n, i + look + 1); j++) [downL, downR] = [Math.max(downL, bend[j]), Math.max(downR, -bend[j])];
    const bends = Math.max(Math.min(upL, downR), Math.min(upR, downL));
    if (bends <= 0) continue;
    const grow = smoothstep((bends / tight - 1 + RIFFLE_RAMP) / (2 * RIFFLE_RAMP));
    strength[i] = grow * (1 - smoothstep(Math.abs(bend[i]) / (RIFFLE_CROSSING * bends)));
  }
  // Smoothed along the river, so each riffle tapers to round ends.
  const riffle = smoothed(strength);
  const ra = new Float64Array(n), rb = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const h = 0.5 * RIFFLE_WIDTH * widths[i] * riffle[i];
    [ra[i], rb[i]] = [h, -h];
  }
  // Mid-channel bars, on the width smoothed so they grow and shrink
  // slowly.
  const wide = smoothed(widths);
  const ba = new Float64Array(n), bb = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const h = 0.5 * MID_BAR * wide[i] * smoothstep((wide[i] / w - MID_BAR_FROM) / (MID_BAR_FULL - MID_BAR_FROM));
    [ba[i], bb[i]] = [h, -h];
  }
  return [...pools, bandRuns(pts, frame, ra, rb), bandRuns(pts, frame, ba, bb)];
}

// Path data for points [start, end) of a flat x, y array.
function pathData(pts, start, end) {
  if (end - start < 2) return '';
  const parts = new Array(end - start);
  for (let i = start; i < end; i++) parts[i - start] = dp1(pts[2 * i]) + ' ' + dp1(pts[2 * i + 1]);
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

// Seeded value noise over [0, span], in [0, 1]: octaves with knots every
// `lens[m]`, weighted 1 / (m + 1).
function valueNoise(rand, span, lens) {
  const octaves = lens.map((len) => Array.from({ length: Math.ceil(span / len) + 2 }, rand));
  const total = lens.reduce((t, _, m) => t + 1 / (m + 1), 0);
  return (x) => {
    let u = 0;
    lens.forEach((len, m) => {
      const t = clamp01(x / span) * span / len, j = Math.floor(t), knots = octaves[m];
      u += (knots[j] + (knots[j + 1] - knots[j]) * smoothstep(t - j)) / (m + 1);
    });
    return u / total;
  };
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

// Seeded hash of whole numbers, which may exceed 32 bits, to [0, 1).
function hash01(...values) {
  let h = 0x811c9dc5;
  for (const v of values) {
    h = Math.imul(h ^ (v >>> 0), 0x01000193);
    h = Math.imul(h ^ Math.floor(v / 4294967296), 0x01000193);
    h ^= h >>> 15;
  }
  return (Math.imul(h, 0x2c1b3c6d) >>> 0) / 4294967296;
}

// Points [x0, y0, x1, y1, …] of a polyline with its end points kept and the
// interior simplified to within `tol` (Douglas–Peucker).
function simplify(pts, tol) {
  const n = pts.length / 2;
  if (n < 3) return pts;
  const keep = new Uint8Array(n);
  keep[0] = keep[n - 1] = 1;
  const stack = [[0, n - 1]];
  while (stack.length) {
    const [a, b] = stack.pop();
    const [ax, ay, bx, by] = [pts[2 * a], pts[2 * a + 1], pts[2 * b], pts[2 * b + 1]];
    const [dx, dy] = [bx - ax, by - ay];
    const len2 = dx * dx + dy * dy;
    let [far, farD] = [-1, tol * tol];
    for (let k = a + 1; k < b; k++) {
      const [px, py] = [pts[2 * k] - ax, pts[2 * k + 1] - ay];
      const t = len2 > 0 ? clamp01((px * dx + py * dy) / len2) : 0;
      const d = (px - t * dx) ** 2 + (py - t * dy) ** 2;
      if (d > farD) [far, farD] = [k, d];
    }
    if (far >= 0) {
      keep[far] = 1;
      stack.push([a, far], [far, b]);
    }
  }
  const out = [];
  for (let k = 0; k < n; k++) if (keep[k]) out.push(pts[2 * k], pts[2 * k + 1]);
  return out;
}

// One pass of Chaikin's corner cutting over a polyline, keeping its end
// points, or over a closed ring whose last point repeats its first.
function chaikin(pts, closed) {
  const n = pts.length / 2;
  if (n < 3) return pts;
  const out = closed ? [] : [pts[0], pts[1]];
  for (let k = 0; k + 1 < n; k++) {
    const [ax, ay, bx, by] = [pts[2 * k], pts[2 * k + 1], pts[2 * k + 2], pts[2 * k + 3]];
    if (closed || k > 0) out.push(0.75 * ax + 0.25 * bx, 0.75 * ay + 0.25 * by);
    if (closed || k + 2 < n) out.push(0.25 * ax + 0.75 * bx, 0.25 * ay + 0.75 * by);
  }
  if (closed) out.push(out[0], out[1]);
  else out.push(pts[2 * n - 2], pts[2 * n - 1]);
  return out;
}

// The convex hull of points given as a flat x, y array, as a flat array
// in order round it, by Andrew's monotone chain.
function hull(pts) {
  const p = [];
  for (let k = 0; k < pts.length; k += 2) p.push([pts[k], pts[k + 1]]);
  p.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross = (o, a, b) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const half = (list) => {
    const out = [];
    for (const q of list) {
      while (out.length >= 2 && cross(out[out.length - 2], out[out.length - 1], q) <= 0) out.pop();
      out.push(q);
    }
    out.pop();
    return out;
  };
  return [...half(p), ...half([...p].reverse())].flat();
}

// A flat x, y array with its points in reverse order.
function reversePoints(pts) {
  const out = new Array(pts.length);
  for (let k = 0; k < pts.length; k += 2) [out[pts.length - 2 - k], out[pts.length - 1 - k]] = [pts[k], pts[k + 1]];
  return out;
}

// Path data for a polyline given as a flat x, y array.
function pointsData(pts) {
  let d = 'M' + dp1(pts[0]) + ' ' + dp1(pts[1]);
  for (let k = 2; k < pts.length; k += 2) d += (k === 2 ? 'L' : ' ') + dp1(pts[k]) + ' ' + dp1(pts[k + 1]);
  return d;
}

// Path data for the leat of `count` points from point `start` of `leats`,
// rounded by a pass of corner cutting.
function leatData(leats, start, count) {
  return pointsData(chaikin(Array.from(leats.subarray(2 * start, 2 * (start + count))), false));
}

// Trace the boundaries of a map of labels, `W` × `H` cells row by row with
// 0 for no label and outside the map, along the cell edges. Each boundary
// between two junctions of three or more regions, or round a region that
// meets no junction, is one arc: its corners as [i0, j0, i1, j1, …] on the
// grid of cell corners, its end corners `start` and `end` as ids
// j·(W + 1) + i, the label `a` on its left as it runs, the label `b` on its
// right, and the cells on its left. A region's outline is the arcs it lies
// left of, run forwards, and the arcs it lies right of, run backwards.
// Yields between rows whenever `due()`, and returns the arcs.
function* boundaryArcs(lab, W, H, due) {
  const L = (i, j) => (i < 0 || j < 0 || i >= W || j >= H ? 0 : lab[j * W + i]);
  const CW = W + 1, nh = W * (H + 1);
  // Edge ids: j·W + i for the edge from corner (i, j) rightwards, and
  // nh + j·CW + i for the edge from corner (i, j) downwards.
  const edges = new Uint8Array(nh + CW * H);
  for (let j = 0; j <= H; j++) {
    if (due()) yield;
    for (let i = 0; i < W; i++) edges[j * W + i] = L(i, j - 1) !== L(i, j);
    if (j < H) for (let i = 0; i <= W; i++) edges[nh + j * CW + i] = L(i - 1, j) !== L(i, j);
  }
  const incident = (i, j, out) => {
    out.length = 0;
    if (i > 0 && edges[j * W + i - 1]) out.push(j * W + i - 1, i - 1, j);
    if (i < W && edges[j * W + i]) out.push(j * W + i, i + 1, j);
    if (j > 0 && edges[nh + (j - 1) * CW + i]) out.push(nh + (j - 1) * CW + i, i, j - 1);
    if (j < H && edges[nh + j * CW + i]) out.push(nh + j * CW + i, i, j + 1);
    return out.length / 3;
  };
  const used = new Uint8Array(edges.length);
  const arcs = [], around = [], step = [];
  // Run from corner (i, j) along edge `e` to corner (ni, nj), and on until
  // a junction or back to the start.
  const walk = (i, j, e, ni, nj) => {
    const [dx, dy] = [ni - i, nj - j];
    const side = (s) => L(Math.floor(i + 0.5 * dx + 0.5 * s * dy), Math.floor(j + 0.5 * dy - 0.5 * s * dx));
    const arc = { pts: [i, j], start: j * CW + i, a: side(1), b: side(-1), cells: [] };
    for (;;) {
      used[e] = 1;
      const [ex, ey] = [ni - i, nj - j];
      const [ci, cj] = [Math.floor(i + 0.5 * ex + 0.5 * ey), Math.floor(j + 0.5 * ey - 0.5 * ex)];
      if (ci >= 0 && cj >= 0 && ci < W && cj < H) arc.cells.push(cj * W + ci);
      [i, j] = [ni, nj];
      arc.pts.push(i, j);
      if (incident(i, j, around) !== 2) break;
      let next = -1;
      for (let k = 0; k < around.length; k += 3) if (!used[around[k]]) next = k;
      if (next < 0) break;
      [e, ni, nj] = [around[next], around[next + 1], around[next + 2]];
    }
    arc.end = j * CW + i;
    arcs.push(arc);
  };
  for (let j = 0; j <= H; j++) {
    if (due()) yield;
    for (let i = 0; i <= W; i++) {
      if (incident(i, j, step) <= 2) continue;
      for (let k = 0; k < step.length; k += 3) if (!used[step[k]]) walk(i, j, step[k], step[k + 1], step[k + 2]);
    }
  }
  // What is left is closed loops, each with an edge along a row.
  for (let j = 0; j <= H; j++) {
    if (due()) yield;
    for (let i = 0; i < W; i++) if (edges[j * W + i] && !used[j * W + i]) walk(i, j, j * W + i, i + 1, j);
  }
  return arcs;
}

// Path data for a circle.
function circleData(x, y, r) {
  const [r1, d] = [dp1(r), dp1(2 * r)];
  return `M${dp1(x - r)},${dp1(y)}a${r1},${r1} 0 1 0 ${d},0a${r1},${r1} 0 1 0 -${d},0`;
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
// is `relief` × 4 × (height − 0.5) for the land heights in `heights`, plus
// the overbank silt in `silts` above its mean over the lake's interior points;
// the mean is part of the fill that sets `level`. Each point's depth moves
// by its fraction in `dry`, if given, towards `level` above the water.
// Where the bed rises above the water the lake
// splits into pools, one subpath each, and the parabola gives each pool a
// round tip there. A plugged end is a round cap when young, lengthening by
// `PLUG_TIP` times the half-width with `age` in [0, 1]. An end cut by the
// channel stays a round cap, hidden under the water.
function lakeData(pts, heights, silts, widths, start, end, from, to, half, level, relief, age, dry) {
  const m = end - start;
  if (m < 2 || level <= 0) return '';
  let mean = 0;
  for (let k = 0; k < m; k++) mean += widths[start + k] / m;
  const wide = half * mean;
  const s = new Float64Array(m), tx = new Float64Array(m), ty = new Float64Array(m);
  const depth = new Float64Array(m);
  let silt = 0;
  for (let k = 1; k < m - 1; k++) silt += silts[start + k] / Math.max(1, m - 2);
  // The end points lie under the channel at a cut, swept clear of silt.
  for (let k = 0; k < m; k++) {
    const extra = k > 0 && k < m - 1 ? silts[start + k] - silt : 0;
    const wet = level - 4 * relief * (heights[start + k] - 0.5) - extra;
    depth[k] = wet - (dry ? dry[k] * (wet + level) : 0);
  }
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
  // Half-width at fraction `u` along segment `j`, at depth `d`: `half`
  // times the width of the channel the lake was cut from.
  const width = (j, u, d) => {
    const b = widths[start + j] + (widths[start + j + 1] - widths[start + j]) * u;
    return half * b * Math.sqrt(Math.min(1, Math.max(0, d)));
  };

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
    // A pool shorter than the half-width narrows with its length, so it
    // shrinks away and does not vanish at once.
    const thin = smoothstep((v0 - u0) / wide);
    if (thin < 0.02) continue;
    // Each end: a round cap of the local width at a plugged or cut end, or
    // a parabolic tip where the bed rises above the water, sampled densely
    // near the tip.
    const end = (at, plugged) => {
      if (at !== a && at !== b) return { len: wide, cap: false };
      const [jj, uu] = locate(at, 0);
      const r = width(jj, uu, depthAt(jj, uu));
      return { len: plugged ? r * (1 + PLUG_TIP * age) : r, cap: true };
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
      const h = thin * shape * width(j, f, depthAt(j, f)) / (Math.hypot(nx, ny) || 1);
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

// Arc lengths of points [start, end) of a flat x, y array from the first,
// and the unit tangents there, by central differences.
function arcFrame(pts, start, end) {
  const m = end - start;
  const s = new Float64Array(m), tx = new Float64Array(m), ty = new Float64Array(m);
  for (let k = 1; k < m; k++) {
    const i = 2 * (start + k);
    s[k] = s[k - 1] + Math.hypot(pts[i] - pts[i - 2], pts[i + 1] - pts[i - 1]);
  }
  for (let k = 0; k < m; k++) {
    const p = 2 * (start + Math.max(0, k - 1)), q = 2 * (start + Math.min(m - 1, k + 1));
    const dx = pts[q] - pts[p], dy = pts[q + 1] - pts[p + 1];
    const l = Math.hypot(dx, dy) || 1;
    tx[k] = dx / l;
    ty[k] = dy / l;
  }
  return { s, tx, ty };
}

// How much of its width an oxbow scar keeps at arc length `at` along a
// piece of length `len`, which starts `along` into an oxbow of length
// `total`. The scar narrows to round tips at the oxbow's own ends, which
// filled first, so it is widest in the middle where the lake last stood.
// An end the channel has cut since narrows over the last width `w`, under
// the channel's water.
function scarTaper(at, len, along, total, w) {
  let f = Math.sqrt(Math.max(0, Math.sin(Math.PI * clamp01((along + at) / total))));
  if (along > 1e-3) f *= Math.sqrt(clamp01(at / w));
  if (along + len < total - 1e-3) f *= Math.sqrt(clamp01((len - at) / w));
  return f;
}

// Outline of an oxbow scar along points [start, end) of a flat x, y array,
// `half` times the old channel's width in `widths` either side of the
// centreline, narrowed by `scarTaper`.
function scarData(pts, widths, start, end, along, total, half, w) {
  const m = end - start;
  if (m < 2) return '';
  const { s, tx, ty } = arcFrame(pts, start, end);
  const len = s[m - 1];
  const left = [], right = [];
  for (let k = 0; k < m; k++) {
    const i = 2 * (start + k);
    const h = half * w * widths[start + k] * scarTaper(s[k], len, along, total, w);
    left.push(dp1(pts[i] - ty[k] * h) + ' ' + dp1(pts[i + 1] + tx[k] * h));
    right.push(dp1(pts[i] + ty[k] * h) + ' ' + dp1(pts[i + 1] - tx[k] * h));
  }
  return 'M' + left.join('L') + 'L' + right.reverse().slice(1, -1).join('L') + 'Z';
}

// Marsh tufts on an oxbow scar, scattered `half` times the old channel's
// width either side of the centreline, drawn as on old survey maps: a short
// base with three blades fanned above it, upright whatever the scar's
// direction. One stands in each `TUFT_STEP` widths of the oxbow, jittered
// along and across the scar and thinned towards its tips. Each is seeded by
// its place along the whole oxbow, so a piece the channel splits off keeps
// its tufts where they were.
function tuftData(pts, widths, start, end, along, total, half, w) {
  const m = end - start;
  if (m < 2) return '';
  const { s, tx, ty } = arcFrame(pts, start, end);
  const len = s[m - 1], step = TUFT_STEP * w, seed = Math.round(total);
  let d = '', k = 0;
  for (let j = Math.floor(along / step); (j - 1) * step < along + len; j++) {
    const at = (j + 0.2 + 0.6 * hash01(seed, j, 1)) * step - along;
    if (at < 0 || at > len) continue;
    const taper = scarTaper(at, len, along, total, w);
    if (hash01(seed, j, 2) > 1.4 * taper - 0.2) continue;
    while (k < m - 2 && s[k + 1] < at) k++;
    const f = (at - s[k]) / (s[k + 1] - s[k] || 1);
    const i = 2 * (start + k);
    const across = (2 * hash01(seed, j, 3) - 1) * half * w * widths[start + k] * taper;
    const x = pts[i] + (pts[i + 2] - pts[i]) * f - ty[k] * across;
    const y = pts[i + 1] + (pts[i + 3] - pts[i + 1]) * f + tx[k] * across;
    const a = TUFT_SIZE * w * (0.75 + 0.5 * hash01(seed, j, 4));
    const [b, c] = [dp1(0.3 * a), dp1(0.5 * a)];
    d += `M${dp1(x - 0.5 * a)} ${dp1(y)}h${dp1(a)}M${dp1(x)} ${dp1(y)}v-${dp1(0.7 * a)}`
      + `M${dp1(x - 0.12 * a)} ${dp1(y)}l-${b}-${c}M${dp1(x + 0.12 * a)} ${dp1(y)}l${b}-${c}`;
  }
  return d;
}

// Marching squares cases: for each set of corners above the level (top left
// 8, top right 4, bottom right 2, bottom left 1), the cell edges each
// contour segment joins, as T, R, B, L. Saddles 5 and 10 list the pairing
// for a centre above the level, then for one below it.
const CASES = [
  [], ['LB'], ['BR'], ['LR'], ['TR'], [['LT', 'BR'], ['LB', 'TR']], ['TB'], ['LT'],
  ['LT'], ['TB'], [['TR', 'LB'], ['LT', 'BR']], ['TR'], ['LR'], ['BR'], ['LB'], [],
];

// `CASES` as numbers for `contourData`: the sides each segment joins, as 0
// top, 1 right, 2 bottom and 3 left, in pairs. Entry 2c is case c, and
// entry 2c + 1 the other pairing of a saddle, or case c again.
const CASE_SIDES = CASES.flatMap((pairs, c) => (c === 5 || c === 10 ? pairs : [pairs, pairs])
  .map((ps) => Int8Array.from(ps.flatMap((p) => [...p].map((s) => 'TRBL'.indexOf(s))))));

// Scratch buffers for `contourData`, grown as needed and reused.
const contourScratch = { v: new Float32Array(0), at: new Int32Array(0), segs: new Int32Array(0), used: new Uint8Array(0) };

// Region where a grid exceeds `level`, as path data for the even-odd fill
// rule. `values` holds `cols` × `rows` cells of side `cell`, row by row; only
// columns [c0, c1) are traced, and cells outside them count as below the
// level, so every contour closes. The contours run through the cell centres,
// placed by linear interpolation along the cell edges, to a tenth of a unit.
function contourData(values, cols, rows, cell, c0, c1, level) {
  // Corner grid over the cell centres, with a one-cell border below the level.
  const W = c1 - c0 + 2, H = rows + 2, n = W * H;
  const scratch = contourScratch;
  if (scratch.v.length < n) {
    scratch.v = new Float32Array(n);
    scratch.at = new Int32Array(4 * n);
    // At most two segments a cell.
    scratch.segs = new Int32Array(4 * n);
    scratch.used = new Uint8Array(2 * n);
  }
  const { v, at, segs, used } = scratch;
  v.fill(-level, 0, n);
  for (let j = 1; j <= rows; j++) {
    const row = (j - 1) * cols + c0 - 1;
    for (let i = 1; i <= c1 - c0; i++) v[j * W + i] = values[row + i];
  }
  // Edge ids: 2k is the edge from corner k rightwards, 2k + 1 downwards. The
  // cell with top-left corner k has the edge 2k + `side[s]` on side s.
  const side = [0, 3, 2 * W, 1];
  // Segments as pairs of edge ids, and up to two segments at each edge.
  at.fill(-1, 0, 4 * n);
  let count = 0;
  for (let j = 0; j < H - 1; j++) {
    for (let i = 0; i < W - 1; i++) {
      const k = j * W + i;
      const tl = v[k], tr = v[k + 1], br = v[k + W + 1], bl = v[k + W];
      const c = (tl > level) * 8 + (tr > level) * 4 + (br > level) * 2 + (bl > level);
      if (c === 0 || c === 15) continue;
      const saddle = (c === 5 || c === 10) && (tl + tr + br + bl) / 4 <= level;
      const sides = CASE_SIDES[2 * c + saddle];
      for (let m = 0; m < sides.length; m += 2) {
        const ea = 2 * k + side[sides[m]], eb = 2 * k + side[sides[m + 1]];
        segs[2 * count] = ea;
        segs[2 * count + 1] = eb;
        at[2 * ea + (at[2 * ea] < 0 ? 0 : 1)] = count;
        at[2 * eb + (at[2 * eb] < 0 ? 0 : 1)] = count;
        count++;
      }
    }
  }
  // Where the contour crosses an edge.
  const point = (id) => {
    const k = id >> 1, i = k % W, j = (k - i) / W;
    const down = id & 1;
    const t = (level - v[k]) / (v[down ? k + W : k + 1] - v[k]);
    const x = (c0 + i + (down ? 0 : t) - 0.5) * cell, y = (j + (down ? t : 0) - 0.5) * cell;
    return Math.round(10 * x) / 10 + ' ' + Math.round(10 * y) / 10;
  };
  // Chain the segments into closed loops.
  used.fill(0, 0, count);
  let d = '';
  for (let s0 = 0; s0 < count; s0++) {
    if (used[s0]) continue;
    let path = point(segs[2 * s0]), points = 1;
    let s = s0, e = segs[2 * s0 + 1];
    while (s >= 0) {
      used[s] = 1;
      path += 'L' + point(e);
      points++;
      const next = at[2 * e] === s ? at[2 * e + 1] : at[2 * e];
      if (next < 0 || used[next]) break;
      s = next;
      e = segs[2 * s] === e ? segs[2 * s + 1] : segs[2 * s];
    }
    if (points > 3) d += 'M' + path + 'Z';
  }
  return d;
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

// Arc length of points [start, end) of a flat x, y array.
function pieceLength(pts, start, end) {
  let len = 0;
  for (let i = start + 1; i < end; i++) len += Math.hypot(pts[2 * i] - pts[2 * i - 2], pts[2 * i + 1] - pts[2 * i - 1]);
  return len;
}

// How far `v` is past `threshold`, from 0 at `ALONGSIDE_RAMP` × `threshold`
// below it to 1 as far above it, eased at both ends.
function ramp(v, threshold) {
  const band = ALONGSIDE_RAMP * threshold;
  return smoothstep((v - threshold + band) / (2 * band));
}

// How dry each lake point [start, end) of a flat x, y array is drawn for
// lying beside the channel, from 0 to 1 by index into the piece, or null if
// none is, and a key that changes when it does. See `ALONGSIDE_GAP`.
// `oxWidths` is in multiples of the reference width `w` and `widths` in
// world units; `channel` is a grid of the channel points `line`.
function alongside(pts, oxWidths, start, end, line, widths, channel, w) {
  const m = end - start;
  let dry = null, key = 0, s = 0;
  const ends = ALONGSIDE_END * w, total = pieceLength(pts, start, end);
  for (let k = 1; k < m - 1; k++) {
    const i = start + k;
    s += Math.hypot(pts[2 * i] - pts[2 * i - 2], pts[2 * i + 1] - pts[2 * i - 1]);
    const inner = ramp(Math.min(s, total - s), ends);
    if (inner <= 0) continue;
    const [x, y] = [pts[2 * i], pts[2 * i + 1]];
    const j = channel.nearest(x, y, channel.size);
    if (j < 0) continue;
    // Tangents over `ALONGSIDE_SPAN` points either side, which steadies
    // them against the channel's jitter and resampling from step to step.
    const c = j >> 1, nc = line.length / 2;
    const [c0, c1] = [Math.max(0, c - ALONGSIDE_SPAN), Math.min(nc - 1, c + ALONGSIDE_SPAN)];
    const [i0, i1] = [Math.max(start, i - ALONGSIDE_SPAN), Math.min(end - 1, i + ALONGSIDE_SPAN)];
    const ux = line[2 * c1] - line[2 * c0], uy = line[2 * c1 + 1] - line[2 * c0 + 1];
    const tx = pts[2 * i1] - pts[2 * i0], ty = pts[2 * i1 + 1] - pts[2 * i0 + 1];
    const ul = Math.hypot(ux, uy) || 1;
    // The gap to the channel's segments, not its nearest point, so it does
    // not move with the spacing of the channel's points along it.
    let across = Infinity;
    for (let a = c0; a < c1; a++) {
      const [ax, ay] = [line[2 * a], line[2 * a + 1]];
      const [bx, by] = [line[2 * a + 2] - ax, line[2 * a + 3] - ay];
      const t = clamp01(((x - ax) * bx + (y - ay) * by) / (bx * bx + by * by || 1));
      across = Math.min(across, Math.hypot(x - ax - t * bx, y - ay - t * by));
    }
    const gap = across - 0.5 * (widths[c] + w * oxWidths[i]);
    const near = 1 - ramp(gap, ALONGSIDE_GAP * w);
    if (near <= 0) continue;
    const cos = (tx * ux + ty * uy) / (ul * (Math.hypot(tx, ty) || 1));
    const d = inner * near * ramp(Math.abs(cos), ALONGSIDE_COS);
    if (d <= 0) continue;
    dry ??= new Float32Array(m);
    dry[k] = d;
    key = (31 * key + k * 64 + Math.round(63 * d)) | 0;
  }
  return { dry, key };
}

// Points of a flat x, y array hashed into square cells of side `size`.
// Hashed grid of points, filled by counting sort into flat arrays. Cells
// along a row hash to consecutive buckets, so the three cells around a
// point in a row are one run of the table.
class Grid {
  constructor(pts, size) {
    const n = pts.length / 2;
    let buckets = 1;
    while (buckets < 4 * n) buckets *= 2;
    this.pts = pts;
    this.size = size;
    this.mask = buckets - 1;
    this.stride = 4099;
    this.start = new Int32Array(buckets + 1);
    const cell = new Int32Array(n);
    for (let i = 0; i < n; i++) {
      cell[i] = this.bucket(Math.floor(pts[2 * i] / size), Math.floor(pts[2 * i + 1] / size));
      this.start[cell[i] + 1]++;
    }
    for (let b = 0; b < buckets; b++) this.start[b + 1] += this.start[b];
    const fill = this.start.slice(0, buckets);
    this.items = new Int32Array(n);
    for (let i = 0; i < n; i++) this.items[fill[cell[i]]++] = 2 * i;
  }

  bucket(cx, cy) {
    return (cy * this.stride + cx) & this.mask;
  }

  // Whether any point lies within `r` (at most `size`) of (x, y).
  near(x, y, r) {
    const cx = Math.floor(x / this.size), cy = Math.floor(y / this.size);
    const { start, mask } = this, r2 = r * r;
    for (let j = cy - 1; j <= cy + 1; j++) {
      // The run of three buckets, in two parts where it wraps.
      const b = this.bucket(cx - 1, j), e = Math.min(b + 3, mask + 1);
      if (this.scan(start[b], start[e], x, y, r2) || this.scan(0, start[b + 3 - e], x, y, r2)) return true;
    }
    return false;
  }

  // Offset in `pts` of the point nearest (x, y) within `r` (at most
  // `size`), or −1 if there is none.
  nearest(x, y, r) {
    const cx = Math.floor(x / this.size), cy = Math.floor(y / this.size);
    const { start, mask, items, pts } = this;
    let [best, bestD] = [-1, r * r];
    const scan = (lo, hi) => {
      for (let m = lo; m < hi; m++) {
        const k = items[m], dx = pts[k] - x, dy = pts[k + 1] - y, d = dx * dx + dy * dy;
        if (d <= bestD) [best, bestD] = [k, d];
      }
    };
    for (let j = cy - 1; j <= cy + 1; j++) {
      const b = this.bucket(cx - 1, j), e = Math.min(b + 3, mask + 1);
      scan(start[b], start[e]);
      scan(0, start[b + 3 - e]);
    }
    return best;
  }

  // Whether any point in items [lo, hi) lies within √r2 of (x, y).
  scan(lo, hi, x, y, r2) {
    for (let m = lo; m < hi; m++) {
      const k = this.items[m], dx = this.pts[k] - x, dy = this.pts[k + 1] - y;
      if (dx * dx + dy * dy <= r2) return true;
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
  constructor(bars, ground, svg) {
    this.layers = [bars, ground, svg];
    this.bars = bars;
    this.ground = ground;
    this.svg = svg;
    for (const s of this.layers) s.setAttribute('preserveAspectRatio', 'xMidYMid meet');
    const round = { fill: 'none', 'stroke-linejoin': 'round', 'stroke-linecap': 'round' };
    // Centrelines turn only a few degrees at each point, where a bevel join
    // looks the same as a round one and is far cheaper to rasterise.
    const line = { ...round, 'stroke-linejoin': 'bevel' };
    this.background = el('rect', { y: 0, fill: `rgb(${SOIL.loam})` }, bars);
    // Soil patches lie under the point bars: ground the channel has swept is
    // its own fresh sand, whatever soil was there before.
    this.soilLayer = el('g', {}, bars);
    // Point bars: each age band is one group of paths in one colour, newest
    // on top. A band is opaque while young, so each spot shows the newest
    // band over it and bands never build up. Old bands fade towards the
    // background colour and turn transparent, so the soil shows through
    // again as it forms on the old bar. Bands that have stopped growing lie
    // in the bottom layer; the growing band lies in the ground layer, so
    // keeping a centreline does not repaint the old ones.
    this.oldBars = el('g', line, bars);
    this.barLayer = el('g', line, ground);
    // Coarse ground lies over the bars and under the vegetation that grows
    // over it. Its pebbles are sized for the channel width at each reset.
    this.pebbles = el('pattern', { id: 'pebbles', patternUnits: 'userSpaceOnUse' }, el('defs', {}, ground));
    this.coarseLayer = el('g', { 'fill-rule': 'evenodd' }, ground);
    this.coarse = [
      el('path', { fill: `rgb(${GRAVEL})`, 'fill-opacity': 0.45 }, this.coarseLayer),
      el('path', { fill: 'url(#pebbles)' }, this.coarseLayer),
    ];
    this.coarseAt = null;
    // Shrub and forest lie over the bars and under the scroll bars, so the
    // ridges show through the trees.
    const stepOpacity = 1 - (1 - FOREST_OPACITY) ** (1 / FOREST_STEPS);
    this.forestLayer = el('g', { 'fill-rule': 'evenodd', 'fill-opacity': stepOpacity.toFixed(4) }, ground);
    this.forest = FOREST.flatMap(({ colour }) => Array.from(
      { length: FOREST_STEPS }, () => el('path', { fill: `rgb(${colour})` }, this.forestLayer)));
    this.forestAt = null;
    // The upland's drier soil, under its fields.
    this.uplandLayer = el('g', { fill: `rgb(${UPLAND})`, 'fill-opacity': UPLAND_OPACITY }, ground);
    // Fields lie over the forest, masked by the grassed ground: each fade
    // region is painted over the last in a lighter grey, so the mask
    // brightens towards the oldest ground with no seams between regions.
    const mask = el('mask', { id: 'farmland', 'color-interpolation': 'sRGB' }, el('defs', {}, ground));
    this.farmland = Array.from({ length: FIELD_FADE }, (_, k) => {
      const grey = Math.round(255 * (k + 1) / FIELD_FADE);
      return el('path', { 'fill-rule': 'evenodd', fill: `rgb(${grey},${grey},${grey})` }, mask);
    });
    // No fields in the hanger woods on the valley sides.
    this.scarpMask = el('path', { fill: 'black' }, mask);
    this.fieldLayer = el('g', { mask: 'url(#farmland)' }, ground);
    this.parcels = el('g', {}, this.fieldLayer);
    this.hedges = el('path', {
      fill: 'none', 'stroke-linecap': 'round', 'stroke-linejoin': 'round', stroke: `rgb(${HEDGE})`, 'stroke-opacity': 0.8,
    }, this.fieldLayer);
    // Path element of each field drawn, by its key.
    this.fieldPaths = new Map();
    // Whether each field drawn, by its key, is meadow.
    this.meadow = new Map();
    this.fieldsAt = null;
    // Oxbow scars lie over the fields and under the scroll bars and trees,
    // so alder and willow crowns stand in them. They change slowly, so they
    // sit in the ground layer, not in the top layer that is redrawn whenever
    // the river moves.
    this.scarLayer = el('g', {}, ground);
    // The valley sides' hachures and hanger woods lie over the fields and
    // under the trees, so the woods' crowns stand on them.
    this.scarpLayer = el('g', {}, ground);
    // Each wall's foot and brow, as distances from the window's rim.
    this.wallLines = null;
    // 1 on each cell of the window's columns where a crown stands in a crop
    // field away from a hedge, and is not drawn on farmland.
    this.cropMask = null;
    // The forest's blurred growth the trees were last sized by, so they can
    // be redrawn when the fields change.
    this.treeBlur = null;
    // Scroll bars: one group for each age band, with one path for each
    // ridge, so trimming a ridge repaints only the ground around it.
    this.scrollLayer = el('g', round, ground);
    this.scrollBands = Array.from({ length: SCROLL_BANDS }, (_, b) => {
      const age = (b + 0.5) / SCROLL_BANDS;
      return el('g', {
        stroke: mix(RIDGE, OLD_RIDGE, smoothstep(2 * age)),
        'stroke-opacity': (0.7 * (1 - smoothstep((age - 0.75) / 0.25))).toFixed(2),
      }, this.scrollLayer);
    });
    // Path element of each ridge drawn, by band and path data.
    this.ridges = new Map();
    this.scrollAt = null;
    // Trees lie over the scroll bars, which the canopy hides as it closes:
    // the shadows of every crown, then the crowns in a few tones, then
    // their highlights.
    this.treeLayer = el('g', {}, ground);
    this.treeKinds = [
      el('g', { fill: `rgb(${TREE_SHADOW})`, 'fill-opacity': 0.3 }, this.treeLayer),
      ...TREE_TONES.map((c) => el('g', { fill: `rgb(${c})`, 'fill-opacity': 0.9 }, this.treeLayer)),
      el('g', { fill: `rgb(${TREE_LIGHT})`, 'fill-opacity': 0.4 }, this.treeLayer),
    ];
    this.treeStrips = [];
    // Ground the channel has swept since the shrub, forest, fields and
    // crowns were last contoured, as its centrelines in the newest band's
    // sand, over all of them. Without it, ground the channel crosses and
    // leaves within one redraw shows the vegetation that stood there before
    // until the next redraw.
    this.sweptLayer = el('g', line, ground);
    // Natural levees: nested translucent silt regions, higher ground inside.
    this.leveeLayer = el('g', { fill: `rgb(${LEVEE})`, 'fill-rule': 'evenodd', 'fill-opacity': 0.35 }, ground);
    this.levees = LEVEE_LEVELS.map(() => el('path', {}, this.leveeLayer));
    this.leveeAt = null;
    // Every clay plug the channel has not swept away keeps a faint marsh,
    // so old meander belts stay legible after their scars fade.
    const plugOpacity = 1 - (1 - PLUG_MARSH) ** (1 / PLUG_LEVELS.length);
    const plugLayer = el('g', { fill: MARSH, 'fill-opacity': plugOpacity.toFixed(4), 'fill-rule': 'evenodd' }, ground);
    this.plugMarsh = PLUG_LEVELS.map(() => el('path', {}, plugLayer));
    this.plugAt = null;
    this.plugJob = null;
    this.plugBlur = null;
    // The farmsteads over all of it: yards, then mounds, the platforms of
    // ruins, and the roofs with their ridges.
    const farms = el('g', { 'stroke-linejoin': 'round' }, ground);
    this.yards = el('path', { fill: `rgb(${YARD_EARTH})`, 'fill-opacity': 0.55 }, farms);
    this.mounds = el('path', { fill: `rgb(${MOUND})`, 'fill-opacity': 0.5 }, farms);
    this.ruinLayer = el('g', { fill: `rgb(${PLATFORM})`, stroke: `rgb(${WALLING})` }, farms);
    this.thatch = el('path', { fill: `rgb(${THATCH})`, stroke: `rgb(${WALLING})` }, farms);
    this.tiles = el('path', { fill: `rgb(${ROOF})`, stroke: `rgb(${WALLING})` }, farms);
    this.ridges = el('path', { fill: 'none', stroke: `rgb(${WALLING})` }, farms);
    this.buildingsAt = null;
    // The bank lies under the lakes, so a lake that meets the channel joins
    // its water with no strip of bank between them.
    // Both are filled outlines at the local width, which a stroke cannot vary.
    // Every lake's reed fringe lies under the water of all of them, so lakes
    // that meet join as one body of water.
    // Tributary streams lie under the bank, so the channel hides where they
    // meet it. Mill leats lie over it, since their ends run along the water's
    // edge, which the bank would cover; the channel's water still hides
    // where they meet it.
    this.streamLayer = el('g', { ...round, stroke: `rgb(${WATER})` }, svg);
    this.streams = [];
    this.bank = el('g', { fill: `rgb(${WET_SAND})` }, svg);
    this.leatLayer = el('g', round, svg);
    this.reedLayer = el('g', { fill: 'none', 'stroke-linejoin': 'round', stroke: REEDS }, svg);
    this.lakeLayer = el('g', {}, svg);
    this.water = el('g', { fill: `rgb(${WATER})` }, svg);
    // Pools, riffles and mid-channel bars, over the channel's water.
    this.depth = [...POOL_COLOURS, SHALLOW, BAR].map((c) => ({ g: el('g', { fill: `rgb(${c})` }, svg), paths: [] }));
    // Over the water: the darker ponds above working weirs, the weirs and
    // the white water below them, the logs of the jams, and the mill houses.
    this.pondLayer = el('g', { ...line, stroke: `rgb(${CLEAR})`, 'stroke-opacity': 0.2, 'stroke-linecap': 'butt' }, svg);
    this.weirLayer = el('g', { fill: 'none' }, svg);
    this.logs = el('path', { ...round, stroke: `rgb(${LOG})`, 'stroke-opacity': 0.9 }, svg);
    this.houseLayer = el('g', {}, svg);
    this.millShapes = [];
    this.millsAt = null;
    this.bankRuns = [];
    this.waterRuns = [];
    this.lakes = [];
    this.reeds = [];
    this.scars = [];
    this.bands = [];
    // Centrelines kept since the last forest redraw, each with its time,
    // path data, tone and element in `sweptLayer` once drawn.
    this.swept = [];
    // Bands dropped since the last draw, still to be removed from the page.
    this.dropped = [];
    // Bar settings the old bands were last shaded for.
    this.shadedFor = null;
    this.lastBar = null;
    this.window = [0, 0];
  }

  // Show only the visible window; the hidden reaches fall outside it.
  reset(river, [w, h], seed) {
    const x = river.view_x();
    for (const s of this.layers) s.setAttribute('viewBox', `${x} 0 ${w} ${h}`);
    this.background.setAttribute('x', x);
    this.background.setAttribute('width', w);
    this.background.setAttribute('height', h);
    this.oldBars.replaceChildren();
    this.barLayer.replaceChildren();
    this.sweptLayer.replaceChildren();
    this.bands = [];
    this.swept = [];
    this.dropped = [];
    this.shadedFor = null;
    this.lastBar = null;
    this.window = [x, w];
    this.size = [w, h];
    this.seed = seed;
    this.leveeAt = null;
    this.coarseAt = null;
    this.millsAt = null;
    this.buildingsAt = null;
    this.wallsAt = { key: river.wall_version(), t: river.time() };
    this.drawPebbles(river.get_param('width'), seed);
    this.plugAt = null;
    this.plugJob = null;
    this.forestAt = null;
    this.forestJob = null;
    // The simulated time each staged layer's picture shows, and the time
    // each floodplain layer fell due, if it waits.
    [this.plugShown, this.fieldsShown, this.forestShown] = [null, null, null];
    this.dueSince = [];
    // The first draw after a reset draws the floodplain whole.
    this.fresh = true;
    this.scrollAt = null;
    this.fieldsAt = null;
    this.fieldJob = null;
    this.cropMask = null;
    this.meadow = new Map();
    for (const scar of this.scars) scar.shape = null;
    for (const lake of this.lakes) lake.inputs = null;
    this.drawSoils(river, x, w, h, seed);
    this.placeTrees(river, x, w, h, seed);
    this.drawWalls(river, x, w, h, seed);
  }

  // Draw the fields of every holding over the window, with their hedges,
  // and mark the cells where crowns stand in crops. `drawFloodplain` calls
  // it when a reorganisation has finished, at most every `FIELD_REFRESH`.
  // The redraw runs until the floodplain's deadline, `until`, and carries on
  // in the calls that follow; a redraw due while one runs waits for it.
  drawFields(river, memory) {
    if (!this.fieldJob) {
      this.fieldsAt = { key: river.land_version(), t: river.time() };
      this.fieldJob = this.fieldStages(river, memory);
    }
    if (this.fieldJob.next().done) {
      this.fieldJob = null;
      this.fieldsShown = this.fieldsAt.t;
    }
  }

  // The stages of a field redraw, which yield whenever the call's
  // `until` has passed. The page is written only in the last stage, so
  // no frame shows half a redraw.
  *fieldStages(river, memory) {
    const due = () => performance.now() > this.until;
    river.sync_holdings();
    // Copies, since later calls can grow memory and detach any view.
    const buf = memory.buffer;
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const owner = new Uint32Array(buf, river.land_owner_ptr(), cols * rows).slice();
    const risk = new Float32Array(buf, river.land_risk_ptr(), cols * rows).slice();
    const hold = new Float32Array(buf, river.holdings_ptr(), 6 * river.holding_slots()).slice();
    const w = river.get_param('width');
    const [c0, c1] = this.windowColumns(cols, cell);
    const W = c1 - c0;
    // Each holding's layout: its strips' width, phase and bend.
    const layouts = [];
    for (let k = 0; 6 * k < hold.length; k++) {
      if (!hold[6 * k]) {
        layouts.push(null);
        continue;
      }
      const seed = hold[6 * k + 5], rand = random(seed);
      const between = ([a, b]) => (a + (b - a) * rand()) * w;
      layouts.push({
        seed, x: hold[6 * k + 1], y: hold[6 * k + 2], ax: hold[6 * k + 3], ay: hold[6 * k + 4],
        strip: between(FIELD_STRIP), phase: rand(), wave: between(FIELD_WAVE) / (2 * Math.PI), bend: 2 * Math.PI * rand(),
      });
    }
    // Label each cell with its field, keyed by holding, strip and place
    // along the strip.
    const lab = new Int32Array(W * rows);
    const fields = [], index = new Map();
    for (let j = 0; j < rows; j++) {
      if (due()) yield;
      const y = (j + 0.5) * cell;
      for (let i = 0; i < W; i++) {
        const g = j * cols + c0 + i, o = owner[g];
        const L = layouts[o];
        if (!L) continue;
        const [dx, dy] = [(c0 + i + 0.5) * cell - L.x, y - L.y];
        const u = dx * L.ax + dy * L.ay;
        const v = dy * L.ax - dx * L.ay + FIELD_BEND * w * Math.sin(u / L.wave + L.bend);
        const k = Math.floor(v / L.strip + L.phase);
        const across = v / L.strip + L.phase - k;
        const len = (FIELD_LENGTH[0] + (FIELD_LENGTH[1] - FIELD_LENGTH[0]) * hash01(L.seed, k + 4096, 1)) * w;
        const lean = (2 * hash01(L.seed, k + 4096, 2) - 1) * FIELD_LEAN * L.strip;
        const m = Math.floor((u + lean * across) / len + hash01(L.seed, k + 4096, 3));
        const key = (o * 4096 + (k + 2048)) * 4096 + (m + 2048);
        let f = index.get(key);
        if (f === undefined) {
          f = fields.length + 1;
          index.set(key, f);
          fields.push({ key, o, cells: 0, risk: 0 });
        }
        lab[j * W + i] = f;
        fields[f - 1].cells++;
        fields[f - 1].risk += risk[g];
      }
    }
    // Fold slivers into a neighbouring field of the same holding.
    const small = MIN_FIELD * w * w / (cell * cell);
    for (let n = 0; n < lab.length; n++) {
      if (n % W === 0 && due()) yield;
      const f = lab[n];
      if (!f || fields[f - 1].cells >= small) continue;
      const [i, j] = [n % W, Math.floor(n / W)];
      for (const [di, dj] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        const [ni, nj] = [i + di, j + dj];
        if (ni < 0 || nj < 0 || ni >= W || nj >= rows) continue;
        const g = lab[nj * W + ni];
        if (g && g !== f && fields[g - 1].o === fields[f - 1].o && fields[g - 1].cells >= small) {
          lab[n] = g;
          break;
        }
      }
    }
    // Land use by mean flood risk, and whether crowns stand in the field.
    const meadowNow = new Map();
    for (const f of fields) {
      f.risk /= Math.max(1, f.cells);
      const u = hash01(f.key, 7);
      f.woods = u < WOODLAND;
      const was = this.meadow.get(f.key);
      const meadow = f.risk >= MEADOW_RISK + (was === undefined ? 0 : was ? -USE_MARGIN : USE_MARGIN);
      meadowNow.set(f.key, meadow);
      const uses = meadow ? MEADOWS : ARABLE;
      f.colour = f.woods ? FOREST.at(-1).colour : uses[Math.floor(hash01(f.key, 8) * uses.length)];
      f.opacity = f.woods ? WOODS_OPACITY : (CROP_OPACITY * (0.8 + 0.4 * hash01(f.key, 9))).toFixed(2);
    }
    const mask = new Uint8Array(W * rows);
    for (let n = 0; n < lab.length; n++) {
      mask[n] = lab[n] > 0 && !fields[lab[n] - 1].woods
        && this.scarpAt((c0 + n % W + 0.5) * cell, (Math.floor(n / W) + 0.5) * cell) !== 2;
    }
    // Trace, simplify and round each boundary once, so neighbouring fields
    // share it exactly.
    const arcs = yield* boundaryArcs(lab, W, rows, due);
    const outlines = new Map();
    let hedgeData = '';
    for (const arc of arcs) {
      if (due()) yield;
      const world = arc.pts.map((v, k) => (k % 2 ? v : c0 + v) * cell);
      const closed = arc.start === arc.end;
      let pts = simplify(world, FIELD_SIMPLIFY * cell);
      pts = chaikin(chaikin(pts, closed), closed);
      for (const [f, forward] of [[arc.a, true], [arc.b, false]]) {
        if (!f) continue;
        if (!outlines.has(f)) outlines.set(f, []);
        outlines.get(f).push(forward
          ? { start: arc.start, end: arc.end, pts }
          : { start: arc.end, end: arc.start, pts: reversePoints(pts) });
      }
      if (!arc.a || !arc.b) continue;
      const [fa, fb] = [fields[arc.a - 1], fields[arc.b - 1]];
      const pair = hash01(Math.min(fa.key, fb.key), Math.max(fa.key, fb.key));
      if (pair >= (fa.o === fb.o ? HEDGE_SIDE : HEDGE_EDGE)) continue;
      hedgeData += pointsData(pts);
      for (const n of arc.cells) mask[n] = 0;
    }
    // Chain each field's arcs into rings, end to start.
    const drawn = [];
    for (const [f, list] of outlines) {
      if (due()) yield;
      const from = new Map();
      for (const piece of list) {
        if (!from.has(piece.start)) from.set(piece.start, []);
        from.get(piece.start).push(piece);
      }
      let d = '';
      for (const first of list) {
        if (first.used) continue;
        const ring = [...first.pts];
        first.used = true;
        let at = first.end;
        while (at !== first.start) {
          const next = from.get(at)?.find((piece) => !piece.used);
          if (!next) break;
          next.used = true;
          for (let k = 2; k < next.pts.length; k++) ring.push(next.pts[k]);
          at = next.end;
        }
        if (ring.length >= 6) d += pointsData(ring) + 'Z';
      }
      drawn.push([fields[f - 1], d]);
    }
    const kept = new Map();
    for (const [field, d] of drawn) {
      const path = this.fieldPaths.get(field.key) ?? el('path', {}, this.parcels);
      this.fieldPaths.delete(field.key);
      kept.set(field.key, path);
      set(path, 'd', d);
      set(path, 'fill', `rgb(${field.colour})`);
      set(path, 'fill-opacity', field.opacity);
    }
    for (const path of this.fieldPaths.values()) path.remove();
    this.meadow = meadowNow;
    this.fieldPaths = kept;
    set(this.hedges, 'd', hedgeData);
    set(this.hedges, 'stroke-width', dp1(HEDGE_WIDTH * w));
    this.cropMask = { mask, c0, W };
    this.treesStale = true;
  }

  // Draw the valley sides over the window [x0, x0 + vw): the upland's tint
  // under its fields, and on each wall the hachures and the hanger woods,
  // with the mask that keeps fields out of the woods. The core narrows the
  // valley at the window edges to keep bends off them; the walls are drawn
  // at its full width, so that device does not show.
  drawWalls(river, x0, vw, h, seed) {
    this.uplandLayer.replaceChildren();
    this.scarpLayer.replaceChildren();
    this.wallLines = null;
    set(this.scarpMask, 'd', '');
    const w = river.get_param('width'), valley = river.get_param('valley_width') * w;
    if (!(valley > 0 && valley < h)) return;
    const rand = random(seed ^ 0xba1e);
    const step = 0.5 * w, n = Math.ceil(vw / step) + 1;
    const between = ([a, b], u) => a + (b - a) * u;
    const combeData = river.wall_combes();
    const sides = [];
    let maskData = '';
    for (const side of [-1, 1]) {
      const breadth = valueNoise(rand, vw, [12 * w, 4 * w]);
      const hanger = valueNoise(rand, vw, [HANGER.knots * w, 4 * w]);
      const combes = [];
      for (let k = 0; k < combeData.length; k += 4) {
        if (combeData[k] === (side < 0 ? 0 : 1)) combes.push({ x: combeData[k + 1], depth: combeData[k + 2], span: combeData[k + 3] });
      }
      const deepest = combes.reduce((m, c) => Math.max(m, c.depth), w);
      // Distances of the foot and brow from the window's top or bottom
      // edge, and how far the slope is wooded.
      const foot = new Float64Array(n), brow = new Float64Array(n), wood = new Float64Array(n);
      for (let k = 0; k < n; k++) {
        const x = x0 + Math.min(k * step, vw);
        // Depth the combes cut back at x, with the profile of `combe_cut`.
        const cut = (scale) => combes.reduce((m, c) => {
          const t = Math.abs(x - c.x) / (scale * c.span);
          return Math.max(m, scale * c.depth * (t < 1 ? (1 - t) ** 2 * (1 + 2 * t) : 0));
        }, 0);
        const at = river.wall_foot(side < 0 ? 0 : 1, x);
        foot[k] = side < 0 ? at : h - at;
        // The brow lies back from the foot with its combes filled in.
        const edge = foot[k] + cut(1);
        brow[k] = Math.min(foot[k] - 0.6 * SCARP[0] * w, edge - between(SCARP, breadth(x - x0)) * w - cut(COMBE_BROW));
        // Woods favour the steeper stretches and the combes.
        const steep = 1 - (foot[k] - brow[k]) / (SCARP[1] * w);
        wood[k] = smoothstep((hanger(x - x0) + 0.3 * steep + 0.5 * cut(1) / deepest - HANGER.threshold) / 0.1);
      }
      // Edge of the wood at point k towards the foot (1) or brow (-1).
      const woodEdge = (k, dir) => brow[k] + (0.5 + dir * 0.6 * wood[k]) * (foot[k] - brow[k]);
      const lines = { side, foot, brow, wood, woodEdge };
      sides.push(lines);
      const y = (d) => dp1(side < 0 ? d : h - d);
      const point = (k, d) => `${dp1(x0 + Math.min(k * step, vw))} ${y(d)}`;
      const trace = (line, offset = 0) => Array.from(line, (d, k) => point(k, d + offset)).join(' ');
      const rim = side < 0 ? 0 : h;
      el('path', { d: `M${dp1(x0)} ${rim} L${trace(foot)} L${dp1(x0 + vw)} ${rim}Z` }, this.uplandLayer);
      // Hanger woods, each tapering to a point at its ends, tinted like the
      // woods among the fields, which they hide.
      let woods = '';
      for (let k = 0; k < n;) {
        if (wood[k] <= 0) {
          k++;
          continue;
        }
        const k0 = Math.max(0, k - 1);
        while (k < n && wood[k] > 0) k++;
        const k1 = Math.min(n, k + 1);
        let d = '';
        for (let j = k0; j < k1; j++) d += `${j > k0 ? 'L' : 'M'}${point(j, woodEdge(j, 1))}`;
        for (let j = k1 - 1; j >= k0; j--) d += `L${point(j, woodEdge(j, -1))}`;
        woods += d + 'Z';
      }
      maskData += woods;
      el('path', { d: woods, fill: `rgb(${FOREST.at(-1).colour})`, 'fill-opacity': WOODS_OPACITY }, this.scarpLayer);
      const lit = side > 0;
      // Hachures, sorted into three weights by steepness.
      const strokes = [[], [], []];
      const at = (line, x) => {
        const t = Math.max(0, Math.min(n - 1, (x - x0) / step)), k = Math.min(n - 2, Math.floor(t));
        return line[k] + (line[k + 1] - line[k]) * (t - k);
      };
      const frac = (x, d) => (d - at(brow, x)) / (at(foot, x) - at(brow, x));
      const gap = HACHURE_GAP * w, e = 0.25 * w;
      for (const row of [0, 1]) {
        for (let x = x0 + (0.5 * row + rand() * 0.3) * gap; x < x0 + vw; x += gap * (0.85 + 0.3 * rand())) {
          const s0 = 0.5 * row + 0.03 + 0.06 * rand(), s1 = 0.5 * row + 0.41 + 0.06 * rand();
          let px = x, pd = at(brow, x) + s0 * (at(foot, x) - at(brow, x));
          const pts = [px, pd];
          for (let s = s0; s < s1; s += 0.08) {
            const gx = (frac(px + e, pd) - frac(px - e, pd)) / (2 * e);
            const gd = 1 / (at(foot, px) - at(brow, px));
            const g2 = gx * gx + gd * gd;
            px += 0.08 * gx / g2;
            pd += 0.08 * gd / g2;
            pts.push(px, pd);
          }
          const run = at(foot, x) - at(brow, x);
          const weight = Math.min(2, Math.floor(3 * clamp01((SCARP[1] * w - run) / ((SCARP[1] - SCARP[0]) * w))));
          let d = '';
          for (let k = 0; k < pts.length; k += 2) d += `${k ? 'L' : 'M'}${dp1(pts[k])} ${y(pts[k + 1])}`;
          strokes[weight].push(d);
        }
      }
      strokes.forEach((list, k) => el('path', {
        d: list.join(''), fill: 'none', 'stroke-linecap': 'round',
        stroke: `rgb(${SCARP_SHADE})`,
        'stroke-width': dp1(HACHURE_WIDTH * w * (0.6 + 0.4 * k)),
        'stroke-opacity': lit ? 0.38 : 0.5,
      }, this.scarpLayer));
    }
    set(this.scarpMask, 'd', maskData);
    this.wallLines = { x0, step, n, h, w, sides };
  }

  // 0 off the valley sides at world point (x, y), 1 on rough grass on
  // them and 2 in a hanger wood.
  scarpAt(x, y) {
    const L = this.wallLines;
    if (!L) return 0;
    const k = Math.max(0, Math.min(L.n - 1, Math.round((x - L.x0) / L.step)));
    for (const { side, foot, brow, wood, woodEdge } of L.sides) {
      const d = side < 0 ? y : L.h - y;
      if (wood[k] > 0 && d <= woodEdge(k, 1) && d >= woodEdge(k, -1)) return 2;
      if (d <= foot[k] && d >= brow[k]) return 1;
    }
    return 0;
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
    const w = river.get_param('width');
    // The floodplain layers sync their own data, which can grow memory, so
    // they go before the views below are built.
    const layers = [
      [this.oldBars, local.showBars], [this.barLayer, local.showBars], [this.sweptLayer, local.showBars],
      [this.leveeLayer, local.showLevees],
      [this.scrollLayer, local.showScrolls],
    ];
    for (const [layer, show] of layers) {
      if (layer.style.display !== (show ? '' : 'none')) layer.style.display = show ? '' : 'none';
    }
    this.drawFloodplain(river, memory, local, this.fresh ? Infinity : FLOODPLAIN_BUDGET);
    this.fresh = false;
    // The floodplain features come as copies, which can grow memory too.
    const [jams, mills, leats] = [river.jams(), river.mills(), river.mill_leats()];
    const [tribs, courses] = [river.tributaries(), river.tributary_courses()];

    river.sync();
    // Views must be rebuilt after every step: memory growth detaches them.
    const buf = memory.buffer;
    const n = river.oxbow_count();
    const line = new Float32Array(buf, river.line_ptr(), river.line_len());
    const oxPts = new Float32Array(buf, river.oxbow_line_ptr(), river.oxbow_line_len());
    const oxOff = new Uint32Array(buf, river.oxbow_offsets_ptr(), n + 1);
    const oxFill = new Float32Array(buf, river.oxbow_fills_ptr(), n);
    const oxScar = new Float32Array(buf, river.oxbow_scar_ages_ptr(), n);
    const oxLen = new Float32Array(buf, river.oxbow_lengths_ptr(), n);
    const oxAlong = new Float32Array(buf, river.oxbow_offsets_along_ptr(), n);
    const oxHeight = new Float32Array(buf, river.oxbow_heights_ptr(), oxPts.length / 2);
    const oxWidth = new Float32Array(buf, river.oxbow_widths_ptr(), oxPts.length / 2);
    const oxSilt = new Float32Array(buf, river.oxbow_silts_ptr(), oxPts.length / 2);
    const widths = new Float32Array(buf, river.widths_ptr(), line.length / 2);
    const scarLife = Math.max(1e-6, river.get_param('scar_lifetime'));
    const frame = channelFrame(line);
    const waterRuns = channelRuns(line, widths, frame, 0.5);
    const bend = smoothed(frame.curv);
    const bankRuns = channelRuns(line, widths, frame, BANK_EDGE, barRim(bend, w));
    // Channel points, to find lake points beside it.
    const reach = (MAX_WIDTH + ALONGSIDE_GAP) * w;
    const channel = new Grid(line, reach);

    this.record(line, river.time(), w, local);
    this.drawBars(river.time(), w, local);

    // Oxbows: a lake that clears and darkens once cut off, plugs from the
    // ends, narrows far more slowly from the banks and turns murky, fading
    // only over the last fifth of its fill, then a marshy scar that fades
    // out. Both plugs and banks advance steadily as it fills, and the water
    // drains first from where the land is high or the most silt has settled, splitting the lake into pools. The reed fringe grows inwards, so
    // it does not hide the narrowing.
    while (this.scars.length < n) {
      const g = el('g', {}, this.scarLayer);
      const [damp, wet, tufts] = SCAR_OPACITY.map(() => el('path', {}, g));
      set(damp, 'fill', `rgb(${SCAR_DAMP})`);
      set(wet, 'fill', `rgb(${SCAR_WET})`);
      for (const [k, v] of Object.entries({ fill: 'none', stroke: `rgb(${SCAR_TUFT})`, 'stroke-linecap': 'round' })) set(tufts, k, v);
      this.scars.push({ g, damp, wet, tufts, shape: null });
    }
    while (this.scars.length > n) this.scars.pop().g.remove();
    resize(this.lakes, n, this.lakeLayer);
    resize(this.reeds, n, this.reedLayer);
    for (let k = 0; k < n; k++) {
      const age = oxFill[k];
      const scar = this.scars[k], lake = this.lakes[k], reed = this.reeds[k];
      // The channel splits an oxbow into pieces of its original polyline,
      // so a piece's ends and length identify its shape.
      const [s0, s1] = [2 * oxOff[k], 2 * oxOff[k + 1]];
      const shape = `${s1 - s0} ${oxPts[s0]} ${oxPts[s0 + 1]} ${oxPts[s1 - 2]} ${oxPts[s1 - 1]}`;
      // Mean width of the channel the oxbow was cut from.
      let wm = 0;
      for (let i = oxOff[k]; i < oxOff[k + 1]; i++) wm += oxWidth[i];
      wm *= w / Math.max(1, oxOff[k + 1] - oxOff[k]);
      const sliver = pieceLength(oxPts, oxOff[k], oxOff[k + 1]) < MIN_LAKE * w;
      // The scar shows as the lake fills over it, and its wet core and
      // tufts dry out before the damp ground does.
      const shown = sliver ? 0 : smoothstep(age);
      const fades = [1, SCAR_WET_LIFE, SCAR_WET_LIFE].map((f, m) => (
        shown * SCAR_OPACITY[m] * (1 - smoothstep(oxScar[k] / (f * scarLife)))).toFixed(2));
      const scarShape = `${shape} ${w} ${local.oxbowShrink}`;
      if (scar.shape !== scarShape && shown > 0) {
        scar.shape = scarShape;
        const [i0, i1, along, total] = [oxOff[k], oxOff[k + 1], oxAlong[k], oxLen[k]];
        const core = 0.5 * (1 - local.oxbowShrink);
        set(scar.damp, 'd', scarData(oxPts, oxWidth, i0, i1, along, total, SCAR_DAMP_HALF, w));
        set(scar.wet, 'd', scarData(oxPts, oxWidth, i0, i1, along, total, core, w));
        set(scar.tufts, 'd', tuftData(oxPts, oxWidth, i0, i1, along, total, 0.7 * SCAR_DAMP_HALF, w));
        set(scar.tufts, 'stroke-width', dp1(0.06 * w));
      }
      set(scar.damp, 'fill-opacity', fades[0]);
      set(scar.wet, 'fill-opacity', fades[1]);
      set(scar.tufts, 'stroke-opacity', fades[2]);
      if (age >= 1 || sliver) {
        // Elements are matched to oxbows by index, so a lake can come back
        // to this element when the channel splits or drops an oxbow before
        // it. Forget its inputs, or it would stay blank.
        set(lake, 'd', '');
        set(reed, 'd', '');
        lake.inputs = null;
        continue;
      }
      const reeds = 0.1 * wm + 0.15 * wm * age;
      // Half-width where the bed is at its mean height.
      const hw = 0.5 * wm * (1 - local.oxbowShrink * age) - 0.5 * reeds;
      // The plugs advance from the ends of the oxbow as cut off, which a
      // piece split off by the channel may lack.
      const plug = PLUG * age * oxLen[k];
      const [from, to] = [plug - oxAlong[k], oxLen[k] - plug - oxAlong[k]];
      const level = hw > 0 ? (hw / (0.5 * wm)) ** 2 : 0;
      // The water covers the inner half of the fringe, which is stroked at
      // twice its width, so the outline lies half a fringe inside `hw`.
      const inset = hw > 0 ? Math.max(0, 1 - 0.5 * reeds / hw) : 0;
      // The silt and the channel change without reshaping the oxbow.
      let silt = 0;
      for (let i = oxOff[k]; i < oxOff[k + 1]; i++) silt += oxSilt[i];
      const { dry, key } = alongside(oxPts, oxWidth, oxOff[k], oxOff[k + 1], line, widths, channel, w);
      const inputs = `${shape} ${Math.round(age / LAKE_STEP)} ${silt} ${w} ${local.oxbowShrink} ${local.oxbowRelief} ${key}`;
      if (lake.inputs !== inputs) {
        lake.inputs = inputs;
        const d = lakeData(oxPts, oxHeight, oxSilt, oxWidth, oxOff[k], oxOff[k + 1], from, to,
          0.5 * w * inset, level, local.oxbowRelief, age, dry);
        set(lake, 'd', d);
        set(reed, 'd', d);
      }
      const opacity = clamp01(5 * (1 - age)).toFixed(2);
      const clear = mixRgb(WATER, CLEAR, smoothstep(age / CLEARING));
      set(lake, 'fill', mix(clear, MURKY, smoothstep(1.5 * (age - CLEARING) / (1 - CLEARING))));
      set(lake, 'opacity', opacity);
      set(reed, 'opacity', opacity);
      set(reed, 'stroke-width', (2 * reeds).toFixed(2));
    }

    resize(this.bankRuns, bankRuns.length, this.bank);
    resize(this.waterRuns, waterRuns.length, this.water);
    bankRuns.forEach((d, k) => set(this.bankRuns[k], 'd', d));
    waterRuns.forEach((d, k) => set(this.waterRuns[k], 'd', d));
    channelDepth(line, widths, frame, bend, w).forEach((runs, m) => {
      const { g, paths } = this.depth[m];
      resize(paths, runs.length, g);
      runs.forEach((d, k) => set(paths[k], 'd', d));
    });
    this.drawStreams(tribs, courses, w);
    this.drawMills(river, mills, leats, line, widths, w);
    this.drawJams(jams, w);
  }

  // Each tributary runs down its combe to the apex of its fan, then along
  // the course the core keeps for it across the floodplain to the water it
  // meets. The course changes only at its lower end, as water meets or
  // leaves it, and when it moves on its fan, so a stream is redrawn only
  // then.
  drawStreams(tribs, courses, w) {
    const n = tribs.length / 8;
    resize(this.streams, n, this.streamLayer);
    for (let k = 0; k < n; k++) {
      const [side, mx, my, start, len, version, scale, depth] = tribs.subarray(8 * k, 8 * k + 8);
      const path = this.streams[k];
      const key = `${version} ${w}`;
      if (path.key === key) continue;
      path.key = key;
      // From the head of the combe, back in the wall, down the course,
      // winding in bends set by the distance from the mouth, so the bends
      // above stay put as the lower end changes. They grow over the first
      // bend from the mouth.
      const pts = [mx, my + (side ? 1 : -1) * 0.9 * depth];
      const rand = random(Math.round(10 * mx) ^ 0x7c1b);
      const [phase, drift] = [2 * Math.PI * rand(), 2 * Math.PI * rand()];
      let [along, angle] = [0, phase];
      for (let i = start; i < start + len; i++) {
        const [x, y] = [courses[2 * i], courses[2 * i + 1]];
        const [a, b] = [Math.max(start, i - 1), Math.min(start + len - 1, i + 1)];
        const [tx, ty] = [courses[2 * b] - courses[2 * a], courses[2 * b + 1] - courses[2 * a + 1]];
        const t = Math.hypot(tx, ty) || 1;
        if (i > start) {
          const step = Math.hypot(x - courses[2 * i - 2], y - courses[2 * i - 1]);
          along += step;
          const bend = STREAM_BEND * w * (1 + 0.5 * Math.sin(drift + along / (5 * STREAM_BEND * w)));
          angle += 2 * Math.PI * step / bend;
        }
        const off = STREAM_SWING * w * Math.min(1, along / (STREAM_BEND * w)) * Math.sin(angle);
        pts.push(x - off * ty / t, y + off * tx / t);
      }
      set(path, 'd', pointsData(chaikin(chaikin(pts, false), false)));
      set(path, 'stroke-width', dp1(STREAM_WIDTH * w * (0.5 + 0.5 * scale)));
    }
  }

  // Each jam is a few logs lodged at its bank and leaning downstream into
  // the water, seeded by the jam, so its logs keep their places and the
  // last ones go first as it breaks up.
  drawJams(jams, w) {
    let d = '';
    for (let k = 0; k < jams.length; k += 8) {
      const [x, y, dx, dy, ix, iy, strength, until] = jams.subarray(k, k + 8);
      const rand = random(Math.round(1000 * until) ^ 0x10c5);
      const logs = Math.round(1 + 3 * strength);
      for (let m = 0; m < logs; m++) {
        const along = (rand() - 0.5) * 1.2 * w, lean = 0.25 + 0.6 * rand();
        const length = (LOG_LENGTH[0] + (LOG_LENGTH[1] - LOG_LENGTH[0]) * rand()) * w;
        const [ux, uy] = [Math.cos(lean) * dx + Math.sin(lean) * ix, Math.cos(lean) * dy + Math.sin(lean) * iy];
        const [sx, sy] = [x + along * dx - 0.2 * w * ix, y + along * dy - 0.2 * w * iy];
        d += `M${dp1(sx)} ${dp1(sy)}L${dp1(sx + length * ux)} ${dp1(sy + length * uy)}`;
      }
    }
    set(this.logs, 'd', d);
    set(this.logs, 'stroke-width', dp1(0.12 * w));
  }

  // Each mill: its leat, the weir across the channel with white water below
  // it, the house on the leat beside the weir, and the darker pond above a
  // working weir. A ruin's weir is broken, its leat runs dry and its house
  // stands roofless, all fading over the 500 years the core keeps it. The
  // mills are rebuilt only when one is founded or ruined, but a working
  // leat is laid each frame, since its ends follow the bank.
  drawMills(river, mills, leats, line, widths, w) {
    const version = river.mill_version(), n = mills.length / 11;
    if (this.millsAt !== version || this.millShapes.length !== n) {
      this.millsAt = version;
      for (const shape of this.millShapes) for (const e of shape.parts) e.remove();
      this.millShapes = Array.from({ length: n }, (_, k) => this.millShape(mills.subarray(11 * k, 11 * k + 11), leats, w));
    }
    const pond = Math.round(river.get_param('pond_length') / SPACING), last = line.length / 2 - 1;
    for (let k = 0; k < n; k++) {
      const [x, y, ax, ay, working, ruin, at, width, , start, count] = mills.subarray(11 * k, 11 * k + 11);
      const shape = this.millShapes[k];
      if (working) set(shape.water, 'd', leatData(leats, start, count));
      const opacity = (1 - smoothstep(ruin)).toFixed(2);
      for (const e of shape.parts) set(e, 'opacity', opacity);
      const i = Math.round(at);
      if (!working || i < 1 || i >= last) {
        set(shape.pond, 'd', '');
        set(shape.foam, 'd', '');
        continue;
      }
      const i0 = Math.max(0, i - pond);
      let mean = 0;
      for (let j = i0; j <= i; j++) mean += widths[j];
      set(shape.pond, 'd', pathData(line, i0, i + 1));
      set(shape.pond, 'stroke-width', dp1(0.8 * mean / (i - i0 + 1)));
      // The channel turns about the weir after it is built, so the weir is
      // laid across the channel as it now runs, with white water a quarter
      // of a width below it.
      let [tx, ty] = [line[2 * i + 2] - line[2 * i - 2], line[2 * i + 3] - line[2 * i - 1]];
      const t = Math.hypot(tx, ty) || 1;
      [tx, ty] = [tx / t, ty / t];
      const [nx, ny] = ty * ax - tx * ay >= 0 ? [ty, -tx] : [-ty, tx];
      const across = (reach, along) => {
        const [cx, cy] = [x + along * tx, y + along * ty];
        return `M${dp1(cx - reach * nx)} ${dp1(cy - reach * ny)}L${dp1(cx + reach * nx)} ${dp1(cy + reach * ny)}`;
      };
      set(shape.weir, 'd', across(0.5 * width + 0.2 * w, 0));
      set(shape.foam, 'd', across(0.5 * width, 0.25 * w));
    }
  }

  // The parts of one mill, from its values in `River::mills` and the leats.
  millShape(m, leats, w) {
    const [x, y, ax, ay, working, , , width, , start, count] = m;
    const leat = leats.subarray(2 * start, 2 * (start + count));
    const dash = { 'stroke-dasharray': `${dp1(0.4 * w)} ${dp1(0.3 * w)}` };
    const water = el('path', {
      d: leatData(leats, start, count), 'stroke-width': dp1(0.22 * w), stroke: `rgb(${working ? WATER : DRY_LEAT})`,
      ...(working ? {} : dash),
    }, this.leatLayer);
    // A ruin's weir lies where it was built; a working one is laid each
    // frame by `drawMills`.
    const reach = 0.5 * width + 0.2 * w;
    const weir = el('path', {
      d: working ? '' : `M${dp1(x - reach * ax)} ${dp1(y - reach * ay)}L${dp1(x + reach * ax)} ${dp1(y + reach * ay)}`,
      stroke: `rgb(${STONE})`, 'stroke-width': dp1(0.25 * w), ...(working ? {} : dash),
    }, this.weirLayer);
    const foam = el('path', { stroke: `rgb(${FOAM})`, 'stroke-width': dp1(0.1 * w), 'stroke-opacity': 0.6 }, this.weirLayer);
    // The house stands on the leat beside the weir, along it.
    const q = Math.max(0, count - 1 - LEAT_TAIL);
    const [a, b] = [Math.max(0, q - 1), Math.min(count - 1, q + 1)];
    let [tx, ty] = [leat[2 * b] - leat[2 * a], leat[2 * b + 1] - leat[2 * a + 1]];
    const t = Math.hypot(tx, ty) || 1;
    [tx, ty] = [tx / t, ty / t];
    const [hl, hd] = [0.5 * HOUSE[0] * w, 0.5 * HOUSE[1] * w];
    const [cx, cy] = [leat[2 * q], leat[2 * q + 1]];
    const corner = (s, r) => `${dp1(cx + s * hl * tx + r * hd * ty)} ${dp1(cy + s * hl * ty - r * hd * tx)}`;
    const house = el('path', {
      d: `M${corner(-1, -1)}L${corner(1, -1)}L${corner(1, 1)}L${corner(-1, 1)}Z`
        + (working ? `M${corner(-1, 0)}L${corner(1, 0)}` : ''),
      fill: working ? `rgb(${ROOF})` : 'none', stroke: `rgb(${WALLING})`, 'stroke-width': dp1(0.06 * w),
      'stroke-linejoin': 'round', ...(working ? {} : dash),
    }, this.houseLayer);
    const pond = el('path', {}, this.pondLayer);
    return { parts: [water, weir, foam, house, pond], water, weir, pond, foam };
  }

  // Draw the farmsteads when a building is raised, abandoned or lost, and
  // every `BUILDING_REFRESH` of simulated time as the platforms fade. Each
  // farmstead's yard is the rounded hull of its buildings but its field
  // barn.
  drawBuildings(river, w) {
    this.buildingsAt = { key: river.building_version(), t: river.time() };
    const data = river.buildings();
    const [x0, vw] = this.window;
    const yards = new Map();
    let [mounds, thatch, tiles, ridges] = ['', '', '', ''];
    const ruins = [];
    for (let k = 0; k < data.length; k += 8) {
      const [cx, cy, ux, uy, kind, mound, age, holding] = data.subarray(k, k + 8);
      if (cx < x0 - 4 * w || cx > x0 + vw + 4 * w) continue;
      const [hl, hd] = [0.5 * FOOTPRINTS[kind][0] * w, 0.5 * FOOTPRINTS[kind][1] * w];
      const corners = (grow) => [[-1, -1], [1, -1], [1, 1], [-1, 1]].flatMap(([s, r]) => [
        cx + s * (hl + grow) * ux + r * (hd + grow) * uy, cy + s * (hl + grow) * uy - r * (hd + grow) * ux,
      ]);
      const box = corners(0);
      const outline = pointsData(box) + 'Z';
      if (mound) {
        const [rx, ry, deg] = [hl + MOUND_RIM * w, hd + MOUND_RIM * w, (Math.atan2(uy, ux) * 180) / Math.PI];
        const arc = `A${dp1(rx)} ${dp1(ry)} ${deg.toFixed(1)} 1 0`;
        const [ax, ay, bx, by] = [cx - rx * ux, cy - rx * uy, cx + rx * ux, cy + rx * uy];
        mounds += `M${dp1(ax)} ${dp1(ay)}${arc} ${dp1(bx)} ${dp1(by)}${arc} ${dp1(ax)} ${dp1(ay)}Z`;
      }
      if (age > 0) {
        ruins.push([outline, 1 - age]);
        continue;
      }
      if (kind === 0 || kind === 5) tiles += outline;
      else thatch += outline;
      if (kind !== 1) ridges += `M${dp1(cx - hl * ux)} ${dp1(cy - hl * uy)}L${dp1(cx + hl * ux)} ${dp1(cy + hl * uy)}`;
      if (kind !== 6) {
        if (!yards.has(holding)) yards.set(holding, []);
        yards.get(holding).push(...corners(YARD_RIM * w));
      }
    }
    let yardData = '';
    for (const pts of yards.values()) {
      if (pts.length < 16) continue;
      const ring = hull(pts);
      yardData += pointsData(chaikin(chaikin([...ring, ring[0], ring[1]], true), true)) + 'Z';
    }
    const stroke = dp1(0.06 * w);
    set(this.yards, 'd', yardData);
    set(this.mounds, 'd', mounds);
    set(this.thatch, 'd', thatch);
    set(this.tiles, 'd', tiles);
    set(this.ridges, 'd', ridges);
    for (const p of [this.thatch, this.tiles, this.ridges]) set(p, 'stroke-width', stroke);
    this.ruinLayer.replaceChildren();
    for (const [d, fade] of ruins) {
      el('path', {
        d, 'fill-opacity': (0.3 * fade).toFixed(2), 'stroke-opacity': (0.6 * fade).toFixed(2), 'stroke-width': stroke,
        'stroke-dasharray': `${dp1(0.2 * w)} ${dp1(0.15 * w)}`,
      }, this.ruinLayer);
    }
  }

  // A pebble pattern for the coarse ground, scaled to the channel width.
  drawPebbles(w, seed) {
    const rand = random(seed ^ 0x9eb1);
    const size = 1.6 * w;
    set(this.pebbles, 'width', dp1(size));
    set(this.pebbles, 'height', dp1(size));
    this.pebbles.replaceChildren();
    for (let k = 0; k < 5; k++) {
      el('ellipse', {
        cx: dp1(rand() * size), cy: dp1(rand() * size), rx: dp1((0.06 + 0.06 * rand()) * w), ry: dp1((0.05 + 0.04 * rand()) * w),
        fill: `rgb(${PEBBLE})`, 'fill-opacity': 0.6,
      }, this.pebbles);
    }
  }

  // Contour the coarse ground over the window. `drawFloodplain` calls it
  // every `COARSE_REFRESH` of simulated time.
  drawCoarse(river, memory) {
    this.coarseAt = { key: null, t: river.time() };
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const coarse = new Float32Array(memory.buffer, river.coarse_ptr(), cols * rows);
    const [c0, c1] = this.windowColumns(cols, cell);
    COARSE_LEVELS.forEach((level, k) => set(this.coarse[k], 'd', contourData(coarse, cols, rows, cell, c0, c1, level)));
  }

  // Redraw the valley walls after their bluffs have slumped, at most every
  // `WALL_REFRESH` of simulated time.
  redrawWalls(river) {
    this.wallsAt = { key: river.wall_version(), t: river.time() };
    const [x, vw] = this.window;
    this.drawWalls(river, x, vw, this.size[1], this.seed);
  }

  // The floodplain layers. Each has the simulated time its picture shows,
  // whether a redraw is due or under way, and a call that draws it or
  // carries its redraw on.
  floodplainLayers(river, memory, local) {
    const t = river.time(), w = river.get_param('width');
    return [
      {
        shown: this.leveeAt?.t,
        due: () => local.showLevees && isDue(this.leveeAt, null, t, LEVEE_REFRESH),
        run: () => this.drawLevees(river, memory),
      },
      {
        shown: this.plugShown,
        due: () => this.plugJob || isDue(this.plugAt, river.cutoffs(), t, LEVEE_REFRESH),
        run: () => this.drawPlugs(river, memory),
      },
      {
        shown: this.fieldsShown,
        due: () => this.fieldJob || isDue(this.fieldsAt, river.land_version(), t, FIELD_REFRESH, true),
        run: () => this.drawFields(river, memory),
      },
      {
        shown: this.forestShown,
        due: () => this.forestJob || isDue(this.forestAt, this.forestKey(river), t, LEVEE_REFRESH),
        run: () => this.drawForest(river, memory),
      },
      // The crowns go stale when fields finish after the forest sized them.
      {
        shown: this.fieldsShown,
        due: () => this.treesStale && this.treeBlur,
        run: () => this.redrawTrees(),
      },
      {
        shown: this.scrollAt?.t,
        due: () => local.showScrolls && isDue(this.scrollAt, river.scroll_version(), t, LEVEE_REFRESH),
        run: () => this.drawScrolls(river, memory, w),
      },
      {
        shown: this.coarseAt?.t,
        due: () => isDue(this.coarseAt, null, t, COARSE_REFRESH),
        run: () => this.drawCoarse(river, memory),
      },
      {
        shown: this.buildingsAt?.t,
        due: () => isDue(this.buildingsAt, river.building_version(), t, BUILDING_REFRESH),
        run: () => this.drawBuildings(river, w),
      },
      {
        shown: this.wallsAt?.t,
        due: () => isDue(this.wallsAt, river.wall_version(), t, WALL_REFRESH, true),
        run: () => this.redrawWalls(river),
      },
    ];
  }

  // Refresh the floodplain layers that are due, for `budget` ms a call.
  // Their refreshes fall due together, on the `LEVEE_REFRESH` clock and
  // after cutoffs, and stacked in one frame they overran it; what does not
  // fit carries on in the calls that follow. A layer starts only within the
  // budget, and a staged redraw stops at the first stage past it, so a call
  // overruns by at most one stage. The layer whose picture is oldest goes
  // first, so a redraw under way carries on in consecutive frames and no
  // layer waits on another that falls due more often. Returns whether any
  // layer did work.
  drawFloodplain(river, memory, local, budget = FLOODPLAIN_BUDGET) {
    this.until = performance.now() + budget;
    const layers = this.floodplainLayers(river, memory, local);
    const shown = (layer) => layer.shown ?? -Infinity;
    const order = [...layers].sort((a, b) => (shown(a) - shown(b)) || 0);
    let worked = false;
    for (const layer of order) {
      if (performance.now() > this.until) break;
      if (!layer.due()) continue;
      layer.run();
      worked = true;
    }
    const t = river.time();
    layers.forEach((layer, k) => {
      this.dueSince[k] = layer.due() ? (this.dueSince[k] ?? t) : null;
    });
    return worked;
  }

  // Simulated time the longest-waiting floodplain redraw has been due.
  behind(river) {
    const t = river.time();
    return Math.max(0, ...this.dueSince.map((since) => since === null ? 0 : t - since));
  }

  // Contour the overbank deposits over the window every `LEVEE_REFRESH` of
  // simulated time, as they build up and the channel sweeps them away.
  drawLevees(river, memory) {
    this.leveeAt = { key: null, t: river.time() };
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const deposit = new Float32Array(memory.buffer, river.deposit_ptr(), cols * rows);
    const [x, vw] = this.window;
    const c0 = Math.max(0, Math.floor(x / cell) - 1);
    const c1 = Math.min(cols, Math.ceil((x + vw) / cell) + 1);
    const top = river.get_param('levee_height');
    LEVEE_LEVELS.forEach((f, k) => {
      this.levees[k].setAttribute('d', top > 0 ? contourData(deposit, cols, rows, cell, c0, c1, f * top) : '');
    });
  }

  // Draw the clay plug marsh after each cutoff, and every `LEVEE_REFRESH`
  // of simulated time as it fades and the channel sweeps it away. Each
  // plug cell is weighted by its age, then blurred over 3 × 3 cells, so
  // its outline follows the lake's curve instead of the cell edges. The
  // redraw contours one of `PLUG_LEVELS` a stage, and runs stages until the
  // floodplain's deadline, as `drawForest` does.
  drawPlugs(river, memory) {
    if (!this.plugJob) {
      this.plugAt = { key: river.cutoffs(), t: river.time() };
      this.plugJob = this.plugStages(river, memory);
    }
    do {
      if (this.plugJob.next().done) {
        this.plugJob = null;
        this.plugShown = this.plugAt.t;
      }
    } while (this.plugJob && performance.now() <= this.until);
  }

  // The stages of a plug marsh redraw: weight and blur the plug grid, then
  // contour each level. The first stage reads WASM memory, so it runs in
  // the call that starts the redraw.
  *plugStages(river, memory) {
    river.sync_plugs();
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const plugs = new Float32Array(memory.buffer, river.plugs_ptr(), cols * rows);
    const [c0, c1] = this.windowColumns(cols, cell);
    const [fill, scar] = [river.get_param('oxbow_lifetime'), Math.max(1e-6, river.get_param('scar_lifetime'))];
    if (this.plugWeight?.length !== cols * rows) this.plugWeight = new Float32Array(cols * rows);
    const weight = this.plugWeight;
    for (let i = 0; i < weight.length; i++) {
      const age = plugs[i];
      weight[i] = age < 0 ? 0 : PLUG_RESIDUAL + (1 - PLUG_RESIDUAL) * (1 - smoothstep((age - fill) / scar));
    }
    // A buffer of its own, since the forest blurs between stages.
    const blur = this.blur(weight, cols, rows, c0, c1, 'plugBlur');
    for (const [k, level] of PLUG_LEVELS.entries()) {
      set(this.plugMarsh[k], 'd', contourData(blur, cols, rows, cell, c0, c1, level));
      if (k < PLUG_LEVELS.length - 1) yield;
    }
  }

  // Redraw the shrub and forest every `LEVEE_REFRESH` of simulated time, and
  // after each cutoff, which clears an oxbow's footprint. The
  // redraw runs stages of `forestStages` until the floodplain's deadline,
  // `until`, at least one a call, so its contours spread over the frames
  // that follow instead of stalling one; a redraw due while one runs waits
  // for it.
  drawForest(river, memory) {
    if (!this.forestJob) {
      this.forestAt = { key: this.forestKey(river), t: river.time() };
      this.forestJob = this.forestStages(river, memory);
    }
    do {
      if (this.forestJob.next().done) {
        this.forestJob = null;
        this.forestShown = this.forestAt.t;
      }
    } while (this.forestJob && performance.now() <= this.until);
  }

  // The events after which the forest is redrawn: cutoffs.
  forestKey(river) {
    return river.cutoffs();
  }

  // The stages of a forest redraw: copy and blur the age grid like the plug
  // grid, contour each forest level, then the farmland clip, then size the
  // tree crowns. The first stage reads WASM memory, so it runs in the call
  // that starts the redraw. The contours are kept until the last stage,
  // which puts them on the page with the crowns, so the vegetation changes
  // in one frame, not in a cascade of partial redraws, and drops the swept
  // ground the new contours have caught up with.
  *forestStages(river, memory) {
    river.sync_forest();
    const synced = river.time();
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const age = new Float32Array(memory.buffer, river.forest_ptr(), cols * rows);
    const [c0, c1] = this.windowColumns(cols, cell);
    // A buffer of its own, since the plug marsh blurs between stages.
    const blur = this.blur(age, cols, rows, c0, c1, 'forestBlur');
    const [shrub, forest] = [river.get_param('vegetation_time'), river.get_param('forest_time')];
    const w = river.get_param('width');
    const forestData = [];
    for (const [k, { at }] of FOREST.entries()) {
      yield;
      const age = shrub + at * forest;
      const start = 0.5 * (age + (k ? shrub + FOREST[k - 1].at * forest : 0));
      for (let s = 0; s < FOREST_STEPS; s++) {
        const level = start + (age - start) * (s + 1) / FOREST_STEPS;
        forestData.push(contourData(blur, cols, rows, cell, c0, c1, level));
      }
    }
    const graze = river.get_param('grazing_time');
    yield;
    const farmData = this.farmland.map((_, k) => {
      const level = graze + (shrub - graze) * k / FIELD_FADE;
      return contourData(blur, cols, rows, cell, c0, c1, level);
    });
    yield;
    const treeArgs = [cols, rows, cell, shrub, forest, w, graze];
    const [trees, crop] = [this.treeData(blur, ...treeArgs), this.cropMask];
    yield;
    forestData.forEach((d, k) => set(this.forest[k], 'd', d));
    farmData.forEach((d, k) => set(this.farmland[k], 'd', d));
    this.setTrees(trees);
    if (this.treeBlur?.length !== blur.length) this.treeBlur = new Float32Array(blur.length);
    this.treeBlur.set(blur);
    this.treeArgs = treeArgs;
    // Fields finished since the crowns were sized leave them stale, for
    // `redrawTrees` to size again.
    if (this.cropMask === crop) this.treesStale = false;
    // The grid stamps the channel as it lay at the sync, so centrelines
    // kept up to then are now bare in the contours.
    const keep = [];
    for (const s of this.swept) {
      if (s.t > synced) keep.push(s);
      else s.e?.remove();
    }
    this.swept = keep;
  }

  // Redraw the crowns from the last forest redraw's growth, after new
  // fields change which crowns stand in crops.
  redrawTrees() {
    this.drawTrees(this.treeBlur, ...this.treeArgs);
    this.treesStale = false;
  }

  // Carry on with work spread over frames, on a frame that draws nothing.
  idle(river, memory, { local }) {
    this.drawFloodplain(river, memory, local, FLOODPLAIN_IDLE_BUDGET);
  }

  // Bring the floodplain up to date in one call, when the river stops. A
  // pass finishes the redraws under way, which can leave their layers due
  // again, so it repeats until a pass finds nothing to do.
  settle(river, memory, { local }) {
    for (let k = 0; k < 3 && this.drawFloodplain(river, memory, local, Infinity); k++);
  }

  // Scatter crowns over the window on a jittered grid, each with its own
  // threshold, size and tone, and sort them into strips.
  placeTrees(river, x0, vw, h, seed) {
    for (const g of this.treeKinds) g.replaceChildren();
    const rand = random(seed ^ 0x7ee5);
    const w = river.get_param('width');
    const [gap, strip] = [TREE_GAP * w, TREE_STRIP * w];
    const count = Math.ceil(vw / strip);
    this.treeStrips = Array.from({ length: count }, () => ({
      crowns: [],
      paths: this.treeKinds.map((g) => el('path', {}, g)),
    }));
    for (let y = gap / 2; y < h; y += gap) {
      for (let x = x0 + gap / 2; x < x0 + vw; x += gap) {
        const cx = x + (rand() - 0.5) * gap, cy = y + (rand() - 0.5) * gap;
        const s = Math.min(count - 1, Math.max(0, Math.floor((cx - x0) / strip)));
        const tone = Math.floor(rand() * TREE_TONES.length);
        // Lobes as offsets and radii in multiples of the crown's radius.
        const turn = 2 * Math.PI * rand();
        const lobes = [[0, 0, 0.7]];
        for (let k = 0; k < TREE_LOBES; k++) {
          const a = turn + 2 * Math.PI * (k + 0.3 * rand()) / TREE_LOBES, off = 0.35 + 0.15 * rand();
          lobes.push([off * Math.cos(a), off * Math.sin(a), 0.5 + 0.2 * rand()]);
        }
        this.treeStrips[s].crowns.push({ x: cx, y: cy, at: rand(), size: 0.6 + 0.4 * rand(), tone, lobes });
      }
    }
  }

  // Size each crown by the blurred growth of the cell under it, and
  // rebuild the paths of each strip.
  drawTrees(...args) {
    this.setTrees(this.treeData(...args));
  }

  // Put path data from `treeData` on the page.
  setTrees(data) {
    this.treeStrips.forEach(({ paths }, s) => paths.forEach((p, k) => set(p, 'd', data[s][k])));
  }

  // Path data of the crowns of each strip, one string for each of its
  // paths, without touching the page.
  treeData(blur, cols, rows, cell, shrub, forest, w, farm) {
    const [from, to] = [shrub + FOREST[1].at * forest, shrub + FOREST[2].at * forest];
    const grow = Math.max(1e-6, TREE_GROW * forest);
    const light = this.treeKinds.length - 1;
    const crop = this.cropMask;
    return this.treeStrips.map(({ crowns, paths }) => {
      const d = paths.map(() => []);
      for (const c of crowns) {
        const i = Math.floor(c.x / cell), j = Math.floor(c.y / cell);
        if (i < 0 || i >= cols || j < 0 || j >= rows) continue;
        const k = i - (crop?.c0 ?? 0);
        if (crop && k >= 0 && k < crop.W && crop.mask[j * crop.W + k] && blur[j * cols + i] > farm) continue;
        const start = from + c.at * (to - from);
        const r = TREE_RADIUS * w * c.size * clamp01((blur[j * cols + i] - start) / grow);
        if (r < TREE_MIN * w) continue;
        d[0].push(circleData(c.x + 0.15 * r, c.y + 0.2 * r, r));
        for (const [dx, dy, lr] of c.lobes) d[1 + c.tone].push(circleData(c.x + dx * r, c.y + dy * r, lr * r));
        d[light].push(circleData(c.x - 0.3 * r, c.y - 0.3 * r, 0.45 * r));
      }
      return d.map((parts) => parts.join(''));
    });
  }

  // Erodibility grid columns over the window, with a column to spare each
  // side.
  windowColumns(cols, cell) {
    const [x, vw] = this.window;
    return [Math.max(0, Math.floor(x / cell) - 1), Math.min(cols, Math.ceil((x + vw) / cell) + 1)];
  }

  // `values` on the erodibility grid averaged over 3 × 3 cells, in columns
  // [c0, c1), in the buffer `this[name]`, reused between calls.
  blur(values, cols, rows, c0, c1, name) {
    if (this[name]?.length !== cols * rows) this[name] = new Float32Array(cols * rows);
    const out = this[name];
    for (let j = 0; j < rows; j++) {
      for (let i = c0; i < c1; i++) {
        let sum = 0, count = 0;
        for (let jj = Math.max(0, j - 1); jj <= Math.min(rows - 1, j + 1); jj++) {
          for (let ii = Math.max(0, i - 1); ii <= Math.min(cols - 1, i + 1); ii++) {
            sum += values[jj * cols + ii];
            count++;
          }
        }
        out[j * cols + i] = sum / count;
      }
    }
    return out;
  }

  // Draw the scroll bars in age bands when they change, and every
  // `LEVEE_REFRESH` of simulated time as they age. A band's ridges darken
  // as vegetation takes them, and fade over the last quarter of their life.
  drawScrolls(river, memory, w) {
    this.scrollAt = { key: river.scroll_version(), t: river.time() };
    river.sync_scrolls();
    // Views must be built after syncing: it can grow memory.
    const buf = memory.buffer;
    const n = river.scroll_count();
    const pts = new Float32Array(buf, river.scroll_line_ptr(), river.scroll_line_len());
    const off = new Uint32Array(buf, river.scroll_offsets_ptr(), n + 1);
    const ages = new Float32Array(buf, river.scroll_ages_ptr(), n);
    const life = Math.max(1e-6, river.get_param('scroll_lifetime'));
    const [x, vw] = this.window;
    // Keep the path of each ridge already drawn in its band, and add the
    // rest; what is left in `this.ridges` afterwards is gone.
    const ridges = new Map();
    for (let k = 0; k < n; k++) {
      // Skip ridges wholly outside the window.
      let inside = false;
      for (let i = off[k]; i < off[k + 1] && !inside; i++) inside = pts[2 * i] >= x - w && pts[2 * i] <= x + vw + w;
      if (!inside) continue;
      const band = Math.min(SCROLL_BANDS - 1, Math.floor(SCROLL_BANDS * ages[k] / life));
      const d = pathData(pts, off[k], off[k + 1]);
      let key = `${band}${d}`;
      if (ridges.has(key)) key += `#${k}`;
      const path = this.ridges.get(key) ?? el('path', { d }, this.scrollBands[band]);
      this.ridges.delete(key);
      ridges.set(key, path);
    }
    for (const path of this.ridges.values()) path.remove();
    this.ridges = ridges;
    set(this.scrollLayer, 'stroke-width', (0.15 * w).toFixed(2));
  }

  // Record the centreline without drawing, during warm-up.
  capture(river, memory, { local }) {
    river.sync_line();
    const line = new Float32Array(memory.buffer, river.line_ptr(), river.line_len());
    this.record(line, river.time(), river.get_param('width'), local);
  }

  // Keep the visible part of the centreline as point-bar sand once it has
  // moved `BAR_STEP` widths from the last kept one, into the newest band.
  // A band spans `barMemory / BANDS` of simulated time, and is dropped once
  // all its centrelines are older than `barMemory`. This only records the
  // bands; `drawBars` puts them on the page.
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
      // Path data still to be added to the page, and the band's group there.
      band = { t0: t, t1: t, pending: [], g: null };
      this.bands.push(band);
    }
    band.t1 = t;
    const tone = this.barTone(t);
    band.pending.push({ d, tone });
    this.swept.push({ t, d, tone, e: null });
    while (this.bands.length && t - this.bands[0].t1 > local.barMemory) this.dropped.push(this.bands.shift());
  }

  // Tone of the sand laid at time `t`, from 0 on the ridges to
  // `BAR_TONES - 1` in the swales.
  barTone(t) {
    const knot = (len, j) => hash01(this.seed, len, j);
    let u = 0;
    for (const [len, weight] of [[BAR_GRAIN, 0.6], [5 * BAR_GRAIN, 0.4]]) {
      const s = t / len, j = Math.floor(s);
      u += weight * (knot(len, j) + (knot(len, j + 1) - knot(len, j)) * smoothstep(s - j));
    }
    // The sum of two octaves gathers about the middle; stretch it so every
    // tone is laid.
    return Math.min(BAR_TONES - 1, Math.floor(BAR_TONES * clamp01(1.6 * (u - 0.5) + 0.5)));
  }

  // Path element of a kept centreline, stroked in its tone's colour from
  // its group.
  barPath(d, tone, g) {
    return el('path', { d, style: `stroke:var(--tone${tone})` }, g);
  }

  // Style setting the colour of each tone of a band laid at mean time
  // `mid`, at time `t`: bare sand when new, then grassed over, fading into
  // the background as it nears `barMemory`.
  bandStyle(t, mid, local) {
    const fresh = clamp01(1 - (t - mid) / local.barMemory);
    const sand = mixRgb(OLD_BAR, BAR, fresh * fresh);
    const contrast = BAR_CONTRAST * (BAR_RELICT + (1 - BAR_RELICT) * fresh);
    let style = '';
    for (let k = 0; k < BAR_TONES; k++) {
      const tone = mixRgb(sand, SWALE, contrast * k / (BAR_TONES - 1));
      style += `--tone${k}:${mix(SOIL.loam, tone, local.barOpacity * fresh)};`;
    }
    return { style, opacity: smoothstep(2 * fresh).toFixed(2) };
  }

  // Bring the bands on the page up to date: add the centrelines kept since
  // the last draw, move bands that have stopped growing to the bottom layer
  // and remove dropped ones. Shade each band by its age (see `bandStyle`),
  // turning transparent over the soil beneath as it nears `barMemory`. The
  // old bands are shaded only when the bottom layer changes anyway, or the
  // bar settings do, so it stays cached in between. The swept ground is
  // shaded as the newest band.
  drawBars(t, w, local) {
    let changed = false;
    for (const b of this.dropped) {
      changed ||= b.g?.parentNode === this.oldBars;
      b.g?.remove();
    }
    this.dropped = [];
    const newest = this.bands.at(-1);
    for (const b of this.bands) {
      const layer = b === newest ? this.barLayer : this.oldBars;
      if (!b.g) b.g = el('g', {}, layer);
      else if (b.g.parentNode !== layer) layer.append(b.g);
      else if (!b.pending.length) continue;
      changed ||= layer === this.oldBars;
      for (const { d, tone } of b.pending) this.barPath(d, tone, b.g);
      b.pending = [];
    }
    for (const s of this.swept) s.e ??= this.barPath(s.d, s.tone, this.sweptLayer);
    const key = `${w} ${local.barMemory} ${local.barOpacity}`;
    if (key !== this.shadedFor) changed = true;
    this.shadedFor = key;
    for (const layer of [this.oldBars, this.barLayer, this.sweptLayer]) set(layer, 'stroke-width', w);
    for (const b of changed ? this.bands : this.bands.slice(-1)) {
      const { style, opacity } = this.bandStyle(t, 0.5 * (b.t0 + b.t1), local);
      set(b.g, 'style', style);
      // Group opacity, so a band's own paths do not build up where they
      // overlap.
      set(b.g, 'opacity', opacity);
    }
    if (newest) {
      const { style, opacity } = this.bandStyle(t, 0.5 * (newest.t0 + newest.t1), local);
      set(this.sweptLayer, 'style', style);
      set(this.sweptLayer, 'opacity', opacity);
    }
  }

  // Serialise the current frame, both layers, as a standalone SVG file.
  save() {
    const copy = this.svg.cloneNode(true);
    copy.prepend(...[...this.bars.childNodes, ...this.ground.childNodes].map((n) => n.cloneNode(true)));
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
const renderer = new SvgRenderer(document.getElementById('bars'), document.getElementById('ground'), svg);
document.getElementById('save').addEventListener('click', () => renderer.save());

startApp({
  view: svg,
  renderer,
  display: {
    oxbowShrink: 0.6,
    oxbowRelief: 1,
    barMemory: 600,
    barOpacity: 0.75,
    showBars: true,
    showLevees: true,
    showScrolls: true,
  },
});
