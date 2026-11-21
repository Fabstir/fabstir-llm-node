// Spec section 3: the five calls, as pure functions over injected
// configuration, store, challenge store, verifier, clock and logger. No HTTP,
// no globals, no clock of their own.
//
// The order inside open and replace is the security of the service:
//
//   1. shape checks (400), chain (R4), size (413);
//   2. PEEK the nonce: it must be live and issued for this address, this chain
//      and this purpose (R3), else 401 challenge-unknown. Nothing is consumed;
//   3. rebuild the signed text from the STORED challenge and, for a replace,
//      from the SHA-256 this service computes over the vault text it received,
//      and from `replaces`. No text from the caller is ever verified against;
//   4. verify on-chain. Unavailable is 503, invalid is 401; in both cases the
//      nonce stays live (R2);
//   5. only now CONSUME the nonce. Consumption is atomic, so of two verified
//      requests racing on one nonce exactly one goes on;
//   6. read or write the record, serialised per account (R6).
//
// v0.2: storage work runs under the account's lock WITH a timeout: the caller
// is answered 503 when it passes, the lock stays held until the storage call
// has really settled, and until then everything for that account is refused
// (R6a). A missing current.json beside an existing previous.json is a damaged
// record, 503, never "no vault" (R6c). Challenges and verifications are
// limited per client, never per account (R7).
import { createHash } from 'node:crypto';
import { ChallengeClientLimitError, ChallengeStoreFullError, type Challenge, type ChallengeStore } from './challenge.js';
import { NONCE_RE, SHA256_HEX_RE, buildMessage, buildTypedData, isPurpose, type ChallengeFields, type Purpose } from './message.js';
import { isWellFormedSignature, type OwnershipProof, type SignatureVerifier } from './signature.js';
import type { RateLimiter } from './ratelimit.js';
import { CorruptRecordError, StorageTimeoutError, storageFailureKind, type AccountLocks, type LockContext, type VaultRecord, type VaultStore } from './store.js';
import { epochIsCurrent, issueReadToken, verifyReadToken } from './token.js';

export interface HandlerConfig {
  region: string;
  portal: string;
  /** The one configured chain (R4). */
  chainId: number;
  tokenSecret: Buffer;
  readTokenSeconds: number;
  maxVaultBytes: number;
  /** How long a caller waits for the storage before it is answered 503. */
  storageTimeoutMs: number;
}

/** R9: address, purpose, outcome and timing; v0.2: the kind of a storage
 *  failure and, on a 429, the client key. There is deliberately no field that
 *  could hold a vault, a signature, a nonce or a token. */
export interface LogEntry {
  call: string;
  address?: string;
  purpose?: Purpose;
  /** "ok" or the error code returned. */
  outcome: string;
  status: number;
  ms: number;
  /** Storage failure: `timeout`, `unsettled`, `corrupt`, or the S3 error code. */
  kind?: string;
  /** On a 429: the client key the limit was counted by. */
  client?: string;
}

export interface HandlerDeps {
  config: HandlerConfig;
  store: VaultStore;
  challenges: ChallengeStore;
  verify: SignatureVerifier;
  /** R7: signature verifications per client per minute. */
  verifyLimiter: RateLimiter;
  /** Epoch milliseconds. */
  now: () => number;
  log: (entry: LogEntry) => void;
  locks: AccountLocks;
}

export interface HandlerResult {
  status: number;
  body: Record<string, unknown>;
  headers?: Record<string, string>;
}

const RETRY_AFTER_SECONDS = '5';
const ADDRESS_RE = /^0x[0-9a-fA-F]{40}$/;
const CHAIN_ID_TEXT_RE = /^[1-9][0-9]{0,15}$/;

function fail(status: number, error: string, extra: Record<string, unknown> = {}, headers?: Record<string, string>): HandlerResult {
  return headers ? { status, body: { error, ...extra }, headers } : { status, body: { error, ...extra } };
}

const invalidRequest = () => fail(400, 'invalid-request');
const unsupportedChain = () => fail(400, 'unsupported-chain');
const challengeUnknown = () => fail(401, 'challenge-unknown');

