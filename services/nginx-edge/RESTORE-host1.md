# RESTORE-host1.md — Rebuilding the FabDiscover geo-fencing host

Disaster-recovery runbook for the **FabDiscover discovery edge + geo trust boundary**
on the host that serves `host1.fabstir.net` (machine: **fabstirserv1**, LAN `192.168.1.136`,
public egress `89.222.16.158`, **IPv4-only — no IPv6**).

This is the host side of v2 territorial geo-fencing. The geo design and the canonical
nginx block live in this repo: see `README.md` ("FabDiscover Edge") and
`host1/fabdiscover-location.conf`. This file is the *operational* rebuild + the gotchas
that aren't obvious from the configs.

> **Validated end-to-end 2026-05-29:** `reconcile:once` → `total:7`; `POST /search`
> returns the CN-excluded title for `X-Geo-Country: FR` but omits it for `CN`.

---

## 0. Cheat-sheet (the exact values)

| Thing | Value |
|---|---|
| Edge app dir | `~/fabstir/fabstir-discovery-edge` (jules23) — **NOT a git checkout**; deployed from tarball |
| Reader (search) | systemd `fabdiscover-search.service` → `node --env-file-if-exists=.env src/run-search.mjs`, binds **`127.0.0.1:7700`** |
| Indexer | systemd `fabdiscover-indexer.service` → `src/run-indexer.mjs` |
| Geo DB | `/usr/share/GeoIP/dbip-country-lite.mmdb` (DB-IP Lite, **not** MaxMind), pointed at by `GEOIP_DB_PATH` in the edge `.env` |
| SDK artifacts | `~/fabstir/fabstir-sdk-deploy.tgz` (`@fabstir/sdk`, territorial), `~/fabstir/fabstir-fabstirdb-deploy.tgz` (`@fabstir/fabstirdb@2.0.0`), `~/fabstir/fabstir-sdk-core-1.23.1.tgz` |
| S5 portal | `s5.platformlessai.ai` / `dl.platformlessai.ai` → **IPv6-only in DNS**; reachable IPv4 = **`95.179.239.163`** (Vultr) |
| nginx server block | `/etc/nginx/sites-enabled/fabstir-host1` (the inline `location /fabdiscover/`) |
| nginx fabcdn | `/etc/nginx/snippets/fabcdn-location.conf` + `/etc/nginx/conf.d/10-fabcdn-map.conf` |
| TLS | `/etc/letsencrypt/live/host1.fabstir.net/` (certbot, ACME webroot via the `:80` server block) |
| S5 download bridge | `s5-bridge` container on `:5522` (used by the transcoder) |

---

## A. Fast restore (from the backup tarball)

If you have `fabstirserv1-backup-<date>.tar.gz(.gpg)` (see "Making the backup" at the end):

```bash
# decrypt if needed
gpg -d fabstirserv1-backup-<date>.tar.gz.gpg > fabstirserv1-backup.tar.gz
# restore files to their absolute locations
sudo tar xzf fabstirserv1-backup.tar.gz -C /
# reload everything
sudo systemctl daemon-reload
sudo nginx -t && sudo systemctl reload nginx
sudo systemctl enable --now fabdiscover-search fabdiscover-indexer
```
Then jump to **§D Verification**. If the backup predates a change (e.g. portal IP moved),
fix the one item and re-verify — don't rebuild from scratch.

---

## B. From-scratch rebuild (no/partial backup)

### B1. Edge app + identity + dependencies
The edge dir is **not git**; the code arrives as a tarball from v2. The `.env` holds the
**private key that derives the S5 identity owning `fabstir-catalogue-live`** — without it
you cannot read the catalogue. Restore `.env` from your secure backup; never regenerate it.

```bash
mkdir -p ~/fabstir/fabstir-discovery-edge && cd ~/fabstir/fabstir-discovery-edge
tar xzf /path/to/fabstir-discovery-edge-geo.tgz   # v2's edge build (excludes node_modules/.env/data)
# restore your saved .env (identity, MODE=real, CORS_ORIGIN incl https://v2.fabstir.io, GEOIP_DB_PATH, portal/contract vars)
cp /secure/backup/edge.env .env
```

**Dependencies — the prune trap (read this):** `@fabstir/sdk` + `@fabstir/sdk-core` are
**optional peerDependencies** and `maxmind` is an **undeclared lazy import**. Any bare
`npm install` / `npm i X` **prunes** them ("removed N packages"). So: do all npm work
first, naming every survivor in ONE command, then hand-extract the `@fabstir` tarballs
LAST (they are root-layout, not `package/`-prefixed — `npm install <tgz>` mangles them).

