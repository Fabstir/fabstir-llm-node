#!/usr/bin/env bash
# Install or update fabstir-kbs on the broker VPS (design §13). Idempotent. Run as root.
#
#   ./install.sh <path/to/fabstir-kbs> <path/to/libcuda-stub.so>
#
# The stub is registered system-wide (/etc/ld.so.conf.d + ldconfig) so the tooling
# also runs as the service user outside the unit.
#
# Asserts before enabling anything: nginx already proxies /v1/kbs/, the http{} zone
# snippet is in place and `nginx -t` passes, NTP is synchronised (and
# systemd-time-wait-sync is enabled), ISRG Root YE is in the system store, and
# `ldd` resolves libcuda.so.1 under the unit's LD_LIBRARY_PATH with nothing "not found".
set -euo pipefail

BIN_SRC="${1:?fabstir-kbs binary}"
STUB_SRC="${2:?libcuda stub (/usr/local/cuda/lib64/stubs/libcuda.so on the build box)}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
USER_NAME=fabstir-kbs
DATA=/var/lib/fabstir-kbs
LIB=/opt/fabstir-kbs/lib

fail() { echo "install.sh: $*" >&2; exit 1; }

[ "$(id -u)" = 0 ] || fail "run as root"
# Zones first (the site file references them), then the site must already be there:
# deployment/phala/kbs-ca/vps-install.sh installs both; this re-installs the zones so
# either script can run first without a circular nginx -t failure.
install -d -m 0755 /etc/nginx/conf.d
install -m 0644 "$HERE/kbs-limits.conf" /etc/nginx/conf.d/kbs-limits.conf
grep -qs 'location /v1/kbs/' /etc/nginx/sites-enabled/* /etc/nginx/conf.d/* 2>/dev/null \
  || fail "nginx does not proxy /v1/kbs/ yet (deployment/phala/kbs-ca/vps-install.sh install); run that first"

# user + directory tree (design §2 modes: data dir 0711 so www-data can traverse to public/)
id "$USER_NAME" >/dev/null 2>&1 || useradd --system --home "$DATA" --shell /usr/sbin/nologin "$USER_NAME"
install -d -m 0711 -o "$USER_NAME" -g "$USER_NAME" "$DATA"
install -d -m 0700 -o "$USER_NAME" -g "$USER_NAME" "$DATA/memo" "$DATA/capture"
install -d -m 0755 -o "$USER_NAME" -g "$USER_NAME" "$DATA/public" "$DATA/public/policies" "$DATA/public/blobs"
install -d -m 0750 -o root -g "$USER_NAME" /etc/fabstir-kbs
[ -f /etc/fabstir-kbs/env ] || install -m 0640 -o root -g "$USER_NAME" "$HERE/env.example" /etc/fabstir-kbs/env

# binary + the driver stub
install -m 0755 "$BIN_SRC" /usr/local/bin/fabstir-kbs
install -d -m 0755 "$LIB"
install -m 0644 "$STUB_SRC" "$LIB/libcuda.so.1"
# System-wide, not only the unit's LD_LIBRARY_PATH: the tooling (`keyring add`,
# `policy sign`, `seal`, `reseal`) runs as `sudo -u fabstir-kbs`, and sudo strips
# LD_LIBRARY_PATH.
echo "$LIB" > /etc/ld.so.conf.d/fabstir-kbs.conf
ldconfig
if ldd /usr/local/bin/fabstir-kbs | grep -q 'not found'; then
  ldd /usr/local/bin/fabstir-kbs | grep 'not found' >&2
  fail "unresolved shared libraries (libssl3? the stub?)"
fi
ldd /usr/local/bin/fabstir-kbs | grep -q "libcuda.so.1 => $LIB/libcuda.so.1" \
  || fail "libcuda.so.1 does not resolve to the stub"
sudo -u "$USER_NAME" /usr/local/bin/fabstir-kbs --version >/dev/null \
  || fail "the binary does not start as $USER_NAME (ldconfig?)"

# clock (design D15)
systemctl enable --now systemd-time-wait-sync.service >/dev/null 2>&1 || true
[ "$(timedatectl show -p NTPSynchronized --value)" = "yes" ] || fail "NTP is not synchronised (timedatectl)"

# ISRG Root YE in the system store (pccs.phala.network is issued by Let's Encrypt YE1).
# The Debian bundle carries no subject text, so decode it rather than grep the file.
openssl crl2pkcs7 -nocrl -certfile /etc/ssl/certs/ca-certificates.crt 2>/dev/null \
  | openssl pkcs7 -print_certs -noout 2>/dev/null | grep -q 'ISRG Root YE' \
  || fail "ISRG Root YE missing from the system store (update-ca-certificates with root-ye.pem)"

# nginx check + unit
nginx -t || fail "nginx -t failed with kbs-limits.conf in place"
install -m 0644 "$HERE/fabstir-kbs.service" /etc/systemd/system/fabstir-kbs.service
systemctl daemon-reload
systemctl enable fabstir-kbs >/dev/null
systemctl reload nginx

echo "install.sh: done. Next: keyring (as $USER_NAME: sudo -u $USER_NAME fabstir-kbs keyring add ...),"
echo "policies in $DATA/public/policies/, then: systemctl restart fabstir-kbs && journalctl -u fabstir-kbs -f"
echo "If it refuses (exit 78) fix the cause, then: systemctl reset-failed fabstir-kbs && systemctl start fabstir-kbs"
