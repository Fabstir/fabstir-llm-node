// FC1.2 — the session-open service: the ONE path from a fiat user to vault
// money. Order is the security property (Decision 3): credential, then the
// gatekeeper hold (atomic in the ledger), and only then the chain; a failed
// create releases the hold to the penny. A create that succeeds but cannot be
// bound is a divergence alarm and escapes loudly — never a silent release.
import { join } from 'node:path';
import { sessionShapeFor } from './escrow';
import { FiatCredentials } from './fiat-credentials';
import {
  allowanceFloatMicro,
  makeVaultChain,
  signSessionAuthorisation,
  type SessionAuthorisation,
} from './fiat-vault';
import {
  describeGatekeeperConfig,
  gatekeeperConfigFromEnv,
  largestSessionCapMicro,
  makeGatekeeper,
  type GatekeeperConfig,
  type GateRefusal,
  type Gatekeeper,
  type SessionKind,
} from './gatekeeper';
import { IdempotencyStore, requestFingerprint } from './idempotency';
import { CreditsLedger, JsonlLedgerStore } from './ledger';

export interface FiatSessionRequest {
  credential: string;
  host: string;
  modelId: string;
  depositMicro: bigint;
  /** The burner that will connect over WS — bound into the FC1.6 authorisation. */
  clientAddress: string;
  /** FC2.8: caller-supplied retry key. Same (user, key) => the SAME escrow, ever.
   *  Absent = no dedupe (unchanged behaviour for callers that do not send one). */
  idempotencyKey?: string;
  /** The job kind. Selects the SERVICE-OWNED on-chain shape and the per-kind
   *  caps; absent = standard (chat/render, today's behaviour). A training run
   *  needs the 14400 / 1000 / 3600 shape or it fails the node's accept gate
   *  AFTER escrow, which is why the browser names a kind and never numbers. */
  kind?: SessionKind;
}

export type FiatSessionOutcome =
  | { status: 'ok'; sessionId: bigint; jobId: bigint; authorisation: SessionAuthorisation; replayed?: true }
  /** The same key is mid-flight: the chain call may be in the air, so the honest
   *  answer is "wait and ask again", never a second escrow. */
  | { status: 'in_flight' }
  /** The key was first used for a DIFFERENT request: a client bug, surfaced. */
  | { status: 'key_conflict' }
  | { status: 'unauthorised' }
  | { status: 'refused'; reason: GateRefusal }
  | { status: 'chain_error'; message: string };

/** The chain surface the service needs (structural subset of VaultChain). */
export interface FiatChain {
  ensureAllowance(depositMicro: bigint): Promise<void>;
  createSession(params: {
    host: string;
    modelId: string;
    depositMicro: bigint;
    kind?: SessionKind;
    /** The price this service already read from the registry for this open,
     *  pinned so the create does not read it a second time. */
    pricePerToken?: bigint;
    /** Fired with the tx hash on submit, before confirmation (M2 orphan marker). */
    onSubmitted?: (txHash: string) => Promise<void>;
  }): Promise<{ jobId: bigint; depositor: string; txHash: string }>;
  /** Registry price for (host, model); 0n = not advertised, throw = unreadable.
   *  Optional so a chain fake without it behaves exactly as before. */
  modelPrice?(host: string, modelId: string): Promise<bigint>;
}

export interface FiatSessionDeps {
  ledger: CreditsLedger;
  credentials: FiatCredentials;
  gatekeeper: Gatekeeper;
  chain: FiatChain;
  signAuth: (sessionId: bigint, clientAddress: string) => SessionAuthorisation;
  /** FC2.8 retry keys. Optional so existing tests and callers are unaffected. */
  idempotency?: IdempotencyStore;
  /** FT1 D4 — per-user ATTEMPTS-per-minute budget, applied before any chain
   *  read and before the idempotency reserve. Optional so existing tests and
   *  callers are unaffected (no bucket = no attempts limit, as before). */
  attempts?: AttemptsBucket;
}

/**
 * FT1 D4 — attempts of EVERY outcome per user per minute. The ledger's own
 * per-minute counter counts HOLDS, so refusals never touched it and a free
 * self-serve credential could drive the model-price read at line rate against
 * the RPC settlement depends on. SYNCHRONOUS by contract: `tryTake` must
 * decide before any await, or two duplicate-key requests could both reserve.
 * In-memory (single instance is a stated constraint); a restart resets it.
 */
