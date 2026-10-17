#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Generate a policy-schema-3 authority key, OFFLINE (D16), and print its address.
#
#   make-keys.sh root <directory on the SEPARATE medium>
#   make-keys.sh signer <name> [<directory>]        (default ~/fabstir-authority-keys)
#
# The ROOT key signs only authority manifests (who may sign host profile sets): it
# lives on a separate medium from the signer (encrypted USB; F9), mounted only for
# `authority sign`. A SIGNER key signs host profile sets and lives on this machine,
# like the CA root. A signer key is never reused across roots (D6).
#
# Writes the key (64 hex, mode 0600, directory 0700), never overwriting one, and
# prints its Ethereum address (computed by `fabstir-kbs key address`, the same code
# that signs) plus the line to paste into deployment/phala/authority/README.md.
# The key itself never reaches stdout. Set KBS_BIN to a built fabstir-kbs if it is
# not on PATH (`cargo build --release --no-default-features --features
# inference,kbs --bin fabstir-kbs -j 4` -> ./target/release/fabstir-kbs).
set -euo pipefail

KBS="${KBS_BIN:-fabstir-kbs}"
fail() { echo "make-keys.sh: $*" >&2; exit 1; }
command -v "$KBS" >/dev/null 2>&1 || [ -x "$KBS" ] || fail "no fabstir-kbs (set KBS_BIN)"
command -v openssl >/dev/null || fail "openssl is required"

kind="${1:?usage: make-keys.sh root <dir on the separate medium> | signer <name> [<dir>]}"
case "$kind" in
root)
    dir="${2:?root needs the directory on the separate medium}"
    file="$dir/fabstir-authority-root.key"
    # The root and the signer must never share a medium (D16, F9): a stolen laptop
    # must not yield both. The signer lives under $HOME. Checked on the nearest
    # existing ancestor BEFORE anything is created (a typo creates nothing).
    probe="$dir"
    while [ ! -e "$probe" ]; do probe="$(dirname "$probe")"; done
    if [ "$(stat -c %d "$probe")" = "$(stat -c %d "$HOME")" ]; then
        if [ "${FABSTIR_AUTHORITY_SAME_MEDIUM_OK:-}" = "dry-run" ]; then
            echo "make-keys.sh: WARNING: $dir is on the same filesystem as \$HOME (dry run only; never for the real root)" >&2
        else
            fail "$dir is on the same filesystem as \$HOME: the root lives on a SEPARATE medium (mount the encrypted USB and point at it)"
        fi
    fi
    role="root"
    ;;
signer)
    name="${2:?signer needs a name, e.g. fabstir-1}"
    [[ "$name" =~ ^[a-z0-9][a-z0-9-]{0,31}$ ]] || fail "signer name: [a-z0-9][a-z0-9-]{0,31}"
    dir="${3:-$HOME/fabstir-authority-keys}"
    file="$dir/signer-$name.key"
    role="signer $name"
    ;;
*) fail "usage: make-keys.sh root <dir> | signer <name> [<dir>]" ;;
esac

[ ! -e "$file" ] || fail "refusing: $file exists (a key is never overwritten; a new key is a new address)"
umask 077
mkdir -p "$dir"
# A FAT/exFAT medium ignores modes (they come from the mount options): warn, do
# not abort half way.
chmod 700 "$dir" || echo "make-keys.sh: WARNING: cannot chmod $dir on this filesystem" >&2
# 32 random bytes as hex straight into the file: the key is never in a shell
# variable, an argument or the terminal.
openssl rand -hex 32 > "$file"
chmod 600 "$file" || echo "make-keys.sh: WARNING: cannot chmod $file on this filesystem" >&2
if ! addr="$("$KBS" key address --key-file "$file" | sed -n 's/^address=//p')" || [ -z "$addr" ]; then
    rm -f "$file"
    fail "fabstir-kbs could not read the new key (removed it); run again"
fi

echo "$role key: $file (0600)"
echo "address: $addr"
echo "README line: | $role | \`$addr\` | $(date -u +%F) | <where the key lives> |"
if [ "$kind" = root ]; then
    echo "Next: write manifest-v1.json naming the signer(s), then"
    echo "  fabstir-kbs authority sign --manifest manifest-v1.json --key-file $file --first --out manifest-v1.signed.json"
    echo "and unmount the medium."
fi
