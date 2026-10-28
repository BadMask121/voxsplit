#!/usr/bin/env bash
# Build the release engine binary (Apple Silicon). Output: engine/target/release/voxsplit-engine
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/engine"
cargo build --release --locked
echo "$ROOT/engine/target/release/voxsplit-engine"