export class AttemptsBucket {
  private readonly rows = new Map<string, number[]>();
  constructor(
    private readonly limit: number,
    private readonly now: () => number = Date.now
  ) {}

  /** Take one attempt for `userId`; false when the minute's budget is spent. */
  tryTake(userId: string): boolean {
    const t = this.now();
    const recent = (this.rows.get(userId) ?? []).filter((at) => t - at < 60_000);
    if (recent.length >= this.limit) {
      this.rows.set(userId, recent);
      return false;
    }
    recent.push(t);
    this.rows.set(userId, recent);
    return true;
  }
}

/**
 * FT1 D5 — the boot invariant: no ENABLED kind may have a per-session cap
 * above the approve float, because `ensureAllowance` refuses a deposit above
 * the float and such a session could never open. Pure so it is testable
 * without a vault key; `buildBackend` calls it at construction. With
 * FIAT_SETTLEMENT_ENABLED=1 a throw here leaves the process up with every
 * route answering 500 until restart (Next memoises the rejected prepare), so
 * FIAT_TRAINING_MODEL_IDS and the raised float go into the env in ONE edit.
 */
export function assertBootInvariants(config: GatekeeperConfig, floatMicro: bigint): void {
  const largest = largestSessionCapMicro(config);
  if (largest > floatMicro) {
    // Name the offender: the standard cap seeds the maximum, so a standard cap
    // above the float refuses to boot too (at HEAD it merely chain_error'd
    // every open above the float).
    const offender =
      config.maxDepositPerSessionMicro >= largest
        ? 'FIAT_MAX_SESSION_DEPOSIT_MICRO'
        : 'FIAT_TRAINING_MAX_SESSION_DEPOSIT_MICRO (enabled by FIAT_TRAINING_MODEL_IDS)';
    throw new Error(
      `FIAT_VAULT_ALLOWANCE_FLOAT_MICRO (${floatMicro}) is below the largest ENABLED per-kind session cap (${largest}, from ${offender}) — raise the float deliberately, in the same edit`
    );
  }
  // A bad standard proof-window knob is a boot error, not a per-open chain_error.
  sessionShapeFor('standard');
}

