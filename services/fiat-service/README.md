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
| GET  | `/v1/fiat/packs` (public) | browser | ✅ |
| GET  | `/v1/fiat/balance` (bearer credential) | browser | ✅ |
| POST | `/v1/fiat/session` | browser | ✅ |
| POST | `/v1/fiat/cashout` | browser | ✅ |
| GET  | `/v1/fiat/credential/challenge` | browser | ✅ |
| POST | `/v1/fiat/credential/self-serve` | browser | ✅ |
| POST | `/v1/fiat/credential` | operator (server) | ❌ |
| POST | `/v1/fiat/stripe/webhook` | Stripe (server) | ❌ |
| POST | `/v1/fiat/stripe/reconcile` | cron, daily (`FIAT_ADMIN_TOKEN`) | ❌ |

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
| `INSUFFICIENT_BALANCE`, `RATE_LIMITED`, `INVALID_DEPOSIT` | as named (a balance below zero after a refund or chargeback reads as `INSUFFICIENT_BALANCE`) |
| `ACCOUNT_FROZEN` | the user has an open chargeback: no session and no refund until it closes |

None of the refusals journals a hold: the gatekeeper is previewed before the model-price
read, the read comes before the hold, and every no-hold outcome frees the idempotency key
so the same key retries cleanly. A FAILED CREATE still journals a hold and still counts
toward the gross day.

Rule of thumb: if your code can name two different addresses when asked "who is the
client", you have this bug latent. Pick the one whose private key signs the session
init.

## Card packs in pounds (GP)

Plan: `docs/development/IMPLEMENTATION-GBP-CARD-PRICING.md` v0.6; the exchange rule:
`docs/fabstir-v2-reference/platformless-ai-card-fx-handoff/SPEC-CARD-FX-RULE.md` v1.3, shared
with Fabstir v2. A credit keeps its face value (one US cent); cards are charged in **pounds,
VAT included**, for **packs only**: 500, 1,000, 2,500 and 10,000 credits.

- `src/lib/credit-price.ts` and `src/lib/fx-rule.ts` are v2's modules, copied **unchanged**
  and pinned by SHA-256 in `test/gp-pricing.test.ts`. An update is a fresh copy from v2 plus
  the new hash, never a local edit.
- `src/lib/card-pricing.ts`: the settings (VAT 2,000 bps; card costs 330 bps + 28p; USDC
  refill 100 bps; treasury 0) and the pack prices at a given rate.
- `src/lib/card-fx-sources.ts`, `card-fx-store.ts`, `card-fx.ts`: the feed. Kraken's
  15-minute USDC/GBP candles at :03, :18, :33, :48 (up to 10 retries a minute apart when the
  candle that closed 3 minutes earlier is not final yet) and the ECB 90-day file at boot and
  at :01 past every hour. `servedNow()` is the one source of the rate: `/packs`, `/purchase`
  and a 60-second tick call it, and a change is fsynced to `served.jsonl` BEFORE the answer
  leaves. It starts at boot from `instrumentation.ts` **always** (not behind
  `FIAT_SETTLEMENT_ENABLED`); routes start it lazily too.

**`GET /v1/fiat/packs`** answers `{ currency: 'gbp', pricing: 'gbp1', packs: [{ credits,
priceMinor, vatMinor }] }` with `Cache-Control: no-store`, or `503 card_unavailable` when no
rate is served. The rate itself is not in the answer.

**`POST /v1/fiat/purchase`** body: `{ clientAddress, credits, priceMinor, consent, email? }`.
`credits` must be a pack (`400 not_a_pack`); `priceMinor` is the pence the buyer was shown
(`400 price_required` without it). If the price at the served rate differs, the answer is
`409 price_changed` with `{ currency, pricing, packs }` and no session is created (R12). No
rate served: `503 card_unavailable`. The Checkout Session is `currency=gbp`,
`tax_behavior=inclusive`, `expires_at` = request time + 1,860 s (R15), Adaptive Pricing off,
and carries `fc1Credits`, `fc1FxMicro`, `fc1PriceMinor`, `fc1Pricing=gbp1` on the session and
the PaymentIntent.

