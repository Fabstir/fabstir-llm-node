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
import { fetchUsdcMicroBalance, rpcUrl, usdcTokenAddress } from './balance';
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
}): SettlementListener {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let running: Promise<void> = Promise.resolve();

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
      if (latest < from) return;
      const events = await opts.source.query(from, latest);
      await applySettlementEvents(opts.ledger, events, opts.onAlarm);
      await opts.cursor.save(latest + 1);
    } catch (e) {
      opts.onAlarm(`settlement listener tick failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  function schedule(): void {
    if (stopped) return;
    timer = setTimeout(() => {
      running = tickOnce().finally(schedule);
    }, opts.pollMs ?? 15_000);
  }
  if (!opts.manual) schedule();

  return {
    tick: () => {
      running = tickOnce();
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

let productionListener: SettlementListener | undefined;

/**
 * Entry point for instrumentation.ts. Gated on FIAT_SETTLEMENT_ENABLED=1 and
 * a process-wide singleton (dev hot reloads must not double-start).
 */
export async function startProductionSettlementListener(): Promise<SettlementListener | undefined> {
  if (process.env.FIAT_SETTLEMENT_ENABLED !== '1') return undefined;
  if (productionListener) return productionListener;
  const { ledger } = await getFiatDeps();
  const dataDir = process.env.FIAT_DATA_DIR ?? './data/fiat';
  const fromBlock = Number(process.env.FIAT_SETTLEMENT_FROM_BLOCK ?? '0');
  productionListener = startSettlementListener({
    ledger,
    source: makeChainSettlementSource(),
    cursor: makeFileCursor(join(dataDir, 'settlement-cursor.json')),
    fromBlock,
    onAlarm: (message) => console.error(`[fiat-settlement] ALARM: ${message}`),
    solvency: makeVaultHoldings(),
    reconcile: { reader: makeChainReceiptReader() },
  });
  console.log('[fiat-settlement] listener started');
  return productionListener;
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
  const deposits = new Contract(
    jobMarketplaceAddress(),
    ['function userDepositsToken(address user, address token) view returns (uint256)'],
    new JsonRpcProvider(rpcUrl())
  );
  return {
    async holdings(): Promise<bigint> {
      let total = await fetchUsdcMicroBalance(vaultAddress);
      total += (await deposits.userDepositsToken(vaultAddress, usdcTokenAddress())) as bigint;
      if (treasury) total += await fetchUsdcMicroBalance(treasury);
      return total;
    },
  };
}
