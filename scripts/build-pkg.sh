#!/usr/bin/env bash
# Build dist/VoxSplit-<version>.pkg: the app, the VST3/AU plugins and the `voxsplit`
# command, all sharing one copy of the engine and models in
# /Library/Application Support/VoxSplit. Writes dist/VoxSplit-<version>.pkg.sha256 too.
#   VOXSPLIT_SIGN_IDENTITY="Developer ID Application: …"   sign the code (default: ad-hoc)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
die() { echo "error: $*" >&2; exit 1; }

# One version everywhere (semver): the engine, the plugin and the app.
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/engine/Cargo.toml" | head -1)"
grep -q "project(VoxSplit VERSION $VERSION " "$ROOT/plugin/CMakeLists.txt" \
  || die "plugin/CMakeLists.txt version differs from engine $VERSION"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$ROOT/macos/Info.plist")" == "$VERSION" ]] \
  || die "macos/Info.plist version differs from engine $VERSION"

# Built and staged outside the checkout (iCloud stamps Finder metadata on bundles there).
WORK="$HOME/Library/Caches/VoxSplit/pkg"
export VOXSPLIT_STAGE="$WORK/bundles"
export VOXSPLIT_SLIM=1 # bundles without engine/models: they use the shared install
rm -rf "$WORK"
mkdir -p "$VOXSPLIT_STAGE"
"$ROOT/scripts/build-plugin.sh" --stage-only
"$ROOT/scripts/build-app.sh" --no-dist >/dev/null

PAYLOAD="$WORK/root"
SUPPORT="$PAYLOAD/Library/Application Support/VoxSplit"
mkdir -p "$PAYLOAD/Applications" "$PAYLOAD/Library/Audio/Plug-Ins/VST3" \
  "$PAYLOAD/Library/Audio/Plug-Ins/Components" "$SUPPORT/models"
ditto --norsrc --noextattr "$VOXSPLIT_STAGE/VoxSplit.app" "$PAYLOAD/Applications/VoxSplit.app"
ditto --norsrc --noextattr "$VOXSPLIT_STAGE/VoxSplit.vst3" "$PAYLOAD/Library/Audio/Plug-Ins/VST3/VoxSplit.vst3"
ditto --norsrc --noextattr "$VOXSPLIT_STAGE/VoxSplit.component" "$PAYLOAD/Library/Audio/Plug-Ins/Components/VoxSplit.component"

cp -f "$ROOT/engine/target/release/voxsplit-engine" "$SUPPORT/voxsplit-engine"
IDENTITY="${VOXSPLIT_SIGN_IDENTITY:--}"
SIGN=(--force --options runtime --sign "$IDENTITY" --identifier com.voxsplit.engine)
[[ "$IDENTITY" != "-" ]] && SIGN+=(--timestamp)
xattr -c "$SUPPORT/voxsplit-engine"
codesign "${SIGN[@]}" "$SUPPORT/voxsplit-engine"
for m in Kim_Vocal_2 UVR_MDXNET_KARA_2 BS_Roformer_Karaoke; do
  cp -f "$ROOT/models/$m.onnx" "$ROOT/models/$m.json" "$SUPPORT/models/"
done
chmod -R u=rwX,go=rX "$PAYLOAD"
chmod 755 "$SUPPORT/voxsplit-engine"
for b in "$PAYLOAD/Applications/VoxSplit.app" "$PAYLOAD/Library/Audio/Plug-Ins/VST3/VoxSplit.vst3" \
  "$PAYLOAD/Library/Audio/Plug-Ins/Components/VoxSplit.component" "$SUPPORT/voxsplit-engine"; do
  codesign --verify --strict "$b" || die "signature broken: $b"
done

# Install exactly where the payload says. By default the installer "relocates" a bundle
# onto any copy with the same id elsewhere on disk (a dev build, a copy in Downloads).
COMPONENTS="$WORK/components.plist"
pkgbuild --analyze --root "$PAYLOAD" "$COMPONENTS" >/dev/null
count="$(/usr/libexec/PlistBuddy -c 'Print' "$COMPONENTS" | grep -c 'RootRelativeBundlePath')"
for ((i = 0; i < count; i++)); do
  /usr/libexec/PlistBuddy -c "Set :$i:BundleIsRelocatable false" "$COMPONENTS" 2>/dev/null \
    || /usr/libexec/PlistBuddy -c "Add :$i:BundleIsRelocatable bool false" "$COMPONENTS"
done

# --compression latest (LZMA for macOS 14+) saves ~11 MB over the default, which keeps
# the installer under the 300 MiB that `wrangler r2 object put` can upload.
pkgbuild --root "$PAYLOAD" --component-plist "$COMPONENTS" --scripts "$ROOT/installer/scripts" \
  --identifier com.voxsplit.pkg --version "$VERSION" --install-location / \
  --compression latest --min-os-version 14.0 \
  "$WORK/VoxSplit-component.pkg" >/dev/null
sed "s/__VERSION__/$VERSION/" "$ROOT/installer/distribution.xml" > "$WORK/distribution.xml"
OUT="$WORK/VoxSplit-$VERSION.pkg"
productbuild --distribution "$WORK/distribution.xml" --resources "$ROOT/installer/resources" \
  --package-path "$WORK" "$OUT" >/dev/null

mkdir -p "$ROOT/dist"
cp -f "$OUT" "$ROOT/dist/"
(cd "$ROOT/dist" && shasum -a 256 "VoxSplit-$VERSION.pkg" > "VoxSplit-$VERSION.pkg.sha256")
echo "$ROOT/dist/VoxSplit-$VERSION.pkg ($(du -h "$OUT" | cut -f1))"
