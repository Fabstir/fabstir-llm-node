#!/bin/sh
# Daily read-only check of every served authority bundle (policy schema 3, D17),
# run by fabstir-kbs-check.service as the service user. For each bundle
# {root}.json in KBS_AUTHORITY_DIR it runs `fabstir-kbs authority check` with every
# served policy that names that root, --warn-days 30 and the broker's hwm/.
#
# Exit: 0 all clean; 1 any refusal (or a policy whose root has no bundle); 3
# warnings only (an expiry within 30 days, an expired profile still in the set,
# the two-boot first epoch, no HWM yet). The unit fails on any non-zero exit and
# every finding is logged as a CRITICAL line, which is what the alert keys on.
# Nothing is written: the tool opens hwm/ read-only (L1).
set -u
DATA="${KBS_DATA_DIR:-/var/lib/fabstir-kbs}"
AUTH="${KBS_AUTHORITY_DIR:-$DATA/public/authority}"
POLICIES="$DATA/public/policies"
KBS="${KBS_BIN:-/usr/local/bin/fabstir-kbs}"
rc=0
worst() { # refused (1) outranks a usage error (2), which outranks warnings (3)
    case "$1" in
    0) ;;
    1) rc=1 ;;
    *) if [ "$rc" -eq 0 ] || { [ "$rc" -eq 3 ] && [ "$1" -eq 2 ]; }; then rc=$1; fi ;;
    esac
}
roots=""
found=0
for bundle in "$AUTH"/*.json; do
    [ -e "$bundle" ] || continue
    found=1
    root="0x$(basename "$bundle" .json)"
    roots="$roots $root"
    set --
    for p in "$POLICIES"/*.json; do
        [ -e "$p" ] || continue
        # The root's address appears in a schema-3 policy only as host_authority.root.
        if grep -qi "\"$root\"" "$p"; then set -- "$@" --policy "$p"; fi
    done
    out=$("$KBS" authority check --bundle "$bundle" "$@" --warn-days 30 --hwm-dir "$DATA/hwm" 2>&1)
    code=$?
    printf '%s\n' "$out"
    if [ "$code" -ne 0 ]; then
        found_lines=$(printf '%s\n' "$out" | grep -E '^(refused|warning): ' || true)
        if [ -n "$found_lines" ]; then
            printf '%s\n' "$found_lines" | sed "s|^|CRITICAL: authority $root: |" >&2
        else
            # The tool itself failed (an unreadable file, a usage error): still CRITICAL.
            printf '%s\n' "$out" | head -n 3 | sed "s|^|CRITICAL: authority $root: exit $code: |" >&2
        fi
        worst "$code"
    fi
done
[ "$found" -eq 1 ] || { echo "CRITICAL: no authority bundle in $AUTH" >&2; worst 1; }
# A served policy whose root has no bundle refuses every request for its model.
for p in "$POLICIES"/*.json; do
    [ -e "$p" ] || continue
    covered=0
    for root in $roots; do
        if grep -qi "\"$root\"" "$p"; then covered=1; fi
    done
    if [ "$covered" -eq 0 ]; then
        echo "CRITICAL: policy $p names no served authority bundle" >&2
        worst 1
    fi
done
exit "$rc"
