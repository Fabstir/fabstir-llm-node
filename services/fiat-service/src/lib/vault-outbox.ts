// D20 item 1 — the vault outbox: ONE send path, ONE lock, every send journalled
// (and fsynced) before it is broadcast.
//
// Why: the card path's create used to release its hold and free its key on ANY
// throw, including throws after the transaction had reached the mempool, so a
// retry with the same key could open a second session while the first one mined
// (G4). And three senders (the create, its approves, the stranded-session
// sweeper) signed from the vault key with no shared nonce view, so a lagging
// load-balanced RPC could hand two of them the same nonce.
//
// The rules, in one place:
//   - `vaultSend(kind, request, link)` = `withVaultLock(() => sendLocked(...))`.
//     The lock is NON-reentrant; inside it, sends use `sendLocked`, which takes
//     no lock (so nothing ever re-enters it).
//   - `sendLocked`: nonce = max(RPC pending, highest outbox nonce in ANY state
//     + 1) → estimate → fees → the vault's ETH must cover this send's maximum
//     cost (gasLimit × maxFeePerGas + Base's L1 data fee) plus every unresolved
//     entry's → sign → ONE fsynced append `{kind, nonce, hash, raw, link, at}`
//     → broadcast. Every check that can abort runs BEFORE the append; once
//     appended an entry is always broadcast (and rebroadcast by resolution).
//   - A REBROADCAST is not a send: the entry's own raw bytes, under the
//     per-entry lock of resolution; no new nonce, no new entry. The vault never
//     re-prices or replaces its own transactions.
//   - Resolution (each tick, the lock taken PER ENTRY for its reads and any
//     rebroadcast only): a receipt → mined/reverted; else read latest L, then
//     pending P, then the receipt again: L ≤ N and P ≤ N → rebroadcast; L ≤ N <
//     P → wait; L > N → record block+time first seen, and once BOTH 12 blocks
//     AND 10 minutes have passed, a fresh read still absent → dropped.
//   - Stuck: the lowest unresolved entry still unmined 30 min after its `at`
//     (outside a dropped margin) → alarm and new opens answer `vault_stuck`.
//   - Every boot refuses opens (`vault_stuck`) while the vault's pending nonce
//     is above its latest, unless the outbox owns every nonce in between.
//
// No module-level state here on purpose: Next compiles the routes and the
// instrumentation hook into separate bundles, each with its own copy of every
// module variable. The ONE outbox instance (its journal, index, lock and
// in-flight sets) is built in `buildBackend` and shared through `__fiatBackend`.
import { Transaction, type TransactionLike } from 'ethers';
import { ESCROW_INTERFACE } from './escrow';
import type { LedgerStore } from './ledger';

export type OutboxKind = 'create' | 'approve' | 'sweep';
export type OutboxOutcome = 'mined' | 'reverted' | 'dropped';

export interface ChainReceipt {
  status: number | null | undefined;
  blockNumber: number;
  logs: ReadonlyArray<{ topics: readonly string[]; data: string }>;
}

/** The vault's view of the chain. Production wraps an ethers JsonRpcProvider
 *  (`staticNetwork`); tests script it. Every call is bounded by `rpcTimeoutMs`. */
export interface VaultRpc {
  getNonce(tag: 'latest' | 'pending'): Promise<number>;
  getBalance(): Promise<bigint>;
  estimateGas(tx: { from: string; to: string; data: string; value?: bigint }): Promise<bigint>;
  getFeeData(): Promise<{ maxFeePerGas: bigint; maxPriorityFeePerGas: bigint }>;
  /** Base (OP-stack) L1 data fee: GasPriceOracle.getL1Fee(unsigned serialised tx). */
  l1Fee(unsignedSerialized: string): Promise<bigint>;
  broadcast(raw: string): Promise<void>;
  receipt(hash: string): Promise<ChainReceipt | null>;
  blockNumber(): Promise<number>;
}

export interface VaultSigner {
  address: string;
  signTransaction(tx: TransactionLike<string>): Promise<string>;
}

