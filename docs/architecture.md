# Architecture

## Core (`src/`)

### Step

`Sim::step` resamples the centreline and migrates it with the *Howard–Knutson* kernel. It then smooths and clamps the line and runs the neck cutoff, chutes and capture, resampling after every cut. The kernel's local term reads curvature through a [1, 6, 1] filter, which stops a zigzag from flipping direction each step. Smoothing also moves any point where the line turns back by more than 120°, a hairpin too short for the neck cutoff. Migration is scaled by channel slope (`Sim::slope`), so winding reaches migrate more slowly. A sideways push, `jitter`, keeps the river from straightening. It comes from value noise redrawn every `jitter_time` (5 years) and blended between draws.

### Ground (`field.rs`)

Erodibility is *fractional Brownian motion* (fBm) noise on a grid. The channel stamps its cells as bare point-bar sand. Growth binds sand as shrub at `vegetation_time` (250 years) and stiffens it to forest after a further `forest_time`, faster on deeper overbank silt. Silt is laid at `overbank_rate`, thins away from the bank over `levee_length` and stops at `levee_height`. Growth and deposit each advance one band of rows per step.

### Valley walls (`walls.rs`)

The walls come from a seed stream of their own. Each foot wanders outwards of `valley_width`, and *combes* cut back into it. Banks that press on a wall retreat its foot (`Walls::erode`), which slumps colluvium and bumps `wall_version`.

### Oxbows, chutes and width

A cutoff leaves an *oxbow* lake. It fills to a clay plug over `oxbow_lifetime`, then stays as a scar for `scar_lifetime`. `Sim::capture` takes up an open lake that meets the channel twice, when the lake route is shorter by more than `capture_ratio`. `Sim::chute` cuts across the point bar of a long bend, with chance `chute_rate` scaled by the share of the chord that is bare bar. *Scroll bars* form on inner banks every `scroll_time` (125 years) on average. Local width (`Sim::widths`) follows the bank-pull and bar-push model of Parker et al. (2011). The neck, capture and rework searches share one hashed grid, rebuilt when `line_version` changes.

### Landholdings (`land.rs`)

Holdings are the *geodesic Voronoi cells* of farmsteads. Water is a barrier, and flood risk is the cost of crossing a cell. Risk falls with distance from water and on high deposit (`RIDGE_RELIEF`). *Wet ground* is low reworked ground older than 250 years; it carries at least `WET_RISK`, so it becomes meadow. Every `reorganise_time` (150 years) on average, one holding moves its seat, is absorbed, takes cheaper cells or divides. At `BOUNDARY_TIME` (1,000 years) the centreline is kept as a *parish boundary*, which costs `BOUNDARY_COST` to cross. Holdings take ground only near the page's window (`Sim::window`), so nothing is founded in the hidden reaches.

### Settlement

- Farmsteads (`farm.rs`): each reorganisation changes a farmstead by one building, towards a plan set by the seat's risk against `SEAT_RISK` and `FLEE_RISK`. Ruins fade over `RUIN_TIME`. A division founds a hamlet with chance `hamlet_chance`. After the parish boundary is set, churches are founded on dry seats at least `CHURCH_GAP` apart.
- Fishponds (`ponds.rs`): a chain of ponds dug into the clay plug of a filled oxbow near a complete farmstead.
- Tracks (`tracks.rs`): a search from every farmstead and church, joined as a *greedy spanner* (`DETOUR`). Tracks cross the channel only at fords. A lane in use for `HOLLOW_TIME` (1,000 years) becomes a *hollow way*, and stays as an earthwork after it is abandoned.
- Mills (`mills.rs`): holdings found mills on straight reaches. A *weir* ponds its reach until the channel outflanks it. A *leat* follows the bank, and the mill is ruined when the bank reaches it.

### Other processes

- Clearance: `Sim::supply()` rises from 1 to `clearance_supply` from `clearance_start` (1,500 years) over `clearance_time` (1,000 years). It multiplies overbank silt, lake filling and fan growth.
- Wood (`wood.rs`): mature forest on eroding banks drops jams, which push the flow off the bank and break up over `jam_lifetime`.
- Tributaries (`tributary.rs`): deep combes carry streams. Each builds a gravel fan into the `coarse` grid, speeds migration below its confluence, shifts on its fan and builds a delta into an open lake.

### Randomness

`rng.rs` is the seeded *pseudo-random number generator* (PRNG). Wood, mills and scroll bars draw from `events`, and tributary shifts from `streams`. Walls, farmsteads and the works (churches and ponds) each have a stream of their own. Turning a process off leaves the river's draws unchanged. Only the holdings change the river, through their mills. `ips.rs` derives Ω, Γ and α from the hydraulic parameters.

## WASM (`wasm/`)

`River` wraps `Sim` with hidden reaches of `UPSTREAM` (60 widths) and `DOWNSTREAM` (20 widths). `sync()` copies geometry into flat `f32` buffers exposed as raw pointers. The `sync_*` calls and the `Vec<f32>` accessors can grow WebAssembly (WASM) memory, so the worker's `pack` calls them all before it reads the buffer. The `*_version` counters tell the renderer when to rebuild.

## Terminal (`tui/`)

`make tui` runs the river in a terminal with *ratatui*. It steps `Sim` natively on the main thread, so it needs no worker or snapshot. The world is 70 channel widths tall, enough for the valley and its walls, and as wide as the terminal's aspect allows. A resize keeps the river and rescales the picture to fit.

