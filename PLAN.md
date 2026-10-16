# Plan: floodplain processes

Branch `feat/floodplain-processes`. Seven processes, each a core change with its own parameters (in `Params`, `NAMES`, `field`), a test, a WASM sync and a drawing. Every length is in channel widths and every time in units of 2.5 years. Baseline step time (snapshot, `UP=60 DOWN=20`, 30,000 steps, seed 1): median 0.106 ms, 99.9% 0.538 ms, max 0.83 ms.

## Status

The core is done and uncommitted; the WASM syncs, the renderer and the `CLAUDE.md` frontend notes are still to do.

- [x] 1 clearance alluviation
- [x] 2 chute cutoffs
- [x] 3 large wood
- [x] 4 bluff erosion
- [x] 5 tributary fans
- [x] 6 crevasse splays and avulsion
- [x] 7 mills and weirs
- [x] CLAUDE.md core notes
- [ ] WASM syncs
- [ ] renderer
- [ ] CLAUDE.md frontend notes

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

- `sync_jams`: per jam, x, y, side, strength (`Jam::strength(time, jam_lifetime)`), and the bank direction from the centreline point `at`.
- `sync_splays`: per splay, apex, dir, length and age over `SPLAY_LIFE`; `splay_version`.
- `sync_tributaries`: per tributary, mouth, radius, scale, and the junction point `centerline[at]`. Junctions move every 10 steps, so sync with the line.
- `sync_mills`: per mill, weir, across, working flag, age of ruin over `MILL_RUIN`, then the leat polylines; `mill_version`.
- `wall_version()` getter, and `supply()` for the panel or year label.
- `sync_forest` and the deposit pointer already carry splay burial and fan deposits; add a `coarse_ptr` for the gravel and colluvium.

Every new sync can grow memory, so call them in `draw` before building the typed-array views, as for `sync_scrolls`.

### Renderer (`web/main.js`)

All vector, no raster, in the existing layers:

- Log jams: a few short dark strokes across the bank at each jam, length and count from strength, in `#view` over the water.
- Splays: a pale sand lobe (a fan path from apex along dir) in `#ground`, fading with age as the forest contours take it back.
- Tributaries: a thin stream from the combe mouth across its fan to the junction, wiggled by seeded noise; the fan as a faint cone of stippled gravel from `coarse`, contoured like the levees.
- Walls: redraw `drawWalls` when `wall_version` changes, at most every few seconds; colluvium lobes from `coarse` at the foot.
- Mills: the weir as a short bar across the channel at the weir, the leat as a thin water line, the mill house as a small rectangle on the leat beside the weir, the pond as a slightly wider, darker water reach above; ruins drawn greyed for `MILL_RUIN`.
- Clearance: no new drawing needed. Lakes fill and scroll bars vanish faster, which the existing layers show. Optionally tint the upland as it is cleared.

### CLAUDE.md frontend notes

Describe the new sync functions and drawings once they exist.

### Open questions for the user

- Fords and embankments: answered in chat, not built.
- Mill founding is constant in time; a medieval peak and later decline could follow the clearance clock.
