# Architecture

Detailed notes on the model and the renderer, moved out of `CLAUDE.md` so they load only when needed. Times are in simulated units of 2.5 years unless given in years.

## meander-core (`src/`)

### Step

`Sim::step` resamples the centreline, computes Menger curvature, migrates points along their normals using the Howard–Knutson upstream-weighted kernel (an O(n) forward recurrence), smooths, clamps to the domain, then runs the neck cutoff and any capture. It resamples again after every cut. Flow runs from index 0 (upstream) to the last index. Geometry is `f64`.

The model draws no discrete floods, which changed the map in a single step. Overbank silt and lake filling run at steady mean rates, and scroll bars and tributary shifts come as Poisson processes, so the floodplain changes a little at a time.

The rate is scaled by the channel slope (`Sim::slope`): the valley slope divided by the sinuosity of the reach `SLOPE_REACH` (12) widths either side, raised to `slope_exponent` (0.5, Chézy velocity) and taken relative to a reach of sinuosity `slope_sinuosity` (1.25, the mean measured without the feedback). A winding reach migrates more slowly, so necks close later.

Migration reads the grid and the valley-wall taper at the bank a point moves into, chosen by the sign of the rate alone, not at the centreline. A recent cut no longer holds back a neck whose banks have closed to half the cutoff distance.

### Erodibility and vegetation (`field.rs`)

`field.rs` holds the fBm erodibility grid, at half a width per cell. Each step stamps the cells under the channel as fresh point-bar sand with no vegetation growth. Growth, in age-equivalent time, decides when shrub binds the sand (at `vegetation_time`) and when forest stiffens it to the old floodplain's erodibility (a further `forest_time`). Each step advances one of 10 bands of rows, so every cell grows every 10 steps, at a rate set by the cell's overbank deposit, from 0.6 on bare bar sand to 1.6 at `vegetation_height`, so a median bar binds in about `vegetation_time`.

Overbank silt is laid at `overbank_rate`, the mean of what floods of uniform magnitude laid when the model drew them. Each step lays one of `DEPOSIT_EVERY` (50) bands of rows, so a cell gains silt every 5 units (12.5 years). The deposit thins away from the bank over `levee_length` and stops at the flood level `levee_height`. It slows erosion where banks are high and is drawn as levee contours.

### Valley walls (`walls.rs`)

`walls.rs` sets the valley walls at each reset, from a stream of the seed of their own so the river's draws are unchanged: each wall's foot wanders up to 2.5 widths outwards of `valley_width`, never inwards, and combes cut back into it up to 4.5 widths. `Sim::floor` gives the floor between the feet; the taper reads it narrowed at the pinches, the land survey without them, as drawn.

Bluff erosion: banks pressing on a wall retreat its foot (`Walls::erode`, up to 8 widths), and each half width of retreat slumps colluvium and bumps `wall_version`.

### Oxbows and capture

A cutoff stamps the oxbow footprint as a lake that fills to a clay plug. Each step also cuts an oxbow where the channel centreline crosses one of its segments or comes within `REWORK_RADIUS` (0.1 widths) of a point, so a channel alongside a lake hides it under the water before removing it. A new oxbow absorbs the parts of older oxbows under its water, leaving each remnant one point inside it so the lakes join.

`Sim::capture` lets the channel take up an open lake (fill below `CAPTURE_FILL`) whose banks meet the channel's at two places, when the channel between them is longer than the route through the lake by more than `capture_ratio`; the lake becomes channel and the bypassed reach a new oxbow. The lake's ends are plugged for `END_PLUG` widths, and a route that turns through a right angle or crosses the channel is rejected.

Lake silt is read only from cells still stamped as lake; each lake fills over `oxbow_lifetime`, faster where overbank silt settles in it or bedload enters a connected entrance at `bedload_rate`, then stays for `scar_lifetime` as a scar. Each oxbow keeps the widths of the channel it was cut from, for its footprint, its bedload entrance and its drawn lake and scar.

