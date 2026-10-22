#!/bin/bash

# Run formatter
echo cargo +nightly fmt
cargo +nightly fmt

# Run linter
echo cargo +nightly clippy
cargo +nightly clippy --lib --bins --examples -- -D warnings

echo cargo +nightly clippy --features zenoh
cargo +nightly clippy --no-default-features --features zenoh --lib --bins --examples -- -D warnings

