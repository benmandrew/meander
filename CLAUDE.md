# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

A live browser simulation of a meandering river: a Rust core compiled to WASM, drawn as SVG with parameter sliders. `PLAN.md` is the handoff document. It holds the current status, the numbered list of model options (1–13, referred to by number in commits), the tuning history, and the references. Read it before starting a new option, and update its checklist when one lands.

## Commands

- `make build`: build the WASM package into `web/pkg` (needs `wasm-pack`).
- `make serve`: build, then serve `web/` at http://localhost:8000. `?warmup=T` overrides the warm-up in simulated time.
- `make test`: `cargo test --release -p meander-core`. Tests run long simulations, so always use `--release`.
- One test: `cargo test --release -p meander-core <test_name>`, e.g. `sine_perturbation_grows_and_drifts_downstream`.
- Snapshots for tuning: `cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]`. Override any parameter with its upper-case name as an env var (`ALPHA=0.8`). `UP` and `DOWN` set hidden reach lengths in widths.

## Architecture

Three layers, with data flowing one way:

- `meander-core` is pure Rust with no dependencies. `Sim::step` resamples the centreline, computes Menger curvature, migrates points along their normals using the Howard–Knutson upstream-weighted kernel (an O(n) forward recurrence), smooths, clamps to the domain, then runs the hard neck cutoff and any flood-driven cutoffs. It resamples again after every cut. Flow runs from index 0 (upstream) to the last index. Geometry is `f64`. `field.rs` holds the fBm erodibility grid, at half a width per cell. Each step stamps the cells under the channel as fresh point-bar sand, and a cutoff stamps the oxbow footprint as a lake that fills to a clay plug. Floods lay overbank deposits on it, which slow erosion where banks are high and are drawn as levee contours. Migration reads the grid at the bank a point moves into, chosen by the sign of the rate alone, not at the centreline. Each step also removes any part of an oxbow that the channel overlaps; oxbows live for `oxbow_lifetime` as lakes, then `scar_lifetime` as scars. `ips.rs` derives Ω, Γ and α from hydraulic inputs. `rng.rs` is a local seeded PRNG; do not add `rand` or `getrandom`.
- `meander-wasm` wraps `Sim` as `River`. The simulated river is longer than the visible window: hidden reaches of `UPSTREAM` = 60 and `DOWNSTREAM` = 20 widths let bends develop before they come into view. `sync()` copies geometry into flat `f32` buffers exposed as raw pointers. JS must call `sync()` after stepping and rebuild every typed-array view, because WASM memory growth detaches old views.
- `web/` is plain ES modules with no bundler. `ui.js` owns the control panel, the frame loop and the warm-up (spread across frames). `main.js` draws the SVG, including seeded soil patches as vector blobs typed by the erodibility noise. The frontend is SVG only. A WebGL view existed in commit e6cd034 and was removed at the user's request; do not reintroduce it without asking. Likewise a raster floodplain map (canvas or embedded image) was tried and rejected; keep floodplain detail as vector shapes.

### Parameters are addressed by string name

Adding a simulation parameter touches four places: the `Params` field and its `Default`, `Params::NAMES`, the `Params::field` match (or a special case in `get`/`set` for non-`f64` values such as `cutoff_min_sep` and `physics`), and an entry in `SIM_CONTROLS` in `web/ui.js`. The snapshot example picks up env overrides from `NAMES` with no further change. Most length parameters are in channel widths, not world units.

With the `physics` flag on, every `Params::set` call re-derives `omega`, `gamma` and `alpha` from `friction`, `aspect`, `froude` and `scour`, so setting those three directly has no effect. The UI marks them `derived`.

## Constraints

- The hot path avoids `hypot`, `ln` and `cos`. They are software routines in WASM and made a step 3.6× slower than native. `Sim` keeps scratch buffers to avoid per-step allocation.
- The validation tests in `meander-core/src/lib.rs` pin the model's behaviour. A sine perturbation must grow and drift downstream (this checks the sign convention). A local-only kernel must not drift. Noise runs must cut off, never self-intersect, and keep sinuosity bounded. Run `make test` after any change to the dynamics.
- Commits use conventional prefixes and name the PLAN.md option where one applies, e.g. `feat: stochastic flood-driven cutoffs (option 10)`.
