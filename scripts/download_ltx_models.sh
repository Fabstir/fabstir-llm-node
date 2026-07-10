#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Download LTX 2.3 (22b) weights + HDR IC-LoRA into the ComfyUI models volume.
# Mirrors scripts/download_embedding_model.sh. chmod-intent: after checkout run
#   chmod +x scripts/download_ltx_models.sh   # make executable
set -euo pipefail

MODELS_DIR="${MODELS_DIR:-./models/ltx}"
CKPT_DIR="${MODELS_DIR}/checkpoints"
LORA_DIR="${MODELS_DIR}/loras"

# HuggingFace repos (resolve/main).
# TODO(Jules): confirm exact filenames published under these repos.
LTX_REPO="https://huggingface.co/Lightricks/LTX-2.3/resolve/main"
HDR_REPO="https://huggingface.co/Lightricks/LTX-2.3-22b-IC-LoRA-HDR/resolve/main"
LTX_FILE="ltx-2.3-22b.safetensors"               # TODO(Jules): confirm exact filename
HDR_FILE="ltx-2.3-22b-ic-lora-hdr.safetensors"   # TODO(Jules): confirm exact filename
mkdir -p "${CKPT_DIR}" "${LORA_DIR}"

# Download with skip-if-present + minimum-size sanity check.
download() {
    local url="$1" out="$2" min="$3" desc="$4"
    if [ -f "${out}" ] && [ "$(stat -c%s "${out}" 2>/dev/null || echo 0)" -ge "${min}" ]; then
        echo "✓ ${desc} already present — skipping"; return 0
    fi
    echo "Downloading ${desc}..."
    if command -v wget &>/dev/null; then
        wget -q --show-progress "${url}" -O "${out}"
    else
        curl -L --progress-bar "${url}" -o "${out}"
    fi
    [ -s "${out}" ] || { echo "Error: failed to download ${desc}" >&2; exit 1; }
    echo "✓ ${desc} downloaded"
}

# 22b base weights (~tens of GB) + HDR IC-LoRA (~hundreds of MB).
download "${LTX_REPO}/${LTX_FILE}" "${CKPT_DIR}/${LTX_FILE}" 1000000000 "LTX 2.3 22b checkpoint"
download "${HDR_REPO}/${HDR_FILE}" "${LORA_DIR}/${HDR_FILE}" 10000000 "LTX 2.3 HDR IC-LoRA"
echo "LTX models ready under ${MODELS_DIR}/"
