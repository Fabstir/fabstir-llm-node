#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phase 5 run 2 on kbs.fabstir.net (root): switch the SERVED policy + container between the
# BASE slot (what Phase A released under, recorded by vps-stage-video.sh) and the staged
# VIDEO policy (docs/development/PLAN-PHASE5-RUN2-VIDEO.md D8). Run BEFORE the matching
# Update Code.
#   ssh kbs 'bash -s -- video' < deployment/phala/run2/vps-switch.sh     (rollback: -- base)
set -euo pipefail
SLOT="${1:?usage: vps-switch.sh base|video}"
MODEL_ID=892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650
WORK=/srv/blobvol/work
PUB=/srv/blobvol/public
SERVE=/var/lib/fabstir-kbs/public
case "$SLOT" in
base)  POL="$WORK/policy-base.signed.json"; ENC=$(cat "$WORK/base-target" 2>/dev/null || true) ;;
video) POL="$WORK/policy-video.signed.json"; ENC="$PUB/video/qwen.enc" ;;
*) echo "usage: vps-switch.sh base|video" >&2; exit 2 ;;
esac
[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }
[ -f "$POL" ] && [ -n "$ENC" ] && [ -f "$ENC" ] || { echo "missing $POL or its container (stage first)" >&2; exit 1; }
install -m 0644 -o fabstir-kbs -g fabstir-kbs "$POL" "$SERVE/policies/$MODEL_ID.json.new"
mv -f "$SERVE/policies/$MODEL_ID.json.new" "$SERVE/policies/$MODEL_ID.json"
ln -sfn "$ENC" "$SERVE/blobs/qwen.enc"
echo "--- now served:"
curl -sk "https://kbs.fabstir.net/policies/$MODEL_ID.json" | python3 -c \
  'import json,sys; p=json.load(sys.stdin)["policy"]; print("policy version", p["policy_version"], "compose", p["cvm"]["compose_hash"][:12])'
echo "container: $(readlink "$SERVE/blobs/qwen.enc")"
curl -skI "https://kbs.fabstir.net/blobs/qwen.enc" | grep -iE '^(HTTP|content-length|last-modified)'
echo "SWITCHED to $SLOT"