export async function openFiatSession(
  deps: FiatSessionDeps,
  request: FiatSessionRequest
): Promise<FiatSessionOutcome> {
  const userId = deps.credentials.authenticate(request.credential);
  if (!userId) return { status: 'unauthorised' };

  // FC2.8 — retry key. Checked AFTER authentication (an unauthenticated caller
  // learns nothing about anyone's keys) and BEFORE the hold, so a replay never
  // touches the gatekeeper or the chain.
  const key = request.idempotencyKey;
  const fingerprint = key
    ? requestFingerprint({
        host: request.host,
        modelId: request.modelId,
        depositMicro: request.depositMicro,
        clientAddress: request.clientAddress,
        ...(request.kind ? { kind: request.kind } : {}),
      })
    : undefined;
  if (key && deps.idempotency) {
    const prior = await deps.idempotency.lookup(userId, key);
    if (prior) {
      if (prior.fingerprint !== fingerprint) return { status: 'key_conflict' };
      if (prior.state === 'pending') return { status: 'in_flight' };
      // The authorisation is re-derived, not stored: the signature over
      // (sessionId, clientAddress) is deterministic, so the replay is
      // byte-identical to the original reply without keeping a secret at rest.
      return {
        status: 'ok',
        sessionId: prior.jobId,
        jobId: prior.jobId,
        authorisation: deps.signAuth(prior.jobId, prior.clientAddress),
        replayed: true,
      };
    }
  }

  // FT1 D4 — the attempts budget, BEFORE the reserve (a rate-limited attempt
  // writes nothing) and before anything that could reach the chain.
  if (deps.attempts && !deps.attempts.tryTake(userId)) {
    return { status: 'refused', reason: 'RATE_LIMITED' };
  }
  if (key && deps.idempotency) {
    // Claim the key BEFORE anything can spend, so a crash mid-create leaves it
    // pending and the retry is refused rather than charging twice.
    await deps.idempotency.reserve(userId, key, fingerprint!);
  }
  // Every outcome below that journals NO hold frees the key: a reserved key
  // would otherwise answer `in_flight` for 24 h to exactly the retry that
  // INSUFFICIENT_BALANCE-then-purchase or a chain_error invites.
  const freeKey = async () => {
    if (key && deps.idempotency) await deps.idempotency.release(userId, key);
  };

  const holdRequest = {
    userId,
    host: request.host,
    depositMicro: request.depositMicro,
    modelId: request.modelId,
    ...(request.kind ? { kind: request.kind } : {}),
  };

  // FT1 D4 — the gatekeeper PREVIEW: every policy refusal (allow-list, kind↔model
  // binding, caps, balance, daily window, concurrency, rate) precedes any chain
  // read, so an off-list host still reads HOST_NOT_ALLOWED and learns nothing,
  // and a refusal costs the user nothing. Advice only: `openHold` re-decides
  // under the ledger mutex below and is the authority.
  const preview = await deps.ledger.previewOpen(holdRequest, deps.gatekeeper);
  if (!preview.allow) {
    await freeKey();
    return { status: 'refused', reason: preview.reason };
  }

  // The unpriced-model check, BEFORE the hold and before any chain write. A
  // host that has not advertised this model can never open a session for it,
  // so the honest answer is a policy refusal the client must not retry — not
  // the chain_error the create would have thrown, which it would. A failed
  // READ stays chain_error: that one may well succeed on retry. Neither
  // journals a hold. The answer is pinned into the create (one read per open).
  let pricePerToken: bigint | undefined;
  if (deps.chain.modelPrice) {
    try {
      pricePerToken = await deps.chain.modelPrice(request.host, request.modelId);
    } catch (e) {
      await freeKey();
      return { status: 'chain_error', message: e instanceof Error ? e.message : String(e) };
    }
    if (pricePerToken <= 0n) {
      await freeKey();
      return { status: 'refused', reason: 'MODEL_NOT_PRICED' };
    }
  }

  const open = await deps.ledger.openHold(holdRequest, deps.gatekeeper);
  if (!open.ok) {
    // Refused by policy under the mutex (the preview was advice): nothing was
    // escrowed, so the key must not stay claimed.
    await freeKey();
    return { status: 'refused', reason: open.reason };
  }

  let created: { jobId: bigint; depositor: string; txHash: string };
  try {
    await deps.chain.ensureAllowance(request.depositMicro);
    created = await deps.chain.createSession({
      host: request.host,
      modelId: request.modelId,
      depositMicro: request.depositMicro,
      ...(request.kind ? { kind: request.kind } : {}),
      ...(pricePerToken !== undefined ? { pricePerToken } : {}),
      // Durably mark the pending create the instant it is submitted, so a crash
      // during confirmation leaves a recoverable orphan the R5 job can bind (M2).
      onSubmitted: (txHash) => deps.ledger.recordCreatePending(open.holdId, txHash),
    });
  } catch (e) {
    await deps.ledger.releaseHold(open.holdId);
    // The create threw, so the hold is released and no session exists; free the
    // key too, or an honest retry would be refused by our own bookkeeping. The
    // ambiguous case (create submitted, confirmation lost) does NOT land here:
    // it leaves the key pending and is reconciled as an orphan (M2/R5).
    await freeKey();
    return { status: 'chain_error', message: e instanceof Error ? e.message : String(e) };
  }

  // A crash between the create tx confirming (above) and this bind leaves the
  // hold `held` but unbound — a recoverable orphan (M2). The R5 reconciliation
  // job matches ledger.unboundHolds() to on-chain vault-depositor creation
  // events by host+deposit and binds them; no refund is lost, only deferred.
  //
  // That recovery is also a competitor. The reconcile sweep runs on the
  // settlement listener's tick, so when a tick lands inside our confirmation
  // wait it resolves the same receipt and binds first — and bindSession is
  // deliberately strict about rebinding (see the adversarial guard), so this
  // call then threw and the route 500'd on a session that had been created and
  // paid for. Live on 2026-07-26 as job 987: the customer saw a failure, the
  // deposit was spent, and the session was stranded. Losing this race is a
  // success, not an error — the bind we wanted is exactly the one that already
  // happened. Anything else still propagates.
  try {
    await deps.ledger.bindSession(open.holdId, created.jobId);
  } catch (e) {
    if (deps.ledger.holdForJob(created.jobId) !== open.holdId) throw e;
  }
  // Bind the key to the session that exists, so any retry replays THIS one.
  if (key && deps.idempotency) {
    await deps.idempotency.complete(userId, key, created.jobId, request.clientAddress);
  }
  return {
    status: 'ok',
    sessionId: created.jobId,
    jobId: created.jobId,
    authorisation: deps.signAuth(created.jobId, request.clientAddress),
  };
}

