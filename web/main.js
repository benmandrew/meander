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
// Reed swamp fills a lake's shallows: on a level bed, a fringe
// `REED_FRINGE[0]` of the old channel's half-width wide when cut off,
// widening by `REED_FRINGE[1]` over the fill. Where the bed slopes along the
// lake the fringe reaches further. At a plugged end the open water stops
// `REED_END` fringes short, as the reeds grow out over the plug.
const REED_FRINGE = [0.2, 0.3];
const REED_END = 2;
// Deep water: the open water is tinted `SHALLOW_TINT` of the way to
// `SHALLOWS` once it clears, under nested bands `DEPTH_BANDS` deeper than
// its edge, in the depth units of `lakeBed`, each `DEEP` at `DEEP_OPACITY`.
// They fade
// by `DEEP_MURK` of that as the lake turns murky, and lie towards the old
// outer bank by `DEEP_SKEW` of the room they have (see `lakeData`).
const SHALLOWS = [110, 160, 176];
const SHALLOW_TINT = 0.3;
const DEEP = [16, 50, 78];
const DEEP_OPACITY = 0.3;
const DEEP_MURK = 0.7;
const DEPTH_BANDS = [0.12, 0.3];
const DEEP_SKEW = 0.7;
// Silt plumes: floods bring silty water in at each end of a lake, and a
// tributary at the head of its delta. A plume fades over `PLUME_LENGTH`
// widths from an end, or within `DELTA_PLUME` widths of a delta's head, from
// `PLUME` at up to `PLUME_OPACITY`. Under wooded upland it is `PLUME_BASE`
// of that, rising to all of it as clearance raises the supply of sediment
// to `clearance_supply`.
const PLUME = [150, 140, 100];
const PLUME_OPACITY = 0.6;
const PLUME_BASE = 0.35;
const PLUME_LENGTH = 4;
const DELTA_PLUME = 2.5;
// Water lilies (see `lilyData`): pads up to `LILY_SIZE` widths in radius, in
// clusters reaching `LILY_REACH` widths, over open water up to `LILY_DEPTH`
// deep, in each `LILY_STEP` widths of the oxbow. In a lake left to fill on
// its own, they open from `LILY_START` of the fill, about 750 years after
// the cutoff, until about 2,100 years.
const LILY = [104, 134, 76];
const LILY_SIZE = 0.14;
const LILY_REACH = 0.4;
const LILY_DEPTH = 0.25;
const LILY_STEP = 0.8;
const LILY_START = 0.25;
const LILY_SPREAD = 0.45;
// Duckweed (see `weedData`): mats `WEED_STEP` widths apart that start from
// `WEED_START` of the fill, about 2,150 years after the cutoff of a lake left
// to fill on its own, and cover the water by about 2,800 years.
const WEED = [156, 172, 84];
const WEED_OPACITY = 0.75;
const WEED_STEP = 3;
const WEED_START = 0.72;
const WEED_SPREAD = 0.1;
const WEED_GROW = 0.12;
// Points either side over which a lake's bend is smoothed, and its gain in
// widths (see `lakeBed`): a bend of radius 2.5 widths has a bend of 1.
const BEND_SPAN = 3;
const BEND_GAIN = 2.5;
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
// nothing else. Work beyond it carries on in the frames that follow. As
// the floodplain falls behind from half the lag `ui.js` allows to all of
// it, a frame's budget rises to the idle frame's, so the river slows less.
// While the floodplain only sets the river's speed, the budget stays put:
// on a phone at 8× throttling the idle budget there cut the frames that
// showed the river moving from 28 to 24 a second, for 17% more speed.
const FLOODPLAIN_BUDGET = 4;
const FLOODPLAIN_IDLE_BUDGET = 10;
// Each layer's expected redraw time in real time is a running mean, which
// keeps `REDRAW_MEMORY` of the mean before, and a redraw is expected to
// take at least `REDRAW_SLACK` ms more. A redraw's time counts at most
// `REDRAW_GAP` ms from one frame to the next.
const REDRAW_MEMORY = 0.8;
// Seconds in real time a floodplain layer rests after showing a new
// picture before it may start another. Its refreshes are set in simulated
// time, so at full speed the forest fell due six times a second; resting
// is not counted as due, so it does not slow the river.
const REDRAW_REST = 0.5;
const REDRAW_SLACK = 100;
const REDRAW_GAP = 100;
// Valley sides. Each wall is a scarp from its foot, where the floodplain
// ends, up to its brow, where the older upland begins. The core sets the
// foot, with the combes cut back into it (see `walls.rs`). The scarp is
// `SCARP` widths wide, varying with its own noise, and at a combe the brow
// cuts back `COMBE_BROW` times as deep and wide as the foot, so the
// contours bend into a V.
// Relief is drawn as on old survey maps: hachures run down the slope over
// the fields, in two staggered rows `HACHURE_GAP` widths apart, heavier
// where the slope is steeper. Each is a wedge that thins down the slope
// from `HACHURE_TAPER[0]` to `HACHURE_TAPER[1]` of its width. The light
// comes from the upper left, as for the trees, so the wall at the top
// faces away from it, with darker hachures and a shadow over the scarp,
// and the wall at the bottom catches it. The shade or light is laid in
// nested bands, `SCARP_TONE` opaque where they overlap over the steep
// middle of the slope, so it softens towards the brow and foot. Where `HANGER` noise passes its threshold, favoured on steep
// stretches and in combes, the slope is a hanger wood in place of fields,
// tapering at each end. The upland's drier soil tints the ground beyond
// the foot, under its fields.
const UPLAND = [190, 182, 132];
const UPLAND_OPACITY = 0.3;
const SCARP_SHADE = [96, 100, 66];
const SCARP_LIGHT = [244, 236, 200];
const SCARP_TONE = { shade: 0.24, light: 0.26 };
const SCARP_BANDS = [[0, 1], [0.15, 0.85], [0.3, 0.7]];
const SCARP = [1.2, 4.5];
const COMBE_BROW = 1.6;
const HANGER = { knots: 14, threshold: 0.62 };
const HACHURE_GAP = 0.45;
const HACHURE_WIDTH = 0.11;
const HACHURE_TAPER = [1.5, 0.25];
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
// Each circle of a crown is a dot: a zero-length line with round caps,
// stroked at its diameter, which takes about 12 characters of path data
// where two arcs took about 46. A path has one stroke width, so radii are
// rounded to classes `CROWN_CLASS` apart, within 9%, from `CROWN_MIN`
// widths, below the smallest highlight.
const CROWN_CLASS = 2 ** (1 / 4);
const CROWN_MIN = 0.025;
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
// The fields redraw after them, as which fields the fade reaches changes.
const FIELD_FADE = 8;
// A square larger than any window, for clips that cut holes in the plane.
const OUTSIDE = 'M-1e6-1e6h2e6v2e6h-2e6z';
// The opacity that a copy of a fading field adds in region `k` of the
// fade, 1 to `FIELD_FADE`, over the copies in the regions before it, so
// that `k` copies stack to `k / FIELD_FADE` of the field's `opacity`.
function fadeStep(opacity, k) {
  const share = opacity / FIELD_FADE;
  return Math.round(1000 * share / (1 - share * (k - 1))) / 1000;
}
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
// Ridge and furrow: centuries of ploughing each strip towards its middle
// raised it into a ridge, and arable fields show their furrows,
// `FURROW_GAP` widths apart, along the field's length. An ox team began to
// swing left before the headland, so each ridge bends in a reversed S,
// `FURROW_S` of the field's length either way. The furrows stop
// `FURROW_HEAD` widths short of each end, where the team turned on the
// headland, and are sampled every `FURROW_STEP` widths.
const FURROW = [124, 122, 78];
const FURROW_GAP = 0.6;
const FURROW_S = 0.05;
const FURROW_HEAD = 0.4;
const FURROW_STEP = 0.5;
const FURROW_WIDTH = 0.09;
const FURROW_OPACITY = 0.3;
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
// Wet ground: the backswamps the core finds in clay hollows of the low
// floodplain far from the water (see `land.rs`), drawn as wet meadow. The
// blurred wetness is contoured at `WET_LEVELS` in marsh, `WET_MARSH`
// opaque where all of them overlap, with tufts of reed over the wettest.
// It changes as slowly as the coarse ground and is redrawn as often.
const WET_MARSH = 0.3;
const WET_LEVELS = [0.2, 0.5, 0.8];
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
// Farthest a leat runs from its weir along the channel, in widths: the
// core's `LEAT_UP`.
const MILL_REACH = 7;
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
const FOOTPRINTS = [[1.1, 0.7], [0.9, 0.35], [1.5, 0.8], [1.0, 0.55], [0.8, 0.5], [0.8, 0.55], [0.7, 0.45], [3.4, 2.2]];
const THATCH = [146, 128, 96];
const YARD_EARTH = [204, 190, 152];
const MOUND = [180, 170, 126];
const PLATFORM = [150, 140, 108];
const MOUND_RIM = 0.3;
const YARD_RIM = 0.35;
const BUILDING_REFRESH = 20;
// The parish church, laid east and west in a churchyard of radii
// `CHURCH_YARD` widths: a west tower, the nave and the chancel at the east
// end, each a centre along the church and half extents along and across
// it, in widths. Stone walls and a roof of stone slates, with a yew or two
// in the churchyard. A ruin fades over the core's `CHURCH_RUIN_TIME`
// (1,000 years).
const CHURCH_YARD = [1.7, 1.1];
const CHURCH_PARTS = [[-1.0, 0.27, 0.27], [-0.09, 0.64, 0.36], [0.85, 0.3, 0.26]];
const CHURCH_ROOF = [150, 146, 132];
const TOWER = [122, 118, 106];
const CHURCHYARD = [150, 170, 110];
const YEW = [44, 66, 48];
const YEW_RADIUS = 0.24;
// Fishponds in the old channels: the core's `POND` footprint in widths,
// still water darker than the river's, a fringe of reeds, and the spoil
// banks `POND_BANK` widths wide around each. An abandoned pond silts to
// marsh over the core's `POND_RUIN_TIME` (250 years).
const POND = [1.3, 0.5];
const POND_WATER = [72, 116, 124];
const POND_BANK = 0.16;
const POND_MARSH = [118, 134, 96];
// Tracks: an earth surface `TRACK_WIDTH` wide on a darker verge
// `TRACK_VERGE` wide, simplified to within `TRACK_SIMPLIFY` widths and
// redrawn at most every `TRACK_REFRESH`.
const TRACK = [206, 186, 138];
const VERGE = [118, 106, 74];
const TRACK_WIDTH = 0.3;
const TRACK_VERGE = 0.5;
const TRACK_SIMPLIFY = 0.35;
const TRACK_REFRESH = 20;
// Hollow ways: a track the core has worn past each of `HOLLOW_LEVELS` of
// depth gains a pair of banks `HOLLOW_WIDTH` widths wide, the shaded one
// set towards the upper-left light and the lit one away from it, by
// `HOLLOW_SHIFT` widths, so the banks stack deeper as the lane sinks. An
// abandoned hollow way keeps its banks over a grassy floor until it fills.
const HOLLOW_LEVELS = [0.2, 0.45, 0.7];
const HOLLOW_WIDTH = 0.9;
const HOLLOW_SHIFT = 0.12;
const HOLLOW_SHADE = [86, 90, 58];
const HOLLOW_LIGHT = [240, 232, 196];
const HOLLOW_FLOOR = [126, 140, 92];
// A ford: a band of broken, shallow water `FORD_WIDTH` wide across the
// channel, with stepping stones of bar sand along it.
const FORD_WATER = [150, 188, 196];
const FORD_WIDTH = 0.7;
// The parish boundary: a dash-dot line, as on an estate map, simplified to
// within `BOUNDARY_SIMPLIFY` widths and drawn `BOUNDARY_WIDTH` wide.
const BOUNDARY = [86, 70, 92];
const BOUNDARY_WIDTH = 0.16;
const BOUNDARY_DASH = [1.4, 0.45, 0.2, 0.45];
const BOUNDARY_SIMPLIFY = 0.15;
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
// The inner bank of a bend shoals onto its point bar, in the riffles'
// shallow water reaching `SHOAL` of the width out from the bank at a bend
// of radius `BAR_RADIUS` widths or tighter. With the riffles, the shallow
// water crosses from the inner bank of each bend to the inner bank of the
// next.
const SHOAL = 0.25;
// Pools and shoals are drawn `THIN_BAND` widths narrower, so they end where
// they would be too thin to see and are not drawn along the reaches
// between bends.
const THIN_BAND = 0.03;
// Where the channel is wider than `MID_BAR_FROM` widths, a sand bar splits
// the flow, growing to `MID_BAR` of the width across at `MID_BAR_FULL`. Its
// head is blunt, and its tail narrows downstream by a width across for
// every `MID_BAR_TAIL` widths along. In a bend of radius `BAR_RADIUS`
// widths or tighter it lies `MID_BAR_SHIFT` of the width towards the inner
// bank, clear of the pool. It rises from shallow water reaching
// `MID_BAR_HALO` times its half-width from its middle, through wet sand
// `MID_BAR_RIM` widths wide, to dry sand.
const MID_BAR_FROM = 1.5;
const MID_BAR_FULL = 1.8;
const MID_BAR = 0.4;
const MID_BAR_TAIL = 3;
const MID_BAR_SHIFT = 0.15;
const MID_BAR_HALO = 1.6;
const MID_BAR_RIM = 0.08;
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
// a tile, so short runs spare each tile from stroking the whole line, but
// each element costs a write, style and paint. Against runs of 24, runs of
// 96 cut a channel redraw's page-thread time by about 0.4 ms (seed 7,
// 1280 × 800) and left its GPU time unchanged; one path for the whole
// channel cut 0.5 ms but added 0.3 ms of GPU time.
const RUN = 96;
// Channel drawn beyond each side of the window, in widths: the widest bank
// with its bar reaches 1.5 widths from the centreline, and a curve's
// handles up to half a width beyond its ends.
const CHANNEL_MARGIN = 3;
// Segments of the last centreline searched for each point's foot, behind
// and ahead of the last foot. Resampling shifts the points along by under
// a segment a step, so this spans ten steps at the top speed.
const FOOT_BACK = 2;
const FOOT_AHEAD = 8;
// Device pixels the channel must have moved for a frame to redraw it. At
// the default speed it moves 0.1–0.3 CSS px a frame on a landscape phone.
const MIN_MOVE = 0.5;
// Sine of the turn between the tangents at a segment's ends, about 3°,
// below which a channel edge is drawn as a straight segment and not a
// curve. At 3° the curve lies under 0.1 px from the chord, and about three
// segments in four turn less, so the path data stays short.
const CURVE_TURN = 0.05;
// Distance in world units from the line through its neighbours within which
// `lineData` leaves a point out: 0.25 px on a desktop window 768 units wide
// at 1280 px, and 0.37 device px on a landscape phone with 3 device pixels to
// the CSS pixel. Below the 0.1 units to which path data is written, which
// it first rounds to, points could not move.
const SIMPLIFY = 0.15;
// Change in a lake's fill that redraws its outline. A lake fills over about
// a thousand units of simulated time, so its outline is rebuilt every few
// units, not every step, and moves less than a pixel at each rebuild.
const LAKE_STEP = 1e-3;
// Milliseconds a draw for lake rebuilds. At high speeds every lake's fill
// passes a `LAKE_STEP` between draws; lakes beyond the budget wait for a
// later draw.
const LAKE_BUDGET = 2;

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

// A detached twin of the path `e` with path data `d`, or `e` itself if its
// data is already `d`. Chrome parses path data when it is set, which took
// up to 8 ms for the forest's 370 KB at 4× throttling, so a staged redraw
// parses its twins stage by stage and `swap` puts them on the page together.
function twin(e, d) {
  if (e.attrs?.d === d) return e;
  const t = document.createElementNS(NS, 'path');
  for (const a of e.attributes) if (a.name !== 'd') t.setAttribute(a.name, a.value);
  t.attrs = { ...e.attrs, d };
  t.setAttribute('d', d);
  return t;
}

