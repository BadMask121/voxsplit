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

# One payload folder per optional part, plus the shared core.
ROOTS="$WORK/roots"
SUPPORT="$ROOTS/core/Library/Application Support/VoxSplit"
mkdir -p "$ROOTS/app/Applications" "$ROOTS/plugin/Library/Audio/Plug-Ins/VST3" \
  "$ROOTS/plugin/Library/Audio/Plug-Ins/Components" "$SUPPORT/models"
ditto --norsrc --noextattr "$VOXSPLIT_STAGE/VoxSplit.app" "$ROOTS/app/Applications/VoxSplit.app"
ditto --norsrc --noextattr "$VOXSPLIT_STAGE/VoxSplit.vst3" "$ROOTS/plugin/Library/Audio/Plug-Ins/VST3/VoxSplit.vst3"
ditto --norsrc --noextattr "$VOXSPLIT_STAGE/VoxSplit.component" "$ROOTS/plugin/Library/Audio/Plug-Ins/Components/VoxSplit.component"

cp -f "$ROOT/engine/target/release/voxsplit-engine" "$SUPPORT/voxsplit-engine"
IDENTITY="${VOXSPLIT_SIGN_IDENTITY:--}"
SIGN=(--force --options runtime --sign "$IDENTITY" --identifier com.voxsplit.engine)
[[ "$IDENTITY" != "-" ]] && SIGN+=(--timestamp)
xattr -c "$SUPPORT/voxsplit-engine"
codesign "${SIGN[@]}" "$SUPPORT/voxsplit-engine"
for m in Kim_Vocal_2 UVR_MDXNET_KARA_2 BS_Roformer_Karaoke; do
  cp -f "$ROOT/models/$m.onnx" "$ROOT/models/$m.json" "$SUPPORT/models/"
done
chmod -R u=rwX,go=rX "$ROOTS"
chmod 755 "$SUPPORT/voxsplit-engine"
for b in "$ROOTS/app/Applications/VoxSplit.app" "$ROOTS/plugin/Library/Audio/Plug-Ins/VST3/VoxSplit.vst3" \
  "$ROOTS/plugin/Library/Audio/Plug-Ins/Components/VoxSplit.component" "$SUPPORT/voxsplit-engine"; do
  codesign --verify --strict "$b" || die "signature broken: $b"
done

PKGS="$WORK/pkgs"
mkdir -p "$PKGS"
# --compression latest (LZMA for macOS 14+) keeps the installer under the 300 MiB
# that `wrangler r2 object put` can upload.
COMMON=(--version "$VERSION" --install-location / --compression latest --min-os-version 14.0)

# Install bundles exactly where the payload says, and always replace what's there. By
# default the installer "relocates" a bundle onto any copy with the same id elsewhere on
# disk (a dev build, a Downloads copy), and skips a bundle whose installed version is newer.
component_pkg() { # <part> [extra pkgbuild args...]
  local part="$1"; shift
  local plist="$WORK/$part-components.plist"
  pkgbuild --analyze --root "$ROOTS/$part" "$plist" >/dev/null
  local count
  count="$(/usr/libexec/PlistBuddy -c 'Print' "$plist" | grep -c 'RootRelativeBundlePath' || true)"
  for ((i = 0; i < count; i++)); do
    for key in BundleIsRelocatable BundleIsVersionChecked; do
      /usr/libexec/PlistBuddy -c "Set :$i:$key false" "$plist" 2>/dev/null \
        || /usr/libexec/PlistBuddy -c "Add :$i:$key bool false" "$plist"
    done
  done
  pkgbuild --root "$ROOTS/$part" --component-plist "$plist" --identifier "com.voxsplit.pkg.$part" \
    "${COMMON[@]}" "$@" "$PKGS/VoxSplit-$part.pkg" >/dev/null
}
pkgbuild --root "$ROOTS/core" --identifier com.voxsplit.pkg.core "${COMMON[@]}" "$PKGS/VoxSplit-core.pkg" >/dev/null
component_pkg app
component_pkg plugin --scripts "$ROOT/installer/scripts/plugin"
# The command is only a link, made by its postinstall: no payload.
pkgbuild --nopayload --scripts "$ROOT/installer/scripts/cli" --identifier com.voxsplit.pkg.cli \
  --version "$VERSION" "$PKGS/VoxSplit-cli.pkg" >/dev/null

sed "s/__VERSION__/$VERSION/g" "$ROOT/installer/distribution.xml" > "$WORK/distribution.xml"
OUT="$WORK/VoxSplit-$VERSION.pkg"
productbuild --distribution "$WORK/distribution.xml" --resources "$ROOT/installer/resources" \
  --package-path "$PKGS" "$OUT" >/dev/null

# site/public/install.template.sh (--cli) copies the engine and models out of this exact
# path; fail here rather than in someone's Terminal.
CHECK="$WORK/check"
pkgutil --expand-full "$OUT" "$CHECK" >/dev/null
[[ -x "$CHECK/VoxSplit-core.pkg/Payload/Library/Application Support/VoxSplit/voxsplit-engine" ]] \
  || die "the engine isn't where install.sh --cli looks for it"
rm -rf "$CHECK"

mkdir -p "$ROOT/dist"
cp -f "$OUT" "$ROOT/dist/"
(cd "$ROOT/dist" && shasum -a 256 "VoxSplit-$VERSION.pkg" > "VoxSplit-$VERSION.pkg.sha256")
echo "$ROOT/dist/VoxSplit-$VERSION.pkg ($(du -h "$OUT" | cut -f1))"
