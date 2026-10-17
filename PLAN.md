# Plan: drawing performance

Make the page run smoothly on older phones. The simulation is cheap: a native step takes a median of 0.14 ms (99.9% 0.66 ms, max 1.37 ms; snapshot, `UP=60 DOWN=20`, 24,000 steps, seed 1). The cost is in drawing.

## How it was measured

Headless Chromium (Playwright) on an Apple M1, with the main thread slowed by `Emulation.setCPUThrottlingRate`. Chrome DevTools calls 4× "mid-tier mobile" and 6× "low-end mobile", relative to an average laptop, so 4× from an M1 stands for a somewhat faster phone than the label. The reference case is 4× on an 844 × 390 viewport, a landscape phone, run for 60 s at the default speed after the warm-up. The world is 480 units high and as wide as the screen's aspect ratio, so a landscape phone shows the longest reach and draws three times the SVG nodes of a portrait one.

Three measurements, all on the default seed:

- A line-level CPU profile of the unmodified page (`Profiler.start`, 200 µs sampling).
- A copy of the page whose renderer methods, heavier free functions and `River` calls are wrapped in timers, giving calls, mean and maximum per method, and a log of what ran in each frame over 16 ms. The wrappers raise the mean JS frame from 4.2 to 5.8 ms, so its figures compare items and are not absolute.
- A browser trace (`devtools.timeline`) for style, layout and paint.

Throttling slows only the main thread, and headless Chromium rasterises without a GPU, so paint and raster on a real phone are not measured. Thermal throttling and Safari are not covered either.

## Findings (commit 6bdd2af)

At 4× on 844 × 390 the main thread is busy 43% of the time, and the river keeps its set speed of 5 years a second. Shares of the busy time:

| Work | Share |
|---|---|
| Channel drawing in `draw()`: outline, depth bands, lakes | 51% |
| Browser style, layout, paint and commit | 23% |
| Floodplain layers (`drawFloodplain`) | 15% |
| WASM `advance` | 6% |
| Garbage collection | 3% |

`draw()` runs 20 times a second, at 11.9 ms a call at 4× (2.8 ms unthrottled on 1280 × 800). Of the 3,509 frames, 208 took over 16 ms.

1. The channel is rebuilt in full at every draw: about 136 paths and 120 KB of path data into `#view`, 54,350 `d` writes and 49 MB of strings in 20 s. `set` skips almost none of them (2,700 writes a second of about 2,720 path updates), since resampling moves every point. `dp1` was 16% of profile line ticks, `setAttribute` 14%, and the joins in `channelData` and `bandData` 9%. `channelDepth` (61 ms/s instrumented) and `channelRuns` (48 ms/s) are the largest functions. `sync_line` copies the whole centreline, hidden reaches included, and every run was drawn: at least 20–24% of the `#view` path data lay wholly off-screen on desktop and landscape viewports, and 62% in portrait.
2. Floodplain stages overrun the 4 ms budget, and they cause the p99 spikes: they ran in all 208 long frames, 2,702 ms of their time. Worst calls at 4×: `drawFields` 40.2 ms (mean 10.2), the forest's `treeData` stage 30.5 ms (mean 20.2), `redrawTrees` 28.1 ms (mean 21.2), `drawWalls` 15.9 ms (mean 13.1, no stages), `drawPlugs` 11.1 ms.
3. Browser rendering on the main thread: Layout 23 ms/s, Paint 21, Commit 16, style recalculation 14, Layerize 13, UpdateLayer 9.5. Layout runs once a draw, about 20 times a second, so fewer `#view` writes cut it too.
4. Allocation: `channelData` and `bandData` allocate two `Float64Array`s a run, `smoothed` four arrays a call, and `draw()` builds a new `Grid` for `alongside` each time (its `add` and `scan` are about 3% of line ticks).
5. Warm-up is simulation time: WASM `advance` took 4.85 s of the 5.39 s warm-up at 4× on 844 × 390, and drawing during it (`capture`, `record`) 83 ms.

Not targets: `drawBars`, `record`, `drawMills`, `drawJams` and `drawStreams` are each under 2% of drawing time.

## Changes

