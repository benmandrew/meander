# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

A live browser simulation of a meandering river: a Rust core compiled to WASM, drawn as SVG with parameter sliders.

## Commands

The Nix flake supplies every tool below. `.envrc` loads its dev shell through direnv (run `direnv allow` once), or enter it with `nix develop`. The shell's `wasm-bindgen-cli` version is read from `Cargo.lock`, so after bumping the `wasm-bindgen` crate, nixpkgs must carry the matching `wasm-bindgen-cli_0_2_N`.

- `nix build .#web-dist`: build the deployable site (HTML, JS, CSS and `pkg/`) into `result/`, without `wasm-pack`.
- `make build`: build the WASM package into `web/pkg` (needs `wasm-pack`).
- `make serve`: build, then serve `web/` at http://localhost:8000. `?warmup=T` overrides the warm-up in simulated time.
- `make test`: `cargo test --release -p meander-core`. Tests run long simulations, so always use `--release`.
- One test: `cargo test --release -p meander-core <test_name>`, e.g. `sine_perturbation_grows_and_drifts_downstream`.
- Snapshots for tuning: `cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]`. Override any parameter with its upper-case name as an env var (`ALPHA=0.8`). `UP` and `DOWN` set hidden reach lengths in widths.

## Architecture

Three layers, with data flowing one way:

- `meander-core` is pure Rust with no dependencies. `Sim::step` resamples the centreline, computes Menger curvature, migrates points along their normals using the Howard–Knutson upstream-weighted kernel (an O(n) forward recurrence), smooths, clamps to the domain, then runs the hard neck cutoff and any flood-driven cutoffs. It resamples again after every cut. Flow runs from index 0 (upstream) to the last index. Geometry is `f64`. `field.rs` holds the fBm erodibility grid, at half a width per cell. Each step stamps the cells under the channel as fresh point-bar sand, and a cutoff stamps the oxbow footprint as a lake that fills to a clay plug. Floods lay overbank deposits on it, which slow erosion where banks are high and are drawn as levee contours. Migration reads the grid at the bank a point moves into, chosen by the sign of the rate alone, not at the centreline. Each step also cuts an oxbow where the channel centreline crosses one of its segments or comes within `REWORK_RADIUS` (0.1 widths) of a point, so a channel alongside a lake hides it under the water before removing it. Lake silt is read only from cells still stamped as lake; each lake fills over `oxbow_lifetime`, faster where floods lay silt in it or bedload enters a connected entrance, then stays for `scar_lifetime` as a scar. Floods also leave scroll bars along inner banks (`Sim::scrolls`), which are drawn but do not affect the dynamics. Large floods can breach a levee at a sharp bend and lay a crevasse splay (`Sim::splays`), a fan of raised, loose sand stamped into the field. Each point carries a local width (`Sim::widths`, in multiples of `width`), after the bank-pull and bar-push model of Parker et al. (2011): the outer bank retreats at the migration rate times its erodibility, the inner bank advances at `bar_push` times the rate, and the width relaxes to 1 over `width_time`. Floods widen bends by `flood_widening`. The centreline keeps the Howard–Knutson rate, so width changes the planform only through the bank reads and the cutoffs, which fire when the banks meet. Each oxbow keeps the widths of the channel it was cut from, for its footprint, its bedload entrance and its drawn lake and scar. `ips.rs` derives Ω, Γ and α from hydraulic inputs. The neck search and the oxbow and scroll rework share one hashed grid, rebuilt only when `line_version` shows the centreline has moved. `oxbow_version` counts changes to oxbow shapes, not fills, so `sync` can skip recomputing their heights. `rng.rs` is a local seeded PRNG; do not add `rand` or `getrandom`.
- `meander-wasm` wraps `Sim` as `River`. The simulated river is longer than the visible window: hidden reaches of `UPSTREAM` = 60 and `DOWNSTREAM` = 20 widths let bends develop before they come into view. `sync()` copies geometry into flat `f32` buffers exposed as raw pointers. JS must call `sync()` after stepping and rebuild every typed-array view, because WASM memory growth detaches old views. `sync_scrolls` and `sync_splays` can grow memory too, so `draw` runs the floodplain layers that call them before it builds its own views. `sync_line` copies only the centreline and its widths, for the warm-up's point-bar capture.
- `web/` is plain ES modules with no bundler. `ui.js` owns the control panel, the frame loop and the warm-up (spread across frames). `main.js` draws the SVG, including seeded soil patches as vector blobs typed by the erodibility noise. The channel is a filled outline at the local width in runs of 24 segments, overlapping by a segment so no seam shows, with the inner bank clamped short of the centre of tight bends. It draws into three stacked SVGs: `#bars` holds the old point bars, `#ground` the floodplain, and `#view` the channel and lakes. The first two are compositing layers, so a moving channel does not repaint them. The frame loop draws only when a step was taken or a setting changed, and `set` skips attribute writes whose value has not changed. The frontend is SVG only. A WebGL view existed in commit e6cd034 and was removed at the user's request; do not reintroduce it without asking. Likewise a raster floodplain map (canvas or embedded image) was tried and rejected; keep floodplain detail as vector shapes.

### Parameters are addressed by string name

Adding a simulation parameter touches four places: the `Params` field and its `Default`, `Params::NAMES`, the `Params::field` match (or a special case in `get`/`set` for non-`f64` values such as `cutoff_min_sep` and `physics`), and an entry in `SIM_CONTROLS` in `web/ui.js`. The snapshot example picks up env overrides from `NAMES` with no further change. Most length parameters are in channel widths, not world units.

With the `physics` flag on, every `Params::set` call re-derives `omega`, `gamma` and `alpha` from `friction`, `aspect`, `froude` and `scour`, so setting those three directly has no effect. The UI marks them `derived`.

## Constraints

- The hot path avoids `hypot`, `ln` and `cos`. They are software routines in WASM and made a step 3.6× slower than native. `Sim` keeps scratch buffers to avoid per-step allocation.
- The validation tests in `meander-core/src/lib.rs` pin the model's behaviour. A sine perturbation must grow and drift downstream (this checks the sign convention). A local-only kernel must not drift. Noise runs must cut off, never self-intersect, and keep sinuosity bounded. Run `make test` after any change to the dynamics.
- Commits use conventional prefixes, e.g. `feat: stochastic flood-driven cutoffs`.
