.PHONY: build install test fmt clippy

ROOT := $(abspath ../..)

build:
	cd $(ROOT) && cargo build -p cli --release

install: build
	mkdir -p ~/.local/bin
	-~/.local/bin/factory stop 2>/dev/null || true
	install -m 755 $(ROOT)/target/release/factory ~/.local/bin/factory.new
	mv -f ~/.local/bin/factory.new ~/.local/bin/factory
	cp $(ROOT)/target/release/afc ~/.local/bin/afc 2>/dev/null || true
	cp $(ROOT)/target/release/factory ~/.cargo/bin/factory 2>/dev/null || true
	cp $(ROOT)/target/release/afc ~/.cargo/bin/afc 2>/dev/null || true
	chmod +x ~/.local/bin/factory ~/.local/bin/afc ~/.cargo/bin/factory ~/.cargo/bin/afc 2>/dev/null || true

test:
	cd $(ROOT) && cargo test -p cli

fmt:
	cd $(ROOT) && cargo fmt --all

clippy:
	cd $(ROOT) && cargo clippy -p cli -- -D warnings
