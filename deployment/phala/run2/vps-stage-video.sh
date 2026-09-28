#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phase 5 run 2, on kbs.fabstir.net (root): STAGE the Phase B (video) policy without changing
# anything served (docs/development/PLAN-PHASE5-RUN2-VIDEO.md D8). The broker reads the SAME
# policy file nginx serves, so the video policy cannot live at another URL: vps-switch.sh
# swaps it in on the day. This script
#   (a) records what is served NOW (policy file + the blobs/qwen.enc target) as the BASE slot,
#       so `vps-switch.sh base` always rolls back to what Phase A actually released under;
#   (b) RESEALS the served container under the video policy (~90 s; no GGUF needed);
#   (c) keeps the signed video policy in work/.
# Happy path (Tuesday night, v4 served):
#   scp docs/archive/phase5-artefacts/run2/policy-run2-v5.signed.json kbs:/tmp/policy-video.signed.json
#   ssh kbs 'bash -s -- 5' < deployment/phala/run2/vps-stage-video.sh
# After a Phase A day re-pin: rm -rf /srv/blobvol/public/video FIRST, then re-stage a video
# policy built with the day's registers (and, if Phase A was refused on compose_hash, the video
# hash RE-PREDICTED from the day's attestation JSON, passed as the 2nd argument) under the next
# unused version:  ssh kbs 'bash -s -- <N> <video compose hash>' < …/vps-stage-video.sh
set -euo pipefail

VERSION="${1:?usage: vps-stage-video.sh <video policy version> [<video compose hash>]}"
MODEL_ID=892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650
PROVIDER=0x3d66986d29160af27409b4bc847b567e3f665b1c
VIDEO_COMPOSE_HASH="${2:-a5a8f8c03419dc52bb99a9526f37ddd9ef029007742a1ccc483a807ed75d9c8b}"
IN=/tmp/policy-video.signed.json
KBS=/usr/local/bin/fabstir-kbs
WORK=/srv/blobvol/work
PUB=/srv/blobvol/public
SERVE=/var/lib/fabstir-kbs/public

[[ "$VIDEO_COMPOSE_HASH" =~ ^[0-9a-f]{64}$ ]] || { echo "VIDEO_COMPOSE_HASH not filled in" >&2; exit 1; }
[[ "$VERSION" =~ ^[0-9]+$ ]] || { echo "version must be a number" >&2; exit 1; }
[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }
mountpoint -q /srv/blobvol || { echo "/srv/blobvol is not mounted" >&2; exit 1; }
[ -f "$IN" ] || { echo "no $IN: scp the signed video policy first" >&2; exit 1; }
[ -f "$WORK/qwen.dek" ] || { echo "no $WORK/qwen.dek" >&2; exit 1; }
[ ! -e "$PUB/video/qwen.enc" ] || { echo "a video container is already staged: rm -rf $PUB/video first" >&2; exit 1; }

python3 - "$IN" "$VERSION" "$VIDEO_COMPOSE_HASH" "$PROVIDER" "$MODEL_ID" <<'PY'
import json, sys
s = json.load(open(sys.argv[1])); p = s["policy"]
mid = bytes(p["model_id"]).hex() if isinstance(p["model_id"], list) else str(p["model_id"]).removeprefix("0x")
got = (p["policy_version"], p["cvm"]["compose_hash"], s["signer"].lower(), s["encrypted_ref"], mid)
want = (int(sys.argv[2]), sys.argv[3], sys.argv[4], "qwen.enc", sys.argv[5])
if got != want:
    raise SystemExit(f"video policy {got} != expected {want}")
print(f"video policy OK: v{got[0]} compose {got[1][:12]}")
PY

# (a) The BASE slot = exactly what is served now.
BASE_TARGET=$(readlink -f "$SERVE/blobs/qwen.enc")
[ -f "$BASE_TARGET" ] || { echo "nothing served at $SERVE/blobs/qwen.enc (seal the base first)" >&2; exit 1; }
cp -p "$SERVE/policies/$MODEL_ID.json" "$WORK/policy-base.signed.json"
printf '%s\n' "$BASE_TARGET" > "$WORK/base-target"
python3 -c 'import json,sys; p=json.load(open(sys.argv[1]))["policy"]; print("base slot: served v%s compose %s" % (p["policy_version"], p["cvm"]["compose_hash"][:12]))' "$WORK/policy-base.signed.json"
echo "base container: $BASE_TARGET"

# (b) + (c)
install -m 0644 "$IN" "$WORK/policy-video.signed.json"
echo "--- space (the reseal writes ~29 GB):"; df -h /srv/blobvol | tail -1
install -d -m 0755 "$PUB/video"
"$KBS" reseal --in "$BASE_TARGET" --dek-file "$WORK/qwen.dek" --policy "$WORK/policy-video.signed.json" \
    --out "$PUB/video/qwen.enc.new" --provider "$PROVIDER"
chmod 644 "$PUB/video/qwen.enc.new"
mv -f "$PUB/video/qwen.enc.new" "$PUB/video/qwen.enc"
ls -l "$BASE_TARGET" "$PUB/video/qwen.enc"
echo "--- still served:"
curl -sk "https://kbs.fabstir.net/policies/$MODEL_ID.json" | python3 -c \
  'import json,sys; p=json.load(sys.stdin)["policy"]; print("served version", p["policy_version"])'
echo "STAGED video v$VERSION"
