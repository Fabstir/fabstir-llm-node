// FC1.1 — the gatekeeper (Decision 3, SECURITY-CRITICAL): the only policy that
// can authorise a vault spend. Pure decision over a ledger snapshot; the
// ledger calls it INSIDE its mutex (check and hold are atomic), so passing it
// around never opens a check-then-act race.
//
// Server-only module: config comes from server env, never NEXT_PUBLIC_*.

export interface GatekeeperConfig {
  /** Managed-host allow-list (Decision 8). EMPTY by default: an unconfigured
   *  deployment refuses every fiat open rather than trusting any host. */
  allowedHosts: string[];
  maxDepositPerSessionMicro: bigint;
  /** Rolling 24h per-user velocity cap over hold amounts (Decision 8). */
  maxDailySpendMicro: bigint;
  maxOpensPerMinute: number;
}

/** Snapshot of one user's ledger state, taken under the ledger mutex. */
export interface LedgerView {
  availableMicro: bigint;
  /** Sum of holds this user opened in the last 24h (settled or not — velocity
   *  measures money moved, not money kept). */
  spentInWindowMicro: bigint;
  /** Holds this user opened in the last 60s. */
  opensInWindow: number;
}

export interface OpenRequest {
  host: string;
  depositMicro: bigint;
}

export type GateDecision = { allow: true } | { allow: false; reason: GateRefusal };
export type GateRefusal =
  | 'HOST_NOT_ALLOWED'
  | 'INVALID_DEPOSIT'
  | 'DEPOSIT_OVER_CAP'
  | 'INSUFFICIENT_BALANCE'
  | 'DAILY_CAP_EXCEEDED'
  | 'RATE_LIMITED';

export type Gatekeeper = (view: LedgerView, request: OpenRequest) => GateDecision;

export function makeGatekeeper(config: GatekeeperConfig): Gatekeeper {
  const allowed = new Set(config.allowedHosts.map((h) => h.toLowerCase()));
  return (view, request) => {
    // Allow-list first: an off-list request learns nothing about balances.
    if (!allowed.has(request.host.toLowerCase())) return refuse('HOST_NOT_ALLOWED');
    if (request.depositMicro <= 0n) return refuse('INVALID_DEPOSIT');
    if (request.depositMicro > config.maxDepositPerSessionMicro) return refuse('DEPOSIT_OVER_CAP');
    if (view.availableMicro < request.depositMicro) return refuse('INSUFFICIENT_BALANCE');
    if (view.spentInWindowMicro + request.depositMicro > config.maxDailySpendMicro) {
      return refuse('DAILY_CAP_EXCEEDED');
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

/**
 * Cap defaults are placeholders — TODO(Jules): set the real allow-list and cap
 * numbers (Decision 8 / "What Jules provides" #3) in server env:
 *   FIAT_ALLOWED_HOSTS=0x...,0x...     (managed hosts only; unset = refuse all)
 *   FIAT_MAX_SESSION_DEPOSIT_MICRO    (default 2 USDC)
 *   FIAT_MAX_DAILY_SPEND_MICRO        (default 10 USDC per user per 24h)
 *   FIAT_MAX_OPENS_PER_MINUTE         (default 3 per user)
 */
export function gatekeeperConfigFromEnv(): GatekeeperConfig {
  const hosts = (process.env.FIAT_ALLOWED_HOSTS ?? '')
    .split(',')
    .map((h) => h.trim())
    .filter((h) => h.length > 0);
  return {
    allowedHosts: hosts,
    maxDepositPerSessionMicro: bigintEnv('FIAT_MAX_SESSION_DEPOSIT_MICRO', 2_000_000n),
    maxDailySpendMicro: bigintEnv('FIAT_MAX_DAILY_SPEND_MICRO', 10_000_000n),
    maxOpensPerMinute: intEnv('FIAT_MAX_OPENS_PER_MINUTE', 3),
  };
}
