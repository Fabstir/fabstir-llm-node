#!/usr/bin/env bash
# Smoke test for the FabCDN edge (mock or prod).
#
# Usage:
#   bash smoke-test.sh                              # defaults to local mock
#   bash smoke-test.sh http://localhost:8090/fabcdn
#   BASE_URL=... CID_PUBLIC=... FLUSH_CMD='...' bash smoke-test.sh
#
# Env vars:
#   BASE_URL    Edge URL root (ends with /fabcdn). Positional $1 wins.
#   CID_PUBLIC  A known-resolvable S5 blob CID to probe. Default: set below.
#   FLUSH_CMD   Shell snippet that clears the edge cache. Default assumes
#               local mock via docker compose exec. If empty, cold-state
#               tests are skipped with a warning.

set -euo pipefail

BASE_URL="${1:-${BASE_URL:-http://localhost:8090/fabcdn}}"
# ⚠ The default below is a placeholder — the authoring environment could
# not verify it resolves on s5.platformlessai.ai. Before a real smoke run,
# override with a known-public CID:   CID_PUBLIC=<real-cid> bash smoke-test.sh
CID_PUBLIC="${CID_PUBLIC:-zb2rhYjPDYSAaqcoveiA27skiaZe3gA7P2bVJ68TvD3kFFY4j}"
FLUSH_CMD="${FLUSH_CMD:-docker compose exec -T nginx-edge-mock sh -c 'find /var/cache/fabcdn -mindepth 1 -delete && nginx -s reload'}"

FAILED=0
FAILED_TESTS=()

# check_header URL METHOD HEADER EXPECTED_SUBSTRING
# Fails loudly on mismatch.
check_header() {
    local url="$1" method="$2" header="$3" expected="$4"
    local headers
    if [ "$method" = "HEAD" ]; then
        headers=$(curl -sI "$url")
    else
        headers=$(curl -s -D - -X "$method" -o /dev/null "$url")
    fi
    local line
    line=$(printf '%s\n' "$headers" | grep -i "^${header}:" | head -1 || true)
    if [ -z "$line" ]; then
        echo "  FAIL: $method $url — header '$header' missing"
        return 1
    fi
    if ! printf '%s' "$line" | grep -qi -- "$expected"; then
        echo "  FAIL: $method $url — header '$header' = '$line', expected to contain '$expected'"
        return 1
    fi
    echo "  OK:   $method $url — $header contains '$expected'"
    return 0
}

record_fail() { FAILED=1; FAILED_TESTS+=("$1"); }

test_mock_reachable() {
    echo "[1/7] test_mock_reachable"
    local code
    code=$(curl -sI -o /dev/null -w "%{http_code}" "$BASE_URL/s5/blob/$CID_PUBLIC" 2>/dev/null || true)
    if [ "$code" = "200" ]; then
        echo "  OK:   HEAD $BASE_URL/s5/blob/\$CID → 200"
    else
        echo "  FAIL: HEAD $BASE_URL/s5/blob/\$CID → $code (expected 200)"
        record_fail test_mock_reachable
    fi
}

test_cold_cache_miss_on_get() {
    echo "[2/7] test_cold_cache_miss_on_get"
    if [ -z "$FLUSH_CMD" ]; then
        echo "  SKIP: FLUSH_CMD empty"
        return 0
    fi
    eval "$FLUSH_CMD" >/dev/null 2>&1 || { echo "  FAIL: FLUSH_CMD errored"; record_fail test_cold_cache_miss_on_get; return 0; }
    check_header "$BASE_URL/s5/blob/$CID_PUBLIC" GET "X-Cache" "MISS" \
        || record_fail test_cold_cache_miss_on_get
}

test_warm_cache_hit_on_get() {
    echo "[3/7] test_warm_cache_hit_on_get"
    check_header "$BASE_URL/s5/blob/$CID_PUBLIC" GET "X-Cache" "HIT" \
        || record_fail test_warm_cache_hit_on_get
}

test_head_does_not_warm_cache() {
    echo "[4/7] test_head_does_not_warm_cache"
    if [ -z "$FLUSH_CMD" ]; then
        echo "  SKIP: FLUSH_CMD empty"
        return 0
    fi
    eval "$FLUSH_CMD" >/dev/null 2>&1 || { echo "  FAIL: FLUSH_CMD errored"; record_fail test_head_does_not_warm_cache; return 0; }
    check_header "$BASE_URL/s5/blob/$CID_PUBLIC" HEAD "X-Cache" "MISS" \
        || { record_fail test_head_does_not_warm_cache; return 0; }
    check_header "$BASE_URL/s5/blob/$CID_PUBLIC" HEAD "X-Cache" "MISS" \
        || { record_fail test_head_does_not_warm_cache; return 0; }
    check_header "$BASE_URL/s5/blob/$CID_PUBLIC" GET "X-Cache" "MISS" \
        || { record_fail test_head_does_not_warm_cache; return 0; }
    check_header "$BASE_URL/s5/blob/$CID_PUBLIC" HEAD "X-Cache" "HIT" \
        || record_fail test_head_does_not_warm_cache
}