```bash
npm install                                   # edge's own deps (this prunes the extras — fine, re-added next)
test -f node_modules/libsodium-wrappers/dist/modules-esm/libsodium.mjs && LS="" || LS="libsodium"
npm install maxmind $LS ~/fabstir/fabstir-sdk-core-1.23.1.tgz   # name all non-registry survivors together

rm -rf node_modules/@fabstir/sdk node_modules/@fabstir/fabstirdb
mkdir -p node_modules/@fabstir/sdk node_modules/@fabstir/fabstirdb
tar xzf ~/fabstir/fabstir-sdk-deploy.tgz       -C node_modules/@fabstir/sdk
tar xzf ~/fabstir/fabstir-fabstirdb-deploy.tgz -C node_modules/@fabstir/fabstirdb
# DO NOT run a bare `npm install` after this — it re-prunes the three packages.

# verify (all four present, imports resolve):
ls node_modules/@fabstir/                      # → sdk  sdk-core  fabstirdb
node -e "import('@fabstir/fabstirdb').then(m=>console.log('fabstirdb',typeof m.createFabstirDB))"
node -e "import('@fabstir/sdk').then(m=>console.log('sdk',typeof m.createChainSDK))"   # both → function
```

### B2. Geo database (DB-IP Lite — no account needed)
```bash
cd /tmp
curl -fSLO https://download.db-ip.com/free/dbip-country-lite-$(date +%Y-%m).mmdb.gz   # or last month if 404
gunzip -f dbip-country-lite-*.mmdb.gz
sudo install -D -m644 dbip-country-lite-*.mmdb /usr/share/GeoIP/dbip-country-lite.mmdb
# ensure the edge .env has:  GEOIP_DB_PATH=/usr/share/GeoIP/dbip-country-lite.mmdb
```

### B3. nginx geo trust boundary
host1 carries an **inline** `location /fabdiscover/` in its server block. Deploy is a
**REPLACE of that block's body**, never an added `include` (a 2nd `location /fabdiscover/`
= nginx "duplicate location"). Source of truth: `host1/fabdiscover-location.conf` in this repo.
```bash
sudo nginx -T | awk '/# configuration file/{f=$0} /location \/fabdiscover\//{print f; exit}'  # find the file
# in that file, the /fabdiscover/ block must contain (the 3 strips are the security delta):
#   proxy_pass http://127.0.0.1:7700/;   proxy_http_version 1.1;   proxy_read_timeout 30s;
#   proxy_set_header Host $host;          proxy_set_header X-Forwarded-Proto $scheme;
#   proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
#   proxy_set_header X-Geo-Country "";    proxy_set_header CF-IPCountry "";    proxy_set_header CloudFront-Viewer-Country "";
# (NO CORS here — the reader sets ACAO via CORS_ORIGIN; double-ACAO breaks browsers.)
sudo nginx -t && sudo systemctl reload nginx
```

### B4. /etc/hosts portal pin (CRITICAL on this IPv4-only box)
The S5 portal is IPv6-only in DNS; this box has no IPv6. Pin the portal's **IPv4** so both
the host processes and nginx reach it (the DNS A record may exist but resolution has flapped):
```bash
sudo sed -i '/platformlessai\.ai/d' /etc/hosts
echo '95.179.239.163 s5.platformlessai.ai dl.platformlessai.ai' | sudo tee -a /etc/hosts
```

### B5. systemd units
Restore `/etc/systemd/system/fabdiscover-search.service` + `fabdiscover-indexer.service`
(placeholders: `User=jules23`, `WorkingDirectory=/home/jules23/fabstir/fabstir-discovery-edge`;
they load `.env` via `--env-file-if-exists`, so do NOT add `EnvironmentFile=`).
```bash
sudo systemctl daemon-reload
sudo systemctl enable --now fabdiscover-search fabdiscover-indexer
```

### B6. TLS
certbot with the ACME webroot in the `:80` server block (`/.well-known/acme-challenge/`
→ `/var/www/html`). `sudo certbot renew --dry-run` must pass. nginx owns `:80` — if a
leftover **Caddy** squats it on reboot, `sudo systemctl disable --now caddy` then restart nginx.

### B7. Start + index + verify → §D.
```bash
cd ~/fabstir/fabstir-discovery-edge
rm -f data/index-state.json
MODE=real CATALOGUE=real npm run reconcile:once     # expect total > 0
```

---

## C. Gotchas (hard-won — each cost real time)

1. **`npm install` prunes the SDK.** `@fabstir/sdk`/`sdk-core` are optional peers and
   `maxmind` is an undeclared lazy import; any reify removes them. Assemble in one shot,
   hand-extract `@fabstir`, then never bare-`npm install` again. (See B1.)
2. **The `@fabstir/sdk` deploy tgz is not self-contained** — it imports `@fabstir/fabstirdb`
   (ship + extract it too) and is root-layout, so extract by hand, don't `npm install` it.
3. **Portal is IPv6-only; this box is IPv4-only.** Pin the portal **IPv4** in `/etc/hosts`
   (`95.179.239.163`). Pinning the IPv6 guarantees failure. Symptom of getting this wrong:
   reader crash-loops on `S5 client creation timed out`; the new SDK build **hard-requires**
   S5 (the old one degraded to localStorage and stayed up).
