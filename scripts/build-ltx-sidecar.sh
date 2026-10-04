#!/usr/bin/env bash
# Build the LTX ComfyUI sidecar image OUTSIDE compose (NM1 D1), context-free: the Dockerfile copies only
# between its own stages, so it is read from stdin and no build context is sent. The three build args
# are the LTX 2.5 stack the probes ran (ComfyUI core v0.38.0, ComfyUI-LTXVideo bf2ca026); the
# Dockerfile's own defaults stay at today's pins, because the Phala run-2 recipe builds FROM it.
#
#   bash scripts/build-ltx-sidecar.sh <tag>        e.g.  bash scripts/build-ltx-sidecar.sh nm1
#
# Then record the image ID:  docker image inspect ltx-sidecar:<tag> --format '{{.Id}}'
set -euo pipefail
TAG="${1:?usage: build-ltx-sidecar.sh <tag>}"
cd "$(dirname "$0")/.."
docker build \
  --build-arg COMFYUI_REPO=https://github.com/Comfy-Org/ComfyUI.git \
  --build-arg COMFYUI_COMMIT=6b747c0428c343e1417219641db93a4fb7cb69ae \
  --build-arg LTXVIDEO_COMMIT=bf2ca0264f706db64cb8931155695ca481fc9d91 \
  -t "ltx-sidecar:${TAG}" - < docker/Dockerfile.ltx-sidecar
docker image inspect "ltx-sidecar:${TAG}" --format 'built ltx-sidecar:'"${TAG}"' {{.Id}}'
