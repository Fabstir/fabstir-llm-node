#!/usr/bin/env bash
# Exercise cache-flush.sh + cache-stats.sh against the running mock.
#
# Preconditions: mock is up (docker compose up -d from services/nginx-edge/).
# Failures report script+line for fast triage.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

FLUSH="$REPO_DIR/cache-flush.sh"
STATS="$REPO_DIR/cache-stats.sh"
BASE_URL="${BASE_URL:-http://localhost:8090/fabcdn}"
CID_PUBLIC="${CID_PUBLIC:-zb2rhYjPDYSAaqcoveiA27skiaZe3gA7P2bVJ68TvD3kFFY4j}"

fail() { echo "FAIL ($(basename "$0"):$1): $2" >&2; exit 1; }

for script in "$FLUSH" "$STATS"; do
    [ -x "$script" ] || fail "$LINENO" "$script missing or not executable"
done

# Read file_count from cache-stats.sh output. awk (not grep|cut) so a
# missing file_count line returns empty instead of triggering pipefail
# exit — the caller's [ empty != "0" ] branch reports the clearer error.
stats_count() {
    MODE=mock "$STATS" | awk -F= '/^file_count=/ {print $2; exit}'
}

echo "==> test-helpers.sh"

# 1. flush + assert empty.
MODE=mock "$FLUSH" >/dev/null || fail "$LINENO" "cache-flush.sh (1st) returned non-zero"
count=$(stats_count) || fail "$LINENO" "cache-stats.sh (1st) returned non-zero"
[ "$count" = "0" ] || fail "$LINENO" "after initial flush, file_count=$count (want 0)"
echo "  OK:  flush leaves cache empty"

# 2. Warm the cache with a GET, expect >=1 file.
curl -s -o /dev/null "$BASE_URL/s5/blob/$CID_PUBLIC" || fail "$LINENO" "warm GET failed"
count=$(stats_count) || fail "$LINENO" "cache-stats.sh (2nd) returned non-zero"
[ "$count" -ge 1 ] 2>/dev/null || fail "$LINENO" "after warm GET, file_count='$count' (want >=1)"
echo "  OK:  warm GET populates cache (file_count=$count)"

# 3. flush again, expect empty.
MODE=mock "$FLUSH" >/dev/null || fail "$LINENO" "cache-flush.sh (2nd) returned non-zero"
count=$(stats_count) || fail "$LINENO" "cache-stats.sh (3rd) returned non-zero"
[ "$count" = "0" ] || fail "$LINENO" "after 2nd flush, file_count=$count (want 0)"
echo "  OK:  flush empties warmed cache"

echo "==> all test-helpers assertions passed"
