#!/bin/sh
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phase 5 run 2: one-shot init for the confidential GPU CVM (no bind mounts are
# allowed in the measured compose, so everything the sidecars read from disk is
# produced here, inside the CVM, into named volumes on the dstack-encrypted disk).
#
#   cvm-init.sh template   fast (seconds, no network). The training template, its
#                          baked tokenizer, and the trainer volumes' ownership. The
#                          node starts only after this completes, because it loads
#                          TRAINING_TEMPLATE_PATH once at boot and a missing file
#                          disables training for the whole boot.
#   cvm-init.sh weights    slow (the Qwen3.8-27B training base, 55.6 GB, and
#                          FLUX.2 Klein 4B, 23.7 GB). One marker per model; each
#                          sidecar waits only for its own model's marker.
#
# Integrity, not secrecy: both repos are public (Apache-2.0, ungated) and are
# fetched at PINNED revisions; the trainer then re-verifies every Qwen weight file
# against the sha256 list in the template before it opens its socket (pins.py),
# and this script checks the tokenizer against the template's pin itself.
# Runs as root (the compose sets user: "0") so it can hand the volumes to uid 1000.
set -eu

MODE="${1:?usage: cvm-init.sh template|weights}"

QWEN_REPO="unsloth/Qwen3.8-27B"
QWEN_REV="3ea932cee0a432ae86e9c7826cbe8aef52323a28"
FLUX_REPO="black-forest-labs/FLUX.2-klein-4B"
FLUX_REV="e7b7dc27f91deacad38e78976d1f2b499d76a294"

TEMPLATE_DIR=/opt/fabstir/template
WEIGHTS_DIR=/weights

export HF_HUB_DISABLE_TELEMETRY=1
export HF_HUB_DISABLE_PROGRESS_BARS=1   # public logs stay readable

case "$MODE" in
template)
    # No network here: v1.json and the pinned tokenizer are baked into the image
    # (Dockerfile.trainer-cvm), so this cannot fail on a Hugging Face outage.
    mkdir -p "$TEMPLATE_DIR"
    cp /opt/fabstir/template-src/v1.json /opt/fabstir/template-src/tokenizer.json "$TEMPLATE_DIR/"
    TEMPLATE_DIR="$TEMPLATE_DIR" python - <<'PY'
import hashlib, json, os
tdir = os.environ["TEMPLATE_DIR"]
template = json.load(open(os.path.join(tdir, "v1.json")))
want = template["base"]["tokenizerSha256"].lower()
got = "0x" + hashlib.sha256(open(os.path.join(tdir, "tokenizer.json"), "rb").read()).hexdigest()
if got != want:
    raise SystemExit(f"tokenizer.json sha {got} != template pin {want}: refusing")
print(f"[cvm-init] template + tokenizer ready ({got})")
PY
    chmod 755 "$TEMPLATE_DIR"
    chmod 644 "$TEMPLATE_DIR"/v1.json "$TEMPLATE_DIR"/tokenizer.json
    # The trainer runs as uid 1000; named volumes are created root-owned
    # (fabstir-trainer/docs/DEPLOY-CONTAINER.md, Permissions).
    chown 1000:1000 /var/run/fabstir-trainer /var/lib/fabstir/training/staging /var/lib/fabstir/training/work
    chmod 770 /var/run/fabstir-trainer
    echo "[cvm-init] template mode done"
    ;;
weights)
    # One completion marker PER MODEL: the trainer waits only for the Qwen one and the
    # image sidecar only for the FLUX one (compose entrypoints), so neither waits on,
    # or can be blocked by, the other's download, and `docker compose up` never blocks.
    # restart: on-failure re-runs this until both succeed; a matching marker makes that
    # model's re-run a no-op. Qwen first: training is the longer path (download + the
    # trainer's 55 GB hash + the run itself).
    mkdir -p "$WEIGHTS_DIR"
    for spec in "$QWEN_REPO|$QWEN_REV|qwen38-27b" "$FLUX_REPO|$FLUX_REV|$FLUX_REPO"; do
        repo=${spec%%|*}; rest=${spec#*|}; rev=${rest%%|*}; sub=${rest#*|}
        marker="$WEIGHTS_DIR/.complete-$(echo "$sub" | tr '/' '_')"
        if [ -f "$marker" ] && [ "$(cat "$marker")" = "$repo@$rev" ]; then
            echo "[cvm-init] $repo already complete"
            continue
        fi
        rm -f "$marker"
        REPO="$repo" REV="$rev" DEST="$WEIGHTS_DIR/$sub" python - <<'PY'
import os, time
from huggingface_hub import snapshot_download

repo, rev, dest = os.environ["REPO"], os.environ["REV"], os.environ["DEST"]
t0 = time.time()
for attempt in range(1, 6):  # resumes on retry; compose then re-runs the service (on-failure)
    try:
        path = snapshot_download(repo, revision=rev, local_dir=dest, max_workers=8)
        break
    except Exception as exc:  # network, 5xx, rate limit
        if attempt == 5:
            raise
        print(f"[cvm-init] {repo} attempt {attempt} failed ({exc!r}); retrying in {30 * attempt}s", flush=True)
        time.sleep(30 * attempt)
print(f"[cvm-init] {repo}@{rev[:12]} -> {path} in {time.time() - t0:.0f}s", flush=True)
PY
        chmod -R a+rX "$WEIGHTS_DIR/$sub"
        printf '%s' "$repo@$rev" > "$marker"
        chmod 644 "$marker"
        echo "[cvm-init] $repo complete"
    done
    echo "[cvm-init] weights mode done"
    ;;
*)
    echo "usage: cvm-init.sh template|weights" >&2
    exit 2
    ;;
esac