**The webhook** credits a session only when it is `gbp`, stamped `gbp1` with a pack and
whole-number stamps, the billing address is **GB** (a `gbp1` price carries UK VAT only,
whatever `SALES_EU` says), and **R14** holds on Stripe's signed `created`/`expires_at`:
(a) stamped rate ≤ 1.40, (b) ≤ the highest rate served from `created − 300` to
`created + 60`, (c) `expires_at − created` ≤ 1,920 s. It credits the pack when Stripe charged
the stamped price with its VAT (within a penny), otherwise what was paid, **never more than
the pack**, with an alarm. Anything else is refunded in full by the gate path and raises an
URGENT fx alert (reasons `currency`, `pricing`, `rate_ceiling`, `rate_window`,
`session_lifetime`). If the feed cannot be built the webhook answers 503 for a pack session
(Stripe redelivers) and handles every other event as before.

**Refunds** return the share of what was paid, card-cost share and VAT included, per charge
in that charge's currency. A leg of a pound charge can be 0p (10,000 credits for 9,526p): it
moves the credits, makes no Stripe refund (refund id `none:0p`) and the penny returns on a
later leg; a request that would return 0p in all is `400 refund_too_small`. Every leg, row
and preview carries `currency`; previews and outcomes carry `byCurrency: [{ currency,
grossMinor }]` (a refund can span an old dollar charge and a pound charge). A refund row's
VAT is the charge's own share, the emptying leg taking the remainder.

**Settings.** `FIAT_CARD_FX_MAX_USD_PER_GBP` must read exactly `1.40` (the rule's ceiling,
only read and checked). `FIAT_CARD_FX_PIN_USD_PER_GBP` (optional; two decimals, 1.00 to
1.40) replaces the rule while set, with an alert once a day. `FIAT_FX_URGENT_NTFY_URL`
(optional) receives a push for every urgent fx alert. A bad ceiling or a malformed pin
puts the feed in a **misconfigured** state: no rate is served (card top-ups `503`), an
urgent alert names the setting, every other route works. **Retired:**
`FIAT_STRIPE_CURRENCY` and `FIAT_MAX_PURCHASE_CREDITS` (ignored if left set).

**Alerts.** `[fiat-fx] ALERT:` lines (stale over 1 h; a candle refused by, or used without,
the ECB check; no ECB fetch since 00:00 UTC by 02:00 UTC, when the rate holds and cannot fall
and a pin is the remedy; a ceiling cut; a pin set) and `[fiat-fx] URGENT:` lines (no rate:
dead, floor, none or misconfigured; an uncreditable payment). Each is raised when its
condition starts, cleared with a log line when it ends, and raised again if it recurs. Every
change of the served rate is a `[fiat-fx] served rate … -> …` log line. The host clock must
be NTP-synchronised (R14): check `timedatectl` at deploy.

**The fx store**, `FIAT_DATA_DIR/fx/` (on the same backed-up volume as the ledger):
`candles.jsonl` (every final candle fetched; a later line for a candle replaces it),
`ecb.jsonl` (every ECB entry, and `{fetchedAt}` for every successful fetch) and
`served.jsonl` (`{at, fxMicro|null}` on every change of what is served). All append-only and
fsynced; a torn last line is skipped on load. A restart re-reads them and reaches the same
rate. The first line of `served.jsonl` with a rate is the instant this service first served
under the rule; it is logged at boot. **A lost or older `served.jsonl` fails closed:**
payments whose window it no longer covers are refunded with an urgent alert, never credited
at a rate not on record.

**Reconciliation** judges each pound session by the same rules (R14 against
`served.jsonl`, the cap): one that should have been refunded is expected among the refused
purchases, not the charges. An old dollar charge is compared the old way.

**Rolling back** to the binary before GP after a pound purchase: there is no journal format
bump (only optional `currency` fields were added), so the old binary opens the journal and
balances are right, but it lists a pound purchase in dollars and its webhook would refund
every new pound session (it expects `usd`). Roll the site back with it.

## Checkout, VAT, refunds and chargebacks (TX1)

Plan: `docs/development/IMPLEMENTATION-TX1-STRIPE-TAX.md`. What a client needs to know:

**`POST /v1/fiat/purchase`** body: `{ clientAddress, credits, priceMinor, consent: { termsVersion, over18: true, useNow: true }, email? }`
(GP: packs in pounds, VAT included; see above). `consent` is
required (`400 consent_required` without it), and ALL THREE statements must be true: the
version of the Terms accepted, the statement of being 18 or over, and `useNow`, the "I
want to use my credits straight away" box (it starts unticked on the page and must be
ticked to buy). It rides in the session metadata and is journaled with the purchase.
`FIAT_CONSENT_REQUIRED=off` exists only for the web app's transition. Checkout starts are
rationed: 5 an hour per account and 10 an hour per client address (`429 rate_limited`,
`retry-after`); a start is given back when Stripe fails. `FIAT_STRIPE_TOS_CONSENT=on` adds
Stripe's own required checkbox (needs a Terms URL in the Stripe dashboard).

