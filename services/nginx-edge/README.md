# nginx-edge (FabCDN Edge)

Transparent nginx reverse-proxy of `s5.platformlessai.ai` that caches
`GET /s5/blob/{cid}` with `X-Cache` + CORS headers. Deployed as configuration
drop-ins on the existing host-level nginx at `host2.fabstir.net` for the
Wed 2026-04-29 Fabstir-v2 demo. Edge URL: `https://host2.fabstir.net/fabcdn/`.

The same snippets under `host2/` drive both the production deploy
(`/etc/nginx/conf.d/` + `/etc/nginx/snippets/`) and the local mock under
`mock/` (`docker compose up`). Single source of truth — no "copy to prod
later" round trip.

## Architecture

```
  browser
     │ GET/HEAD https://host2.fabstir.net/fabcdn/s5/blob/{cid}
     ▼
  nginx @ host2 (server_name host2.fabstir.net, TLS)
     │ location /fabcdn/
     ▼
  proxy_cache fabcdn ──► /var/cache/fabcdn   (50 GB LRU, 30d inactive)
     │ cache MISS
     ▼
  proxy_pass https://s5.platformlessai.ai/   (upstream S5 portal)
     │
     ▼
  cipherbytes streamed back → response rewritten with:
     X-Cache: HIT | MISS | BYPASS | EXPIRED
     X-Origin: s5.platformlessai.ai
     Access-Control-Allow-Origin: *
     Access-Control-Expose-Headers: X-Cache, X-Origin, ETag, Content-Length
```

### HEAD-no-write design

The fabstir-v2 client probes cache status with a HEAD before its GET. If
that HEAD populated the cache with an empty body, the GET would hit an
empty entry. Two maps fix this:

- `$fabcdn_is_blob` — 1 only for URIs matching `^/fabcdn/s5/blob/`.
- `$fabcdn_no_write` — 0 only when `(is_blob=1, is_head=0)`; otherwise 1.
  Fed into `proxy_no_cache`, so HEAD requests *read* the cache (report HIT
  when warm) but never *write* it. Writes are exclusively blob-path GETs.

## Local Mock

```bash
cd services/nginx-edge
docker compose up -d                      # http://localhost:8090/fabcdn/
bash tests/smoke-test.sh                  # end-to-end check, all green
MODE=mock ./cache-flush.sh                # wipe the cache + reload nginx
MODE=mock ./cache-stats.sh                # size + file count
```

The mock is what the fabstir-v2 developer points their dev build at before
the Monday prod cutover:

```
NEXT_PUBLIC_ENABLE_FABCDN=true
NEXT_PUBLIC_FABCDN_EDGE_URL=http://localhost:8090/fabcdn
```

Localhost is a secure context in Chromium — a HEAD-probe from an HTTP
dev page won't be blocked as mixed content, so HTTPS isn't required.

## Production Deploy

Target: `host2.fabstir.net`. Prereqs: SSH + sudo as an operator user.
All commands run on the box unless marked "(local)".

### 1. Stage files on the box

```bash
# (local) from services/nginx-edge/ — replace $USER if login on host2 differs
ssh "$USER"@host2.fabstir.net 'mkdir -p /tmp/fabcdn-deploy'
scp host2/10-fabcdn-map.conf host2/fabcdn-location.conf \
    "$USER"@host2.fabstir.net:/tmp/fabcdn-deploy/
```

### 2. Create the cache dir

```bash
sudo mkdir -p /var/cache/fabcdn
sudo chown -R www-data:www-data /var/cache/fabcdn
sudo chmod 755 /var/cache/fabcdn
ls -ld /var/cache/fabcdn          # drwxr-xr-x www-data www-data
```

### 3. Drop the snippets into place

```bash
sudo mkdir -p /etc/nginx/snippets          # defensive
sudo cp /tmp/fabcdn-deploy/10-fabcdn-map.conf /etc/nginx/conf.d/
sudo cp /tmp/fabcdn-deploy/fabcdn-location.conf /etc/nginx/snippets/
```

### 4. Wire the location into the existing server block

Find the server block:

```bash
sudo grep -rl "server_name host2.fabstir.net" /etc/nginx/
# typical result: /etc/nginx/sites-enabled/default
```

Inspect the block (sanity check, and look for any regex `location ~ …`
that could preempt prefix routing for `/fabcdn/`):

```bash
sudo grep -nA 40 'server_name host2\.fabstir\.net' /etc/nginx/sites-enabled/default
```

Edit that file and add **one** line inside the `server { }` block (near
the existing `location /` is the readable spot; nginx matches by
longest-prefix, not file order):

