# Development

Detailed notes on the tooling, moved out of `CLAUDE.md` so they load only when needed.

## Nix

The Nix flake supplies every tool (`direnv allow` once, or `nix develop`). The shell's `wasm-bindgen-cli` version is read from `Cargo.lock`, so after bumping the `wasm-bindgen` crate, nixpkgs must carry the matching `wasm-bindgen-cli_0_2_N`.

`nix build .#web-dist` builds the deployable site into `result/`, without `wasm-pack`.

## Hooks and CI

The pre-commit hook in `.githooks/` runs on a copy of the index, so only staged changes count. Staged Rust gets `cargo fmt --check` and `cargo clippy -- -D warnings`; staged Rust or `web/` also gets `make build` and the smoke test. The dev shell sets `core.hooksPath` to enable it.

CI (`.github/workflows/ci.yml`) runs fmt, clippy and `make test`'s tests in the same dev shell on every push to `main` and every PR, caching the Nix store and Cargo's build output. It skips the build and the smoke test.

## Make targets

- `make build`: build the WASM package into `web/pkg`.
- `make serve`: build, then serve `web/` at http://localhost:8000 through `serve.py`, which turns caching off (`Cache-Control: no-store`). This is for development only; a deployment must set its own caching headers.
- `make smoke`: build, then load the page in headless Chrome (`test/smoke.py`, through Playwright) for 20 units of warm-up and 8 s of running. It fails on any page or console error, or if the channel in `#view` stops being redrawn. It drives the installed Google Chrome; set `PLAYWRIGHT_BROWSERS_PATH` to use Playwright's own Chromium instead.
- `make test`: `cargo test --release -p meander-core`. Tests run long simulations, hence `--release`.

## URL parameters

`?warmup=T` overrides the warm-up in simulated time. `?seed=S&tick=N&view=W` (the panel's Link button) replays to step N and pauses. `<html data-idle>` marks when drawing has finished, for screenshots.

## Snapshots

`cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]` writes snapshots for tuning and prints step-time percentiles. Override any parameter with its upper-case name as an env var (`ALPHA=0.8`); the example reads the names from `Params::NAMES`. `UP` and `DOWN` set the hidden reach lengths in widths.
