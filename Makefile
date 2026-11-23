.PHONY: lint format format-check test build install clean

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

install:
	$(MAKE) -C apps/cli install

clean:
	cargo clean
