#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phase 5 run 2, after the Tuesday GO, on kbs.fabstir.net (run as root): seal Qwen3.8
# under the signed run-2 policy and put the policy and the container where the CVM
# fetches them (TEE_POLICY_URL, TEE_BLOB_URL). Needs vps-prepare-volume.sh done (GGUF
# verified + qwen.dek in /srv/blobvol/work) and the signed policy copied to /tmp first:
#   scp docs/archive/phase5-artefacts/run2/policy-run2-v4.signed.json kbs:/tmp/
#   ssh kbs 'bash -s' < deployment/phala/run2/vps-seal-run2.sh
# The seal takes about 90 s. The DEK file is KEPT (a re-pin on the day reseals); it is
# shredded at close.
set -euo pipefail

MODEL_ID=892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650
PROVIDER=0x3d66986d29160af27409b4bc847b567e3f665b1c
COMPOSE_HASH=0b1874dc6a02e1d1d28a91c4a541f6b4c88113fee826c2255071dfe2274d5dc4
POLICY_VERSION=4
IN=/tmp/policy-run2-v4.signed.json
KBS=/usr/local/bin/fabstir-kbs
MNT=/srv/blobvol
WORK=$MNT/work
PUB=$MNT/public
SERVE=/var/lib/fabstir-kbs/public

[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }
[ -x "$KBS" ] || { echo "no $KBS" >&2; exit 1; }
mountpoint -q "$MNT" || { echo "$MNT is not mounted (vps-prepare-volume.sh first)" >&2; exit 1; }
[ -f "$WORK/Qwen3.8-27B-Q8_0.gguf" ] && [ -f "$WORK/qwen.dek" ] || { echo "GGUF or qwen.dek missing in $WORK" >&2; exit 1; }
[ -f "$IN" ] || { echo "no $IN: scp the signed policy first" >&2; exit 1; }

# The file is the one this run expects: version, compose_hash, signer, served name.
python3 - "$IN" "$POLICY_VERSION" "$COMPOSE_HASH" "$PROVIDER" "$MODEL_ID" <<'PY'
import json, sys
s = json.load(open(sys.argv[1])); p = s["policy"]
checks = {
    "policy_version": (p["policy_version"], int(sys.argv[2])),
    "compose_hash": (p["cvm"]["compose_hash"], sys.argv[3]),
    "signer": (s["signer"].lower(), sys.argv[4]),
    "encrypted_ref": (s["encrypted_ref"], "qwen.enc"),
    "model_id": (bytes(p["model_id"]).hex() if isinstance(p["model_id"], list) else str(p["model_id"]).removeprefix("0x"), sys.argv[5]),
}
bad = {k: v for k, v in checks.items() if v[0] != v[1]}
if bad:
    raise SystemExit(f"signed policy does not match run 2: {bad}")
print("signed policy OK: v%s, compose_hash %s..., signer %s" % (p["policy_version"], sys.argv[3][:12], s["signer"]))
PY

# Seal beside the served name, then swap in (a half-written container is never served).
cd "$WORK"
rm -f "$PUB/qwen.enc.new"
"$KBS" seal --model Qwen3.8-27B-Q8_0.gguf --dek-file qwen.dek --model-id "$MODEL_ID" \
    --policy "$IN" --out "$PUB/qwen.enc.new" --provider "$PROVIDER"
chmod 644 "$PUB/qwen.enc.new"
mv -f "$PUB/qwen.enc.new" "$PUB/qwen.enc"
install -d -m 0755 "$SERVE/blobs" "$SERVE/policies"
ln -sfn "$PUB/qwen.enc" "$SERVE/blobs/qwen.enc"

install -m 0644 -o fabstir-kbs -g fabstir-kbs "$IN" "$SERVE/policies/$MODEL_ID.json.new"
mv -f "$SERVE/policies/$MODEL_ID.json.new" "$SERVE/policies/$MODEL_ID.json"

# What the CVM will see (the site uses the private root, hence -k for this local look).
echo "--- served container:"
curl -skI "https://kbs.fabstir.net/blobs/qwen.enc" | grep -iE '^(HTTP|content-length|etag|accept-ranges)'
ls -l "$PUB/qwen.enc"
echo "--- served policy:"
curl -sk "https://kbs.fabstir.net/policies/$MODEL_ID.json" | python3 -c \
  'import json,sys; s=json.load(sys.stdin); print("version", s["policy"]["policy_version"], "compose_hash", s["policy"]["cvm"]["compose_hash"][:12])'
echo "--- broker:"
curl -s http://127.0.0.1:3030/v1/kbs/info; echo
echo "SEAL DONE"
