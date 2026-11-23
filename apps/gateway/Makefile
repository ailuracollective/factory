.PHONY: lint format format-check test dev start build clean deploy

lint:
	cargo clippy --target wasm32-unknown-unknown -- -D warnings

format:
	cargo fmt

format-check:
	cargo fmt --check

test:
	cargo test

dev:
	npx wrangler dev

start: dev

build:
	cargo build --target wasm32-unknown-unknown --release

clean:
	cargo clean

deploy:
	npx wrangler deploy
