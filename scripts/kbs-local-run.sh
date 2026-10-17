#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Policy schema 3, the hands-on local run (V3.6). DEV CONTAINER ONLY: it
# refuses on a box with /var/lib/fabstir-kbs (a broker home).
#
# Builds the documents with the real tooling under throw-away keys (root,
# signer, provider: fresh per run, test only), starts `fabstir-kbs serve` on
# loopback under KBS_CPU_EVIDENCE=simulator + KBS_GPU_EVIDENCE=canned with a
# TEST keyring, and runs the driver (tests/kbs/test_local_driver.rs) against
# the live process, swapping the served bundle between steps:
#   1. epoch 1 (the simulator profile + an expired twin)  -> released
#   2. epoch 2 (the simulator profile removed)            -> refused, archive hint
#   3. epoch 1 re-served                                  -> refused below the HWM
#   4. epoch 3 (the profile back)                         -> released
# under ONE signed policy whose policy_hash is printed at every step.
#
# Egress: each release posts NVIDIA's canned sample evidence to NRAS (keyless,
# free; NRAS answers NONCE_NOT_MATCHING, which only a test entry tolerates).
# Nothing else leaves the box.
#
# Usage: scripts/kbs-local-run.sh [run dir]    (default: a fresh mktemp dir)
# Output: <run dir>/transcript.txt (this script's output), serve.log (the
# broker), build.log and driver.log (cargo). The run dir keeps the throw-away
# keys (mode 0600); delete it when done.
set -euo pipefail

if [ -e /var/lib/fabstir-kbs ]; then
    echo "kbs-local-run: /var/lib/fabstir-kbs exists: this is a broker box, refusing" >&2
    exit 1
fi
REPO=$(cd "$(dirname "$0")/.." && pwd)
umask 077
RUN=${1:-$(mktemp -d "${TMPDIR:-/tmp}/kbs-local-run.XXXXXX")}
mkdir -p "$RUN"
if [ -n "$(ls -A "$RUN")" ]; then
    echo "kbs-local-run: $RUN is not empty" >&2
    exit 1
fi
RUN=$(cd "$RUN" && pwd)
mkdir -p "$RUN/keys" "$RUN/home" "$RUN/data/public/policies" "$RUN/data/public/authority"
exec > >(tee "$RUN/transcript.txt") 2>&1

export RISC0_SKIP_BUILD=${RISC0_SKIP_BUILD:-1}
KBS=${CARGO_TARGET_DIR:-$REPO/target}/debug/fabstir-kbs
cd "$REPO"

echo "== kbs-local-run $(date -u +%FT%TZ) in $RUN"
echo "   tree $(git rev-parse --short HEAD 2>/dev/null || echo unknown)$(git diff --quiet 2>/dev/null || echo ' (+ uncommitted changes)')"
echo "== build (cargo test --no-run; log: build.log)"
cargo test --features kbs --test kbs_tests -j 4 --no-run >"$RUN/build.log" 2>&1 || {
    tail -n 30 "$RUN/build.log"
    exit 1
}
[ -x "$KBS" ] || { echo "kbs-local-run: no binary at $KBS" >&2; exit 1; }
echo "   fabstir-kbs sha256 $(sha256sum "$KBS" | cut -c1-16)…"

# The tooling, with HOME pointed at the run (the authority ledger lives there).
kbs() { HOME="$RUN/home" "$KBS" "$@"; }

# One ignored driver test; only its `transcript| ` lines are shown.
driver() {
    local t=$1
    shift
    if ! env "$@" cargo test --features kbs --test kbs_tests -j 4 -q \
        "kbs::test_local_driver::$t" -- --ignored --exact --nocapture --test-threads=1 \
        >>"$RUN/driver.log" 2>&1; then
        grep '^transcript| ' "$RUN/driver.log" | sed 's/^transcript| /   /' || true
        echo "-- driver $t FAILED; the end of driver.log:"
        tail -n 30 "$RUN/driver.log"
        exit 1
    fi
}

# check <ok|warn|refused> <args>: `authority check` (exit 0 ok, 3 warnings only,
# 1 refused) must give the verdict this step expects, or the run stops.
check() {
    local want=$1 rc=0 got
    shift
    kbs authority check "$@" | sed 's/^/   /' || rc=${PIPESTATUS[0]}
    case $rc in 0) got=ok ;; 3) got=warn ;; 1) got=refused ;; *) got="exit $rc" ;; esac
    if [ "$got" != "$want" ]; then
        echo "kbs-local-run: authority check gave $got, expected $want" >&2
        exit 1
    fi
}

echo "== throw-away keys (never printed)"
for k in root signer provider; do
    openssl rand -hex 32 >"$RUN/keys/$k.key"
done
addr() { kbs key address --key-file "$RUN/keys/$1.key" | sed -n 's/^address=//p'; }
ROOT=$(addr root)
SIGNER=$(addr signer)
PROVIDER=$(addr provider)
echo "   root=$ROOT signer=$SIGNER provider=$PROVIDER"
# A test model id: the reserved t5t: prefix (74 35 74 3a) a test entry requires.
MODEL=7435743a$(printf '0%.0s' $(seq 54))36
NOW=$(date +%s)