/** A storage failure: 503, with its kind noted for the log (never its message). */
function storageUnavailable(meta: Meta, err: unknown): HandlerResult {
  meta.kind = storageFailureKind(err);
  return fail(503, 'storage-unavailable', {}, { 'Retry-After': RETRY_AFTER_SECONDS });
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isChainIdNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
}

function isVersionNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function sha256Hex(text: string): string {
  return createHash('sha256').update(text, 'utf8').digest('hex');
}

interface Meta {
  address?: string;
  purpose?: Purpose;
  kind?: string;
}

/** Runs one call, then writes its one log line. */
async function logged(deps: HandlerDeps, call: string, client: string | undefined, fn: (meta: Meta) => Promise<HandlerResult>): Promise<HandlerResult> {
  const started = deps.now();
  const meta: Meta = {};
  let result: HandlerResult;
  try {
    result = await fn(meta);
  } catch {
    // Nothing of the error is logged or returned: it could carry request data.
    result = fail(500, 'internal-error');
  }
  const outcome = result.status === 200 ? 'ok' : typeof result.body.error === 'string' ? result.body.error : 'error';
  const entry: LogEntry = { call, outcome, status: result.status, ms: Math.max(0, deps.now() - started) };
  if (meta.address !== undefined) entry.address = meta.address;
  if (meta.purpose !== undefined) entry.purpose = meta.purpose;
  if (meta.kind !== undefined) entry.kind = meta.kind;
  if (result.status === 429 && client !== undefined) entry.client = client;
  deps.log(entry);
  return result;
}

function fieldsFor(config: HandlerConfig, challenge: Challenge, replace?: { vaultSha256: string; replaces: number }): ChallengeFields {
  const fields: ChallengeFields = {
    purpose: challenge.purpose,
    region: config.region,
    portal: config.portal,
    address: challenge.address,
    chainId: challenge.chainId,
    nonce: challenge.nonce,
    expires: new Date(challenge.expiresAt).toISOString(),
  };
  if (replace) {
    fields.vaultSha256 = replace.vaultSha256;
    fields.replaces = replace.replaces;
  }
  return fields;
}

// ---------------------------------------------------------------------------
// 3.1 POST /vault/v1/challenge
// ---------------------------------------------------------------------------

export function handleChallenge(deps: HandlerDeps, body: unknown, client: string = 'unknown'): Promise<HandlerResult> {
  return logged(deps, 'challenge', client, async (meta) => {
    if (!isObject(body)) return invalidRequest();
    const { address, chainId, purpose, vaultSha256, replaces } = body;
    if (typeof address !== 'string' || !ADDRESS_RE.test(address)) return invalidRequest();
    meta.address = address.toLowerCase();
    if (!isChainIdNumber(chainId)) return invalidRequest();
    if (!isPurpose(purpose)) return invalidRequest();
    meta.purpose = purpose;

    let replace: { vaultSha256: string; replaces: number } | undefined;
    if (purpose === 'replace') {
      if (typeof vaultSha256 !== 'string' || !SHA256_HEX_RE.test(vaultSha256)) return invalidRequest();
      if (!isVersionNumber(replaces)) return invalidRequest();
      replace = { vaultSha256, replaces };
    } else if (vaultSha256 !== undefined || replaces !== undefined) {
      return invalidRequest();
    }
    if (chainId !== deps.config.chainId) return unsupportedChain();

    let challenge: Challenge;
    try {
      challenge = deps.challenges.issue(address, chainId, purpose, client);
    } catch (err) {
      // The client's own share (R7) or the global cap: the same answer.
      if (err instanceof ChallengeClientLimitError || err instanceof ChallengeStoreFullError) {
        return fail(429, 'too-many-challenges', {}, { 'Retry-After': RETRY_AFTER_SECONDS });
      }
      throw err;
    }

    // The hash and the version are not stored: message and typed data are
    // computed from the stored challenge plus the values in this request.
    const fields = fieldsFor(deps.config, challenge, replace);
    const out: Record<string, unknown> = {
      purpose: fields.purpose,
      region: fields.region,
      portal: fields.portal,
      address: fields.address,
      chainId: fields.chainId,
      nonce: fields.nonce,
      expires: fields.expires,
    };
    if (replace) {
      out.vaultSha256 = replace.vaultSha256;
      out.replaces = replace.replaces;
    }
    out.message = buildMessage(fields);
    out.typedData = buildTypedData(fields);
    return { status: 200, body: out };
  });
}

