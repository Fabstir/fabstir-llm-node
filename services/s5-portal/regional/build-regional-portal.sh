#!/usr/bin/env bash
# Build an isolated regional S5 portal on a fresh Ubuntu 24.04 box.
#
#   Layout:  Caddy (443) -> S5 node (/s5/p2p) | Garage read-only web (/s5-data/*) | auth proxy (the rest)
#   Store:   Garage on this box's own disk (STORAGE=garage). External S3 is NOT implemented yet (see below).
#   Network: no S5 peers, node does not advertise itself, only 22/80/443 open.
#
# STATUS: written 2026-11-21 from the hand build of s5-uk (192.248.146.118), step for step.
#         NOT YET RUN END TO END. Its first real run is the next regional box; read it through before that run.
#
# Usage (on the new box, as root, from the copied kit folder):
#   bash build-regional-portal.sh <region> <domain> [garage-capacity]
#   e.g. bash build-regional-portal.sh uk s5-uk.platformlessai.ai 50G
#
# Before running: DNS for <domain> must already point at this box (Caddy needs it for the certificate).
# Safe to re-run: it never regenerates a secret or a config file that already exists.
set -euo pipefail

REGION="${1:?region label, e.g. uk or eu}"
DOMAIN="${2:?public domain, e.g. s5-uk.platformlessai.ai}"
CAPACITY="${3:-50G}"
STORAGE="${STORAGE:-garage}"
KIT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROXY_SRC="$(cd "$KIT/../proxy" 2>/dev/null && pwd || echo "$KIT/../proxy")"   # the proxy lives in ../proxy/ (copy the whole s5-portal folder to the box, not just this one)

GARAGE_VERSION="v2.5.0"
GARAGE_SHA256="2ff667286217cd73ebcb1c03af993822365000441dfe31a80cb64c5189a9dea8"   # x86_64-unknown-linux-musl, as installed on s5-uk
S5_IMAGE="ghcr.io/s5-dev/node@sha256:8cd186e9fe1765d2d9b20cbe045d520756a0c9f1d5c780bbe14c5385f23731ff"   # reports 0.14.1; the build the Global portal runs
RPC_URL="${BASE_SEPOLIA_RPC_URL:-https://base-sepolia-rpc.publicnode.com}"
BUCKET="s5-data"
WEB_HOST="${BUCKET}.web.s5-${REGION}.localhost"

say() { printf '\n== %s\n' "$*"; }
# has PATTERN CMD...: true when CMD's output contains PATTERN. Captures the output first, because
# "cmd | grep -q" under pipefail fails whenever grep exits early and the writer gets a broken pipe.
has() { local pat="$1" out; shift; out="$("$@" 2>&1 || true)"; grep -q -- "$pat" <<<"$out"; }
[ "$(id -u)" = 0 ] || { echo "run as root"; exit 1; }
[[ "$REGION" =~ ^[a-z]{2,8}$ ]] || { echo "region must be a short lower-case label"; exit 1; }
if [ "$STORAGE" != garage ]; then
  echo "STORAGE=$STORAGE is not implemented. An external S3 service needs a decision on the download path"
  echo "(public-read bucket or presigned links) and on how the auth proxy checks that a blob exists. Stop."
  exit 1
fi
[ -f "$PROXY_SRC/server.js" ] || { echo "kit is incomplete: $PROXY_SRC/server.js missing"; exit 1; }

say "1. Harden: key-only SSH, firewall, security updates"
cat > /etc/ssh/sshd_config.d/10-fabstir-hardening.conf <<EOF
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin prohibit-password
EOF
sshd -t && systemctl reload ssh
ufw allow 22/tcp >/dev/null; ufw allow 80/tcp >/dev/null; ufw allow 443/tcp >/dev/null
ufw --force enable >/dev/null
systemctl is-enabled unattended-upgrades >/dev/null 2>&1 || echo "WARNING: unattended-upgrades is not enabled"

say "2. Packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
# --force-confold: Vultr's image carries a modified cloud.cfg and apt would otherwise stop at a prompt.
apt-get install -y -qq -o Dpkg::Options::="--force-confold" docker.io caddy nodejs npm sqlite3 curl openssl >/dev/null

say "3. Garage ($GARAGE_VERSION), on this box only"
if [ ! -x /usr/local/bin/garage ]; then
  curl -fsSL -o /usr/local/bin/garage.new "https://garagehq.deuxfleurs.fr/_releases/${GARAGE_VERSION}/x86_64-unknown-linux-musl/garage"
  echo "${GARAGE_SHA256}  /usr/local/bin/garage.new" | sha256sum -c - >/dev/null || { echo "Garage checksum mismatch"; rm -f /usr/local/bin/garage.new; exit 1; }
  mv /usr/local/bin/garage.new /usr/local/bin/garage; chmod 755 /usr/local/bin/garage
