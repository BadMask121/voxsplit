#!/usr/bin/env bash
# Make sure every model listed in models/*.json is present.
#  - Sidecars with a "url" are downloaded and SHA-256 verified.
#  - Sidecars with an "export" block are built locally (scripts/export-roformer.sh),
#    because no ready-made ONNX exists for them.
# Developer-time only: end users get the weights inside the built bundles.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODELS="$ROOT/models"

json_field() { /usr/bin/plutil -extract "$2" raw -o - "$1" 2>/dev/null; }

for sidecar in "$MODELS"/*.json; do
  file="$(json_field "$sidecar" file)"
  dest="$MODELS/$file"

  if script="$(json_field "$sidecar" export.script)"; then
    if [[ -f "$dest" ]]; then
      echo "ok        $file (exported)"
    else
      echo "exporting $file via $script"
      "$ROOT/$script"
    fi
    continue
  fi

  url="$(json_field "$sidecar" url)"
  want="$(json_field "$sidecar" sha256)"
  if [[ -f "$dest" ]] && [[ "$(shasum -a 256 "$dest" | cut -d' ' -f1)" == "$want" ]]; then
    echo "ok        $file"
    continue
  fi

  echo "fetching  $file"
  curl -fL --progress-bar -o "$dest.partial" "$url"
  got="$(shasum -a 256 "$dest.partial" | cut -d' ' -f1)"
  if [[ "$got" != "$want" ]]; then
    rm -f "$dest.partial"
    echo "error: $file sha256 mismatch (got $got, want $want)" >&2
    exit 1
  fi
  mv -f "$dest.partial" "$dest"
  echo "ok        $file"
done
