#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Build + push the Phala CVM image from a RELEASE TARBALL (never from a source
# build: the paid box pulls, never builds), then print the digest to paste into
# compose.gpu.yml and compose.cpu.yml. Run on a box with docker (not the dev
# container, which has none).
#
#   deployment/phala/build.sh fabstir-llm-node-v8.54.0-vault-session-guard.tar.gz
#   REGISTRY=ghcr.io/fabstir IMAGE=llm-node-phala deployment/phala/build.sh <tarball>
#
# After pushing: prove the image carries the current collector
#   docker run --rm --entrypoint grep <image@digest> -c 'DevTools (CC development) mode' \
#       /usr/local/bin/collect_gpu_evidence.py      # prints 1
# then paste the printed @sha256 into BOTH composes AND the PENDING row of
# tests/phase5_release_pins.rs, and run
#   cargo test --test phase5_compose_guard --test phase5_release_pins
# (gate A-19; the pins test is red until the digest is real, by design).
set -euo pipefail

TARBALL="$(readlink -f "${1:?usage: build.sh <release tarball> [tag]}")"
[ -f "$TARBALL" ] || { echo "no such tarball: $1" >&2; exit 66; }
TAG="${2:-$(basename "$TARBALL" .tar.gz | sed 's/^fabstir-llm-node-//')}"
REGISTRY="${REGISTRY:-ewr.vultrcr.com/fabstir}"
IMAGE="${IMAGE:-llm-node-phala}"
REF="${REGISTRY}/${IMAGE}:${TAG}"

HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"
# Never leave a 1.2 GB binary in the tracked context directory, success or
# failure: .dockerignore whitelists it, so a stale copy would be baked into the
# next manual build of this directory.
trap 'rm -f "$HERE/fabstir-llm-node"' EXIT

[ -f "$HERE/kbs-root.pem" ] || { echo "missing $HERE/kbs-root.pem (the broker's private root; see kbs-ca/README.md step 1)" >&2; exit 66; }
# The collector enters the image from THIS checkout, not the tarball (Dockerfile COPY).
# A checkout behind the P4.5 commit would bake a collector that does not refuse
# DevTools mode; refuse to build it. Belt-and-braces only: the effective check is the
# image grep after the push (deployment/phala/README.md), since a stale checkout has a
# stale copy of this script too.
grep -q 'DevTools (CC development) mode' "$HERE/collect_gpu_evidence.py" \
    || { echo "collect_gpu_evidence.py in this checkout lacks the DevTools refusal (pre-P4.5); pull first" >&2; exit 66; }

echo "==> extracting fabstir-llm-node from $TARBALL"
tar -xzf "$TARBALL" -C "$HERE" fabstir-llm-node
chmod 0755 fabstir-llm-node
echo "    binary sha256: $(sha256sum fabstir-llm-node | cut -d' ' -f1)"
# `strings | grep -m1` would SIGPIPE `strings` under pipefail and print "v… ?";
# let grep read to the end instead and take the first match with head.
# The version constant is not at a line start in the binary's string table
# (it sits mid-string), so match it anywhere and cut it out.
VERSION_LINE="$( { strings -n 8 fabstir-llm-node | grep -oE 'v[0-9]+\.[0-9]+\.[0-9]+-[A-Za-z0-9-]+-[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -n1; } 2>/dev/null || true)"
echo "    version string: ${VERSION_LINE:-?}"

echo "==> docker build $REF (no cache: the binary changed)"
docker build --no-cache --pull -t "$REF" "$HERE"

echo "==> docker push $REF"
docker push "$REF"

DIGEST="$(docker inspect --format='{{index .RepoDigests 0}}' "$REF" | sed 's/.*@//')"
echo
echo "==> pushed. First prove the image carries the current collector:"
echo "    docker run --rm --entrypoint grep ${REGISTRY}/${IMAGE}@${DIGEST} -c 'DevTools (CC development) mode' /usr/local/bin/collect_gpu_evidence.py   # prints 1"
echo "    then paste this into deployment/phala/compose.gpu.yml AND compose.cpu.yml:"
echo "    image: ${REGISTRY}/${IMAGE}@${DIGEST}"
echo "    and into the PENDING row of tests/phase5_release_pins.rs, then:"
echo "    cargo test --test phase5_compose_guard --test phase5_release_pins   (gate A-19)"

