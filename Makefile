.PHONY: build test serve

build:
	wasm-pack build wasm --release --target web --out-dir ../web/pkg

test:
	cargo test --release -p meander-core

serve: build
	python3 -m http.server 8000 --bind 127.0.0.1 --directory web
