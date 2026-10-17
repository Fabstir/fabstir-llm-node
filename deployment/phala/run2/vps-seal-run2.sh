#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Seal Qwen3.8 on kbs.fabstir.net (run as root) under a signed policy (schema 3) and
# put the policy and the container where the CVM fetches them (TEE_POLICY_URL,
# TEE_BLOB_URL). Written for run 2; schema-3 readers since policy v3 (the policy no
# longer pins a compose hash: host identities are the authority bundle's, so the seal
# is checked against the SERVED bundle with --authority and `authority check` confirms
# the profile). Needs vps-prepare-volume.sh done (GGUF verified + qwen.dek in
# /srv/blobvol/work), the bundle already in /var/lib/fabstir-kbs/public/authority/ and
# the signed policy copied to /tmp first:
#   scp <policy>.signed.json kbs:/tmp/policy.signed.json
#   ssh kbs 'bash -s -- <authority root 0x...> <policy version>' < deployment/phala/run2/vps-seal-run2.sh
# The seal takes about 90 s. The DEK file is KEPT (a host change never reseals under
# schema 3); it is shredded at close. The mainnet cut is a FRESH seal under a new DEK
# (D19).
set -euo pipefail

ROOT="${1:?usage: vps-seal-run2.sh <authority root 0x...> <policy version>}"
POLICY_VERSION="${2:?usage: vps-seal-run2.sh <authority root 0x...> <policy version>}"
MODEL_ID=892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650
PROVIDER=0x3d66986d29160af27409b4bc847b567e3f665b1c
IN=/tmp/policy.signed.json
KBS=/usr/local/bin/fabstir-kbs
MNT=/srv/blobvol
WORK=$MNT/work
PUB=$MNT/public
SERVE=/var/lib/fabstir-kbs/public
BUNDLE="$SERVE/authority/${ROOT#0x}.json"

[[ "$ROOT" =~ ^0x[0-9a-f]{40}$ ]] || { echo "root must be 0x + 40 lowercase hex" >&2; exit 1; }
[[ "$POLICY_VERSION" =~ ^[0-9]+$ ]] || { echo "policy version must be a number" >&2; exit 1; }
[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }
[ -f "$BUNDLE" ] || { echo "no served authority bundle $BUNDLE (deployment/phala/authority/README.md)" >&2; exit 1; }
[ -x "$KBS" ] || { echo "no $KBS" >&2; exit 1; }
mountpoint -q "$MNT" || { echo "$MNT is not mounted (vps-prepare-volume.sh first)" >&2; exit 1; }
[ -f "$WORK/Qwen3.8-27B-Q8_0.gguf" ] && [ -f "$WORK/qwen.dek" ] || { echo "GGUF or qwen.dek missing in $WORK" >&2; exit 1; }
[ -f "$IN" ] || { echo "no $IN: scp the signed policy first" >&2; exit 1; }

# The file is the one expected: a schema-3 release policy, its version, signer, served
# name, model and authority root (the seal below re-verifies the signature).
python3 - "$IN" "$POLICY_VERSION" "$ROOT" "$PROVIDER" "$MODEL_ID" <<'PY'
import json, sys
s = json.load(open(sys.argv[1])); p = s["policy"]
checks = {
    "doc": (p.get("doc"), "fabstir-model-release-policy"),
    "schema_version": (p.get("schema_version"), 3),
    "policy_version": (p["policy_version"], int(sys.argv[2])),
    "host_authority.root": (p["host_authority"]["root"], sys.argv[3]),
    "signer": (s["signer"].lower(), sys.argv[4]),
    "encrypted_ref": (s["encrypted_ref"], "qwen.enc"),
    "model_id": (bytes(p["model_id"]).hex() if isinstance(p["model_id"], list) else str(p["model_id"]).removeprefix("0x"), sys.argv[5]),
}
bad = {k: v for k, v in checks.items() if v[0] != v[1]}
if bad:
    raise SystemExit(f"signed policy is not the expected one: {bad}")
print("signed policy OK: v%s, root %s, signer %s" % (p["policy_version"], sys.argv[3], s["signer"]))
PY

# Seal beside the served name, then swap in (a half-written container is never served).
cd "$WORK"
rm -f "$PUB/qwen.enc.new"
"$KBS" seal --model Qwen3.8-27B-Q8_0.gguf --dek-file qwen.dek --model-id "$MODEL_ID" \
    --policy "$IN" --out "$PUB/qwen.enc.new" --provider "$PROVIDER" --authority "$BUNDLE"
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
  'import json,sys; s=json.load(sys.stdin); print("version", s["policy"]["policy_version"], "root", s["policy"]["host_authority"]["root"])'
echo "--- broker:"
curl -s http://127.0.0.1:3030/v1/kbs/info; echo
echo "SEAL DONE"