Each cell shows two pixels as an upper half block, the top one in its foreground colour and the bottom one in its background. `Picture::paint` samples the ground grids at each pixel's centre: soil, gravel, silt, vegetation, oxbow scars and the holdings' fields and hedges. Field strips run along each holding's axis, `STRIP` widths apart. Water and tracks are stamped over the ground at their width, at least one pixel wide. A lake plugs from its ends and greens towards reeds as it fills. A stream runs from the world's edge, down the upland to the head of its combe, then along its course from the core. Ground and water are painted at 3×3 samples a pixel (`SUPERSAMPLE`). `resolve` treats each pixel as an edge between its centre sample's colour and the colour farthest from it. It mixes the two by the share of samples nearer the centre's, weighted by a tent filter, and `SHARPEN` pushes that share towards 0 or 1. An edge then ramps over half a pixel, where a plain mean ramps over a whole one and reads as blur at this size. Lines are `LINE` (1.2) pixels wide and hedges `HEDGE_WIDTH` (1.25), so a line straddling two pixels still dominates one. The ground's colour up to its fields, hedges and valley walls is worked out once a frame for each grid cell in view (`Cell`), and only those three are worked out for each sample. A whole draw takes 9.3 ms on a 200×50 terminal and 23.7 ms on a 400×100 one. Buildings and working mills take whole cells as glyphs.

On a 50-row terminal a pixel is about 6 world units, and a channel width is 8 units, so the channel is one to three pixels wide. Scroll bars, crowns, furrows and hachures fall below a pixel and are not drawn.

## Web (`web/`)

### Worker

The river steps in a dedicated worker (`worker.js`). After each request the worker packs everything the renderer reads into one `ArrayBuffer` and transfers it (`pack` in `snapshot.js`). On the page, `RiverView` answers the same calls as `River` from that snapshot.

### Frame loop (`ui.js`)

`ui.js` owns the panel, the frame loop and the warm-up. It keeps one stepping request in flight, and a reset drops replies to earlier requests. The warm-up returns the centreline every 50 steps for the point bars. A replay link (`?seed=S&tick=N&view=W`) records the same centrelines, so two replays give identical SVG. It carries the window width because the river's bounds follow the window.

The channel trails the river by one step, and `tween` interpolates between the last two centrelines. It is drawn at `river.time() + owed - dt`, so the drawn time never goes back. It redraws only once it could have moved `MIN_MOVE` device pixels.

Floodplain layers share `FLOODPLAIN_BUDGET` a frame, or `FLOODPLAIN_IDLE_BUDGET` on an idle frame. When a redraw falls behind by more than `MAX_LAG` or `LAG_SECONDS`, whichever is longer, the river holds. Before that, the river slows to a pace at which every redraw under way finishes inside the limit (`PACE_LAG`, `PACE_SECONDS`). A layer that has just shown a new picture rests for `REDRAW_REST`. Lake rebuilds share `LAKE_BUDGET`. On a pause, a single step or the end of the warm-up, `settle` finishes every redraw in one frame. `<html data-idle>` marks a paused page with nothing left to draw.

### Layers (`main.js`)

Nine stacked SVGs, from the bottom: `#bars` (old point bars), `#band` (the growing bar band), `#ground` (coarse ground, forest, upland), `#fields`, `#lower` (scars, scarps, scroll bars), `#swept` (swept ground), `#upper` (levees, marsh, ponds, tracks, farmsteads), `#trees` (crowns, parish boundary) and `#view` (channel and lakes). All but `#view` are *compositing layers*. Safari repaints whatever lies under a change in its layer, on the page's thread, so content that changes often is kept apart from content that is costly to paint. Safari also re-renders an SVG mask on every repaint of its layer, so the renderer uses clip paths of one shape each instead.

Redraws run in budgeted stages. Each stage sets path data on detached twins, since Chrome parses path data when it is set. The last stage swaps the twins in, so a layer changes in one frame.

Path data is relative, from a pen held in whole tenths of a unit (`lineData`, `ringData`). `lineData` drops points within `SIMPLIFY` of the line by *Douglas–Peucker simplification*. Tree crowns are dots, zero-length lines with round caps, grouped by radius into classes `CROWN_CLASS` apart.

### Channel and lakes

The channel is a filled outline at the local width, drawn in runs of `RUN` segments and culled to `CHANNEL_MARGIN` beyond the window. Pools, *shoals*, riffles and mid-channel bars come from the smoothed curvature and width, with one path per colour. A lake is drawn on a parabolic bed (`lakeBed`), with reeds over the shallows, deeper bands, silt plumes, lilies and duckweed.

### Floodplain

Fields are strips along each holding's long axis. Arable fields carry *ridge and furrow* (`FURROW_GAP`, `FURROW_S`). Fields fade in from 50 to 250 years after the ground is swept. The forest redraw contours the fade into `FIELD_FADE` nested clip paths, and a fading field is drawn as stacked `<use>` copies of one outline (`fadeStep`). The `#unwooded` clip keeps fields off the hanger woods. A forest or wall redraw sets `fieldsStale`, and the next field redraw puts the new clips on the page with its fields. Crowns are sized a `TREE_STRIP` at a time and cleared from fields by `cropMask`. Farmsteads, marsh, tracks with hollow-way banks, churches, fishponds and the parish boundary all lie in `#upper` or `#trees`.

### Walls, streams and mills

The walls are drawn from `wall_foot` and `wall_combes`, and redraw in stages after a slump. Each scarp is shaded in bands, with hachures drawn as wedges that thin down the slope. A working weir's leat redraws every frame to follow the bank.
