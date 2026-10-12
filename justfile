# ── unifly justfile ─────────────────────────────────────────────
# https://github.com/hyperb1iss/unifly

set dotenv-load

# List available recipes
default:
    @just --list --unsorted

# ── Build ───────────────────────────────────────────────────────

# Build all crates (debug)
build:
    cargo build --workspace

# Build all crates (release)
build-release:
    cargo build --workspace --release

# ── Install ─────────────────────────────────────────────────────

# Install unifly (CLI + TUI + graphics charts)
install:
    cargo install --path crates/unifly

# Install CLI-only binary
install-cli:
    cargo install --path crates/unifly --no-default-features --features cli

# ── Quality ─────────────────────────────────────────────────────

# Run all checks (format + maintainability + clippy + test)
check: fmt-check maintainability clippy test

# Run clippy with workspace lints
clippy:
    cargo clippy --workspace --all-targets

# Auto-fix clippy + formatting
fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty
    cargo fmt --all
    npx prettier --write .

# Format all code
fmt:
    cargo fmt --all
    npx prettier --write .

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check
    npx prettier --check .

# Check maintainability hotspots for line-count regressions
maintainability:
    scripts/check-maintainability.sh

# Lint = format check + maintainability + clippy
lint: fmt-check maintainability clippy

# ── Test ────────────────────────────────────────────────────────

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run tests for a specific crate
test-crate crate:
    cargo test -p {{crate}}

# Start the local UniFi controller used for e2e testing
e2e-up:
    docker compose -f tests/e2e/docker-compose.yml up -d

# Stop and remove the local e2e controller
e2e-down:
    docker compose -f tests/e2e/docker-compose.yml down -v --remove-orphans

# Wait until the local e2e controller is ready
e2e-wait:
    tests/e2e/wait-for-controller.sh

# Compile the gated e2e test binary without running it
e2e-build:
    cargo test -p unifly --features e2e --test e2e_test --no-run

# Run the gated e2e test suite against a running controller
e2e-test:
    cargo test -p unifly --features e2e --test e2e_test -- --test-threads=1

# Full e2e lifecycle: start controller, wait, test, tear down
e2e:
    #!/usr/bin/env bash
    set -euo pipefail
    cleanup() { just e2e-down; }
    trap cleanup EXIT
    just e2e-up
    just e2e-wait
    just e2e-test

# Update insta snapshots
snap-review:
    cargo insta review

# ── Run ─────────────────────────────────────────────────────────

# Run the CLI with args
cli *args:
    cargo run -p unifly -- {{args}}

# Run the TUI dashboard
tui *args:
    cargo run -p unifly -- tui {{args}}

# ── Docs ────────────────────────────────────────────────────────

# Generate rustdoc for the workspace
doc:
    cargo doc --workspace --no-deps --open

# Build the Zola docs site + generate llms.txt
docs-build:
    cd docs && zola build && ./scripts/gen-llms-txt.sh

# Serve the docs site with live reload
docs-serve:
    cd docs && zola serve

# ── Clean ───────────────────────────────────────────────────────

# Remove build artifacts
clean:
    cargo clean

# ── Release ─────────────────────────────────────────────────────

# Dry-run cargo-dist
dist-plan:
    cargo dist plan

# Build distributable artifacts
dist-build:
    cargo dist build

# Move the CHANGELOG [Unreleased] notes under a dated version heading
changelog-cut version:
    scripts/changelog-cut.sh {{version}}

# Update AUR package for a new release
aur-update version:
    cd aur && ./update-aur.sh {{version}}
