.PHONY: lint format format-check test dev start build clean

lint:
	cargo clippy -- -D warnings

format:
	cargo fmt

format-check:
	cargo fmt --check

test:
	cargo test

dev:
	cargo run

start:
	cargo run --release

build:
	cargo build --release

clean:
	cargo clean