echo "== unsigned documents (the driver's prepare: the simulator recording's identity)"
driver prepare LOCAL_KBS_DIR="$RUN" LOCAL_KBS_ROOT="$ROOT" LOCAL_KBS_SIGNER="$SIGNER" \
    LOCAL_KBS_MODEL="$MODEL"
echo "   policy.json manifest.json profile-live.json profile-expired.json"

echo "== manifest v1 (authority sign --first)"
kbs authority sign --manifest "$RUN/manifest.json" --key-file "$RUN/keys/root.key" --first \
    --out "$RUN/manifest.signed.json" | sed 's/^/   /'

# epoch <n> <previous bundle or -> <profile>...: assemble, sign, bundle.
epoch() {
    local n=$1 prev=$2
    shift 2
    local prior=(--first)
    [ "$prev" = - ] || prior=(--previous "$RUN/$prev")
    local profiles=()
    for p in "$@"; do profiles+=(--profile "$RUN/profile-$p.json"); done
    echo "== epoch $n: $*"
    kbs profile assemble --root "$ROOT" --epoch "$n" --issued-at "$NOW" "${prior[@]}" \
        "${profiles[@]}" --out "$RUN/set-e$n.json" | sed 's/^/   /'
    kbs profile sign --set "$RUN/set-e$n.json" --key-file "$RUN/keys/signer.key" \
        --manifest "$RUN/manifest.signed.json" --out "$RUN/set-e$n.signed.json" | sed 's/^/   /'
    kbs authority assemble --manifest-signed "$RUN/manifest.signed.json" \
        --set-signed "$RUN/set-e$n.signed.json" "${prior[@]}" --out "$RUN/bundle-e$n.json" |
        grep -v '^ledger=' | sed 's/^/   /'
}
epoch 1 - live expired
epoch 2 bundle-e1.json expired
epoch 3 bundle-e2.json live

echo "== policy (policy sign --authority bundle-e1.json) and the test keyring entry"
kbs policy sign --policy "$RUN/policy.json" --key-file "$RUN/keys/provider.key" \
    --encrypted-ref models/local-run.enc --out "$RUN/policy.signed.json" \
    --authority "$RUN/bundle-e1.json" | sed 's/^/   /'
cp "$RUN/policy.signed.json" "$RUN/data/public/policies/$MODEL.json"
kbs keyring add --model-id "$MODEL" --provider "$PROVIDER" --generate \
    --dek-out "$RUN/keys/dek.hex" --test --note "kbs-local-run" \
    --file "$RUN/data/keyring.json" 2>&1 | sed "s|$RUN/||; s/^/   /"

# Epoch 2 revokes the only live profile, so the tool refuses it (nothing would
# release): served anyway here, to show the broker's refusal and its hint.
echo "== authority check, each bundle (expected: warn, refused, ok)"
check warn --bundle "$RUN/bundle-e1.json" --policy "$RUN/policy.signed.json"
check refused --bundle "$RUN/bundle-e2.json" --previous "$RUN/bundle-e1.json" \
    --policy "$RUN/policy.signed.json"
check ok --bundle "$RUN/bundle-e3.json" --previous "$RUN/bundle-e2.json" \
    --policy "$RUN/policy.signed.json"

PORT=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
echo "== serve on 127.0.0.1:$PORT (log: serve.log)"
env -i PATH="$PATH" HOME="$RUN/home" RUST_LOG=info NO_COLOR=1 \
    KBS_LISTEN="127.0.0.1:$PORT" KBS_DATA_DIR="$RUN/data" \
    KBS_CPU_EVIDENCE=simulator KBS_GPU_EVIDENCE=canned \
    "$KBS" serve >"$RUN/serve.log" 2>&1 &
SERVE=$!
trap 'kill "$SERVE" 2>/dev/null || true; wait "$SERVE" 2>/dev/null || true' EXIT
ready=no
for _ in $(seq 60); do
    curl -sf --max-time 2 "http://127.0.0.1:$PORT/v1/kbs/info" >/dev/null && { ready=yes; break; }
    kill -0 "$SERVE" 2>/dev/null || break
    sleep 0.5
done
if [ "$ready" != yes ]; then
    echo "kbs-local-run: serve did not answer /v1/kbs/info; serve.log:"
    cat "$RUN/serve.log"
    exit 1
fi

echo "== transcript (the driver against the live broker)"
driver transcript LOCAL_KBS_DIR="$RUN" LOCAL_KBS_URL="http://127.0.0.1:$PORT"
grep '^transcript| ' "$RUN/driver.log" | sed 's/^transcript| /   /' | grep -v '^   prepared '

strip() { sed 's/\x1b\[[0-9;]*m//g' "$RUN/serve.log"; }
echo "== the broker's own lines (serve.log)"
strip | grep -E 'request_key refused|challenge refused| released|CRITICAL|high-water' |
    sed -E 's/^[0-9T:.-]+Z +//; s/^/   /' | cut -c1-400
echo "== confirm-loaded: the served bundle against the broker's HWM"
check ok --bundle "$RUN/bundle-e3.json" --policy "$RUN/policy.signed.json" \
    --hwm-dir "$RUN/data/hwm"
echo "   archive: $(find "$RUN/data/archive" -type f -printf "%P\n" | sort | tr '\n' ' ')"
echo "== done: $RUN"
