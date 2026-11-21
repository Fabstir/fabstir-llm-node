# Portal vault service

Holds each account's wrapped vault on a regional storage portal, and releases or replaces it only for a caller who proves control of the account's wallet.

The wrapped vault is the user's master seed, encrypted by the UI under the user's password and recovery code. To this service it is an opaque piece of text. The service never sees a password, a recovery code or a seed, and it never inspects, logs or changes the text it stores.

The contract is `docs/development/IMPLEMENTATION-PORTAL-VAULT-SERVICE.md`. This README is a guide to the code, and where the two differ the specification is right.

## How a caller proves control

1. The caller asks for a challenge for an address, a chain and a purpose (`read` or `replace`).
2. The service returns a nonce and the exact text to sign. The text names the region, the portal host, the address, the chain, the nonce and the expiry. For a replace it also names the SHA-256 of the new vault text and the version being replaced.
3. The wallet signs that text, either as a plain message (`personal_sign`) or as typed data (`eth_signTypedData_v4`).
4. The service rebuilds the text itself from what it stored, checks the signature on chain, and only then acts.

Plain keys, deployed smart accounts (ERC-1271) and smart accounts that are not yet deployed (ERC-6492) are all verified by the same on-chain call.

A nonce is consumed only by a request whose signature verified. A failed attempt, or an attempt made while the chain could not be asked, leaves the nonce live until it expires. This is deliberate: challenges are issued idempotently, so anyone who knows an address can learn its live nonce, and if a failed attempt burnt it that person could keep the owner locked out.

## Calls

All calls are under `/vault/v1`, with JSON in and out. Errors are `{ "error": "<code>" }`.

| Call | Purpose | Answers |
| --- | --- | --- |
| `POST /vault/v1/challenge` | Issue the nonce and the text to sign. Body: `address`, `chainId`, `purpose`, and for a replace `vaultSha256` and `replaces`. | 200, 400 `invalid-request`, 400 `unsupported-chain`, 429 `too-many-challenges` |
| `POST /vault/v1/open` | Return the vault and a read token. Body: `address`, `chainId`, `nonce`, `signature`, `form` (`message` or `typedData`). | 200, 404 `no-vault`, 401 `challenge-unknown`, 401 `signature-invalid`, 503 `signature-check-unavailable` |
| `GET /vault/v1/current?address=…&chainId=…` | Return the vault for a holder of a read token (`Authorization: Bearer <readToken>`). | 200, 401 `token-invalid`, 404 `no-vault` |
| `POST /vault/v1/replace` | Store a new vault. Body: as for open, plus `vault` and `replaces`. | 200, 409 `version-conflict` (with `currentVersion`), 413 `vault-too-large`, 400, 401 and 503 as for open |
| `GET /vault/v1/health` | Liveness and storage state. No account data. Outside the rate limit; the storage check is remembered for five seconds. | 200 |

Answers the HTTP layer adds, which the specification leaves open: 404 `not-found`, 405 `method-not-allowed`, 413 `body-too-large`, 429 `rate-limited`, 503 `storage-unavailable` and 500 `internal-error`. Every 429 and 503 carries `Retry-After`. A signature longer than 8 KiB is 400 `invalid-request`.

A replace keeps the previous version (`previous.json`) and revokes every read token issued before it.

## Configuration

Read from the environment. The service refuses to start unless every required value is present and well formed, and it names what is wrong. There is no default for any required value.

Required:

| Variable | Meaning |
| --- | --- |
| `VAULT_REGION` | Region name written into every signed message, for example `uk`. |
| `VAULT_PORTAL_HOST` | Public host name of the portal, written into every signed message. No scheme, no path. |
| `VAULT_CHAIN_ID` | The one accepted chain: `84532` (Base Sepolia) or `8453` (Base). |
| `VAULT_RPC_URLS` | Comma list of RPC URLs, tried in order. The next is asked only when the one before failed. Each must be https unless its host is loopback. |
| `VAULT_TOKEN_SECRET` | Hex, at least 32 random bytes (for example `openssl rand -hex 32`). Signs read tokens. Changing it revokes every read token. A value with fewer than 8 distinct byte values is refused. |
| `VAULT_S3_ENDPOINT` | S3 endpoint URL, for example `http://127.0.0.1:3900`. Must be https unless its host is loopback. |
| `VAULT_S3_ACCESS_KEY`, `VAULT_S3_SECRET_KEY` | A key that can read and write the vault bucket and nothing else. |
| `VAULT_S3_BUCKET` | A bucket used for nothing else, with no public access. |
| `VAULT_S3_REGION` | S3 region. Garage needs `us-east-1`. |

