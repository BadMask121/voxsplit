#!/usr/bin/env bash
# Every automated check: engine (unit + CLI), plugin (headless job/preview), app (Swift Testing).
# Needs the models (scripts/fetch-models.sh); builds what it tests.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRATCH="${VOXSPLIT_SWIFT_BUILD:-$HOME/Library/Caches/VoxSplit/swift-build}"
JUCE_SOURCE_DIR="${JUCE_SOURCE_DIR:-$ROOT/plugin/build/JUCE}"

"$ROOT/scripts/fetch-models.sh"

echo "== engine"
(cd "$ROOT/engine" && cargo clippy --release --all-targets -- -D warnings && cargo test --release)

echo "== plugin"
CONFIGURE=(-S "$ROOT/plugin" -B "$ROOT/plugin/build/cmake" -DCMAKE_BUILD_TYPE=Release)
[[ -f "$JUCE_SOURCE_DIR/CMakeLists.txt" ]] && CONFIGURE+=(-DFETCHCONTENT_SOURCE_DIR_JUCE="$JUCE_SOURCE_DIR")
cmake "${CONFIGURE[@]}" >/dev/null
cmake --build "$ROOT/plugin/build/cmake" --config Release --target VoxSplitTests -j "$(sysctl -n hw.ncpu)" >/dev/null
"$ROOT/plugin/build/cmake/VoxSplitTests_artefacts/Release/VoxSplitTests" | tail -1

echo "== app"
swift test --package-path "$ROOT/macos" --scratch-path "$SCRATCH"
