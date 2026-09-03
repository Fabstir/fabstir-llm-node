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
     │ (1) GET/HEAD https://host2.fabstir.net/fabcdn/s5/blob/{cid}
     ▼
  nginx @ host2 — location /fabcdn/          [the redirect fetcher]
     │ proxy_pass https://s5.platformlessai.ai/
     │ 307 comes back pointing at a presigned R2 URL; proxy_redirect
     │ rewrites its Location so the browser bounces back to US, not to R2
     ▼
     │ (2) GET https://host2.fabstir.net/fabcdn/r2/{bucket}/{key}?X-Amz-…
     ▼
  nginx @ host2 — location /fabcdn/r2/       [the byte path]
     │ slice 1m — each 1 MB slice is a separate cache entry
     ▼
  proxy_cache fabcdn ──► /var/cache/fabcdn   (50 GB LRU, 30d inactive)
     │ cache MISS
     ▼
  proxy_pass https://<acct>.r2.cloudflarestorage.com/   (Host restored,
     │                    so the presigned SigV4 signature still validates)
     ▼
  cipherbytes streamed back → response rewritten with:
     X-Cache: HIT | MISS | BYPASS | EXPIRED
     X-Origin: r2.cloudflarestorage.com
     Access-Control-Allow-Origin: *
     Access-Control-Expose-Headers: X-Cache, X-Origin, ETag, Content-Length,
                                    Content-Range, Accept-Ranges
```

Step (2) is what makes the edge a cache at all. Without the R2 `proxy_redirect`
the browser leaves the edge after step (1) and every byte flows direct from R2 —
the edge caches a redirect and nothing else. `/fabcdn/dl/` is the older byte path
(`dl.platformlessai.ai`); the portal no longer redirects there, but the block is
kept correct so it works the moment it ever does again.

### Ranged media: `slice` is not optional

Media players seek, so they send `Range:`. Caching a `206` under a key that does
not include the range makes two different ranges of one object collide on a
single cache entry — nginx serves whichever landed first for **both**, and the
video is silently corrupt. The three byte-serving blocks therefore all use:

```nginx
slice             1m;
proxy_set_header  Range $slice_range;
proxy_cache_key   $scheme$proxy_host$uri$slice_range;   # ← $slice_range REQUIRED
```

`$args` is deliberately **excluded** from the key: the R2 presigned signature
rotates, so including it would give every request a fresh key and nothing would
ever hit. The accepted consequence is that warm bytes are served without
re-checking the signature — see Known Limitations.

`tests/smoke-test.sh` → `test_range_integrity` is the regression guard; it fails
loudly if the key ever loses its range component.

### HEAD-no-write design

The fabstir-v2 client probes cache status with a HEAD before its GET. If
that HEAD populated the cache with an empty body, the GET would hit an
empty entry. Two maps fix this:

- `$fabcdn_is_blob` — 1 for URIs matching `^/fabcdn/s5/blob/`,
  `^/fabcdn/dl/` or `^/fabcdn/r2/`. **Every byte-serving location must be
  listed here.** `$fabcdn_bypass` defaults to 1, so a location missing from
  the map reads `BYPASS` forever and caches nothing — it fails silently,
  behind a perfectly healthy-looking 200.
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
# The byte paths need the slice module. Stock Ubuntu nginx has it; check anyway,
# because without it `nginx -t` fails on the `slice` directive.
nginx -V 2>&1 | tr ' ' '\n' | grep http_slice_module   # expect a match

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

### 7. The 307 now points back at the edge (the go/no-go for caching)

This is the one check that says whether the edge caches bytes at all. If the
`Location` still names `r2.cloudflarestorage.com`, the browser leaves the edge
and nothing downstream matters.

```bash
curl -sD- -o/dev/null https://host2.fabstir.net/fabcdn/s5/blob/<CID> \
    | grep -i '^location:'
# want: Location: https://host2.fabstir.net/fabcdn/r2/…?X-Amz-…
# bad:  Location: https://<acct>.r2.cloudflarestorage.com/…   ← proxy_redirect
#       is not matching; the account host in the config has drifted from the
#       one the portal presigns. Fix the host, do not chase the cache.
```

### 8. Bytes are cached, and ranges come back correct

```bash
# follow the redirect twice — the second must be a HIT
curl -s -o/dev/null -L -w '%{http_code}\n' https://host2.fabstir.net/fabcdn/s5/blob/<CID>
curl -sD- -o/dev/null -L https://host2.fabstir.net/fabcdn/s5/blob/<CID> | grep -i x-cache
# want: 200 (or 206), then X-Cache: HIT

