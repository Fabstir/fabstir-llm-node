// FC1.1 — the credits ledger: the fiat user's money record (IMPLEMENTATION-
// FIAT-CREDITS-VAULT.md, Decisions 3/5/6). Server-only.
//
// All amounts are integer USDC micro-units (bigint) — no floats anywhere near
// money. Storage is an append-only journal (JSONL) replayed into an in-memory
// projection on open (D1; Postgres is the noted go-live upgrade path). Every
// mutation runs on an internal serial queue, and the gatekeeper decision is
// taken INSIDE that queue, so a balance can never be checked and spent by two
// concurrent opens (the double-spend gate).
//
// The journal is the user's money — R5: it must be backed up and reconciled
// against chain events in production.
import { appendFile, mkdir, readFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import type { GateDecision, Gatekeeper, GateRefusal, LedgerView, SessionKind } from './gatekeeper';

export interface LedgerStore {
  /** All previously appended lines, in append order. */
  load(): Promise<string[]>;
  /** Durably append one line. Called only from the ledger's serial queue. */
  append(line: string): Promise<void>;
}

export class MemoryLedgerStore implements LedgerStore {
  private lines: string[] = [];
  async load(): Promise<string[]> {
    return [...this.lines];
  }
  async append(line: string): Promise<void> {
    this.lines.push(line);
  }
}

export class JsonlLedgerStore implements LedgerStore {
  constructor(private readonly path: string) {}
  async load(): Promise<string[]> {
    try {
      const text = await readFile(this.path, 'utf8');
      return text.split('\n').filter((l) => l.length > 0);
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') return [];
      throw e;
    }
  }
  async append(line: string): Promise<void> {
    await mkdir(dirname(this.path), { recursive: true });
    await appendFile(this.path, `${line}\n`, 'utf8');
  }
}

// Journal entry shapes. Amounts are serialised as decimal strings (JSON has no
// bigint); `at` is epoch ms from the injected clock.
type Entry =
  | { t: 'purchase'; userId: string; amount: string; eventId: string; pi?: string; at: number }
  | { t: 'hold'; holdId: string; userId: string; amount: string; host: string; at: number; kind?: SessionKind }
  | { t: 'create-pending'; holdId: string; txHash: string; at: number }
  | { t: 'bind'; holdId: string; jobId: string; at: number }
  | { t: 'release'; holdId: string; at: number }
  | { t: 'settle'; jobId: string; refund: string; at: number }
  | { t: 'cashout'; userId: string; amount: string; pi?: string; at: number }
  | { t: 'cashout-reversal'; userId: string; amount: string; pi: string; at: number };

interface Hold {
  holdId: string;
  userId: string;
  amountMicro: bigint;
  host: string;
  atMs: number;
  state: 'held' | 'bound' | 'settled' | 'released';
  /** Absent on every hold journaled before kinds existed = `standard`. */
  kind?: SessionKind;
  jobId?: bigint;
  /** The create tx hash, recorded the instant it is submitted (before the
   *  confirmation wait). A `held` hold carrying this is a crash-recoverable
   *  orphan: reconciliation reads the receipt and binds deterministically (M2). */
  pendingTxHash?: string;
}

const DAY_MS = 24 * 60 * 60 * 1000;
const MINUTE_MS = 60 * 1000;
/** FT1 D11 — how long a `held` (not yet bound) hold still counts as a LIVE
 *  training hold. A create cannot still be in flight after this; an older
 *  `held` hold is an orphan of a pre-existing kind (a crash between openHold
 *  and onSubmitted, or a create tx evicted from a public mempool across a
 *  restart) whose debit stands but which must not lock the user out of
 *  training for ever. A ledger constant on purpose: never an env re-read
 *  (`nowMs − atMs < NaN` is false and would count nothing). */
export const LIVE_HOLD_MS = 2 * 60 * 60 * 1000;

export type OpenHoldResult = { ok: true; holdId: string } | { ok: false; reason: GateRefusal };
export type CashoutReason = 'INSUFFICIENT_BALANCE' | 'INVALID_AMOUNT' | 'EXCEEDS_REFUNDABLE';
export type CashoutResult = { ok: true } | { ok: false; reason: CashoutReason };

export class CreditsLedger {
  private available = new Map<string, bigint>();
  private holds = new Map<string, Hold>();
  private jobToHold = new Map<string, string>();
  private seenStripeEvents = new Set<string>();
  // jobId -> the refund that settled it (also the "already settled" guard —
  // presence means settled).
  private settledRefunds = new Map<string, bigint>();
  // FC1.4 cash-out provenance: refunds go to the original card only.
  private purchasesByUser = new Map<string, Array<{ pi?: string; amountMicro: bigint; atMs: number }>>();
  private refundedByPi = new Map<string, bigint>();
  private holdCounter = 0;
  private queue: Promise<unknown> = Promise.resolve();

  private constructor(
    private readonly store: LedgerStore,
    private readonly now: () => number
  ) {}

  static async open(store: LedgerStore, opts?: { now?: () => number }): Promise<CreditsLedger> {
    const ledger = new CreditsLedger(store, opts?.now ?? Date.now);
    const lines = await store.load();
    for (let i = 0; i < lines.length; i++) {
      let entry: Entry;
      try {
        entry = JSON.parse(lines[i]!) as Entry;
      } catch (e) {
        // appendFile is not guaranteed atomic; a crash mid-write can leave a
        // torn FINAL line. Tolerate exactly that (last line only) so the money
        // record still opens; a malformed line anywhere earlier is real
        // corruption and must not be swallowed.
        if (i === lines.length - 1) {
          console.error(`[ledger] dropping torn trailing journal line on load: ${(e as Error).message}`);
          break;
        }
        throw new Error(`ledger journal corrupt at line ${i + 1}: ${(e as Error).message}`);
      }
      ledger.apply(entry);
    }
    return ledger;
  }

  /** Serialise a mutation: apply in memory only AFTER the journal append succeeds. */
  private run<T>(op: () => Promise<T>): Promise<T> {
    const result = this.queue.then(op);
    // The queue must survive a failed op (the failure still reaches the caller).
    this.queue = result.catch(() => undefined);
    return result;
  }

  private async commit(entry: Entry): Promise<void> {
    await this.store.append(JSON.stringify(entry));
    this.apply(entry);
  }

  /** Replay/apply one journal entry into the projection. Must stay total: the
   *  journal is trusted (it was validated before being written). */
  private apply(entry: Entry): void {
    switch (entry.t) {
      case 'purchase': {
        this.seenStripeEvents.add(entry.eventId);
        this.credit(entry.userId, BigInt(entry.amount));
        const purchases = this.purchasesByUser.get(entry.userId) ?? [];
        purchases.push({ pi: entry.pi, amountMicro: BigInt(entry.amount), atMs: entry.at });
        this.purchasesByUser.set(entry.userId, purchases);
        break;
      }
      case 'hold': {
        this.credit(entry.userId, -BigInt(entry.amount));
        const n = Number(entry.holdId.replace(/^h/, ''));
        if (Number.isInteger(n) && n >= this.holdCounter) this.holdCounter = n + 1;
        this.holds.set(entry.holdId, {
          holdId: entry.holdId,
          userId: entry.userId,
          amountMicro: BigInt(entry.amount),
          host: entry.host,
          atMs: entry.at,
          state: 'held',
          ...(entry.kind ? { kind: entry.kind } : {}),
        });
        break;
      }
      case 'create-pending': {
        const hold = this.holds.get(entry.holdId);
        if (hold) hold.pendingTxHash = entry.txHash;
        break;
      }
      case 'bind': {
        const hold = this.holds.get(entry.holdId);
        if (hold) {
          hold.state = 'bound';
          hold.jobId = BigInt(entry.jobId);
          hold.pendingTxHash = undefined; // resolved
          this.jobToHold.set(entry.jobId, entry.holdId);
        }
        break;
      }
      case 'release': {
        const hold = this.holds.get(entry.holdId);
        if (hold && hold.state === 'held') {
          hold.state = 'released';
          hold.pendingTxHash = undefined;
          this.credit(hold.userId, hold.amountMicro);
        }
        break;
      }
      case 'settle': {
        this.settledRefunds.set(entry.jobId, BigInt(entry.refund));
        const holdId = this.jobToHold.get(entry.jobId);
        const hold = holdId ? this.holds.get(holdId) : undefined;
        if (hold && hold.state === 'bound') {
          hold.state = 'settled';
          this.credit(hold.userId, BigInt(entry.refund));
        }
        break;
      }
      case 'cashout': {
        this.credit(entry.userId, -BigInt(entry.amount));
        if (entry.pi) {
          this.refundedByPi.set(entry.pi, (this.refundedByPi.get(entry.pi) ?? 0n) + BigInt(entry.amount));
        }
        break;
      }
      case 'cashout-reversal': {
        this.credit(entry.userId, BigInt(entry.amount));
        this.refundedByPi.set(entry.pi, (this.refundedByPi.get(entry.pi) ?? 0n) - BigInt(entry.amount));
        break;
      }
    }
  }

  private credit(userId: string, deltaMicro: bigint): void {
    this.available.set(userId, (this.available.get(userId) ?? 0n) + deltaMicro);
  }

  availableMicro(userId: string): bigint {
    return this.available.get(userId) ?? 0n;
  }

  /** Sum of all user balances plus active (unsettled, unreleased) holds — what
   *  the vault must be able to cover (the FC1.4 solvency invariant). */
  outstandingMicro(): bigint {
    let total = 0n;
    for (const balance of this.available.values()) total += balance;
    for (const hold of this.holds.values()) {
      if (hold.state === 'held' || hold.state === 'bound') total += hold.amountMicro;
    }
    return total;
  }

  /** Deposits currently sitting in the marketplace escrow (bound holds — the
   *  create tx confirmed, settlement hasn't). This money has LEFT the hot/
   *  treasury vaults into the contract, so the solvency check must count it as
   *  backing outstanding liabilities, else it would false-alarm on every live
   *  session (M1). */
  escrowedMicro(): bigint {
    let total = 0n;
    for (const hold of this.holds.values()) {
      if (hold.state === 'bound') total += hold.amountMicro;
    }
    return total;
  }

  userForJob(jobId: bigint): string | undefined {
    const holdId = this.jobToHold.get(jobId.toString());
    return holdId ? this.holds.get(holdId)?.userId : undefined;
  }

  /** Which hold a jobId is bound to, if any. Lets a caller that lost the
   *  bind race to the reconcile sweep tell "already bound, by us, to this very
   *  job" (benign) from every other bind failure (not). */
  holdForJob(jobId: bigint): string | undefined {
    return this.jobToHold.get(jobId.toString());
  }

  /**
   * Holds that debited a balance but never bound to a jobId (state `held`) —
   * orphan candidates for the R5 reconciliation job (M2). A crash between the
   * on-chain create and `bindSession` leaves exactly these; each carries the
   * host + deposit needed to match an on-chain SessionJobCreatedForModel (where
   * depositor == vault, same host, same deposit) and bind it, so no refund is
   * lost — only deferred until reconciliation runs.
   */
  unboundHolds(): Array<{ holdId: string; userId: string; host: string; amountMicro: bigint; atMs: number }> {
    const out: Array<{ holdId: string; userId: string; host: string; amountMicro: bigint; atMs: number }> = [];
    for (const hold of this.holds.values()) {
      if (hold.state === 'held') {
        out.push({
          holdId: hold.holdId,
          userId: hold.userId,
          host: hold.host,
          amountMicro: hold.amountMicro,
          atMs: hold.atMs,
        });
      }
    }
    return out;
  }

  /** The exact refund a settled job credited, for cross-checks (FC1.3). */
  refundForJob(jobId: bigint): bigint | undefined {
    return this.settledRefunds.get(jobId.toString());
  }

  /** Jobs the ledger is still waiting on (bound holds, neither settled nor
   *  released) — the work-list for the settlement state sweep. Settling a job
   *  flips its hold to 'settled', so recovered jobs drop out automatically. */
  boundJobIds(): bigint[] {
    const out: bigint[] = [];
    for (const hold of this.holds.values()) {
      if (hold.state === 'bound' && hold.jobId !== undefined) out.push(hold.jobId);
    }
    return out;
  }

  /** Every job the ledger still waits on, with its kind and age — the
   *  reclaimer's work-list. Kind matters there: a training session lives up
   *  to four hours BY DESIGN, and on the chat clock it would read as stranded
   *  at two. */
  boundJobsWithAge(nowMs?: number): Array<{ jobId: bigint; kind: SessionKind; ageMs: number }> {
    const now = nowMs ?? this.now();
    const out: Array<{ jobId: bigint; kind: SessionKind; ageMs: number }> = [];
    for (const hold of this.holds.values()) {
      if (hold.state !== 'bound' || hold.jobId === undefined) continue;
      out.push({ jobId: hold.jobId, kind: hold.kind ?? 'standard', ageMs: now - hold.atMs });
    }
    return out;
  }

  /** Credit a Stripe purchase. Idempotent per Stripe event id (webhook replays
   *  are a no-op). The ONLY way money enters the ledger (Decision 6). The
   *  paymentIntentId is the card charge a later cash-out may refund against. */
  purchase(
    userId: string,
    amountMicro: bigint,
    stripeEventId: string,
    opts?: { paymentIntentId?: string }
  ): Promise<{ applied: boolean }> {
    return this.run(async () => {
      if (amountMicro <= 0n) throw new Error(`purchase amount must be positive, got ${amountMicro}`);
      if (this.seenStripeEvents.has(stripeEventId)) return { applied: false };
      await this.commit({
        t: 'purchase',
        userId,
        amount: amountMicro.toString(),
        eventId: stripeEventId,
        ...(opts?.paymentIntentId ? { pi: opts.paymentIntentId } : {}),
        at: this.now(),
      });
      return { applied: true };
    });
  }

  /** The newest card-backed purchase and what is still refundable against it
   *  (cash-outs go to the original card only — the clean AML posture). NOTE:
   *  this is a SNAPSHOT for the window/pick decision; the authoritative
   *  remaining check runs inside `cashout` under the serial queue, so two
   *  racing cash-outs cannot both refund the same charge (F1). */
  latestRefundablePurchase(
    userId: string
  ): { paymentIntentId: string; remainingMicro: bigint; atMs: number } | undefined {
    const purchases = this.purchasesByUser.get(userId) ?? [];
    // Newest-first, skipping charges already fully refunded so an older charge
    // with room is still reachable (L3) rather than stranding the funds.
    for (let i = purchases.length - 1; i >= 0; i--) {
      const purchase = purchases[i]!;
      if (!purchase.pi) continue;
      const remaining = this.remainingForCharge(purchase.pi);
      if (remaining <= 0n) continue;
      return { paymentIntentId: purchase.pi, remainingMicro: remaining, atMs: purchase.atMs };
    }
    return undefined;
  }

  /** Micro-USDC still refundable against one card charge: total purchased under
   *  that paymentIntent minus what has already been refunded to it. */
  private remainingForCharge(paymentIntentId: string): bigint {
    let purchased = 0n;
    for (const purchases of this.purchasesByUser.values()) {
      for (const purchase of purchases) {
        if (purchase.pi === paymentIntentId) purchased += purchase.amountMicro;
      }
    }
    const remaining = purchased - (this.refundedByPi.get(paymentIntentId) ?? 0n);
    return remaining < 0n ? 0n : remaining;
  }

  /**
   * The ONLY authoriser of a vault spend: runs the gatekeeper and places the
   * hold atomically on the serial queue. Refusal changes nothing.
   */
  /** The gatekeeper's view of one user, for a request of `kind`, at `nowMs`. */
  private viewFor(userId: string, kind: SessionKind, nowMs: number): LedgerView {
    let spentInWindowMicro = 0n;
    let opensInWindow = 0;
    let liveTrainingHolds = 0;
    for (const hold of this.holds.values()) {
      if (hold.userId !== userId) continue;
      const holdKind: SessionKind = hold.kind ?? 'standard';
      // Each kind's daily cap is its own budget (one training deposit is
      // the size of a day of chat), so only holds of the request's kind
      // count toward it. The per-minute open rate stays per user, all kinds.
      if (nowMs - hold.atMs < DAY_MS && holdKind === kind) {
        spentInWindowMicro += hold.amountMicro;
      }
      if (nowMs - hold.atMs < MINUTE_MS) opensInWindow += 1;
      // D11: live = bound, or held (hash or not) younger than LIVE_HOLD_MS.
      if (
        holdKind === 'training' &&
        (hold.state === 'bound' || (hold.state === 'held' && nowMs - hold.atMs < LIVE_HOLD_MS))
      ) {
        liveTrainingHolds += 1;
      }
    }
    return {
      availableMicro: this.availableMicro(userId),
      spentInWindowMicro,
      opensInWindow,
      liveTrainingHolds,
    };
  }

  /**
   * FT1 D4 — the gatekeeper's decision WITHOUT a hold: advice for the session
   * service so every policy refusal precedes any chain read. Runs on the same
   * serial queue as `openHold`, which re-decides under the mutex and is the
   * only authority; a preview that allows may still be refused at the hold.
   */
  previewOpen(
    request: { userId: string; host: string; depositMicro: bigint; kind?: SessionKind; modelId?: string },
    gatekeeper: Gatekeeper
  ): Promise<GateDecision> {
    return this.run(async () => {
      const kind: SessionKind = request.kind ?? 'standard';
      return gatekeeper(this.viewFor(request.userId, kind, this.now()), {
        host: request.host,
        depositMicro: request.depositMicro,
        kind,
        ...(request.modelId !== undefined ? { modelId: request.modelId } : {}),
      });
    });
  }

  openHold(
    request: { userId: string; host: string; depositMicro: bigint; kind?: SessionKind; modelId?: string },
    gatekeeper: Gatekeeper
  ): Promise<OpenHoldResult> {
    return this.run(async () => {
      const nowMs = this.now();
      const kind: SessionKind = request.kind ?? 'standard';
      // `modelId` reaches the gatekeeper (D10) and is NEVER journaled: the
      // fingerprint already carries it and standard lines stay byte-identical.
      const decision = gatekeeper(this.viewFor(request.userId, kind, nowMs), {
        host: request.host,
        depositMicro: request.depositMicro,
        kind,
        ...(request.modelId !== undefined ? { modelId: request.modelId } : {}),
      });
      if (!decision.allow) return { ok: false, reason: decision.reason };

      const holdId = `h${this.holdCounter}`;
      await this.commit({
        t: 'hold',
        holdId,
        userId: request.userId,
        amount: request.depositMicro.toString(),
        host: request.host,
        at: nowMs,
        // Journaled only for a non-standard kind: standard lines stay byte-
        // identical to every line written before kinds existed.
        ...(kind !== 'standard' ? { kind } : {}),
      });
      return { ok: true, holdId };
    });
  }

  /** Record the create tx hash the instant it is submitted (before the
   *  confirmation wait), so a crash before bindSession leaves a deterministically
   *  recoverable orphan (M2). Idempotent-ish: only meaningful on a `held` hold. */
  recordCreatePending(holdId: string, txHash: string): Promise<void> {
    return this.run(async () => {
      const hold = this.holds.get(holdId);
      if (!hold) throw new Error(`no hold ${holdId}`);
      if (hold.state !== 'held') return; // already bound/released — nothing to mark
      await this.commit({ t: 'create-pending', holdId, txHash, at: this.now() });
    });
  }

  /** Held holds whose create tx was submitted (txHash known) but never bound —
   *  the deterministic recovery set for the R5 reconciliation job. */
  pendingCreates(): Array<{
    holdId: string;
    userId: string;
    host: string;
    amountMicro: bigint;
    txHash: string;
    atMs: number;
  }> {
    const out: Array<{ holdId: string; userId: string; host: string; amountMicro: bigint; txHash: string; atMs: number }> = [];
    for (const hold of this.holds.values()) {
      if (hold.state === 'held' && hold.pendingTxHash) {
        out.push({
          holdId: hold.holdId,
          userId: hold.userId,
          host: hold.host,
          amountMicro: hold.amountMicro,
          txHash: hold.pendingTxHash,
          atMs: hold.atMs,
        });
      }
    }
    return out;
  }

  /** Bind a hold to the on-chain jobId from the SessionJobCreatedForModel receipt. */
  bindSession(holdId: string, jobId: bigint): Promise<void> {
    return this.run(async () => {
      const hold = this.holds.get(holdId);
      if (!hold) throw new Error(`no hold ${holdId}`);
      // On-chain jobIds are unique; a second bind to the same jobId means a
      // duplicated receipt/misbind and would misdirect the refund — alarm,
      // never a silent mapping overwrite.
      const existing = this.jobToHold.get(jobId.toString());
      if (existing !== undefined && existing !== holdId) {
        throw new Error(`jobId ${jobId} is already bound to hold ${existing}`);
      }
      if (hold.state !== 'held') throw new Error(`hold ${holdId} is ${hold.state}, cannot bind`);
      await this.commit({ t: 'bind', holdId, jobId: jobId.toString(), at: this.now() });
    });
  }

  /** Reverse a hold whose create tx never produced a session (tx failed). */
  releaseHold(holdId: string): Promise<void> {
    return this.run(async () => {
      const hold = this.holds.get(holdId);
      if (!hold) throw new Error(`no hold ${holdId}`);
      if (hold.state !== 'held') throw new Error(`hold ${holdId} is ${hold.state}, cannot release`);
      await this.commit({ t: 'release', holdId, at: this.now() });
    });
  }

  /**
   * Apply a settlement: credit exactly the on-chain userRefund back and close
   * the hold (the spent remainder left the vault to host/treasury). Idempotent
   * per jobId; unknown jobIds are not ours and change nothing.
   */
  settle(jobId: bigint, userRefundMicro: bigint): Promise<{ applied: boolean }> {
    return this.run(async () => {
      const key = jobId.toString();
      if (this.settledRefunds.has(key)) return { applied: false };
      const holdId = this.jobToHold.get(key);
      const hold = holdId ? this.holds.get(holdId) : undefined;
      if (!hold || hold.state !== 'bound') return { applied: false };
      if (userRefundMicro < 0n || userRefundMicro > hold.amountMicro) {
        // More back than we put in = our event mapping is wrong. Divergence
        // alarm (R5), never a silent credit.
        throw new Error(
          `refund ${userRefundMicro} exceeds recorded deposit ${hold.amountMicro} for job ${key}`
        );
      }
      await this.commit({ t: 'settle', jobId: key, refund: userRefundMicro.toString(), at: this.now() });
      return { applied: true };
    });
  }

  /**
   * Debit for a Stripe refund (Decision 6 — cash only ever leaves via Stripe).
   * When a paymentIntentId is given, the per-charge remaining is checked HERE,
   * inside the serial queue, so concurrent cash-outs can never together refund
   * more than a card was charged (F1 — the on-ramp over-refund vector). The
   * balance and per-charge checks and the commit are one atomic step.
   */
  cashout(userId: string, amountMicro: bigint, opts?: { paymentIntentId?: string }): Promise<CashoutResult> {
    return this.run(async () => {
      if (amountMicro <= 0n) return { ok: false, reason: 'INVALID_AMOUNT' };
      if (this.availableMicro(userId) < amountMicro) return { ok: false, reason: 'INSUFFICIENT_BALANCE' };
      if (opts?.paymentIntentId && amountMicro > this.remainingForCharge(opts.paymentIntentId)) {
        return { ok: false, reason: 'EXCEEDS_REFUNDABLE' };
      }
      await this.commit({
        t: 'cashout',
        userId,
        amount: amountMicro.toString(),
        ...(opts?.paymentIntentId ? { pi: opts.paymentIntentId } : {}),
        at: this.now(),
      });
      return { ok: true };
    });
  }

  /** Compensate a cash-out whose Stripe refund failed after the debit. */
  reverseCashout(userId: string, amountMicro: bigint, paymentIntentId: string): Promise<void> {
    return this.run(async () => {
      if (amountMicro <= 0n) throw new Error(`reversal amount must be positive, got ${amountMicro}`);
      await this.commit({
        t: 'cashout-reversal',
        userId,
        amount: amountMicro.toString(),
        pi: paymentIntentId,
        at: this.now(),
      });
    });
  }
}
