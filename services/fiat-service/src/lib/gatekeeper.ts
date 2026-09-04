// FC1.1 — the gatekeeper (Decision 3, SECURITY-CRITICAL): the only policy that
// can authorise a vault spend. Pure decision over a ledger snapshot; the
// ledger calls it INSIDE its mutex (check and hold are atomic), so passing it
// around never opens a check-then-act race.
//
// Server-only module: config comes from server env, never NEXT_PUBLIC_*.

/** The job kinds a vault session can be opened for. `standard` is today's
 *  chat + render shape (an absent `kind` on the wire); `training` is a
 *  fine-tuning run, which lives hours and posts a proof per slice. The SERVICE
 *  owns the on-chain shape per kind (escrow.ts) and the caps per kind (here):
 *  the vault fronts the money, so a browser chooses neither. */
export type SessionKind = 'standard' | 'training';
export const SESSION_KINDS: readonly SessionKind[] = ['standard', 'training'];

export interface KindCaps {
  maxDepositPerSessionMicro: bigint;
  /** Rolling 24h per-user velocity cap over hold amounts OF THIS KIND. */
  maxDailySpendMicro: bigint;
}

export interface GatekeeperConfig {
  /** Managed-host allow-list (Decision 8). EMPTY by default: an unconfigured
   *  deployment refuses every fiat open rather than trusting any host. */
  allowedHosts: string[];
  maxDepositPerSessionMicro: bigint;
  /** Rolling 24h per-user velocity cap over hold amounts (Decision 8). */
  maxDailySpendMicro: bigint;
  maxOpensPerMinute: number;
  /** Per-kind caps (Decision 8: a per-model or per-kind cap, never a raised
   *  global). `standard` always uses the two global fields above; a kind
   *  absent here falls back to them, so an unconfigured kind is never LOOSER
   *  than standard. Each kind's daily window is its own budget: the ledger
   *  sums only holds of the request's kind into `spentInWindowMicro`. */
  perKind?: Partial<Record<Exclude<SessionKind, 'standard'>, KindCaps>>;
  /** FT1 D10 — the registered TRAINING model ids, compared lowercased. The
   *  binding is symmetric: `kind: "training"` needs one of these, and one of
   *  these needs `kind: "training"` (a chat-shaped session on the training
   *  model fails the node's accept gate AFTER escrow). EMPTY by default = the
   *  training kind refuses every open, fail closed like the allow-list. */
  trainingModelIds?: string[];
  /** FT1 D11 — live training holds a user may have at once (bound, or held
   *  younger than the ledger's live window). Default 1. */
  maxConcurrentTraining?: number;
}

/** The caps that govern a request of `kind` under `config`. */
export function capsFor(config: GatekeeperConfig, kind: SessionKind | undefined): KindCaps {
  const own = kind && kind !== 'standard' ? config.perKind?.[kind] : undefined;
  return (
    own ?? {
      maxDepositPerSessionMicro: config.maxDepositPerSessionMicro,
      maxDailySpendMicro: config.maxDailySpendMicro,
    }
  );
}

/** A kind is ENABLED when something can actually open it: `standard` always;
 *  `training` only once a training model id is configured (D10). Everything
 *  sized against "the largest cap" ranges over enabled kinds only, so a
 *  training-disabled deployment keeps today's float and alarm floor (D5). */
export function isKindEnabled(config: GatekeeperConfig, kind: SessionKind): boolean {
  if (kind !== 'training') return true;
  return (config.trainingModelIds?.length ?? 0) > 0;
}

/** The largest single deposit any ENABLED kind may open — what the vault's
 *  approve float and the spendable-balance alarm must be sized against. */
export function largestSessionCapMicro(config: GatekeeperConfig): bigint {
  // Seeded at zero, not at the standard cap: `standard` is always enabled,
  // so the loop below produces it, and the function then plainly reads as
  // "the max over ENABLED kinds".
  let largest = 0n;
  for (const kind of SESSION_KINDS) {
    if (!isKindEnabled(config, kind)) continue;
    const cap = capsFor(config, kind).maxDepositPerSessionMicro;
    if (cap > largest) largest = cap;
  }
  return largest;
}

