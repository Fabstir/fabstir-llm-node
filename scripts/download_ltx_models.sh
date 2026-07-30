#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Download the LTX 2.3 weight set for the ComfyUI sidecar (allowlist v16 modes).
#
# The inventory below mirrors the PROVEN host install (3XS-Z, F:\AI\ltx-weights,
# enumerated 2026-07-23): 15 files, ~85.5 GB, in ComfyUI category subdirs. Sizes
# are exact bytes from that host — a size mismatch means a wrong/partial file.
# The Foley LoRA is deliberately EXCLUDED (licence-parked; not in allowlist v16).
#
# Layout produced (mounted at /opt/ComfyUI/models by docker-compose.prod.yml):
#   $MODELS_DIR/checkpoints/            two 22B fp8 checkpoints
#   $MODELS_DIR/loras/                  distilled + IC-/mode-LoRAs
#   $MODELS_DIR/depthanything/          CV1 depth (vits/Apache ONLY — Base/Large are CC-BY-NC)
#   $MODELS_DIR/text_encoders/          gemma fp4 encoder
#   $MODELS_DIR/latent_upscale_models/  spatial upscaler x2
#   $MODELS_DIR/geometry_estimation/    MoGe (guided modes)
#
# GATED repos (accept once on huggingface.co while logged in, then pass a token):
#   Lightricks/LTX-2.3-22b-IC-LoRA-{Water-Simulation,Day-To-Night,Ingredients}
#   → HF_TOKEN=hf_xxx MODELS_DIR=./models/ltx ./scripts/download_ltx_models.sh
# Two files have NO public HF source (ComfyUI-converted artifacts) and are marked
# MANUAL below — scp them from the proven host's F:\AI\ltx-weights instead.
#
# Usage:  HF_TOKEN=hf_xxx MODELS_DIR=./models/ltx ./scripts/download_ltx_models.sh
# Resume-safe: wget -c; re-run until it reports all files OK.
set -euo pipefail

MODELS_DIR="${MODELS_DIR:-./models/ltx}"
AUTH_ARGS=()
[ -n "${HF_TOKEN:-}" ] && AUTH_ARGS=(--header "Authorization: Bearer ${HF_TOKEN}")

# manifest: subdir|filename|exact_bytes|url[|fallback_url ...]
MANIFEST=$(cat <<'EOF'
checkpoints|ltx-2.3-22b-dev-fp8.safetensors|29145431166|https://huggingface.co/Lightricks/LTX-2.3-fp8/resolve/main/ltx-2.3-22b-dev-fp8.safetensors
checkpoints|ltx-2.3-22b-distilled-fp8.safetensors|29531884062|https://huggingface.co/Lightricks/LTX-2.3-fp8/resolve/main/ltx-2.3-22b-distilled-fp8.safetensors
latent_upscale_models|ltx-2.3-spatial-upscaler-x2-1.1.safetensors|995743560|https://huggingface.co/Lightricks/LTX-2.3/resolve/main/ltx-2.3-spatial-upscaler-x2-1.1.safetensors
loras|ltx-2.3-22b-distilled-lora-384.safetensors|7605507256|https://huggingface.co/Lightricks/LTX-2.3/resolve/main/ltx-2.3-22b-distilled-lora-384.safetensors
text_encoders|gemma_3_12B_it_fp4_mixed.safetensors|9447702218|https://huggingface.co/Comfy-Org/ltx-2/resolve/main/split_files/text_encoders/gemma_3_12B_it_fp4_mixed.safetensors
loras|gemma-3-12b-it-abliterated_lora_rank64_bf16.safetensors|628203616|https://huggingface.co/Comfy-Org/ltx-2/resolve/main/split_files/loras/gemma-3-12b-it-abliterated_lora_rank64_bf16.safetensors
loras|ltx_2.3_22b_distilled_1.1_lora_dynamic_fro09_avg_rank_111_bf16.safetensors|2741024390|https://huggingface.co/Comfy-Org/ltx-2.3/resolve/main/split_files/loras/ltx_2.3_22b_distilled_1.1_lora_dynamic_fro09_avg_rank_111_bf16.safetensors
loras|ltx-2.3-22b-ic-lora-union-control-ref0.5.safetensors|654465352|https://huggingface.co/Lightricks/LTX-2.3-22b-IC-LoRA-Union-Control/resolve/main/ltx-2.3-22b-ic-lora-union-control-ref0.5.safetensors
loras|ltx-2.3-22b-ic-lora-water-simulation-0.9.safetensors|906071437|https://huggingface.co/Lightricks/LTX-2.3-22b-IC-LoRA-Water-Simulation/resolve/main/ltx-2.3-22b-ic-lora-water-simulation-0.9.safetensors
loras|ltx-2.3-22b-ic-lora-day-to-night-0.9.safetensors|327309305|https://huggingface.co/Lightricks/LTX-2.3-22b-IC-LoRA-Day-To-Night/resolve/main/ltx-2.3-22b-ic-lora-day-to-night-0.9.safetensors
loras|ltx-2.3-22b-ic-lora-ingredients-0.9.safetensors|1308778338|https://huggingface.co/Lightricks/LTX-2.3-22b-IC-LoRA-Ingredients/resolve/main/ltx-2.3-22b-ic-lora-ingredients-0.9.safetensors
loras|LTX2.3-22B_IC-LoRA-CrossView-Warp_v0.9_18000.safetensors|201431592|https://huggingface.co/Cseti/LTX2.3-22B_IC-LoRA-CrossView-Warp/resolve/main/LTX2.3-22B_IC-LoRA-CrossView-Warp_v0.9_18000.safetensors
depthanything|depth_anything_v2_vits_fp16.safetensors|49595202|https://huggingface.co/Kijai/DepthAnythingV2-safetensors/resolve/5aa7ab578df757d94c743998b157a0204ff29215/depth_anything_v2_vits_fp16.safetensors
loras|ltx-2.3-22b-ic-lora-outpaint.safetensors|1308756416|https://huggingface.co/oumoumad/LTX-2.3-22b-IC-LoRA-Outpaint/resolve/main/ltx-2.3-22b-ic-lora-outpaint.safetensors|https://huggingface.co/DeepBeepMeep/LTX-2/resolve/main/ltx-2.3-22b-ic-lora-outpaint.safetensors
loras|ltx23_edit_anything_global_rank128_v1_9000steps_adamw.safetensors|1308756416|https://huggingface.co/Alissonerdx/LTX-LoRAs/resolve/main/ltx23_edit_anything_global_rank128_v1_9000steps_adamw.safetensors
geometry_estimation|moge_2_vitl_normal_fp16.safetensors|661859924|MANUAL
loras|ltx2.3-video-restoration-general.safetensors|100767488|MANUAL
EOF
)

