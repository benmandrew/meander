# Architecture

Detailed notes on the model and the renderer, moved out of `CLAUDE.md` so they load only when needed. Times are in simulated units of 2.5 years unless given in years.

## meander-core (`src/`)

### Step

`Sim::step` resamples the centreline and migrates it with the *Howard–Knutson* kernel. Its local term reads the curvature through a [1, 6, 1] filter, since at `dt` a zigzag from point to point otherwise overshoots and flips direction each step. The filter halves the term's hold on kinks, so `smoothing` is 0.045, up from 0.03, which keeps the seed fuzzer free of self-crossings. It then smooths and clamps it and runs the neck cutoff and any capture, resampling after every cut. The rate is scaled by channel slope (`Sim::slope`): the valley slope over the sinuosity within `SLOPE_REACH`, raised to `slope_exponent`, relative to `slope_sinuosity`. Winding reaches migrate more slowly, so necks close later. Migration reads the grid and the wall taper at the bank a point moves into, chosen by the sign of the rate. A recent cut does not hold back a neck whose banks have closed to half the cutoff distance. A random sideways push, `jitter`, keeps the river from straightening as bends drift out of the domain. Each point reads it from value noise over the floodplain, redrawn every `jitter_time` (5 years) and blended between draws, so a push turns gradually and does not flip from step to step. `JITTER_BLEND` scales it to the long-run drift of a fresh push each step.

### Erodibility and vegetation (`field.rs`)

Erodibility is *fractional Brownian motion* (fBm) noise on the grid. Each step stamps the cells under the channel as bare point-bar sand. Growth binds sand as shrub at `vegetation_time` (250 years) and stiffens it to old-floodplain erodibility as forest after a further `forest_time`. It advances one of 10 bands of rows per step, faster on deeper overbank deposit up to `vegetation_height`. Overbank silt is laid at `overbank_rate`, one of `DEPOSIT_EVERY` (50) bands per step. It thins away from the bank over `levee_length`, stops at `levee_height`, and slows erosion on high banks.

### Valley walls (`walls.rs`)

The walls are set at each reset from a seed stream of their own. Each foot wanders outwards of `valley_width`, and *combes* cut back into it. `Sim::floor` gives the floor; the taper reads it narrowed at the pinches, the land survey without them. Banks pressing on a wall retreat its foot (`Walls::erode`), and each half width of retreat slumps colluvium and bumps `wall_version`.

### Oxbows, scroll bars and width

A cutoff stamps an *oxbow* lake. It fills over `oxbow_lifetime` to a clay plug, faster under overbank silt or with bedload at `bedload_rate`, then stays as a scar for `scar_lifetime`. The channel reworks oxbow within `REWORK_RADIUS`, and a new oxbow absorbs older ones under its water. `Sim::capture` takes up an open lake (fill below `CAPTURE_FILL`) that meets the channel twice, when the bypassed channel is longer than the lake route by more than `capture_ratio`. `Sim::chute` cuts a *chute* across the point bar of a bend at least `CHUTE_RATIO` (3) times as long as a chord of 1.5–5 widths. It scans `CHUTE_SCAN` (8) points a step, and cuts with chance `chute_rate` times the supply and the share of the chord that is bare bar, so vegetation binding a bar stops it. The neck search and the rework share one hashed grid, rebuilt only when `line_version` changes. `oxbow_version` counts shape changes, not fills, so `sync` can skip recomputing heights.

*Scroll bars* come every `scroll_time` (125 years) on average along the inner banks of migrating bends, raised by `scroll_height`. Local width (`Sim::widths`) follows the bank-pull and bar-push model of Parker et al. (2011), through `bar_push` and `width_time`. It changes the planform only through the bank reads and the cutoffs.

### Landholdings (`land.rs`)

