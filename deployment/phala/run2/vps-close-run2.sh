#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phase 5 run 2 close, on kbs.fabstir.net (root), AFTER the CVM is destroyed. Mirrors run 1's
# close (2026-09-23): shred the DEK working file (the keyring entry stays, it is the source of
# truth), drop the served blob link, unmount the blob volume and comment its fstab line (backup
# kept), report any nginx allow-list. Then delete the Vultr volume in the Vultr dashboard:
# nothing on it is irreplaceable (the GGUF re-downloads, every container reseals from the
# keyring, the signed policies live in the repo's docs/archive).
#   ssh kbs 'bash -s' < deployment/phala/run2/vps-close-run2.sh
set -euo pipefail
MNT=/srv/blobvol
SERVE=/var/lib/fabstir-kbs/public
STAMP=$(date -u +%Y%m%d)
[ "$(id -u)" -eq 0 ] || { echo "run as root" >&2; exit 1; }

# 1. The DEK working file.
if [ -f "$MNT/work/qwen.dek" ]; then
    shred -u "$MNT/work/qwen.dek"
    echo "qwen.dek shredded (keyring entry kept)"
else
    echo "qwen.dek: not present"
fi

# 2. The served container link (it points into the volume).
if [ -L "$SERVE/blobs/qwen.enc" ] || [ -e "$SERVE/blobs/qwen.enc" ]; then
    rm -f "$SERVE/blobs/qwen.enc"
    echo "blob link removed"
fi

# 3. The volume.
if mountpoint -q "$MNT"; then
    umount "$MNT"
    echo "$MNT unmounted"
fi
if grep -qE "^[^#].*[[:space:]]$MNT[[:space:]]" /etc/fstab; then
    cp -p /etc/fstab "/root/fstab.pre-volume-removal-$STAMP"
    sed -i -E "s|^([^#].*[[:space:]]$MNT[[:space:]].*)$|# removed at run-2 close $STAMP: \1|" /etc/fstab
    systemctl daemon-reload
    echo "fstab line commented (backup /root/fstab.pre-volume-removal-$STAMP)"
fi

# 4. Report only.
echo "--- nginx allow-list (run 1 removed its 'deny all' at close; nothing changed here):"
grep -rn 'deny all' /etc/nginx/sites-enabled/ 2>/dev/null || echo "none"
echo "--- served policy (left in place; public by design):"
curl -sk "https://kbs.fabstir.net/policies/892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650.json" \
  | python3 -c 'import json,sys; p=json.load(sys.stdin)["policy"]; print("policy version", p["policy_version"])' || true
echo "--- block devices:"; lsblk -o NAME,SIZE,MOUNTPOINT
echo "CLOSED run 2 on the key server; now detach and delete the Vultr volume"
