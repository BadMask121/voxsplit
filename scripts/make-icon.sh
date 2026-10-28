#!/usr/bin/env bash
# Regenerate macos/AppIcon.icns from assets/icon.svg (commit the result).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

swiftc -O -o "$WORK/render-svg" "$ROOT/scripts/render-svg.swift"
SET="$WORK/AppIcon.iconset"
mkdir -p "$SET"
for size in 16 32 128 256 512; do
  "$WORK/render-svg" "$ROOT/assets/icon.svg" "$SET/icon_${size}x${size}.png" "$size"
  "$WORK/render-svg" "$ROOT/assets/icon.svg" "$SET/icon_${size}x${size}@2x.png" "$((size * 2))"
done
iconutil -c icns -o "$ROOT/macos/AppIcon.icns" "$SET"
echo "$ROOT/macos/AppIcon.icns"
