#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phala CVM entrypoint. Two jobs: put the CUDA driver STUB on the library path
# for the CPU-CVM variant (and only then), and make the GPU-half test mode
# impossible to miss in the logs. Everything else is the node's own env.
set -euo pipefail

if [ "${TEE_CPU_ONLY_STUB:-0}" = "1" ]; then
    # Refuse to shadow a real driver: a GPU present with the stub requested is
    # a mis-deployed compose, not a CPU CVM.
    if [ -e /dev/nvidiactl ] || command -v nvidia-smi >/dev/null 2>&1; then
        echo "entrypoint: TEE_CPU_ONLY_STUB=1 but a GPU/driver is present; refusing to start" >&2
        exit 78
    fi
    export LD_LIBRARY_PATH="/opt/cuda-stub${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    echo "entrypoint: CPU-only CVM; CUDA driver STUB on LD_LIBRARY_PATH (no GPU inference possible)"
fi

case "${TEE_GPU_EVIDENCE:-real}" in
    real)
        if [ ! -e /dev/nvidiactl ]; then
            echo "entrypoint: WARNING no /dev/nvidiactl; real GPU evidence collection will fail closed at attestation" >&2
        fi
        ;;
    canned)
        echo "entrypoint: CRITICAL: TEE_GPU_EVIDENCE=canned; GPU evidence is nvtrust's canned sample with a FIXED nonce. Test keyring only. Never on the GPU CVM." >&2
        ;;
    *)
        echo "entrypoint: TEE_GPU_EVIDENCE must be 'real' (default) or 'canned', got '${TEE_GPU_EVIDENCE}'" >&2
        exit 78
        ;;
esac

exec /usr/local/bin/fabstir-llm-node "$@"
