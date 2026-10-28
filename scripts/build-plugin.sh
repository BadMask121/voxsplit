#!/usr/bin/env bash
# Build the JUCE plugin (VST3 + AU + Standalone), embed the engine and models,
# sign, copy to dist/, and install the VST3/AU into ~/Library/Audio/Plug-Ins.
#   --no-install   build into dist/ only
#   JUCE_SOURCE_DIR=<path>   use a local JUCE 9.0.4 checkout instead of downloading
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INSTALL=1
[[ "${1:-}" == "--no-install" ]] && INSTALL=0

"$ROOT/scripts/fetch-models.sh"
"$ROOT/scripts/build-engine.sh" >/dev/null

BUILD="$ROOT/plugin/build/cmake"
JUCE_SOURCE_DIR="${JUCE_SOURCE_DIR:-$ROOT/plugin/build/JUCE}"
CONFIGURE=(-S "$ROOT/plugin" -B "$BUILD" -DCMAKE_BUILD_TYPE=Release)
[[ -f "$JUCE_SOURCE_DIR/CMakeLists.txt" ]] && CONFIGURE+=(-DFETCHCONTENT_SOURCE_DIR_JUCE="$JUCE_SOURCE_DIR")
cmake "${CONFIGURE[@]}" >/dev/null
cmake --build "$BUILD" --config Release -j "$(sysctl -n hw.ncpu)"

ART="$BUILD/VoxSplit_artefacts/Release"
DIST="$ROOT/dist"
# Bundles are embedded and signed outside the checkout: if the repo lives in an
# iCloud-synced folder, the file provider stamps com.apple.FinderInfo onto new bundle
# folders, and codesign rejects them. dist/ gets clean copies.
STAGE="${VOXSPLIT_STAGE:-$HOME/Library/Caches/VoxSplit/stage}"
mkdir -p "$DIST" "$STAGE"
for bundle in "VST3/VoxSplit.vst3" "AU/VoxSplit.component" "Standalone/VoxSplit.app"; do
  name="$(basename "$bundle")"
  # The Standalone build is a test harness for the plugin UI; keep it apart from the SwiftUI app.
  [[ "$name" == "VoxSplit.app" ]] && name="VoxSplit Plugin Standalone.app"
  rm -rf "${STAGE:?}/$name" "${DIST:?}/$name"
  # No resource forks / xattrs: JUCE's Finder metadata also breaks codesign.
  ditto --norsrc --noextattr "$ART/$bundle" "$STAGE/$name"
  "$ROOT/scripts/embed.sh" "$STAGE/$name"
  ditto --norsrc --noextattr "$STAGE/$name" "$DIST/$name"
done

if [[ $INSTALL == 1 ]]; then
  PLUGINS="$HOME/Library/Audio/Plug-Ins"
  mkdir -p "$PLUGINS/VST3" "$PLUGINS/Components"
  rm -rf "$PLUGINS/VST3/VoxSplit.vst3" "$PLUGINS/Components/VoxSplit.component"
  ditto --norsrc --noextattr "$STAGE/VoxSplit.vst3" "$PLUGINS/VST3/VoxSplit.vst3"
  ditto --norsrc --noextattr "$STAGE/VoxSplit.component" "$PLUGINS/Components/VoxSplit.component"
  codesign --verify --deep --strict "$PLUGINS/VST3/VoxSplit.vst3" "$PLUGINS/Components/VoxSplit.component"
  # Make the AU visible without a logout.
  killall -9 AudioComponentRegistrar 2>/dev/null || true
  echo "installed VoxSplit.vst3 and VoxSplit.component into $PLUGINS"
fi
