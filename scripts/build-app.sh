#!/usr/bin/env bash
# Build the SwiftUI app and assemble dist/VoxSplit.app with the engine and models embedded.
#   VOXSPLIT_SWIFT_BUILD=<dir>   SwiftPM scratch path (default: ~/Library/Caches/VoxSplit/swift-build)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Outside the checkout on purpose: if the repo lives in an iCloud-synced folder, the file
# provider stamps Finder metadata onto bundles in .build/ and codesign rejects them.
SCRATCH="${VOXSPLIT_SWIFT_BUILD:-$HOME/Library/Caches/VoxSplit/swift-build}"

"$ROOT/scripts/fetch-models.sh"
"$ROOT/scripts/build-engine.sh" >/dev/null

swift build -c release --package-path "$ROOT/macos" --scratch-path "$SCRATCH" --arch arm64
BIN="$(swift build -c release --package-path "$ROOT/macos" --scratch-path "$SCRATCH" --arch arm64 --show-bin-path)"

# Assemble and sign outside the checkout (same iCloud reason), then copy into dist/.
STAGE="${VOXSPLIT_STAGE:-$HOME/Library/Caches/VoxSplit/stage}"
APP="$STAGE/VoxSplit.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp -f "$BIN/VoxSplit" "$APP/Contents/MacOS/VoxSplit"
cp -f "$ROOT/macos/Info.plist" "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"
"$ROOT/scripts/embed.sh" "$APP"

mkdir -p "$ROOT/dist"
rm -rf "$ROOT/dist/VoxSplit.app"
ditto --norsrc --noextattr "$APP" "$ROOT/dist/VoxSplit.app"
echo "$ROOT/dist/VoxSplit.app"
