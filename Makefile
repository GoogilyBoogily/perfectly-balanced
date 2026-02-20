.PHONY: build dev test clean package fmt fmt-check lint ci

VERSION ?= 0.1.0
TARGET = x86_64-unknown-linux-musl

# Development build (native platform)
dev:
	cargo build

# Run locally for development
run:
	PB_CONFIG_PATH=./dev.cfg PB_DB_PATH=./dev.db PB_MNT_BASE=./test_mnt RUST_LOG=perfectly_balanced=debug cargo run

# Run tests
test:
	cargo test

# Production build (static musl binary for Unraid)
build:
	cross build --release --target $(TARGET)

# Create the Slackware .txz package
package: build
	bash build.sh $(VERSION)

# Clean build artifacts
clean:
	cargo clean
	rm -f *.db *.db-wal *.db-shm
	rm -rf packaging/*.txz

# Format code
fmt:
	cargo fmt --all

# Check formatting (CI mode)
fmt-check:
	cargo fmt --all -- --check

# Lint with strict clippy
lint:
	cargo clippy --all-targets -- -D warnings

# Check compilation without building
check:
	cargo check

# Full CI pipeline
ci: fmt-check lint test
	@echo "All checks passed."