// ---------------------------------------------------------------------------
// Shared by open and replace: steps 2 to 5 of the header comment.
// ---------------------------------------------------------------------------

interface SignedRequest {
  address: string; // lower-case
  nonce: string;
  signature: string;
  form: 'message' | 'typedData';
}

/** Checks shared by open and replace on the request body. Returns the parsed
 *  request, or the refusal. Sets the log's address as soon as it is known. */
function parseSigned(deps: HandlerDeps, body: Record<string, unknown>, meta: Meta): SignedRequest | HandlerResult {
  const { address, chainId, nonce, signature, form } = body;
  if (typeof address !== 'string' || !ADDRESS_RE.test(address)) return invalidRequest();
  meta.address = address.toLowerCase();
  if (!isChainIdNumber(chainId)) return invalidRequest();
  if (typeof nonce !== 'string' || !NONCE_RE.test(nonce)) return invalidRequest();
  if (!isWellFormedSignature(signature)) return invalidRequest();
  if (form !== 'message' && form !== 'typedData') return invalidRequest();
  if (chainId !== deps.config.chainId) return unsupportedChain();
  return { address: meta.address, nonce, signature, form };
}

function isRefusal(value: SignedRequest | HandlerResult): value is HandlerResult {
  return 'status' in value;
}

/** Peek, rebuild, verify, consume. Returns null when the caller has proved
 *  control of the wallet and the nonce is now spent; a refusal otherwise. */
async function authorise(
  deps: HandlerDeps,
  client: string,
  request: SignedRequest,
  purpose: Purpose,
  replace?: { vaultSha256: string; replaces: number }
): Promise<HandlerResult | null> {
  // R3: the nonce must be live AND issued for this address, chain and purpose.
  // A read nonce can never authorise a replace, whatever the signature says.
  const challenge = deps.challenges.peek(request.nonce);
  if (
    !challenge ||
    challenge.address !== request.address ||
    challenge.chainId !== deps.config.chainId ||
    challenge.purpose !== purpose
  ) {
    return challengeUnknown();
  }

  // R7: verifications per CLIENT per minute. Counted only here, for a request
  // that is about to make the chain call. Never per account: a stranger could
  // then spend the owner's allowance. The nonce stays live.
  const rate = deps.verifyLimiter.check(client);
  if (!rate.allowed) return fail(429, 'rate-limited', {}, { 'Retry-After': String(rate.retryAfterSeconds) });

  // The signed text is rebuilt here from what the server stored and computed.
  const fields = fieldsFor(deps.config, challenge, replace);
  const proof: OwnershipProof =
    request.form === 'typedData'
      ? { address: request.address, typedData: buildTypedData(fields), signature: request.signature }
      : { address: request.address, message: buildMessage(fields), signature: request.signature };

  let verified: boolean;
  try {
    verified = await deps.verify(proof);
  } catch {
    // R1: the chain could not be asked, or too many checks are in flight
    // (SignatureCheckUnavailableError). Never reported as invalid. Any other failure of the check is treated the same
    // way: the question is unanswered. R2: the nonce stays live.
    return fail(503, 'signature-check-unavailable', {}, { 'Retry-After': RETRY_AFTER_SECONDS });
  }
  if (verified !== true) return fail(401, 'signature-invalid'); // R2: the nonce stays live

  // R2: consumed only now, by a request whose signature verified.
  if (!deps.challenges.consume(request.nonce)) return challengeUnknown();
  return null;
}