```nginx
include /etc/nginx/snippets/fabcdn-location.conf;
```

### 5. Validate + reload

```bash
sudo nginx -t                     # must report "syntax is ok"
sudo nginx -s reload
sudo tail -n 20 /var/log/nginx/error.log
```

Watch `error.log` while running the first verification curl in the next
section. Any `[emerg]` / `[error]` abort the deploy — see Rollback.

## Verification (Post-Deploy)

Run these from a **separate machine** (not host2) so the TLS + routing
path is exercised end-to-end. Replace `<CID>` with any known-public
blob CID — ask the fabstir-v2 team for one tied to the demo film, or
copy one from devtools while Viewer 1 is playing.

### 1. Cold MISS on a fresh CID

```bash
curl -sI https://host2.fabstir.net/fabcdn/s5/blob/<CID> | grep -i x-cache
# X-Cache: MISS
```

### 2. Warm HIT on replay

```bash
curl -sI https://host2.fabstir.net/fabcdn/s5/blob/<CID> | grep -i x-cache
# X-Cache: HIT
```

### 3. HEAD does not warm the cache

```bash
# flush first (from an ssh session on host2):
ssh host2.fabstir.net 'sudo find /var/cache/fabcdn -mindepth 1 -delete && sudo nginx -s reload'

# two HEADs against a fresh CID — both MISS:
curl -sI https://host2.fabstir.net/fabcdn/s5/blob/<CID> | grep -i x-cache   # MISS
curl -sI https://host2.fabstir.net/fabcdn/s5/blob/<CID> | grep -i x-cache   # MISS (still)
```

### 4. HEAD after a warm GET shows HIT

```bash
curl -s  https://host2.fabstir.net/fabcdn/s5/blob/<CID> -o /dev/null        # GET warms
curl -sI https://host2.fabstir.net/fabcdn/s5/blob/<CID> | grep -i x-cache   # HIT
```

### 5. Non-blob path is BYPASS (transparent pass-through)

```bash
curl -sI https://host2.fabstir.net/fabcdn/s5/account/register | grep -i x-cache
# X-Cache: BYPASS
```

### 6. CORS headers present

```bash
curl -sI -H "Origin: http://localhost:3000" \
    https://host2.fabstir.net/fabcdn/s5/blob/<CID> \
    | grep -iE 'access-control'
# Access-Control-Allow-Origin: *
# Access-Control-Allow-Methods: GET, HEAD, OPTIONS
# Access-Control-Expose-Headers: X-Cache, X-Origin, ETag, Content-Length
```

## Rollback

Non-destructive: the edge is a single `include` line — comment it out and
reload nginx. Cache dir + snippets can stay in place (they do nothing
without the include).

```bash
# Edit the server block file (path from the deploy step):
sudo sed -i 's|^\s*include /etc/nginx/snippets/fabcdn-location.conf;|# &|' \
    /etc/nginx/sites-enabled/default
sudo nginx -t && sudo nginx -s reload
# Confirm the edge is dead: should 404 now (location gone).
curl -sI https://host2.fabstir.net/fabcdn/s5/blob/<CID> | head -1
```

Optional full removal:

```bash
sudo rm /etc/nginx/conf.d/10-fabcdn-map.conf
sudo rm /etc/nginx/snippets/fabcdn-location.conf
sudo rm -rf /var/cache/fabcdn       # only if you really want to lose the warm cache
```

## Rehearsal Flush Procedure

Before each rehearsal run (and at T-10 minutes before the live demo), flush
the cache so Viewer 1 hits a guaranteed cold MISS:

```bash
ssh host2.fabstir.net 'sudo find /var/cache/fabcdn -mindepth 1 -delete && sudo nginx -s reload'
ssh host2.fabstir.net 'sudo find /var/cache/fabcdn -type f | wc -l'   # 0
ssh host2.fabstir.net 'systemctl is-active nginx'                     # active
```

## FabDiscover Edge (Geo-Fencing Trust Boundary)

Separate from the `/fabcdn/` blob cache, the host1.fabstir.net nginx also fronts
the local **fabdiscover-search** reader (`127.0.0.1:7700`) at `/fabdiscover/`,
proxying `POST /search`, `GET /health`, and (Phase 4.6) `GET /geo`. The block —
`host1/fabdiscover-location.conf` — is the **security trust boundary** for
territorial geo-fencing.

