.PHONY: lint format format-check test build clean

lint:
	cargo clippy -p factory -- -D warnings

format:
	cargo fmt --all

format-check:
	cargo fmt --all -- --check

test:
	cargo test -p factory

build:
	cargo build -p factory

clean:
	cargo clean