// Put each of `twins` on the page in place of the path at its index in
// `paths`, and in `paths`.
function swap(paths, twins) {
  twins.forEach((t, k) => {
    if (t === paths[k]) return;
    paths[k].replaceWith(t);
    paths[k] = t;
  });
}

// `v` to one decimal place, without the trailing zero `toFixed` keeps,
// and several times faster. The whole part and the tenths are written as
// integers with the point between them, since V8 writes an integer faster
// than it finds the shortest form of a fraction. A value between -1 and 0
// has a whole part of -0, which writes as "0", so it takes its sign here.
function dp1(v) {
  const r = Math.round(10 * v), q = Math.trunc(r / 10), f = Math.abs(r - 10 * q);
  return (r < 0 && q === 0 ? '-0' : '' + q) + (f ? '.' + f : '');
}

// Path data is written in relative coordinates from a pen held in whole
// tenths of a unit, so each point lands where `dp1` would put it but its
// numbers are shorter: the polylines on the page took 34–42% fewer bytes.
// A subpath starts with an absolute M, from `penAt`, so path data from
// separate calls can be joined. `num` writes tenths without a leading zero,
// and `pair` and `sep` leave out a space before a minus sign.
let penX = 0, penY = 0;

function num(r) {
  const a = r < 0 ? -r : r, q = Math.trunc(a / 10), f = a - 10 * q;
  return (r < 0 ? '-' : '') + (f ? (q ? q + '.' : '.') + f : '' + q);
}

function pair(x, y) {
  return num(x) + (y < 0 ? '' : ' ') + num(y);
}

// `s` after a number, with a space unless it starts with a minus sign.
function sep(s) {
  return s.charCodeAt(0) === 45 ? s : ' ' + s;
}

// The point (x, y) in absolute coordinates, moving the pen there.
function penAt(x, y) {
  penX = Math.round(10 * x);
  penY = Math.round(10 * y);
  return pair(penX, penY);
}

// The point (x, y) relative to the pen, moving the pen there.
function penTo(x, y) {
  const px = Math.round(10 * x), py = Math.round(10 * y);
  const s = pair(px - penX, py - penY);
  penX = px;
  penY = py;
  return s;
}

// A point relative to the pen without moving it, for a curve's handles.
function penOff(x, y) {
  return pair(Math.round(10 * x) - penX, Math.round(10 * y) - penY);
}

// Path data for points [start, end) of a flat x, y array: an absolute M and
// relative lines, closed with `close`. Points within `SIMPLIFY` of the
// line through the points kept either side are left out (`keepPoints`).
function lineData(pts, start = 0, end = pts.length / 2, close = false) {
  if (end - start < 2) return '';
  const keep = SIMPLIFY > 0 ? keepPoints(pts, start, end, SIMPLIFY) : null;
  let d = 'M' + penAt(pts[2 * start], pts[2 * start + 1]), first = true;
  for (let i = start + 1; i < end; i++) {
    if (keep && !keep[i - start]) continue;
    const p = penTo(pts[2 * i], pts[2 * i + 1]);
    d += first ? 'l' + p : sep(p);
    first = false;
  }
  return close ? d + 'z' : d;
}

// Scratch for `keepPoints`, grown as needed and reused.
const keepScratch = { keep: new Uint8Array(0), stack: new Int32Array(0) };

// Flags, in a scratch array the next call overwrites, for the points
// [start, end) of a flat x, y array that a Douglas–Peucker simplification
// to within `tol` keeps, the end points always.
function keepPoints(pts, start, end, tol) {
  const n = end - start;
  if (keepScratch.keep.length < n) {
    keepScratch.keep = new Uint8Array(Math.ceil(1.25 * n));
    keepScratch.stack = new Int32Array(2 * Math.ceil(1.25 * n));
  }
  const { keep, stack } = keepScratch;
  keep.fill(0, 0, n);
  keep[0] = keep[n - 1] = 1;
  let top = 0;
  stack[top++] = 0;
  stack[top++] = n - 1;
  const tol2 = tol * tol;
  while (top) {
    const b = stack[--top], a = stack[--top];
    const ax = pts[2 * (start + a)], ay = pts[2 * (start + a) + 1];
    const dx = pts[2 * (start + b)] - ax, dy = pts[2 * (start + b) + 1] - ay;
    const len2 = dx * dx + dy * dy;
    let far = -1, farD = tol2;
    for (let k = a + 1; k < b; k++) {
      const px = pts[2 * (start + k)] - ax, py = pts[2 * (start + k) + 1] - ay;
      const t = len2 > 0 ? clamp01((px * dx + py * dy) / len2) : 0;
      const ex = px - t * dx, ey = py - t * dy, d = ex * ex + ey * ey;
      if (d > farD) { far = k; farD = d; }
    }
    if (far >= 0) {
      keep[far] = 1;
      stack[top++] = a; stack[top++] = far;
      stack[top++] = far; stack[top++] = b;
    }
  }
  return keep;
}

// Closed path data out along `left` and back along `right`, flat x, y
// arrays that share their end points, each kept once.
function ringData(left, right) {
  for (let i = right.length / 2 - 2; i > 0; i--) left.push(right[2 * i], right[2 * i + 1]);
  return lineData(left, 0, left.length / 2, true);
}