export interface OutboxEntry {
  kind: OutboxKind;
  nonce: number;
  hash: string;
  raw: string;
  link: string;
  /** The append time = the first broadcast attempt (the stuck clock's origin). */
  at: number;
  /** gasLimit × maxFeePerGas + L1 fee at sign time (the ETH check's reserve). */
  maxCost: bigint;
  outcome?: OutboxOutcome;
  resolvedAt?: number;
  /** A create's SessionJobCreatedForModel jobId, when its receipt carries one. */
  jobId?: bigint;
  /** First tick that saw latest nonce > N with no receipt (the dropped margin). */
  margin?: { block: number; at: number };
}

type Line =
  | { t: 'send'; kind: OutboxKind; nonce: number; hash: string; raw: string; link: string; at: number; maxCost: string }
  | { t: 'resolve'; hash: string; outcome: OutboxOutcome; at: number; jobId?: string }
  | { t: 'margin'; hash: string; block: number; at: number };

export interface SendRequest {
  to: string;
  data: string;
  value?: bigint;
}

export interface SendResult {
  hash: string;
  nonce: number;
  /** The append happened; only the broadcast threw (resolution rebroadcasts). */
  broadcastError?: unknown;
}

/** Thrown before any append when the vault cannot pay for the send. */
export class VaultEthError extends Error {}
/** Thrown before any append when the lock could not be taken in time. */
export class VaultLockTimeout extends Error {}
/** The outbox append itself threw (its write or fsync): AMBIGUOUS — the line
 *  may be on disk, and a restart would replay and rebroadcast it. Nothing was
 *  broadcast by this process. */
export class OutboxAppendFailed extends Error {
  constructor(
    readonly kind: OutboxKind,
    readonly hash: string,
    readonly nonce: number,
    message: string
  ) {
    super(message);
  }
}

export const DROPPED_MARGIN_BLOCKS = 12;
export const DROPPED_MARGIN_MS = 10 * 60 * 1000;
export const STUCK_AFTER_MS = 30 * 60 * 1000;
export const LATE_RECEIPT_WINDOW_MS = 24 * 60 * 60 * 1000;

const CREATED_TOPIC = ESCROW_INTERFACE.getEvent('SessionJobCreatedForModel')!.topicHash;

/** Base Sepolia. */
export const DEFAULT_CHAIN_ID = 84532n;

export interface OutboxOptions {
  rpc: VaultRpc;
  signer: VaultSigner;
  chainId?: bigint;
  onAlarm?: (message: string) => void;
  now?: () => number;
  /** Every RPC call under the lock: default 20 s. */
  rpcTimeoutMs?: number;
  /** Receipt polling cadence while a caller waits for a confirmation. */
  pollMs?: number;
  /** How long resolution waits for the per-entry lock before trying next tick. */
  resolveLockWaitMs?: number;
}

