# ==============================================================================
#  local-tools — justfile
# ==============================================================================

# Default recipe: list available recipes
default:
    @just --list

# ------------------------------------------------------------------------------
# Development & Quality
# ------------------------------------------------------------------------------

# Check syntax and types across all workspace targets
check:
    cargo check --workspace --all-targets

# Run tests across all workspace members
test:
    cargo test --workspace

# Run clippy linter across all workspace targets with warnings as errors
lint:
    cargo clippy --all-targets -- -D warnings

# Format all code files
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt --check

# Full CI / QA gate (format check, lint, test)
qa: fmt-check lint test

# ------------------------------------------------------------------------------
# Build
# ------------------------------------------------------------------------------

# Build all crates in debug mode
build:
    cargo build --workspace

# Build all crates in release mode
build-release:
    cargo build --workspace --release

# ------------------------------------------------------------------------------
# Installation
# ------------------------------------------------------------------------------

# Install a single tool into ~/.cargo/bin (e.g. `just install webdriver`)
install tool:
    cargo install --path "crates/{{tool}}" --force

# Install all CLI tools into ~/.cargo/bin
install-all:
    @for tool in scaffold portkill jwt devclean fanout webdriver; do \
        echo "==> Installing $tool..."; \
        cargo install --path "crates/$tool" --force; \
    done

# ------------------------------------------------------------------------------
# Housekeeping
# ------------------------------------------------------------------------------

# Clean build artifacts
clean:
    cargo clean