The neck search and the oxbow and scroll rework share one hashed grid, rebuilt only when `line_version` shows the centreline has moved. `oxbow_version` counts changes to oxbow shapes, not fills, so `sync` can skip recomputing their heights.

### Scroll bars and width

Sets of scroll bars come as a Poisson process with mean interval `scroll_time` (50 units, 125 years), each along the inner banks of the migrating bends (`Sim::scrolls`). Their ridges raise the ground by `scroll_height`, so vegetation takes to them first.

Each point carries a local width (`Sim::widths`, in multiples of `width`), after the bank-pull and bar-push model of Parker et al. (2011): the outer bank retreats at the migration rate times its erodibility, the inner bank advances at `bar_push` times the rate, and the width relaxes to 1 over `width_time`. The centreline keeps the Howard–Knutson rate, so width changes the planform only through the bank reads and the cutoffs, which fire when the banks meet.

### Landholdings (`land.rs`)

`land.rs` shares the ground between medieval landholdings, each the geodesic Voronoi cell of a farmstead over the grid: the channel and oxbow ground are barriers, and flood risk raises the cost of crossing a cell. Risk falls with distance from the water, and on high ground by up to `RIDGE_RELIEF` (0.6) of that, in proportion to the overbank deposit's height over `levee_height`. The relief comes in over the first `RIDGE_CLEAR` (1.5) widths from the water, so the bank keeps its full risk. Old levee ground and scroll ridges beside the river then stand at moderate risk, and young bars and silting lakes at high risk. Holdings take up ground beside them once grass has grown on it for `grazing_time` (20 units, 50 years) and keep ground the river cuts them off from. Each reorganises at random, with mean interval `reorganise_time` (60 units, 150 years), one at a time: it moves a threatened farmstead, is absorbed below 0.3 `holding_area`, takes cells it reaches more cheaply than their holder by a margin, and divides above twice `holding_area`. Holdings read the river and change it only through their mills.

### Farmsteads (`farm.rs`)

Each holding raises a farmstead of small buildings at its seat, one change at each reorganisation, so a farmstead grows over centuries. Buildings never grow; a farmstead grows by an outshut against a wall and by new buildings. The plan follows the flood risk at the seat. At or below `SEAT_RISK` it is a courtyard of up to seven buildings (house, barn, outshut, byre, shed, byre, shed), the first `1 + 4a` of them for a holding of `a` times `holding_area`, laid square to the holding's axis around a yard on the house's drier side. Above `SEAT_RISK` it is a row of up to three along the bank, square to the rise in risk, on mounds. A holding with a working mill on its ground keeps a miller's cottage, and one with 15% of its ground at risk 0.35 or more keeps a field barn on that meadow, to risk 0.8.

At a reorganisation the holding abandons its worst threatened building: one past `FLEE_RISK`, or on ground it no longer holds. It then raises one building towards its plan, or abandons the newest beyond it. Sites lie against the existing buildings, or on the sides of the yard, and are scored by distance from where the plan wants them, by six times the risk, and by a random part, so new buildings go to the driest side and a farmstead creeps back from an advancing bank. A farmstead that moves or is absorbed leaves its buildings as ruins, which fade over `RUIN_TIME` (60 units, 150 years). No building is sited within `CLEAR` (0.15 widths) of a tributary, whose course `Sim` hands to the land whenever it changes, drawn 0.15 to 0.3 widths wide. A new house whose seat lies beside a stream stands off it, at the nearest clear site within `HOUSE_SHIFT` (1.5 widths), and still serves that seat. The end of each survey removes the buildings and ruins the water has reached, a stream among them, so a stream that shifts across a farmstead takes the buildings in its way. A division places the heir's farmstead with chance `hamlet_chance` (0.5) on the nearest safe cell 4 widths or more from the parent's seat, making a hamlet.

The farmsteads draw from their own PRNG and take nothing from the holdings' search budget, so with `hamlet_chance` 0 the holdings and the river run exactly as with `farmsteads` off.

### Floodplain processes

