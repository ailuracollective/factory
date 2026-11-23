# Makefile for bot (Rust)

.PHONY: help lint format fmt format-check test dev start build clean

help:
	@echo "Available commands:"
	@echo "  make lint          - Run clippy (lint)"
	@echo "  make format        - Format code with rustfmt"
	@echo "  make format-check  - Check formatting without modifying"
	@echo "  make test          - Run tests"
	@echo "  make dev           - Run in development mode (cargo run)"
	@echo "  make start         - Run in release mode (cargo run --release)"
	@echo "  make build         - Build in release mode"
	@echo "  make clean         - Clean build artifacts"

lint:
	cargo clippy --all-targets --all-features -- -D warnings

format fmt:
	cargo fmt

format-check:
	cargo fmt --check

test:
	cargo test

dev:
	@if [ -f .env ]; then set -a; . ./.env; set +a; fi; cargo run

start:
	@if [ -f .env ]; then set -a; . ./.env; set +a; fi; cargo run --release

build:
	cargo build --release

clean:
	cargo clean