export interface FiatSessionService {
  open(request: FiatSessionRequest): Promise<FiatSessionOutcome>;
}

let overrideService: FiatSessionService | undefined;
let overrideDeps: Partial<FiatSessionDeps> | undefined;

// The built backend lives on globalThis, NOT module scope. Next compiles
// instrumentation.js and the route handlers into separate bundles, each with
// its own copy of module variables: a module-local here gave the settlement
// listener its OWN CreditsLedger instance over the same journal file — it
// loaded the journal at boot and never saw holds the routes bound afterwards
// (heartbeat: "boundJobs 0" while a live session's hold sat in the route
// bundle's copy). One process, ONE ledger, whichever bundle asks first.
const g = globalThis as typeof globalThis & {
  __fiatBackend?: Promise<{ deps: FiatSessionDeps; service: FiatSessionService }>;
};

/** Tests inject a stub; pass undefined to restore the env-built service. */
export function setFiatSessionServiceForTest(service: FiatSessionService | undefined): void {
  overrideService = service;
}

/** Tests inject shared deps (e.g. just a ledger for the webhook route). */
export function setFiatDepsForTest(deps: Partial<FiatSessionDeps> | undefined): void {
  overrideDeps = deps;
}

/** Tests only: forget a memoised (possibly rejected) backend build so the next
 *  `getFiatDeps()` builds again from the current env. */
export function resetFiatBackendForTest(): void {
  g.__fiatBackend = undefined;
}

function buildBackend(): Promise<{ deps: FiatSessionDeps; service: FiatSessionService }> {
  // A PROMISE on globalThis (not the built value): if two bundles boot
  // concurrently, both await the same construction — one ledger, ever.
  g.__fiatBackend ??= (async () => {
    // The journals ARE the users' money (R5): git-ignored data dir, back it up.
    // (makeVaultChain() below throws the "not configured" error when the vault
    // key is unset — one authority for that message.)
    const dataDir = process.env.FIAT_DATA_DIR ?? './data/fiat';
    // FT1 D5 — the boot invariants, BEFORE any journal is opened or the vault
    // is built: a cap above the float, or a bad proof-window knob, refuses to
    // boot (see assertBootInvariants for what that looks like in production).
    const config = gatekeeperConfigFromEnv();
    assertBootInvariants(config, allowanceFloatMicro());
    console.log(`[fiat] ${describeGatekeeperConfig(config)}`);
    const deps: FiatSessionDeps = {
      ledger: await CreditsLedger.open(new JsonlLedgerStore(join(dataDir, 'ledger.jsonl'))),
      credentials: await FiatCredentials.open(new JsonlLedgerStore(join(dataDir, 'credentials.jsonl'))),
      idempotency: await IdempotencyStore.open(new JsonlLedgerStore(join(dataDir, 'idempotency.jsonl'))),
      gatekeeper: makeGatekeeper(config),
      chain: makeVaultChain(),
      signAuth: signSessionAuthorisation,
      attempts: new AttemptsBucket(config.maxOpensPerMinute),
    };
    return { deps, service: { open: (request) => openFiatSession(deps, request) } };
  })();
  return g.__fiatBackend;
}

export async function getFiatSessionService(): Promise<FiatSessionService> {
  if (overrideService) return overrideService;
  return (await buildBackend()).service;
}

/** The FC1.3 listener and FC1.4 webhook must share THIS ledger instance. */
export async function getFiatDeps(): Promise<FiatSessionDeps> {
  if (overrideDeps) return overrideDeps as FiatSessionDeps;
  return (await buildBackend()).deps;
}
