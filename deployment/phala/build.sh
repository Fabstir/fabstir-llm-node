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
# After pushing: paste the printed @sha256 into BOTH composes and run
#   cargo test --test phase5_compose_guard
# which is gate A-19 (red until the digest is real).
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

echo "==> extracting fabstir-llm-node from $TARBALL"
tar -xzf "$TARBALL" -C "$HERE" fabstir-llm-node
chmod 0755 fabstir-llm-node
echo "    binary sha256: $(sha256sum fabstir-llm-node | cut -d' ' -f1)"
# `strings | grep -m1` would SIGPIPE `strings` under pipefail and print "v… ?";
# let grep read to the end instead and take the first match with head.
VERSION_LINE="$( { strings -n 8 fabstir-llm-node | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+' | head -n1; } 2>/dev/null || true)"
echo "    version string: ${VERSION_LINE:-?}"

echo "==> docker build $REF (no cache: the binary changed)"
docker build --no-cache --pull -t "$REF" "$HERE"

echo "==> docker push $REF"
docker push "$REF"

DIGEST="$(docker inspect --format='{{index .RepoDigests 0}}' "$REF" | sed 's/.*@//')"
echo
echo "==> pushed. Paste this into deployment/phala/compose.gpu.yml AND compose.cpu.yml:"
echo "    image: ${REGISTRY}/${IMAGE}@${DIGEST}"
echo
echo "    then: cargo test --test phase5_compose_guard   (gate A-19)"