ok=0; failed=0; skipped_list=""

fetch_one() {
    local subdir="$1" file="$2" bytes="$3"; shift 3
    local out="${MODELS_DIR}/${subdir}/${file}"
    mkdir -p "${MODELS_DIR}/${subdir}"

    local have
    have=$(stat -c%s "${out}" 2>/dev/null || echo 0)
    if [ "${have}" = "${bytes}" ]; then
        echo "OK      ${subdir}/${file} (already present, ${bytes} bytes)"
        ok=$((ok+1)); return 0
    fi

    local url
    for url in "$@"; do
        if [ "${url}" = "MANUAL" ]; then
            echo "MANUAL  ${subdir}/${file} — no public source; scp from the proven host (F:\\AI\\ltx-weights\\${subdir}\\${file})"
            failed=$((failed+1)); skipped_list="${skipped_list}\n  ${subdir}/${file} (manual transfer)"
            return 0
        fi
        # Probe with a 1-byte ranged GET so a wrong/gated repo fails fast, not after GB.
        if ! curl -sfL "${AUTH_ARGS[@]}" -r 0-0 -o /dev/null "${url}"; then
            echo "  probe miss (wrong path or gated without HF_TOKEN): ${url}"
            continue
        fi
        echo "FETCH   ${subdir}/${file} <- ${url}"
        wget -c -q --show-progress "${AUTH_ARGS[@]}" "${url}" -O "${out}" || { echo "  download error, trying next source"; continue; }
        have=$(stat -c%s "${out}" 2>/dev/null || echo 0)
        if [ "${have}" = "${bytes}" ]; then
            echo "OK      ${subdir}/${file} (${bytes} bytes, size verified)"
            ok=$((ok+1)); return 0
        fi
        echo "  SIZE MISMATCH: got ${have}, want ${bytes} — wrong file at this source; removing"
        rm -f "${out}"
    done

    echo "FAILED  ${subdir}/${file} — no source delivered the exact file"
    failed=$((failed+1)); skipped_list="${skipped_list}\n  ${subdir}/${file}"
    return 0
}

while IFS='|' read -r subdir file bytes urls; do
    [ -z "${subdir}" ] && continue
    IFS='|' read -r -a urlarr <<< "${urls}"
    fetch_one "${subdir}" "${file}" "${bytes}" "${urlarr[@]}"
done <<< "${MANIFEST}"

echo
echo "==== ${ok} OK, ${failed} failed ===="
if [ "${failed}" -gt 0 ]; then
    echo -e "Files needing manual transfer (scp from the proven host's F:\\AI\\ltx-weights):${skipped_list}"
    exit 1
fi
echo "All weights present and size-verified under ${MODELS_DIR}/"
