#!/usr/bin/env bash
# Validate the host2-bound nginx snippets using `nginx -t` in a throwaway
# nginx:alpine container. The synthesized top-level nginx.conf mirrors the
# production include graph (explicit includes — no conf.d glob — so the
# image's stock default.conf isn't silently loaded).
#
# Usage:  bash tests/validate-prod-config.sh
# Env:    NGINX_IMAGE (default: nginx:alpine)
#
# Exits 0 on syntax ok, non-zero otherwise.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
NGINX_IMAGE="${NGINX_IMAGE:-nginx:alpine}"

MAP_CONF="$REPO_DIR/host2/10-fabcdn-map.conf"
LOC_CONF="$REPO_DIR/host2/fabcdn-location.conf"

for f in "$MAP_CONF" "$LOC_CONF"; do
    [ -f "$f" ] || { echo "FAIL: $f not found"; exit 1; }
done

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cat >"$TMP_DIR/nginx.conf" <<'EOF'
user  nginx;
worker_processes  1;
error_log  /dev/stderr warn;
pid        /var/run/nginx.pid;

events { worker_connections 1024; }

http {
    include       /etc/nginx/mime.types;
    default_type  application/octet-stream;
    sendfile      on;
    keepalive_timeout 65;

    include /etc/nginx/conf.d/10-fabcdn-map.conf;

    server {
        listen      80;
        server_name _;

        include /etc/nginx/snippets/fabcdn-location.conf;
    }
}
EOF

echo "==> validate-prod-config.sh (image=$NGINX_IMAGE)"

if ! command -v docker >/dev/null 2>&1; then
    echo "SKIP: docker not installed; run this on a machine with docker or on host2 after deploy"
    exit 0
fi

docker run --rm \
    -v "$TMP_DIR/nginx.conf:/etc/nginx/nginx.conf:ro" \
    -v "$MAP_CONF:/etc/nginx/conf.d/10-fabcdn-map.conf:ro" \
    -v "$LOC_CONF:/etc/nginx/snippets/fabcdn-location.conf:ro" \
    "$NGINX_IMAGE" nginx -t

echo "==> PASS: nginx -t ok"