Optional:

| Variable | Default | Meaning |
| --- | --- | --- |
| `VAULT_BIND` | `127.0.0.1` | Address to listen on. |
| `VAULT_PORT` | `5052` | Port to listen on. |
| `VAULT_READ_TOKEN_DAYS` | `30` | Life of a read token. |
| `VAULT_MAX_BYTES` | `32768` | Largest vault text, in UTF-8 bytes. |
| `VAULT_ALLOWED_ORIGINS` | empty | Comma list of exact browser origins that receive CORS headers. Empty means none. |
| `VAULT_RATE_PER_MIN` | `60` | Requests per client per minute. |
| `VAULT_CHALLENGE_MAX` | `10000` | Most live challenges at once. |

The request body limit is 64 KiB and is not configurable.

## Limits

A client is the address of the connection. `X-Forwarded-For` is believed only when the connection comes from the loopback address, which is the reverse proxy on the same box, and then its last entry is used. An IPv4 client is its address. An IPv6 client is its /64. An IPv4 address written as IPv6 counts as the IPv4 address.

| Limit | Value | Answer past it |
| --- | --- | --- |
| Requests per client per minute | `VAULT_RATE_PER_MIN` | 429 `rate-limited` |
| Live challenges per client | 20 | 429 `too-many-challenges`, for that client only |
| Live challenges in total | `VAULT_CHALLENGE_MAX` | 429 `too-many-challenges` |
| Signature checks per client per minute | 10 | 429 `rate-limited` |
| Signature checks in flight at once | 16 | 503 `signature-check-unavailable` |
| Wait for the storage | 10 seconds | 503 `storage-unavailable` |

Clients that share an address share these limits: everyone behind one office or home connection, and the many subscribers a mobile carrier puts behind one address. A user who is refused with 429 waits for the `Retry-After` time.

### Behind a reverse proxy

The service is meant to sit behind a reverse proxy (Caddy on the portal box). Two things must hold, or the limits above stop meaning what they say:

- **The proxy runs on the same machine and reaches the service over loopback.** `X-Forwarded-For` is believed only from a loopback connection. A proxy on another machine is seen as one client, and its header is ignored.
- **The proxy sets `X-Forwarded-For` to the address it saw**, replacing or appending to whatever the caller sent. The service uses the last entry.

If either fails, every user shares one client key, and the whole service is limited to 20 live challenges and 10 sign-ins a minute for all users together. A sign that this has happened: 429 lines in the log whose `client` is `127.0.0.1` or the proxy's address.

A second proxy or a CDN in front of the first has the same effect, because the last entry is then that proxy's address for everyone.

No limit is keyed on the account address alone, because a stranger could then lock an owner out. A client that asks for a challenge which is already live is handed it and is not charged for it.

## Build, test and run

Node 18.17 or later.

```bash
npm ci
npm test            # vitest, one file at a time
npm run typecheck   # source and tests
npm run build       # TypeScript to dist/
npm start           # node dist/server.js
```

The box that runs the service needs only `dist/`, `package.json`, `package-lock.json` and the production dependencies (`npm ci --omit=dev`). It does not need TypeScript.

The S3 store has one integration test. It runs only when `VAULT_TEST_S3_ENDPOINT`, `VAULT_TEST_S3_ACCESS_KEY`, `VAULT_TEST_S3_SECRET_KEY`, `VAULT_TEST_S3_BUCKET` and `VAULT_TEST_S3_REGION` are all set, and is skipped otherwise. Point it at a test bucket, never at the live one.

The service writes one JSON line per call to standard output, with the time, the call, the address, the purpose, the outcome, the status and the duration. A storage failure also carries its `kind`: `timeout`, `unsettled`, `corrupt`, or the error code the storage gave, such as `AccessDenied`. A 429 also carries the `client` key it was counted by. The service never writes a vault, a signature, a nonce, a token or the text of an error.

It runs as a single instance. Reads and writes for one account are serialised inside the process, and live challenges are held in memory, so a restart discards them and callers ask for a new one.

## Runbook

### What is stored

Each account has at most two objects in the vault bucket:

```
v1/<chainId>/<address>/current.json
v1/<chainId>/<address>/previous.json
```

`<address>` is `0x` and 40 lower-case hex characters. Each object is one JSON record: `v`, `address`, `chainId`, `version`, `epoch`, `updatedAt`, `vault`. The `vault` field is the user's wrapped vault. Treat both objects as secret material: do not print them, paste them or copy them off the box.

