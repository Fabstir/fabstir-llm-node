# @fabstir/fiat-service

The **one** shared fiat-credits backend for the Fabstir platform: buy credits with a
card, hold a credits balance, spend it on compute via a pre-funded vault, and cash
out. It is deployed **standalone** and called over HTTP by every frontend that needs
fiat credits — the Platformless AI UI (platformlessai.ai), the Blender website, and
the Blender extension. **One vault, one ledger, one Stripe account, the same smart
contracts.** It is API-only: no UI, no wallet stack, no S5.

This is the extraction of the FC1/FC2 fiat backend that was proven live inside
`platformless-blender-website` (buy 500 credits → vault-paid render, job 954 → cash
out via a real Stripe refund). The route handlers and libs are ported verbatim; only
the mount path (`/api/fiat/*` → `/v1/fiat/*`), CORS, and the standalone config are new.

## Routes (the v1 contract)

Mounted under `/v1/fiat/*`. Browser clients point `NEXT_PUBLIC_FIAT_SERVICE_URL` at
`https://<host>/v1/fiat`.

| Method | Path | Caller | CORS |
|---|---|---|---|
| POST | `/v1/fiat/purchase` | browser | ✅ |
| GET  | `/v1/fiat/balance?address=` | browser | ✅ |
| POST | `/v1/fiat/session` | browser | ✅ |
| POST | `/v1/fiat/cashout` | browser | ✅ |
| GET  | `/v1/fiat/credential/challenge` | browser | ✅ |
| POST | `/v1/fiat/credential/self-serve` | browser | ✅ |
| POST | `/v1/fiat/credential` | operator (server) | ❌ |
| POST | `/v1/fiat/stripe/webhook` | Stripe (server) | ❌ |

The four client files the UI drops in (`fiat-fetch`, `fiat-purchase-client`,
`fiat-credential-client`, `fiat-credits`) already call these paths with a configurable
`baseUrl`. See `docs/archive/FC2-FIAT-CLIENT-DROP-README.md`.

## CORS

Handled in `middleware.ts`, scoped to the **browser routes only** (the table above).
Allowed origins come from `FIAT_CORS_ALLOWED_ORIGINS` (comma-separated; a single `*`
allows any). Preflight `OPTIONS` is answered in the middleware; `content-type` and
`authorization` are the allowed request headers; **credentials are OFF** (these routes
carry no cookies — they authenticate with a backend-key signature or a spending
credential in the body). The Stripe webhook and the operator credential route are
server-to-server and get **no** browser CORS.

The middleware also stamps `Cache-Control: no-store` on every browser-route response:
these are money reads/writes that must never be cached, and it sidesteps a Next 14
quirk where a middleware-set `Vary: Origin` is replaced by the framework's own Vary
on actual responses (preflights do carry `Vary: Origin`). Locked by
`test/middleware.test.ts`.

> The node's own compute routes (`/v1/session-auth`, `/v1/ws`, inference, …) already
> ship `CorsLayer::permissive()` node-side, so a browser executor can reach them
> cross-origin without any change here. This service's CORS is only for its own
> `/v1/fiat/*` routes.

## Environment

Copy `.env.example` → `.env` and fill it in. Two classes:

- **`FIAT_*` secrets** (Stripe keys, `FIAT_VAULT_PRIVATE_KEY`,
  `FIAT_BACKEND_AUTH_PRIVATE_KEY`, `FIAT_ADMIN_TOKEN`) — **runtime only**, never
  `NEXT_PUBLIC_`, never logged, never committed.
- **`NEXT_PUBLIC_*` chain config** (RPC, contract/USDC/host/model/price) — Next inlines
  these **at build time**, so they must be present when you run `next build`, not only
  at runtime. (They are `NEXT_PUBLIC_`-named because the libs are shared verbatim with
  the website; here they are just server-side config.)

`FIAT_DATA_DIR` is the single ledger + credential journal directory. **It is the users'
money — back it up out of band.** It is gitignored.

## Run

```bash
npm install
cp .env.example .env      # fill in
npm run typecheck         # tsc --noEmit
npm test                  # vitest (ported FC2 route/lib tests + the middleware CORS test)
npm run build             # next build
npm run dev               # dev server on 0.0.0.0:3020
npm start                 # production server on 0.0.0.0:3020
```

Bind `-H 0.0.0.0` (the scripts do) so a Docker port map can forward to the container.

## Gotchas (learned live — do not rediscover)

- **Single instance only.** The challenge-nonce store and the in-memory credential
  cache live in-process (on `globalThis`, to survive dev HMR). A nonce issued by
  instance A is invisible to instance B, so **do not run replicas** behind a load
  balancer yet; multi-instance needs a shared store (Redis).
- **Do not restart the server mid-mint.** A restart drops in-flight challenge nonces,
  so a GET-challenge → POST-self-serve straddling a restart fails with
  "challenge unknown". (This bit us live during GATE 2.)
- **The Stripe webhook needs the RAW body** for signature verification; the route reads
  it as text — do not insert a body parser in front of it.
- **Rate-limit `/v1/fiat/credential/*` per-IP at the edge** before public exposure.
- **Currency:** credits are USD-pegged (1 credit = 1 cent = 10,000 micro-USDC). Model A
  (charge USD, the card network converts) is the default and needs no code. Model B
  (charge local currency) needs an FX lookup **and** a webhook change (credit from
  `metadata.credits`, not `amount_total`). See the contract's Currency & FX section.

## Deploy (Jules)

1. `npm ci` (or `npm install`), set `.env` with **test** Stripe keys + the test vault
   key, and set `FIAT_CORS_ALLOWED_ORIGINS` to the UI origin(s).
2. `npm run build` with the `NEXT_PUBLIC_*` chain config present in the env.
3. `npm start` behind the reverse proxy (`services/nginx-edge`), on a stable host.
   Mount a persistent, backed-up volume at `FIAT_DATA_DIR`.
4. Give the UI the host for `NEXT_PUBLIC_FIAT_SERVICE_URL = https://<host>/v1/fiat`.
5. Point the Stripe webhook endpoint at `https://<host>/v1/fiat/stripe/webhook` and set
   `FIAT_STRIPE_WEBHOOK_SECRET` to its signing secret.

The full paid walk (buy → balance → cash-out, then card-paid render) is a **Jules-run
live gate**, not part of scaffolding.

## Deliberately NOT in this service

- The three **client** libs (`fiat-fetch`, `fiat-purchase-client`,
  `fiat-credential-client`) and the `fiat-credits`/`credits` display helpers — those
  are the frontend's (the UI already has them).
- **S5 seed derivation** (`generateS5SeedFromAddress`) — a browser concern. `seed.ts`
  here is trimmed to the one constant the fiat path uses (`BASE_SEPOLIA_CHAIN_ID`), so
  the service does not vendor `@fabstir/sdk-core`.
- The **helper pairing / sealed-seed** flow (`pairing.ts`) — a website+helper concern.
- The **card-paid render path** — that additionally needs the SDK to attach to a
  vault-deposited session and present the FC1.6 authorisation to the node. See
  `docs/archive/FC1.6-SESSION-AUTH-HANDSHAKE.md`.