test_non_blob_passthrough_returns_bypass() {
    echo "[5/7] test_non_blob_passthrough_returns_bypass"
    check_header "$BASE_URL/s5/account/register" GET "X-Cache" "BYPASS" \
        || record_fail test_non_blob_passthrough_returns_bypass
}

test_cors_headers_present() {
    echo "[6/7] test_cors_headers_present"
    local headers
    headers=$(curl -s -D - -H "Origin: http://localhost:3000" -o /dev/null "$BASE_URL/s5/blob/$CID_PUBLIC" || echo "")
    if ! printf '%s' "$headers" | grep -qi "^Access-Control-Allow-Origin:.*\*"; then
        echo "  FAIL: Access-Control-Allow-Origin missing or not '*'"
        record_fail test_cors_headers_present; return 0
    fi
    if ! printf '%s' "$headers" | grep -qi "^Access-Control-Expose-Headers:.*X-Cache"; then
        echo "  FAIL: Access-Control-Expose-Headers missing X-Cache"
        record_fail test_cors_headers_present; return 0
    fi
    echo "  OK:   CORS headers present with X-Cache exposed"
}

# check_range TMPDIR URL START END LABEL
# Fetches one byte range through the edge and compares it, byte for byte,
# against the same slice of the reference full-object download.
check_range() {
    local tmp="$1" url="$2" start="$3" end="$4" label="$5"
    local len=$(( end - start + 1 ))
    tail -c "+$(( start + 1 ))" "$tmp/full" | head -c "$len" > "$tmp/expect"
    curl -sL -r "${start}-${end}" -o "$tmp/got" "$url" || {
        echo "  FAIL: $label — curl errored"; return 1; }
    if ! cmp -s "$tmp/expect" "$tmp/got"; then
        echo "  FAIL: $label — bytes ${start}-${end} do not match the full object"
        echo "        (got $(wc -c < "$tmp/got") bytes, expected $len)"
        echo "        This is the 206-cache-key collision: ranges sharing one entry."
        return 1
    fi
    echo "  OK:   $label — bytes ${start}-${end} match"
    return 0
}

test_range_integrity() {
    echo "[7/7] test_range_integrity"
    # Regression guard for the range-collision bug. Caching 206 under a key that
    # omits the range makes two different byte ranges of one object collide on a
    # single cache entry; nginx then serves whichever landed first for both, and
    # the video is silently corrupt. `slice` + $slice_range in proxy_cache_key is
    # the fix — this test is what proves it stayed fixed.
    local url="$BASE_URL/s5/blob/$CID_PUBLIC"
    local tmp
    tmp=$(mktemp -d)

    if ! curl -sL -o "$tmp/full" "$url"; then
        echo "  FAIL: reference full-object fetch errored"
        record_fail test_range_integrity; rm -rf "$tmp"; return 0
    fi
    local size
    size=$(wc -c < "$tmp/full")
    if [ "$size" -lt 2097152 ]; then
        echo "  SKIP: object is $size bytes — needs >2 MiB to cross a 1 MB slice"
        echo "        boundary. Re-run with CID_PUBLIC=<a real video blob>."
        rm -rf "$tmp"; return 0
    fi

    # Cold cache, so the first ranged request is the one that populates it.
    if [ -n "$FLUSH_CMD" ]; then
        eval "$FLUSH_CMD" >/dev/null 2>&1 || true
    else
        echo "  WARN: FLUSH_CMD empty — running against a warm cache, weaker signal"
    fi

    local ok=0
    # Order matters. The boundary-crossing range goes first so it populates the
    # cache; the in-slice-0 range that follows is what a colliding key gets wrong.
    check_range "$tmp" "$url" 1040000 1060000 "crosses the 1 MB slice boundary" || ok=1
    check_range "$tmp" "$url"       0  999999 "wholly inside slice 0"           || ok=1
    check_range "$tmp" "$url" 1500000 1600000 "wholly inside slice 1"           || ok=1
    # Same range again, now warm: a HIT must return the same bytes as the MISS.
    check_range "$tmp" "$url" 1040000 1060000 "boundary range re-read warm"     || ok=1

    [ "$ok" -eq 0 ] || record_fail test_range_integrity
    rm -rf "$tmp"
}

echo "==> smoke-test.sh against $BASE_URL (CID=$CID_PUBLIC)"
test_mock_reachable
test_cold_cache_miss_on_get
test_warm_cache_hit_on_get
test_head_does_not_warm_cache
test_non_blob_passthrough_returns_bypass
test_cors_headers_present
test_range_integrity

if [ "$FAILED" -eq 0 ]; then
    echo "==> SUMMARY: all smoke checks passed"
    exit 0
fi
echo "==> SUMMARY: FAILED — ${FAILED_TESTS[*]}"
exit 1