**The per-address limit assumes exactly ONE trusted proxy.** The service must listen on
loopback only, behind the nginx on the same box, whose site config
(`services/nginx-edge/host1/fiat.fabstir.net.conf`) sets `X-Forwarded-For
$proxy_add_x_forwarded_for`: nginx APPENDS the address it saw, so the rightmost entry is
the client and anything to its left was sent by the client and is ignored. If a second
proxy or a CDN is ever put in front of nginx, the rightmost entry becomes THAT proxy's
address and every buyer shares one 10-an-hour budget: change `clientIp` in
`src/lib/purchase-rate-limit.ts` (and nginx's `real_ip` settings) in the same change. If
the service is ever exposed without nginx, the header is the caller's to forge and only
the per-account limit holds.

**Stripe API version.** Every request sends `Stripe-Version: 2026-08-26.dahlia`
(`STRIPE_API_VERSION` in `src/lib/stripe.ts`), and the webhook endpoint must be created
with the same version.

**The webhook** credits (GP: the pack, see above) only when `automatic_tax.status` is
`complete` and the billing address is in a country we sell to (`GB`; a `gbp1` session is
credited for GB only, whatever `SALES_EU` says). Otherwise nothing is credited and the charge is refunded in
full (`Idempotency-Key: gate:<session id>`, `metadata[fc1Origin]=gate`). It journals
gross, tax, net, billing country, card country, whether a tax id was given, and the
consent and (GP) the currency. A paid session of ours that cannot be credited at all (a
currency other than pounds, a missing or malformed `gbp1` stamp, R14, an unusable tax
figure, no user id) is refunded the same way, with an alarm and an urgent fx alert.
Endpoint events to subscribe (ten): `checkout.session.completed`,
`checkout.session.async_payment_succeeded`, `charge.refunded`, `charge.refund.updated`,
`refund.updated`, `refund.failed`, `charge.dispute.created`, `charge.dispute.closed`,
`charge.dispute.funds_withdrawn`, `charge.dispute.funds_reinstated`.

An event about a charge of OURS that the ledger does not know yet (its purchase event is
late) is answered **503** for up to six hours so that Stripe redelivers it; other
products' charges on the shared account are answered 200. One Checkout Session (or
PaymentIntent) is one purchase, whatever the event ids.

**`POST /v1/fiat/cashout`** `{ amountMicro }` refunds that many credits AND the VAT paid
on them, newest card charge first, inside the refund window, never from a disputed
charge. Answer: `{ refunds: [{ refundId, paymentIntentId, amountMicro, grossCents, currency }],
grossCents, byCurrency, failed: [...], refundId, amountMicro }` (`refundId`/`amountMicro` are kept for
clients written before TX1; `grossCents` is in minor units and is a meaningful total only in
one currency, use `byCurrency`). `{ amountMicro, preview: true }` moves nothing and answers
`{ preview: true, grossCents, byCurrency, legs }`: show it before the customer confirms. New refusal
reasons: `ACCOUNT_FROZEN`; (GP) `400 refund_too_small`.

**`GET /v1/fiat/balance`** needs the account's credential (`Authorization: Bearer …`) and
answers `{ availableMicro, owedMicro, refundableMicro, frozen }` for that account. A lookup
by `?address=` alone is refused (`401 credential_required`) unless `FIAT_BALANCE_OPEN=on`.

**Refunds made in the Stripe dashboard and chargebacks** are journaled from
`charge.refunded` / `charge.dispute.*` and may take a balance below zero; such a user opens
no session. Alarms go to the service journal as `[fiat-stripe] ALARM:` lines.

**Daily reconciliation**: `POST /v1/fiat/stripe/reconcile` with `Authorization: Bearer
$FIAT_ADMIN_TOKEN`, from cron once a day. Findings are `[fiat-reconcile] ALARM:` lines and
come back in the answer.

