// FC1.3 — the settlement listener: the ledger's read-side of the chain.
// Watches the marketplace's settlement events for vault-paid sessions and
// credits each user's exact on-chain userRefund back to their ledger balance.
// The refund can arrive as a wallet push (SessionCompleted / SessionTimedOut)
// or as a pull-pattern credit (RefundCreditedToDeposit) — both are vault-owned
// money and settle through the same idempotent ledger path (R4).
// SessionCompletedBy is a cross-check: an amount disagreement is a divergence
// alarm (R5), never a second credit.
//
// Runs as a persistent worker inside the Next server process (instrumentation
// hook), gated on FIAT_SETTLEMENT_ENABLED=1. A request handler cannot watch
// events; this can. Restart-safe: the cursor file only advances after a batch
// is applied, and replaying a batch is a no-op.
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { Contract, Interface, JsonRpcProvider, Wallet } from 'ethers';
import { rpcUrl, usdcTokenAddress } from './balance';
import { jobMarketplaceAddress } from './escrow';
import { getFiatDeps } from './fiat-session-service';
import { makeChainReceiptReader, reconcileOrphans, type CreateReceiptReader } from './fiat-reconcile';
import type { CreditsLedger } from './ledger';

// Exact shapes (indexed layout included) from the deployed Upgradeable ABI.
export const SETTLEMENT_INTERFACE = new Interface([
  'event SessionCompleted(uint256 indexed jobId, uint256 totalTokensUsed, uint256 hostEarnings, uint256 userRefund)',
  'event SessionCompletedBy(uint256 indexed jobId, address indexed completedBy, uint256 tokensUsed, uint256 paymentAmount, uint256 refundAmount)',
  'event SessionTimedOut(uint256 indexed jobId, uint256 hostEarnings, uint256 userRefund)',
  'event RefundCreditedToDeposit(uint256 indexed jobId, address indexed depositor, uint256 amount, address indexed token)',
]);

export type SettlementEvent =
  | { kind: 'completed'; jobId: bigint; userRefund: bigint; blockNumber: number }
  | { kind: 'timedout'; jobId: bigint; userRefund: bigint; blockNumber: number }
  | { kind: 'refund-credited'; jobId: bigint; amount: bigint; depositor: string; blockNumber: number }
  | { kind: 'completed-by'; jobId: bigint; refundAmount: bigint; blockNumber: number };

interface RawLog {
  topics: readonly string[];
  data: string;
  blockNumber: number;
}

/** Decode one marketplace log into a SettlementEvent, or null if unrelated. */
export function parseSettlementLog(log: RawLog): SettlementEvent | null {
  let parsed;
  try {
    parsed = SETTLEMENT_INTERFACE.parseLog({ topics: [...log.topics], data: log.data });
  } catch {
    return null;
  }
  if (!parsed) return null;
  const jobId = BigInt(parsed.args[0]);
  switch (parsed.name) {
    case 'SessionCompleted':
      return { kind: 'completed', jobId, userRefund: BigInt(parsed.args[3]), blockNumber: log.blockNumber };
    case 'SessionTimedOut':
      return { kind: 'timedout', jobId, userRefund: BigInt(parsed.args[2]), blockNumber: log.blockNumber };
    case 'RefundCreditedToDeposit':
      return {
        kind: 'refund-credited',
        jobId,
        depositor: String(parsed.args[1]),
        amount: BigInt(parsed.args[2]),
        blockNumber: log.blockNumber,
      };
    case 'SessionCompletedBy':
      return { kind: 'completed-by', jobId, refundAmount: BigInt(parsed.args[4]), blockNumber: log.blockNumber };
    default:
      return null;
  }
}

/**
 * Apply a batch. Idempotent (ledger settle guard); a poison event alarms and
 * is skipped so one bad decode can never wedge the listener.
 */