The geo design **ships inert**: the reader resolves the viewer's country from a
precedence chain (`X-Geo-Country` → `CF-IPCountry` → `CloudFront-Viewer-Country`
→ app-layer MaxMind on the rightmost `X-Forwarded-For` hop → `XX`) and treats
`XX` (unknown) as **permissive**. With no MaxMind DB and the client geo-headers
stripped, every viewer resolves `XX` → no filtering, nothing breaks. The infra
below is the switch that turns enforcement on. The block itself is
route-agnostic — it proxies the whole prefix, so `/geo` needs no config change
once the reader ships it.

### Deploy the block

Same pattern as the fabcdn snippet — stage, drop into `/etc/nginx/snippets/`,
add one `include` line to the host1.fabstir.net server block, validate, reload:

```bash
scp host1/fabdiscover-location.conf "$USER"@host1.fabstir.net:/tmp/
sudo cp /tmp/fabdiscover-location.conf /etc/nginx/snippets/
# inside the server { } block for host1.fabstir.net, add:
#   include /etc/nginx/snippets/fabdiscover-location.conf;
sudo nginx -t && sudo nginx -s reload
```

### Harden :7700 to loopback (REQUIRED)

The reader binds `0.0.0.0:7700`, so `:7700` is directly reachable today — a
client could hit `host1:7700/search` to bypass the nginx geo-header strip and
spoof their region. Two fixes, do both:

- **Box (this side):** firewall `:7700` to loopback so only nginx reaches it.
  ```bash
  sudo ufw deny 7700/tcp                                    # ufw allows lo by default
  # or, iptables:
  sudo iptables -A INPUT -p tcp --dport 7700 ! -i lo -j DROP
  ```
  nginx → `127.0.0.1:7700` is unaffected.
- **Reader (v2 side):** the reader will default its listen to `127.0.0.1`
  (Phase 4.6) so it is never internet-facing by design.

### Turn enforcement on: GeoLite2 DB

App-layer resolution needs a MaxMind GeoLite2-Country database on the box,
pointed at by `GEOIP_DB_PATH` on the `fabdiscover-search` systemd unit. Until it
exists, geo stays inert (everyone `XX` → permissive).

```bash
# Option A — MaxMind (free account + license key), kept fresh by geoipupdate:
sudo apt install geoipupdate
# /etc/GeoIP.conf:  AccountID / LicenseKey / EditionIDs GeoLite2-Country
sudo geoipupdate                        # → /usr/share/GeoIP/GeoLite2-Country.mmdb
# Option B — no account: DB-IP Lite Country (CC-BY) or IP2Location LITE .mmdb.

# Point the reader at it (systemd drop-in for fabdiscover-search):
#   Environment=GEOIP_DB_PATH=/usr/share/GeoIP/GeoLite2-Country.mmdb
sudo systemctl daemon-reload && sudo systemctl restart fabdiscover-search
```

`XX` (DB miss / private IP / no DB) is permissive by design — a geo miss shows
the title rather than punishing legit users (corporate proxies, IPv6 quirks).
VPN users bypass it; that is the intended soft-enforcement norm.

### CORS & TLS

- **CORS:** do **not** add CORS headers in this block — the reader sets
  `Access-Control-Allow-Origin` itself via `CORS_ORIGIN`, which must include
  `https://v2.fabstir.io` (the UI hits `/geo` cross-origin). Double-ACAO breaks
  browsers (same rule as the fabcdn note above).
- **TLS:** `/search` and `/geo` both ride host1 HTTPS. A lapsed cert makes the
  region gate **fail open** (and can break search) — keep host1 cert
  auto-renewal healthy (`certbot renew --dry-run`; watch for a `:80` Caddy
  squatter blocking the HTTP-01 challenge).

## Known Limitations (Demo Scope)

- **No client-side edge→S5 fallback.** If the edge returns 502/504 mid-demo,
  the fabstir-v2 SW has no automatic retry against the S5 portal — the
  escape hatch is the `NEXT_PUBLIC_ENABLE_FABCDN=false` kill switch + rebuild.
- **CORS is `*`.** Tight post-demo (restrict to fabstir-v2 origins only).
- **Single region.** One edge, at host2. No geographic distribution.
- **nginx is host-level, not containerized.** Fine for demo; the post-demo
  Rust edge-cache service will be containerized and binary-signed.
- **No upstream keepalive.** Each MISS = fresh TLS handshake to S5;
  `upstream { keepalive 16 }` + `Connection ""` would cut that, post-demo.
- **Range requests not cache-split** — assumes full-segment GETs (hls.js
  default). Post-demo: add `slice` module or strip `Range:` at upstream.
- **No payment-contract integration or signed-allowlist flow** — ships
  with the post-demo Rust service.

See `docs/fabstir-v2/fabcdn.md` for the full post-demo roadmap.
