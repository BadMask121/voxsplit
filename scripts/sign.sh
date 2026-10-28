#!/usr/bin/env bash
# sign.sh <bundle>: sign the nested engine first, then the bundle, with the
# hardened runtime. Ad-hoc ("-") by default, which runs on this machine only.
# For distribution: VOXSPLIT_SIGN_IDENTITY="Developer ID Application: …" then notarize.
set -euo pipefail

BUNDLE="${1:?usage: sign.sh <bundle>}"
IDENTITY="${VOXSPLIT_SIGN_IDENTITY:--}"
OPTS=(--force --options runtime --sign "$IDENTITY")
[[ "$IDENTITY" != "-" ]] && OPTS+=(--timestamp)

# Extended attributes (provenance, Finder info) on copied files break the seal.
xattr -cr "$BUNDLE"

if [[ -f "$BUNDLE/Contents/Helpers/voxsplit-engine" ]]; then
  codesign "${OPTS[@]}" --identifier com.voxsplit.engine "$BUNDLE/Contents/Helpers/voxsplit-engine"
fi
codesign "${OPTS[@]}" "$BUNDLE"
codesign --verify --deep --strict "$BUNDLE"
echo "signed    $(basename "$BUNDLE") ($IDENTITY)"
