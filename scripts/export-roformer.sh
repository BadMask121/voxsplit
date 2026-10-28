#!/usr/bin/env bash
# Rebuild models/BS_Roformer_Karaoke.onnx from the pinned PyTorch checkpoint.
# Developer-only: needs uv (https://docs.astral.sh/uv/). End users get the ONNX inside the bundles.
# The checkpoint and the Python env live in ~/Library/Caches/VoxSplit, outside the repo.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SIDECAR="$ROOT/models/BS_Roformer_Karaoke.json"
CACHE="$HOME/Library/Caches/VoxSplit"
WEIGHTS="$CACHE/roformer-weights"
export UV_PROJECT_ENVIRONMENT="$CACHE/roformer-venv"

field() { /usr/bin/plutil -extract "$1" raw -o - "$SIDECAR"; }
CKPT_URL="$(field export.checkpoint_url)"
CKPT_SHA="$(field export.checkpoint_sha256)"
CONFIG_URL="$(field export.config_url)"
SECONDS_PER_CHUNK="$(field export.chunk_seconds)"

command -v uv >/dev/null || { echo "error: uv is required (brew install uv)" >&2; exit 1; }
mkdir -p "$WEIGHTS"
CKPT="$WEIGHTS/$(basename "$CKPT_URL")"
CONFIG="$WEIGHTS/$(basename "$CONFIG_URL")"

if [[ ! -f "$CKPT" ]] || [[ "$(shasum -a 256 "$CKPT" | cut -d' ' -f1)" != "$CKPT_SHA" ]]; then
  echo "fetching  $(basename "$CKPT")"
  curl -fL --progress-bar -o "$CKPT.partial" "$CKPT_URL"
  got="$(shasum -a 256 "$CKPT.partial" | cut -d' ' -f1)"
  [[ "$got" == "$CKPT_SHA" ]] || { rm -f "$CKPT.partial"; echo "error: checkpoint sha256 mismatch ($got)" >&2; exit 1; }
  mv -f "$CKPT.partial" "$CKPT"
fi
[[ -f "$CONFIG" ]] || curl -fsSL -o "$CONFIG" "$CONFIG_URL"

# -I: don't let anything next to the downloads shadow imports.
uv run --project "$ROOT/tools/roformer" --python 3.12 python -I "$ROOT/tools/roformer/export.py" \
  --config "$CONFIG" --checkpoint "$CKPT" --chunk-seconds "$SECONDS_PER_CHUNK" \
  --out "$ROOT/models/BS_Roformer_Karaoke.onnx"