fi
id garage >/dev/null 2>&1 || useradd --system --home /var/lib/garage --shell /usr/sbin/nologin garage
mkdir -p /var/lib/garage/meta /var/lib/garage/data
chown -R garage:garage /var/lib/garage; chmod 750 /var/lib/garage
if [ ! -f /etc/garage.toml ]; then
  ( umask 077; cat > /etc/garage.toml <<EOF
metadata_dir = "/var/lib/garage/meta"
data_dir = "/var/lib/garage/data"
db_engine = "sqlite"

replication_factor = 1

rpc_bind_addr = "127.0.0.1:3901"
rpc_public_addr = "127.0.0.1:3901"
rpc_secret = "$(openssl rand -hex 32)"

[s3_api]
# Signing label only, NOT a location: the S5 node's S3 client cannot be told a region and signs as us-east-1.
s3_region = "us-east-1"
api_bind_addr = "127.0.0.1:3900"
root_domain = ".s3.s5-${REGION}.localhost"

[s3_web]
bind_addr = "127.0.0.1:3902"
root_domain = ".web.s5-${REGION}.localhost"

[admin]
api_bind_addr = "127.0.0.1:3903"
admin_token = "$(openssl rand -base64 32)"
metrics_token = "$(openssl rand -base64 32)"
EOF
  )
fi
chown root:garage /etc/garage.toml; chmod 640 /etc/garage.toml
install -m 644 "$KIT/garage.service" /etc/systemd/system/garage.service
systemctl daemon-reload; systemctl enable --now garage >/dev/null 2>&1; sleep 4
G="garage -c /etc/garage.toml"
if has "NO ROLE ASSIGNED" $G status; then
  NODE=$($G status | awk -v h="$(hostname)" '$2==h {print $1; exit}')
  [ -n "$NODE" ] || { echo "could not find this node in garage status"; exit 1; }
  $G layout assign -z "$REGION" -c "$CAPACITY" "$NODE" >/dev/null
  $G layout apply --version 1 >/dev/null; sleep 2
fi
$G bucket info "$BUCKET" >/dev/null 2>&1 || $G bucket create "$BUCKET" >/dev/null
mkdir -p /etc/s5; chmod 755 /etc/s5
if [ ! -f /etc/s5/s3.env ]; then
  ( umask 077
    $G key create s5-node > /root/.s5-node-key.txt
    AK=$(awk '/Key ID/ {print $NF}' /root/.s5-node-key.txt); SK=$(awk '/Secret key/ {print $NF}' /root/.s5-node-key.txt)
    shred -u /root/.s5-node-key.txt
    [ -n "$AK" ] && [ -n "$SK" ] || { echo "could not read the new Garage key"; exit 1; }
    printf 'S3_ENDPOINT=http://127.0.0.1:3900\nS3_ACCESS_KEY=%s\nS3_SECRET_KEY=%s\nS3_BUCKET=%s\nS3_REGION=us-east-1\n' "$AK" "$SK" "$BUCKET" > /etc/s5/s3.env )
  $G bucket allow --read --write --owner "$BUCKET" --key s5-node >/dev/null
fi
$G bucket website --allow "$BUCKET" >/dev/null     # anonymous READ on 127.0.0.1:3902 only; Caddy fronts it

say "4. S5 node (no peers, not advertised)"
mkdir -p /opt/s5/config /opt/s5/db /opt/s5/cache
if [ ! -f /etc/s5/registration.env ]; then
  ( umask 077; echo "S5_REGISTRATION_TOKEN=$(openssl rand -hex 24)" > /etc/s5/registration.env )
fi
if [ ! -f /opt/s5/config/config.toml ]; then
  ( umask 077; . /etc/s5/s3.env; . /etc/s5/registration.env
    # The node generates a seed only for its OWN default file, so make one here.
    SEED=$(openssl rand 32 | basenc --base64url | tr -d '=\n')
    cat > /opt/s5/config/config.toml <<EOF
name = "s5-${REGION}"

[keypair]
seed = "${SEED}"

[cache]
path = "/cache"
maxSizeInGB = 4

[database]
path = "/db"

[http.api]
port = 5050
bind = "127.0.0.1"
domain = "${DOMAIN}"

[p2p.self]
disabled = true

[p2p.peers]
initial = []

[store]
default = "s3"

[store.s3]
endpointUrl = "${S3_ENDPOINT}"
accessKey = "${S3_ACCESS_KEY}"
secretKey = "${S3_SECRET_KEY}"
bucket = "${S3_BUCKET}"
cdnUrls = ["https://${DOMAIN}/s5-data/"]

[accounts]
enabled = true
authTokensForAccountRegistration = ["${S5_REGISTRATION_TOKEN}"]

[accounts.database]
path = "/db"

[http.api.delete]
enabled = true
EOF
  )
