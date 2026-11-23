.PHONY: lint format format-check test build clean

lint:
	cargo clippy -p cli -- -D warnings

format:
	cargo fmt --all

format-check:
	cargo fmt --all -- --check

test:
	cargo test -p cli

build:
	cargo build -p cli

clean:
	cargo clean