### What `previous.json` is, and what it is not

A replace first copies the current record to `previous.json` and then writes the new record to `current.json`.

- After a successful replace, `previous.json` holds the version before the current one.
- After a replace whose second write failed, `previous.json` **equals** `current.json`. The older version it used to hold is gone. This is normal and harmless; the next successful replace puts it right.
- An account that has only ever had one version has no `previous.json`.
- `previous.json` is one step of history, kept so that an operator can undo a single bad write. It is not a backup. It does not protect against loss of the bucket, and the service never reads it to answer a user. Back the bucket up separately.

### Storage errors in the log

| `kind` | Meaning | What to do |
| --- | --- | --- |
| `timeout` | The storage did not answer within 10 seconds. The caller got 503. The account stays locked until that storage call really finishes, so a late write cannot land on top of a newer one. | Check the storage. Nothing to repair. |
| `unsettled` | A request arrived for an account whose earlier storage call has still not finished. Refused with 503. | Clears by itself when the storage answers. If it does not clear, the storage is hanging: fix it, then restart the service. |
| `corrupt` | `current.json` is not a valid record of that account, or it is missing while `previous.json` exists. Open and replace answer 503 for that account until it is repaired, so the UI can never lead the user into first-time set-up over a damaged record. | Recover the record, below. |
| an S3 code | The storage refused the call, for example `AccessDenied` or `NoSuchBucket`. | Check the key, the bucket and the endpoint. |
| `error` | A failure with no usable code. | Check that the storage is reachable. |

Only one answer from the storage means "this account has no vault": HTTP 404 with the S3 error code `NoSuchKey`. Everything else is treated as a storage error.

### Recovering a corrupt `current.json`

The log line names the address. Work on one account at a time.

1. Stop the service, so that nothing writes to the account while you work.
2. Download both objects for the account to a private directory on the box.
3. Check `previous.json`: it must be valid JSON with `v` equal to 1 and the right `address` and `chainId`.
4. If it is sound, set its `epoch` field to the current Unix time in seconds (edit the field in place with a tool such as `jq`, without printing the file), then upload the result as `current.json`. Raising the epoch keeps every read token issued so far revoked; uploading the file unchanged would bring back to life tokens that a later replace had revoked. The user gets back the version in it. If that is older than the damaged one, the user loses only the last change, which is normally a change of password or of recovery code, and they open the vault with the earlier one.
5. If `previous.json` is missing or damaged too, the record cannot be recovered here. Do **not** delete what is left in order to make the error go away: with both objects gone the account reads as new, and the UI would offer the user first-time set-up, which creates a fresh master key and strands their existing data. Restore both objects from the bucket backup, or leave the account answering 503 and escalate.
6. Delete the downloaded copies, start the service, and ask the user to sign in, which issues a new read token.

### Removing a record on request

Version 1 has no call by which a user deletes their own vault, so an operator does it.

1. Confirm that the request comes from the account's owner. Removing the record is permanent: without their wrapped vault the user can never recover their master key from this portal.
2. Stop the service, or be sure the account is idle.
3. Delete **both** objects, `current.json` and `previous.json`. Deleting only `current.json` leaves a damaged record, and the account answers 503 from then on.
4. Start the service. The account now reads as having no vault. Read tokens issued earlier stop working, and they stay dead even if the user creates a new vault later.

## Source layout

| Module | Role |
| --- | --- |
| `src/config.ts` | Reads and checks the environment. |
| `src/message.ts` | Builds the signed message and the typed data. Pure. |
| `src/challenge.ts` | In-memory store of live challenges. |
| `src/signature.ts` | On-chain signature check. The only module that imports viem. |
| `src/token.ts` | Read tokens. |
| `src/store.ts` | Storage interface, record format, in-memory store, per-account lock with its timeout rule. |
| `src/store-s3.ts` | S3 store. The only module that imports the S3 client. |
| `src/ratelimit.ts` | Client keys, and counts per client per minute. |
| `src/handlers.ts` | The five calls, as pure functions over injected parts. |
| `src/server.ts` | `node:http` server, routing, limits, CORS, start-up and shutdown. |

`src/challenge.ts` and `src/signature.ts` are ported from `services/fiat-service/src/lib/fiat-challenge.ts` and `fiat-signature.ts`, with their tests. The one deliberate difference in behaviour is the rule on when a nonce is consumed, described above.

## Not in version 1

Deleting a vault by the user, per-device revocation of read tokens, and running several instances.