fi
if ! has '^s5-node$' docker ps -a --format '{{.Names}}'; then
  docker run -d --name s5-node --restart unless-stopped --network host -e DOCKER=TRUE \
    -v /opt/s5/config:/config -v /opt/s5/db:/db -v /opt/s5/cache:/cache "$S5_IMAGE" >/dev/null
fi
for _ in $(seq 1 40); do curl -s -m 2 http://127.0.0.1:5050/s5/version >/dev/null && break; sleep 1; done
has '"node"' curl -s -m 5 http://127.0.0.1:5050/s5/version || { echo "S5 node did not come up; see: docker logs s5-node"; exit 1; }

say "5. Auth proxy (the Global portal's proxy, site values from /etc/s5/proxy.env)"
mkdir -p /opt/s5-proxy
install -m 644 "$PROXY_SRC/server.js" "$PROXY_SRC/package.json" "$PROXY_SRC/package-lock.json" /opt/s5-proxy/
( cd /opt/s5-proxy && npm ci --omit=dev --silent )
( umask 077; . /etc/s5/registration.env
  [ -f /root/.s5-master-token ] || printf '%s' "$S5_REGISTRATION_TOKEN" > /root/.s5-master-token
  [ -f /etc/s5/proxy.env ] || cat > /etc/s5/proxy.env <<EOF
BASE_SEPOLIA_RPC_URL=${RPC_URL}
S5_PROXY_STORAGE_BASE=http://127.0.0.1:3902/
S5_PROXY_STORAGE_HOST=${WEB_HOST}
S5_PROXY_CDN_BASE=https://${DOMAIN}/s5-data/
S5_PROXY_BIND=127.0.0.1
EOF
)
install -m 644 "$KIT/s5-proxy.service" /etc/systemd/system/s5-proxy.service
systemctl daemon-reload; systemctl enable s5-proxy >/dev/null 2>&1; systemctl restart s5-proxy; sleep 2
has '"node"' curl -s -m 5 http://127.0.0.1:5051/s5/version || { echo "auth proxy did not come up; see: journalctl -u s5-proxy"; exit 1; }

say "6. Caddy and certificate"
sed -e "s/s5-uk\.platformlessai\.ai/${DOMAIN}/g" -e "s/s5-data\.web\.s5-uk\.localhost/${WEB_HOST}/g" "$KIT/Caddyfile.uk" > /etc/caddy/Caddyfile
caddy validate --config /etc/caddy/Caddyfile >/dev/null 2>&1 || { echo "Caddyfile did not validate"; exit 1; }
systemctl enable caddy >/dev/null 2>&1; systemctl restart caddy

say "7. Checks"
OK=1
for _ in $(seq 1 30); do has '"node"' curl -s -m 5 "https://${DOMAIN}/s5/version" && break; sleep 2; done
has '"node"' curl -s -m 5 "https://${DOMAIN}/s5/version" && echo "PASS public /s5/version" || { echo "FAIL public /s5/version (DNS or certificate?)"; OK=0; }
[ "$(curl -s -m 10 -o /dev/null -w '%{http_code}' "https://${DOMAIN}/s5-data/1/does-not-exist")" = 404 ] && echo "PASS download route answers 404 for a missing blob" || { echo "FAIL download route"; OK=0; }
PUBLIC=$(ss -ltnH | awk '{print $4}' | grep -v '^127\.' | grep -v '^\[::1\]' | sed -E 's/.*:([0-9]+)$/\1/' | sort -un | tr '\n' ' ')
[ "$PUBLIC" = "22 80 443 " ] && echo "PASS only 22, 80, 443 listen beyond this box" || { echo "FAIL listening beyond this box: $PUBLIC"; OK=0; }
has 'connection uris: \[\]' docker logs s5-node && echo "PASS node advertises no address" || { echo "FAIL node advertises an address"; OK=0; }
# Through the proxy: with a domain set the node answers this path only for a Host it accepts.
NODE_ID=$(curl -s -m 5 http://127.0.0.1:5051/s5/p2p/nodes | sed -nE 's/.*"id":"([^"]+)".*/\1/p')
[ -n "$NODE_ID" ] || { echo "FAIL could not read the node id"; OK=0; }
echo
echo "Portal URL : https://${DOMAIN}"
echo "Peer string: wss://${NODE_ID}@${DOMAIN}/s5/p2p"
echo "Registration token: /etc/s5/registration.env on this box (never print it into a chat or a ticket)"
[ "$OK" = 1 ] && echo "BUILD CHECKS PASSED" || { echo "BUILD CHECKS FAILED"; exit 1; }
