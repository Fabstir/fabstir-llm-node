#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Generate the Fabstir KBS private root CA, OFFLINE, with no decisions to make.
#
#   deployment/phala/kbs-ca/make-root.sh
#
# Writes the ROOT PRIVATE KEY to ~/fabstir-kbs-ca/fabstir-kbs-root.key (never
# in the repo, never on the VPS), the public root certificate to the same
# directory AND to deployment/phala/kbs-root.pem (the copy you commit), and
# prints the expiry date, the fingerprint and the key's location. Refuses to
# overwrite an existing root: a second root would orphan every node image
# pinned to the first.
set -euo pipefail

CA_DIR="${KBS_CA_DIR:-$HOME/fabstir-kbs-ca}"
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_PEM="$HERE/../kbs-root.pem"          # deployment/phala/kbs-root.pem
KEY="$CA_DIR/fabstir-kbs-root.key"
PEM="$CA_DIR/fabstir-kbs-root.pem"
DAYS=3650                                 # 10 years

if [ -e "$KEY" ] || [ -e "$PEM" ]; then
    echo "refusing: a root already exists at $CA_DIR (key or pem present)." >&2
    echo "Every node image pins THIS root; a second one would orphan them. Move it away deliberately if you really mean to re-root." >&2
    exit 65
fi

umask 077
mkdir -p "$CA_DIR"
chmod 700 "$CA_DIR"

echo "==> root key (P-384), offline: $KEY"
openssl ecparam -name secp384r1 -genkey -noout -out "$KEY"
chmod 600 "$KEY"

echo "==> self-signed root certificate, $DAYS days"
openssl req -x509 -new -key "$KEY" -sha384 -days "$DAYS" \
    -subj "/O=Fabstir/CN=Fabstir KBS Root CA" \
    -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -addext "subjectKeyIdentifier=hash" \
    -out "$PEM"
chmod 644 "$PEM"

cp "$PEM" "$REPO_PEM"
chmod 644 "$REPO_PEM"

echo
echo "==> DONE. Record these in docs/development/EXECUTION-PHASE5-ATTESTATION.md (Certificate expiries):"
openssl x509 -in "$PEM" -noout -subject -startdate -enddate -fingerprint -sha256 | sed 's/^/    /'
echo
echo "    root PRIVATE KEY (keep offline + one backup; needed again only to sign a server cert): $KEY"
echo "    public root cert (also copied into the repo):                                       $PEM"
echo "    commit this file:                                                                  $(cd "$(dirname "$REPO_PEM")" && pwd)/kbs-root.pem"
echo
echo "    next: wait for the CSR at deployment/phala/kbs-ca/issued/kbs.fabstir.net.csr, then run sign-server-cert.sh"