- [x] `dp1` writes the whole part and the tenths as integers (7ee714b). Same output over 20 million random values. Node: 2 million calls in 152 ms, from 180 ms. Page at 4× on 844 × 390, three alternating 30 s runs each: mean JS frame 4.3–4.6 ms, from 5.3–5.6; p95 13.5–15.0 ms, from 16.3–17.1; p99 22.3–23.8 ms, from 26.8–32.3. In a 60 s profile the main thread is busy 38.8% of the time, from 42.6%, and `dp1` itself takes 1.1% of it, from 5.2%; `draw()` falls from 23.8% to 20.5%.
- [x] Cull the channel to the window. `span` picks the points from the first inside the window, less `CHANNEL_MARGIN` widths, to the last inside it; `channelRuns` and the depth bands draw only those. Curvature, bends and widths are still computed along the whole river. A frame paused through the warm-up is identical pixel for pixel to 7ee714b's at 1280 × 800, 844 × 390 and 390 × 844. At 4×, three alternating 30 s runs each against 7ee714b: on 844 × 390 the mean JS frame is 4.1–4.2 ms, from 4.4–4.5, and p95 13.2–13.7 ms, from 14.2–14.6; on 390 × 844 the mean is 0.8–0.9 ms, from 1.3, and p95 2.3–2.4 ms, from 3.8–4.0. p99 is unchanged, since the floodplain stages set it.
- [x] Cull the rest of `#view`: not worth doing. After the channel cull, 10–14% of its path data still lies wholly off-screen on desktop and landscape viewports, and 40% in portrait. Most of it is tributary streams in the hidden reaches (on 844 × 390, 14,414 of the 16,894 off-screen characters), which `drawStreams` rewrites only when a course's version changes. The rest is the bank and water within `CHANNEL_MARGIN`.
- [x] Split the crown sizing into a stage per strip (`treeStages`), in the forest redraw and in `redrawTrees`, which runs as a job. A forest redraw that finishes drops a crown job under way (56ea3e1).
- [x] Stage `drawWalls` (`wallStages`): each side's lines, then its hachures a row at a time, with the page and `wallLines` written in the last stage (56ea3e1).
- [x] Yield in the land-use and mask loops of `fieldStages` (56ea3e1). Timing each step of the field job over 60 s showed one running from labelling into the arcs in one call, 22.0 ms; the longest is now 13.5 ms, the final write stage, which must stay whole so no frame shows half a redraw.
- Results of the three, at 4× on 844 × 390. Three alternating 30 s runs before the field yields: mean JS frame 4.1–4.2 ms, from 4.2–4.4; p99 20.3–22.3 ms, from 21.5–22.9. With every renderer method timed over 60 s against 6bdd2af (so `dp1` and the cull are included): frames over 16 ms 55 of about 3,500, from 208; longest `drawFields` 14.9 ms, from 40.2; `redrawTrees` 11.3, from 28.1; `redrawWalls` 14.1, from 15.9 (mean 7.0, from 13.1); `drawForest` 22.1, from 30.5; mean `draw()` 8.7 ms, from 11.9.
- [ ] Split the forest's contour stages. They now lead 45 of the 55 frames over 16 ms (mean 20.9 ms): each stage contours all `FOREST_STEPS` levels of one forest level over the whole window. A stage per level, or per band of rows, would bring them inside the budget.
- [ ] Shorten the field write stage (10–13.5 ms at 4×): it sets `d`, `fill` and `fill-opacity` on every field path. Skip fields whose outline and use have not changed.
- [ ] Write integer coordinates in a ×10 frame (`scale(0.1)` on the layers, stroke widths ×10). In Node, `'' + Math.round(10 * v)` formatted 2 million numbers in 121–142 ms against 224 ms for the old `dp1`, with 13% less text. Measure against the new `dp1` first.
- [ ] Redraw riffles and mid-channel bars every few draws. They follow smoothed bends and widths, which change slowly.
- [ ] Reuse scratch arrays in `channelData`, `bandData` and `smoothed`, and the `alongside` grid between draws.
- [ ] Warm-up, if it is still too long on phones: the cost is in the core's step, or the warm-up could be shorter on a slow device.

Measure each change against the commit before it, alternating old and new runs on the same viewport, since single runs vary: at 8× on 844 × 390 two runs of the same build gave 23 and 43 fps.

# Done: slow change

Two pieces of work, in this order. The first removes floods, which change the map in a single step, and puts steady processes in place of the slow work they do. The second gives each holding a farmstead of small buildings that changes with its fields and with the river. The buildings read the ground the floods leave, so the flood work comes first. Every length is in channel widths and every time in units of 2.5 years.

Baseline to measure against, taken on 3ecd506 (snapshot, `UP=60 DOWN=20`, 24,000 steps, seeds 1–6, all processes on):

