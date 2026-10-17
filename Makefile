.PHONY: build test smoke serve fuzz

build: web/favicon.ico
	wasm-pack build wasm --release --target web --out-dir ../web/pkg

# The SVG drawn at 16, 32 and 48 pixels, for browsers and tools that do not
# read SVG icons.
web/favicon.ico: web/favicon.svg
	tmp=$$(mktemp -d) && \
	for s in 16 32 48; do rsvg-convert -w $$s -h $$s -o $$tmp/$$s.png $<; done && \
	magick $$tmp/16.png $$tmp/32.png $$tmp/48.png $@ && \
	rm -r $$tmp

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
