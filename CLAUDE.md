# CLAUDE.md

A live browser simulation of a meandering river: a Rust core compiled to WASM, drawn as SVG. The panel shows only a speed slider; the simulation runs on its `Params` defaults.

The simulation runs over years, not weeks. One unit of simulated time is 2.5 years (`YEARS_PER_UNIT` in `ui.js`), the warm-up is about 1,250 years, and forest takes about 3,400 years to mature. Describe every change, in code comments and in prose, on that timescale.

Read `docs/architecture.md` before changing a process or a renderer layer: it describes each model process, its parameters, and how `main.js` draws it. Keep it current when behaviour changes.

## Commands

The Nix flake supplies every tool (`direnv allow` once, or `nix develop`). The shell's `wasm-bindgen-cli` version is read from `Cargo.lock`, so after bumping the `wasm-bindgen` crate, nixpkgs must carry the matching `wasm-bindgen-cli_0_2_N`.

- The pre-commit hook in `.githooks/` runs `cargo fmt --check` and `cargo clippy -- -D warnings`. The dev shell sets `core.hooksPath` to enable it.
- `nix build .#web-dist`: build the deployable site into `result/`, without `wasm-pack`.
- `make build`: build the WASM package into `web/pkg`.
- `make serve`: build, then serve `web/` at http://localhost:8000. `?warmup=T` overrides the warm-up in simulated time.
- `make test`: `cargo test --release -p meander-core`. Tests run long simulations, so always use `--release`. One test: `cargo test --release -p meander-core <test_name>`.
- Snapshots for tuning: `cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]`. Override any parameter with its upper-case name as an env var (`ALPHA=0.8`); `UP` and `DOWN` set hidden reach lengths in widths. It also prints step-time percentiles.

## Architecture

Three layers, with data flowing one way:

- `meander-core` (`src/`): pure Rust, no dependencies. `Sim::step` migrates the centreline with the Howard–Knutson kernel, then runs cutoffs. Flow runs from index 0 (upstream) to the last index. Floodplain state lives on a grid at half a width per cell. Modules: `field.rs` (erodibility, vegetation), `walls.rs` (valley walls), `land.rs` (landholdings), `wood.rs` (log jams), `tributary.rs`, `crevasse.rs` (splays, avulsion), `mills.rs`, `ips.rs` (hydraulics to Ω, Γ, α), `rng.rs`.
- `meander-wasm` (`wasm/`): wraps `Sim` as `River`, with hidden reaches upstream and downstream of the visible window. `sync*()` copies geometry into flat `f32` buffers exposed as raw pointers. JS must call `sync()` after stepping and rebuild every typed-array view, because WASM memory growth detaches old views.
- `web/`: plain ES modules, no bundler. `ui.js` owns the panel, the frame loop and the warm-up; `main.js` draws three stacked SVGs (`#bars`, `#ground`, `#view`).

## Parameters

Adding a simulation parameter touches three places: the `Params` field and its `Default`, `Params::NAMES`, and the `Params::field` match (or a special case in `get`/`set` for non-`f64` values such as `cutoff_min_sep` and `physics`). The snapshot example picks up env overrides from `NAMES`. Most length parameters are in channel widths.

With the `physics` flag on (the default), every `Params::set` re-derives `omega`, `gamma` and `alpha` from `friction`, `aspect`, `froude` and `scour`, so setting those three directly has no effect.

## Constraints

- Step time must stay even. Spread periodic grid work across steps in bands of rows, and carry searches across steps within a per-step `BUDGET`. Check with the snapshot example's step-time percentiles.
- The hot path avoids `hypot`, `ln` and `cos`: they are software routines in WASM and made a step 3.6× slower than native. `Sim` keeps scratch buffers to avoid per-step allocation.
- Use the local PRNG in `rng.rs`; do not add `rand` or `getrandom`. New stochastic processes draw from the `events` stream so the river's own draws stay unchanged.
- The validation tests in `src/lib.rs` pin the model's behaviour (sign convention, drift, cutoffs, no self-intersection, bounded sinuosity). Run `make test` after any change to the dynamics.
- The frontend is SVG only. A WebGL view (commit e6cd034) and a raster floodplain map were both rejected; do not reintroduce either without asking. Keep floodplain detail as vector shapes.
- Commits use conventional prefixes, e.g. `feat: stochastic flood-driven cutoffs`.