Holdings are the *geodesic Voronoi cells* of farmsteads, with the channel and oxbows as barriers and flood risk as the cost of crossing a cell. Risk falls with distance from water, and on high deposit by up to `RIDGE_RELIEF`. *Wet ground* is reworked ground that has lain low for over 250 years, its deposit under `WET_DEPTH` (0.7) of the flood level, at least 2 widths from water and off the oxbow scars. Never-swept ground stays dry, since it stands as old floodplain. Wet ground carries a risk of at least `WET_RISK` (0.6), so farmsteads leave it and its fields are meadow. Holdings take up ground once grassed for `grazing_time` (50 years). One at a time, every `reorganise_time` (150 years) on average, a holding moves a threatened seat, is absorbed below 0.3 `holding_area`, takes cheaper cells, or divides above twice `holding_area`. At `BOUNDARY_TIME` (1,000 years) the centreline is kept as a *parish boundary*, which stays put as the channel moves on. Crossing it costs a holding `BOUNDARY_COST` (8 widths) more, so fields settle on it wherever a holding on its far side can reach the ground. Holdings change the river only through their mills.

### Farmsteads (`farm.rs`)

Each reorganisation changes a farmstead by one building, towards a plan set by the seat's risk against `SEAT_RISK`. A building past `FLEE_RISK` or off the holding is abandoned first, and ruins fade over `RUIN_TIME` (150 years). Buildings keep `CLEAR` of tributaries, whose courses `Sim` hands over when they change. Each survey removes buildings the water has reached. A division makes a hamlet with chance `hamlet_chance`. The farmsteads draw from their own *pseudo-random number generator* (PRNG) stream and spend none of the holdings' search budget, so with `hamlet_chance` 0 the holdings and the river run exactly as with `farmsteads` off.

### Tracks and fords (`tracks.rs`)

Tracks join the farmsteads and cross the channel only at fords, since no bridge would stand for long. One search goes out from every farmstead at once, `TRACK_BUDGET` (128) cells a step, costed by flood risk and by `FORD_COST` for each width of channel waded. Its parts meet in 8 bands of rows, one a step. The cheapest meetings are laid first, as a *greedy spanner*: a lane goes in only if the network already laid makes the trip more than `DETOUR` (2) times as long. Ground on the last network costs `TRACK_KEEP` (half), so ways stay in use. A network takes about 340 steps (85 years). The fords are found again on the tracks after every survey of the channel, so they follow the water. Tracks read the land and change nothing else.

### Floodplain processes

- Clearance: `Sim::supply()` rises from 1 to `clearance_supply` from `clearance_start` (1,500 years) over `clearance_time` (1,000 years). It multiplies overbank silt, lake filling and fan growth.
- Wood (`wood.rs`): mature forest on eroding banks drops jams. A jam pushes the flow off its bank and breaks up over `jam_lifetime`, or is lost beyond `JAM_HOLD` of the centreline.
- Tributaries (`tributary.rs`): combes 3 widths deep or more carry a stream. It builds a gravel fan into the `coarse` grid (`coarse_factor`) and speeds migration below its confluence. Its course is re-cut every 10 steps at the first water, so it changes only at its lower end. It shifts on its fan at `stream_shift_rate` (once per 1,000 years) and builds a delta into an open lake at `delta_rate`.
- Mills (`mills.rs`): holdings found mills on straight reaches. A *weir* holds its reach and ponds the reach above until the channel outflanks or leaves it. A *leat*'s ends follow the bank, and the mill is ruined when the bank reaches its course.

### Hydraulics and PRNG streams

`ips.rs` derives Ω, Γ and α from hydraulic inputs, and `rng.rs` is the local seeded PRNG. Falling wood, new mills and scroll bars draw from `events`, tributary shifts from `streams`, and walls and farmsteads from streams of their own. Turning a process off leaves the river's own draws unchanged.

## meander-wasm (`wasm/`)

