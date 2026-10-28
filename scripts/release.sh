#!/usr/bin/env bash
# Publish a release to https://voxsplit.jeffrey.build: build the .pkg, record it in
# site/release.json, upload it to R2, deploy the site, then check the live download.
# Needs `wrangler login` (Cloudflare account that owns triplejmusic.art).
#   --skip-build   publish the existing dist/VoxSplit-<version>.pkg
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SITE="$ROOT/site"
WRANGLER="$SITE/node_modules/.bin/wrangler"
BUCKET=voxsplit-downloads
ORIGIN=https://voxsplit.jeffrey.build
die() { echo "error: $*" >&2; exit 1; }

[[ -x "$WRANGLER" ]] || (cd "$SITE" && npm install --silent)
[[ "${1:-}" == "--skip-build" ]] || "$ROOT/scripts/build-pkg.sh"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/engine/Cargo.toml" | head -1)"
FILE="VoxSplit-$VERSION.pkg"
PKG="$ROOT/dist/$FILE"
[[ -f "$PKG" ]] || die "$PKG is missing; run without --skip-build"
BYTES="$(stat -f %z "$PKG")"
SHA256="$(shasum -a 256 "$PKG" | cut -d' ' -f1)"
# wrangler uploads one object of at most 300 MiB; beyond that it needs a multipart upload
# (R2's S3 API with an API token) instead of `wrangler r2 object put`.
((BYTES <= 300 * 1024 * 1024)) || die "$FILE is $BYTES bytes, over wrangler's 300 MiB upload limit"

cat > "$SITE/release.json" <<JSON
{
  "version": "$VERSION",
  "file": "$FILE",
  "sha256": "$SHA256",
  "bytes": $BYTES,
  "date": "$(date +%Y-%m-%d)"
}
JSON

cd "$SITE"
npm test --silent
"$WRANGLER" r2 bucket info "$BUCKET" >/dev/null 2>&1 || "$WRANGLER" r2 bucket create "$BUCKET"
"$WRANGLER" r2 object put "$BUCKET/$FILE" --file "$PKG" --remote \
  --content-type application/octet-stream --cache-control "public, max-age=31536000, immutable"
"$WRANGLER" deploy

# The live site must serve exactly this file.
live_bytes="$(curl -fsSI "$ORIGIN/download/$FILE" | tr -d '\r' | awk -F': ' 'tolower($1)=="content-length"{print $2}')"
[[ "$live_bytes" == "$BYTES" ]] || die "live download is $live_bytes bytes, expected $BYTES"
curl -fsS "$ORIGIN/install.sh" | grep -q "local sha256=\"$SHA256\"" || die "live install.sh has the wrong checksum"
echo "Published VoxSplit $VERSION: $ORIGIN (sha256 $SHA256)"
