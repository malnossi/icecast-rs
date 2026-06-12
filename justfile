# justfile for High-Concurrency Rust Icecast Studio
# Run `just` or `just --list` to see available recipes

# List available recipes
default:
    @just --list

# Runs the server in development mode using cargo run
run:
    @echo "Running the Icecast server in development mode..."
    cargo run --bin icecast-rs

build:
    @echo "Building highly optimized production release binary..."
    cargo build --release --bin icecast-rs

# Run code quality audit loop (fmt and clippy)
lint:
    @echo "Running cargo fmt..."
    cargo fmt --check
    @echo "Running cargo clippy..."
    cargo clippy -- -D warnings
