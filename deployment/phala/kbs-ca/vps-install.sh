#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# The VPS side of the private CA, run ON kbs.fabstir.net as root, in three
# stages that must happen in this order and never skip the verify:
#
#   vps-install.sh csr                       # server key (stays here) + CSR on stdout
#   vps-install.sh install <kbs.pem> <root.pem>   # nginx site with the private cert,
#                                            # then s_client verify; rolls back on failure
#   vps-install.sh retire                    # only after install verified: certbot out,
#                                            # port 80 closed, after proving nothing else
#                                            # listens on 80
set -euo pipefail

TLS=/etc/fabstir-kbs/tls
SITE=/etc/nginx/sites-available/kbs.fabstir.net
NAME=kbs.fabstir.net

stage="${1:?usage: vps-install.sh csr | install <kbs.pem> <root.pem> | retire}"

case "$stage" in
csr)
    umask 077
    mkdir -p "$TLS" && chmod 700 "$TLS"
    if [ -f "$TLS/kbs.key" ]; then
        echo "server key already exists at $TLS/kbs.key; reusing it for the CSR" >&2
    else
        openssl ecparam -name secp384r1 -genkey -noout -out "$TLS/kbs.key"
        chmod 600 "$TLS/kbs.key"
    fi
    openssl req -new -key "$TLS/kbs.key" -subj "/CN=$NAME" -out "$TLS/kbs.csr"
    cat "$TLS/kbs.csr"
    ;;

install)
    CERT="${2:?install needs <kbs.pem>}"; ROOT="${3:?install needs <root.pem>}"
    [ -f "$TLS/kbs.key" ] || { echo "no server key; run the csr stage first" >&2; exit 66; }
    # The cert must match OUR key and chain to the root we are about to pin.
    [ "$(openssl x509 -in "$CERT" -noout -pubkey)" = "$(openssl ec -in "$TLS/kbs.key" -pubout 2>/dev/null)" ] \
        || { echo "kbs.pem does not match $TLS/kbs.key" >&2; exit 65; }
    openssl verify -CAfile "$ROOT" "$CERT" >/dev/null || { echo "kbs.pem does not chain to the root" >&2; exit 65; }
    install -m 644 "$CERT" "$TLS/kbs.pem"
    install -m 644 "$ROOT" "$TLS/kbs-root.pem"

    # Back up whatever nginx serves now, so a failed verify restores it.
    BK="/root/nginx-before-private-ca.$(date -u +%Y%m%dT%H%M%SZ)"
    mkdir -p "$BK" && cp -a /etc/nginx/sites-enabled "$BK/"
    # The site TARGET and the zone snippet are part of what a failed verify must
    # undo too (the sites-enabled symlink alone would still point at the new file).
    [ -f "$SITE" ] && cp -a "$SITE" "$BK/site.conf"
    [ -f /etc/nginx/conf.d/kbs-limits.conf ] && cp -a /etc/nginx/conf.d/kbs-limits.conf "$BK/kbs-limits.conf"
    restore_from_backup() {
        rm -f /etc/nginx/sites-enabled/$NAME; cp -a "$BK/sites-enabled/." /etc/nginx/sites-enabled/
        if [ -f "$BK/site.conf" ]; then cp -a "$BK/site.conf" "$SITE"; else rm -f "$SITE"; fi
        if [ -f "$BK/kbs-limits.conf" ]; then
            cp -a "$BK/kbs-limits.conf" /etc/nginx/conf.d/kbs-limits.conf
        else
            rm -f /etc/nginx/conf.d/kbs-limits.conf
        fi
    }
    # The site file's limit_req/limit_conn need http{}-context zones: ship the
    # snippet first (deployment/kbs/kbs-limits.conf; inline fallback when the
    # broker tree is not on this box) or nginx -t fails on "zero size shared memory zone".
    if [ -f "$(dirname "$0")/../../kbs/kbs-limits.conf" ]; then
        install -m 644 "$(dirname "$0")/../../kbs/kbs-limits.conf" /etc/nginx/conf.d/kbs-limits.conf
    else
        printf 'limit_req_zone $binary_remote_addr zone=kbs:1m rate=10r/s;\nlimit_conn_zone $binary_remote_addr zone=kbs_conn:1m;\n' \
            > /etc/nginx/conf.d/kbs-limits.conf
    fi
    install -m 644 "$(dirname "$0")/nginx-kbs.conf" "$SITE"
    ln -sf "$SITE" /etc/nginx/sites-enabled/$NAME
    rm -f /etc/nginx/sites-enabled/default
    if ! nginx -t; then
        echo "nginx -t failed; restoring $BK" >&2
        restore_from_backup
        nginx -t || echo "restored config still fails nginx -t; inspect $BK" >&2
        exit 70
    fi
    systemctl reload nginx
    sleep 1
    OUT="$(openssl s_client -connect 127.0.0.1:443 -servername "$NAME" -tls1_3 -CAfile "$ROOT" </dev/null 2>/dev/null || true)"
    if echo "$OUT" | grep -q 'Verify return code: 0 (ok)' && echo "$OUT" | grep -q 'TLSv1.3'; then
        echo "VERIFIED: $NAME serves the private certificate over TLSv1.3, chain OK against the pinned root"
        echo "$OUT" | grep -E 'Verify return code|Protocol|subject=|issuer='
        echo "backup of the previous nginx sites: $BK"
    else
        echo "VERIFY FAILED; restoring the previous nginx sites from $BK" >&2
        restore_from_backup
        nginx -t && systemctl reload nginx
        echo "$OUT" | grep -E 'Verify return code|Protocol' >&2
        exit 70
    fi
    ;;

retire)
    # Never before install has verified.
    openssl s_client -connect 127.0.0.1:443 -servername "$NAME" -tls1_3 -CAfile "$TLS/kbs-root.pem" </dev/null 2>/dev/null \
        | grep -q 'Verify return code: 0 (ok)' || { echo "the private certificate is not what nginx serves; refusing to retire anything" >&2; exit 70; }
    # Prove nothing else serves on port 80 before closing it: the only :80
    # listener may be nginx, and nginx must have no `listen 80` left.
    echo "==> listeners on :80"
    ss -ltnp 'sport = :80' || true
    if ss -ltnp 'sport = :80' | tail -n +2 | grep -v -q 'nginx' && [ -n "$(ss -ltnp 'sport = :80' | tail -n +2)" ]; then
        echo "something other than nginx listens on :80; not closing the port" >&2; exit 70
    fi
    if nginx -T 2>/dev/null | grep -qE '^\s*listen\s+(\[::\]:)?80\b'; then
        echo "nginx still has a listen 80 directive; not closing the port" >&2; exit 70
    fi
    systemctl disable --now certbot.timer 2>/dev/null || true
    certbot delete --cert-name "$NAME" --non-interactive 2>/dev/null || true
    apt-get purge -y certbot python3-certbot-nginx >/dev/null && apt-get autoremove -y >/dev/null
    ufw delete allow 80/tcp || true
    ufw status | grep -E '^(22|80|443)' || true
    echo "RETIRED: certbot removed, port 80 closed in ufw. Remove TCP 80 from the Vultr firewall group (dashboard) to finish."
    ;;
*)
    echo "unknown stage $stage" >&2; exit 64 ;;
esac