function lockKey(chainId: number, address: string): string {
  return `${chainId}|${address}`;
}

/** The current record, or null when the account has none. R6c: a missing
 *  current.json beside an existing previous.json is a DAMAGED record and
 *  throws; it must never read as "no vault", or the UI would lead the user
 *  into first-time set-up over it. Call under the account's lock. */
async function readCurrentChecked(store: VaultStore, chainId: number, address: string): Promise<VaultRecord | null> {
  const current = await store.readCurrent(chainId, address);
  if (current) return current;
  const previous = await store.readPrevious(chainId, address);
  if (previous) throw new CorruptRecordError('current record is missing while a previous one exists');
  return null;
}

function newReadToken(deps: HandlerDeps, record: VaultRecord): { readToken: string; readTokenExpires: string } {
  const expiresAtSeconds = Math.floor(deps.now() / 1000) + deps.config.readTokenSeconds;
  return {
    readToken: issueReadToken(deps.config.tokenSecret, {
      region: deps.config.region,
      portal: deps.config.portal,
      address: record.address,
      chainId: record.chainId,
      epoch: record.epoch,
      expiresAtSeconds,
    }),
    readTokenExpires: new Date(expiresAtSeconds * 1000).toISOString(),
  };
}

// ---------------------------------------------------------------------------
// 3.2 POST /vault/v1/open
// ---------------------------------------------------------------------------

export function handleOpen(deps: HandlerDeps, body: unknown, client: string = 'unknown'): Promise<HandlerResult> {
  return logged(deps, 'open', client, async (meta) => {
    meta.purpose = 'read';
    if (!isObject(body)) return invalidRequest();
    const request = parseSigned(deps, body, meta);
    if (isRefusal(request)) return request;

    const refusal = await authorise(deps, client, request, 'read');
    if (refusal) return refusal;

    let record: VaultRecord | null;
    try {
      record = await deps.locks.run(
        lockKey(deps.config.chainId, request.address),
        () => readCurrentChecked(deps.store, deps.config.chainId, request.address),
        deps.config.storageTimeoutMs
      );
    } catch (err) {
      return storageUnavailable(meta, err);
    }
    if (!record) return fail(404, 'no-vault');
    return {
      status: 200,
      body: { version: record.version, updatedAt: record.updatedAt, vault: record.vault, ...newReadToken(deps, record) },
    };
  });
}

// ---------------------------------------------------------------------------
// 3.3 GET /vault/v1/current
// ---------------------------------------------------------------------------

export interface CurrentRequest {
  address: string | undefined;
  /** The query value, as text. */
  chainId: string | undefined;
  /** The Authorization header, as received. */
  authorization: string | undefined;
}

function bearerToken(authorization: string | undefined): string | null {
  if (typeof authorization !== 'string') return null;
  const m = /^Bearer ([A-Za-z0-9._-]+)$/i.exec(authorization);
  return m ? m[1]! : null;
}

export function handleCurrent(deps: HandlerDeps, request: CurrentRequest): Promise<HandlerResult> {
  return logged(deps, 'current', undefined, async (meta) => {
    meta.purpose = 'read';
    if (typeof request.address !== 'string' || !ADDRESS_RE.test(request.address)) return invalidRequest();
    const address = request.address.toLowerCase();
    meta.address = address;
    if (typeof request.chainId !== 'string' || !CHAIN_ID_TEXT_RE.test(request.chainId)) return invalidRequest();
    const chainId = Number(request.chainId);
    if (chainId !== deps.config.chainId) return unsupportedChain();

    const tokenInvalid = () => fail(401, 'token-invalid');
    const token = bearerToken(request.authorization);
    if (token === null) return tokenInvalid();
    // MAC, expiry, region, host, address and chain first: storage is not
    // touched for a bad token.
    const claims = verifyReadToken(deps.config.tokenSecret, token, {
      region: deps.config.region,
      portal: deps.config.portal,
      address,
      chainId,
      nowSeconds: Math.floor(deps.now() / 1000),
    });
    if (!claims) return tokenInvalid();

    let record: VaultRecord | null;
    try {
      record = await deps.locks.run(
        lockKey(chainId, address),
        () => readCurrentChecked(deps.store, chainId, address),
        deps.config.storageTimeoutMs
      );
    } catch (err) {
      return storageUnavailable(meta, err);
    }
    if (!record) return fail(404, 'no-vault');
    // R5: the epoch must equal the stored record's; a replace revokes.
    if (!epochIsCurrent(claims, record.epoch)) return tokenInvalid();
    return { status: 200, body: { version: record.version, updatedAt: record.updatedAt, vault: record.vault } };
  });
}

