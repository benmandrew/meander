# Handoff: Meandering River Simulation (Rust → WASM, live in browser)

## Status

Build with `make build`, run with `make serve` (http://localhost:8000; `?warmup=T` overrides the warm-up in simulated time). Tests: `make test`. Snapshots for tuning: `cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]`, with parameter overrides as upper-case env vars (e.g. `ALPHA=0.8`) and `UP`/`DOWN` for hidden reaches in widths.

The frontend is SVG only (`web/index.html`, `web/main.js` for drawing, `web/ui.js` for the panel and loop). A WebGL view with a bank mesh, depth shading and a floodplain age map existed in commit e6cd034 and was removed at the user's request.

- [x] Core: resample, Menger curvature, Howard–Knutson kernel (option 8), migration, smoothing, hashed-grid neck cutoffs with a spatial cooldown.
- [x] Validation tests: sine grows and drifts downstream; local-only kernel does not drift; noise run cuts off, never self-intersects, and sinuosity stays bounded.
- [x] SVG view: stroked channel and oxbow paths (options 1 and 2), faded past centrelines tracing the meander belt, Save SVG.
- [x] Heterogeneous erodibility (option 9): fBm noise field, and oxbow footprints become clay plugs.
- [x] Stochastic cutoffs (option 10): floods arrive at `flood_rate` per unit time, each with a magnitude u in [0, 1). A flood breaches necks narrower than `cutoff_dist + u·(breach_dist − cutoff_dist)` and cuts chutes where the arc is at least `chute_ratio / u` times a chord of up to 5 widths that does not cross the channel. Up to four cuts per flood; the hard cutoff at `cutoff_dist` stays as a backstop. Over the second half of a 30,000-step run, mean sinuosity falls from about 1.8 to 1.6 at `flood_rate` 0.1 and `chute_ratio` 3 (seeds 3 and 11).
- [x] Ikeda–Parker–Sawai coefficients (option 11), in `meander-core/src/ips.rs`, behind the `physics` flag (off by default). The reduction gives Ω = −1, Γ = (F² + A + 3)/2 and α = 2·C_f·(w/H) per width. The IPS equation is taken from the form in Sun et al. (1996); the 1981 paper itself was not available to check against. The defaults (C_f 0.01, w/H 30, F 0.3, A 2.89) give Γ ≈ 3.0 against the hand-tuned 2.5, which raised the cutoff count from 34 to 57 over 20,000 steps (seed 4).
- [x] Floodplain soils and oxbow infilling (option 2, revised). The channel removes any part of an oxbow it overlaps, splitting it if needed. Lakes plug from both ends, shrink to a crescent and turn murky over `oxbow_lifetime`, then stay as a marshy scar stroke for `scar_lifetime`. The SVG view scatters seeded soil patches as smooth Bézier blobs: broad clay, silt and sand patches typed by the erodibility noise at their centre, and small gravel and peat patches. Past centrelines are drawn as point-bar sand that grasses over with age. A raster floodplain map (option 6) was tried and removed at the user's request.

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