Five floodplain processes, each with parameters in `Params`:

- Clearance: `Sim::supply()` rises from 1 to `clearance_supply` (3) from `clearance_start` (600) over `clearance_time` (400), multiplying overbank silt, lake filling and fan growth. Overbank silt buries scroll bars where it nears the flood level.
- Wood (`wood.rs`): mature forest on eroding banks drops trees as jams, which stay where they lodge, push the flow off their bank, and break up over `jam_lifetime`. A jam is lost when the channel leaves it `JAM_HOLD` (1.5) widths from the centreline or runs over it.
- Bluff erosion: see the valley walls above.
- Tributaries (`tributary.rs`): combes 3 widths deep or more carry a tributary that builds a gravel fan into the `coarse` grid (erodibility × `coarse_factor` on wholly coarse ground) and speeds migration for 30 widths below its confluence. Each stream keeps a course, routed by Dijkstra's search through low ground and leaning downstream. Every 10 steps the course is cut at the first water it meets and, if it ends dry, run on from its end, so it changes only at its lower end. It moves on its fan as a Poisson process at `stream_shift_rate` (once every 1,000 years on average). A stream that ends in an open lake builds a delta into the bed at `delta_rate`, which fills the lake and dries the stream's end so the course runs on across it, and it does not feed the channel's bars.
- Mills (`mills.rs`): holdings found mills on straight reaches; a weir holds its reach, ponds the reach above (slower, wider, siltier) and is ruined when the channel outflanks or leaves it. A leat keeps the course it was dug along, but its ends follow the bank, and the mill is ruined when the bank reaches the course.

### Other modules

`ips.rs` derives Ω, Γ and α from hydraulic inputs. `rng.rs` is a local seeded PRNG. Falling wood, new mills and scroll bars draw from a second stream, `events`, and tributary shifts from a third, `streams`, so turning a process off leaves the river's own draws unchanged.

## meander-wasm (`wasm/`)

`meander-wasm` wraps `Sim` as `River`. The simulated river is longer than the visible window: hidden reaches of `UPSTREAM` = 60 and `DOWNSTREAM` = 20 widths let bends develop before they come into view.

`sync()` copies geometry into flat `f32` buffers exposed as raw pointers. `sync_scrolls`, `sync_plugs` and `sync_forest` can grow memory too, so `draw` runs the floodplain layers that call them before it builds its own views. `sync_forest` copies the vegetation growth of each cell, which `main.js` contours into nested shrub and forest regions, each fading in over several contours, and uses to size seeded tree crowns on young and mature forest. `sync_plugs` copies the time since cutoff of each cell still stamped as oxbow, which `main.js` draws as a soft-edged marsh that fades with the scar to a faint trace. The scar itself is a tapered crescent of damp ground with a wetter core and survey-map marsh tufts. `sync_line` copies only the centreline and its widths, for the warm-up's point-bar capture.

The floodplain features come as `Vec<f32>` copies, which also allocate: `jams` (8 floats each), `buildings` (8), `tributaries` (8, with each course's start and length in `tributary_courses`), `mills` (11, with each leat's start and length in `mill_leats`). `coarse_ptr` points at the gravel grid, and `mill_version`, `building_version` and `wall_version` tell the renderer when to rebuild.

## web (`web/`)

Plain ES modules with no bundler. `ui.js` owns the control panel, the frame loop and the warm-up (spread across frames). `index.html` preloads `ui.js`, the wasm-bindgen glue and the WASM binary, so they download alongside `main.js` and not one after another, and shows "Loading…" in the overlay until the warm-up takes it over. The panel shows only the speed slider and the mean, p95 and p99 of the JS time of the last 600 frames. The frame loop draws only when a step was taken or a setting changed, and `set` skips attribute writes whose value has not changed.

