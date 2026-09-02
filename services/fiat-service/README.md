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

### `clientAddress` on POST /v1/fiat/session — the identity contract

**`clientAddress` must be the identity that SIGNS the session init, not whichever
address is paying.** The vault opens the job and signs an FC1.6 authorisation naming
this address; the node later recovers a signer from the encrypted `session_init`
signature and serves the session only if the two match. It can only ever compare
against what it can recover, so naming any other address means the session is refused
on arrival, after the escrow is open.

The two are the same key in a browser client that signs with its own wallet, which is
why this is easy to get wrong and easy to miss. They are NOT the same in a client with
separate roles. The Blender helper has exactly that split and shipped the bug:

| Identity | Purpose | Correct to send? |
|---|---|---|
| Burner wallet | holds USDC, pays gas | ❌ never seen by the node's verifier |
| Encryption identity (S5-seed derived) | signs the encrypted session init | ✅ this one |

The failure is loud but only at connect time, and the node names both addresses:

```
SESSION_AUTH_DENIED: client 0xc84be515… (recovered from the E2EE v1 signature)
is not authorised for vault-paid job 1108; authorised client is 0x1a84ef26…
```

If you see that, the fix is on the client: send the signing identity when opening the
session. The escrow from the refused attempt is reclaimable (it settles zero and
refunds in full), but it is a wasted round trip, so get this right before spending.

### `kind` on POST /v1/fiat/session — the service owns the session shape

A fine-tuning run cannot live inside the chat/render session: the node's accept gate
(DESIGN-TRAINING-M0-INTERFACE A.3) needs remaining lifetime ≥ 12600 + 600 s and a
proof window of 3600 s, so a session created with the standard 3600 / 1000 / 300 shape
passes this service's gate, funds from the vault, and then fails after escrow — a funded
round trip that cannot succeed. The body therefore carries an optional `kind`:

| `kind` | On-chain shape (maxDuration / proofInterval / proofTimeoutWindow) | Caps |
|---|---|---|
| absent | 3600 / 1000 / 300 (the standard proof window is an env knob, max 3600) | `FIAT_MAX_SESSION_DEPOSIT_MICRO`, `FIAT_MAX_DAILY_SPEND_MICRO` |
| `"training"` | 14400 / 1000 / 3600 (the SDK's wallet-path shape, proven live) | `FIAT_TRAINING_MAX_SESSION_DEPOSIT_MICRO`, `FIAT_TRAINING_MAX_DAILY_SPEND_MICRO` |

Only the literal `"training"` is accepted; a JSON `null` counts as absent (the route's
`idempotencyKey` convention); anything else is a 400. The browser names a kind and never
numbers, because the vault is the one fronting the money. Everything
else in the body and the whole response are unchanged. The kind is BOUND to the model
id in both directions: `"training"` needs a `modelId` from `FIAT_TRAINING_MODEL_IDS`
(the registered TRAINING model id, `keccak256("fabstir/training/" + templateId)`, not the
template hash and not the serving model), and one of those ids needs `"training"`;
either mismatch is `MODEL_KIND_MISMATCH`, and with the list unset the training kind refuses
every open. Each kind's daily cap is its own budget of GROSS holds over a rolling 24 h (a
settled hold still counts), one training session may be in flight per user
(`FIAT_TRAINING_MAX_CONCURRENT`, counted over live holds only), and the stranded-escrow
reclaimer waits out each kind's own lifetime (`FIAT_TRAINING_RECLAIM_AFTER_MS`, default
6 h, validated at boot against the contract's proof-silence clock) before sending
`triggerSessionTimeout` — which is a precondition for card-paid training, since a card
customer has no wallet to reclaim with. The approve float is exactly
`FIAT_VAULT_ALLOWANCE_FLOAT_MICRO`; the service refuses to boot when an enabled kind's cap
exceeds it (raise both in one edit). The node's gate leaves 1,200 s between session
creation and the `train` frame, so open the session after the dataset upload, immediately
before `submitTraining`.

### Refusals a client must not retry

`403 refused` carries a machine-readable `reason`; `502 chain_error` means the chain
could not be reached or the transaction failed and a retry is reasonable — but attempts
of EVERY outcome count toward the per-user per-minute budget (`FIAT_MAX_OPENS_PER_MINUTE`,
default 3), so the fourth attempt inside a minute is `RATE_LIMITED`. One case that used
to come back as `chain_error` is now a refusal, because retrying it can never work:

| `reason` | Meaning |
|---|---|
| `HOST_NOT_ALLOWED` | the host is not on `FIAT_ALLOWED_HOSTS` (a list of host addresses, not host + model pairs) |
| `MODEL_NOT_PRICED` | the host has not advertised this model: `NodeRegistry.getModelPricing(host, model, USDC)` is 0 |
| `MODEL_KIND_MISMATCH` | `kind` and `modelId` disagree (either direction), or the training list is unset |
| `DEPOSIT_OVER_CAP`, `DAILY_CAP_EXCEEDED` | the per-kind caps above |
| `CONCURRENT_CAP_EXCEEDED` | the user already has a live training hold |
| `INSUFFICIENT_BALANCE`, `RATE_LIMITED`, `INVALID_DEPOSIT` | as named |

None of the refusals journals a hold: the gatekeeper is previewed before the model-price
read, the read comes before the hold, and every no-hold outcome frees the idempotency key
so the same key retries cleanly. A FAILED CREATE still journals a hold and still counts
toward the gross day.

Rule of thumb: if your code can name two different addresses when asked "who is the
client", you have this bug latent. Pick the one whose private key signs the session
init.

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
- **`clientAddress` is the SIGNING identity, not the paying one** (see the routes
  section). Cost us two denied renders and a stranded escrow on 2026-08-20 before the
  node's denial line named both addresses and made it obvious.
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

6. **Post-deploy gate, both halves, scoped to THIS boot:** `/v1/fiat/balance?address=…`
   answers 200 AND the journal shows `[fiat] gatekeeper:`, `reclaim delays:` and
   `[fiat-settlement] listener started` for the current invocation
   (`journalctl -u fiat-service _SYSTEMD_INVOCATION_ID=$(systemctl show -p InvocationID --value fiat-service)`).
   `systemctl is-active` proves nothing: a failed boot check (an enabled kind's cap above
   the float — the STANDARD cap counts too, so read the live `FIAT_MAX_SESSION_DEPOSIT_MICRO`
   before deploying; a bad proof-window knob; a reclaim delay below its floor, checked whenever
   settlement is enabled) leaves the unit active with EVERY route at 500 until restart, and an
   unscoped grep matches yesterday's line. Prove a bad-env boot once on a scratch instance (`npx next start -H 127.0.0.1
   -p 3021` with a scratch `FIAT_DATA_DIR`), never on the live unit.
7. With `FIAT_RECLAIM_STRANDED` OFF, a stranded card-paid session (node restart mid-run)
   keeps its deposit debited AND keeps the user's one training slot occupied until someone
   sends `triggerSessionTimeout(jobId)` by hand; that manual step is the only path back.

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
