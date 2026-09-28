#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phase 5 run 2, T-1 on kbs.fabstir.net (run as root): mount the recreated Vultr block
# volume, fetch the Qwen3.8 GGUF, and rebuild the DEK file from the keyring. The seal
# itself runs after the compose digests are final (the policy pins the predicted
# compose_hash). Refuses to format anything but ONE blank non-root disk of >= 90 GB.
set -euo pipefail

MNT=/srv/blobvol
MODEL_ID=892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650
GGUF_URL=https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/resolve/main/Qwen3.8-27B-Q8_0.gguf
GGUF_SHA=a680f44a06920e5d689774823782006aa3acc8db95750323373b24139b67e348
KEYRING=/var/lib/fabstir-kbs/keyring.json

# 1. The volume: exactly one candidate disk that is not the root disk.
mapfile -t CANDS < <(lsblk -dn -b -o NAME,SIZE,TYPE | awk '$3=="disk" && $1!="vda" {print $1" "$2}')
[ "${#CANDS[@]}" -eq 1 ] || { echo "expected exactly one non-root disk, found: ${CANDS[*]:-none}" >&2; exit 1; }
DEV="/dev/$(echo "${CANDS[0]}" | cut -d' ' -f1)"; SIZE=$(echo "${CANDS[0]}" | cut -d' ' -f2)
[ "$SIZE" -ge $((90 * 1000 * 1000 * 1000)) ] || { echo "$DEV is only $SIZE bytes" >&2; exit 1; }
if ! blkid "$DEV" >/dev/null 2>&1; then
    echo "formatting blank $DEV ($SIZE bytes) as ext4"
    mkfs.ext4 -q -L kbs-blobs "$DEV"
else
    echo "$DEV already has a filesystem: $(blkid -o value -s TYPE "$DEV"); mounting as is"
fi
UUID=$(blkid -o value -s UUID "$DEV")
mkdir -p "$MNT"
grep -q "UUID=$UUID" /etc/fstab || echo "UUID=$UUID $MNT ext4 defaults,noatime,nofail 0 0" >> /etc/fstab
systemctl daemon-reload
mountpoint -q "$MNT" || mount "$MNT"
install -d -m 0700 "$MNT/work"
install -d -m 0755 "$MNT/public"
df -h "$MNT" | tail -1

# 2. The GGUF, checked against the on-chain SHA-256.
cd "$MNT/work"
if [ ! -f Qwen3.8-27B-Q8_0.gguf ] || [ "$(sha256sum Qwen3.8-27B-Q8_0.gguf | cut -d' ' -f1)" != "$GGUF_SHA" ]; then
    curl -fL --retry 5 -C - -o Qwen3.8-27B-Q8_0.gguf "$GGUF_URL"
fi
GOT=$(sha256sum Qwen3.8-27B-Q8_0.gguf | cut -d' ' -f1)
[ "$GOT" = "$GGUF_SHA" ] || { echo "GGUF sha $GOT != on-chain $GGUF_SHA" >&2; exit 1; }
echo "GGUF verified $GOT"

# 3. The DEK file, rebuilt from the keyring's entry (the keyring is the source of truth;
#    the file is shredded again at close). Never printed.
umask 077
python3 - "$KEYRING" "$MODEL_ID" > qwen.dek <<'PY'
import json, sys
k = [e for e in json.load(open(sys.argv[1]))["keys"] if e["model_id"] == sys.argv[2] and not e["test"]]
assert len(k) == 1, "expected exactly one real keyring entry for the model"
print(k[0]["dek"])
PY
chmod 600 qwen.dek
echo "DEK file rebuilt ($(wc -c < qwen.dek) bytes, mode $(stat -c %a qwen.dek))"
