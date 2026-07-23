// FC1.2 — the session-open service: the ONE path from a fiat user to vault
// money. Order is the security property (Decision 3): credential, then the
// gatekeeper hold (atomic in the ledger), and only then the chain; a failed
// create releases the hold to the penny. A create that succeeds but cannot be
// bound is a divergence alarm and escapes loudly — never a silent release.
import { join } from 'node:path';
import { FiatCredentials } from './fiat-credentials';
import { makeVaultChain, signSessionAuthorisation, type SessionAuthorisation } from './fiat-vault';
import { gatekeeperConfigFromEnv, makeGatekeeper, type GateRefusal, type Gatekeeper } from './gatekeeper';
import { CreditsLedger, JsonlLedgerStore } from './ledger';

export interface FiatSessionRequest {
  credential: string;
  host: string;
  modelId: string;
  depositMicro: bigint;
  /** The burner that will connect over WS — bound into the FC1.6 authorisation. */
  clientAddress: string;
}

export type FiatSessionOutcome =
  | { status: 'ok'; sessionId: bigint; jobId: bigint; authorisation: SessionAuthorisation }
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
    /** Fired with the tx hash on submit, before confirmation (M2 orphan marker). */
    onSubmitted?: (txHash: string) => Promise<void>;
  }): Promise<{ jobId: bigint; depositor: string; txHash: string }>;
}

export interface FiatSessionDeps {
  ledger: CreditsLedger;
  credentials: FiatCredentials;
  gatekeeper: Gatekeeper;
  chain: FiatChain;
  signAuth: (sessionId: bigint, clientAddress: string) => SessionAuthorisation;
}

export async function openFiatSession(
  deps: FiatSessionDeps,
  request: FiatSessionRequest
): Promise<FiatSessionOutcome> {
  const userId = deps.credentials.authenticate(request.credential);
  if (!userId) return { status: 'unauthorised' };

  const open = await deps.ledger.openHold(
    { userId, host: request.host, depositMicro: request.depositMicro },
    deps.gatekeeper
  );
  if (!open.ok) return { status: 'refused', reason: open.reason };

  let created: { jobId: bigint; depositor: string; txHash: string };
  try {
    await deps.chain.ensureAllowance(request.depositMicro);
    created = await deps.chain.createSession({
      host: request.host,
      modelId: request.modelId,
      depositMicro: request.depositMicro,
      // Durably mark the pending create the instant it is submitted, so a crash
      // during confirmation leaves a recoverable orphan the R5 job can bind (M2).
      onSubmitted: (txHash) => deps.ledger.recordCreatePending(open.holdId, txHash),
    });
  } catch (e) {
    await deps.ledger.releaseHold(open.holdId);
    return { status: 'chain_error', message: e instanceof Error ? e.message : String(e) };
  }

  // A crash between the create tx confirming (above) and this bind leaves the
  // hold `held` but unbound — a recoverable orphan (M2). The R5 reconciliation
  // job matches ledger.unboundHolds() to on-chain vault-depositor creation
  // events by host+deposit and binds them; no refund is lost, only deferred.
  await deps.ledger.bindSession(open.holdId, created.jobId);
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

function buildBackend(): Promise<{ deps: FiatSessionDeps; service: FiatSessionService }> {
  // A PROMISE on globalThis (not the built value): if two bundles boot
  // concurrently, both await the same construction — one ledger, ever.
  g.__fiatBackend ??= (async () => {
    // The journals ARE the users' money (R5): git-ignored data dir, back it up.
    // (makeVaultChain() below throws the "not configured" error when the vault
    // key is unset — one authority for that message.)
    const dataDir = process.env.FIAT_DATA_DIR ?? './data/fiat';
    const deps: FiatSessionDeps = {
      ledger: await CreditsLedger.open(new JsonlLedgerStore(join(dataDir, 'ledger.jsonl'))),
      credentials: await FiatCredentials.open(new JsonlLedgerStore(join(dataDir, 'credentials.jsonl'))),
      gatekeeper: makeGatekeeper(gatekeeperConfigFromEnv()),
      chain: makeVaultChain(),
      signAuth: signSessionAuthorisation,
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