**Refund legs left pending** (the process died between the debit and Stripe's answer, or
Stripe could not be asked) are settled at every boot and at the start of every
reconciliation: the refund is looked up by its key and the leg confirmed; if there is none
it is made under the same Idempotency-Key (inside 23 hours), or the leg is reversed. The
look-up reads every page of the charge's refunds. The rule that keeps "credits debited ==
cash actually refunded" through failures, stale `charge.refunded` snapshots and the sweep
is written above `reverseCashoutLeg` in `src/lib/ledger.ts` and walked for every ordering
in `test/tx1-refund-invariant.test.ts`.

**The purchase confirmation email** (terms and cancellation form in its text) is built in
`src/lib/purchase-confirmation.ts`; no delivery provider is wired in yet, so it is not sent.

**Journal format 2.** The ledger now refuses to start on a journal entry it does not
understand. The pre-TX1 binary has no such guard: see "Rolling back" under Deploy.

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
money — back it up out of band.** It is gitignored. (GP) The card rate's records live in
`FIAT_DATA_DIR/fx/` and are backed up with it: R14 is judged against `served.jsonl`.

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
- **Currency:** credits are USD-pegged (1 credit = 1 cent = 10,000 micro-USDC). Cards are
  charged in pounds for packs (GP, "Card packs in pounds" above); the rate comes from the
  shared rule, never a hand-set figure.

## Deploy (Jules)

1. `npm ci` (or `npm install`), set `.env` with **test** Stripe keys + the test vault
   key, and set `FIAT_CORS_ALLOWED_ORIGINS` to the UI origin(s).
2. `npm run build` with the `NEXT_PUBLIC_*` chain config present in the env.
3. `npm start` behind the reverse proxy (`services/nginx-edge`), on a stable host.
   Mount a persistent, backed-up volume at `FIAT_DATA_DIR`.
4. Give the UI the host for `NEXT_PUBLIC_FIAT_SERVICE_URL = https://<host>/v1/fiat`.
5. Point the Stripe webhook endpoint at `https://<host>/v1/fiat/stripe/webhook` and set
   `FIAT_STRIPE_WEBHOOK_SECRET` to its signing secret. Subscribe it to the ten events
   listed under "Checkout, VAT, refunds and chargebacks", with API version
   `2026-08-26.dahlia`. Stripe Tax must be ON in the same
   mode (origin address, GB registration) BEFORE this build takes purchases: every Checkout
   Session asks for automatic tax (GP: inclusive in the pack price). GP: set
   `FIAT_CARD_FX_MAX_USD_PER_GBP=1.40`, remove `FIAT_STRIPE_CURRENCY` and
   `FIAT_MAX_PURCHASE_CREDITS`, and check the host clock is NTP-synchronised (`timedatectl`).

6. **Post-deploy gate, both halves, scoped to THIS boot:** `/v1/fiat/balance?address=…`
   answers 401 `credential_required` (200 before TX1, or with `FIAT_BALANCE_OPEN=on`) AND the journal shows `[fiat] gatekeeper:`, `reclaim delays:` and
   `[fiat-settlement] listener started` for the current invocation
   (`journalctl -u fiat-service _SYSTEMD_INVOCATION_ID=$(systemctl show -p InvocationID --value fiat-service)`).
   `systemctl is-active` proves nothing: a failed boot check (an enabled kind's cap above
   the float — the STANDARD cap counts too, so read the live `FIAT_MAX_SESSION_DEPOSIT_MICRO`
   before deploying; a bad proof-window knob; a reclaim delay below its floor, checked whenever
   settlement is enabled) leaves the unit active with EVERY route at 500 until restart, and an
   unscoped grep matches yesterday's line. Prove a bad-env boot once on a scratch instance (`npx next start -H 127.0.0.1
   -p 3021` with a scratch `FIAT_DATA_DIR`), never on the live unit.
7. **Rolling back past this release means restoring the journal backup taken at the
   switch.** Take that backup of `FIAT_DATA_DIR` with the service stopped, immediately
   before starting the new build. The older binary has no guard against journal entries it
   does not know: started on a journal this release has written to, it silently SKIPS the
   new entry types (tax details, refund legs, dashboard refunds, disputes) and serves wrong
   balances without any error. So a rollback is: stop, put the pre-switch journals back,
   start the old build; anything journaled after the switch is then re-entered by hand from
   Stripe. Never start the old binary on the new journal.
8. With `FIAT_RECLAIM_STRANDED` OFF, a stranded card-paid session (node restart mid-run)
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