4. **Long-running containers cache DNS.** After any portal IP / `/etc/hosts` / DNS change,
   **restart the containers that talk to S5** — especially **`s5-bridge`** (`sudo docker
   restart s5-bridge`). Stale DNS here = transcoder source downloads time out → ffmpeg
   "Invalid data" / 0-second source. The reader (host process) re-resolves on `systemctl restart`.
5. **nginx FabCDN resolves its upstream at startup.** `proxy_pass https://s5.platformlessai.ai/`
   means a DNS blip during an nginx restart → `[emerg] host not found in upstream` → **all of
   nginx fails to start** (full host1 outage). The `/etc/hosts` pin (B4) prevents this. Proper
   fix (TODO): `resolver` + variable `proxy_pass` in `fabcdn-location.conf`.
6. **The reader takes ~90s to answer** after restart (S5 identity recovery + portal connect
   run first). An empty `/health` right after a restart is just timing — wait, then re-curl.
7. **`/search` body field is `q`** (not `query`).
8. **`geo:maxmind` must show in `/health`.** If it says `headers-only`, `maxmind` didn't load
   or `GEOIP_DB_PATH` is wrong — it **fails open silently** (everything shows).
9. **`XX` (unknown country) = permissive.** Misconfiguration fails OPEN; a green `/health` is
   NOT proof — only an actually-filtered title proves the gate works.

---

## D. Verification suite (prove a good restore)

```bash
# 1. reader up + geo engine live + S5 persistence working
curl -s http://127.0.0.1:7700/health ; echo                       # ..."geo":"maxmind"...  (NOT headers-only/disabled)
sudo journalctl -u fabdiscover-search --since '3 min ago' | grep -i 'persistence unavailable' \
  && echo "BAD: S5 degraded" || echo "OK: S5 persistence working"

# 2. resolver echoes header when hit directly
curl -s -H 'X-Geo-Country: CN' http://127.0.0.1:7700/geo ; echo   # {"country":"CN"}

# 3. nginx strip is live (spoofed header dropped) — local, bypasses NAT hairpin
curl -s --resolve host1.fabstir.net:443:127.0.0.1 -H 'X-Geo-Country: FR' \
     https://host1.fabstir.net/fabdiscover/geo ; echo             # NEVER "FR" → "XX" or real country

# 4. catalogue indexes + geo filter (the real E2E)
cd ~/fabstir/fabstir-discovery-edge && rm -f data/index-state.json
MODE=real CATALOGUE=real npm run reconcile:once                   # total > 0
curl -s -XPOST http://127.0.0.1:7700/search -H 'content-type: application/json' \
     -H 'X-Geo-Country: CN' -d '{"q":"<CN-excluded title>"}' ; echo   # title ABSENT
curl -s -XPOST http://127.0.0.1:7700/search -H 'content-type: application/json' \
     -H 'X-Geo-Country: FR' -d '{"q":"<CN-excluded title>"}' ; echo   # title PRESENT
```
Green = the title appears for FR but not CN, all other titles in both.

---

## E. After a reboot or a `systemd-networkd` restart on this box

This box also runs Docker (transcoder, s5-bridge, LLM node) and Calico. A network-service
restart or reboot can scramble container/cluster networking and drop DNS state:
- Re-check `ip route` / containers come back; `sudo systemctl restart docker` if container
  networking is scrambled.
- **Restart `s5-bridge`** so it re-resolves the portal (gotcha #4).
- The `/etc/hosts` pin (B4) and DNS A record persist a reboot, so the reader/nginx recover on their own.

---

## Making the backup (pair with this runbook)

```bash
sudo tar czf /tmp/fabstirserv1-backup-$(date +%F).tar.gz -h --ignore-failed-read -C / \
  home/jules23/fabstir/fabstir-discovery-edge \
  home/jules23/fabstir/fabstir-sdk-deploy.tgz \
  home/jules23/fabstir/fabstir-fabstirdb-deploy.tgz \
  home/jules23/fabstir/fabstir-sdk-core-1.23.1.tgz \
  etc/nginx etc/hosts etc/letsencrypt usr/share/GeoIP \
  etc/systemd/system/fabdiscover-search.service \
  etc/systemd/system/fabdiscover-indexer.service
  # + any docker-compose dirs/.env for transcoder / s5-bridge / llm-node (find via:
  #   sudo docker inspect $(sudo docker ps -q) --format '{{index .Config.Labels "com.docker.compose.project.working_dir"}}' | sort -u )
gpg -c /tmp/fabstirserv1-backup-$(date +%F).tar.gz          # encrypt (contains the private key + TLS keys)
# then copy the .gpg OFF the box (scp to another machine + a cloud copy)
```
Contains secrets (edge `.env` private key, TLS keys) — keep it encrypted and off-LAN.
```
