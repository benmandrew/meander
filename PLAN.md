# Handoff: Meandering River Simulation (Rust → WASM, live in browser)

## Status

Build with `make build`, run with `make serve` (http://localhost:8000; `?warmup=T` overrides the warm-up in simulated time). Tests: `make test`. Snapshots for tuning: `cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]`, with parameter overrides as upper-case env vars (e.g. `ALPHA=0.8`) and `UP`/`DOWN` for hidden reaches in widths.

The frontend is SVG only (`web/index.html`, `web/main.js` for drawing, `web/ui.js` for the panel and loop). A WebGL view with a bank mesh, depth shading and a floodplain age map existed in commit e6cd034 and was removed at the user's request.

- [x] Core: resample, Menger curvature, Howard–Knutson kernel (option 8), migration, smoothing, hashed-grid neck cutoffs with a spatial cooldown.
- [x] Validation tests: sine grows and drifts downstream; local-only kernel does not drift; noise run cuts off, never self-intersects, and sinuosity stays bounded.
- [x] SVG view: stroked channel and oxbow paths (options 1 and 2), faded past centrelines tracing the meander belt, Save SVG.
- [x] Heterogeneous erodibility (option 9): fBm noise field, and oxbow footprints that erode fast as open water and stiffen to clay plugs as they fill. Each footprint cell stores its cutoff time, and its multiplier moves linearly from `lake_factor` (3) to `clay_factor` (0.35) over `oxbow_lifetime`, so a bank that reaches a young lake breaks through quickly. Over 20,000 steps (seeds 3, 4 and 11), this gave 18, 14 and 17 cutoffs against 12, 12 and 19 with a flat 0.35.
- [x] Stochastic cutoffs (option 10): floods arrive at `flood_rate` per unit time, each with a magnitude u in [0, 1). A flood breaches necks narrower than `cutoff_dist + u·(breach_dist − cutoff_dist)` and cuts chutes where the arc is at least `chute_ratio / u` times a chord of up to 5 widths that does not cross the channel. Up to four cuts per flood; the hard cutoff at `cutoff_dist` stays as a backstop. Over the second half of a 30,000-step run, mean sinuosity falls from about 1.8 to 1.6 at `flood_rate` 0.1 and `chute_ratio` 3 (seeds 3 and 11).
- [x] Ikeda–Parker–Sawai coefficients (option 11), in `meander-core/src/ips.rs`, behind the `physics` flag (off by default). The reduction gives Ω = −1, Γ = (F² + A + 3)/2 and α = 2·C_f·(w/H) per width. The IPS equation is taken from the form in Sun et al. (1996); the 1981 paper itself was not available to check against. The defaults (C_f 0.01, w/H 30, F 0.3, A 2.89) give Γ ≈ 3.0 against the hand-tuned 2.5, which raised the cutoff count from 34 to 57 over 20,000 steps (seed 4).
- [x] Floodplain soils and oxbow infilling (option 2, revised). The channel removes any part of an oxbow under its water, splitting it if needed. Each piece keeps one point under the water at a cut, so the channel hides the cut end, and remembers its place along the original oxbow, so only the original ends plug. Lakes plug from both ends, at a rate set by their length at cutoff so a channel trim does not make them jump, and narrow far more slowly from the banks. They keep round ends that lengthen to a crescent, and turn murky over `oxbow_lifetime`. The lake bed follows the land height (`Erodibility::height`: the erodibility noise plus a layer at a quarter of its scale), and the lake's width varies with the depth over a parabolic bed. As the water level falls, high ground dries first and splits a long lake into separate pools with round tips. The `Oxbow bed relief` display slider sets the bed's rise as `relief × 4 × (height − 0.5)` of the channel depth, then stay as a marshy scar stroke for `scar_lifetime`. The SVG view scatters seeded soil patches as smooth Bézier blobs: broad clay, silt and sand patches typed by the erodibility noise at their centre, and small gravel and peat patches. Past centrelines are drawn as point-bar sand that grasses over with age. A raster floodplain map (option 6) was tried and removed at the user's request.
- [x] Reworked ground (option 14). Each step stamps the erodibility cell under every centreline point with the time, and clears any oxbow fill there. A swept cell erodes at `bar_factor` (1.3) as bare point-bar sand, moving to 1 over `vegetation_time` (100) as vegetation binds it. A point now reads the erodibility at the bank it moves into, `BANK_READ` = 0.75 widths out, because the cells under the channel are always fresh; the grid cell shrank to half a width so that read misses the channel's own trail. The side is chosen by the sign of the migration rate alone. Choosing it by the jitter as well damped outward and amplified inward noise wherever the two banks differ, a net drift inwards that took mean sinuosity from 1.55 to 1.45 in the first version. Over eight seeds and 30,000 steps, reading at the bank alone raised cutoffs per run from 25 to 35 and mean late sinuosity from 1.55 to 1.65, since a bank now breaks into a young lake as soon as it reaches it. Loose bars then speed the downstream drift of bends against their growth: a bar factor of 1.5 brought sinuosity back to 1.55 with 29 cutoffs. With loose bars on, floods no longer lowered the mean sinuosity (1.47 calm, 1.46 at `flood_rate` 0.1, measured with the jitter bias); with them off, floods lower it from 1.60 to 1.55. The flood test turns the bars off, and overbank deposits too since option 15, to isolate the flood cutoffs. With the side chosen by the rate alone, floods no longer lower sinuosity on every seed (1.62 calm and stormy over eight seeds with bars and deposits off; 1.55 against 1.46 with them on, but seed 1 goes the other way). The test now checks that floods cut loops off shorter: 73.7 against 54.9 widths for seed 3, and 76.5 against 59.2 for seed 11.
- [x] Overbank deposition (option 15). Each flood of magnitude u lays `u·overbank·exp(−d/L)·(1 − h/levee_height)` channel depths on every cell at distance d from the bank, out to 4L, with L = `levee_length` (2 widths). Deposits thin away from the channel into natural levees, and stop at the flood level `levee_height` (0.25 depths above fresh bars). The channel resets the deposit where it sweeps. A bank raised by h erodes 1/(1 + h) as fast, since it holds 1 + h times the material; Howard (1992) couples floodplain height to bank erosion this way, though the paper was not checked for the exact form. The belt ends up raised above the floodplain floods never reach, an alluvial ridge, with levees on the outer banks and low fresh bars on the inner. Like loose bars, high old banks favour drift over growth: at a levee height of 1 depth, sinuosity fell to 1.27 with 14 cutoffs per run, so the defaults are milder (bar factor 1.3, levee height 0.25), giving 1.53 and 25. A flood's deposition costs 0.53 ms natively, about once per 500 steps. The SVG view contours the deposit over the window at 0.3 and 0.7 of `levee_height` as nested translucent silt fills, marching squares in `main.js`, redrawn after each flood and every 10 time units.
- [x] Lakes filled by the channel (option 16). Each oxbow carries a fill fraction, and is dropped `scar_lifetime` after it reaches 1. The fill rises by `dt / oxbow_lifetime` each step (1,200, the time a lake takes with no sediment from the channel). A cutoff sets the footprint's deposit to −1 channel depth (`LAKE_DEPTH`), and floods lay overbank silt there in proportion to the depth of water over the bed, so a lake traps 1 + 1/`levee_height` = 5 times the deposit of fresh bar ground. At each flood the fill becomes at least the mean silt over the lake's interior. A flood also sends bedload in through the upstream entrance: `u · bedload_fill · entry · exp(−d/levee_length)`, with d the entrance's distance from the bank and `entry` the cosine of the diversion angle between the chord of the cut and the oxbow's first segment, after Constantine et al. (2010, *Sedimentology*, Sacramento River; not checked for the exact form). Every neck cutoff in a seed 3 run leaves its loop at more than a right angle, so `entry` is 0 and bedload comes only from chutes cut at a shallow angle. The lake ramp in the erodibility grid moves to clay at the larger of the clock and the cell's silt. Over eight seeds, lakes took 250 to 1,160 time units to fill, with medians from 505 to 876 per seed; with silt capped at the fresh-bar rate, nearly all took 1,134 to 1,200, so floods barely counted. Over 16 seeds, mean sinuosity was 1.47 against 1.46 for option 15, with 22.1 against 21.6 cutoffs per run; the eight-seed figure of 1.53 for option 15 came from a high sample. The view draws each lake by its fill, and lowers the bed where silt exceeds the lake's mean, so a lake dries first at the end nearest the channel.
- [x] Scroll bars (option 17). At each flood, `Sim::add_scrolls` recomputes the migration rate and, along every run of at least 6 points whose rate exceeds 0.05 in one direction, leaves a ridge 0.6 widths in from the centreline on the side the bank moves away from: the inner bank of a migrating bend. Every 10 steps the channel erodes the parts of ridges within half a width of it, and ridges are dropped after `scroll_lifetime` (1,000). They do not feed back into the dynamics; the optional height bump from the proposal was left out, since a later lake's bed comes from the cut channel, not the ridges. Step time stayed at 0.07 and 0.08 ms on seeds 1 and 4. The view syncs them only when `scroll_version` changes or every 10 time units, and strokes them at 0.15 widths in six age bands, pale sand when new and green as vegetation takes them, fading over the last quarter of their life. They read as nested arcs inside the bends.
- [x] Crevasse splays (option 18). A flood of magnitude u above 0.6 breaches a levee with probability `splay_chance × (u − 0.6) / 0.4` (default chance 1). `Sim::crevasse` tries 20 random points and takes the sharpest bend, provided its radius is under 5 widths, then lays a fan from the outer bank: (3 + 5u) widths long, spreading 35° either side of the bend's outward normal, `u × levee_height` deep at the apex and thinning to nothing at the rim. `Erodibility::stamp_splay` adds the sand to the deposit and marks dry cells as freshly swept, so the sand is loose by the bar factor but raised; in a lake the sand silts the bed instead. Over 16 seeds and 30,000 steps, a run makes 9.1 splays, and sinuosity is 1.47 ± 0.02 with splays and without. The view draws each splay as a smooth lobe of pale sand with three rills from the breach, above the soil and levee layers since the sand buries them, greening and fading with age. Abandoned splays stay on the floodplain after the bend that made them moves away.
  - Correction: as first committed, a splay appeared whole in one frame and was dropped whole once the channel came within a width of its middle. Since the breach sits on the outer bank of a sharp bend, which migrates fastest, 56 of 66 splays over 8 seeds went that way, at a median age of 181 time units: about 30 s on screen. Each splay now holds 9 rays across the fan, each with the inner and outer distance between which sand remains. Every 10 steps the rays are trimmed from either end past cells the channel has swept since the splay formed (`Erodibility::swept_since`, within 0.75 widths), and a splay is dropped when no sand remains or after `splay_lifetime`. The bend still erodes most splays within a few hundred time units (43 of 51 fully, median 217), but piece by piece from the breach, and the far rim often survives. The view grows each new lobe out from the breach over 60 time units and draws only the sand that remains. The same fix moved the floodplain layers to the start of `draw`: `sync_scrolls` (option 17) grows memory now and then, which detached the oxbow views built earlier in `draw` and stopped the frame loop.

Deviations from the plan, found while tuning:

- The first version looked like string coiling. Causes and fixes:
  - Speed: it ran 1,200 steps per second, so bends crossed the screen in about two seconds. The page now runs on simulated time per second (default 6, about 60 steps per second).
  - Shape: migration now saturates with curvature (Hickin–Nanson), peaking at bend radius `peak_radius` = 2.5 widths, so bends round out instead of coiling.
  - Kinks: per-step jitter is smoothed along the river over `jitter_scale` = 3 widths, at amplitude 0.05.
  - Direction: `valley_width` = 45 widths confines the belt, so the river reads as flowing down a valley.
  - Upstream straightening: bends grow only as they drift downstream, so the visible window is the middle of a longer river, with 60 widths hidden upstream and 20 downstream (set in `meander-wasm`).
  - Window edges: loops drifting in from the hidden reach straddled the left and right edges and were cut in half. The valley now pinches to `pinch_width` = 6 widths at each window edge (`Sim::pinches`), widening back over `pinch_length` = 20 widths, so the river crosses each edge once, near the axis. Snapshots show bends just inside the left edge as developed as before.
- Hard clamping to the domain pins the river along the walls. Erodibility tapers to zero within `edge_margin` of each edge; the clamp stays as a backstop.
- `hypot`, `ln` and `cos` are software routines in WASM and made a step 3.6 times slower than native. The core avoids them on the hot path (0.095 ms per step in WASM for ~700 points). Warm-up is spread across frames behind a progress note.
- Performance pass (no change to the dynamics; the centreline, oxbows, scrolls, splays and deposits hash identically over 30,000 steps on seeds 1–5). The neck search and the oxbow and scroll rework share one hashed grid, rebuilt only when the centreline has changed since. Cells along a row hash to consecutive buckets, so each 3 × 3 search scans three runs of the table. The cutoff search skips candidates at or below `i + k_sep` in each bucket, and the oxbow check stops at the first point under the water. A native step fell from 0.077–0.086 ms to 0.052–0.058 ms. In the browser, the page draws only when a step was taken or a setting changed. The old point bars and the ground sit in their own SVG layers (`#bars`, `#ground`) under `will-change: transform`, so a moving channel does not repaint them, and the channel is drawn as runs of 24 segments so a repaint covers only the tiles it crosses. Joins are bevelled, which rasterise faster than round ones, and the panel lost its backdrop blur. Lakes and scars are rebuilt only when their piece of oxbow, fill step or silt changes, and `sync` recomputes oxbow heights only when `Sim::oxbow_version` changes. With software raster at 1600 × 900, renderer CPU fell from 2.4 to 0.74 CPU-seconds per second and the frame rate rose from 43 to 60. With GPU raster at device pixel ratio 2 the gains are smaller: renderer CPU fell from a median of 0.26 to 0.23, and main-thread script from 106–152 to 56–71 ms per second.

## Goal

Build a live, interactive browser simulation of a meandering river. The river is a 2D polyline (centerline) that migrates laterally over time. Bends grow and drift downstream, and when a loop folds back on itself the river cuts through the neck, leaving the abandoned loop behind as an oxbow. This repeats indefinitely.

The idea comes from a Houdini setup: a Solver SOP (per-frame feedback loop) that resamples the curve, computes per-point curvature, migrates the points, and uses a shortest-path step to detect neck cutoffs. Houdini is not needed; every step is simple to implement directly.

Target: a Rust core compiled to WASM, rendering in the browser with live parameter sliders.

---

## Core algorithm (per frame)

Flow runs from index 0 (upstream) to index n−1 (downstream). Keep both endpoints fixed.

1. **Resample** the polyline to uniform spacing Δs (≈ 0.5 × channel width w). Walk the arc length and linearly interpolate. Do this every step, because the kernel in step 3 assumes uniform spacing.
2. **Curvature** at each interior point, using signed Menger curvature:
   `a = p[i]-p[i-1]`, `b = p[i+1]-p[i]`, `c = p[i+1]-p[i-1]`,
   `C[i] = 2·cross(a,b) / (|a|·|b|·|c|)`. Positive = left turn.
3. **Migration rate**, using the Howard–Knutson (1984) upstream-weighted kernel:
   `R(s) = Ω·C(s) + Γ · [Σ_{ξ upstream} C(s−ξ) e^{−αξ} Δs] / [Σ e^{−αξ} Δs]`
   Typical values: Ω ≈ −1, Γ ≈ 2.5, α sets the decay length (a few channel widths).
   Compute it in O(n) with a single forward recurrence:
   `I[i] = e^{−αΔs}·(I[i−1] + C[i−1]·Δs)`, `N[i] = e^{−αΔs}·(N[i−1] + Δs)`, `R[i] = Ω·C[i] + Γ·I[i]/N[i]`.
4. **Move** each point along its unit normal: `p[i] += dt · E · R[i] · n̂[i]`.
   **Sign convention:** set it so that bends grow. Test this: a small sine perturbation on a straight line must amplify and drift downstream. If it decays, flip the sign.
   **Stability:** clamp each point's per-step displacement to a fraction of Δs, e.g. 0.1–0.2.
5. **Smooth** with a light Laplacian: `p[i] += k·(p[i−1] + p[i+1] − 2p[i])`, with k small (≈ 0.05–0.25). This suppresses zigzag instability at the scale of the point spacing. It is a numerical aid, not physics.
6. **Cutoff detection.** Find pairs i < j with `j − i > K` (far apart along the river) and `|p[i] − p[j]| < d_cut` (close in space, d_cut ≈ w). Replace the centerline with `p[0..=i] ++ p[j..]` and move `p[i..=j]` into the oxbow list (with a birth time). Use a uniform grid spatial hash with cell size d_cut, giving ~O(n). Allow at most one cutoff per frame, and ignore a small index window around a recent cut point to avoid immediate repeat cutoffs. Resample after a cut.
7. **Render** (see options below).

**Initial condition:** a straight or gently curved line plus small random lateral noise. A perfectly straight line never meanders.

**Growth:** total length grows as the river meanders, so the point count grows. Either cap it with a bounded domain, or let the downstream end leave the screen and trim it.

---

## Parameters to expose as sliders

| Param | Meaning |
|---|---|
| `w` | channel width (sets Δs, d_cut, and render width) |
| `E` | erodibility / migration speed |
| `Ω`, `Γ` | local vs upstream curvature weights |
| `α` | upstream decay rate (phase lag → downstream migration, bend skew) |
| `dt` | timestep (also substeps per frame) |
| `k_smooth` | Laplacian smoothing strength |
| `d_cut`, `K` | cutoff distance and minimum index separation |
| noise amp / seed | initial perturbation |

---

## Suggested Rust/WASM architecture

- **`meander-core`**: pure Rust, no web dependencies. Holds the `Sim { centerline: Vec<[f32;2]>, oxbows: Vec<Oxbow>, params, rng }` with `step(n)`. Unit-testable natively.
- **`meander-wasm`**: a `wasm-bindgen` wrapper. Expose `step`, parameter setters, and pointers/lengths to flat `f32` buffers for zero-copy reads from JS (`new Float32Array(memory.buffer, ptr, len)`). **Re-create views after every step**, because WASM memory growth detaches old `ArrayBuffer` views.
- **RNG:** use a tiny seeded PRNG (xorshift/PCG) or `rand` with `SmallRng`. Avoid pulling in `getrandom` without its wasm/js feature, which is a common build failure.
- **Rendering:** Canvas 2D from JS is enough for options 1–2 below. Use WebGL2 (raw, or via `wgpu`/`web-sys`) for mesh texturing, SDF shading, or the floodplain age map.
- A few thousand points runs comfortably in real time; consider f64 internally if long runs drift.

---

## Options, from pure aesthetics to physical realism

### A. Rendering / aesthetics (no change to the dynamics)

1. **Thick stroke.** Draw the centerline with `lineWidth = w`, `lineJoin = 'round'`. Overlaps in tight bends are invisible. Near-zero effort.
2. **Oxbow ageing.** Draw oxbows thinner, with fading colour or shrinking width over time, to suggest silting up and drying out.
3. **Explicit bank mesh.** Offset ±w/2 along the normals and build a triangle strip with UVs (u = arc length, v = across the channel). This enables a downstream-scrolling water texture and flow lines. Fix inner-bank self-intersection when the radius 1/|C| < w/2 by clamping the inner offset to `min(w/2, 0.9·r)`, or by excising the loops.
4. **Signed-distance-field rendering.** Per pixel, compute the distance to the nearest segment (accelerated with a grid or segment texture). Water where d < w/2. Gives clean unions, soft banks and floodplain gradients.
5. **Asymmetric depth shading.** Real bends are deep at the outer cut bank and shallow over the inner point bar. Shade depth ≈ `d₀·(1 + k·C·v)`, with v ∈ [−1, 1] across the channel.
6. **Floodplain age map.** Rasterise when each cell was last occupied by water and shade it. This produces scroll-bar and meander-belt patterns like those in satellite imagery.

### B. Kinematic models (plausible behaviour, not derived)

7. **Local curvature only (R = C).** Bends grow, but symmetrically and in place, which looks wrong. Useful only as a baseline or debug mode.
8. **Howard–Knutson kernel (default above).** Upstream-weighted curvature gives downstream migration, skewed bends, and a characteristic meander wavelength emerging from noise. This is the recommended baseline.
9. **Heterogeneous erodibility.** Make E(x, y) a noise field (e.g. clay plugs or resistant banks). Produces irregular, more natural planforms. Can also make abandoned oxbow fills less erodible, which is realistic.
10. **Stochastic cutoffs.** Trigger cutoffs probabilistically when banks come near, mimicking flood-driven breakthrough, instead of a hard distance threshold. Chute cutoffs, across the point bar before the neck closes, are an optional extension.

### C. Physically derived models

11. **Ikeda–Parker–Sawai (1981) coefficients.** Migration ζ = E·u_b′, where the excess near-bank velocity solves the linearised shallow-water ODE
    `du_b′/ds + λ u_b′ = a·C + b·dC/ds`, with `λ = 2C_f/H`.
    Its solution is exactly the Howard–Knutson form: `u_b′ = b·C(s) + (a − bλ)∫₀^∞ e^{−λξ} C(s−ξ) dξ`.
    Derive Ω, Γ, α from physical inputs (friction C_f, depth H, velocity U, Froude number, scour factor A) instead of hand-tuning them. Caveat: the exact forms of a and b differ between papers; take them from a primary source (IPS 1981, or the review by Seminara 2006) rather than this summary.
12. **Two independent banks with self-formed width.** Track the outer and inner banks as separate curves, with erosion on one and accretion on the other. Width then evolves instead of being fixed (Parker et al. 2011, *ESPL*; Eke, Parker & Shimizu 2014, *JGR Earth Surface*). Significantly harder: banks must stay paired, and width can blow up or collapse if the rates are unbalanced.
13. **Beyond the linear model.** Full bar–bend interaction theory, sediment transport and bed evolution, variable discharge or flood hydrographs. Research-level; out of scope for a live toy, but noted for completeness.

### D. Floodplain built by the river

14. **Reworked ground.** Record when the channel last swept each cell. Young point-bar sand erodes fast until vegetation binds it, so the river reworks its own recent belt and resists old ground.
15. **Overbank deposition.** Floods raise the land near the channel by an amount that decays with distance, building natural levees and leaving backswamps low. Higher banks erode more slowly (Howard 1992, *Lowland Floodplain Rivers*).
16. **Lakes filled by the channel.** Lakes near the channel take in flood silt and fill fast; distant ones stay open far longer. Lakes cut at a small diversion angle take in bedload (Constantine et al. 2010).
17. **Scroll bars.** Floods leave a ridge along the inner bank of each migrating bend, so point bars show the ridge-and-swale pattern of satellite images.
18. **Crevasse splays.** A large flood breaches the levee at a sharp outer bend and spreads a fan of loose sand over the floodplain. Full avulsion is out of scope.

**Recommended path:** 8 + 1 first, to get the dynamics right. Then add 2, 3 and 5 for visual quality, and 6 and 9 for realism per unit effort. Option 11 is a good next step if physically meaningful parameters matter.

---

## Physics in one paragraph (for context)

Flow round a bend sets up a helical secondary current: centrifugal force pushes surface water outward, the tilted water surface pushes bed water inward. This moves fast water to the outer bank, which erodes (cut bank), and sweeps sediment to the inner bank, which accretes (point bar), so the channel shifts outward. Sharper bends erode faster, so a slightly sinuous channel is unstable and meanders grow. The flow adjusts to curvature over a friction-controlled distance (1/λ), so peak erosion lags downstream of the bend apex, and bends migrate downstream and skew. Nothing in the continuous dynamics limits growth. Neck cutoffs provide the saturation, giving a statistical steady state in sinuosity with a scattering of oxbows.

## Validation checks

- A sine perturbation on a straight channel grows **and** drifts downstream (checks the sign convention and the kernel).
- From random noise, a dominant meander wavelength emerges (on the order of ~10 channel widths in real rivers). Its value depends on α.
- Cutoffs occur, oxbows are left behind, and sinuosity plateaus rather than growing without bound.
- There are no self-intersections of the centerline between cutoffs. If they occur, reduce dt or increase d_cut.

## References

- Ikeda, Parker & Sawai (1981), "Bend theory of river meanders", *J. Fluid Mech.*
- Howard & Knutson (1984), "Sufficient conditions for river meandering: a simulation approach", *Water Resources Research*
- Seminara (2006), "Meanders", *J. Fluid Mech.* (review)
- Parker et al. (2011), *Earth Surface Processes and Landforms* (bank-migration framework)
- Eke, Parker & Shimizu (2014), *JGR: Earth Surface* (self-formed width)
- Stølum (1996), "River meandering as a self-organization process", *Science*