| Seed | Mean sinuosity after step 6,000 | Cutoffs (by floods) | Floods | Avulsions | Lakes filled, median time | Step time median / 99.9% / max (ms) |
|---|---|---|---|---|---|---|
| 1 | 1.323 | 48 (3) | 43 | 2 | 24, 234 | 0.139 / 0.643 / 1.128 |
| 2 | 1.392 | 39 (6) | 40 | 2 | 27, 325 | 0.138 / 0.628 / 0.827 |
| 3 | 1.251 | 46 (8) | 33 | 1 | 25, 399 | 0.133 / 0.531 / 0.981 |
| 4 | 1.493 | 52 (8) | 54 | 2 | 37, 283 | 0.152 / 0.775 / 1.047 |
| 5 | 1.273 | 41 (5) | 46 | 3 | 25, 279 | 0.136 / 0.683 / 1.440 |
| 6 | 1.414 | 52 (11) | 58 | 0 | 36, 295 | 0.147 / 0.766 / 1.397 |

Across the six seeds: mean sinuosity 1.358, 278 cutoffs of which floods made 41 (15%), 10 avulsions in 14,400 units. Each row comes from `UP=60 DOWN=20 cargo run --release -p meander-core --example snapshot -- <out_dir> 24000 2000 <seed>`: the sinuosity column of the `step` lines past 6,000, and the counts on the last one.

## 1 Remove floods

Floods arrive as a Poisson process at `flood_rate` (0.02 a unit, one every 125 years on average), each with a magnitude `u` drawn from [0, 1). `Sim::step` does all of the following in the step a flood arrives:

| Effect | Code | After this work |
|---|---|---|
| Neck breaches and chutes, up to `MAX_FLOOD_CUTS` (4) | `flood_cutoff` | Removed. Neck cutoffs at `cutoff_dist` stay. |
| Bend widening | `widen` | Removed. |
| Overbank deposit (levees) | `Erodibility::flood` | Steady deposit, below. |
| Stripping of shrub younger than `vegetation_time` within `scour_reach` | `Erodibility::flood` | Removed. |
| Lake silting | `fill_lakes` | Folded into the steady lake fill rate. |
| Scroll bars on the inner banks | `add_scrolls` | A Poisson process of their own, below. |
| Crevasse splays and avulsion, in the next step | `crevasse.rs` | Removed, with `sync_splays` and the splay drawing. |
| Jam flushes | `flush_jams` | Removed. Jams still break up over `jam_lifetime`. |
| Mills ruined in a great flood | `flood_mills` | Removed. Mills still fall when outflanked or when the bank reaches the leat. |
| Tributary course shifts | `shift_streams` | A Poisson process of their own, below. |

### As built

`Erodibility::lay_overbank` lays the steady deposit over one of `DEPOSIT_EVERY` (50) bands of rows each step, so every cell gains silt once every 5 units (12.5 years). The banded nearest-point search sorts only the centreline points within reach of the band. A pass every 10 steps over the whole grid raised the median step from 0.147 to 0.198 ms, and the 50-step band brought it back to 0.145 ms. Lakes take their bedload in the band-0 step, over the 50 steps since the last.

The rates are the means of the floods the model drew, at `flood_rate` 0.02 and a mean magnitude of 0.5:

| Parameter | Value | From |
|---|---|---|
| `overbank_rate` | 0.003 a unit | `overbank` 0.3 |
| `bedload_rate` | 0.002 a unit | `bedload_fill` 0.2 |
| `scroll_time` | 50 units (125 years) | one set per flood |
| `stream_shift_rate` | 0.0025 a unit (once in 1,000 years per stream) | `stream_avulsion` 0.25 |

Scroll bars draw from `events`. Stream shifts draw from `streams`. A flood's magnitude set only the chance of a shift, so each shift moves a stream as before. `crevasse.rs`, `flush_jams`, `flood_mills`, `widen` and `flood_cutoff` are gone, with their parameters and constants.

### Results

Over the same runs as the baseline:

| Seed | Mean sinuosity after step 6,000 | Cutoffs | Lakes filled, median time | Step time median / 99.9% / max (ms) |
|---|---|---|---|---|
| 1 | 1.303 | 38 | 15, 395 | 0.137 / 0.292 / 0.463 |
| 2 | 1.394 | 47 | 24, 348 | 0.152 / 0.295 / 0.423 |
| 3 | 1.529 | 47 | 23, 351 | 0.157 / 0.297 / 0.522 |
| 4 | 1.606 | 49 | 27, 336 | 0.163 / 0.331 / 0.396 |
| 5 | 1.450 | 56 | 32, 332 | 0.168 / 0.455 / 0.819 |
| 6 | 1.501 | 42 | 24, 351 | 0.157 / 0.487 / 1.850 |

