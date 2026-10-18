# Plan: floodplain features

Six changes, in this order, one commit each. Measure each against seeds 1–12 at 24,000 steps (`UP=60 DOWN=20`), with `make test` after any change to the dynamics.

- [x] Rebalance the mills. Over seeds 1–12, mills lowered mean sinuosity after step 6,000 from 1.555 to 1.439, the largest effect of any process. Half came from the backwater: with `weir_backwater` at 0 it was 1.480. `weir_backwater` is now 0.25, and a weir is outflanked at a turn of 45° (`OUTFLANK_COS`), not 60°, which also removes the corner the weir drew. Sinuosity with mills is now 1.498, with 1.9 mills working on average against 2.3. Step time is unchanged.
- [x] An old boundary on a past course. The centreline at `BOUNDARY_TIME` (1,000 years) is kept as a parish boundary and drawn as a dash-dot line. A hard barrier would leave bars that form beyond it unfarmed, since the river cuts them off from their own parish's holdings, so crossing it costs `BOUNDARY_COST` (8 widths) instead. On a test valley this cut the cells held across a boundary from their farmstead from 2,606 to 976.
- [x] Tracks between farmsteads, with temporary fords where they cross the channel. No bridges: the channel moves too far. The network is found in the core, so replays match: a search from every farmstead, then a greedy spanner over the places the searches meet (`tracks.rs`). Over seeds 1–3 it holds 44–49 tracks and 3–9 fords at 24,000 steps, and adds 0.015 ms to the median step (0.161–0.181 ms, from 0.146–0.165).
- [x] Chute cutoffs, as a steady process across point bars (`Sim::chute`, `chute_rate` 0.005 a unit times the supply). Over seeds 1–12 at 24,000 steps, chutes make 370 of 758 cutoffs, and mean sinuosity is unchanged (1.461 against 1.462 without them): chutes take bends that would otherwise close at the neck. Step time is unchanged.
- [x] Wet ground: low ground away from the channel drawn as wet meadow and reed. Reworked ground silts fast (90% of vegetated ground stands above a third of the flood level), so wet ground is the reworked ground lowest against that, older than 250 years. Never-swept ground stays dry, or the whole valley would start as marsh. Over the middle half of the window at 24,000 steps it covers 1.3% of the floor on seed 1 and 11% on seed 3. Step time is unchanged (median 0.179 ms against 0.174 ms over seeds 1–3, within run-to-run noise).
- [x] Valley sides that read as slopes: shade or light over each scarp in nested bands, deepest over the steep middle, and hachures drawn as wedges that thin from 1.5 to 0.25 of their width down the slope. Drawing only; walls still redraw at most every 50 years.

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
- Baseline on f3957f4, after the boundary, tracks, wet ground and valley sides, with every renderer method timed over 60 s: 101 frames over 16 ms, the forest redraw in 44 of them. Its stages reached 15.4 ms contouring a forest level and 14.3 ms in the last, which wrote 370 KB of forest contours, 230 KB of farmland mask and 600 KB of crowns. `drawWet`, new since the last profile, is not staged: it blurs twice and contours three levels in one call, and ran in 6 long frames.
- [x] Split the forest's contour stages: a stage per contour, and the copy of the age grid apart from its blur. Timing the last stage's writes showed Chrome parsing path data as it is set, 5–7.7 ms for the forest contours, 2.8–4.2 ms for the farmland and 1.6–10.5 ms for the crowns. Each stage now sets its path data on a detached twin of the path (`twin`), and the last stage swaps the twins onto the page (`swap`), so the vegetation still changes in one frame. Timed, the longest forest stage is the blur, 6.5 ms, from 15.4; the last stage 1.0 ms, from 14.3. Three alternating 30 s runs each against f3957f4: frames over 16 ms 34–47, from 54–67; p99 17.1–17.5 ms, from 20.0–21.1; longest frame 22.6–25.3 ms, from 28.9–34.6; mean JS frame unchanged at 4.5–4.7 ms. Paused frames at ticks 3,000 and 9,000 are identical pixel for pixel.
- [x] Shorten the field write stage. `set` already skipped unchanged fields: a redraw rewrites 61–99 of about 290 outlines, 38–62 KB, in 1.3–4.7 ms, and the hedges in up to 1.5 ms. The rest of the 13.1–14.4 ms was outline chaining: the last stage starts in a fresh call and chains outlines up to the call's budget, 10 ms on an idle frame, before it writes. The outlines and hedges are now set on twins as they are chained, and the writes have a stage of their own, 1.4 ms at most. Frame times are within run-to-run noise, since the channel's `draw()` is in all 60 long frames of a timed run: six alternating 30 s pairs against 8454e15 gave 21–69 frames over 16 ms against 22–75, and p99 16.5–19.0 ms against 16.5–19.6. Paused frames are identical pixel for pixel.
- [x] Integer coordinates in a ×10 frame: not worth doing. In Node, 2 million numbers took 80–86 ms as `'' + Math.round(10 * v)` against 84–86 ms with the new `dp1`, so only the 15% less text would count. In a 30 s profile at 4× on 844 × 390 the main thread was busy 38.6% of the time, with `dp1` at 1.0% and `setAttribute` at 4.2%, so parsing 15% less path data would save under 1% of the time, for a change to every layer and stroke width.
- [x] Redraw riffles and mid-channel bars every few draws: each once in `DEPTH_EVERY` (3) draws, the two in different draws, and both on the first draw after a reset and on a paused river. Since 049cb79 the channel redraws on every frame, so at 60 Hz and the default speed a riffle lags the channel by at most a sixth of a year. The numbers below were measured on f3957f4, before that change, when the channel redrew 20 times a second. Timed, `channelDepth` takes a mean of 1.34 ms, from 1.81, and `draw()` 8.56 ms, from 9.52. Three alternating 30 s runs each against c2e001d: mean JS frame 4.27–4.38 ms, from 4.47–4.82; p95 12.8–13.4 ms, from 13.7–14.6; p99 16.2–16.5 ms, from 17.2–18.3; frames over 16 ms 21–24, from 35–52. Paused frames are identical pixel for pixel.
- [x] Reuse scratch arrays in `channelData`, `bandData` and `smoothed` (`scratch`, by name), and the `alongside` grid between draws (`Grid.fill`). In 30 s profiles at 4× on 844 × 390, self time in `channelData` fell from 2.0% to 1.6% and in `bandData` from 1.8% to 1.6%; garbage collection stayed at 1.2%. Frame times are within noise, with the machine under other load: three alternating 30 s runs each against b5a5c34 gave a mean JS frame of 4.22–4.70 ms against 4.43–4.79 on 844 × 390, and 1.11–1.17 ms against 1.14–1.23 on 390 × 844. Paused frames are identical pixel for pixel on both. The next allocation in a draw is `lakeData`'s `add`, 1.3% of the profile, which writes its bank points with `toFixed(1)` into arrays of strings.
- Results of the four on 556ebd0, which redraws the channel on every frame since 049cb79. Three alternating 30 s runs each at 4×: on 844 × 390, mean JS frame 8.4–9.0 ms, from 9.7–10.6; p99 19.4–21.1 ms, from 25.6–29.1; frames over 16 ms 116–170, from 193–268; longest frame 25.3–32.1 ms, from 37–54. On 390 × 844, mean 1.74–1.88 ms, from 1.97–2.09. Paused frames at ticks 3,000 and 9,000 are identical pixel for pixel on both.
- [ ] The channel, now redrawn on every frame, is again the largest cost: main's mean JS frame at 4× on 844 × 390 rose from 4.9 ms on f3957f4 to 9.7–11.0 ms. Each frame interpolates the centreline (`between`), rebuilds its frame and outlines, and rewrites every channel path.
- [ ] Warm-up, if it is still too long on phones: the cost is in the core's step, or the warm-up could be shorter on a slow device.

