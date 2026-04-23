#!/usr/bin/env bash
# Report FabCDN edge cache stats (size + file count).
# Usage: ./cache-stats.sh (prod) | MODE=mock ./cache-stats.sh
# Env:   CACHE_DIR applies to prod only; mock targets the container volume.
# Output (greppable): size_kb=N, size_human=…, file_count=N

set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")"

CACHE_DIR="${CACHE_DIR:-/var/cache/fabcdn}"
MODE="${MODE:-prod}"

# Portable across GNU coreutils + BusyBox. Guards empty-cache case.
STATS_CMD='
dir="$1"
if [ ! -d "$dir" ]; then
    echo "size_kb=0"; echo "size_human=0"; echo "file_count=0"; exit 0
fi
echo "size_kb=$(du -sk "$dir" 2>/dev/null | awk "{print \$1}")"
echo "size_human=$(du -sh "$dir" 2>/dev/null | awk "{print \$1}")"
echo "file_count=$(find "$dir" -type f 2>/dev/null | wc -l | tr -d " ")"
'

case "$MODE" in
    # sudo in prod — cache subdirs are 700 www-data. Mock hardcodes path.
    prod) sudo sh -c "$STATS_CMD" _ "$CACHE_DIR" ;;
    mock) docker compose exec -T nginx-edge-mock sh -c "$STATS_CMD" _ /var/cache/fabcdn ;;
    *) echo "unknown MODE=$MODE (want prod|mock)" >&2; exit 2 ;;
esac