Across the six seeds: mean sinuosity 1.464 against 1.358, and 279 cutoffs against 278, all now at necks. The median time to fill a lake rose from 234–399 units to 332–395. The 99.9% step time fell from 0.53–0.78 ms to 0.29–0.49 ms, since no step lays a whole flood. Single-run maxima vary from run to run on this machine: a run with timers on every call of the step put the largest spikes in `migrate`, `cutoff` and `capture`, none of which this work changed.

Sinuosity stays inside the bounded-sinuosity test, so `cutoff_dist` is unchanged. All 45 tests pass, and the self-intersection test stays clean over seeds 1–16, with the isolated and the default parameters (32 runs). The renderer drops the splay layer; the levees redraw every `LEVEE_REFRESH`, and the forest after each cutoff.

Not checked: whether a median bar still binds in about `vegetation_time` (250 years) now that floods no longer strip young shrub.

- [x] re-run the baseline
- [x] steady deposit, timed
- [x] scroll bars and stream shifts as Poisson processes
- [x] remove the sudden effects and their parameters
- [x] retune, tests, snapshot numbers
- [x] WASM and renderer: drop the splays
- [x] docs

## 2 Farmsteads

### Model

No calibrated model of farmstead growth exists to take whole, so the rules come from historical geography, as the holdings' rules do:

- **Plan types.** Historic England's farmstead characterisation sorts farmsteads into dispersed, linear, L-plan, loose courtyard and regular courtyard plans. R. W. Brunskill's *Traditional Farm Buildings of Britain* reads them as stages of one farmstead: a house gains an outshut, then a detached barn, then ranges that close a yard. Buildings never grow. A farmstead grows by extensions against a wall and by new buildings around the yard.
- **Hamlets.** Under *partible inheritance* (gavelkind in Kent, the Irish *clachan*), an heir often builds beside the parent's farmstead. Holdings already divide above twice `holding_area`, so hamlets form when the heir's farmstead is sometimes placed next to the parent's.
- **Desertion.** Deserted medieval villages (Beresford and Hurst) survive as *house platforms*: low earthworks where buildings stood. Abandoned buildings become platforms that fade, as mill ruins fade now.
- **Placement.** Emilien et al., "Procedural generation of villages on arbitrary terrains" (*The Visual Computer*, 2012), places buildings one at a time by scoring candidate sites. The score here is flood risk, overlap, distance from the yard and alignment with the buildings already there.

### Building near the river

Lowland rivers give a clear pattern for farmsteads near the water. In the Rhine–Meuse delta, settlement since the Bronze Age has stood on the *stream ridges* (*stroomruggen*): the natural levees and crevasse deposits beside the channel, which stand above the *backswamps* (*kommen*) behind them. The backswamps were left as meadow and hayland. Where no ridge stood high enough, Frisian farmers raised their own mounds (*terpen*). English floodplain meadows show the same split: hay barns stand on the meadow, and the houses stand on the gravel terrace or at the foot of the valley side.

Today's flood risk, in `Land::backward`, depends only on distance from the water: `inside / (1 + u²)`, with `u` the distance over `RISK_LENGTH` (6 widths). Under this rule the safest ground is always farthest from the river. Ground height should lower the risk. Cells whose overbank deposit approaches `levee_height` (levee crests, scroll ridges and fans) take a risk lowered by up to `RIDGE_RELIEF`, and filled lakes and backswamps keep the full risk. Farmsteads then string out along the levees beside the river, with meadow behind them. Risk also raises the cost of crossing ground (`RISK_COST`), so this moves holding boundaries too. Check the field layouts after the change.

Risk then sets what a farmstead looks like:

- Below `SEAT_RISK` (0.25): the full sequence up to a regular courtyard, laid along the holding's axis.
- Between `SEAT_RISK` and `FLEE_RISK` (0.5): fewer buildings in a dispersed or linear plan, laid along the levee crest, parallel to the bank, on a raised platform drawn as a contour ring. The platform stays as an earthwork after the buildings go.
- Meadow above `FLEE_RISK`: at most a field barn, and no house.

### Following the river

The buildings follow the river through the risk field, at the holdings' reorganisations (mean interval 150 years), one building at a time:

