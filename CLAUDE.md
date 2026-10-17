# CLAUDE.md

A live browser simulation of a meandering river: a Rust core (`src/`, no dependencies) compiled to WASM (`wasm/`) and drawn as SVG by plain ES modules in `web/`. The panel shows only a speed slider; the simulation runs on its `Params` defaults.

Describe every change, in comments and prose, in years. One unit of simulated time is 2.5 years (`YEARS_PER_UNIT` in `ui.js`), the warm-up is about 1,250 years, and forest takes about 3,400 years to mature.

Read `docs/architecture.md` before changing a process or a renderer layer, and keep it current. Read `docs/development.md` for the hooks, CI, URL parameters and the smoke test.

## Commands

Tools come from the Nix flake (`direnv allow` or `nix develop`).

- `make build`, `make serve` (http://localhost:8000), `make smoke`, `make test`.
- One test: `cargo test --release -p meander-core <test_name>`. Always use `--release`.
- Tuning: `cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]`, with env overrides such as `ALPHA=0.8`. It prints step-time percentiles.
- The pre-commit hook checks only staged changes.

## Parameters

A new parameter touches the `Params` field and its `Default`, `Params::NAMES`, and the `Params::field` match (or `get`/`set` for non-`f64` values). Most lengths are in channel widths. With `physics` on (the default), `Params::set` derives `omega`, `gamma` and `alpha` from `friction`, `aspect`, `froude` and `scour`, so setting those three directly has no effect.

## Constraints

- Keep step time even: spread periodic grid work across steps in bands of rows, and carry searches across steps within a per-step `BUDGET`. Check with the snapshot percentiles.
- No `hypot`, `ln` or `cos` on the hot path. They are software routines in WASM and made a step 3.6× slower than native. Reuse `Sim`'s scratch buffers; do not allocate per step.
- Use the PRNG in `rng.rs`; do not add `rand` or `getrandom`. New stochastic processes draw from the `events` stream.
- JS must call `sync()` after stepping and rebuild every typed-array view, because WASM memory growth detaches old views.
- Run `make test` after any change to the dynamics. The validation tests in `src/lib.rs` pin the model's behaviour.
- SVG only. A WebGL view (commit e6cd034) and a raster floodplain map were both rejected; ask before reintroducing either. Keep floodplain detail as vector shapes.
- Commits use conventional prefixes, e.g. `feat: stochastic flood-driven cutoffs`.