// Unit left normal and signed curvature at each point of a flat x, y array,
// positive where the river turns towards the left normal.
function channelFrame(pts) {
  const n = pts.length / 2;
  const nx = new Float64Array(n), ny = new Float64Array(n), curv = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    const a = Math.max(0, i - 1), b = Math.min(n - 1, i + 1);
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

// Path data along an edge offset from the centreline over points
// [start, end), given as a flat x, y array: cubic Béziers that leave each
// point along the centreline's tangent, which an offset curve shares, so a
// tight bend draws as a curve and not as a corner at each point. Each
// handle is a third of the chord projected on the tangent, so it shrinks
// to nothing where the edge folds back at a tight bend and cannot loop.
// A segment that turns less than `CURVE_TURN` stays straight. `back` runs
// the edge upstream. The data starts with the first point in absolute
// coordinates, ready to follow an M or an L, and goes on in relative ones,
// writing a command letter only where it changes.
function edgeData(edge, frame, start, back) {
  const { nx, ny } = frame;
  const n = edge.length / 2, s = back ? -1 : 1;
  let k = back ? n - 1 : 0;
  let ax = edge[2 * k], ay = edge[2 * k + 1];
  let tax = s * ny[start + k], tay = -s * nx[start + k];
  let d = penAt(ax, ay), last = '';
  for (let m = 1; m < n; m++) {
    const j = k + s;
    const bx = edge[2 * j], by = edge[2 * j + 1];
    // Tangents in the direction of travel: the left normal turned clockwise.
    const tbx = s * ny[start + j], tby = -s * nx[start + j];
    if (Math.abs(tax * tby - tay * tbx) < CURVE_TURN) {
      const p = penTo(bx, by);
      d += last === 'l' ? sep(p) : 'l' + p;
      last = 'l';
    } else {
      const dx = bx - ax, dy = by - ay;
      const ha = Math.max(0, dx * tax + dy * tay) / 3;
      const hb = Math.max(0, dx * tbx + dy * tby) / 3;
      const p = penOff(ax + ha * tax, ay + ha * tay);
      const c = p + sep(penOff(bx - hb * tbx, by - hb * tby)) + sep(penTo(bx, by));
      d += last === 'c' ? sep(c) : 'c' + c;
      last = 'c';
    }
    k = j; ax = bx; ay = by; tax = tbx; tay = tby;
  }
  return d;
}

// Float64Arrays kept from draw to draw by name, so the channel's per-run
// and per-pass arrays are not allocated each draw. `scratch` returns a view
// of the first `n` entries, whose contents are left from the last use.
const scratchArrays = {};
function scratch(name, n) {
  if (!(scratchArrays[name]?.length >= n)) scratchArrays[name] = new Float64Array(Math.ceil(1.25 * n));
  return scratchArrays[name].subarray(0, n);
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
  const left = scratch('left', 2 * (end - start)), right = scratch('right', 2 * (end - start));
  for (let i = start; i < end; i++) {
    const h = scale * widths[i], k = curv[i], b = bar ? bar[i] : 0;
    const bl = h + Math.max(0, b), br = h + Math.max(0, -b);
    const hl = k > 0 ? Math.min(bl, 0.95 / k) : bl;
    const hr = k < 0 ? Math.min(br, -0.95 / k) : br;
    const x = pts[2 * i], y = pts[2 * i + 1], j = 2 * (i - start);
    left[j] = x + hl * nx[i]; left[j + 1] = y + hl * ny[i];
    right[j] = x - hr * nx[i]; right[j + 1] = y - hr * ny[i];
  }
  return 'M' + edgeData(left, frame, start, false) + 'L' + edgeData(right, frame, start, true) + 'Z';
}

// The feet on `a`, with local widths `wa`, of the points of `b`, with
// `wb`, for `between`. Each point of `b` moves out from its foot on `a`, so
// the channel moves sideways as it migrates, while resampling slides the
// points along it. A point more than `far` from `a`, where a cutoff has
// rerouted the channel, is its own foot. Also returns how far any point
// with x in `[x0, x1]` moves, or its bank by `BANK_EDGE` of its width.
function footing(a, wa, b, wb, far, x0, x1) {
  const na = a.length / 2, nb = b.length / 2;
  const line = new Float32Array(2 * nb), widths = new Float32Array(nb);
  let j = 0, move = 0;
  for (let i = 0; i < nb; i++) {
    const px = b[2 * i], py = b[2 * i + 1];
    let best = far * far, fx = px, fy = py, fw = wb[i], foot = j;
    for (let k = Math.max(0, j - FOOT_BACK); k < Math.min(na - 1, j + FOOT_AHEAD); k++) {
      const ax = a[2 * k], ay = a[2 * k + 1];
      const dx = a[2 * k + 2] - ax, dy = a[2 * k + 3] - ay;
      const len2 = dx * dx + dy * dy;
      const t = len2 > 0 ? clamp01(((px - ax) * dx + (py - ay) * dy) / len2) : 0;
      const qx = ax + t * dx, qy = ay + t * dy;
      const d2 = (px - qx) ** 2 + (py - qy) ** 2;
      if (d2 < best) {
        best = d2; fx = qx; fy = qy; fw = wa[k] + t * (wa[k + 1] - wa[k]); foot = k;
      }
    }
    j = foot;
    line[2 * i] = fx; line[2 * i + 1] = fy; widths[i] = fw;
    if (px >= x0 && px <= x1) {
      move = Math.max(move, Math.sqrt((px - fx) ** 2 + (py - fy) ** 2) + BANK_EDGE * Math.abs(wb[i] - fw));
    }
  }
  return { line, widths, move };
}

// The centreline and widths a fraction `f` of the way from the feet to
// `b`, with `wb`, into arrays kept with the feet. They are `Float32Array`s,
// as the centrelines are, so the channel's functions see one kind of array.
function between(feet, b, wb, f) {
  feet.at ??= { line: new Float32Array(b.length), widths: new Float32Array(wb.length) };
  const { line: fl, widths: fw } = feet, { line, widths } = feet.at;
  for (let i = 0; i < b.length; i++) line[i] = fl[i] + f * (b[i] - fl[i]);
  for (let i = 0; i < wb.length; i++) widths[i] = fw[i] + f * (wb[i] - fw[i]);
  return { line, widths };
}

// Length of a flat x, y polyline.
function lineLength(pts) {
  let len = 0;
  for (let i = 2; i < pts.length; i += 2) len += Math.sqrt((pts[i] - pts[i - 2]) ** 2 + (pts[i + 1] - pts[i - 1]) ** 2);
  return len;
}

// Points [start, end) of a flat x, y array from the first point with x at
// or beyond `x0` to the last with x at or before `x1`, with a point to
// spare each side. The hidden reaches lie outside it, so the views draw
// only the window, though the span keeps any loop inside it whole.
function span(pts, x0, x1) {
  const n = pts.length / 2;
  let [start, end] = [0, n];
  while (start < n && pts[2 * start] < x0) start++;
  while (end > start && pts[2 * end - 2] > x1) end--;
  return [Math.max(0, start - 1), Math.min(n, end + 1)];
}

// Channel outlines over the points `[start, end)` in runs of `RUN`
// segments, each overlapping its neighbours by a segment so no seam shows
// between them. The last run stops at `end`, or it would draw up to `RUN`
// points beyond the window.
function channelRuns(pts, widths, frame, [start, end], scale, bar = null) {
  const runs = [];
  for (let s = start; s < end - 1; s += RUN) {
    runs.push(channelData(pts, widths, frame, scale, bar, Math.max(start, s - 1), Math.min(end, s + RUN + 2)));
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

// `values` smoothed along the river by four [1, 2, 1] passes, in the
// scratch array `name`, which the next call with that name overwrites.
function smoothed(values, name) {
  const n = values.length;
  const [out, between] = [scratch(name, n), scratch('smoothed', n)];
  let prev = values;
  for (const v of [between, out, between, out]) {
    for (let i = 0; i < n; i++) v[i] = 0.25 * prev[Math.max(0, i - 1)] + 0.5 * prev[i] + 0.25 * prev[Math.min(n - 1, i + 1)];
    prev = v;
  }
  return out;
}

// Closed path data for a band along the river over points [start, end),
// between signed offsets `a[i]` and `b[i]` along the left normal.
function bandData(pts, frame, a, b, start, end) {
  const { nx, ny } = frame;
  const left = scratch('left', 2 * (end - start)), right = scratch('right', 2 * (end - start));
  for (let i = start; i < end; i++) {
    const x = pts[2 * i], y = pts[2 * i + 1], j = 2 * (i - start);
    left[j] = x + a[i] * nx[i]; left[j + 1] = y + a[i] * ny[i];
    right[j] = x + b[i] * nx[i]; right[j + 1] = y + b[i] * ny[i];
  }
  return 'M' + edgeData(left, frame, start, false) + 'L' + edgeData(right, frame, start, true) + 'Z';
}

// Bands between offsets `a` and `b` over the stretches where they are
// apart over the points `[from, to)`, one path data string each, skipping
// points where they meet.
function bandRuns(pts, frame, a, b, [from, to]) {
  const n = pts.length / 2, runs = [];
  let start = -1;
  for (let i = from; i <= to; i++) {
    const open = i < to && Math.abs(a[i] - b[i]) > 0.01;
    if (open && start < 0) start = Math.max(0, i - 1);
    if (!open && start >= 0) {
      runs.push(bandData(pts, frame, a, b, start, Math.min(n, i + 1)));
      start = -1;
    }
  }
  return runs;
}

// Pools, shoals, riffles and mid-channel bars, as lists of path data: one
// list per pool level, the shoals, the riffles, then the bars' shallows,
// wet sand and dry sand. All are redrawn with the banks, since any that
// redrew less often visibly lagged them. `bend` is the smoothed signed
// curvature, `widths` the local width in world units, `w` the reference
// width. Only the points `shown` are drawn, though the bends and widths are
// read along the whole river.
function channelDepth(pts, widths, frame, bend, w, shown) {
  const n = bend.length;
  const lag = Math.round(POOL_LAG / SPACING);
  // A band in from the bank on side `side(i)` of the left normal, `reach(i)`
  // less `THIN_BAND` widths wide, over the points `bandRuns` reads.
  const [from, to] = [Math.max(0, shown[0] - 1), Math.min(n, shown[1] + 1)];
  const fromBank = (side, reach) => {
    const a = scratch('bankA', n), b = scratch('bankB', n);
    for (let i = from; i < to; i++) {
      const half = 0.5 * widths[i], s = side(i);
      a[i] = s * half;
      b[i] = s * Math.max(0, half - Math.max(0, reach(i) - THIN_BAND * w));
    }
    return bandRuns(pts, frame, a, b, shown);
  };
  const tight = (k) => smoothstep(Math.abs(k) * w * BAR_RADIUS);
  // Pools against the bank on the outside of the bend upstream.
  const pools = POOL_LEVELS.map((level) => fromBank(
    (i) => (bend[Math.max(0, i - lag)] > 0 ? -1 : 1),
    (i) => level * widths[i] * tight(bend[Math.max(0, i - lag)])));
  const shoals = fromBank(
    (i) => (bend[i] > 0 ? 1 : -1),
    (i) => SHOAL * widths[i] * tight(bend[i]));
  return [...pools, shoals, riffleRuns(pts, widths, frame, bend, w, shown), ...midBarRuns(pts, widths, frame, bend, w, shown)];
}

// Riffles over the points `shown`, as `channelDepth` draws them.
function riffleRuns(pts, widths, frame, bend, w, shown) {
  const n = bend.length;
  // Strength of the riffle at each point: the weaker of the strongest
  // bends of opposite sign upstream and downstream, ramped about
  // `RIFFLE_RADIUS`, where the curvature here is small beside it. Only
  // over the points the smoothing below carries into the points `shown`.
  const look = Math.round(RIFFLE_LOOK / SPACING), tight = 1 / (RIFFLE_RADIUS * w);
  const strength = scratch('strength', n).fill(0);
  for (let i = Math.max(0, shown[0] - 6); i < Math.min(n, shown[1] + 6); i++) {
    let upL = 0, upR = 0, downL = 0, downR = 0;
    for (let j = Math.max(0, i - look); j <= i; j++) {
      if (bend[j] > upL) upL = bend[j];
      else if (-bend[j] > upR) upR = -bend[j];
    }
    for (let j = i; j < Math.min(n, i + look + 1); j++) {
      if (bend[j] > downL) downL = bend[j];
      else if (-bend[j] > downR) downR = -bend[j];
    }
    const bends = Math.max(Math.min(upL, downR), Math.min(upR, downL));
    if (bends <= 0) continue;
    const grow = smoothstep((bends / tight - 1 + RIFFLE_RAMP) / (2 * RIFFLE_RAMP));
    strength[i] = grow * (1 - smoothstep(Math.abs(bend[i]) / (RIFFLE_CROSSING * bends)));
  }
  // Smoothed along the river, so each riffle tapers to round ends.
  const riffle = smoothed(strength, 'riffle');
  const ra = scratch('riffleA', n), rb = scratch('riffleB', n);
  for (let i = Math.max(0, shown[0] - 1); i < Math.min(n, shown[1] + 1); i++) {
    ra[i] = 0.5 * RIFFLE_WIDTH * widths[i] * riffle[i];
    rb[i] = -ra[i];
  }
  return bandRuns(pts, frame, ra, rb, shown);
}

// Mid-channel bars over the points `shown`, on the width smoothed so they
// grow and shrink slowly: their shallows, wet sand and dry sand.
function midBarRuns(pts, widths, frame, bend, w, shown) {
  const n = widths.length;
  const wide = smoothed(widths, 'wide');
  // The points that reach the points `shown`: a tail of up to 5 points,
  // the smoothing's 4 and the 1 `bandRuns` reads beyond them.
  const [lo, hi] = [Math.max(0, shown[0] - 10), Math.min(n, shown[1] + 5)];
  if (!wide.subarray(lo, hi).some((x) => x > MID_BAR_FROM * w)) return [[], [], []];
  // Half-width of the bar, its tail narrowing downstream from its widest.
  const drop = SPACING * w / (2 * MID_BAR_TAIL), grown = scratch('barHalf', n).fill(0, 0, lo);
  for (let i = lo; i < hi; i++) {
    const h = 0.5 * MID_BAR * wide[i] * smoothstep((wide[i] / w - MID_BAR_FROM) / (MID_BAR_FULL - MID_BAR_FROM));
    grown[i] = Math.max(h, i > 0 ? grown[i - 1] - drop : 0);
  }
  const half = smoothed(grown, 'barSmooth');
  // Middle of the bar, towards the inner bank of a bend and inside the
  // channel.
  const mid = scratch('barMid', n);
  for (let i = lo; i < hi; i++) {
    const room = Math.max(0, 0.45 * widths[i] - half[i]);
    const shift = Math.sign(bend[i]) * MID_BAR_SHIFT * widths[i] * smoothstep(Math.abs(bend[i]) * w * BAR_RADIUS);
    mid[i] = Math.max(-room, Math.min(room, shift));
  }
  const reaches = [
    (i) => Math.min(MID_BAR_HALO * half[i], 0.5 * widths[i] - Math.abs(mid[i])),
    (i) => half[i],
    (i) => Math.max(0, half[i] - MID_BAR_RIM * w),
  ];
  return reaches.map((reach) => {
    const a = scratch('barA', n), b = scratch('barB', n);
    for (let i = lo; i < hi; i++) {
      const r = reach(i);
      a[i] = mid[i] + r;
      b[i] = mid[i] - r;
    }
    return bandRuns(pts, frame, a, b, shown);
  });
}

// Path data for points [start, end) of a flat x, y array.
function pathData(pts, start, end) {
  return lineData(pts, start, end);
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
  return lineData(pts);
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
  // How many edges meet at corner (i, j), which is cheaper than listing
  // them, and at most corners none do.
  const degree = (i, j) => (i > 0 ? edges[j * W + i - 1] : 0) + (i < W ? edges[j * W + i] : 0)
    + (j > 0 ? edges[nh + (j - 1) * CW + i] : 0) + (j < H ? edges[nh + j * CW + i] : 0);
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
      if (degree(i, j) <= 2) continue;
      incident(i, j, step);
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

// Path data for a circle: two relative arcs from its left-hand point, the
// second written without its command letter, which repeats, and the flags
// without the spaces between them.
function circleData(x, y, r) {
  const r1 = dp1(r), d = Math.round(20 * r), radii = r1 + ' ' + r1;
  return 'M' + penAt(x - r, y) + 'a' + radii + ' 0 10' + num(d) + ' 0 ' + radii + ' 0 10-' + num(d) + ' 0';
}

// Outline of a soil patch: an oval with radii `rx`, `ry` at a random angle,
// its radius jittered at a few vertices and smoothed with a closed
// Catmull–Rom spline written as cubic Béziers. Returns the path data and
// the box [x0, y0, x1, y1] of the control points, which holds the curve.
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
  const box = [Infinity, Infinity, -Infinity, -Infinity];
  const point = (x, y) => {
    [box[0], box[1], box[2], box[3]] = [Math.min(box[0], x), Math.min(box[1], y), Math.max(box[2], x), Math.max(box[3], y)];
    return `${f(x)} ${f(y)}`;
  };
  let d = `M${point(pts[0][0], pts[0][1])}`;
  for (let k = 0; k < n; k++) {
    const [p0, p1, p2, p3] = [-1, 0, 1, 2].map((j) => pts[(k + j + n) % n]);
    d += `C${point(p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6)} `
      + `${point(p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6)} ${point(p2[0], p2[1])}`;
  }
  return { d: d + 'Z', box };
}

// Bed of an oxbow lake along points [start, end) of a flat x, y array, as
// `lakeData` and the features on the lake read it. The bed is a parabola
// across the old channel, so the water's half-width where it is `d` deep
// is √d times half the old channel's width in `widths`, capped there by the
// old banks. The depth is `level` less the bed's rise, which is `relief` ×
// 4 × (height − 0.5) for the land heights in `heights`, plus the overbank
// silt in `silts` above its mean over the lake's interior points; the mean
// is part of the fill that sets `level`. Each point's depth moves by its
// fraction in `dry`, if given, towards `level` above the water. `bend` is
// the old channel's curvature, positive where it turns to the left of its
// tangent, smoothed over `BEND_SPAN` points either side and scaled by
// `BEND_GAIN` times the reference width `w` into [−1, 1]. Null if the lake
// has no water.
function lakeBed(pts, heights, silts, widths, start, end, level, relief, dry, w) {
  const m = end - start;
  if (m < 2 || level <= 0) return null;
  let mean = 0;
  for (let k = 0; k < m; k++) mean += widths[start + k] / m;
  const { s, tx, ty } = arcFrame(pts, start, end);
  const depth = new Float64Array(m), turn = new Float64Array(m), bend = new Float64Array(m);
  let silt = 0;
  for (let k = 1; k < m - 1; k++) silt += silts[start + k] / Math.max(1, m - 2);
  // The end points lie under the channel at a cut, swept clear of silt.
  for (let k = 0; k < m; k++) {
    const extra = k > 0 && k < m - 1 ? silts[start + k] - silt : 0;
    const wet = level - 4 * relief * (heights[start + k] - 0.5) - extra;
    depth[k] = wet - (dry ? dry[k] * (wet + level) : 0);
  }
  // The sine of the turn between the tangents either side, over the arc
  // between them.
  for (let k = 1; k < m - 1; k++) {
    turn[k] = (tx[k - 1] * ty[k + 1] - ty[k - 1] * tx[k + 1]) / (s[k + 1] - s[k - 1] || 1);
  }
  for (let k = 0; k < m; k++) {
    const [j0, j1] = [Math.max(1, k - BEND_SPAN), Math.min(m - 2, k + BEND_SPAN)];
    let sum = 0;
    for (let j = j0; j <= j1; j++) sum += turn[j];
    const c = BEND_GAIN * w * sum / Math.max(1, j1 - j0 + 1);
    bend[k] = c < -1 ? -1 : c > 1 ? 1 : c;
  }
  return { pts, widths, start, m, s, tx, ty, depth, bend, mean };
}

// Point and unit normal, to the left of the tangent, at arc length `at`
// along a lake's bed, and the bed's depth and bend there.
function bedAt(bed, at) {
  const { pts, start, m, s, tx, ty, depth, bend } = bed;
  // The last segment starting at or before `at`, by bisection.
  let [j, hi] = [0, m - 2];
  while (j < hi) {
    const mid = (j + hi + 1) >> 1;
    if (s[mid] <= at) j = mid;
    else hi = mid - 1;
  }
  const f = clamp01((at - s[j]) / (s[j + 1] - s[j] || 1)), i = 2 * (start + j);
  const nx = -(ty[j] + (ty[j + 1] - ty[j]) * f), ny = tx[j] + (tx[j + 1] - tx[j]) * f;
  const l = Math.hypot(nx, ny) || 1;
  return {
    x: pts[i] + (pts[i + 2] - pts[i]) * f,
    y: pts[i + 1] + (pts[i + 3] - pts[i + 1]) * f,
    nx: nx / l,
    ny: ny / l,
    width: bed.widths[start + j] + (bed.widths[start + j + 1] - bed.widths[start + j]) * f,
    depth: depth[j] + (depth[j + 1] - depth[j]) * f,
    bend: bend[j] + (bend[j + 1] - bend[j]) * f,
  };
}

// Outline of an oxbow lake on its bed (see `lakeBed`) where the water is
// deeper than `below`, covering arc lengths [from, to] along it. Its
// half-width is `half` × √(depth − `below`) times the old channel's width,
// capped at `half` times that width by the old banks. With `skew`, the
// outline moves towards the outer bank of the old bend by `skew` × the
// bend of the room between it and the outline `room` shallower, as the
// channel scoured its bed deepest against the outer bank.
// Where the bed rises above the water the lake splits into pools, one
// subpath each, and the parabola gives each pool a round tip there. A
// plugged end is a round cap when young, lengthening by `PLUG_TIP` times
// the half-width with `age` in [0, 1]. An end cut by the channel stays a
// round cap, hidden under the water.
function lakeData(bed, from, to, half, age, { below = 0, skew = 0, room = 0 } = {}) {
  if (!bed) return '';
  const { pts, widths, start, m, s, tx, ty, bend, mean } = bed;
  const wide = half * mean;
  const depth = below ? bed.depth.map((d) => d - below) : bed.depth;
  const [a, b] = [Math.max(0, from), Math.min(s[m - 1], to)];
  if (b <= a) return '';
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
      let x = pts[i] + (pts[i + 2] - pts[i]) * f, y = pts[i + 1] + (pts[i + 3] - pts[i + 1]) * f;
      const nx = -(ty[j] + (ty[j + 1] - ty[j]) * f), ny = tx[j] + (tx[j + 1] - tx[j]) * f;
      const scale = thin * shape / (Math.hypot(nx, ny) || 1), d = depthAt(j, f);
      const r = width(j, f, d), h = scale * r;
      if (skew) {
        // The outer bank lies on the right of a bend to the left.
        const o = skew * (bend[j] + (bend[j + 1] - bend[j]) * f) * scale * (width(j, f, d + room) - r);
        x -= nx * o;
        y -= ny * o;
      }
      left.push(x + nx * h, y + ny * h);
      right.push(x - nx * h, y - ny * h);
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
    path += ringData(left, right);
  }
  return path;
}

// Water lilies on a lake's bed over arc lengths [from, to], where the open
// water is deeper than `below`, half-width `half` times the old channel's
// width: clusters of round pads on the open water up to `LILY_DEPTH`
// deeper than that. Two clusters may stand in each `LILY_STEP` widths of
// the oxbow, more often on the inner bank of the old bend, which
// the point bar left shallow. Each opens at its own fill between
// `LILY_START` and `LILY_START` + `LILY_SPREAD` of `age`, and is seeded by
// its place along the whole oxbow, as the scar's tufts are; this piece
// starts `along` into an oxbow of length `total`.
function lilyData(bed, from, to, half, below, age, along, total, w) {
  if (!bed || age < LILY_START) return '';
  const len = bed.s[bed.m - 1], step = LILY_STEP * w, seed = Math.round(total);
  const [a, b] = [Math.max(0, from) + w, Math.min(len, to) - w];
  let d = '';
  for (let j = Math.floor((along + a) / step); j * step < along + b; j++) {
    for (const side of [0, 1]) {
      const h = (n) => hash01(seed, j, side, n);
      if (age < LILY_START + LILY_SPREAD * h(1)) continue;
      const at = (j + h(2)) * step - along;
      if (at < a || at > b) continue;
      // Pools on either side must reach the cluster, or it would hang over
      // the reeds at a pool's tip.
      if (bedAt(bed, at - w).depth <= below || bedAt(bed, at + w).depth <= below) continue;
      const p = bedAt(bed, at);
      if (p.depth <= below) continue;
      // Across the bed to the depth the cluster floats over: the depth at
      // fraction `x` of the half-width is the depth on the centreline less
      // x².
      const x = Math.sqrt(Math.max(0, p.depth - below - LILY_DEPTH * h(3)));
      if (x >= 1) continue;
      const inner = Math.sign(p.bend) || 1;
      const sign = h(4) < 0.5 + 0.35 * Math.abs(p.bend) ? inner : -inner;
      // Drawn in from the depth it floats over by the cluster's reach, so
      // its pads stay over the water.
      const off = sign * Math.max(0, x * half * p.width - LILY_REACH * w);
      const [cx, cy] = [p.x + p.nx * off, p.y + p.ny * off];
      const pads = 2 + Math.floor(4 * h(5));
      for (let q = 0; q < pads; q++) {
        const r = LILY_SIZE * w * (0.6 + 0.4 * h(10 + q));
        const t = 2 * Math.PI * h(20 + q), o = (q ? 0.2 : 0) * w * (0.5 + h(30 + q));
        d += circleData(cx + o * Math.cos(t), cy + o * Math.sin(t), r);
      }
    }
  }
  return d;
}

// Duckweed on a lake's bed over arc lengths [from, to], where the open
// water is deeper than `below`, half-width `half` times the old channel's
// width: mats across the open water, one centred in each `WEED_STEP`
// widths of the oxbow, that start at their own fill between `WEED_START`
// and `WEED_START` + `WEED_SPREAD` and lengthen over `WEED_GROW` of it
// until they meet. Seeded as `lilyData` is.
function weedData(bed, from, to, half, below, age, along, total, w) {
  if (!bed || age < WEED_START) return '';
  const len = bed.s[bed.m - 1], step = WEED_STEP * w, seed = Math.round(total);
  const [a, b] = [Math.max(0, from), Math.min(len, to)];
  let d = '';
  for (let j = Math.floor((along + a) / step) - 1; (j - 1) * step < along + b; j++) {
    const h = (n) => hash01(seed, j, 7, n);
    const grown = smoothstep((age - WEED_START - WEED_SPREAD * h(1)) / WEED_GROW);
    if (grown <= 0) continue;
    const at = (j + 0.5 + 0.3 * (h(2) - 0.5)) * step - along, reach = 0.65 * step * grown;
    const [u, v] = [Math.max(a, at - reach), Math.min(b, at + reach)];
    // A mat's ends that are not the lake's own are round caps inside the
    // water, which goes on beyond them.
    if (v > u) d += lakeData(bed, u === a ? from : u, v === b ? to : v, half, age, { below });
  }
  return d;
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
    left.push(pts[i] - ty[k] * h, pts[i + 1] + tx[k] * h);
    right.push(pts[i] + ty[k] * h, pts[i + 1] - tx[k] * h);
  }
  return ringData(left, right);
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
  // Every tuft but the first moves to its base relative to the last.
  const move = (x, y) => (d ? 'm' + penTo(x, y) : 'M' + penAt(x, y));
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
    const [b, c] = [0.3 * a, 0.5 * a];
    d += move(x - 0.5 * a, y) + 'l' + penTo(x + 0.5 * a, y) + 'm' + penTo(x, y) + 'l' + penTo(x, y - 0.7 * a)
      + 'm' + penTo(x - 0.12 * a, y) + 'l' + penTo(x - 0.12 * a - b, y - c)
      + 'm' + penTo(x + 0.12 * a, y) + 'l' + penTo(x + 0.12 * a + b, y - c);
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
const contourScratch = {
  v: new Float32Array(0), at: new Int32Array(0), segs: new Int32Array(0), used: new Uint8Array(0), loop: new Float64Array(0),
};

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
    // A loop passes each edge at most once.
    scratch.loop = new Float64Array(4 * n);
  }
  const { v, at, segs, used, loop } = scratch;
  v.fill(-level, 0, W);
  v.fill(-level, n - W, n);
  for (let j = 1; j <= rows; j++) {
    const row = (j - 1) * cols + c0;
    v.set(values.subarray(row, row + c1 - c0), j * W + 1);
    v[j * W] = v[j * W + W - 1] = -level;
  }
  // Edge ids: 2k is the edge from corner k rightwards, 2k + 1 downwards. The
  // cell with top-left corner k has the edge 2k + `side[s]` on side s.
  const side = [0, 3, 2 * W, 1];
  // Segments as pairs of edge ids, and up to two segments at each edge.
  at.fill(-1, 0, 4 * n);
  let count = 0;
  for (let j = 0; j < H - 1; j++) {
    // A cell's left corners, and whether they are above the level, are the
    // right corners of the cell before.
    let tr = v[j * W], br = v[j * W + W];
    let up = +(tr > level), down = +(br > level);
    for (let i = 0; i < W - 1; i++) {
      const k = j * W + i;
      const tl = tr, bl = br, left = up * 8 + down;
      tr = v[k + 1];
      br = v[k + W + 1];
      up = +(tr > level);
      down = +(br > level);
      const c = left + up * 4 + down * 2;
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
  // Where the contour crosses an edge, appended to the loop's points.
  let points = 0;
  const point = (id) => {
    const k = id >> 1, i = k % W, j = (k - i) / W;
    const down = id & 1;
    const t = (level - v[k]) / (v[down ? k + W : k + 1] - v[k]);
    loop[2 * points] = (c0 + i + (down ? 0 : t) - 0.5) * cell;
    loop[2 * points + 1] = (j + (down ? t : 0) - 0.5) * cell;
    points++;
  };
  // Chain the segments into closed loops.
  used.fill(0, 0, count);
  let d = '';
  for (let s0 = 0; s0 < count; s0++) {
    if (used[s0]) continue;
    points = 0;
    point(segs[2 * s0]);
    let s = s0, e = segs[2 * s0 + 1];
    while (s >= 0) {
      used[s] = 1;
      point(e);
      const next = at[2 * e] === s ? at[2 * e + 1] : at[2 * e];
      if (next < 0 || used[next]) break;
      s = next;
      e = segs[2 * s] === e ? segs[2 * s + 1] : segs[2 * s];
    }
    if (points > 3) d += lineData(loop, 0, points, true);
  }
  return d;
}

// Points of a flat x, y array with x in [x0, x1]: path data with one subpath
// per run inside the range, and the points themselves.
function cropped(line, x0, x1) {
  const kept = [];
  let d = '', from = 0;
  for (let i = 0; i <= line.length; i += 2) {
    if (i < line.length && line[i] >= x0 && line[i] <= x1) {
      kept.push(line[i], line[i + 1]);
      continue;
    }
    if (i / 2 - from > 1) d += lineData(line, from, i / 2);
    from = i / 2 + 1;
  }
  return { d, pts: kept };
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
    this.stride = 4099;
    this.fill(pts, size);
  }

  // Hash `pts` afresh, in the arrays of the last fill while they are large
  // enough. Returns the grid.
  fill(pts, size) {
    const n = pts.length / 2;
    let buckets = 1;
    while (buckets < 4 * n) buckets *= 2;
    this.pts = pts;
    this.size = size;
    this.mask = buckets - 1;
    if (!(this.start?.length >= buckets + 1)) {
      this.start = new Int32Array(buckets + 1);
      this.next = new Int32Array(buckets);
    }
    if (!(this.items?.length >= n)) {
      this.items = new Int32Array(n);
      this.cell = new Int32Array(n);
    }
    const { start, next, items, cell } = this;
    start.fill(0, 0, buckets + 1);
    for (let i = 0; i < n; i++) {
      cell[i] = this.bucket(Math.floor(pts[2 * i] / size), Math.floor(pts[2 * i + 1] / size));
      start[cell[i] + 1]++;
    }
    for (let b = 0; b < buckets; b++) start[b + 1] += start[b];
    next.set(start.subarray(0, buckets));
    for (let i = 0; i < n; i++) items[next[cell[i]]++] = 2 * i;
    return this;
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
  constructor(bars, [band, ground, fields, lower, swept, upper, trees], svg) {
    this.bars = bars;
    this.grounds = [band, ground, fields, lower, swept, upper, trees];
    this.layers = [bars, ...this.grounds, svg];
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
    // in the bottom layer; the growing band lies in a layer of its own, so
    // keeping a centreline repaints neither the old ones nor the forest.
    this.oldBars = el('g', line, bars);
    this.barLayer = el('g', line, band);
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
    // Fields lie over the forest, in a layer of their own, so a field redraw
    // repaints only the fields and the rest of the floodplain's redraws do
    // not repaint them. They fade in over the nested regions of the grassed
    // ground. A field the fade reaches is drawn again in each region it
    // reaches, clipped to the region, each copy adding to those before, so
    // the copies brighten towards the oldest ground with no seams. A mask of
    // the regions did this before, but Safari renders a mask again whenever
    // its layer repaints, and every field, forest or wall redraw held up a
    // frame by 30–100 ms; a clip of one path is plain geometry.
    // WebKit clips to a clip path of one shape as geometry, with no image.
    const defs = el('defs', {}, fields);
    this.fieldDefs = defs;
    this.fadeRegions = Array.from({ length: FIELD_FADE }, (_, k) =>
      el('path', { 'clip-rule': 'evenodd' }, el('clipPath', { id: `fade-${k + 1}` }, defs)));
    // No fields in the hanger woods on the valley sides: the clip is the
    // whole plane with each wood a hole in it.
    this.scarpClip = el('path', { 'clip-rule': 'evenodd', d: OUTSIDE }, el('clipPath', { id: 'unwooded' }, defs));
    this.fieldLayer = el('g', { 'clip-path': 'url(#unwooded)' }, fields);
    // The rest of the floodplain lies over the fields, the swept ground in
    // a layer of its own between what lies under it and the rest.
    ground = lower;
    // The fields the fade does not reach, and the first copy of those it
    // reaches throughout, then the copies in each region.
    this.parcels = el('g', {}, this.fieldLayer);
    this.parcelSteps = Array.from({ length: FIELD_FADE }, (_, k) =>
      el('g', { 'clip-path': `url(#fade-${k + 1})` }, this.fieldLayer));
    // The furrows and hedges of the fields the fade does not reach, and of
    // those it does, drawn again in each region.
    const strokes = (name, colour, opacity) => {
      const style = { fill: 'none', 'stroke-linecap': 'round', 'stroke-linejoin': 'round', stroke: `rgb(${colour})` };
      const old = el('path', { ...style, 'stroke-opacity': opacity }, this.fieldLayer);
      const faded = el('path', { ...style, id: `fading-${name}` }, defs);
      for (let k = 1; k <= FIELD_FADE; k++) {
        el('use', { href: `#fading-${name}`, 'clip-path': `url(#fade-${k})`, 'stroke-opacity': fadeStep(opacity, k) }, this.fieldLayer);
      }
      return [old, faded];
    };
    this.furrows = strokes('furrows', FURROW, FURROW_OPACITY);
    this.hedges = strokes('hedges', HEDGE, 0.8);
    // Path element of each field drawn, by its key.
    this.fieldPaths = new Map();
    // The outline of each field drawn in several copies, by its key, which
    // the copies use, so a change to the field rewrites one path.
    this.fieldShapes = new Map();
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
    // Ground the channel has swept since the shrub, forest and fields were
    // last contoured, as its centrelines in the newest band's sand, over
    // them and the scroll bars. Without it, ground the channel crosses and
    // leaves within one redraw shows the vegetation that stood there before
    // until the next redraw. It lies under the farmsteads, which are
    // redrawn whenever the water takes a building, and under the crowns, so
    // a crown on newly swept ground stands until the next forest redraw.
    this.sweptLayer = el('g', line, swept);
    ground = upper;
    // Natural levees: nested translucent silt regions, higher ground inside,
    // over the ground and under the farmsteads and trees, so the silt tints
    // the fields and swept sand but not the roofs and crowns.
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
    // Wet ground lies with the plug marsh, and its reed tufts are sized for
    // the channel width at each reset.
    this.reedTufts = el('pattern', { id: 'reed-tufts', patternUnits: 'userSpaceOnUse' }, el('defs', {}, ground));
    const wetOpacity = 1 - (1 - WET_MARSH) ** (1 / WET_LEVELS.length);
    const wetLayer = el('g', { 'fill-rule': 'evenodd' }, ground);
    this.wetMarsh = WET_LEVELS.map(() => el('path', { fill: MARSH, 'fill-opacity': wetOpacity.toFixed(4) }, wetLayer));
    this.wetReeds = el('path', { fill: 'url(#reed-tufts)' }, wetLayer);
    this.wetAt = null;
    // The farmsteads lie over the fields, scroll bars, swept ground, levees
    // and marsh, and under the trees, so crowns overhang the buildings:
    // yards, then mounds, the platforms of ruins, and the roofs with their
    // ridges.
    // Tracks lie over the fields, levees and marsh, and under the
    // farmsteads they lead to and the trees that overhang them.
    // Fishponds lie over the marsh of the plugs they are dug in, and under
    // the tracks that run along their dams: the spoil banks, then the
    // water with its reeds, then the ponds silting up.
    const ponds = el('g', { 'stroke-linejoin': 'round' }, ground);
    this.pondBanks = el('path', { fill: `rgb(${MOUND})`, 'fill-opacity': 0.55 }, ponds);
    this.fishponds = el('path', {
      fill: `rgb(${POND_WATER})`, 'fill-opacity': 0.9, stroke: REEDS, 'stroke-opacity': 0.8,
    }, ponds);
    this.siltLayer = el('g', {}, ponds);
    const tracks = el('g', round, ground);
    // Hollow ways' banks lie under the lanes, deepest on top.
    this.hollowFloor = el('path', { stroke: `rgb(${HOLLOW_FLOOR})`, 'stroke-opacity': 0.35 }, tracks);
    this.hollowBanks = HOLLOW_LEVELS.map(() => [HOLLOW_LIGHT, HOLLOW_SHADE].map((c) => el('path', {
      stroke: `rgb(${c})`, 'stroke-opacity': c === HOLLOW_SHADE ? 0.22 : 0.3,
    }, tracks)));
    this.verges = el('path', { stroke: `rgb(${VERGE})`, 'stroke-opacity': 0.4 }, tracks);
    this.tracks = el('path', { stroke: `rgb(${TRACK})`, 'stroke-opacity': 0.85 }, tracks);
    this.tracksAt = null;
    const farms = el('g', { 'stroke-linejoin': 'round' }, ground);
    this.churchyards = el('path', {
      fill: `rgb(${CHURCHYARD})`, 'fill-opacity': 0.5, stroke: `rgb(${STONE})`, 'stroke-opacity': 0.9,
    }, farms);
    this.yards = el('path', { fill: `rgb(${YARD_EARTH})`, 'fill-opacity': 0.55 }, farms);
    this.mounds = el('path', { fill: `rgb(${MOUND})`, 'fill-opacity': 0.5 }, farms);
    this.ruinLayer = el('g', { fill: `rgb(${PLATFORM})`, stroke: `rgb(${WALLING})` }, farms);
    this.thatch = el('path', { fill: `rgb(${THATCH})`, stroke: `rgb(${WALLING})` }, farms);
    this.tiles = el('path', { fill: `rgb(${ROOF})`, stroke: `rgb(${WALLING})` }, farms);
    this.churches = el('path', { fill: `rgb(${CHURCH_ROOF})`, stroke: `rgb(${WALLING})` }, farms);
    this.towers = el('path', { fill: `rgb(${TOWER})`, stroke: `rgb(${WALLING})` }, farms);
    this.roofRidges = el('path', { fill: 'none', stroke: `rgb(${WALLING})` }, farms);
    this.yews = el('path', { fill: `rgb(${YEW})`, 'fill-opacity': 0.9 }, farms);
    this.buildingsAt = null;
    // Trees lie over the scroll bars, which the canopy hides as it closes:
    // the shadows of every crown, then the crowns in a few tones, then
    // their highlights.
    // Each kind's opacity is the group's, so its dots of different sizes,
    // which lie in different paths, do not build up where they overlap.
    // They lie in a layer of their own, so a crown redraw and a farmstead
    // redraw do not repaint each other.
    ground = trees;
    this.treeLayer = el('g', { fill: 'none', 'stroke-linecap': 'round' }, ground);
    this.treeKinds = [
      el('g', { stroke: `rgb(${TREE_SHADOW})`, opacity: 0.3 }, this.treeLayer),
      ...TREE_TONES.map((c) => el('g', { stroke: `rgb(${c})`, opacity: 0.9 }, this.treeLayer)),
      el('g', { stroke: `rgb(${TREE_LIGHT})`, opacity: 0.4 }, this.treeLayer),
    ];
    this.treeStrips = [];
    // The parish boundary lies over the trees, as a map draws it, and under
    // the water, which hides it where the channel still runs on it.
    this.boundary = el('path', { ...round, stroke: `rgb(${BOUNDARY})`, 'stroke-opacity': 0.75 }, ground);
    this.boundaryAt = null;
    // The bank lies under the lakes, so a lake that meets the channel joins
    // its water with no strip of bank between them.
    // Both are filled outlines at the local width, which a stroke cannot vary.
    // Every lake's reeds lie under the water of all of them, so lakes that
    // meet join as one body of water.
    // Tributary streams lie under the bank, so the channel hides where they
    // meet it. Mill leats lie over it, since their ends run along the water's
    // edge, which the bank would cover; the channel's water still hides
    // where they meet it.
    this.streamLayer = el('g', { ...round, stroke: `rgb(${WATER})` }, svg);
    this.streams = [];
    this.bank = el('g', { fill: `rgb(${WET_SAND})` }, svg);
    this.leatLayer = el('g', round, svg);
    this.defs = el('defs', {}, svg);
    this.plumeIds = 0;
    this.reedLayer = el('g', { fill: REEDS }, svg);
    this.lakeLayer = el('g', {}, svg);
    // Over the water of all lakes, as the reeds lie under it: the deeper
    // water, the plumes, the duckweed and the lilies.
    this.deepLayers = DEPTH_BANDS.map(() => el('g', { fill: `rgb(${DEEP})` }, svg));
    this.plumeLayer = el('g', {}, svg);
    this.weedLayer = el('g', { fill: `rgb(${WEED})`, 'fill-opacity': WEED_OPACITY }, svg);
    this.lilyLayer = el('g', { fill: `rgb(${LILY})` }, svg);
    this.water = el('g', { fill: `rgb(${WATER})` }, svg);
    // Pools, shoals, riffles and mid-channel bars, over the channel's water.
    this.depth = [...POOL_COLOURS, SHALLOW, SHALLOW, SHALLOW, WET_SAND, BAR].map((c) => ({ g: el('g', { fill: `rgb(${c})` }, svg), paths: [] }));
    // Over the water: the darker ponds above working weirs, the weirs and
    // the white water below them, the logs of the jams, and the mill houses.
    this.fordWater = el('path', { fill: 'none', stroke: `rgb(${FORD_WATER})`, 'stroke-opacity': 0.6 }, svg);
    this.fordStones = el('path', { fill: 'none', stroke: `rgb(${BAR})`, 'stroke-opacity': 0.8, 'stroke-linecap': 'round' }, svg);
    this.fordsAt = null;
    this.pondLayer = el('g', { ...line, stroke: `rgb(${CLEAR})`, 'stroke-opacity': 0.2, 'stroke-linecap': 'butt' }, svg);
    this.weirLayer = el('g', { fill: 'none' }, svg);
    this.logs = el('path', { ...round, stroke: `rgb(${LOG})`, 'stroke-opacity': 0.9 }, svg);
    this.houseLayer = el('g', {}, svg);
    this.millShapes = [];
    this.millsAt = null;
    this.bankRuns = [];
    this.waterRuns = [];
    this.lakes = [];
    // The lake the next draw rebuilds first, the first left waiting by the
    // last, and whether the next draw rebuilds every lake.
    this.nextLake = 0;
    this.settling = false;
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
    this.from = this.to = this.pair = null;
    this.scale = null;
    this.window = [x, w];
    this.size = [w, h];
    this.seed = seed;
    this.leveeAt = null;
    this.coarseAt = null;
    this.millsAt = null;
    this.buildingsAt = null;
    this.tracksAt = null;
    this.fordsAt = null;
    this.boundaryAt = null;
    set(this.boundary, 'd', '');
    this.wallsAt = { key: river.wall_version(), t: river.time() };
    this.wallJob = null;
    this.drawPebbles(river.get_param('width'), seed);
    this.drawTufts(river.get_param('width'), seed);
    this.wetAt = null;
    this.plugAt = null;
    this.plugJob = null;
    this.forestAt = null;
    this.forestJob = null;
    this.treeJob = null;
    // The simulated time each staged layer's picture shows, and the time
    // each floodplain layer fell due, if it waits.
    [this.plugShown, this.fieldsShown, this.forestShown] = [null, null, null];
    this.dueSince = [];
    // Each layer's picture as last seen, and when in real time it changed.
    this.lastShown = [];
    this.shownAt = [];
    this.redrawTime ??= [];
    // The first draw after a reset draws the floodplain and the channel
    // whole.
    this.fresh = true;
    this.scrollAt = null;
    this.fieldsAt = null;
    this.fieldsStale = false;
    this.fieldJob = null;
    // The fade's regions and the woods' clip waiting for the next field
    // redraw to put them on the page.
    this.fadeNext = null;
    this.unwoodedNext = null;
    this.cropMask = null;
    this.churchyardsAt = null;
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
      this.fieldsStale = false;
      this.fieldJob = this.fieldStages(river, memory);
    }
    if (this.fieldJob.next().done) {
      this.fieldJob = null;
      this.fieldsShown = this.fieldsAt.t;
    }
  }

  // The stages of a field redraw, which yield whenever the call's
  // `until` has passed. The outlines are set on twins of the field paths
  // as they are chained, and the page is written only in the last stage,
  // so no frame shows half a redraw.
  *fieldStages(river, memory) {
    const due = () => performance.now() > this.until;
    river.sync_holdings();
    // Copies, since later calls can grow memory and detach any view.
    const buf = memory.buffer;
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const owner = new Uint32Array(buf, river.land_owner_ptr(), cols * rows).slice();
    const risk = new Float32Array(buf, river.land_risk_ptr(), cols * rows).slice();
    const hold = new Float32Array(buf, river.holdings_ptr(), 6 * river.holding_slots()).slice();
    // The forest's age grid, as its last redraw drew the fade's regions,
    // which a redraw finishing during this one would change. The regions and
    // the woods' clip go on the page with the fields drawn for them, so
    // `#fields` repaints once for both.
    const age = this.treeBlur?.slice();
    const [fade, unwooded] = [this.fadeNext, this.unwoodedNext];
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
    // A strip's length, lean and offset, and the last cell's field, kept
    // for the cells after while they lie in the same strip and field.
    let strip = { o: -1, k: 0, len: 0, lean: 0, shift: 0 };
    let last = { key: -1, f: 0, field: null };
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
        if (strip.o !== o || strip.k !== k) {
          strip = {
            o, k,
            len: (FIELD_LENGTH[0] + (FIELD_LENGTH[1] - FIELD_LENGTH[0]) * hash01(L.seed, k + 4096, 1)) * w,
            lean: (2 * hash01(L.seed, k + 4096, 2) - 1) * FIELD_LEAN * L.strip,
            shift: hash01(L.seed, k + 4096, 3),
          };
        }
        const m = Math.floor((u + strip.lean * across) / strip.len + strip.shift);
        const key = (o * 4096 + (k + 2048)) * 4096 + (m + 2048);
        if (key !== last.key) {
          let f = index.get(key);
          if (f === undefined) {
            f = fields.length + 1;
            index.set(key, f);
            fields.push({ key, o, k, m, strip, cells: 0, risk: 0 });
          }
          last = { key, f, field: fields[f - 1] };
        }
        lab[j * W + i] = last.f;
        last.field.cells++;
        last.field.risk += risk[g];
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
    for (const [k, f] of fields.entries()) {
      if (k % 64 === 0 && due()) yield;
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
    // No crowns in the fields (`mask`), which keep out of the hanger woods
    // on the valley sides. Each field's first and last region of
    // the fade, from 0, short of the first, to `FIELD_FADE`.
    const [graze, shrub] = [river.get_param('grazing_time'), river.get_param('vegetation_time')];
    const mask = new Uint8Array(W * rows);
    for (const f of fields) [f.first, f.last] = [FIELD_FADE, 0];
    for (let n = 0; n < lab.length; n++) {
      if (n % W === 0 && due()) yield;
      if (!lab[n]) continue;
      const [i, j] = [n % W, Math.floor(n / W)];
      if (this.scarpAt((c0 + i + 0.5) * cell, (j + 0.5) * cell) === 2) continue;
      const f = fields[lab[n] - 1];
      mask[n] = !f.woods;
      const sward = age ? (age[j * cols + c0 + i] - graze) / (shrub - graze) : -1;
      const k = Math.max(0, Math.min(FIELD_FADE, Math.floor(sward * FIELD_FADE) + 1));
      [f.first, f.last] = [Math.min(f.first, k), Math.max(f.last, k)];
    }
    // The furrows of each arable field: lines along its length at even
    // places across its strip, bent by the strip's wave and the reversed
    // S, kept where they cross the field's own cells.
    const furrowData = ['', ''];
    const [head, step] = [FURROW_HEAD * w, FURROW_STEP * w];
    for (const [n, f] of fields.entries()) {
      if (n % 4 === 0 && due()) yield;
      if (f.woods || meadowNow.get(f.key)) continue;
      const L = layouts[f.o], { len, lean, shift } = f.strip;
      if (len <= 2 * head) continue;
      const count = Math.max(1, Math.round(L.strip / (FURROW_GAP * w)));
      const steps = Math.max(2, Math.ceil((len - 2 * head) / step));
      for (let r = 0; r < count; r++) {
        const across = (r + 0.5) / count;
        const [u0, v0] = [(f.m - shift) * len - lean * across, (f.k + across - L.phase) * L.strip];
        let run = [];
        for (let q = 0; q <= steps + 1; q++) {
          let inside = false, x = 0, y = 0;
          if (q <= steps) {
            const t = (head + (len - 2 * head) * q / steps) / len, u = u0 + t * len;
            const v = v0 - FURROW_S * len * Math.sin(2 * Math.PI * t) - FIELD_BEND * w * Math.sin(u / L.wave + L.bend);
            [x, y] = [L.x + u * L.ax - v * L.ay, L.y + u * L.ay + v * L.ax];
            const [i, j] = [Math.floor(x / cell) - c0, Math.floor(y / cell)];
            inside = i >= 0 && j >= 0 && i < W && j < rows && lab[j * W + i] === n + 1;
          }
          if (inside) {
            run.push(x, y);
          } else {
            if (run.length >= 4) furrowData[+(f.first < FIELD_FADE)] += pointsData(run);
            run = [];
          }
        }
      }
    }
    // Trace, simplify and round each boundary once, so neighbouring fields
    // share it exactly.
    const arcs = yield* boundaryArcs(lab, W, rows, due);
    const outlines = new Map();
    const hedgeData = ['', ''];
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
      hedgeData[+(Math.min(fa.first, fb.first) < FIELD_FADE)] += pointsData(pts);
      for (const n of arc.cells) mask[n] = 0;
    }
    // Chain each field's arcs into rings, end to start.
    const drawn = [], shapes = [];
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
      // A copy throughout the field at the opacity of its first region,
      // then one in each region after it.
      const field = fields[f - 1], { first, last } = field;
      const copies = first > 0 ? [[0, field.opacity * first / FIELD_FADE]] : [];
      for (let k = first + 1; k <= last; k++) copies.push([k, fadeStep(field.opacity, k)]);
      if (copies.length === 1) {
        const [[k, opacity]] = copies, key = field.key * (FIELD_FADE + 1) + k;
        const old = this.fieldPaths.get(key);
        drawn.push([key, k, field.colour, opacity, twin(old?.tagName === 'path' ? old : el('path', {}), d)]);
        continue;
      }
      const id = `field-${field.key}`;
      shapes.push([field.key, twin(this.fieldShapes.get(field.key) ?? el('path', { id }), d)]);
      for (const [k, opacity] of copies) {
        const key = field.key * (FIELD_FADE + 1) + k, old = this.fieldPaths.get(key);
        drawn.push([key, k, field.colour, opacity, old?.tagName === 'use' ? old : el('use', { href: `#${id}` })]);
      }
    }
    const hedges = hedgeData.map((d, k) => twin(this.hedges[k], d));
    const furrows = furrowData.map((d, k) => twin(this.furrows[k], d));
    // The writes alone in the last stage, which a call's chaining would
    // otherwise run into.
    yield;
    const keptShapes = new Map();
    for (const [key, shape] of shapes) {
      const old = this.fieldShapes.get(key);
      this.fieldShapes.delete(key);
      if (!old) this.fieldDefs.append(shape);
      else if (old !== shape) old.replaceWith(shape);
      keptShapes.set(key, shape);
    }
    for (const shape of this.fieldShapes.values()) shape.remove();
    this.fieldShapes = keptShapes;
    const kept = new Map();
    for (const [key, k, colour, opacity, path] of drawn) {
      const old = this.fieldPaths.get(key);
      this.fieldPaths.delete(key);
      if (!old) (k ? this.parcelSteps[k - 1] : this.parcels).append(path);
      else if (old !== path) old.replaceWith(path);
      kept.set(key, path);
      set(path, 'fill', `rgb(${colour})`);
      set(path, 'fill-opacity', Math.round(1000 * opacity) / 1000);
    }
    for (const path of this.fieldPaths.values()) path.remove();
    this.meadow = meadowNow;
    this.fieldPaths = kept;
    swap(this.hedges, hedges);
    for (const path of this.hedges) set(path, 'stroke-width', dp1(HEDGE_WIDTH * w));
    swap(this.furrows, furrows);
    for (const path of this.furrows) set(path, 'stroke-width', dp1(FURROW_WIDTH * w));
    if (fade) swap(this.fadeRegions, fade);
    if (unwooded) set(this.scarpClip, 'd', unwooded);
    if (this.fadeNext === fade) this.fadeNext = null;
    if (this.unwoodedNext === unwooded) this.unwoodedNext = null;
    // The fields redraw after each forest redraw, mostly over the same
    // cells, and the crowns go stale only when those change.
    const crop = this.cropMask;
    if (!crop || crop.c0 !== c0 || crop.W !== W || crop.mask.some((v, n) => v !== mask[n])) {
      this.cropMask = { mask, c0, W };
      this.treesStale = true;
    }
  }

  // Draw the valley sides over the window [x0, x0 + vw): the upland's tint
  // under its fields, and on each wall the hachures and the hanger woods,
  // with the clip that keeps fields out of the woods. The core narrows the
  // valley at the window edges to keep bends off them; the walls are drawn
  // at its full width, so that device does not show.
  drawWalls(river, x0, vw, h, seed) {
    const job = this.wallStages(river, x0, vw, h, seed);
    while (!job.next().done);
  }

  // The stages of a wall redraw (see `drawWalls`): each side's lines, then
  // its hachures a row at a time. The page and `wallLines`, which the
  // fields read, change only in the last stage.
  *wallStages(river, x0, vw, h, seed) {
    const w = river.get_param('width'), valley = river.get_param('valley_width') * w;
    if (!(valley > 0 && valley < h)) {
      this.uplandLayer.replaceChildren();
      this.scarpLayer.replaceChildren();
      this.wallLines = null;
      this.unwoodedNext = OUTSIDE;
      this.fieldsStale = true;
      return;
    }
    // Each path as its layer and attributes, put on the page at the end.
    const paths = [];
    const rand = random(seed ^ 0xba1e);
    const step = 0.5 * w, n = Math.ceil(vw / step) + 1;
    const between = ([a, b], u) => a + (b - a) * u;
    const combeData = river.wall_combes();
    const sides = [];
    let clipData = OUTSIDE;
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
      const y = (d) => (side < 0 ? d : h - d);
      // Appends point k at distance `d` from the edge to the flat x, y array
      // `pts`.
      const point = (pts, k, d) => pts.push(x0 + Math.min(k * step, vw), y(d));
      const rim = side < 0 ? 0 : h;
      const upland = [x0, rim];
      foot.forEach((d, k) => point(upland, k, d));
      upland.push(x0 + vw, rim);
      paths.push([this.uplandLayer, { d: lineData(upland, 0, upland.length / 2, true) }]);
      const lit = side > 0;
      // Shade or light over the scarp, in nested bands between fractions
      // of the way from brow to foot.
      const tone = lit ? SCARP_TONE.light : SCARP_TONE.shade;
      const bandOpacity = 1 - (1 - tone) ** (1 / SCARP_BANDS.length);
      let bands = '';
      for (const [a, b] of SCARP_BANDS) {
        const band = [];
        for (let k = 0; k < n; k++) point(band, k, brow[k] + a * (foot[k] - brow[k]));
        for (let k = n - 1; k >= 0; k--) point(band, k, brow[k] + b * (foot[k] - brow[k]));
        bands += lineData(band, 0, band.length / 2, true);
      }
      paths.push([this.scarpLayer, {
        d: bands, fill: `rgb(${lit ? SCARP_LIGHT : SCARP_SHADE})`, 'fill-opacity': bandOpacity.toFixed(3),
      }]);
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
        const ring = [];
        for (let j = k0; j < k1; j++) point(ring, j, woodEdge(j, 1));
        for (let j = k1 - 1; j >= k0; j--) point(ring, j, woodEdge(j, -1));
        woods += lineData(ring, 0, ring.length / 2, true);
      }
      paths.push([this.scarpLayer, { d: woods, fill: `rgb(${FOREST.at(-1).colour})`, 'fill-opacity': WOODS_OPACITY }]);
      clipData += woods;
      // Hachures, sorted into three weights by steepness, each a wedge
      // around its course, thinning down the slope.
      const half = [0, 1, 2].map((k) => 0.5 * HACHURE_WIDTH * w * (0.6 + 0.4 * k));
      const strokes = [[], [], []];
      const at = (line, x) => {
        const t = Math.max(0, Math.min(n - 1, (x - x0) / step)), k = Math.min(n - 2, Math.floor(t));
        return line[k] + (line[k + 1] - line[k]) * (t - k);
      };
      const frac = (x, d) => (d - at(brow, x)) / (at(foot, x) - at(brow, x));
      const gap = HACHURE_GAP * w, e = 0.25 * w;
      for (const row of [0, 1]) {
        yield;
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
          // Offset each point across the course, by the taper at its place.
          const m = pts.length / 2, left = [], right = [];
          for (let k = 0; k < m; k++) {
            const [a, b] = [Math.max(0, k - 1), Math.min(m - 1, k + 1)];
            const tx = pts[2 * b] - pts[2 * a], td = pts[2 * b + 1] - pts[2 * a + 1];
            const len = Math.sqrt(tx * tx + td * td) || 1;
            const r = half[weight] * between(HACHURE_TAPER, k / Math.max(1, m - 1));
            const [ox, od] = [-td / len * r, tx / len * r];
            left.push(pts[2 * k] + ox, y(pts[2 * k + 1] + od));
            right.push(pts[2 * k] - ox, y(pts[2 * k + 1] - od));
          }
          for (let k = m - 1; k >= 0; k--) left.push(right[2 * k], right[2 * k + 1]);
          strokes[weight].push(lineData(left, 0, left.length / 2, true));
        }
      }
      strokes.forEach((list) => paths.push([this.scarpLayer, {
        d: list.join(''), fill: `rgb(${SCARP_SHADE})`, 'fill-opacity': lit ? 0.38 : 0.55,
      }]));
    }
    this.uplandLayer.replaceChildren();
    this.scarpLayer.replaceChildren();
    for (const [layer, attrs] of paths) el('path', attrs, layer);
    this.unwoodedNext = clipData;
    this.wallLines = { x0, step, n, h, w, sides };
    // Which fields the fade reaches changes with the woods they keep out of.
    this.fieldsStale = true;
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
        // Made before the test, so the draws stay in step and the patches
        // in the window keep their places.
        const { d, box } = blobData(rand, cx, cy, r, r * aspect);
        if (box[2] <= x0 || box[0] >= x0 + vw || box[3] <= 0 || box[1] >= h) continue;
        el('path', {
          d,
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

  // One lake's paths, one in each lake layer, and the gradients of its
  // plumes: from each end of its water, and around a delta's head.
  newLake() {
    const grads = ['linearGradient', 'linearGradient', 'radialGradient'].map((kind) => {
      const g = el(kind, { id: `plume${this.plumeIds++}`, gradientUnits: 'userSpaceOnUse' }, this.defs);
      for (const [offset, opacity] of [[0, 1], [0.4, 0.5], [1, 0]]) {
        el('stop', { offset, 'stop-color': `rgb(${PLUME})`, 'stop-opacity': opacity }, g);
      }
      return g;
    });
    const lake = {
      inputs: null,
      grads,
      reed: el('path', {}, this.reedLayer),
      water: el('path', {}, this.lakeLayer),
      deep: this.deepLayers.map((g) => el('path', {}, g)),
      plumes: grads.map((g) => el('path', { fill: `url(#${g.id})` }, this.plumeLayer)),
      weed: el('path', {}, this.weedLayer),
      lilies: el('path', {}, this.lilyLayer),
    };
    lake.paths = [lake.reed, lake.water, ...lake.deep, ...lake.plumes, lake.weed, lake.lilies];
    lake.owned = [...lake.paths, ...grads];
    return lake;
  }

  draw(river, memory, { local, lagging = 0 }) {
    const w = river.get_param('width');
    // The floodplain layers sync their own data, which on a `River` can
    // grow memory, so they go before the views below are built.
    const layers = [
      [this.oldBars, local.showBars], [this.barLayer, local.showBars], [this.sweptLayer, local.showBars],
      [this.leveeLayer, local.showLevees],
      [this.scrollLayer, local.showScrolls],
    ];
    for (const [layer, show] of layers) {
      if (layer.style.display !== (show ? '' : 'none')) layer.style.display = show ? '' : 'none';
    }
    const urgency = clamp01(2 * lagging - 1);
    const floodplain = FLOODPLAIN_BUDGET + (FLOODPLAIN_IDLE_BUDGET - FLOODPLAIN_BUDGET) * urgency;
    this.drawFloodplain(river, memory, local, this.fresh ? Infinity : floodplain);
    this.fresh = false;
    // The floodplain features come as copies, which can grow memory too.
    const [jams, mills, leats] = [river.jams(), river.mills(), river.mill_leats()];
    const [tribs, courses] = [river.tributaries(), river.tributary_courses()];
    const fords = this.fordsAt === river.ford_version() ? null : river.fords();

    river.sync();
    // Views must be rebuilt after every step: each snapshot brings a new
    // buffer, as memory growth would.
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
    // `tween` draws the channel between this centreline and the last. A
    // cutoff shortens the river by a loop, and the channel jumps across it.
    const t = river.time();
    if (this.to?.t !== t) {
      const next = { line: line.slice(), widths: widths.slice(), t, length: lineLength(line) };
      this.from = this.to && this.to.length - next.length < w ? this.to : next;
      this.to = next;
    }
    // Channel points, to find lake points beside it.
    const reach = (MAX_WIDTH + ALONGSIDE_GAP) * w;
    const channel = this.channelGrid = this.channelGrid?.fill(line, reach) ?? new Grid(line, reach);

    this.record(line, river.time(), w, local);
    this.drawBars(river.time(), w, local);

    // Oxbows: a lake that clears and darkens once cut off, plugs from the
    // ends, narrows far more slowly from the banks and turns murky, fading
    // only over the last fifth of its fill, then a marshy scar that fades
    // out. Both plugs and banks advance steadily as it fills, and the water
    // drains first from where the land is high or the most silt has
    // settled, splitting the lake into pools. Reeds fill its shallows, and
    // its deeper water lies towards the old outer bank. Silt plumes come in
    // at its ends and at deltas, lilies open on its shallows, and duckweed
    // covers it late in its fill.
    while (this.scars.length < n) {
      const g = el('g', {}, this.scarLayer);
      const [damp, wet, tufts] = SCAR_OPACITY.map(() => el('path', {}, g));
      set(damp, 'fill', `rgb(${SCAR_DAMP})`);
      set(wet, 'fill', `rgb(${SCAR_WET})`);
      for (const [k, v] of Object.entries({ fill: 'none', stroke: `rgb(${SCAR_TUFT})`, 'stroke-linecap': 'round' })) set(tufts, k, v);
      this.scars.push({ g, damp, wet, tufts, shape: null });
    }
    while (this.scars.length > n) this.scars.pop().g.remove();
    while (this.lakes.length < n) this.lakes.push(this.newLake());
    while (this.lakes.length > n) for (const e of this.lakes.pop().owned) e.remove();
    // Where tributaries end in lakes, at the heads of their deltas.
    const mouths = [];
    for (let j = 0; j < tribs.length; j += 9) {
      if (!tribs[j + 8] || !tribs[j + 4]) continue;
      const i = 2 * (tribs[j + 3] + tribs[j + 4] - 1);
      mouths.push([courses[i], courses[i + 1]]);
    }
    const most = river.get_param('clearance_supply');
    const plumes = PLUME_OPACITY * (PLUME_BASE + (1 - PLUME_BASE) * clamp01((river.supply() - 1) / (most - 1 || 1)));
    // Lakes rebuild in turn within `LAKE_BUDGET`, except one the channel
    // has reshaped, which rebuilds at once.
    const [lakesFrom, budget] = [performance.now(), this.settling ? Infinity : LAKE_BUDGET];
    const first = n ? this.nextLake % n : 0;
    const [x0, vw] = this.window;
    const [lo, hi] = [x0 - 2 * w, x0 + vw + 2 * w];
    let waiting = -1;
    this.settling = false;
    for (let q = 0; q < n; q++) {
      const k = (first + q) % n;
      const age = oxFill[k];
      const scar = this.scars[k], lake = this.lakes[k];
      // An oxbow wholly in the hidden reaches is left blank, and drawn
      // afresh should its elements come to a lake in the window.
      let seen = false;
      for (let i = oxOff[k]; i < oxOff[k + 1] && !seen; i++) seen = oxPts[2 * i] >= lo && oxPts[2 * i] <= hi;
      if (!seen) {
        for (const e of [scar.damp, scar.wet, scar.tufts, ...lake.paths]) set(e, 'd', '');
        scar.shape = null;
        lake.inputs = null;
        continue;
      }
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
        // to these elements when the channel splits or drops an oxbow
        // before it. Forget its inputs, or it would stay blank.
        for (const e of lake.paths) set(e, 'd', '');
        lake.inputs = null;
        continue;
      }
      const [i0, i1] = [oxOff[k], oxOff[k + 1]];
      // Fractions of the old channel's half-width on a level bed: the reeds'
      // outer edge, where the water meets the bank, and the open water's.
      // The bed is a parabola, so the open water's edge lies `below` deeper.
      const outer = 1 - local.oxbowShrink * age;
      const open = Math.max(0, outer - REED_FRINGE[0] - REED_FRINGE[1] * age);
      const level = outer * outer, below = level - open * open;
      // The plugs advance from the ends of the oxbow as cut off, which a
      // piece split off by the channel may lack.
      const plug = PLUG * age * oxLen[k], len = pieceLength(oxPts, i0, i1);
      const [from, to] = [plug - oxAlong[k], oxLen[k] - plug - oxAlong[k]];
      const end = REED_END * (outer - open) * 0.5 * wm;
      const [wFrom, wTo] = [from > 0 ? from + end : from, to < len ? to - end : to];
      const reshaped = !lake.inputs?.startsWith(`${shape} `);
      if (!reshaped && performance.now() - lakesFrom > budget) {
        if (waiting < 0) waiting = k;
      } else {
        // The head of a delta on this piece, by the arc length of the nearest
        // point within a width of the old channel of its centreline.
        let mouth = null;
        for (const [mx, my] of mouths) {
          let [best, at] = [wm * wm, -1];
          for (let i = i0; i < i1; i++) {
            const d2 = (oxPts[2 * i] - mx) ** 2 + (oxPts[2 * i + 1] - my) ** 2;
            if (d2 < best) [best, at] = [d2, i];
          }
          if (at >= 0) mouth = { x: mx, y: my, at: pieceLength(oxPts, i0, at + 1) };
        }
        // The silt and the channel change without reshaping the oxbow.
        let silt = 0;
        for (let i = i0; i < i1; i++) silt += oxSilt[i];
        const { dry, key } = alongside(oxPts, oxWidth, i0, i1, line, widths, channel, w);
        const delta = mouth ? Math.round(mouth.at / (0.1 * w)) : '';
        const inputs = `${shape} ${Math.round(age / LAKE_STEP)} ${silt} ${w} ${local.oxbowShrink} ${local.oxbowRelief} ${key} ${delta}`;
        if (lake.inputs !== inputs) {
          lake.inputs = inputs;
          const bed = lakeBed(oxPts, oxHeight, oxSilt, oxWidth, i0, i1, level, local.oxbowRelief, dry, w);
          const half = 0.5 * w;
          // Open water over arc lengths [u, v], keeping the water's own ends.
          const [a, b] = [Math.max(0, wFrom), Math.min(len, wTo)];
          const section = (u, v, opts = {}) => (Math.min(b, v) > Math.max(a, u)
            ? lakeData(bed, u <= a ? wFrom : u, v >= b ? wTo : v, half, age, { below, ...opts }) : '');
          set(lake.reed, 'd', lakeData(bed, from, to, half, age));
          set(lake.water, 'd', section(a, b));
          DEPTH_BANDS.forEach((band, m) => set(lake.deep[m], 'd',
            section(a, b, { below: below + band, skew: DEEP_SKEW, room: band })));
          // A plume fades along the lake from each end of its water.
          const reach = PLUME_LENGTH * w;
          [[a, 1], [b, -1]].forEach(([at, dir], m) => {
            set(lake.plumes[m], 'd', dir > 0 ? section(a, a + reach) : section(b - reach, b));
            if (!bed) return;
            const [p, q] = [bedAt(bed, at), bedAt(bed, at + dir * reach)];
            for (const [name, v] of [['x1', p.x], ['y1', p.y], ['x2', q.x], ['y2', q.y]]) set(lake.grads[m], name, dp1(v));
          });
          const r = DELTA_PLUME * w;
          set(lake.plumes[2], 'd', mouth ? section(mouth.at - r, mouth.at + r) : '');
          if (mouth) for (const [name, v] of [['cx', mouth.x], ['cy', mouth.y], ['r', r]]) set(lake.grads[2], name, dp1(v));
          set(lake.lilies, 'd', lilyData(bed, wFrom, wTo, half, below, age, oxAlong[k], oxLen[k], w));
          set(lake.weed, 'd', weedData(bed, wFrom, wTo, half, below, age, oxAlong[k], oxLen[k], w));
        }
      }
      const opacity = clamp01(5 * (1 - age)).toFixed(2);
      const clearing = smoothstep(age / CLEARING);
      const murk = smoothstep(1.5 * (age - CLEARING) / (1 - CLEARING));
      const clear = mixRgb(mixRgb(WATER, CLEAR, clearing), SHALLOWS, SHALLOW_TINT * clearing);
      set(lake.water, 'fill', mix(clear, MURKY, murk));
      for (const e of lake.paths) set(e, 'opacity', opacity);
      // The deep water shows once the silt settles, and hides again as the
      // lake turns murky. The plumes show against the clearer water.
      const deep = (DEEP_OPACITY * clearing * (1 - DEEP_MURK * murk)).toFixed(2);
      for (const e of lake.deep) set(e, 'fill-opacity', deep);
      const plume = (plumes * clearing).toFixed(2);
      for (const e of lake.plumes) set(e, 'fill-opacity', plume);
    }
    this.nextLake = Math.max(0, waiting);

    this.drawStreams(tribs, courses, w);
    this.drawMills(river, mills, leats, line, widths, w);
    this.drawJams(jams, w);
    if (fords) this.drawFords(river, fords, w);
  }

  // Device pixels a unit. Read again only after a resize, since reading the
  // window's size after a frame's writes forces a layout.
  pixels() {
    if (!this.scale) {
      const [w, h] = this.size;
      this.scale = Math.min(innerWidth / w, innerHeight / h) * (devicePixelRatio || 1);
      addEventListener('resize', () => { this.scale = null; }, { once: true });
    }
    return this.scale;
  }

  // Draw the channel as it stood at simulated time `at`, between the
  // centrelines of the last two draws. At low speeds the river steps only
  // every few frames, by up to a pixel, so the channel trails it by a step
  // and moves on every frame that would move it by `MIN_MOVE` device
  // pixels. Returns whether it drew.
  tween(river, at) {
    const { from, to } = this;
    if (!to) return false;
    const f = to.t > from.t ? clamp01((at - from.t) / (to.t - from.t)) : 1;
    const w = river.get_param('width');
    // The points to draw: the window, and room for the widest bank and its
    // bar beyond it each side.
    const [x0, vw] = this.window;
    // The feet of a pair's points on the centreline before, found once and
    // only for a frame between the two, since at high speeds a frame
    // mostly draws the last.
    const feet = (pair) => pair.feet ??= footing(pair.from.line, pair.from.widths, pair.line, pair.widths, w, x0 - CHANNEL_MARGIN * w, x0 + vw + CHANNEL_MARGIN * w);
    // How far the drawn channel may lie from this pair's at `drawnF`: a
    // new pair starts from where the last left off, and a cutoff, which
    // starts a pair at its end, always redraws.
    if (this.pair !== to) {
      const last = this.pair;
      const left = last && this.drawnF < 1 ? (1 - this.drawnF) * feet(last).move : 0;
      this.offset = last && from === last && from !== to ? this.offset + left : Infinity;
      [this.pair, this.drawnF, to.from] = [to, 0, from];
    }
    const ahead = f === this.drawnF ? 0 : from === to ? Infinity : f < 1 || to.feet ? Math.abs(f - this.drawnF) * feet(to).move : Infinity;
    const move = this.offset + ahead;
    if (move === 0 || (move * this.pixels() < MIN_MOVE && at !== Infinity)) return false;
    [this.offset, this.drawnF] = [0, f];
    const { line, widths } = f < 1 ? between(feet(to), to.line, to.widths, f) : to;
    const frame = channelFrame(line);
    const shown = span(line, x0 - CHANNEL_MARGIN * w, x0 + vw + CHANNEL_MARGIN * w);
    const waterRuns = channelRuns(line, widths, frame, shown, 0.5);
    const bend = smoothed(frame.curv, 'bend');
    const bankRuns = channelRuns(line, widths, frame, shown, BANK_EDGE, barRim(bend, w));
    resize(this.bankRuns, bankRuns.length, this.bank);
    resize(this.waterRuns, waterRuns.length, this.water);
    bankRuns.forEach((d, k) => set(this.bankRuns[k], 'd', d));
    waterRuns.forEach((d, k) => set(this.waterRuns[k], 'd', d));
    // Each depth band's runs are short stretches, so they share one path,
    // which cut 0.2 ms more of style and paint.
    channelDepth(line, widths, frame, bend, w, shown).forEach((runs, m) => {
      const { g, paths } = this.depth[m];
      resize(paths, 1, g);
      set(paths[0], 'd', runs.join(''));
    });
    return true;
  }

  // Each tributary runs down its combe to the apex of its fan, then along
  // the course the core keeps for it across the floodplain to the water it
  // meets. The course changes only at its lower end, as water meets or
  // leaves it, and when it moves on its fan, so a stream is redrawn only
  // then.
  drawStreams(tribs, courses, w) {
    const n = tribs.length / 9;
    const [x0, vw] = this.window;
    const [lo, hi] = [x0 - 2 * w, x0 + vw + 2 * w];
    resize(this.streams, n, this.streamLayer);
    for (let k = 0; k < n; k++) {
      const [side, mx, my, start, len, version, scale, depth] = tribs.subarray(9 * k, 9 * k + 8);
      const path = this.streams[k];
      let seen = mx >= lo && mx <= hi;
      for (let i = start; i < start + len && !seen; i++) seen = courses[2 * i] >= lo && courses[2 * i] <= hi;
      if (!seen) {
        set(path, 'd', '');
        path.key = null;
        continue;
      }
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
    const [x0, vw] = this.window;
    let d = '';
    for (let k = 0; k < jams.length; k += 8) {
      const [x, y, dx, dy, ix, iy, strength, until] = jams.subarray(k, k + 8);
      if (x < x0 - 2 * w || x > x0 + vw + 2 * w) continue;
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
      for (const shape of this.millShapes) for (const e of shape?.parts ?? []) e.remove();
      // A mill whose leat and pond cannot reach the window has no shape.
      const [x0, vw] = this.window;
      const reach = (Math.max(MILL_REACH, river.get_param('pond_length')) + 2) * w;
      this.millShapes = Array.from({ length: n }, (_, k) => {
        const m = mills.subarray(11 * k, 11 * k + 11);
        return m[0] < x0 - reach || m[0] > x0 + vw + reach ? null : this.millShape(m, leats, w);
      });
    }
    const pond = Math.round(river.get_param('pond_length') / SPACING), last = line.length / 2 - 1;
    for (let k = 0; k < n; k++) {
      const [x, y, ax, ay, working, ruin, at, width, , start, count] = mills.subarray(11 * k, 11 * k + 11);
      const shape = this.millShapes[k];
      if (!shape) continue;
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
    // A box about `along` widths from the centre along the ridge and
    // `across` widths across it, `hl` by `hd` widths.
    const piece = (cx, cy, ux, uy, along, across, hl, hd) => {
      const [bx, by] = [cx + along * w * ux + across * w * uy, cy + along * w * uy - across * w * ux];
      return pointsData([[-1, -1], [1, -1], [1, 1], [-1, 1]].flatMap(([s, r]) => [
        bx + s * hl * w * ux + r * hd * w * uy, by + s * hl * w * uy - r * hd * w * ux,
      ])) + 'Z';
    };
    const ellipse = (cx, cy, rx, ry) => {
      const arc = `A${dp1(rx)} ${dp1(ry)} 0 1 0`;
      return `M${dp1(cx - rx)} ${dp1(cy)}${arc} ${dp1(cx + rx)} ${dp1(cy)}${arc} ${dp1(cx - rx)} ${dp1(cy)}Z`;
    };
    let [yardsOf, churches, towers, yews] = ['', '', '', ''];
    const churchyards = [];
    for (let k = 0; k < data.length; k += 8) {
      const [cx, cy, ux, uy, kind, mound, age, holding] = data.subarray(k, k + 8);
      if (cx < x0 - 4 * w || cx > x0 + vw + 4 * w) continue;
      if (kind === 7) {
        const yard = ellipse(cx, cy, CHURCH_YARD[0] * w, CHURCH_YARD[1] * w);
        const parts = CHURCH_PARTS.map(([at, hl, hd]) => piece(cx, cy, ux, uy, at, 0, hl, hd));
        if (age > 0) {
          ruins.push([parts.join(''), 1 - age], [yard, 1 - age, true]);
          continue;
        }
        yardsOf += yard;
        churchyards.push(cx, cy);
        towers += parts[0];
        churches += parts[1] + parts[2];
        for (const [at, hl] of CHURCH_PARTS.slice(1)) {
          ridges += `M${dp1(cx + (at - hl) * w * ux)} ${dp1(cy + (at - hl) * w * uy)}`
            + `L${dp1(cx + (at + hl) * w * ux)} ${dp1(cy + (at + hl) * w * uy)}`;
        }
        // A yew or two south of the nave, where they stand in most
        // churchyards, placed by the church's site.
        const yewCount = 1 + Math.floor(2 * hash01(Math.round(cx), Math.round(cy), 11));
        for (let y = 0; y < yewCount; y++) {
          const along = -0.6 + 1.2 * hash01(Math.round(cx), Math.round(cy), 12 + y);
          const [yx, yy] = [cx + along * w * ux - 0.72 * w * uy, cy + along * w * uy + 0.72 * w * ux];
          yews += circleData(yx, yy, YEW_RADIUS * w);
        }
        continue;
      }
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
    // Crowns stand clear of the churchyards, which hold only their yews.
    if (`${churchyards}` !== `${this.churchyardsAt}`) {
      this.churchyardsAt = churchyards;
      this.treesStale = true;
    }
    set(this.churchyards, 'd', yardsOf);
    set(this.churches, 'd', churches);
    set(this.towers, 'd', towers);
    set(this.yews, 'd', yews);
    for (const p of [this.churches, this.towers]) set(p, 'stroke-width', stroke);
    set(this.churchyards, 'stroke-width', dp1(0.1 * w));
    set(this.yards, 'd', yardData);
    set(this.mounds, 'd', mounds);
    set(this.thatch, 'd', thatch);
    set(this.tiles, 'd', tiles);
    set(this.roofRidges, 'd', ridges);
    for (const p of [this.thatch, this.tiles, this.roofRidges]) set(p, 'stroke-width', stroke);
    this.ruinLayer.replaceChildren();
    // A ruined churchyard keeps only its wall.
    for (const [d, fade, wall] of ruins) {
      el('path', {
        d, 'fill-opacity': wall ? 0 : (0.3 * fade).toFixed(2), 'stroke-opacity': (0.6 * fade).toFixed(2), 'stroke-width': stroke,
        'stroke-dasharray': `${dp1(0.2 * w)} ${dp1(0.15 * w)}`,
      }, this.ruinLayer);
    }
    this.drawPonds(river, w);
  }

  // The fishponds over the window: each a rounded rectangle along its old
  // channel, its spoil banks around it. A pond kept by its holding holds
  // water edged with reeds; an abandoned one turns from water to marsh over
  // the first half of its silting and fades over the whole.
  drawPonds(river, w) {
    const data = river.ponds();
    const [x0, vw] = this.window;
    const [hl, hd] = [0.5 * POND[0] * w, 0.5 * POND[1] * w];
    const outline = (cx, cy, ux, uy, grow) => pointsData(chaikin(chaikin([[-1, -1], [1, -1], [1, 1], [-1, 1], [-1, -1]]
      .flatMap(([s, r]) => [cx + s * (hl + grow) * ux - r * (hd + grow) * uy, cy + s * (hl + grow) * uy + r * (hd + grow) * ux]),
    true), true)) + 'Z';
    let [banks, water] = ['', ''];
    this.siltLayer.replaceChildren();
    for (let k = 0; k < data.length; k += 5) {
      const [cx, cy, ux, uy, age] = data.subarray(k, k + 5);
      if (cx < x0 - 2 * w || cx > x0 + vw + 2 * w) continue;
      if (age > 0) {
        el('path', {
          d: outline(cx, cy, ux, uy, 0), fill: mix(POND_WATER, POND_MARSH, smoothstep(2 * age)),
          'fill-opacity': (0.85 * (1 - age)).toFixed(2),
        }, this.siltLayer);
        continue;
      }
      banks += outline(cx, cy, ux, uy, POND_BANK * w);
      water += outline(cx, cy, ux, uy, 0);
    }
    set(this.pondBanks, 'd', banks);
    set(this.fishponds, 'd', water);
    set(this.fishponds, 'stroke-width', dp1(0.1 * w));
  }

  // The tracks over the window. Tracks share their ends near the
  // farmsteads, where one search reached the cells of both, so each cell is
  // drawn once: a track is cut into runs of cells not drawn already, each
  // run starting from the cell where it leaves a track drawn before, and
  // each run is simplified and rounded on its own. The runs then meet at
  // the cell centres they share. The stretches of each run, and of each
  // abandoned hollow way, deeper than each of `HOLLOW_LEVELS` gain that
  // level's banks.
  drawTracks(river, w) {
    this.tracksAt = { key: river.track_version(), t: river.time() };
    const [pts, lengths, depths] = [river.track_points(), river.track_lengths(), river.track_depths()];
    const [x0, vw] = this.window;
    const [lo, hi] = [x0 - 2 * w, x0 + vw + 2 * w];
    const drawn = new Set();
    const line = (run) => pointsData(chaikin(chaikin(simplify(run, TRACK_SIMPLIFY * w), false), false));
    const shown = (run) => {
      for (let k = 0; k < run.length; k += 2) if (run[k] >= lo && run[k] <= hi) return true;
      return false;
    };
    let [d, floor] = ['', ''];
    const banks = HOLLOW_LEVELS.map(() => '');
    const hollow = (run, deep) => HOLLOW_LEVELS.forEach((level, b) => {
      let part = [];
      for (let k = 0; k <= deep.length; k++) {
        if (k < deep.length && deep[k] >= level) {
          part.push(run[2 * k], run[2 * k + 1]);
          continue;
        }
        if (part.length >= 4) banks[b] += line(part);
        part = [];
      }
    });
    const flush = (run, deep) => {
      if (run.length < 4 || !shown(run)) return;
      d += line(run);
      hollow(run, deep);
    };
    let start = 0;
    for (const len of lengths) {
      let [run, deep] = [[], []];
      for (let k = start; k < start + len; k++) {
        const [x, y] = [pts[2 * k], pts[2 * k + 1]];
        const key = `${x} ${y}`;
        if (drawn.has(key)) {
          if (run.length) {
            run.push(x, y);
            deep.push(depths[k]);
            flush(run, deep);
          }
          [run, deep] = [[x, y], [depths[k]]];
          continue;
        }
        drawn.add(key);
        run.push(x, y);
        deep.push(depths[k]);
      }
      flush(run, deep);
      start += len;
    }
    const [hpts, hlengths, hdepths] = [river.hollow_points(), river.hollow_lengths(), river.hollow_depths()];
    start = 0;
    for (const len of hlengths) {
      const run = Array.from(hpts.subarray(2 * start, 2 * (start + len)));
      const deep = hdepths.subarray(start, start + len);
      start += len;
      if (run.length < 4 || !shown(run)) continue;
      hollow(run, deep);
      if (deep.some((v) => v >= HOLLOW_LEVELS[0])) floor += line(run);
    }
    set(this.tracks, 'd', d);
    set(this.verges, 'd', d);
    set(this.tracks, 'stroke-width', dp1(TRACK_WIDTH * w));
    set(this.verges, 'stroke-width', dp1(TRACK_VERGE * w));
    set(this.hollowFloor, 'd', floor);
    set(this.hollowFloor, 'stroke-width', dp1(TRACK_VERGE * w));
    banks.forEach((data, b) => this.hollowBanks[b].forEach((bank, side) => {
      // Light from the upper left: the shaded bank lies towards it.
      const shift = dp1((side ? -1 : 1) * HOLLOW_SHIFT * w * Math.SQRT1_2);
      set(bank, 'd', data);
      set(bank, 'transform', `translate(${shift} ${shift})`);
      set(bank, 'stroke-width', dp1(HOLLOW_WIDTH * w));
    }));
  }

  // Each ford: a band of shallow water across the channel from bank to
  // bank, with stepping stones along it.
  drawFords(river, fords, w) {
    this.fordsAt = river.ford_version();
    const [x0, vw] = this.window;
    const [lo, hi] = [x0 - 2 * w, x0 + vw + 2 * w];
    let d = '';
    for (let k = 0; k < fords.length; k += 4) {
      const [ax, ay, bx, by] = fords.subarray(k, k + 4);
      if (Math.max(ax, bx) < lo || Math.min(ax, bx) > hi) continue;
      d += `M${dp1(ax)} ${dp1(ay)}L${dp1(bx)} ${dp1(by)}`;
    }
    set(this.fordWater, 'd', d);
    set(this.fordStones, 'd', d);
    set(this.fordWater, 'stroke-width', dp1(FORD_WIDTH * w));
    set(this.fordStones, 'stroke-width', dp1(0.18 * w));
    set(this.fordStones, 'stroke-dasharray', `${dp1(0.01 * w)} ${dp1(0.3 * w)}`);
  }

  // The parish boundary, once, when the core sets it on the channel's course.
  drawBoundary(river, w) {
    this.boundaryAt = { key: null, t: river.time() };
    const [x0, vw] = this.window, line = river.boundary();
    const [start, end] = span(line, x0 - 2 * w, x0 + vw + 2 * w);
    const pts = chaikin(simplify(Array.from(line.subarray(2 * start, 2 * end)), BOUNDARY_SIMPLIFY * w), false);
    set(this.boundary, 'd', pointsData(pts));
    set(this.boundary, 'stroke-width', dp1(BOUNDARY_WIDTH * w));
    set(this.boundary, 'stroke-dasharray', BOUNDARY_DASH.map((v) => dp1(v * w)).join(' '));
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

  // A pattern of reed tufts for the wet ground, scaled to the channel
  // width, as the marsh sign of survey maps: a short line of water with
  // blades rising from it. The tufts sit on a staggered lattice, jittered,
  // so the wet ground reads as an even stipple.
  drawTufts(w, seed) {
    const rand = random(seed ^ 0x2eed);
    const size = 2 * w;
    set(this.reedTufts, 'width', dp1(size));
    set(this.reedTufts, 'height', dp1(size));
    this.reedTufts.replaceChildren();
    let d = '';
    for (const [u, v] of [[0.25, 0.25], [0.75, 0.75]]) {
      const x = (u + 0.15 * (rand() - 0.5)) * size, y = (v + 0.15 * (rand() - 0.5)) * size;
      const half = 0.22 * w;
      d += `M${dp1(x - half)} ${dp1(y)}h${dp1(2 * half)}`;
      for (const [at, lean] of [[-0.5, -0.25], [0, 0], [0.5, 0.25]]) {
        const h = (at === 0 ? 0.3 : 0.2) * w * (0.85 + 0.3 * rand());
        d += `M${dp1(x + at * half)} ${dp1(y)}l${dp1(lean * h)} ${dp1(-h)}`;
      }
    }
    el('path', {
      d, fill: 'none', stroke: REEDS, 'stroke-width': dp1(0.05 * w), 'stroke-linecap': 'round', 'stroke-opacity': 0.7,
    }, this.reedTufts);
  }

  // Contour the wet ground over the window, blurred twice over 3 × 3 cells so its
  // edge follows the hollows instead of the cell edges. `drawFloodplain`
  // calls it every `COARSE_REFRESH` of simulated time.
  drawWet(river, memory) {
    this.wetAt = { key: null, t: river.time() };
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const wet = new Float32Array(memory.buffer, river.land_wet_ptr(), cols * rows);
    const [c0, c1] = this.windowColumns(cols, cell);
    const blur = this.blur(this.blur(wet, cols, rows, c0, c1, 'wetBlur'), cols, rows, c0, c1, 'wetBlur2');
    WET_LEVELS.forEach((level, k) => set(this.wetMarsh[k], 'd', contourData(blur, cols, rows, cell, c0, c1, level)));
    set(this.wetReeds, 'd', this.wetMarsh.at(-1).getAttribute('d'));
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
    if (!this.wallJob) {
      this.wallsAt = { key: river.wall_version(), t: river.time() };
      const [x, vw] = this.window;
      this.wallJob = this.wallStages(river, x, vw, this.size[1], this.seed);
    }
    do {
      if (this.wallJob.next().done) this.wallJob = null;
    } while (this.wallJob && performance.now() <= this.until);
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
        job: () => this.plugJob,
      },
      {
        shown: this.fieldsShown,
        due: () => this.fieldJob || this.fieldsStale || isDue(this.fieldsAt, river.land_version(), t, FIELD_REFRESH, true),
        run: () => this.drawFields(river, memory),
        job: () => this.fieldJob,
      },
      {
        shown: this.forestShown,
        due: () => this.forestJob || isDue(this.forestAt, this.forestKey(river), t, LEVEE_REFRESH),
        run: () => this.drawForest(river, memory),
        job: () => this.forestJob,
      },
      // The crowns go stale when fields finish after the forest sized them.
      {
        shown: this.fieldsShown,
        due: () => this.treesStale && this.treeBlur,
        run: () => this.redrawTrees(),
        job: () => this.treeJob,
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
        shown: this.wetAt?.t,
        due: () => isDue(this.wetAt, null, t, COARSE_REFRESH),
        run: () => this.drawWet(river, memory),
      },
      {
        shown: this.buildingsAt?.t,
        due: () => isDue(this.buildingsAt, river.building_version(), t, BUILDING_REFRESH),
        run: () => this.drawBuildings(river, w),
      },
      {
        shown: this.tracksAt?.t,
        due: () => isDue(this.tracksAt, river.track_version(), t, TRACK_REFRESH, true),
        run: () => this.drawTracks(river, w),
      },
      {
        shown: this.boundaryAt?.t,
        due: () => !this.boundaryAt && river.has_boundary(),
        run: () => this.drawBoundary(river, w),
      },
      {
        shown: this.wallsAt?.t,
        due: () => this.wallJob || isDue(this.wallsAt, river.wall_version(), t, WALL_REFRESH, true),
        run: () => this.redrawWalls(river),
        job: () => this.wallJob,
      },
    ].map((layer, k) => ({ ...layer, due: () => layer.due() && this.rested(k, layer) }));
  }

  // Whether layer `k` may start a redraw: once `REDRAW_REST` has passed in
  // real time since it last showed a new picture, or at once while its
  // redraw is under way or the river is settling.
  rested(k, layer) {
    if (this.settling || layer.job?.()) return true;
    return performance.now() - (this.shownAt[k] ?? -Infinity) >= 1000 * REDRAW_REST;
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
    // A redraw ends when its layer shows a new picture, though the next
    // may be due at once. Its time counts at most `REDRAW_GAP` ms between
    // calls, so a hidden tab does not count.
    const t = river.time(), now = performance.now();
    const gap = Math.min(REDRAW_GAP, now - (this.floodplainAt ?? now));
    this.floodplainAt = now;
    this.floodplainLayers(river, memory, local).forEach((layer, k) => {
      if (layer.shown !== this.lastShown[k]) [this.lastShown[k], this.shownAt[k]] = [layer.shown, now];
      let since = this.dueSince[k];
      if (since) since.spent += gap;
      if (since && (layer.shown !== since.shown || !layer.due())) {
        const took = since.spent;
        this.redrawTime[k] = REDRAW_MEMORY * (this.redrawTime[k] ?? took) + (1 - REDRAW_MEMORY) * took;
        since = null;
      }
      this.dueSince[k] = layer.due() ? since ?? { t, spent: 0, shown: layer.shown } : null;
    });
    return worked;
  }

  // Simulated time the longest-waiting floodplain redraw has been due.
  behind(river) {
    const t = river.time();
    return Math.max(0, ...this.dueSince.map((since) => since ? t - since.t : 0));
  }

  // The highest speed, in simulated time a second, at which every redraw
  // under way finishes within `lag` of simulated time from falling due, if
  // it takes `margin` times as long in real time as that layer's recent
  // redraws. A redraw past that is expected to take half as long again as
  // it has so far.
  allowance(river, lag, margin) {
    const t = river.time();
    let speed = Infinity;
    this.dueSince.forEach((since, k) => {
      const took = this.redrawTime[k];
      if (!since || took === undefined) return;
      const age = since.spent;
      const left = Math.max(REDRAW_SLACK, margin * took - age, age / 2) / 1000;
      speed = Math.min(speed, Math.max(0, lag - (t - since.t)) / left);
    });
    return speed;
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

  // The stages of a forest redraw: copy the age grid, blur it like the plug
  // grid, contour each of the forest's nested regions, then each region of
  // the fields' fade, a stage for each, then size the tree crowns. A
  // stage that contoured every region of a level at once overran the
  // floodplain's budget. The first stage reads WASM memory, so it runs in
  // the call that starts the redraw. The contours are kept on twins until
  // the last stage, which swaps them onto the page with the crowns, so the
  // vegetation changes in one frame, not in a cascade of partial redraws,
  // and drops the swept ground the new contours have caught up with.
  *forestStages(river, memory) {
    river.sync_forest();
    const synced = river.time();
    const [cols, rows, cell] = [river.field_cols(), river.field_rows(), river.field_cell()];
    const age = new Float32Array(memory.buffer, river.forest_ptr(), cols * rows);
    const [c0, c1] = this.windowColumns(cols, cell);
    if (this.forestAge?.length !== age.length) this.forestAge = new Float32Array(age.length);
    this.forestAge.set(age);
    yield;
    // A buffer of its own, since the plug marsh blurs between stages.
    const blur = this.blur(this.forestAge, cols, rows, c0, c1, 'forestBlur');
    const [shrub, forest] = [river.get_param('vegetation_time'), river.get_param('forest_time')];
    const w = river.get_param('width');
    const forestData = [];
    for (const [k, { at }] of FOREST.entries()) {
      const age = shrub + at * forest;
      const start = 0.5 * (age + (k ? shrub + FOREST[k - 1].at * forest : 0));
      for (let s = 0; s < FOREST_STEPS; s++) {
        yield;
        const level = start + (age - start) * (s + 1) / FOREST_STEPS;
        forestData.push(twin(this.forest[forestData.length], contourData(blur, cols, rows, cell, c0, c1, level)));
      }
    }
    const graze = river.get_param('grazing_time');
    const farmData = [];
    for (let k = 0; k < FIELD_FADE; k++) {
      yield;
      const level = graze + (shrub - graze) * k / FIELD_FADE;
      farmData.push(twin(this.fadeRegions[k], contourData(blur, cols, rows, cell, c0, c1, level)));
    }
    yield;
    const treeArgs = [cols, rows, cell, shrub, forest, w, graze];
    const [crop, yards] = [this.cropMask, this.churchyardsAt];
    const trees = yield* this.treeStages(blur, ...treeArgs);
    yield;
    swap(this.forest, forestData);
    this.fadeNext = farmData;
    this.setTrees(trees);
    // A crown redraw under way reads `treeBlur`, which changes here.
    this.treeJob = null;
    if (this.treeBlur?.length !== blur.length) this.treeBlur = new Float32Array(blur.length);
    this.treeBlur.set(blur);
    this.treeArgs = treeArgs;
    // Which fields the fade reaches follows these regions.
    this.fieldsStale = true;
    // Fields or churches changed since the crowns were sized leave them
    // stale, for `redrawTrees` to size again.
    if (this.cropMask === crop && this.churchyardsAt === yards) this.treesStale = false;
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
  // fields change which crowns stand in crops, a strip at a time while the
  // call's `until` has not passed.
  redrawTrees() {
    if (!this.treeJob) {
      this.treeJob = this.treeStages(this.treeBlur, ...this.treeArgs);
      [this.treeCrop, this.treeYards] = [this.cropMask, this.churchyardsAt];
    }
    let step;
    do step = this.treeJob.next(); while (!step.done && performance.now() <= this.until);
    if (!step.done) return;
    this.treeJob = null;
    this.setTrees(step.value);
    // Fields or churches changed while the crowns were sized leave them
    // stale still.
    if (this.cropMask === this.treeCrop && this.churchyardsAt === this.treeYards) this.treesStale = false;
  }

  // Carry on with work spread over frames, on a frame that draws nothing.
  // Returns whether there was any.
  idle(river, memory, { local }) {
    return this.drawFloodplain(river, memory, local, FLOODPLAIN_IDLE_BUDGET);
  }

  // Bring the floodplain up to date in one call, when the river stops. A
  // pass finishes the redraws under way, which can leave their layers due
  // again, so it repeats until a pass finds nothing to do. The next draw
  // rebuilds every lake that waits.
  settle(river, memory, { local }) {
    this.settling = true;
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
    // Each strip's paths by `crownKey`, and the group of each key's size.
    this.treeStrips = Array.from({ length: count }, () => ({ crowns: [], paths: new Map() }));
    this.crownGroups = new Map();
    this.crownWidth = w;
    const min = CROWN_MIN * w;
    this.crownClasses = Uint8Array.from({ length: Math.ceil(10 * TREE_RADIUS * w) + 1 },
      (_, k) => Math.max(0, Math.round(Math.log(Math.max(k, 0.5) / (10 * min)) / Math.log(CROWN_CLASS))));
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

  // Put the paths from `treeStages` on the page, each in its kind and size
  // class's group.
  setTrees(data) {
    this.treeStrips.forEach(({ paths }, s) => {
      for (const [key, t] of data[s]) {
        const old = paths.get(key);
        if (t === old) continue;
        if (old) old.replaceWith(t);
        else this.crownGroup(key).append(t);
        paths.set(key, t);
      }
    });
  }

  // Key of a dot of kind `kind` and radius `r`, by its size class, looked
  // up by the radius in tenths of a unit.
  crownKey(kind, r) {
    const classes = this.crownClasses, tenths = Math.round(10 * r);
    return 64 * kind + classes[Math.min(classes.length - 1, tenths)];
  }

  // The group that strokes the dots of `key` at their class's diameter.
  crownGroup(key) {
    let g = this.crownGroups.get(key);
    if (!g) {
      const width = 2 * CROWN_MIN * this.crownWidth * CROWN_CLASS ** (key % 64);
      g = el('g', { 'stroke-width': Math.round(100 * width) / 100 }, this.treeKinds[key >> 6]);
      this.crownGroups.set(key, g);
    }
    return g;
  }

  // Size each crown by the blurred growth of the cell under it, a stage
  // for each strip, and return twins of each strip's paths with its crowns,
  // without touching the page.
  *treeStages(blur, cols, rows, cell, shrub, forest, w, farm) {
    const [from, to] = [shrub + FOREST[1].at * forest, shrub + FOREST[2].at * forest];
    const grow = Math.max(1e-6, TREE_GROW * forest);
    const light = this.treeKinds.length - 1;
    const crop = this.cropMask;
    const yards = this.churchyardsAt ?? [];
    const [rx, ry] = [CHURCH_YARD[0] * w, CHURCH_YARD[1] * w];
    const data = [];
    for (const { crowns, paths } of this.treeStrips) {
      if (data.length) yield;
      // Path data by key, each with its own pen, since the keys interleave.
      const d = new Map();
      const dot = (kind, x, y, r) => {
        const key = this.crownKey(kind, r);
        const e = d.get(key);
        if (!e) {
          d.set(key, { d: 'M' + penAt(x, y) + 'h0', x: penX, y: penY });
          return;
        }
        [penX, penY] = [e.x, e.y];
        e.d += 'm' + penTo(x, y) + 'h0';
        [e.x, e.y] = [penX, penY];
      };
      for (const c of crowns) {
        const i = Math.floor(c.x / cell), j = Math.floor(c.y / cell);
        if (i < 0 || i >= cols || j < 0 || j >= rows) continue;
        const k = i - (crop?.c0 ?? 0);
        if (crop && k >= 0 && k < crop.W && crop.mask[j * crop.W + k] && blur[j * cols + i] > farm) continue;
        let yard = false;
        for (let y = 0; y < yards.length && !yard; y += 2) {
          yard = ((c.x - yards[y]) / rx) ** 2 + ((c.y - yards[y + 1]) / ry) ** 2 < 1;
        }
        if (yard) continue;
        const start = from + c.at * (to - from);
        const r = TREE_RADIUS * w * c.size * clamp01((blur[j * cols + i] - start) / grow);
        if (r < TREE_MIN * w) continue;
        dot(0, c.x + 0.15 * r, c.y + 0.2 * r, r);
        for (const [dx, dy, lr] of c.lobes) dot(1 + c.tone, c.x + dx * r, c.y + dy * r, lr * r);
        dot(light, c.x - 0.3 * r, c.y - 0.3 * r, 0.45 * r);
      }
      // Twins of the strip's paths, new ones for keys it had none for, and
      // empty ones for keys it no longer uses.
      const twins = new Map();
      for (const [key, { d: data }] of d) {
        const old = paths.get(key);
        if (old) {
          twins.set(key, twin(old, data));
        } else {
          const t = el('path', { d: data });
          t.attrs = { d: data };
          twins.set(key, t);
        }
      }
      for (const [key, old] of paths) if (!d.has(key)) twins.set(key, twin(old, ''));
      data.push(twins);
    }
    return data;
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
  capture(river, line, t, { local }) {
    this.record(line, t, river.get_param('width'), local);
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

  // Serialise the current frame, every layer, as a standalone SVG file.
  save() {
    const copy = this.svg.cloneNode(true);
    copy.prepend(...[this.bars, ...this.grounds].flatMap((s) => [...s.childNodes]).map((n) => n.cloneNode(true)));
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
const grounds = ['band', 'ground', 'fields', 'lower', 'swept', 'upper', 'trees'].map((id) => document.getElementById(id));
const renderer = new SvgRenderer(document.getElementById('bars'), grounds, svg);
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