/** Snapshot of one user's ledger state, taken under the ledger mutex. */
export interface LedgerView {
  availableMicro: bigint;
  /** Sum of holds OF THE REQUEST'S KIND this user opened in the last 24h
   *  (settled or not — velocity measures money moved, not money kept). */
  spentInWindowMicro: bigint;
  /** Holds this user opened in the last 60s. */
  opensInWindow: number;
  /** FT1 D11 — this user's LIVE training holds: bound, or held younger than
   *  the ledger's live window. Absent = 0 (callers that predate kinds). */
  liveTrainingHolds?: number;
}

export interface OpenRequest {
  host: string;
  depositMicro: bigint;
  /** Absent = `standard`. Selects the caps; the ledger selects the window. */
  kind?: SessionKind;
  /** FT1 D10 — bound to the kind. Absent = no binding check EXCEPT that a
   *  training kind without a model id is refused (fail closed). */
  modelId?: string;
}

export type GateDecision = { allow: true } | { allow: false; reason: GateRefusal };
export type GateRefusal =
  | 'HOST_NOT_ALLOWED'
  | 'INVALID_DEPOSIT'
  | 'DEPOSIT_OVER_CAP'
  | 'INSUFFICIENT_BALANCE'
  | 'DAILY_CAP_EXCEEDED'
  | 'RATE_LIMITED'
  /** Decided by the session service, not here: the host has not advertised
   *  this model (NodeRegistry price 0), so no session for it can ever open.
   *  Carried on the same refusal channel so clients see ONE shape and can
   *  tell it from a chain_error, which they are right to retry. */
  | 'MODEL_NOT_PRICED'
  /** FT1 D10: the kind and the model id disagree (either direction). */
  | 'MODEL_KIND_MISMATCH'
  /** FT1 D11: the user already has the allowed number of live training holds. */
  | 'CONCURRENT_CAP_EXCEEDED';

export type Gatekeeper = (view: LedgerView, request: OpenRequest) => GateDecision;

export function makeGatekeeper(config: GatekeeperConfig): Gatekeeper {
  const allowed = new Set(config.allowedHosts.map((h) => h.toLowerCase()));
  const trainingIds = new Set((config.trainingModelIds ?? []).map((m) => m.toLowerCase()));
  const maxConcurrentTraining = config.maxConcurrentTraining ?? 1;
  return (view, request) => {
    // Allow-list first: an off-list request learns nothing about balances.
    if (!allowed.has(request.host.toLowerCase())) return refuse('HOST_NOT_ALLOWED');
    if (request.depositMicro <= 0n) return refuse('INVALID_DEPOSIT');
    // D10: the kind↔model binding, both directions, before any money check.
    const isTraining = request.kind === 'training';
    if (request.modelId !== undefined) {
      const isTrainingId = trainingIds.has(request.modelId.toLowerCase());
      if (isTraining !== isTrainingId) return refuse('MODEL_KIND_MISMATCH');
    } else if (isTraining) {
      return refuse('MODEL_KIND_MISMATCH');
    }
    const caps = capsFor(config, request.kind);
    if (request.depositMicro > caps.maxDepositPerSessionMicro) return refuse('DEPOSIT_OVER_CAP');
    if (view.availableMicro < request.depositMicro) return refuse('INSUFFICIENT_BALANCE');
    if (view.spentInWindowMicro + request.depositMicro > caps.maxDailySpendMicro) {
      return refuse('DAILY_CAP_EXCEEDED');
    }
    // D11: one training session in flight per user (default), counted over
    // LIVE holds only so a stranded hold cannot lock the user out for ever.
    if (isTraining && (view.liveTrainingHolds ?? 0) >= maxConcurrentTraining) {
      return refuse('CONCURRENT_CAP_EXCEEDED');
    }
    if (view.opensInWindow >= config.maxOpensPerMinute) return refuse('RATE_LIMITED');
    return { allow: true };
  };
}

function refuse(reason: GateRefusal): GateDecision {
  return { allow: false, reason };
}

/** Parse an integer micro-USDC env value, or a fallback when unset. Shared by
 *  every money-env reader so the parse rule lives in one place. */
