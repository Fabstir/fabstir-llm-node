#!/usr/bin/env bash
# Flush the FabCDN edge cache + reload nginx so the keys_zone is rebuilt.
# Usage: ./cache-flush.sh (prod, needs sudo) | MODE=mock ./cache-flush.sh
# Env:   CACHE_DIR=/var/cache/fabcdn  MODE=prod|mock
# Note:  CACHE_DIR applies to prod mode only; mock mode always targets the
#        container-side /var/cache/fabcdn (the docker-compose volume mount).

set -euo pipefail

# cd to script dir so MODE=mock finds docker-compose.yml regardless of cwd.
cd "$(dirname "$(readlink -f "$0")")"

CACHE_DIR="${CACHE_DIR:-/var/cache/fabcdn}"
MODE="${MODE:-prod}"

case "$MODE" in
    prod)
        # /var/cache/fabcdn is owned by www-data — sudo required.
        sudo find "$CACHE_DIR" -mindepth 1 -delete
        sudo nginx -s reload
        flushed="$CACHE_DIR"
        ;;
    mock)
        # -T disables TTY (required for non-interactive parents like the
        # test harness). Path is hardcoded to the container-side mount —
        # mock mode ignores $CACHE_DIR.
        docker compose exec -T nginx-edge-mock sh -c \
            'find /var/cache/fabcdn -mindepth 1 -delete && nginx -s reload'
        flushed="/var/cache/fabcdn (mock)"
        ;;
    *) echo "unknown MODE=$MODE (want prod|mock)" >&2; exit 2 ;;
esac

echo "flushed $flushed"