Measure each change against the commit before it, alternating old and new runs on the same viewport, since single runs vary: at 8× on 844 × 390 two runs of the same build gave 23 and 43 fps.

## Path data and redraw rate

Measured in Chrome on an Apple M1, headless, with GPU rasterisation. Static frames are seed 7 at tick 4,000 (1280 × 800) and seed 3 at tick 14,000 (844 × 390). The per-redraw benchmark redraws each layer in full 16 times on a frozen replay (seed 7, tick 4,000, 1280 × 800, two runs). Every floodplain redraw then re-rasterised all of `#ground`. Whole-page runs are seed 7 at 1280 × 800, 10 s after the warm-up, two alternating runs each. All changes are in `web/main.js` unless stated.

- [x] Relative path data, from a pen held in whole tenths of a unit (`penAt`, `penTo`, `penOff`, `num`, `pair`, `sep`, `lineData`, `ringData`). Each point lands where `dp1` put it, in fewer characters. Every subpath starts with an absolute `M`, so data from separate calls can be joined. Circles (`circleData`) are two relative arcs with the flags written without spaces. Valley walls, scars, lakes, contours, bars (`cropped`) and polylines all go through it. `edgeData` writes relative `l` and `c`, with a command letter only where it changes. With the circle compaction, path data on the seed 7 static frame fell from 1,792 KB to 1,410 KB.
- [x] Simplification. `lineData` leaves out points within `SIMPLIFY` (0.15 units) of the line through the kept neighbours, by Douglas–Peucker (`keepPoints`, scratch buffers, no allocation). That is about 0.25 px on a 1280 px desktop window 768 units wide, and 0.37 device px on a landscape phone at 3 device px to the CSS px. Channel edges with curves are not simplified.
- [x] Tree crowns as dots: each circle is a zero-length line with round caps stroked at its diameter, about 12 characters where two arcs took about 46. Radii are rounded to classes `CROWN_CLASS` (2^(1/4)) apart, within 9%, from `CROWN_MIN` (0.025 widths), looked up from a table by the radius in tenths. Each strip has one path per kind and class, in a group per class that sets the stroke width. Each kind's opacity moved onto its group, so dots of different classes do not build up where they overlap, nor across strips as the old per-path fill-opacity allowed. Classes 2^(1/6) apart gave 840 crown paths and about 0.7 ms more paint a forest redraw; 2^(1/4) brought the forest redraw's page-thread time back to the baseline's. Crowns on seed 7: 483 KB to 110 KB.
- [ ] Crown clusters look slightly flatter than before. Flagged to the user, not settled.
- [x] Channel runs: `RUN` 96, up from 24. A channel redraw's page-thread time fell by about 0.4 ms (seed 7, 1280 × 800), GPU time unchanged. One path for the whole channel cut 0.5 ms but added 0.3 ms of GPU time. The depth bands (pools, shoals, riffles, mid-channel bars) share one path per colour, which cut 0.2 ms more of style and paint. The last run stops at the end of the shown span; before, it drew up to `RUN` points beyond the window.
- [x] Redraw rest: a floodplain layer rests `REDRAW_REST` (0.5 s real time) after showing a new picture, unless its redraw is under way or the river is settling. Its refreshes are set in simulated time, so at full speed the forest fell due six times a second. Resting is not counted as due (`rested`, inside each layer's `due`), so it does not slow the river. Against the same build with `REDRAW_REST` 0 at speed 60: 8,019–8,100 steps in 10 s at 4× throttling, against 4,817–4,852; unthrottled, 406–410 µs of main-thread time a step against 559–563.
- Static frames, all three path changes: seed 7 1,792 KB to 539 KB, 822 to 1,410 elements; seed 3 1,957 KB to 643 KB, 1,251 to 1,894 elements.
- Per redraw, GPU process main thread: forest or field 8.2–8.5 ms before, 6.1–6.3 ms with relative data and crown dots, 4.5–4.8 ms with simplification too. Wall 9.6–9.8 ms to 6.3–6.7 ms. Forest page-thread time unchanged at about 12.9 ms: JavaScript 10.4–10.6 to 10.8–11.0 ms, style 1.0 to 0.84 ms, paint 0.4 to 0.85 ms, garbage collection 0.85–1.06 to 0.27 ms. `setAttribute` takes only 0.1–0.7 ms a redraw, so the bytes cost little to parse; fewer calls cut it more than fewer bytes.
- Whole page, all changes:
  - Speed 2, unthrottled: 2.48–2.67 ms of main-thread time a frame against 2.33–2.41 (no gain; within run-to-run noise).
  - Speed 60, unthrottled: 5.75–6.06 ms a frame against 8.45–8.73, with the same number of steps (8,570–8,680 in 10 s).
  - Speed 2, 4× throttling: 4.14–4.41 ms a frame against 4.51.
  - Speed 60, 4× throttling: 8,096–8,160 steps in 10 s against 4,762–4,855, at 1,213 µs of main-thread time a step against 2,053–2,089; 553–556 frames against 515–521.
- [x] Fields in their own SVG layer (`main.js`, `index.html`, `style.css`). Five stacked SVGs: `#bars`, `#ground` (bar band, coarse ground, forest, upland), `#fields` (farmland mask and fields), `#upper` (the rest of the floodplain) and `#view`; the first four are compositing layers. The constructor takes `[ground, fields, upper]`; `save` copies every layer. Safari renders a mask again whenever its layer repaints, on the page's thread, so every floodplain redraw re-rendered all the fields through `#farmland`: 86 redraws in 20 s at the default speed (scroll bars 39, farmsteads 11, fields 8, crowns 8). Now only field, forest and wall redraws repaint `#fields`, 14 of the 86. Safari on an M1 laptop, 1440 × 666 at 2 device px to the CSS px, seed 7, default speed, 10 s after a 2 s settle, frames by `requestAnimationFrame`: 570 of 600 frames and 9 gaps over 30 ms, against 464–537 and 46–56. Mask removed: 600 and 3. Floodplain hidden: 602 and 1. Flat pebble and reed patterns: 498 and 57 (no gain). No colour changes by more than 3 levels of 255 except a handful of pixels where separate layers blend.
- [x] Channel drawn at `river.time() + owed - dt` (`ui.js`, the `tween` call in `update`). Subtracting the time asked too sent the channel back to the start of its step at each request: 201 times in 602 frames in Chrome, so at the default speed only 200 of 602 frames redrew it. In Safari the drawn time went back 91 times in 508 frames. Now it never goes back, and headless Chrome (1280 × 800 at 2×, default speed) redraws the channel in 362 of 602 frames, limited by `MIN_MOVE`.
- [ ] The remaining long frames in Safari come from mask re-renders on field, forest and wall redraws. A possible next step is replacing the mask, for example with per-field opacity, which would change the look.
- Measured, not done:
  - Tiling the forest contours. Forest age grows everywhere, so every contour moves: 0 of 509 contour loops were unchanged between redraws at speed 2, and 356 of 22,483 at speed 60. The crowns already lie in strips, and 5,698 of 5,976 crown paths (95%) come through a redraw unchanged.
  - Removing the farmland mask saves 0.3–0.5 ms of GPU time per `#ground` repaint and nothing on the page thread. Replacing it means clipping fields to the farmland contours in JS. Kept.
  - Lake gradients: no measurable cost. Kept.
  - Level of detail for furrows. They cost 2.9 ms of a 14.4 ms field redraw (seed 3, tick 14,000) and no GPU time. With the rest, fields redraw at most twice a second, so dropping them saves at most about 6 ms a second. A pixel-size rule would never fire, since furrows are 1.2–1.7 device px wide on the displays checked.
  - For a decision: a Canvas 2D `#view` (channel and lakes), prototyped separately. It saved 0.7–1.5 ms a frame unthrottled and 2.5–4 ms at 4× (15–20% of main-thread time at speed 60), with GPU-process time unchanged. It breaks the smoke test's `#view path` check, Save SVG and the identical-SVG replay check, and `CLAUDE.md` says SVG only.

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

- Mill founding is constant in time. A medieval peak and a later decline could follow the clearance clock.

# Plan: high-speed pauses (from 2026-10-18)

At high speeds the river paused at least once a second. Profiled with Playwright and installed Chrome on an Apple M1, 1280 × 800 for desktop, and 844 × 390 with the page and worker at 8× for the phone, seed 7 from tick 10,000 for 10 s unless stated. Each change is left uncommitted, in the order below.

- [x] Pace the river from the floodplain's redraw times (`ui.js`, `allowance` in `main.js`). Before: at speed 60 the desktop had 64 still frames in 650 and 9 runs of 3 or more, all holds at `MAX_LAG` (25 years), which a staged redraw passes in 10 frames at that speed. After: no still frames on desktop at speeds 20, 40 and 60 over seeds 1, 3 and 7, at 58 units a second at speed 60. Phone at speed 60: 13.6 units a second and no still frames, against 16.2 units a second with 251 still frames of 377 under the hold alone. Paused SVGs at seeds 7 (ticks 3,000 and 10,000) and 3 (tick 6,000) match HEAD byte for byte.
- [x] Drop owed time beyond `MAX_OWED` while a reply is late. Seed 1 from tick 20,000 had a 6-unit jump after 2 still frames; after it, the largest step a frame is 1.1 units.
- [x] The page's preload of the WASM binary went unused since the worker loads it; preload `snapshot.js` instead.
- [x] Redraw the channel only once it could have moved `MIN_MOVE` (0.5) device pixels (`footing`, `tween`). At speed 2 on the phone (device pixel ratio 3), 177 of 301 frames redraw it; three alternating 10 s runs gave a mean frame of 8.5–8.9 ms against 14.1–14.2, `tween` 5.2–5.4 ms against 10.7, and p99 25–29 ms against 34–38. Speed 60 is unchanged (`tween` 9.4 ms either way), since each frame there draws a new centreline. Paused SVGs match HEAD. On the way, reading `innerWidth` after the frame's writes forced a layout, which raised `tween` at speed 60 from 9.4 to 12.8 ms.
- [x] Format the year and tick only when they change. `toLocaleString` on every draw took 0.69% of the phone's main thread at speed 60.
- [x] Start the pace at the allowed speed after a reset, not at 0, so the river starts at full speed as before. A redraw's time counts at most 100 ms a frame, so a hidden tab does not slow the river after it.
- [x] `contourData` writes its points with `dp1`, which gives the same strings as `Math.round(10 * x) / 10` over 20 million random values and 2.4× faster in Node. Its inclusive share of the phone's main thread at speed 60 fell from 7.7–8.0% to 6.3–6.8% over two alternating 8 s CPU profiles. Paused SVGs match HEAD.
- [x] `contourData` copies each grid row with `set` and carries each cell's left corners from the cell before. With `dp1`, a contour takes 0.27 ms against 0.37 ms in a Node benchmark on a 262 × 122 grid, with the same output, and its inclusive share of the phone's main thread at speed 60 fell from 7.7–8.0% to 5.0%. Paused SVGs match HEAD.
- [x] Field redraws keep the last strip's hashes and the last cell's field while the next cells share them, and `boundaryArcs` counts the edges at a corner before it lists them. A field redraw took 75–83 ms against 106–108 ms on the phone at 4× throttle and speed 20, and paused SVGs match HEAD. The river there runs at 13.8–14.0 units/s against 13.5–13.7.
- [x] A new speed from the slider takes effect at once; only a pace held below the set speed rises over `PACE_SECONDS`. Before, a move from speed 1 to 60 took over 2.5 s to reach full speed (48.7 units/s over the 1.5 s after the first second). Now that window runs at 60.1 units/s, as HEAD does (59.3).
- [x] `Sim::cutoff` builds the hashed grid with `capture`'s cells (`Sim::build_grid_of`), so the capture search reuses it and a step builds the grid 2.0 times, down from 2.9. The median native step fell from 0.111 to 0.107 ms over three alternating runs (seed 7, 20,000 steps). The snapshot example's stats and SVGs match HEAD for seeds 1, 3 and 7, and `make test` passes.
- [x] The floodplain's budget rises to the idle budget only as a redraw passes half the lag, and no longer whenever the floodplain sets the river's speed. On the 4× phone at speed 60 the page drew 49 frames a second against 38, all showing the river moving, at 24.2 units/s against 34.3; HEAD draws 56 frames a second, of which 22 show it moving, at 26.5 units/s. On the 8× phone at speed 60 it draws 28 moving frames a second against 24, and HEAD 11. Desktop at speed 60 still runs at 60.0 units/s against HEAD's 54.4.
- [x] Owed time is capped at the request under way plus one step plus `MAX_OWED`, in place of the larger of one step and `MAX_OWED`. At speed 2, 40 ms of river time is less than a step, so the cap held the river to a step a request and it stood still for runs of 3 frames: 35 such runs in 10 s on the 4× phone, against none after.
- [x] `PACE_LAG` is 1, up from 0.7, so the pace leaves no margin over each layer's recent redraw times. On the 4× phone the river ran at 9.7 against 9.5 units/s at speed 10, 11.5 against 10.7 at speed 20 and 26.8 against 23.8 at speed 60 (seed 7), and at 18.7 against 17.9 (seed 1) and 16.9 against 16.8 (seed 3) at speed 40. No run stood still for 3 frames, and desktop at speed 60 holds 60.0 units/s.
- [x] The channel's outlines (`edgeData`, `channelData`, `bandData`), `channelFrame` and `footing` use scalar locals in place of array destructuring in their loops. A pair of outlines over 1,500 points takes 0.69 ms against 0.91 ms in Node with the same output, and `footing` and `channelFrame` return the same values on 300 random lines. On the 4× phone at speed 60, `tween` took 3.7 ms a drawn frame against 4.3 ms over two alternating runs. Paused SVGs match HEAD.
- Measured, not done:
  - Building the channel's outlines in the worker. At speed 60 only 10 of 301 redraws draw the newest centreline as it stands; the rest interpolate on the page.
  - Recycling snapshot buffers. Garbage collection is 1.6% of the phone's main thread at speed 60, but `fieldStages` holds a snapshot's buffer across frames, so a buffer handed back to the worker would be detached under a field redraw.
  - At speed 20 on the 4× phone, a reply from the worker waits 16 ms for the busy main thread. Stepping takes 1.0 ms of that and packing 0.4 ms, so the worker is not what limits the speed.
  - Ramping the floodplain's budget up from a tenth of the lag raised the river's speed by 5–12% at speeds 10–40 on the 4× phone. It also lost 3–7% of frames and doubled the frames over 25 ms at speed 10 (48 → 98), so it was dropped.
  - Lakes and the floodplain spend their budgets each frame, so faster `lakeData` would buy more rebuilds and no shorter frames.
  - At speed 20 the page writes 200 KB of path data a frame on the phone: 88 KB for the channel and 57 KB for the lakes.
  - Relative path commands for the channel. A 1,750-character path takes 3.0 µs to set against 2.1–2.8 µs for the same path in relative commands (1,130 characters), about 1% of the phone's main thread.
  - Skipping blocks of `contourData` cells with every corner on one side of the level: 0.29 ms a contour against 0.26 ms on 36 grids taken from the page, since the bounds pass costs more than it saves. Bounds computed once per grid would give 0.23 ms.
  - `contourData` clearing only the edges it used: 0.268 ms a contour either way. Marching takes 0.11 ms of a contour and chaining the points into strings 0.16 ms.
  - A floodplain budget of 5 or 6 ms on the 4× phone: speeds from 8% lower to 11% higher than 4 ms across speeds 10–60, and 1–6 fewer frames a second.
  - Lakes rebuild every `LAKE_STEP` of fill, and at speed 20 the rebuilds due exceed `LAKE_BUDGET`, so a coarser step would make lakes lag further without shortening frames.
  - For a decision: `LAG_SECONDS` sets how far behind the floodplain may be drawn, and with it the phone's speed. On the 4× phone (seed 7), 0.5 s gives 11.7 units/s at speed 20 and 27.7 at speed 60, with the floodplain a median 7 and 35 years behind. 0.75 s gives 13.7 and 43.4 units/s at 13 and 57 years behind, and 1 s gives 16.6 and 52.0 units/s at 20 and 67 years (102 at most). HEAD runs at 15.2 and 25.5 units/s, and stands still for 3 frames or more 13 and 23 times in 10 s. No setting stood still for 3 frames.
- `make fuzz RUNS=64 BASE=1000` passes with all the changes above: 0 of 64 runs of 2,500 years failed.
- Final comparison with HEAD, seed 7 from tick 10,000 for 10 s (units/s, frames a second showing the river move, runs of 3 or more still frames):
  - Desktop 1280 × 800 at speed 60: 60.0, 60, 0 against 54.2, 54, 9. At 2560 × 1440 (pixel ratio 2): 60.0, 60, 0 against 51.5, 52, 25.
  - Phone at 2× throttling, speed 60: 48.7, 60, 0 against 38.2, 38, 32. Speed 20: 18.6, 60, 0 against 19.6, 59, 0.
  - Phone at 4×, speed 60: 27.7, 51, 0 against 25.5, 22, 23. Speed 20: 11.7, 55, 0 against 15.2, 42, 13. Speed 10: 9.8, 58, 0 against 9.8, 54, 4.
  - Phone at 8×, speed 60: 12.5, 30, 0 against 12.5, 11, 11.
