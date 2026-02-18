#!/bin/bash
# Entrypoint for SGLang diffusion sidecar
# Auto-detects WSL2 and patches NCCL -> gloo backend if needed
set -e

if grep -qi microsoft /proc/version 2>/dev/null; then
    echo "[entrypoint-diffusion] WSL2 detected - patching NCCL backend to gloo..."
    export NCCL_P2P_DISABLE=1
    export NCCL_IB_DISABLE=1
    export NCCL_SHM_DISABLE=1
    sed -i 's/backend=backend/backend="gloo"/' \
        /sgl-workspace/sglang/python/sglang/multimodal_gen/runtime/distributed/parallel_state.py 2>/dev/null || true
else
    echo "[entrypoint-diffusion] Native Linux detected - using NCCL backend"
fi

exec sglang serve "$@"
