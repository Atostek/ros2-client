#!/bin/bash

# Run formatter
echo cargo +nightly-2026-07-22 fmt
cargo +nightly-2026-07-22 fmt

# Run linter
echo cargo +nightly clippy
cargo +nightly clippy

# Run linter on examples
echo cargo +nightly clippy --examples
cargo +nightly clippy --examples