export async function applySettlementEvents(
  ledger: CreditsLedger,
  events: SettlementEvent[],
  onAlarm: (message: string) => void
): Promise<{ settled: number }> {
  let settled = 0;
  for (const event of events) {
    const refund =
      event.kind === 'completed' || event.kind === 'timedout'
        ? event.userRefund
        : event.kind === 'refund-credited'
          ? event.amount
          : event.refundAmount;
    const already = ledger.refundForJob(event.jobId);
    if (already !== undefined) {
      if (already !== refund) {
        onAlarm(
          `settlement divergence on job ${event.jobId}: ledger settled ${already}, ${event.kind} event says ${refund}`
        );
      }
      continue;
    }
    try {
      const result = await ledger.settle(event.jobId, refund);
      if (result.applied) settled += 1;
    } catch (e) {
      onAlarm(`settlement failed on job ${event.jobId}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  return { settled };
}

export interface SettlementSource {
  latestBlock(): Promise<number>;
  query(fromBlock: number, toBlock: number): Promise<SettlementEvent[]>;
}

export interface SettlementCursor {
  load(): Promise<number | undefined>;
  save(nextBlock: number): Promise<void>;
}

export interface SettlementListener {
  tick(): Promise<void>;
  stop(): Promise<void>;
}

/** Executed-state view of one session, for the log-free reconciliation sweep. */
export interface SessionStateReader {
  /** undefined = temporarily unknown (treat as not-ended and retry next tick). */
  session(jobId: bigint): Promise<{ ended: boolean; refundedToUser: bigint } | undefined>;
}

export function startSettlementListener(opts: {
  ledger: CreditsLedger;
  source: SettlementSource;
  cursor: SettlementCursor;
  fromBlock: number;
  onAlarm: (message: string) => void;
  pollMs?: number;
  /** FC1.4 solvency invariant: vault holdings >= outstanding ledger money,
   *  asserted every tick when provided. */
  solvency?: { holdings(): Promise<bigint> };
  /** R5/M2 reconciliation: bind/release orphaned holds (crash between create and
   *  bind) each tick when provided. A no-op when there are no pending creates. */
  reconcile?: { reader: CreateReceiptReader };
  /** Tests drive tick() themselves; production self-schedules. */
  manual?: boolean;
  /** Scan only to latest − safetyLag. A load-balanced public RPC can answer
   *  latestBlock() from a fresh replica and getLogs from a lagging one; logs a
   *  few blocks behind the head are present everywhere. Default 0 (exact). */
  safetyLag?: number;
  /** Re-scan this many blocks behind the cursor every tick. Re-delivery is free
   *  (settle() is idempotent via the settled-refunds guard), and it gives a
   *  replica that lagged past safetyLag more chances. Default 0 (exact). */
  overlapBlocks?: number;
  /** The job-962 lesson (2026-07-23, second miss THROUGH the 30-block overlap):
   *  gateways can route eth_getLogs to an indexer cluster lagging MINUTES behind
   *  the head that eth_blockNumber reports, so no fixed overlap is safe. This
   *  sweep reconciles by STATE instead: every tick, each job the ledger still
   *  waits on is checked via eth_call (executed state, no log indexer involved)
   *  and settled from the contract's own refundedToUser. Events stay the fast
   *  path; the sweep is the guarantee. */
  stateSweep?: SessionStateReader;
  /** The tick-freeze lesson (2026-07-23, incident #3): none of the tick's RPC
   *  awaits had a timeout, so ONE hung request silently stopped all future
   *  ticks — the only failure shape no per-event alarm can see. A tick that
   *  exceeds this budget is abandoned with an ALARM and the loop reschedules;
   *  abandonment is safe because every apply is idempotent. Default 120s. */
  tickTimeoutMs?: number;
  /** Liveness heartbeat: every N completed ticks, report tick count, cursor and
   *  outstanding bound jobs. Makes journal silence itself diagnostic — a quiet
   *  listener is now provably dead rather than possibly idle. 0 disables. */
  heartbeatEvery?: number;
  onHeartbeat?: (line: string) => void;
}): SettlementListener {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running: Promise<void> = Promise.resolve();
  let tickCount = 0;

  async function tickOnce(): Promise<void> {
    try {
      if (opts.reconcile) {
        // Recover orphaned holds (crash between create and bind) BEFORE the
        // solvency check, so a just-reconciled session is counted correctly.
        const r = await reconcileOrphans(opts.ledger, opts.reconcile.reader, opts.onAlarm);
        if (r.bound || r.released) {
          opts.onAlarm(`reconciliation: bound ${r.bound}, released ${r.released}, still-pending ${r.pending}`);
        }
      }
      if (opts.solvency) {
        // Backing = hot/treasury vault USDC + deposits currently in escrow for
        // live sessions (that money left the vault into the contract and returns
        // on settlement). Without the escrow term this alarms on every open
        // session (M1).
        const holdings = await opts.solvency.holdings();
        const backing = holdings + opts.ledger.escrowedMicro();
        const outstanding = opts.ledger.outstandingMicro();
        if (backing < outstanding) {
          opts.onAlarm(
            `solvency breach: backing ${backing} (holdings ${holdings} + escrow ${opts.ledger.escrowedMicro()}) below outstanding ledger ${outstanding}`
          );
        }
      }
      const from = (await opts.cursor.load()) ?? opts.fromBlock;
      const latest = await opts.source.latestBlock();
      // The job-960 lesson (2026-07-23): a tick once scanned a range whose logs a
      // lagging RPC replica had not indexed yet, then advanced the cursor —
      // permanently skipping a real settlement. Hold back from the head and
      // re-scan an overlap; both are free because apply is idempotent.
      const safeLatest = latest - (opts.safetyLag ?? 0);
      const scanFrom = Math.max(from - (opts.overlapBlocks ?? 0), opts.fromBlock);
      if (safeLatest < scanFrom) return;
      const events = await opts.source.query(scanFrom, safeLatest);
      await applySettlementEvents(opts.ledger, events, opts.onAlarm);
      // Never let the overlap walk the cursor backwards.
      await opts.cursor.save(Math.max(safeLatest + 1, from));
      if (opts.stateSweep) {
        for (const jobId of opts.ledger.boundJobIds()) {
          try {
            const s = await opts.stateSweep.session(jobId);
            if (!s || !s.ended) continue;
            const result = await opts.ledger.settle(jobId, s.refundedToUser);
            if (result.applied) {
              // Recovery via state means the event path missed it — say so.
              opts.onAlarm(
                `state-sweep recovered job ${jobId}: refund ${s.refundedToUser} (the event path missed it)`
              );
            }
          } catch (e) {
            opts.onAlarm(
              `state-sweep failed on job ${jobId}: ${e instanceof Error ? e.message : String(e)}`
            );
          }
        }
      }
    } catch (e) {
      opts.onAlarm(`settlement listener tick failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  async function guardedTick(): Promise<void> {
    const budget = opts.tickTimeoutMs ?? 120_000;
    let watchdog: ReturnType<typeof setTimeout> | undefined;
    const timedOut = new Promise<'timeout'>((resolve) => {
      watchdog = setTimeout(() => resolve('timeout'), budget);
    });
    const result = await Promise.race([tickOnce().then(() => 'done' as const), timedOut]);
    if (watchdog) clearTimeout(watchdog);
    if (result === 'timeout') {
      opts.onAlarm(
        `tick watchdog: tick exceeded ${budget}ms — abandoned and rescheduled (hung RPC await; applies are idempotent so late effects are harmless)`
      );
      return;
    }
    tickCount += 1;
    const every = opts.heartbeatEvery ?? 0;
    if (opts.onHeartbeat && every > 0 && tickCount % every === 0) {
      const cursorVal = await opts.cursor.load().catch(() => undefined);
      opts.onHeartbeat(
        `heartbeat: tick #${tickCount}, cursor ${cursorVal ?? 'unset'}, boundJobs ${opts.ledger.boundJobIds().length}`
      );
    }
  }

  function schedule(): void {
    if (stopped) return;
    timer = setTimeout(() => {
      running = guardedTick().finally(schedule);
    }, opts.pollMs ?? 15_000);
  }
  if (!opts.manual) schedule();

  return {
    tick: () => {
      running = guardedTick();
      return running;
    },
    stop: async () => {
      stopped = true;
      if (timer) clearTimeout(timer);
      await running;
    },
  };
}

/** Event source over the real marketplace: one queryFilter per event type. */
export function makeChainSettlementSource(deps?: {
  provider?: JsonRpcProvider;
  marketplaceAddress?: string;
}): SettlementSource {
  const provider = deps?.provider ?? new JsonRpcProvider(rpcUrl());
  const address = deps?.marketplaceAddress ?? jobMarketplaceAddress();
  const contract = new Contract(address, SETTLEMENT_INTERFACE, provider);
  // Order is load-bearing with the stable sort below: within a block the
  // canonical userRefund events settle FIRST, so SessionCompletedBy lands as
  // the divergence cross-check (never the value that settles).
  const names = ['SessionCompleted', 'SessionTimedOut', 'RefundCreditedToDeposit', 'SessionCompletedBy'];
  return {
    latestBlock: () => provider.getBlockNumber(),
    async query(fromBlock: number, toBlock: number): Promise<SettlementEvent[]> {
      const events: SettlementEvent[] = [];
      for (const name of names) {
        const logs = await contract.queryFilter(contract.filters[name]!(), fromBlock, toBlock);
        for (const log of logs) {
          const parsed = parseSettlementLog({
            topics: log.topics,
            data: log.data,
            blockNumber: log.blockNumber,
          });
          if (parsed) events.push(parsed);
        }
      }
      return events.sort((a, b) => a.blockNumber - b.blockNumber);
    },
  };
}

/** State reader over the real marketplace: one eth_call per outstanding job.
 *  eth_call reflects executed state directly — no log indexer in the path. */
export function makeChainSessionReader(): SessionStateReader {
  const provider = new JsonRpcProvider(rpcUrl());
  const contract = new Contract(
    jobMarketplaceAddress(),
    [
      'function sessionJobs(uint256) view returns (uint256 id, address depositor, address host, address paymentToken, uint256 deposit, uint256 pricePerToken, uint256 tokensUsed, uint256 maxDuration, uint256 startTime, uint256 lastProofTime, uint256 proofInterval, uint256 proofTimeoutWindow, uint8 status, bool withdrawnByHost, uint256 refundedToUser, string conversationCID, bytes32 lastProofHash, string lastProofCID)',
    ],
    provider
  );
  return {
    async session(jobId: bigint) {
      const s = await contract.sessionJobs(jobId);
      // SessionStatus: 0 Active, 1 Completed, 2 TimedOut (API_REFERENCE.md).
      // Non-active means the contract has finalised refundedToUser (0 is a
      // legitimate fully-consumed deposit).
      return { ended: Number(s.status) !== 0, refundedToUser: BigInt(s.refundedToUser) };
    },
  };
}

/** Cursor persisted next to the ledger journal (R5: part of the money record). */
export function makeFileCursor(path: string): SettlementCursor {
  return {
    async load(): Promise<number | undefined> {
      try {
        const parsed = JSON.parse(await readFile(path, 'utf8')) as { nextBlock?: number };
        return typeof parsed.nextBlock === 'number' ? parsed.nextBlock : undefined;
      } catch {
        return undefined;
      }
    },
    async save(nextBlock: number): Promise<void> {
      await mkdir(dirname(path), { recursive: true });
      await writeFile(path, JSON.stringify({ nextBlock }), 'utf8');
    },
  };
}

// On globalThis, NOT module scope: Next compiles instrumentation.js and the
// route handlers into SEPARATE bundles, each with its own copy of this module's
// variables — a module-local here left the tick endpoint seeing `undefined`
// while the instrumentation-owned listener was demonstrably running (first
// deploy of the endpoint, 2026-07-23 20:08). Same dual-bundle trap the
// challenge store already solves the same way.
const g = globalThis as typeof globalThis & {
  __fiatSettlementListener?: SettlementListener;
};

/** The externally-kicked tick (POST /v1/fiat/settlement/tick). Four freezes on
 *  2026-07-23 — two different builds, identical signature: tick #1 completes,
 *  tick #2 never begins — showed the in-process timer chain cannot be trusted
 *  in this server. Correctness now rides on this: cron kicks one tick per
 *  minute over HTTP, a fresh request context each time, running the SAME
 *  guarded tick against the SAME in-memory ledger. The internal loop remains
 *  as a best-effort fast path. */
export function getProductionSettlementListener(): SettlementListener | undefined {
  return g.__fiatSettlementListener;
}

/** Tests only. */
export function setProductionSettlementListenerForTest(l: SettlementListener | undefined): void {
  g.__fiatSettlementListener = l;
}

/**
 * Entry point for instrumentation.ts. Gated on FIAT_SETTLEMENT_ENABLED=1 and
 * a process-wide singleton (dev hot reloads must not double-start).
 */
export async function startProductionSettlementListener(): Promise<SettlementListener | undefined> {
  if (process.env.FIAT_SETTLEMENT_ENABLED !== '1') return undefined;
  if (g.__fiatSettlementListener) return g.__fiatSettlementListener;
  const { ledger } = await getFiatDeps();
  const dataDir = process.env.FIAT_DATA_DIR ?? './data/fiat';
  const fromBlock = Number(process.env.FIAT_SETTLEMENT_FROM_BLOCK ?? '0');
  g.__fiatSettlementListener = startSettlementListener({
    ledger,
    source: makeChainSettlementSource(),
    cursor: makeFileCursor(join(dataDir, 'settlement-cursor.json')),
    fromBlock,
    onAlarm: (message) => console.error(`[fiat-settlement] ALARM: ${message}`),
    solvency: makeVaultHoldings(),
    reconcile: { reader: makeChainReceiptReader() },
    // Public-RPC replica lag protection (see the tickOnce comment). ~2s blocks:
    // lag 5 ≈ 10s behind head, overlap 30 ≈ a minute of re-scan each tick.
    safetyLag: Number(process.env.FIAT_SETTLEMENT_SAFETY_LAG ?? '5'),
    overlapBlocks: Number(process.env.FIAT_SETTLEMENT_OVERLAP_BLOCKS ?? '30'),
    // The guarantee layer: log-free, lag-proof reconciliation by executed state.
    stateSweep: makeChainSessionReader(),
    // Liveness: abandon hung ticks loudly; heartbeat every 40 ticks (~10 min at
    // the 15s poll) so journal silence beyond that provably means dead.
    tickTimeoutMs: Number(process.env.FIAT_SETTLEMENT_TICK_TIMEOUT_MS ?? '120000'),
    heartbeatEvery: Number(process.env.FIAT_SETTLEMENT_HEARTBEAT_EVERY ?? '40'),
    onHeartbeat: (line) => console.log(`[fiat-settlement] ${line}`),
  });
  console.log('[fiat-settlement] listener started');
  return g.__fiatSettlementListener;
}

/**
 * Vault holdings = hot vault wallet USDC + the vault's IN-CONTRACT deposit
 * balance + optional treasury (FIAT_TREASURY_ADDRESS). The in-contract term
 * matters: a settlement refund can arrive either as a wallet push
 * (SessionCompleted, observed live on the FC1.0 spike) OR as a pull-pattern
 * credit to the depositor's in-contract deposit balance (RefundCreditedToDeposit,
 * present in the deployed ABI). Both are vault-owned (only the vault can
 * withdrawToken the latter), so both count toward backing outstanding
 * liabilities — else a pull-pattern refund would look like a solvency shortfall.
 */
function makeVaultHoldings(): { holdings(): Promise<bigint> } | undefined {
  const key = process.env.FIAT_VAULT_PRIVATE_KEY;
  if (!key) return undefined;
  const vaultAddress = new Wallet(key).address;
  const treasury = process.env.FIAT_TREASURY_ADDRESS;
  // ONE provider for the lifetime of the listener. fetchUsdcMicroBalance builds
  // a fresh JsonRpcProvider per call — at one solvency check per 15s tick that
  // is constant connection/detection churn against the public gateway (and a
  // hang candidate in the 2026-07-23 tick-freeze incident). Reuse everything.
  const provider = new JsonRpcProvider(rpcUrl());
  const deposits = new Contract(
    jobMarketplaceAddress(),
    ['function userDepositsToken(address user, address token) view returns (uint256)'],
    provider
  );
  const usdc = new Contract(
    usdcTokenAddress(),
    ['function balanceOf(address) view returns (uint256)'],
    provider
  );
  return {
    async holdings(): Promise<bigint> {
      let total = BigInt(await usdc.balanceOf(vaultAddress));
      total += (await deposits.userDepositsToken(vaultAddress, usdcTokenAddress())) as bigint;
      if (treasury) total += BigInt(await usdc.balanceOf(treasury));
      return total;
    },
  };
}
