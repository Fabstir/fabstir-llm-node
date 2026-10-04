#!/usr/bin/env bash
# LTX 2.5 weights for the ltx-sidecar's 2.5 stack (NM1: Alpha Gen + Layout to Render), bf16 only.
#
# Files: the distilled transformer, the Gemma 4 text encoder, the video VAE, the audio VAE (Alpha Gen's
# spine), the latent x2 upscaler, and the two IC-LoRAs. Expected sizes and sha256 are fetched from the
# Hugging Face API BY THIS SCRIPT, ON THE HOST (never pasted from elsewhere: the dev container masks every
# 64-hex string in fetched content), and cross-checked against what PROVENANCE.txt recorded.
#
#   bash scripts/download_ltx25_models.sh            download what is missing (resumable), verify, record
#   bash scripts/download_ltx25_models.sh --verify   download NOTHING: check the files on disk; any BAD = exit 1
#
# The Hugging Face token is read hidden when HF_TOKEN is unset; it is never printed or stored. Every repo
# is gated: the token's account must have accepted the licences. The tree is the compose mount
# ${LTX25_MODELS_DIR:-./models/ltx25} (docker/ltx-extra_model_paths.yaml maps its subfolders).
set -euo pipefail
cd "$(dirname "$0")/.."
DEST="${LTX25_MODELS_DIR:-./models/ltx25}"
VERIFY=0
[ "${1:-}" = "--verify" ] && VERIFY=1

# repo|path in repo|subfolder
FILES=(
  "Lightricks/LTX-2.5|diffusion_models/ltx-2.5-22b-distilled-transformer-bf16.safetensors|diffusion_models"
  "Lightricks/LTX-2.5|text_encoders/gemma4-12b-with-proj-ltx-2.5-bf16.safetensors|text_encoders"
  "Lightricks/LTX-2.5|vae/ltx-2.5-video-vae-bf16.safetensors|vae"
  "Lightricks/LTX-2.5|vae/ltx-2.5-audio-vae-bf16.safetensors|vae"
  "Lightricks/LTX-2.5|latent_upscale_models/ltx-2.5-latent-spatial-upscaler-x2-bf16-1.0.safetensors|latent_upscale_models"
  "Lightricks/LTX-2.5-22b-IC-LoRA-Alpha-Gen|ltx-2.5-22b-ic-lora-alpha-gen-0.9.safetensors|loras"
  "Lightricks/LTX-2.5-22b-IC-LoRA-Layout-To-Render|ltx-2.5-22b-ic-lora-layout-to-render-1.0.safetensors|loras"
)

if [ -z "${HF_TOKEN:-}" ]; then read -r -s -p "Hugging Face token (hidden): " HF_TOKEN; echo; fi
[ -n "$HF_TOKEN" ] || { echo "no token"; exit 1; }

# Expected "<bytes> <sha256>" for one file, from the Hugging Face API (the LFS object's size and oid).
expected() {
  local repo=$1 path=$2
  curl -sf --retry 3 -H "Authorization: Bearer $HF_TOKEN" \
    --data-urlencode "paths=$path" "https://huggingface.co/api/models/$repo/paths-info/main" \
  | python3 -c '
import json, sys
info = json.load(sys.stdin)
lfs = (info[0] or {}).get("lfs") if info else None
if not lfs:
    sys.exit("no LFS record")
print(lfs["size"], lfs["oid"])
'
}

# What PROVENANCE.txt last recorded for this file's sha256 (empty if never recorded).
recorded_sha() {
  [ -f "$DEST/PROVENANCE.txt" ] || return 0
  awk -F'\t' -v r="$1" -v p="$2" '$2 == r && $3 == p { s = $5 } END { if (s != "") print s }' "$DEST/PROVENANCE.txt"
}

fail=0
for f in "${FILES[@]}"; do
  IFS='|' read -r repo path sub <<<"$f"
  name=$(basename "$path")
  out="$DEST/$sub/$name"
  if ! exp=$(expected "$repo" "$path"); then
    echo "BAD  $name: Hugging Face gave no size/sha256 (token, licence at https://huggingface.co/$repo, or network)"
    fail=1
    continue
  fi
  read -r bytes sha <<<"$exp"
  rec=$(recorded_sha "$repo" "$path")
  if [ -n "$rec" ] && [ "$rec" != "$sha" ]; then
    echo "BAD  $name: Hugging Face now lists a different sha256 than PROVENANCE.txt recorded — the file changed upstream"
    fail=1
    continue
  fi
  have=$(stat -c %s "$out" 2>/dev/null || echo 0)
  if [ "$have" != "$bytes" ]; then
    if [ "$VERIFY" = 1 ]; then
      echo "BAD  $name: on disk $have bytes, expected $bytes"
      fail=1
      continue
    fi
    mkdir -p "$DEST/$sub"
    echo "GET  $name"
    if ! curl -L --fail --retry 5 --retry-delay 10 -C - -H "Authorization: Bearer $HF_TOKEN" \
         -o "$out" "https://huggingface.co/$repo/resolve/main/$path"; then
      echo "BAD  $name: download failed (a 401/403 means the licence at https://huggingface.co/$repo is not accepted)"
      fail=1
      continue
    fi
  fi
  got=$(sha256sum "$out" | cut -d' ' -f1)
  if [ "$(stat -c %s "$out")" = "$bytes" ] && [ "$got" = "$sha" ]; then
    echo "ok   $name"
    if [ "$VERIFY" = 0 ] && [ -z "$rec" ]; then
      printf '%s\t%s\t%s\t%s\t%s\n' "$(date -Is)" "$repo" "$path" "$bytes" "$sha" >> "$DEST/PROVENANCE.txt"
    fi
  else
    echo "BAD  $name: size or sha256 does not match Hugging Face"
    fail=1
  fi
done

if [ "$fail" = 0 ]; then
  echo "ALL LTX 2.5 WEIGHTS PRESENT AND VERIFIED in $DEST"
else
  echo "some files are BAD — fix them before any window"
  exit 1
fi
