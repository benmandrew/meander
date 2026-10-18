# Development

## Nix

The flake supplies every tool. The shell's `wasm-bindgen-cli` version is read from `Cargo.lock`, so after bumping the `wasm-bindgen` crate, nixpkgs must carry the matching `wasm-bindgen-cli_0_2_N`. `nix build .#web-dist` builds the deployable site into `result/`.

## Hooks and CI

The pre-commit hook in `.githooks/` checks a copy of the index. Staged Rust gets `cargo fmt --check` and `cargo clippy -- -D warnings`. Staged Rust or `web/` also gets `make build` and the smoke test. CI runs fmt, clippy and the tests on every push to `main` and every PR, without the build or the smoke test.

## Make targets

- `make serve`: build, then serve `web/` at http://localhost:8000 with caching off.
- `make smoke`: load the page in headless Chrome (`test/smoke.py`, Playwright). It fails on any page or console error, or if the channel stops being redrawn. Set `PLAYWRIGHT_BROWSERS_PATH` to use Playwright's Chromium in place of the installed Chrome.
- `make fuzz`: run `RUNS` random seeds and window widths for `YEARS` each, checking `Sim::check` after every step. Each failure prints its seed, step and a replay link. `BASE=N` repeats a sweep.
- `make video`: render the river in headless Chrome (`test/record.py`) to a lossless `video/master.mkv`, then encode an MP4 and an animated WebP named for their settings. `make video-encode` re-encodes without rendering. See the `Makefile` for the settings.

## URL parameters

`?seed=S` sets the seed and `?warmup=T` the warm-up, in simulated units. `?seed=S&tick=N&view=W`, from the panel's Link button, replays to step N and pauses.

## Snapshots

The snapshot example writes SVGs for tuning and prints step-time percentiles. Without a seed it draws one and prints it on the first line. Any parameter can be overridden with its upper-case name as an env var, and `UP` and `DOWN` set the hidden reach lengths in widths.