// ---------------------------------------------------------------------------
// 3.4 POST /vault/v1/replace
// ---------------------------------------------------------------------------

export function handleReplace(deps: HandlerDeps, body: unknown, client: string = 'unknown'): Promise<HandlerResult> {
  return logged(deps, 'replace', client, async (meta) => {
    meta.purpose = 'replace';
    if (!isObject(body)) return invalidRequest();
    const request = parseSigned(deps, body, meta);
    if (isRefusal(request)) return request;
    const { vault, replaces } = body;
    if (typeof vault !== 'string' || vault.length === 0) return invalidRequest();
    if (!isVersionNumber(replaces)) return invalidRequest();
    if (Buffer.byteLength(vault, 'utf8') > deps.config.maxVaultBytes) return fail(413, 'vault-too-large');

    // R3: the signature must cover the hash of the text RECEIVED, computed
    // here, and the version replaced. A hash supplied by the caller is ignored.
    const vaultSha256 = sha256Hex(vault);
    const refusal = await authorise(deps, client, request, 'replace', { vaultSha256, replaces });
    if (refusal) return refusal;

    const chainId = deps.config.chainId;
    try {
      // R6a: the lock is held until this whole function has settled, even if
      // the caller was answered 503 at the timeout; meanwhile nothing else is
      // accepted for the account, so a late write here can never land on top
      // of a newer acknowledged one.
      return await deps.locks.run(lockKey(chainId, request.address), async (ctx: LockContext): Promise<HandlerResult> => {
        const existing = await readCurrentChecked(deps.store, chainId, request.address);
        const currentVersion = existing ? existing.version : 0;
        if (currentVersion !== replaces) return fail(409, 'version-conflict', { currentVersion });

        const nowMs = deps.now();
        const next: VaultRecord = {
          v: 1,
          address: request.address,
          chainId,
          version: replaces + 1,
          // Every replace increases the epoch (R5). Taking at least the clock
          // in seconds also keeps it increasing across a record that an
          // operator removed and the user created again, so a token from
          // before the removal cannot come back to life.
          epoch: Math.max((existing ? existing.epoch : 0) + 1, Math.floor(nowMs / 1000)),
          updatedAt: new Date(nowMs).toISOString(),
          vault,
        };
        // R6: previous.json (the old current) first, then current.json.
        // If the caller has already been told 503, write nothing for nobody.
        if (ctx.abandoned) throw new StorageTimeoutError();
        if (existing) await deps.store.writePrevious(existing);
        if (ctx.abandoned) throw new StorageTimeoutError();
        await deps.store.writeCurrent(next);
        return { status: 200, body: { version: next.version, updatedAt: next.updatedAt, ...newReadToken(deps, next) } };
      }, deps.config.storageTimeoutMs);
    } catch (err) {
      return storageUnavailable(meta, err);
    }
  });
}

// ---------------------------------------------------------------------------
// 3.5 GET /vault/v1/health
// ---------------------------------------------------------------------------

export function handleHealth(deps: HandlerDeps): Promise<HandlerResult> {
  return logged(deps, 'health', undefined, async () => {
    let storage: 'ok' | 'error' = 'error';
    try {
      storage = (await deps.store.ping()) ? 'ok' : 'error';
    } catch {
      storage = 'error';
    }
    return {
      status: 200,
      body: { status: 'ok', region: deps.config.region, portal: deps.config.portal, chainId: deps.config.chainId, storage },
    };
  });
}
