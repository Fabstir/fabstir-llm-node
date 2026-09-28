#!/bin/sh
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Phase 5 run 2, Phase B (video): one-shot init for the LTX sidecar in the confidential GPU
# CVM (docs/development/PLAN-PHASE5-RUN2-VIDEO.md D3/D4). Baked into ltx-sidecar-cvm.
#
#   ltx-cvm-init.sh templates     copy the baked allow-list bundle into the ltx-templates
#                                 volume. NEVER fails: the node treats a bad TEMPLATE_DIR as
#                                 "no LTX this boot" (main.rs:914-925), and a non-zero exit
#                                 here would stop the node itself (depends_on).
#   ltx-cvm-init.sh weights       download the pinned manifest into /models, verify size +
#                                 sha256, then write /models/.complete-ltx (the sidecar waits
#                                 for it). A file already verified is skipped.
#   ltx-cvm-init.sh check-urls    HEAD every pinned URL: commit, size and sha256 as pinned.
#   ltx-cvm-init.sh verify <dir>  size + sha256 of each manifest file present under
#                                 <dir>/<category>/ (the 3XS-Z rehearsal's bind mount).
#
# Integrity, not secrecy: every repo is public and ungated; each URL pins a commit and the
# manifest pins size + sha256 (the Hugging Face LFS record for that commit).
set -eu

MODE="${1:?usage: ltx-cvm-init.sh templates|weights|check-urls|verify <dir>}"
# Paths are the image/compose layout; the overrides exist only for testing the script.
MANIFEST="${LTX_MANIFEST:-/opt/fabstir/ltx-weights.txt}"
TEMPLATES_SRC="${LTX_TEMPLATES_SRC:-/opt/fabstir/ltx-templates-src}"
TEMPLATES_DEST="${LTX_TEMPLATES_DEST:-/opt/fabstir/ltx-templates}"
WEIGHTS="${LTX_WEIGHTS:-/models}"

entries() { grep -v '^#' "$MANIFEST" | grep -v '^[[:space:]]*$'; }

case "$MODE" in
templates)
    if cp -a "$TEMPLATES_SRC"/. "$TEMPLATES_DEST"/ && chmod -R a+rX "$TEMPLATES_DEST"; then
        echo "[ltx-init] templates ready ($(ls "$TEMPLATES_DEST" | wc -l) entries)"
    else
        echo "[ltx-init] template copy FAILED: the node starts without LTX this boot"
    fi
    exit 0
    ;;
weights)
    # The loop runs in THIS shell (input redirected, not piped) so set -e applies inside it:
    # a pipeline followed by `|| exit` would silently disable errexit for the whole loop.
    list=$(mktemp)
    entries > "$list"
    while IFS='|' read -r dir file bytes sha url; do
        mkdir -p "$WEIGHTS/$dir"
        f="$WEIGHTS/$dir/$file"
        if [ -f "$f" ] && [ -f "$f.verified" ] && [ "$(cat "$f.verified")" = "$sha" ] \
            && [ "$(stat -c %s "$f")" = "$bytes" ]; then
            echo "[ltx-init] $file already verified"
            continue
        fi
        rm -f "$f" "$f.verified"
        t0=$(date +%s)
        # On failure keep the .part: the next restart resumes it (-C -).
        curl -fL --retry 5 --retry-delay 15 --retry-all-errors -C - -sS -o "$f.part" "$url"
        got=$(stat -c %s "$f.part")
        if [ "$got" != "$bytes" ]; then
            echo "[ltx-init] $file: $got bytes, pinned $bytes: deleting" >&2
            rm -f "$f.part"; exit 1
        fi
        hash=$(sha256sum "$f.part" | cut -d' ' -f1)
        if [ "$hash" != "$sha" ]; then
            echo "[ltx-init] $file: sha256 $hash != pinned $sha: deleting" >&2
            rm -f "$f.part"; exit 1
        fi
        mv "$f.part" "$f"
        chmod 0644 "$f"
        printf '%s' "$sha" > "$f.verified"
        echo "[ltx-init] $file verified ($bytes bytes, $(( $(date +%s) - t0 ))s)"
    done < "$list"
    rm -f "$list"
    printf 'ltx-weights.txt verified' > "$WEIGHTS/.complete-ltx"
    chmod 0644 "$WEIGHTS/.complete-ltx"
    echo "[ltx-init] weights mode done"
    ;;
check-urls)
    bad=0
    entries | {
        while IFS='|' read -r dir file bytes sha url; do
            rev=$(echo "$url" | sed -n 's#.*/resolve/\([0-9a-f]\{40\}\)/.*#\1#p')
            h=$(curl -sSI --max-time 30 "$url" | tr -d '\r')
            size=$(echo "$h" | sed -n 's/^x-linked-size: //Ip' | head -1)
            etag=$(echo "$h" | sed -n 's/^x-linked-etag: "\{0,1\}\([0-9a-f]*\)"\{0,1\}$/\1/Ip' | head -1)
            commit=$(echo "$h" | sed -n 's/^x-repo-commit: //Ip' | head -1)
            if [ "$size" = "$bytes" ] && [ "$etag" = "$sha" ] && [ "$commit" = "$rev" ]; then
                echo "OK   $file"
            else
                echo "BAD  $file (size=$size etag=$etag commit=$commit)"; bad=1
            fi
        done
        [ "$bad" -eq 0 ] && echo "check-urls: all pinned" || { echo "check-urls: MISMATCH"; exit 1; }
    }
    ;;
verify)
    dir="${2:?usage: ltx-cvm-init.sh verify <dir>}"
    entries | while IFS='|' read -r cat file bytes sha url; do
        f="$dir/$cat/$file"
        if [ ! -f "$f" ]; then echo "ABSENT $cat/$file"; continue; fi
        got=$(stat -c %s "$f"); hash=$(sha256sum "$f" | cut -d' ' -f1)
        if [ "$got" = "$bytes" ] && [ "$hash" = "$sha" ]; then echo "OK     $cat/$file"
        else echo "BAD    $cat/$file ($got bytes, $hash)"; fi
    done
    ;;
*)
    echo "usage: ltx-cvm-init.sh templates|weights|check-urls|verify <dir>" >&2
    exit 2
    ;;
esac
