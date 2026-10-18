# Development

Detailed notes on the tooling, moved out of `CLAUDE.md` so they load only when needed.

## Nix

The Nix flake supplies every tool (`direnv allow` once, or `nix develop`). The shell's `wasm-bindgen-cli` version is read from `Cargo.lock`, so after bumping the `wasm-bindgen` crate, nixpkgs must carry the matching `wasm-bindgen-cli_0_2_N`.

`nix build .#web-dist` builds the deployable site into `result/`, without `wasm-pack`.

## Hooks and CI

The pre-commit hook in `.githooks/` runs on a copy of the index, so only staged changes count. Staged Rust gets `cargo fmt --check` and `cargo clippy -- -D warnings`; staged Rust or `web/` also gets `make build` and the smoke test. The dev shell sets `core.hooksPath` to enable it.

CI (`.github/workflows/ci.yml`) runs fmt, clippy and `make test`'s tests in the same dev shell on every push to `main` and every PR, caching the Nix store and Cargo's build output. It skips the build and the smoke test.

## Make targets

- `make build`: build the WASM package into `web/pkg`, and `web/favicon.ico` from `web/favicon.svg`.
- `make serve`: build, then serve `web/` at http://localhost:8000 through `serve.py`, which turns caching off (`Cache-Control: no-store`). This is for development only; a deployment must set its own caching headers.
- `make smoke`: build, then load the page in headless Chrome (`test/smoke.py`, through Playwright) on seed 1 for 20 units of warm-up and 8 s of running. It fails on any page or console error, or if the channel in `#view` stops being redrawn. It drives the installed Google Chrome; set `PLAYWRIGHT_BROWSERS_PATH` to use Playwright's own Chromium instead.
- `make video`: build, then render the river in headless Chrome (`test/record.py`) to a lossless `video/master.mkv`, and encode that to an MP4 (H.264, for a `<video autoplay muted loop playsinline>` tag) and an animated WebP (for the README). `make video-encode` encodes the master again, so the encoder settings (`CRF`, `MP4_WIDTH`, `WEBP_WIDTH`, `WEBP_FPS`, `WEBP_QUALITY`) can be tried without rendering. Each file is named for its settings, such as `video/meander-w1920-crf20.mp4` and `video/meander-w800-f15-q70.webp`, and an encode skips any file already made from the current master, so changing one setting makes only that file; a render replaces the master. `SEED` (1), `TICK` (the end of the warm-up), `SPEED` (8 units a second, 20 years), `SECONDS` (20), `FPS` (30) and `WIDTH`×`HEIGHT` (1920×1080) set the clip. The page runs on a virtual clock, one frame at a time, so each frame is drawn whole and evenly spaced whatever the machine's speed. `JOBS` browsers (half the cores) each replay to the start of their share of the clip and render it; four took 26 s for the default clip on an M1, against 50 s for one and 37 s for eight. GitHub plays a video in a README only from a `github.com/user-attachments` URL, which needs a manual upload, but it shows a committed WebP as an image.
- `make test`: `cargo test --release -p meander-core`. Tests run long simulations, hence `--release`.
- `make fuzz`: run the river as the page does for `RUNS` (32) random seeds and window widths, `YEARS` (2,500) each after the warm-up, checking `Sim::check` after every step: a centreline inside the domain that does not cross itself, local widths in bounds, whole oxbows, and no building across a stream. Each failure prints its seed, step and a replay link for `make serve`. `BASE=N` repeats a sweep; the default draws from the clock and is printed. 16 runs take about 20 s on 8 cores.

## URL parameters

`?warmup=T` overrides the warm-up in simulated time. The page starts from a random seed; `?seed=S` sets it. `?seed=S&tick=N&view=W` (the panel's Link button) replays to step N and pauses. `<html data-idle>` marks when drawing has finished, for screenshots.

## Snapshots

`cargo run --release -p meander-core --example snapshot -- <out_dir> [steps] [every] [seed]` writes snapshots for tuning and prints step-time percentiles. Without `seed` it draws one from the clock and prints it on the first line. Override any parameter with its upper-case name as an env var (`ALPHA=0.8`); the example reads the names from `Params::NAMES`. `UP` and `DOWN` set the hidden reach lengths in widths.