`main.js` draws into three stacked SVGs: `#bars` holds the old point bars, `#ground` the floodplain, and `#view` the channel and lakes. The first two are compositing layers, so a moving channel does not repaint them. The floodplain layers share a budget of `FLOODPLAIN_BUDGET` (4 ms) a frame, or `FLOODPLAIN_IDLE_BUDGET` (10 ms) on a frame that draws nothing else. The layer whose picture is oldest goes first, so no layer waits on another that falls due more often. When a redraw has been due for more than `MAX_LAG` (10 units, 25 years), `ui.js` holds the river until the floodplain catches up, so a slow machine runs below the set speed. When the river is paused or stepped once, `settle` finishes every redraw under way in one frame, so the last changes land together.

### Channel and lakes

The channel is a filled outline at the local width in runs of 24 segments, overlapping by a segment so no seam shows, with the inner bank clamped short of the centre of tight bends. Its edges, and those of the pools, riffles and mid-channel bars, are cubic Béziers that leave each point along the centreline's tangent, so a tight bend draws as a curve and not as a corner at each point; a segment that turns less than `CURVE_TURN` (about 3°) stays straight, which keeps the path data short. Only the points from the first within `CHANNEL_MARGIN` (3 widths) of the window to the last are drawn, though the curvature, bends and widths are computed along the whole river, so the hidden reaches cost no path data and the picture is unchanged. Pools, riffles and mid-channel bars are drawn over the water from the curvature and local width, both smoothed along the river. Every lake's reed fringe is stroked in a layer under the water of all lakes, so lakes that meet read as one body of water.

### Floodplain

Seeded soil patches are drawn as vector blobs typed by the erodibility noise. Fields are laid out in strips along each holding's long axis from its seed, traced along the cell edges of the label map into arcs shared by neighbouring fields, simplified and rounded, and redrawn after a reorganisation finishes, at most every `FIELD_REFRESH`. A field redraw runs within the floodplain's frame budget and writes the page only when it finishes. Meadow takes ground at high flood risk and arable the drier ground. Fields are masked by nested contours that fade them in from `grazing_time` (50 years), where holdings take up grassed ground, to `vegetation_time` (250 years), so the channel cuts them as it migrates and they follow it onto the bars it leaves. Tree crowns in crop fields are skipped on farmland, by a per-cell crop mask; crowns in woods and beside hedges stay. Crowns are sized a strip of `TREE_STRIP` widths at a time, in the forest redraw and again when new fields change the mask; a forest redraw that finishes drops a crown redraw under way, which would otherwise write crowns sized from the old growth.

Coarse ground is contoured as a gravel tint and a pebble pattern every 50 years.

The farmsteads lie over the fields, oxbow scars and scroll bars and under the tree crowns, which overhang them, and under the translucent levee silt and plug marsh. Each standing building is a tiled or thatched roof at its core footprint with a ridge line (an outshut has none), on an elliptical mound where the core raised one, and each farmstead's yard is the rounded hull of its buildings but its field barn, in trodden earth. A ruin is a dashed house platform fading over its 150 years. They are redrawn when `building_version` changes, and at least every 50 years.

### Valley walls

The walls are drawn from the core's feet (`wall_foot`) and combes (`wall_combes`), without the pinches, which exist only to keep bends off the window edges. The upland's tint lies under the fields. Hachures, shaded for light from the upper left, run down each scarp over the fields, as on old survey maps, and hanger woods on the steeper stretches and in combes hide the fields and let tree crowns stand. The walls are redrawn after the bluffs slump, at most every 50 years, in stages within the floodplain's frame budget: each side's lines, then its hachures a row at a time. The page and the wall lines that the field mask reads change only in the last stage.

### Streams, jams and mills

Under the bank and the lakes `draw` lays the tributary streams along the core's courses, winding in bends set by the distance from the mouth and redrawn only when a course changes. Each mill's leat lies over the bank and under the water, since its ends run along the water's edge. Over the water it lays the logs of each jam, and each mill's house, pond and weir. A working weir is laid across the channel as it now runs, since the channel turns about it, and its leat is redrawn each frame to the bank. A ruin fades out over its 500 years with a dashed weir, a dry leat and a roofless house.
