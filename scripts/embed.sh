#!/usr/bin/env bash
# embed.sh <bundle>: copy the engine and models into a .vst3/.component/.app
# using the layout in docs/01-architecture.md, then sign it inside-out.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE="${1:?usage: embed.sh <bundle>}"
ENGINE="$ROOT/engine/target/release/voxsplit-engine"

[[ -x "$ENGINE" ]] || { echo "error: build the engine first (scripts/build-engine.sh)" >&2; exit 1; }
MODELS=(Kim_Vocal_2 UVR_MDXNET_KARA_2 BS_Roformer_Karaoke)
for m in "${MODELS[@]}"; do
  [[ -f "$ROOT/models/$m.onnx" ]] || { echo "error: models missing; run scripts/fetch-models.sh" >&2; exit 1; }
done

mkdir -p "$BUNDLE/Contents/Helpers" "$BUNDLE/Contents/Resources/models"
cp -f "$ENGINE" "$BUNDLE/Contents/Helpers/voxsplit-engine"
for m in "${MODELS[@]}"; do
  cp -f "$ROOT/models/$m.onnx" "$ROOT/models/$m.json" "$BUNDLE/Contents/Resources/models/"
done

"$ROOT/scripts/sign.sh" "$BUNDLE"
