# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# fabstir-llm-node image for Phala Cloud confidential VMs (Phase 5 attestation).
#
# WHY THIS SHAPE. On Phala the compose runs inside the CVM with no host checkout,
# so the binary is baked in (docker-compose.prod.yml bind-mounts it from
# ./target/release; that has nothing to point at in a CVM). The compose hash is
# measured into RTMR3 and the images are referenced by digest, so this image is
# what the key broker's policy ultimately pins.
#
# WHY python:3.11-slim AND NOT a CUDA base. The shipped node binary links cudart
# and cuBLAS statically and needs only libcuda.so.1 from the guest driver, which
# the NVIDIA container runtime injects on the GPU CVM. GPU evidence collection
# (nvtrust) needs Python + pynvml, and pynvml needs libnvidia-ml.so, also from
# the driver. A CUDA base would add ~2 GB for nothing.
#
# The CPU-CVM variant (gate column A) has no driver at all. /opt/cuda-stub holds
# the CUDA toolkit's link-time stub as libcuda.so.1; entrypoint.sh puts it on
# LD_LIBRARY_PATH ONLY under TEE_CPU_ONLY_STUB=1 and refuses if a GPU is
# present. Proven 2026-09-17: v8.54.0 starts, logs "CUDA driver is a stub
# library", loads a GGUF on CPU (gate A-1).
#
# Base digests resolved 2026-09-17 (multi-arch index digests):
FROM nvidia/cuda:12.4.1-devel-ubuntu22.04@sha256:da6791294b0b04d7e65d87b7451d6f2390b4d36225ab0701ee7dfec5769829f5 AS cuda-stub

FROM python:3.11-slim@sha256:9534e5a8e315485d4061ed659af0fd78a284c015f9b73661b41d6bab25604534

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1

# libgomp1 + libssl3: the only dynamic libraries the node binary needs besides
# libcuda.so.1 (readelf NEEDED on the v8.54.0 tarball binary, 2026-09-17).
RUN apt-get update && apt-get install -y --no-install-recommends \
        libgomp1 libssl3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# nvtrust local GPU verifier at the version of the reference commit
# (NVIDIA/nvtrust 07cd693..., local_gpu_verifier pyproject version 2.6.3). Used
# on the NODE only for evidence COLLECTION (collect_gpu_evidence_remote); the
# broker verifies via NRAS on the first run (D8). Pins nvidia-ml-py==12.550.52.
RUN pip install --no-cache-dir nv-local-gpu-verifier==2.6.3

# CUDA driver stub for the CPU-CVM variant (see header). Never on the GPU path.
COPY --from=cuda-stub /usr/local/cuda/lib64/stubs/libcuda.so /opt/cuda-stub/libcuda.so.1

# The broker's private root CA (deployment/phala/kbs-ca/README.md): the ONLY
# trust anchor the node accepts for kbs.fabstir.net, so it lives in the image
# and is therefore measured into compose_hash with everything else.
COPY kbs-root.pem /etc/fabstir/kbs-root.pem

# The node binary is extracted from a release tarball by build.sh into this
# context; it is never built here (the paid box pulls, never builds).
COPY fabstir-llm-node /usr/local/bin/fabstir-llm-node
COPY collect_gpu_evidence.py /usr/local/bin/collect_gpu_evidence.py
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod 0755 /usr/local/bin/fabstir-llm-node /usr/local/bin/entrypoint.sh \
    && chmod 0644 /usr/local/bin/collect_gpu_evidence.py

ENV TEE_GPU_EVIDENCE_SCRIPT=/usr/local/bin/collect_gpu_evidence.py \
    TEE_KBS_CA_FILE=/etc/fabstir/kbs-root.pem

WORKDIR /app
EXPOSE 8080 9000 9001 9002
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