/** Bound one promise by `ms`; the timer never outlives the race. */
export async function withTimeout<T>(p: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} timed out after ${ms} ms`)), ms);
  });
  try {
    return await Promise.race([p, timeout]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, Math.max(0, ms)));
}

/** A NON-reentrant mutex with a bounded wait. Re-acquiring from inside the
 *  holder simply waits (and times out): nesting is a bug, never a feature. */
class VaultLock {
  private held = false;
  private waiters: Array<{ grant: () => void }> = [];

  isHeld(): boolean {
    return this.held;
  }

  acquire(acquireBy: number | undefined, now: () => number): Promise<void> {
    if (!this.held) {
      this.held = true;
      return Promise.resolve();
    }
    return new Promise<void>((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const waiter = {
        grant: () => {
          if (timer) clearTimeout(timer);
          resolve();
        },
      };
      this.waiters.push(waiter);
      if (acquireBy !== undefined) {
        timer = setTimeout(() => {
          const i = this.waiters.indexOf(waiter);
          if (i >= 0) {
            this.waiters.splice(i, 1);
            reject(new VaultLockTimeout('vault lock not acquired in time — nothing was sent'));
          }
        }, Math.max(0, acquireBy - now()));
      }
    });
  }

  release(): void {
    const next = this.waiters.shift();
    if (next) next.grant();
    else this.held = false;
  }
}

export class VaultOutbox {
  private readonly entries = new Map<string, OutboxEntry>();
  private readonly lock = new VaultLock();
  /** D20 — the shared in-flight sets (live opens' holds and keys; chosen sweeps). */
  readonly inFlight = { holds: new Set<string>(), keys: new Set<string>(), sweeps: new Set<string>() };
  private readonly alarmed = new Set<string>();
  private bootWait = true;
  /** Set when an outbox append threw (its write or fsync): the line may be on
   *  disk with its nonce, so nothing more is sent until a restart replays it. */
  private journalFailed: string | null = null;
  /** One lock-timeout alarm per episode (reset by the next acquisition). */
  private lockTimeoutAlarmed = false;
  private stuck: string | null = null;
  private lastLatest: number | undefined;
  /** What resolution last did per entry (diagnostics for the stuck alarm). */
  private readonly lastAction = new Map<string, 'rebroadcast' | 'held-in-mempool'>();
  readonly chainId: bigint;
  private readonly now: () => number;
  private readonly rpcTimeoutMs: number;
  private readonly pollMs: number;
  private readonly resolveLockWaitMs: number;
  private readonly alarmSink: (message: string) => void;

  private constructor(
    private readonly store: LedgerStore,
    readonly rpc: VaultRpc,
    readonly signer: VaultSigner,
    opts: OutboxOptions
  ) {
    this.chainId = opts.chainId ?? DEFAULT_CHAIN_ID;
    this.now = opts.now ?? Date.now;
    this.rpcTimeoutMs = opts.rpcTimeoutMs ?? 20_000;
    this.pollMs = opts.pollMs ?? 2_000;
    this.resolveLockWaitMs = opts.resolveLockWaitMs ?? 30_000;
    this.alarmSink = opts.onAlarm ?? ((m) => console.error(`[fiat-vault] ALARM: ${m}`));
  }

  /** Replays the journal ONCE (boot). Every boot starts in the nonce wait. */
  static async open(store: LedgerStore, opts: OutboxOptions): Promise<VaultOutbox> {
    const outbox = new VaultOutbox(store, opts.rpc, opts.signer, opts);
    const lines = await store.load();
    for (let i = 0; i < lines.length; i++) {
      let line: Line;
      try {
        line = JSON.parse(lines[i]!) as Line;
      } catch (e) {
        if (i === lines.length - 1) break; // a torn final line: never synced, never broadcast
        throw new Error(`vault outbox journal corrupt at line ${i + 1}: ${(e as Error).message}`);
      }
      outbox.apply(line);
    }
    // The sweep in-flight set is rebuilt from unresolved sweep entries, so a
    // restart can never send a second sweep for a job whose first is pending.
    for (const entry of outbox.entries.values()) {
      if (entry.kind === 'sweep' && !entry.outcome) outbox.inFlight.sweeps.add(entry.link);
    }
    return outbox;
  }

  private apply(line: Line): void {
    switch (line.t) {
      case 'send':
        this.entries.set(line.hash, {
          kind: line.kind,
          nonce: line.nonce,
          hash: line.hash,
          raw: line.raw,
          link: line.link,
          at: line.at,
          maxCost: BigInt(line.maxCost),
        });
        break;
      case 'resolve': {
        const entry = this.entries.get(line.hash);
        if (!entry) break;
        entry.outcome = line.outcome;
        entry.resolvedAt = line.at;
        if (line.jobId !== undefined) entry.jobId = BigInt(line.jobId);
        if (entry.kind === 'sweep') this.inFlight.sweeps.delete(entry.link);
        break;
      }
      case 'margin': {
        const entry = this.entries.get(line.hash);
        if (entry && !entry.margin) entry.margin = { block: line.block, at: line.at };
        break;
      }
    }
  }

  private async commit(line: Line): Promise<void> {
    try {
      await this.store.append(JSON.stringify(line));
    } catch (e) {
      // The standard fsync-failure response: the line's bytes may have
      // survived (a send at its nonce), and the in-memory index cannot know.
      if (!this.journalFailed) {
        this.journalFailed = e instanceof Error ? e.message : String(e);
        this.alarm(
          `vault outbox journal write failed (${this.journalFailed}) — every send is refused and opens answer vault_stuck ` +
            `until a restart replays the journal; check the disk, then restart`
        );
      }
      throw e;
    }
    this.apply(line);
  }

  alarm(message: string): void {
    this.alarmSink(message);
  }

  /** An alarm raised ONCE per id per process (pre-migration entries). */
  alarmOnce(id: string, message: string): void {
    if (this.alarmed.has(id)) return;
    this.alarmed.add(id);
    this.alarmSink(message);
  }

  /** One RPC call bounded by the 20 s RPC timeout and, when `by` is given,
   *  by what is left until `by` (D20: a request's 150 s / 170 s bounds are
   *  enforced, not merely checked between calls). Past `by` it is not made. */
  timed<T>(p: Promise<T>, what: string, by?: number): Promise<T> {
    if (by === undefined) return withTimeout(p, this.rpcTimeoutMs, what);
    const left = by - this.now();
    if (left <= 0) {
      p.catch(() => undefined);
      return Promise.reject(new Error(`${what} not made: its deadline has passed`));
    }
    return withTimeout(p, Math.min(this.rpcTimeoutMs, left), what);
  }

  // ---- the lock ----

  /** The ONE vault lock. NON-reentrant; `acquireBy` bounds the wait. */
  async withVaultLock<T>(fn: () => Promise<T>, opts: { acquireBy?: number } = {}): Promise<T> {
    try {
      await this.lock.acquire(opts.acquireBy, this.now);
    } catch (e) {
      // A holder that never lets go (a hung fsync, an RPC call past its
      // bound) shows otherwise only as chain_error: alarm once per episode.
      if (e instanceof VaultLockTimeout && !this.lockTimeoutAlarmed) {
        this.lockTimeoutAlarmed = true;
        this.alarm('vault lock not acquired in time — its holder may be hung (an outbox fsync or an RPC call); sends are failing until it lets go');
      }
      throw e;
    }
    this.lockTimeoutAlarmed = false;
    try {
      return await fn();
    } finally {
      this.lock.release();
    }
  }

  isLocked(): boolean {
    return this.lock.isHeld();
  }

  // ---- reads over the index ----

  all(): OutboxEntry[] {
    return [...this.entries.values()].sort((a, b) => a.nonce - b.nonce);
  }

  unresolved(kind?: OutboxKind): OutboxEntry[] {
    return this.all().filter((e) => !e.outcome && (kind === undefined || e.kind === kind));
  }

  byLink(kind: OutboxKind, link: string): OutboxEntry[] {
    return this.all().filter((e) => e.kind === kind && e.link === link);
  }

  /** Highest nonce of ANY entry (mined, reverted, dropped or unresolved). */
  highestNonce(): number | undefined {
    let highest: number | undefined;
    for (const e of this.entries.values()) if (highest === undefined || e.nonce > highest) highest = e.nonce;
    return highest;
  }

  // ---- sending ----

  /**
   * One send, under the lock the CALLER holds. Every abortable check precedes
   * the append; after the append the only failure is the broadcast's, which is
   * returned (not thrown) so the caller knows the entry exists.
   */
  async sendLocked(
    kind: OutboxKind,
    request: SendRequest,
    link: string,
    opts: { appendBy?: number; deadline?: number } = {}
  ): Promise<SendResult> {
    if (!this.lock.isHeld()) throw new Error('sendLocked called outside withVaultLock');
    if (this.journalFailed) {
      throw new Error(`vault sends refused until restart: an outbox journal write failed (${this.journalFailed}) — nothing was sent`);
    }
    // Every pre-append read is bounded by the append window: past it nothing
    // is appended anyway, so a slow RPC must not hold the request beyond it.
    const by = opts.appendBy;
    const pending = await this.timed(this.rpc.getNonce('pending'), 'pending nonce read', by);
    const highest = this.highestNonce();
    const nonce = Math.max(pending, highest === undefined ? 0 : highest + 1);
    const value = request.value ?? 0n;
    const gasLimit = await this.timed(
      this.rpc.estimateGas({ from: this.signer.address, to: request.to, data: request.data, value }),
      'gas estimate',
      by
    );
    const fees = await this.timed(this.rpc.getFeeData(), 'fee read', by);
    const tx: TransactionLike<string> = {
      type: 2,
      chainId: this.chainId,
      nonce,
      to: request.to,
      data: request.data,
      value,
      gasLimit,
      maxFeePerGas: fees.maxFeePerGas,
      maxPriorityFeePerGas: fees.maxPriorityFeePerGas,
    };
    const unsigned = Transaction.from(tx).unsignedSerialized;
    const l1Fee = await this.timed(this.rpc.l1Fee(unsigned), 'L1 fee read', by);
    const maxCost = gasLimit * fees.maxFeePerGas + l1Fee;
    // ethers estimates gas BEFORE fees are known, so nothing upstream checks
    // that the vault can pay: an empty vault would otherwise append entries
    // whose broadcasts fail, then mine hours later when ETH arrived.
    const committed = this.unresolved().reduce((sum, e) => sum + e.maxCost, 0n);
    const balance = await this.timed(this.rpc.getBalance(), 'ETH balance read', by);
    if (balance < maxCost + committed) {
      const message =
        `vault ETH floor: balance ${balance} wei cannot cover this ${kind} (${maxCost} wei incl. L1 fee ${l1Fee}) ` +
        `plus ${committed} wei reserved by unresolved outbox entries — top the vault up; nothing was sent`;
      this.alarm(message);
      throw new VaultEthError(message);
    }
    const raw = await this.signer.signTransaction(tx);
    const hash = Transaction.from(raw).hash!;
    if (opts.appendBy !== undefined && this.now() > opts.appendBy) {
      throw new Error(`${kind} append window passed — nothing was sent`);
    }
    // THE durable write: fsynced before the broadcast, so a power cut can
    // never erase an entry whose transaction may mine.
    try {
      await this.commit({ t: 'send', kind, nonce, hash, raw, link, at: this.now(), maxCost: maxCost.toString() });
    } catch (e) {
      throw new OutboxAppendFailed(kind, hash, nonce, `${kind} ${hash} outbox append failed: ${e instanceof Error ? e.message : String(e)}`);
    }
    // The broadcast is bounded by what is left of the request's deadline (a
    // slow fsync must not buy it a fresh 20 s); past the deadline it is not
    // attempted at all. Either way resolution rebroadcasts the same bytes.
    try {
      await this.timed(this.rpc.broadcast(raw), 'broadcast', opts.deadline);
      return { hash, nonce };
    } catch (e) {
      return { hash, nonce, broadcastError: e };
    }
  }

  /** `withVaultLock(() => sendLocked(...))`. `recheck` runs under the lock and
   *  may veto the send (the sweeper's second guard). */
  vaultSend(
    kind: OutboxKind,
    request: SendRequest,
    link: string,
    opts: { appendBy?: number; recheck?: () => boolean } = {}
  ): Promise<SendResult | 'vetoed'> {
    return this.withVaultLock(
      async () => {
        if (opts.recheck && !opts.recheck()) return 'vetoed' as const;
        return this.sendLocked(kind, request, link, { appendBy: opts.appendBy });
      },
      { acquireBy: opts.appendBy }
    );
  }

  // ---- receipts and resolution ----

  private async recordResolution(entry: OutboxEntry, outcome: OutboxOutcome, jobId?: bigint): Promise<void> {
    if (entry.outcome === outcome && (jobId === undefined || entry.jobId === jobId)) return; // idempotent
    if (entry.outcome && entry.outcome !== 'dropped') return; // mined/reverted are final
    await this.commit({
      t: 'resolve',
      hash: entry.hash,
      outcome,
      at: this.now(),
      ...(jobId !== undefined ? { jobId: jobId.toString() } : {}),
    });
  }

  private async recordReceipt(entry: OutboxEntry, receipt: ChainReceipt): Promise<void> {
    if (receipt.status === 0) return this.recordResolution(entry, 'reverted');
    let jobId: bigint | undefined;
    if (entry.kind === 'create') {
      const log = receipt.logs.find((l) => l.topics[0] === CREATED_TOPIC);
      if (log) jobId = BigInt(log.topics[1]!);
    }
    return this.recordResolution(entry, 'mined', jobId);
  }

  /** Poll for a receipt until `untilMs`; record the verdict when one appears. */
  async waitForReceipt(hash: string, untilMs: number): Promise<ChainReceipt | null> {
    const entry = this.entries.get(hash);
    for (;;) {
      // What is left is checked BEFORE each read, and bounds it: a read
      // started near the deadline must not run a fresh 20 s past it.
      if (untilMs - this.now() <= 0) return null;
      let receipt: ChainReceipt | null = null;
      try {
        receipt = await this.timed(this.rpc.receipt(hash), 'receipt read', untilMs);
      } catch {
        receipt = null;
      }
      if (receipt) {
        if (entry) await this.recordReceipt(entry, receipt);
        return receipt;
      }
      const left = untilMs - this.now();
      if (left <= 0) return null;
      await sleep(Math.min(this.pollMs, left));
    }
  }

  /** A late receipt for a create resolved `dropped` (R5's 24 h watch). */
  async checkLateReceipt(entry: OutboxEntry): Promise<boolean> {
    const receipt = await this.timed(this.rpc.receipt(entry.hash), 'receipt read');
    if (!receipt || receipt.status === 0) return false;
    await this.recordReceipt(entry, receipt);
    return true;
  }

  /** One resolution pass (each settlement tick). Verdicts are idempotent:
   *  the cron route and the 15 s loop may overlap. */
  async resolve(): Promise<void> {
    for (const entry of this.unresolved()) {
      try {
        await this.withVaultLock(() => this.resolveOne(entry), {
          acquireBy: this.now() + this.resolveLockWaitMs,
        });
      } catch (e) {
        this.alarm(`outbox resolution of ${entry.kind} ${entry.hash} (nonce ${entry.nonce}) deferred: ${e instanceof Error ? e.message : String(e)}`);
      }
    }
    this.refreshStuck();
  }

  private async resolveOne(entry: OutboxEntry): Promise<void> {
    if (entry.outcome) return; // another tick resolved it while we waited
    let receipt = await this.timed(this.rpc.receipt(entry.hash), 'receipt read');
    if (receipt) return this.recordReceipt(entry, receipt);
    const latest = await this.timed(this.rpc.getNonce('latest'), 'latest nonce read');
    const pending = await this.timed(this.rpc.getNonce('pending'), 'pending nonce read');
    this.lastLatest = latest;
    receipt = await this.timed(this.rpc.receipt(entry.hash), 'receipt read');
    if (receipt) return this.recordReceipt(entry, receipt);
    const n = entry.nonce;
    if (latest <= n && pending <= n) {
      this.lastAction.set(entry.hash, 'rebroadcast');
      // Nothing of ours occupies N (a lost broadcast, or a gap below a queued
      // send): the SAME bytes again, so gaps fill themselves. Never a re-sign.
      try {
        await this.timed(this.rpc.broadcast(entry.raw), 'rebroadcast');
      } catch {
        // rejected again (full pool, fee floor): the stuck clock is running
      }
      return;
    }
    if (latest <= n) {
      this.lastAction.set(entry.hash, 'held-in-mempool');
      return; // L ≤ N < P: another transaction sits at N; a rebroadcast could replace it
    }
    // L > N with no receipt: dropped only after a FRESH read once both 12
    // blocks AND 10 minutes have passed (a lagging replica can hide a receipt
    // for a burst of blocks, and blocks alone pass in seconds).
    const block = await this.timed(this.rpc.blockNumber(), 'block number read');
    if (!entry.margin) {
      await this.commit({ t: 'margin', hash: entry.hash, block, at: this.now() });
      return;
    }
    if (block - entry.margin.block >= DROPPED_MARGIN_BLOCKS && this.now() - entry.margin.at >= DROPPED_MARGIN_MS) {
      await this.recordResolution(entry, 'dropped');
    }
  }

  /** The stuck rule over the lowest unresolved entry. */
  private refreshStuck(): void {
    const lowest = this.unresolved()[0];
    if (!lowest || lowest.margin || this.now() - lowest.at < STUCK_AFTER_MS) {
      this.stuck = null;
      return;
    }
    let gap = '';
    if (this.lastLatest !== undefined && this.lastLatest < lowest.nonce) {
      const owned = new Set(this.all().map((e) => e.nonce));
      const missing: number[] = [];
      for (let n = this.lastLatest; n < lowest.nonce; n++) if (!owned.has(n)) missing.push(n);
      if (missing.length > 0) {
        gap =
          `; nonce gap ${missing[0]}..${missing[missing.length - 1]} below it is not the outbox's — ` +
          `fill nonces ${missing[0]}..${missing[missing.length - 1]} with 0-value self-sends first`;
      }
    }
    this.stuck =
      `vault outbox stuck: ${lowest.kind} ${lowest.hash} at nonce ${lowest.nonce} unmined ` +
      `${Math.round((this.now() - lowest.at) / 60000)} min after its first broadcast ` +
      `(${this.lastAction.get(lowest.hash) === 'held-in-mempool' ? 'held in the mempool, e.g. a fee below the base fee' : 'rebroadcast, never accepted or never mined'})${gap}. ` +
      `Runbook: top up the vault's ETH, or wait for the base fee to fall (the same bytes are rebroadcast), ` +
      `or cancel nonce ${lowest.nonce} with a 0-value self-send at a higher fee. NEVER re-sign the create.`;
    this.alarm(this.stuck);
  }

  /** An outbox append failed in this process: the index may lack a line that
   *  is on disk, so rules that read "no entry" as "nothing sent" must wait for
   *  the restart's replay. */
  appendFailed(): boolean {
    return this.journalFailed !== null;
  }

  stuckMessage(): string | null {
    return this.stuck;
  }

  // ---- the every-boot nonce wait ----

  bootWaiting(): boolean {
    return this.bootWait;
  }

  /** Clears the boot wait once latest = pending, or the outbox owns every
   *  nonce from latest to pending − 1 (no foreign transaction in flight). */
  async checkBootNonce(): Promise<boolean> {
    if (!this.bootWait) return true;
    const latest = await this.timed(this.rpc.getNonce('latest'), 'latest nonce read');
    const pending = await this.timed(this.rpc.getNonce('pending'), 'pending nonce read');
    const owned = new Set(this.all().map((e) => e.nonce));
    let foreign = false;
    for (let n = latest; n < pending; n++) if (!owned.has(n)) foreign = true;
    if (!foreign) {
      this.bootWait = false;
      return true;
    }
    this.alarm(
      `vault boot nonce wait: pending nonce ${pending} is above latest ${latest} with transactions the outbox does not own — ` +
        `opens answer vault_stuck until they mine or are cancelled`
    );
    return false;
  }

  /** What a new open must answer instead of proceeding, if anything. */
  async openRefusal(): Promise<'vault_stuck' | null> {
    if (this.bootWait) {
      try {
        await this.checkBootNonce();
      } catch {
        // unreadable: keep refusing
      }
    }
    if (this.journalFailed || this.bootWait || this.stuck) return 'vault_stuck';
    return null;
  }
}