`River` wraps `Sim` with hidden reaches of `UPSTREAM` = 60 and `DOWNSTREAM` = 20 widths. `sync()` copies geometry into flat `f32` buffers exposed as raw pointers. `sync_scrolls`, `sync_plugs` (time since cutoff per oxbow cell) and `sync_forest` (growth per cell) can also grow WebAssembly (WASM) memory. So `draw` runs the floodplain layers that call them before it builds its own typed-array views. `sync_line` copies only the centreline and widths, for the warm-up's point-bar capture. `jams`, `buildings`, `tributaries` and `mills` return `Vec<f32>` copies, which also allocate. `coarse_ptr` points at the gravel grid, and `mill_version`, `building_version` and `wall_version` tell the renderer when to rebuild.

## web (`web/`)

`ui.js` owns the panel, the frame loop and the warm-up, which it spreads across frames. The panel shows the speed slider, the tick (`River::steps` since the reset, warm-up included) and JavaScript frame-time percentiles.

A replay restarts the river, runs it to a tick and pauses. The Link's `view=W` is the window width in world units, because the river's bounds follow the window and a slightly wider one soon diverges. The warm-up and replays record the centreline every 50 steps from the reset, so two replays of one link give identical Scalable Vector Graphics (SVG). A live run records each frame, so its point bars differ. `ui.js` sets `data-idle` once a paused page has nothing left to draw. The loop draws only after a step or a setting change, and schedules no frames while paused with no floodplain redraw under way. The channel is the exception: while the river runs, it trails the river by one step, and `tween` redraws it every frame between the last two centrelines. Each point of the newer line moves out from its foot on the older one, and a cutoff makes the channel jump across. At the default speed the river steps every third frame by up to a pixel, so the channel would otherwise move at 20 Hz; trailing it costs about 1.2 ms a frame.

`main.js` draws three stacked SVGs: `#bars` (old point bars), `#ground` (floodplain) and `#view` (channel and lakes). The first two are *compositing layers*, so a moving channel does not repaint them. Floodplain layers share `FLOODPLAIN_BUDGET` (4 ms) a frame, or `FLOODPLAIN_IDLE_BUDGET` (10 ms) on an otherwise idle frame, oldest picture first. When a redraw has been due for over `MAX_LAG` (25 years), `ui.js` holds the river until the floodplain catches up. On a pause or a single step, `settle` finishes every redraw in one frame.

### Channel and lakes

The channel is a filled outline at the local width. Only points within `CHANNEL_MARGIN` of the window are drawn, though curvature and widths span the whole river. Pools, riffles and mid-channel bars come from the smoothed curvature and width; the riffles and the bars change slowly, so each redraws one draw in three. Every reed fringe lies under the water of all lakes, so lakes that meet read as one.

### Floodplain

Fields are strips along each holding's long axis. They redraw after a reorganisation, at most every `FIELD_REFRESH`, and write the page only when done, swapping in twins of the paths that changed. Contours fade them in from `grazing_time` to `vegetation_time`, so they follow the channel onto new bars. Forest growth is contoured into shrub and forest, a contour a stage, and sizes the tree crowns, a `TREE_STRIP` at a time; a crop mask clears crowns from fields. Each stage sets its path data on detached twins of the paths, since Chrome parses path data when it is set, and the last stage swaps the twins onto the page. A finished forest redraw drops any crown redraw under way, which would use stale growth. Farmsteads lie under the crowns and redraw on `building_version`, at least every 50 years. Wet ground is blurred twice and contoured into marsh with the survey maps' reed sign, every `COARSE_REFRESH` (50 years). Tracks lie under the farmsteads and redraw when they change; a cell that several tracks share is drawn once. The parish boundary is drawn once, as a dash-dot line over the crowns and under the water.

### Valley walls, streams, jams and mills

The walls are drawn from `wall_foot` and `wall_combes` without the pinches, which only keep bends off the window edges. They redraw after a slump, at most every 50 years, in budgeted stages, and the page and the wall lines the field mask reads change only in the last stage. Each scarp is shaded in nested bands, darker on the top wall, which faces away from the upper-left light, and lighter on the bottom wall. Its hachures are wedges that thin down the slope, as on old survey maps. Tributaries redraw when a course changes. Leats lie under the water, and fords, jams and mills over it. A working weir follows the channel, and its leat redraws each frame to the bank.
