# Plan: slow change

Two pieces of work, in this order. The first removes floods, which change the map in a single step, and puts steady processes in place of the slow work they do. The second gives each holding a farmstead of small buildings that changes with its fields and with the river. The buildings read the ground the floods leave, so the flood work comes first. Every length is in channel widths and every time in units of 2.5 years.

Baseline to measure against (snapshot, `UP=60 DOWN=20`, 24,000 steps, seeds 1–6, all processes on): mean sinuosity after step 6,000 of 1.39, 50 cutoffs, 1 avulsion per 1,600 units. Step time: median 0.132 ms, 99.9% 0.62 ms, max 0.79 ms. Re-run the baseline before starting, since the renderer and loading-screen work has landed since.

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

The steady deposit runs a deposit-only pass of `Erodibility::flood` every `DEPOSIT_EVERY` steps, laying the mean of what floods lay today: `0.5 · overbank · supply · flood_rate · DEPOSIT_EVERY · dt`, where 0.5 is the mean magnitude. Measure one pass's cost first. If it breaks the step-time budget, spread it over bands of channel points, as `grow_rows` spreads growth over bands of rows. Each pass lays a small fraction of a levee, so the levee contours thicken without a visible jump.

Scroll bars come one set per flood today, 125 years apart on average. They become a Poisson process of their own with mean interval `scroll_time` (50 units, 125 years), so the spacing of the ridges on a bar keeps its present mean and stays irregular.

Tributary course shifts also become a Poisson process of their own, at today's mean rate: a shift now happens in a flood with a chance that grows with its magnitude, so the new rate is `flood_rate` times that chance averaged over magnitude. Each shift draws a magnitude as a flood does now, so `shift_streams` keeps its behaviour. Fans still spread across their cones.

Both processes draw from the `events` stream. Removing the floods changes the river's own draws once; after that, these processes leave them alone.

Parameters: `flood_rate` splits into `scroll_time`, the deposit rate (given per unit time as `overbank_rate`) and the shift rate (`stream_shift_rate`, in place of `stream_avulsion`). Drop `breach_dist`, the chute parameters, `flood_widening`, `scour_reach`, `avulsion_flood`, `avulsion_ratio` and `crevasse_drop`, each from `Params`, `NAMES` and the `field` match. Keep the snapshot's env overrides working for the parameters that remain.

Retuning:

- Floods made 13 of 70 cutoffs in the old baseline, so sinuosity will rise. If it leaves the range the bounded-sinuosity test allows, lower `cutoff_dist`, and do not loosen the test.
- `vegetation_time` was tuned with scour stripping young shrub. Check that a median bar still binds in about `vegetation_time` (250 years).
- Check that lakes still fill over about `oxbow_lifetime` without the flood silt.

Tests: delete the tests that pin flood behaviour (`floods_cut_off_earlier_and_never_self_intersect`, `floods_strip_young_growth_beside_the_bank`, the crevasse and avulsion tests). Recast `floods_build_levees_that_slow_erosion`, `floods_silt_up_lakes` and `floods_leave_scroll_bars_inside_migrating_bends` on the steady processes. The validation tests in `src/lib.rs` must pass unchanged, and the self-intersection configurations must stay clean over seeds 1–16.

Docs: remove floods, splays and avulsion from `docs/architecture.md` (the erodibility, lakes, scroll bar and floodplain process sections) and from the `CLAUDE.md` module list. `crevasse.rs` goes.

- [ ] re-run the baseline
- [ ] steady deposit, timed
- [ ] scroll bars and stream shifts as Poisson processes
- [ ] remove the sudden effects and their parameters
- [ ] retune, tests, snapshot numbers
- [ ] WASM and renderer: drop the splays
- [ ] docs

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

- [ ] risk from ground height, field layouts checked
- [ ] `Building`, targets, placement score
- [ ] following the river, hamlets, ruins
- [ ] tests, step time
- [ ] WASM sync, drawing
- [ ] docs

## Left over

- The weir pins one point, and the channel can turn up to 60° about it before the mill counts as outflanked, which draws as a visible corner. A lower limit would ruin mills sooner.
- Mill founding is constant in time. A medieval peak and a later decline could follow the clearance clock.
