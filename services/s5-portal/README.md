# S5 portal

The code and scripts that run our S5 storage portals: the auth proxy in front of the S5 node, the kit that builds a regional portal, and the one operations script the Global portal needs. Nothing here holds a secret. Secrets live only on the servers (see "Secrets" below), and the `.example` files carry placeholders.

Background: `docs/development/IMPLEMENTATION-SOVEREIGN-STORAGE-REGIONS.md` and `EXECUTION-SOVEREIGN-STORAGE-REGIONS.md`.

## Folders

| Folder | What it is |
| --- | --- |
| `proxy/` | The regional auth proxy, `server.js`. Site addresses come from the environment (`BASE_SEPOLIA_RPC_URL`, `S5_PROXY_STORAGE_BASE`, `S5_PROXY_STORAGE_HOST`, `S5_PROXY_CDN_BASE`, `S5_PROXY_BIND`). It includes the 22 November fix: older style ids are answered from our own store. Also `package.json`, the lock file and `test/` (`node:test`). |
| `global/` | A snapshot of what runs on the Global portal as of 22 November 2026: `server.js` (its first four lines are a header comment; the rest is byte for byte the deployed file, sha256 starting `d46dd297792d51de`), its `s5-proxy.service` and its `Caddyfile`. It predates the kit version and has its addresses as constants. |
| `ops/` | `block-public-peers.sh` and its unit: refuses the node's outbound links to remembered public S5 peers. |
| `regional/` | The kit a regional portal is built from: `build-regional-portal.sh`, unit files, `garage.toml.example`, `s5-config.toml.example`, `proxy.env.uk.example`, `Caddyfile.uk`. The build installs the proxy from `../proxy/`, so copy the whole `s5-portal` folder to the new box. `patch-from-live.py` is historical: it only reproduced the constants step from an old copy of the Global file and is no longer used. |

## Which server runs what today

| Server | Proxy |
| --- | --- |
| Global portal | `global/server.js` |
| UK portal | `proxy/server.js`, deployed 22 November 2026 at 16:01 UTC (sha256 begins d61d35a292a6b626); the previous file is kept on the box as `server.js.bak-20261122-legacy-direct`. |

## Tests

From `proxy/`, serially:

```
npm test            # the kit proxy (proxy/server.js)
npm run test:global # the same tests against ../global/server.js
```

The shipped file is loaded with stubbed `http`, `fs`, `ethers` and timers, so no port, secret or chain is touched. One test reads real media ids from `docs/fabstir-v2-reference/V2-MEDIA-IDS-2026-11-16.tsv` at run time and skips if that file is absent. Those ids embed decryption keys: never copy one into the repository, a test or a message.

## Deploying a proxy update safely

1. Back up the running file on the server: `cp /opt/s5-proxy/server.js /opt/s5-proxy/server.js.bak-$(date +%F)`.
2. Copy the new file over `/opt/s5-proxy/server.js`.
3. `node --check /opt/s5-proxy/server.js`. Do not go on if this fails; put the backup back.
4. `systemctl restart s5-proxy`, then check `curl -s http://127.0.0.1:5051/s5/version` and `journalctl -u s5-proxy -n 20`.

The storage websockets (`/s5/p2p`) go straight from Caddy to the node and do not pass through the proxy, so restarting the proxy does not drop them. Reloading the web front (Caddy) does, so avoid that for a proxy change.

## Lessons of 22 November 2026

1. The node answers older style ids only through locations it learned from peers, so cutting peers breaks those ids.
2. Never cut peers before the proxy serves every id form from our own store.
3. Verify isolation by watching live connections, not by reading configuration.
4. Test with real id forms (the reference file), not only synthetic ones.
5. HEAD is not answered by the node's blob route.

## Secrets

Not in this folder, by design: `/etc/garage.toml`, `/etc/s5/*.env`, `/root/.s5-master-token`, `/opt/s5/config/config.toml`. The Global `Caddyfile` carries the storage backend's basic auth header as the placeholder `REPLACE_WITH_RENTERD_BASIC_AUTH_BASE64`; the real value stays on the server. `.gitignore` keeps `node_modules` and any `*.env` that is not an `.example` out of git.

## Open follow-ups

1. Unify Global onto the kit proxy (then `global/server.js` goes).
2. Purge the remembered peers at the next planned node restart, then retire `ops/`.
3. Add an access log with timings on the download address.
