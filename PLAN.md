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

# Done: floodplain processes

Branch `feat/floodplain-processes`. Seven processes, each a core change with its own parameters (in `Params`, `NAMES`, `field`), a test, a WASM sync and a drawing. Every length is in channel widths and every time in units of 2.5 years. Baseline step time (snapshot, `UP=60 DOWN=20`, 30,000 steps, seed 1): median 0.106 ms, 99.9% 0.538 ms, max 0.83 ms.

## Status

The core is on `main` in one commit per process. The WASM syncs, the renderer and the `CLAUDE.md` frontend notes are done on `feat/floodplain-render`, uncommitted. The renderer added 0.1–0.2 ms to the mean frame and 0.4 ms to p95 (1.5 → 1.7 ms and 5.2 → 5.7 ms, 600 frames at speed 2, headless Chrome); p99 stayed at 6.3–7.1 ms.

- [x] 1 clearance alluviation
- [x] 2 chute cutoffs
- [x] 3 large wood
- [x] 4 bluff erosion
- [x] 5 tributary fans
- [x] 6 crevasse splays and avulsion
- [x] 7 mills and weirs
- [x] CLAUDE.md core notes
- [x] WASM syncs
- [x] renderer
- [x] CLAUDE.md frontend notes

Core results. With every new process off (`CLEARANCE_SUPPLY=1 CHUTE_VEGETATION=0 WOOD_RATE=0 BLUFF_RATE=0 FAN_RADIUS=0 CREVASSE_DROP=0 MILL_RATE=0`) the snapshot matches the baseline exactly: 70 cutoffs, 13 in 64 floods, sinuosity 1.46. With all on, over seeds 1–6 and 24,000 steps: mean sinuosity after step 6,000 is 1.39 (1.34 with all off), 50 cutoffs (54), about 6 jams and 5 splays at a time, 1–2 working mills, 1 avulsion per 1,600 units, walls retreating up to 6 widths. Step time with all on: median 0.132 ms, 99.9% 0.62 ms, max 0.79 ms. 48 tests pass, and 32 runs of the self-intersection configurations (seeds 1–16, calm and stormy) stay clean.

## How the core turned out

Where the code differs from the first plan:

- Clearance multiplies the overbank amount, the lake fill rate, the bedload fill and fan growth. It leaves `levee_length` alone: the decay length is set by how fast silt settles, and lengthening it widened the flood's band and its cost by 1.7 times.
- Scroll bars are dropped where the deposit stands within `BURY` (0.95) of the flood level.
- Jams push at most `jam_push` (0.15) summed over a reach, smoothed along the channel over `JAM_REACH` (3 widths). A tree within `JAM_MERGE` (4 widths) of a jam on either bank joins it. Stronger or unsmoothed pushes kinked the channel into small loops that the neck cutoff, which needs 20 points of separation, cannot remove.
- Bluff erosion uses the square of the wall taper's shortfall at the bank. Each slump bumps `wall_version`, about 600 times per run of 2,400 units, so the renderer must throttle wall redraws.
- A breach is found in the step after its flood (`Sim::breach`), so the flood deposit and the avulsion search never share a step. The search runs on 2 × 2 blocks of grid cells, each as low as its lowest cell, and takes the rejoining point with the best length ratio among routes whose mean ground stands below the levee crest.
- A weir holds its points against migration and smoothing within `WEIR_HOLD` and frees them linearly over as far again; `pin_weirs` puts the weir point back after smoothing. A mill is ruined when the channel turns more than 60° over a width within twice `WEIR_HOLD` of the weir (outflanked), when it moves off the weir, or in a great flood.
- `Sim::cutoff` now cuts through the cooldown after a recent cut when any pair of points has closed to half the cutoff distance. The cooldown let necks cross before.

## Remaining work

### WASM syncs (`wasm/src/lib.rs`)

Flat `f32` buffers as for the scrolls, each behind its version counter, and the window offset applied as for the other geometry:

- `jams`: per jam, x, y, the downstream and into-channel directions fixed where it lodged, strength (`Jam::strength(time, jam_lifetime)`), and `until`.
- `sync_splays`: per splay, apex, dir, length and age over `SPLAY_LIFE`; `splay_version`.
- `tributaries` and `tributary_courses`: per tributary, mouth, scale and the course the core keeps from the mouth to the water it meets, with a version that changes with the course.
- `sync_mills`: per mill, weir, across, working flag, age of ruin over `MILL_RUIN`, then the leat polylines; `mill_version`.
- `wall_version()` getter, and `supply()` for the panel or year label.
- `sync_forest` and the deposit pointer already carry splay burial and fan deposits; add a `coarse_ptr` for the gravel and colluvium.

Every new sync can grow memory, so call them in `draw` before building the typed-array views, as for `sync_scrolls`.

### Renderer (`web/main.js`)

All vector, no raster, in the existing layers:

- Log jams: a few short dark strokes across the bank at each jam, length and count from strength, in `#view` over the water.
- Splays: a pale sand lobe (a fan path from apex along dir) in `#ground`, fading with age as the forest contours take it back.
- Tributaries: a thin stream from the combe mouth along its course, winding in bends set by the distance from the mouth; the fan as a faint cone of stippled gravel from `coarse`, contoured like the levees.
- Walls: redraw `drawWalls` when `wall_version` changes, at most every few seconds; colluvium lobes from `coarse` at the foot.
- Mills: the weir as a short bar across the channel at the weir, the leat as a thin water line, the mill house as a small rectangle on the leat beside the weir, the pond as a slightly wider, darker water reach above; ruins drawn greyed for `MILL_RUIN`.
- Clearance: no new drawing needed. Lakes fill and scroll bars vanish faster, which the existing layers show. Optionally tint the upland as it is cleared.

### CLAUDE.md frontend notes

Describe the new sync functions and drawings once they exist.

### Left over from the renderer

- A mill's leat is fixed where it was built, so the channel can migrate over its ends; the core could trim or rebuild it.
- The weir pins one point, and the channel can turn up to 60° about it before the mill counts as outflanked, which draws as a visible corner. A lower limit would ruin mills sooner.

### Open questions for the user

- Fords and embankments: answered in chat, not built.
- Mill founding is constant in time; a medieval peak and later decline could follow the clearance clock.
