.PHONY: build test smoke serve fuzz

build:
	wasm-pack build wasm --release --target web --out-dir ../web/pkg

test:
	cargo test --release -p meander-core

# Seed fuzzer: RUNS random seeds and window widths for YEARS each.
RUNS ?= 32
YEARS ?= 2500
fuzz:
	cargo run --release -p meander-core --example fuzz -- $(RUNS) $(YEARS) $(BASE)

smoke: build
	python3 test/smoke.py

serve: build
	python3 serve.py 8000
