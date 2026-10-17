.PHONY: build test serve

build:
	wasm-pack build wasm --release --target web --out-dir ../web/pkg

test:
	cargo test --release -p meander-core

serve: build
	python3 serve.py 8000
