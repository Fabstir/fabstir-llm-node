#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Sign the broker's server CSR with the offline root. No decisions to make.
#
#   deployment/phala/kbs-ca/sign-server-cert.sh [csr]
#
# Default CSR: deployment/phala/kbs-ca/issued/kbs.fabstir.net.csr (the file the
# VPS-side step writes into the repo). Output: kbs.fabstir.net.pem beside it,
# which is public and can be committed; the VPS-side step picks it up from there.
# 5-year leaf, SAN kbs.fabstir.net, serverAuth, CA:FALSE. Refuses a CSR whose
# subject is not CN=kbs.fabstir.net, and refuses to overwrite an existing,
# still-valid issued certificate (re-issue by moving the old one away first).
set -euo pipefail

CA_DIR="${KBS_CA_DIR:-$HOME/fabstir-kbs-ca}"
HERE="$(cd "$(dirname "$0")" && pwd)"
CSR="${1:-$HERE/issued/kbs.fabstir.net.csr}"
OUT="$(dirname "$CSR")/kbs.fabstir.net.pem"
KEY="$CA_DIR/fabstir-kbs-root.key"
PEM="$CA_DIR/fabstir-kbs-root.pem"
DAYS=1826                                 # 5 years
NAME="kbs.fabstir.net"

[ -f "$KEY" ] && [ -f "$PEM" ] || { echo "no root at $CA_DIR; run make-root.sh first" >&2; exit 66; }
[ -f "$CSR" ] || { echo "no CSR at $CSR (the VPS-side step writes it there)" >&2; exit 66; }
if [ -f "$OUT" ] && openssl x509 -in "$OUT" -noout -checkend 0 >/dev/null 2>&1; then
    echo "refusing: $OUT exists and is still valid; move it away first if this is a deliberate re-issue" >&2
    exit 65
fi

subject="$(openssl req -in "$CSR" -noout -subject)"
case "$subject" in
    *"CN = $NAME"*|*"CN=$NAME"*) ;;
    *) echo "refusing: CSR subject is '$subject', expected CN=$NAME" >&2; exit 65 ;;
esac
openssl req -in "$CSR" -noout -verify >/dev/null

EXT="$(mktemp)"
trap 'rm -f "$EXT"' EXIT
printf 'subjectAltName=DNS:%s\nextendedKeyUsage=serverAuth\nkeyUsage=critical,digitalSignature\nbasicConstraints=critical,CA:FALSE\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid\n' "$NAME" > "$EXT"

echo "==> signing $CSR with the offline root, $DAYS days"
openssl x509 -req -in "$CSR" -CA "$PEM" -CAkey "$KEY" -CAcreateserial \
    -days "$DAYS" -sha384 -extfile "$EXT" -out "$OUT" 2>/dev/null
chmod 644 "$OUT"
openssl verify -CAfile "$PEM" "$OUT"

echo
echo "==> DONE. Record the enddate in docs/development/EXECUTION-PHASE5-ATTESTATION.md (Certificate expiries):"
openssl x509 -in "$OUT" -noout -subject -enddate -ext subjectAltName | sed 's/^/    /'
echo
echo "    issued certificate (public; commit it): $OUT"
echo "    the VPS-side step installs it; nothing else is yours."