export function bigintEnv(name: string, fallback: bigint): bigint {
  const raw = process.env[name];
  if (raw === undefined || raw === '') return fallback;
  try {
    return BigInt(raw);
  } catch {
    throw new Error(`${name} must be an integer USDC micro-unit amount, got "${raw}"`);
  }
}

function intEnv(name: string, fallback: number): number {
  const raw = process.env[name];
  if (raw === undefined || raw === '') return fallback;
  const n = Number(raw);
  if (!Number.isInteger(n) || n < 0) throw new Error(`${name} must be a non-negative integer, got "${raw}"`);
  return n;
}

function listEnv(name: string): string[] {
  return (process.env[name] ?? '')
    .split(',')
    .map((h) => h.trim())
    .filter((h) => h.length > 0);
}

/**
 * Server env (Decision 8 / "What Jules provides" #3):
 *   FIAT_ALLOWED_HOSTS=0x...,0x...            (managed hosts only; unset = refuse all)
 *   FIAT_MAX_SESSION_DEPOSIT_MICRO           (standard kind; default 2 USDC)
 *   FIAT_MAX_DAILY_SPEND_MICRO               (standard kind; default 10 USDC per user per 24h)
 *   FIAT_MAX_OPENS_PER_MINUTE                (all kinds; default 3 per user — also the
 *                                             per-user ATTEMPTS budget of every outcome)
 *   FIAT_TRAINING_MODEL_IDS=0x...,0x...       (the registered training model ids; unset = the
 *                                             training kind refuses every open)
 *   FIAT_TRAINING_MAX_SESSION_DEPOSIT_MICRO  (training kind; default 10 USDC — a run of the
 *                                             feasibility example's size deposits ~8.6)
 *   FIAT_TRAINING_MAX_DAILY_SPEND_MICRO      (training kind; default 20 USDC = two such runs)
 *   FIAT_TRAINING_MAX_CONCURRENT             (live training holds per user; default 1)
 * The training caps are a separate budget, set here rather than by raising the
 * globals (the design doc rejects that). Both count GROSS holds for 24h. Env
 * typos are silent defaults, so the effective values are logged once at boot
 * (`describeGatekeeperConfig`).
 */
export function gatekeeperConfigFromEnv(): GatekeeperConfig {
  return {
    allowedHosts: listEnv('FIAT_ALLOWED_HOSTS'),
    maxDepositPerSessionMicro: bigintEnv('FIAT_MAX_SESSION_DEPOSIT_MICRO', 2_000_000n),
    maxDailySpendMicro: bigintEnv('FIAT_MAX_DAILY_SPEND_MICRO', 10_000_000n),
    maxOpensPerMinute: intEnv('FIAT_MAX_OPENS_PER_MINUTE', 3),
    perKind: {
      training: {
        maxDepositPerSessionMicro: bigintEnv('FIAT_TRAINING_MAX_SESSION_DEPOSIT_MICRO', 10_000_000n),
        maxDailySpendMicro: bigintEnv('FIAT_TRAINING_MAX_DAILY_SPEND_MICRO', 20_000_000n),
      },
    },
    trainingModelIds: listEnv('FIAT_TRAINING_MODEL_IDS').map((m) => m.toLowerCase()),
    maxConcurrentTraining: intEnv('FIAT_TRAINING_MAX_CONCURRENT', 1),
  };
}

/** One line for the boot log: the EFFECTIVE caps and bindings, so a defaulted
 *  value (an env typo is a silent default) is distinguishable from a set one. */
export function describeGatekeeperConfig(config: GatekeeperConfig): string {
  const t = capsFor(config, 'training');
  return (
    `gatekeeper: hosts=${config.allowedHosts.length} ` +
    `standard session=${config.maxDepositPerSessionMicro} day=${config.maxDailySpendMicro} ` +
    `opens/min=${config.maxOpensPerMinute} ` +
    `training ${isKindEnabled(config, 'training') ? 'ENABLED' : 'disabled (no FIAT_TRAINING_MODEL_IDS)'} ` +
    `session=${t.maxDepositPerSessionMicro} day=${t.maxDailySpendMicro} ` +
    `concurrent=${config.maxConcurrentTraining ?? 1} ids=[${(config.trainingModelIds ?? []).join(',')}]`
  );
}
