set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

fmt:
    cargo fmt --all -- --check

lint:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo nextest run --all-features

test-doc:
    cargo test --doc --all-features

check: fmt lint test test-doc

run *args:
    cargo run -- {{args}}

install prefix="${HOME}/.local":
    cargo install --path . --root "{{prefix}}" --locked

package:
    cargo build --profile dist --locked

security:
    cargo deny check

ci: check security