# X-Cache: BYPASS instead of MISS/HIT means the path is not in $fabcdn_is_blob
# in 10-fabcdn-map.conf. It will never cache and will never error. Check there
# first — this is the failure mode that looks entirely healthy.
```

Then the range check, against a **video** CID (>2 MiB — a manifest CID is a few
hundred bytes and the test will skip):

```bash
CID_PUBLIC=<video-cid> BASE_URL=https://host2.fabstir.net/fabcdn \
FLUSH_CMD='ssh host2 "sudo find /var/cache/fabcdn -mindepth 1 -delete && sudo nginx -s reload"' \
    bash tests/smoke-test.sh
```

`test_range_integrity` downloads the object whole, then re-reads three byte
ranges through the edge and compares them byte for byte. It is the guard against
the range-collision bug — a green run here is what licenses "caching is on".

Finally confirm the indexer survived the reload:

```bash
curl -s https://host2.fabstir.net/fabindex/health
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

### Deploy the block (replace-inline)

host1 **already has an inline `location /fabdiscover/`** — an earlier,
pre-hardening version without the geo strips. Deploy is therefore a **replace of
that inline block**, *not* an `include`: a second `location /fabdiscover/` in the
same server is an nginx "duplicate location" error that aborts the reload. This
`.conf` is the canonical content for that inline block (it is a valid standalone
location block, but here it lives inline rather than as an `include`d snippet).

Locate the server-block file, replace the block's body, validate, reload:

```bash
# find the file (resolves through any sites-enabled symlink):
sudo nginx -T | awk '/# configuration file/{f=$0} /location \/fabdiscover\//{print f; exit}'

# in that file, REPLACE the body of the existing `location /fabdiscover/ { … }`
# with host1/fabdiscover-location.conf — i.e. add the 3 strip lines to the live
# block. Do NOT add an `include`; that creates a duplicate location.

sudo nginx -t && sudo nginx -s reload
```

### Rollback

Unlike the fabcdn block there is no `include` to comment out — rollback is
restoring the prior inline block, i.e. **delete the three
`proxy_set_header …Country "";` lines** and reload:

```bash
sudo nginx -t && sudo nginx -s reload    # after removing the 3 strip lines
```

Safe while enforcement is inert: without the strips the block reverts to plain
passthrough, and every viewer still resolves `XX` → permissive regardless.

### Restrict :7700 to loopback

The reader binds `0.0.0.0:7700`, so a client *on a network that can reach the
box* could hit `:7700/search` directly and bypass the nginx geo-header strip.

The **durable, topology-independent** control is the app-layer bind: the reader
defaults its listen to `127.0.0.1` (Phase 4.2 —
`listen(config.port, config.bindHost ?? '127.0.0.1')`), so it is never
internet-facing by design. (`GET /geo` itself is Phase 4.6.)

On this host `:7700` also isn't internet-reachable (NAT forwards only 80/443 —
verified externally), so the residual is LAN-local and a box firewall is
optional here. But that NAT fact is **host-specific** — re-confirm `:7700`
exposure on any cloud/k8s migration (the exact portability case the
`X-Geo-Country` contract targets); the loopback bind is what stays load-bearing
across that move.

A box firewall is **optional** defense-in-depth — not needed where `:7700` isn't
exposed, but if you want it anyway:

```bash
sudo ufw deny 7700/tcp                                    # ufw allows lo by default
# or, iptables:
sudo iptables -A INPUT -p tcp --dport 7700 ! -i lo -j DROP
```
nginx → `127.0.0.1:7700` is unaffected either way.

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
- **The R2 account host is hardcoded** in `proxy_redirect` / `proxy_pass`, as
  `dl.platformlessai.ai` was before it. If the portal rotates buckets the
  redirect stops matching and delivery degrades to direct-and-uncached — the
  safe failure, not an outage, but nothing alerts. Worth a monitor on `X-Cache`
  never reaching HIT.
- **Cached bytes are served without re-checking the presigned signature.**
  Intended — FabCDN is ciphertext-only by design — but it does make these paths
  an unauthenticated mirror of whatever has been cached.
- **A cold slice still needs a live presign.** Warm slices serve regardless, but
  a miss against an expired `X-Amz-Signature` 403s. The `/fabcdn/` block caches
  the 307 for 5m, well inside the 24h presign, so this only bites a cold cache
  reached through an old URL.
- **No payment-contract integration or signed-allowlist flow** — ships
  with the post-demo Rust service.

See `docs/fabstir-v2/fabcdn.md` for the full post-demo roadmap.