- **The bank comes closer.** A building whose cell passes `FLEE_RISK` is abandoned at the holding's next reorganisation, nearest the water first, and its replacement goes on the side of the yard away from the water. The farmstead creeps back over several reorganisations. The whole-farmstead move (`Land::refuge`, for a seat past `FLEE_RISK`) stays for a bank that comes faster than this.
- **The bank reaches a building.** It goes into the river in that step, as the channel cuts the fields now.
- **The river moves away.** A holding takes the new bar after `grazing_time` (50 years), as now. A field barn may follow on meadow there after `vegetation_time` (250 years), and houses only once the risk falls below `SEAT_RISK`.
- **Mills.** A working mill adds one cottage to the target of the holding that founded it, and a ruined mill takes it away.
- **Absorption and moves.** The buildings of an absorbed holding, or of a farmstead that moved, become platforms over `RUIN_TIME` (60 units, 150 years).

### Core (`land.rs`)

- A `Building` holds a centre and a long-axis direction in world units, a kind (house, barn, byre, cart shed, field barn, cottage), an optional parent for an extension, and an optional ruin time. Footprints are fixed per kind, at the mill house's scale (`HOUSE`, 1.1 × 0.7 widths). Buildings keep world positions, so a farmstead that moves leaves them standing.
- A holding's target count comes from `area / holding_area` and its seat's risk band. Each reorganisation adds or ruins at most one building towards the target, so change stays gradual.
- A division places the heir's farmstead on the heir's share nearest the parent's seat with chance `HAMLET_CHANCE`, and on the safe cell farthest from it otherwise, as now.
- Every draw comes from the `events` stream, so the river's draws do not change. The work runs only at reorganisations, one holding at a time, and must fit the per-step `BUDGET`.

### WASM, renderer and docs

- `buildings()`: a `Vec<f32>` copy, per building x, y, direction, kind, and ruin age over `RUIN_TIME`, behind `building_version`. Platforms come with the building that stands on them.
- `main.js`: a group in `#ground` above the fields, redrawn on `building_version` within the floodplain budget. Each building is a rectangle with a ridge line, a ruin a faint outline, a platform a contour ring. Clear each yard from the crop mask.
- Tests: no building stands on the channel or past `FLEE_RISK`; the count follows the holding's area; a division can form a hamlet; turning the buildings on leaves the river's course unchanged.
- Update the Landholdings and Floodplain sections of `docs/architecture.md`.

### As built

Risk: `RIDGE_RELIEF` is 0.6, and the relief comes in over the first `RIDGE_CLEAR` (1.5) widths from the water, so the bank keeps its full risk. Over seeds 1 and 3 at 24,000 steps, held ground at risk 0.25–0.5 now stands at a median 0.84–0.97 of `levee_height`, and ground above 0.5 at 0.50–0.58: the moderate band is old levee ground, and the high band young bars and silting lakes. The holdings still found 42–44 farmsteads. The fields look sound in the browser, though the river takes another course there, since holdings found mills.

Where the farmsteads differ from the plan above:

- They draw from a PRNG of their own in `Land`, and a building takes nothing from the search budget. With `hamlet_chance` 0 the holdings and the river then match a run with `farmsteads` off exactly. Hamlets change the holdings, and through the mills the river: on seed 2 the river ran 591 points long against 500.
- A field barn stands on meadow to risk `BARN_RISK` (0.8), not above `FLEE_RISK`, while 15% of the holding is at risk 0.35 or more.
- A courtyard's buildings go round a yard 1.6 widths across on the house's drier side, long wall to the yard. Scoring by distance from the seat alone joined every barn to the end of its house in the same T.
- The miller's cottage belongs to the holding that has the ground at the middle of a working mill's leat.
- The bank takes a building at the end of the next survey, within 40 steps (10 years).
- The yards are drawn over the fields and the tree crowns, so the crop mask is left alone.

Results on seed 1 over 24,000 steps: 178–194 buildings standing on 44–47 holdings, most with 3–5, 28–37 of them on mounds and 2–9 ruins at a time; the farmsteads changed in 901 of the steps. The step time with `hamlet_chance` 0 matches `farmsteads` off: median 0.146–0.149 ms against 0.145–0.148. All 50 tests pass.

- [x] risk from ground height, field layouts checked
- [x] `Building`, targets, placement score
- [x] following the river, hamlets, ruins
- [x] tests, step time
- [x] WASM sync, drawing
- [x] docs

## Left over

- The weir pins one point, and the channel can turn up to 60° about it before the mill counts as outflanked, which draws as a visible corner. A lower limit would ruin mills sooner.
- Mill founding is constant in time. A medieval peak and a later decline could follow the clearance clock.
