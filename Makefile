.PHONY: build test smoke serve

build:
	wasm-pack build wasm --release --target web --out-dir ../web/pkg

test:
	cargo test --release -p meander-core

smoke: build
	python3 test/smoke.py

serve: build
	python3 serve.py 8000
