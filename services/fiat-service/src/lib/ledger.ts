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
import { mkdir, open as openFile, readFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import type {
  GateDecision,
  Gatekeeper,
  GateRefusal,
  LedgerView,
  OpenRequest,
  SessionKind,
} from './gatekeeper';

/** An `OpenRequest` plus the user it is for: everything the ledger needs to
 *  reach a gate decision. It EXTENDS `OpenRequest` on purpose — a new gate
 *  input is then declared once and both `previewOpen` and `openHold` carry
 *  it, so the advice can never drift from the authority. */
export interface GateRequest extends OpenRequest {
  userId: string;
  /** D20 item 2 — the idempotency key, journaled on the hold (never gated). */
  key?: string;
}

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
  /** Whether this process has checked (and, if torn, repaired) the file's
   *  tail since it last could have been left torn. */
  private tailChecked = false;
  /** EVERY file operation on this store (load's repair, and each append's
   *  tail check + repair + write + fsync) runs one at a time through this
   *  queue. The outbox and idempotency journals have no queue of their own
   *  (resolve lines are written outside the vault lock; claims, completes and
   *  R5 releases race), and two unserialised appends could glue a line onto
   *  one still being written, or a stale repair could truncate a line another
   *  append had already synced. Stays alive after a rejection. */
  private queue: Promise<unknown> = Promise.resolve();

  private serial<T>(fn: () => Promise<T>): Promise<T> {
    const run = this.queue.then(fn);
    this.queue = run.catch(() => undefined);
    return run;
  }

  /** `fsync` defaults ON (D20 item 1): every append is synced to disk before
   *  it returns, so a hold or key a vault outbox entry links to can never be
   *  lost to a power cut while the entry (and its mined transaction) survive.
   *  The flag exists only so a test can name which journal it turns off. */
  constructor(
    private readonly path: string,
    private readonly opts: { fsync?: boolean } = {}
  ) {}

  /** The journal's lines. A torn tail (a partial last line with no newline:
   *  a power cut mid-append) is cut off the FILE here, durably, before anything
   *  is appended after it — otherwise the next fsynced line would be glued
   *  onto the fragment and dropped as "torn" at the following boot (D20). */
  load(): Promise<string[]> {
    return this.serial(() => this.loadNow());
  }

  private async loadNow(): Promise<string[]> {
    let text: string;
    try {
      text = await readFile(this.path, 'utf8');
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') {
        this.tailChecked = true;
        return [];
      }
      throw e;
    }
    text = await this.repairTail(text);
    this.tailChecked = true;
    return text.split('\n').filter((l) => l.length > 0);
  }

  /** Truncate a torn tail back to the last newline and fsync the cut. The
   *  torn fragment was never acknowledged (its fsync never returned), so
   *  nothing that was promised durable is lost. Answers the repaired text. */
  private async repairTail(text: string): Promise<string> {
    if (text.length === 0 || text.endsWith('\n')) return text;
    const keep = text.lastIndexOf('\n') + 1;
    const keepBytes = Buffer.byteLength(text.slice(0, keep), 'utf8');
    const handle = await openFile(this.path, 'r+');
    try {
      await handle.truncate(keepBytes);
      await handle.sync();
    } finally {
      await handle.close();
    }
    console.error(
      `[fiat-journal] ${this.path}: cut a torn tail of ${Buffer.byteLength(text, 'utf8') - keepBytes} bytes (a crash mid-append; never acknowledged)`
    );
    return text.slice(0, keep);
  }

  append(line: string): Promise<void> {
    return this.serial(() => this.appendNow(line));
  }

  private async appendNow(line: string): Promise<void> {
    await mkdir(dirname(this.path), { recursive: true });
    if (!this.tailChecked) {
      // Never loaded in this process, or an earlier append failed part-way:
      // repair before appending, so this line starts on a line of its own.
      try {
        await this.repairTail(await readFile(this.path, 'utf8'));
      } catch (e) {
        if ((e as NodeJS.ErrnoException).code !== 'ENOENT') throw e;
      }
      this.tailChecked = true;
    }
    const handle = await openFile(this.path, 'a');
    try {
      await handle.appendFile(`${line}\n`, 'utf8');
      if (this.opts.fsync !== false) await handle.sync();
    } catch (e) {
      this.tailChecked = false; // a partial write may have left a torn tail
      throw e;
    } finally {
      await handle.close();
    }
  }
}

// Journal entry shapes. Amounts are serialised as decimal strings (JSON has no
// bigint); `at` is epoch ms from the injected clock.
type Entry =
  | {
      t: 'purchase';
      userId: string;
      amount: string;
      eventId: string;
      pi?: string;
      at: number;
      /** TX1 — what Stripe charged, in CENTS: the gross (VAT included) and the
       *  VAT in it. Absent on every line written before TX1, when no tax was
       *  charged: the gross is then the net in cents. `amount` stays the NET
       *  in micro-units, which is what was credited. */
      grossCents?: string;
      taxCents?: string;
      /** The Checkout Session id. */
      cs?: string;
      /** Place-of-supply evidence: the billing address country and the card's
       *  country of issue. */
      country?: string;
      cardCountry?: string;
      /** Whether the buyer gave a tax id (it is on their VAT invoice). */
      taxId?: boolean;
      /** The consent recorded with the purchase (legal list 2.5): the version
       *  of the Terms accepted, the 18 or over statement, and the request to
       *  use the credits straight away. */
      terms?: string;
      over18?: boolean;
      useNow?: boolean;
    }
  | { t: 'hold'; holdId: string; userId: string; amount: string; host: string; at: number; kind?: SessionKind; key?: string }
  | { t: 'create-pending'; holdId: string; txHash: string; at: number }
  | { t: 'bind'; holdId: string; jobId: string; at: number }
  | { t: 'release'; holdId: string; at: number }
  | { t: 'settle'; jobId: string; refund: string; at: number }
  | { t: 'cashout'; userId: string; amount: string; pi?: string; at: number; grossCents?: string }
  | { t: 'cashout-reversal'; userId: string; amount: string; pi: string; at: number; grossCents?: string }
  /** TX1 §7 — the journal format marker. A binary that meets a version above
   *  its own refuses to start (as it does on an unknown entry type). */
  | { t: 'format'; v: number; at: number }
  /** TX1 §3 — a paid Checkout the webhook REFUSED (billing country outside
   *  the area we sell to, or Stripe Tax did not complete): nothing credited,
   *  the charge refunded in full. Journaled so the `charge.refunded` that
   *  follows is explained, and for reconciliation. */
  | {
      t: 'purchase-refused';
      userId: string;
      eventId: string;
      pi?: string;
      cs?: string;
      grossCents: string;
      taxCents: string;
      country?: string;
      reason: string;
      refundId?: string;
      /** Whether the charge really was refunded. False when the refusal could
       *  only be alarmed (no payment intent to refund). Absent on lines from
       *  before this flag: read then as "a refund id was kept". */
      refunded?: boolean;
      at: number;
    }
  /** TX1 §5 — one refund request: the debit of every leg, ONE line, so the
   *  request is journaled whole or not at all. A leg is one Stripe refund
   *  against one charge; its key is `cashout:<rid>:<pi>`. */
  | {
      t: 'cashout-legs';
      userId: string;
      rid: string;
      legs: Array<{ pi: string; amount: string; grossCents: string }>;
      at: number;
    }
  | { t: 'cashout-leg-ok'; key: string; refundId: string; at: number }
  | { t: 'cashout-leg-reversed'; key: string; why: string; refundId?: string; eventId?: string; at: number }
  /** TX1 §3 — `charge.refunded`: Stripe's cumulative refunded amount for a
   *  charge (`seenCents`). `grossCents` is the part this ledger had not
   *  already debited (a refund made in the Stripe dashboard) and `amount` its
   *  net share, debited here. Both are "0" when the event only confirms a
   *  refund this service issued. */
  | {
      t: 'stripe-refund';
      pi: string;
      userId: string;
      eventId: string;
      seenCents: string;
      amount: string;
      grossCents: string;
      at: number;
    }
  /** A refund made outside this service that never reached the card: the net
   *  share it debited is credited back. */
  | {
      t: 'stripe-refund-failed';
      pi: string;
      userId: string;
      refundId: string;
      eventId: string;
      amount: string;
      grossCents: string;
      /** The failed refund's own amount (it can exceed `grossCents`, the part
       *  the ledger had debited, when its `charge.refunded` never arrived). */
      failedCents?: string;
      at: number;
    }
  /** TX1 §3 — a chargeback, journaled under the dispute id: `amount` is the
   *  net debited (dispute amount x net/gross of the charge), `grossCents` the
   *  disputed amount. While it is open the user is frozen. */
  | { t: 'dispute'; id: string; pi: string; userId: string; eventId: string; amount: string; grossCents: string; at: number }
  | { t: 'dispute-closed'; id: string; outcome: 'won' | 'lost'; eventId: string; at: number }
  /** Stripe took the disputed funds, or put them back: for reconciliation. */
  | { t: 'dispute-funds'; id: string; kind: 'withdrawn' | 'reinstated'; cents: string; eventId: string; at: number }
  /** D20 item 3 — the one-off marker the first boot of the outbox code
   *  journals. Every hold (and its `create-pending` mirror) written BEFORE it
   *  is handled conservatively: a receipt still binds or releases; otherwise
   *  an alarm, never an automatic release. */
  | { t: 'migration'; at: number }
  /** D20 item 3 — a vault-funded session nobody is credited for (a late
   *  receipt for a released hold, or an outbox create linked to no hold).
   *  Idempotent by `holdId`; swept after the per-kind delay; closed once a
   *  state read shows the session not Active. Counted in the escrow term. */
  | { t: 'vault-orphan'; jobId: string; holdId: string; kind: SessionKind; amount: string; at: number }
  | { t: 'orphan-closed'; holdId: string; at: number };

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
  /** D20 item 2 — the idempotency key this hold was opened under (the hold↔key
   *  link R5 follows). Absent for an unkeyed open and for every pre-deploy hold. */
  key?: string;
  /** Journaled before the `migration` marker (D20 item 3). Derived on replay. */
  preMigration: boolean;
  /** The create tx hash, recorded the instant it is submitted (before the
   *  confirmation wait). A `held` hold carrying this is a crash-recoverable
   *  orphan: reconciliation reads the receipt and binds deterministically (M2). */
  pendingTxHash?: string;
}

const DAY_MS = 24 * 60 * 60 * 1000;
const MINUTE_MS = 60 * 1000;
const MICRO_PER_CENT = 10_000n;

/** TX1 §7 — the journal format this binary writes and understands. 2 = the
 *  TX1 entry types (tax, refunds, disputes). An older binary has no default
 *  case and would silently skip them: ROLLING BACK PAST TX1 NEEDS THE TX1
 *  BINARY. */
export const JOURNAL_FORMAT = 2;

/** One card charge (a PaymentIntent) and what has come back out of it. */
interface Charge {
  pi: string;
  userId: string;
  /** What was credited: the price before tax. */
  netMicro: bigint;
  /** What the card paid, VAT included, and the VAT in it. */
  grossCents: bigint;
  taxCents: bigint;
  atMs: number;
  /** Net already debited from the user for refunds of this charge. */
  refundedNetMicro: bigint;
  /** Gross the ledger knows was refunded (or is being refunded) to the card. */
  refundedGrossCents: bigint;
  /** The highest cumulative `amount_refunded` Stripe has reported. */
  stripeSeenCents: bigint;
  /** Refunds on this charge that Stripe later reported as never reaching the
   *  card. Once this is above zero a `charge.refunded` snapshot can OVERSTATE
   *  what was refunded (it may predate the failure), so the webhook reads the
   *  charge's current state before debiting on one. */
  refundFailures: number;
  sessionId?: string;
  country?: string;
  cardCountry?: string;
  taxIdPresent?: boolean;
}

interface Leg {
  key: string;
  userId: string;
  pi: string;
  amountMicro: bigint;
  grossCents: bigint;
  atMs: number;
  state: 'pending' | 'confirmed' | 'reversed';
  refundId?: string;
}

interface Dispute {
  id: string;
  pi: string;
  userId: string;
  debitMicro: bigint;
  grossCents: bigint;
  atMs: number;
  state: 'open' | 'won' | 'lost';
  withdrawnCents: bigint;
  reinstatedCents: bigint;
}

interface RefusedPurchase {
  userId: string;
  eventId: string;
  pi?: string;
  sessionId?: string;
  grossCents: bigint;
  taxCents: bigint;
  country?: string;
  reason: string;
  refundId?: string;
  refunded: boolean;
  atMs: number;
}

/** Gross cents refunded once `cumNet` of a charge's net has been refunded:
 *  floor(gross x cumNet / net), and EXACTLY the gross once the charge is
 *  emptied, so no cent drifts across partial refunds (TX1 §5). */
function grossAtCumNet(c: Charge, cumNetMicro: bigint): bigint {
  if (cumNetMicro <= 0n || c.netMicro <= 0n) return 0n;
  if (cumNetMicro >= c.netMicro) return c.grossCents;
  return (c.grossCents * cumNetMicro) / c.netMicro;
}

/** The inverse, for a refund this service did not issue: the net that
 *  corresponds to `cumGross` cents refunded (the whole net at the full gross). */
function netAtCumGross(c: Charge, cumGrossCents: bigint): bigint {
  if (cumGrossCents <= 0n || c.grossCents <= 0n) return 0n;
  if (cumGrossCents >= c.grossCents) return c.netMicro;
  return (c.netMicro * cumGrossCents) / c.grossCents;
}

/** A Stripe amount in cents: a non-negative safe integer, or it is refused
 *  BEFORE anything is journaled. A value that `BigInt()` would reject on
 *  replay must never reach the journal: the ledger would then refuse to open. */
function assertCents(what: string, value: unknown): asserts value is number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${what} must be a non-negative integer number of cents, got ${String(value)}`);
  }
}

export function cashoutLegKey(requestId: string, paymentIntentId: string): string {
  return `cashout:${requestId}:${paymentIntentId}`;
}

/** What the webhook journals with a purchase (TX1 §3). */
export interface PurchaseDetails {
  paymentIntentId?: string;
  grossCents?: number;
  taxCents?: number;
  sessionId?: string;
  billingCountry?: string;
  cardCountry?: string;
  taxIdPresent?: boolean;
  consent?: { termsVersion: string; over18: boolean; useNow: boolean };
}

export interface ChargeSnapshot {
  paymentIntentId: string;
  userId: string;
  netMicro: bigint;
  grossCents: bigint;
  taxCents: bigint;
  atMs: number;
  refundedNetMicro: bigint;
  refundedGrossCents: bigint;
  stripeSeenCents: bigint;
  refundFailures: number;
  /** Net still refundable: 0 while a dispute on it is open or lost. */
  remainingNetMicro: bigint;
  disputed: boolean;
  sessionId?: string;
  country?: string;
  cardCountry?: string;
}

export interface CashoutLegSnapshot {
  key: string;
  userId: string;
  paymentIntentId: string;
  amountMicro: bigint;
  grossCents: bigint;
  atMs: number;
  state: 'pending' | 'confirmed' | 'reversed';
  refundId?: string;
}

export interface DisputeSnapshot {
  disputeId: string;
  paymentIntentId: string;
  userId: string;
  debitMicro: bigint;
  grossCents: bigint;
  atMs: number;
  state: 'open' | 'won' | 'lost';
  withdrawnCents: bigint;
  reinstatedCents: bigint;
}

export interface RefusedPurchaseSnapshot {
  userId: string;
  eventId: string;
  paymentIntentId?: string;
  sessionId?: string;
  grossCents: bigint;
  taxCents: bigint;
  country?: string;
  reason: string;
  refundId?: string;
  /** Whether the charge was refunded (false: alarmed only, nothing went back). */
  refunded: boolean;
  atMs: number;
}

export interface PlannedLeg {
  key: string;
  paymentIntentId: string;
  amountMicro: bigint;
  grossCents: bigint;
}
export type PlanCashoutReason =
  | CashoutReason
  | 'NO_REFUNDABLE_PURCHASE'
  | 'REFUND_WINDOW_EXPIRED'
  | 'ACCOUNT_FROZEN';
export type PlanCashoutResult =
  | { ok: true; legs: PlannedLeg[]; grossCents: bigint }
  | { ok: false; reason: PlanCashoutReason };
/** FT1 D11 — how long a `held` (not yet bound) hold still counts as a LIVE
 *  training hold. A create cannot still be in flight after this; an older
 *  `held` hold is an orphan of a pre-existing kind (a crash between openHold
 *  and onSubmitted, or a create tx evicted from a public mempool across a
 *  restart) whose debit stands but which must not lock the user out of
 *  training for ever. A ledger constant on purpose: never an env re-read
 *  (`nowMs − atMs < NaN` is false and would count nothing). */
export const LIVE_HOLD_MS = 2 * 60 * 60 * 1000;

/** A read-only view of one hold, for R5 and the routes. */
export interface HoldSnapshot {
  holdId: string;
  userId: string;
  amountMicro: bigint;
  host: string;
  atMs: number;
  state: 'held' | 'bound' | 'settled' | 'released';
  kind: SessionKind;
  jobId?: bigint;
  key?: string;
  preMigration: boolean;
  pendingTxHash?: string;
}

export interface VaultOrphan {
  holdId: string;
  jobId: bigint;
  kind: SessionKind;
  amountMicro: bigint;
  atMs: number;
}

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
  /** TX1 — every card charge by PaymentIntent, with what was refunded. */
  private charges = new Map<string, Charge>();
  private legs = new Map<string, Leg>();
  private disputes = new Map<string, Dispute>();
  /** Refused purchases, by PaymentIntent (or `evt:<event id>` without one). */
  private refused = new Map<string, RefusedPurchase>();
  /** Refund ids already handled as failed (one refund fails once). */
  private failedRefunds = new Set<string>();
  /** Checkout Session ids already credited or refused (one purchase each). */
  private seenSessions = new Set<string>();
  private formatVersion = 1;
  private holdCounter = 0;
  /** `${userId lowercased} ${key}` -> holdId (D20 item 2). */
  private keyToHold = new Map<string, string>();
  private migrated = false;
  private orphans = new Map<string, VaultOrphan & { closed: boolean }>();
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
      try {
        ledger.apply(entry);
      } catch (e) {
        // TX1 §7 — fail closed: an entry this binary does not understand (a
        // journal written by a NEWER binary) must stop the service, never be
        // skipped: skipping replays wrong balances.
        throw new Error(`ledger journal line ${i + 1} cannot be replayed: ${(e as Error).message}`);
      }
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
        if (entry.cs) this.seenSessions.add(entry.cs);
        const net = BigInt(entry.amount);
        this.credit(entry.userId, net);
        const purchases = this.purchasesByUser.get(entry.userId) ?? [];
        purchases.push({ pi: entry.pi, amountMicro: net, atMs: entry.at });
        this.purchasesByUser.set(entry.userId, purchases);
        if (entry.pi) {
          // Before TX1 no tax was charged: the gross is the net in cents.
          const gross = entry.grossCents !== undefined ? BigInt(entry.grossCents) : net / MICRO_PER_CENT;
          const tax = entry.taxCents !== undefined ? BigInt(entry.taxCents) : 0n;
          const charge = this.charges.get(entry.pi);
          if (charge) {
            charge.netMicro += net;
            charge.grossCents += gross;
            charge.taxCents += tax;
          } else {
            this.charges.set(entry.pi, {
              pi: entry.pi,
              userId: entry.userId,
              netMicro: net,
              grossCents: gross,
              taxCents: tax,
              atMs: entry.at,
              refundedNetMicro: 0n,
              refundedGrossCents: 0n,
              stripeSeenCents: 0n,
              refundFailures: 0,
              ...(entry.cs ? { sessionId: entry.cs } : {}),
              ...(entry.country ? { country: entry.country } : {}),
              ...(entry.cardCountry ? { cardCountry: entry.cardCountry } : {}),
              ...(entry.taxId !== undefined ? { taxIdPresent: entry.taxId } : {}),
            });
          }
        }
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
          ...(entry.key !== undefined ? { key: entry.key } : {}),
          preMigration: !this.migrated,
        });
        if (entry.key !== undefined) {
          this.keyToHold.set(`${entry.userId.toLowerCase()} ${entry.key}`, entry.holdId);
        }
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
        const amount = BigInt(entry.amount);
        this.credit(entry.userId, -amount);
        const charge = entry.pi ? this.charges.get(entry.pi) : undefined;
        if (charge) {
          const gross =
            entry.grossCents !== undefined
              ? BigInt(entry.grossCents)
              : grossAtCumNet(charge, charge.refundedNetMicro + amount) - charge.refundedGrossCents;
          charge.refundedNetMicro += amount;
          charge.refundedGrossCents += gross;
        }
        break;
      }
      case 'cashout-reversal': {
        const amount = BigInt(entry.amount);
        this.credit(entry.userId, amount);
        const charge = this.charges.get(entry.pi);
        if (charge) {
          const gross =
            entry.grossCents !== undefined
              ? BigInt(entry.grossCents)
              : charge.refundedGrossCents - grossAtCumNet(charge, charge.refundedNetMicro - amount);
          charge.refundedNetMicro -= amount;
          charge.refundedGrossCents -= gross;
        }
        break;
      }
      case 'format': {
        if (!Number.isInteger(entry.v) || entry.v > JOURNAL_FORMAT) {
          throw new Error(
            `journal format ${String(entry.v)} is newer than this binary understands (${JOURNAL_FORMAT}) — run the binary that wrote it`
          );
        }
        if (entry.v > this.formatVersion) this.formatVersion = entry.v;
        break;
      }
      case 'purchase-refused': {
        this.seenStripeEvents.add(entry.eventId);
        if (entry.cs) this.seenSessions.add(entry.cs);
        this.refused.set(entry.pi ?? `evt:${entry.eventId}`, {
          userId: entry.userId,
          eventId: entry.eventId,
          ...(entry.pi ? { pi: entry.pi } : {}),
          ...(entry.cs ? { sessionId: entry.cs } : {}),
          grossCents: BigInt(entry.grossCents),
          taxCents: BigInt(entry.taxCents),
          ...(entry.country ? { country: entry.country } : {}),
          reason: entry.reason,
          ...(entry.refundId ? { refundId: entry.refundId } : {}),
          refunded: entry.refunded ?? Boolean(entry.refundId),
          atMs: entry.at,
        });
        break;
      }
      case 'cashout-legs': {
        for (const leg of entry.legs) {
          const amount = BigInt(leg.amount);
          const gross = BigInt(leg.grossCents);
          this.credit(entry.userId, -amount);
          const charge = this.charges.get(leg.pi);
          if (charge) {
            charge.refundedNetMicro += amount;
            charge.refundedGrossCents += gross;
          }
          const key = cashoutLegKey(entry.rid, leg.pi);
          this.legs.set(key, {
            key,
            userId: entry.userId,
            pi: leg.pi,
            amountMicro: amount,
            grossCents: gross,
            atMs: entry.at,
            state: 'pending',
          });
        }
        break;
      }
      case 'cashout-leg-ok': {
        const leg = this.legs.get(entry.key);
        if (leg && leg.state === 'pending') {
          leg.state = 'confirmed';
          leg.refundId = entry.refundId;
        }
        break;
      }
      case 'cashout-leg-reversed': {
        if (entry.eventId) this.seenStripeEvents.add(entry.eventId);
        if (entry.refundId) this.failedRefunds.add(entry.refundId);
        const leg = this.legs.get(entry.key);
        if (leg && leg.state !== 'reversed') {
          leg.state = 'reversed';
          this.credit(leg.userId, leg.amountMicro);
          const charge = this.charges.get(leg.pi);
          if (charge) {
            charge.refundedNetMicro -= leg.amountMicro;
            charge.refundedGrossCents -= leg.grossCents;
            if (entry.refundId) {
              // The refund was MADE and then failed: Stripe's cumulative figure
              // drops by it too. Left as it was, a later reversal's re-check
              // would read the stale figure as an unexplained refund and debit
              // credits for money that never reached the card.
              charge.refundFailures += 1;
              charge.stripeSeenCents =
                charge.stripeSeenCents > leg.grossCents ? charge.stripeSeenCents - leg.grossCents : 0n;
            }
          }
        }
        break;
      }
      case 'stripe-refund': {
        this.seenStripeEvents.add(entry.eventId);
        const charge = this.charges.get(entry.pi);
        if (charge) {
          const seen = BigInt(entry.seenCents);
          if (seen > charge.stripeSeenCents) charge.stripeSeenCents = seen;
          charge.refundedNetMicro += BigInt(entry.amount);
          charge.refundedGrossCents += BigInt(entry.grossCents);
        }
        this.credit(entry.userId, -BigInt(entry.amount));
        break;
      }
      case 'stripe-refund-failed': {
        this.seenStripeEvents.add(entry.eventId);
        this.failedRefunds.add(entry.refundId);
        const charge = this.charges.get(entry.pi);
        if (charge) {
          const gross = BigInt(entry.grossCents);
          charge.refundedNetMicro -= BigInt(entry.amount);
          charge.refundedGrossCents -= gross;
          charge.refundFailures += 1;
          const failed = entry.failedCents !== undefined ? BigInt(entry.failedCents) : gross;
          charge.stripeSeenCents = charge.stripeSeenCents > failed ? charge.stripeSeenCents - failed : 0n;
        }
        this.credit(entry.userId, BigInt(entry.amount));
        break;
      }
      case 'dispute': {
        this.seenStripeEvents.add(entry.eventId);
        if (!this.disputes.has(entry.id)) {
          this.disputes.set(entry.id, {
            id: entry.id,
            pi: entry.pi,
            userId: entry.userId,
            debitMicro: BigInt(entry.amount),
            grossCents: BigInt(entry.grossCents),
            atMs: entry.at,
            state: 'open',
            withdrawnCents: 0n,
            reinstatedCents: 0n,
          });
          this.credit(entry.userId, -BigInt(entry.amount));
        }
        break;
      }
      case 'dispute-closed': {
        this.seenStripeEvents.add(entry.eventId);
        const dispute = this.disputes.get(entry.id);
        if (dispute && dispute.state === 'open') {
          dispute.state = entry.outcome;
          // Won: exactly the journaled debit comes back. Lost: it stays.
          if (entry.outcome === 'won') this.credit(dispute.userId, dispute.debitMicro);
        }
        break;
      }
      case 'dispute-funds': {
        this.seenStripeEvents.add(entry.eventId);
        const dispute = this.disputes.get(entry.id);
        if (dispute) {
          if (entry.kind === 'withdrawn') dispute.withdrawnCents += BigInt(entry.cents);
          else dispute.reinstatedCents += BigInt(entry.cents);
        }
        break;
      }
      case 'migration': {
        this.migrated = true;
        break;
      }
      case 'vault-orphan': {
        if (!this.orphans.has(entry.holdId)) {
          this.orphans.set(entry.holdId, {
            holdId: entry.holdId,
            jobId: BigInt(entry.jobId),
            kind: entry.kind,
            amountMicro: BigInt(entry.amount),
            atMs: entry.at,
            closed: false,
          });
        }
        break;
      }
      case 'orphan-closed': {
        const orphan = this.orphans.get(entry.holdId);
        if (orphan) orphan.closed = true;
        break;
      }
      default: {
        // TX1 §7 — fail closed. Before TX1 there was no default case, so an
        // older binary silently ignored entry types it did not know.
        throw new Error(`unknown journal entry type "${String((entry as { t?: unknown }).t)}"`);
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
    // TX1 §3 — only what we OWE: a refund or chargeback can take a balance
    // below zero, and netting that debt against other users' credits would
    // hide a shortfall from the solvency alarm. Debts are `receivableMicro`.
    for (const balance of this.available.values()) {
      if (balance > 0n) total += balance;
    }
    for (const hold of this.holds.values()) {
      if (hold.state === 'held' || hold.state === 'bound') total += hold.amountMicro;
    }
    return total;
  }

  /** TX1 §3 — the total users owe us: the sum of the balances below zero (a
   *  refund or chargeback landed after the credits were spent). A separate
   *  receivable with its own alarm; never netted into `outstandingMicro`. */
  receivableMicro(): bigint {
    let total = 0n;
    for (const balance of this.available.values()) {
      if (balance < 0n) total -= balance;
    }
    return total;
  }

  /** The users whose balance is below zero, and by how much. */
  negativeBalances(): Array<{ userId: string; owedMicro: bigint }> {
    const out: Array<{ userId: string; owedMicro: bigint }> = [];
    for (const [userId, balance] of this.available) {
      if (balance < 0n) out.push({ userId, owedMicro: -balance });
    }
    return out;
  }

  /** TX1 §3 — a user with an OPEN dispute is frozen: no new session and no
   *  refund until it closes. (After a lost dispute the debit stays; a balance
   *  below zero then refuses sessions and refunds by itself.) */
  isFrozen(userId: string): boolean {
    for (const dispute of this.disputes.values()) {
      if (dispute.userId === userId && dispute.state === 'open') return true;
    }
    return false;
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
    // D20 item 3 — an open vault orphan's deposit is vault money in escrow
    // too (nobody is credited for it), so it backs the vault like a bound hold.
    for (const orphan of this.orphans.values()) {
      if (!orphan.closed) total += orphan.amountMicro;
    }
    return total;
  }

  // ---- D20: the migration marker, the hold↔key link, vault orphans ----

  hasMigrationMarker(): boolean {
    return this.migrated;
  }

  /** The first boot of the outbox code journals this once (buildBackend). */
  writeMigrationMarker(): Promise<void> {
    return this.run(async () => {
      if (this.migrated) return;
      await this.commit({ t: 'migration', at: this.now() });
    });
  }

  /** TX1 §7 — the journal format marker, written once by the first boot of
   *  this binary (buildBackend). */
  journalFormat(): number {
    return this.formatVersion;
  }

  writeFormatMarker(): Promise<void> {
    return this.run(async () => {
      if (this.formatVersion >= JOURNAL_FORMAT) return;
      await this.commit({ t: 'format', v: JOURNAL_FORMAT, at: this.now() });
    });
  }

  private snapshot(hold: Hold): HoldSnapshot {
    return {
      holdId: hold.holdId,
      userId: hold.userId,
      amountMicro: hold.amountMicro,
      host: hold.host,
      atMs: hold.atMs,
      state: hold.state,
      kind: hold.kind ?? 'standard',
      ...(hold.jobId !== undefined ? { jobId: hold.jobId } : {}),
      ...(hold.key !== undefined ? { key: hold.key } : {}),
      preMigration: hold.preMigration,
      ...(hold.pendingTxHash ? { pendingTxHash: hold.pendingTxHash } : {}),
    };
  }

  hold(holdId: string): HoldSnapshot | undefined {
    const hold = this.holds.get(holdId);
    return hold ? this.snapshot(hold) : undefined;
  }

  allHolds(): HoldSnapshot[] {
    return [...this.holds.values()].map((h) => this.snapshot(h));
  }

  /** The hold opened under (user, key), if any (D20 item 2). */
  holdForKey(userId: string, key: string): HoldSnapshot | undefined {
    const holdId = this.keyToHold.get(`${userId.toLowerCase()} ${key}`);
    return holdId ? this.hold(holdId) : undefined;
  }

  /** Bind only if still `held` — R5's idempotent verdict (overlapping ticks). */
  bindIfHeld(holdId: string, jobId: bigint): Promise<boolean> {
    return this.run(async () => {
      const hold = this.holds.get(holdId);
      if (!hold || hold.state !== 'held') return false;
      const existing = this.jobToHold.get(jobId.toString());
      if (existing !== undefined && existing !== holdId) {
        throw new Error(`jobId ${jobId} is already bound to hold ${existing}`);
      }
      await this.commit({ t: 'bind', holdId, jobId: jobId.toString(), at: this.now() });
      return true;
    });
  }

  /** Release only if still `held` — R5's idempotent verdict. */
  releaseIfHeld(holdId: string): Promise<boolean> {
    return this.run(async () => {
      const hold = this.holds.get(holdId);
      if (!hold || hold.state !== 'held') return false;
      await this.commit({ t: 'release', holdId, at: this.now() });
      return true;
    });
  }

  /** Journal a vault orphan, idempotently by `holdId` (overlapping ticks never
   *  count one deposit twice in the escrow term). True when newly recorded. */
  recordVaultOrphan(orphan: { holdId: string; jobId: bigint; kind: SessionKind; amountMicro: bigint }): Promise<boolean> {
    return this.run(async () => {
      if (this.orphans.has(orphan.holdId)) return false;
      await this.commit({
        t: 'vault-orphan',
        jobId: orphan.jobId.toString(),
        holdId: orphan.holdId,
        kind: orphan.kind,
        amount: orphan.amountMicro.toString(),
        at: this.now(),
      });
      return true;
    });
  }

  /** Close an orphan once its session is seen not Active (never swept twice). */
  closeVaultOrphan(holdId: string): Promise<boolean> {
    return this.run(async () => {
      const orphan = this.orphans.get(holdId);
      if (!orphan || orphan.closed) return false;
      await this.commit({ t: 'orphan-closed', holdId, at: this.now() });
      return true;
    });
  }

  openVaultOrphans(): VaultOrphan[] {
    return [...this.orphans.values()]
      .filter((o) => !o.closed)
      .map(({ closed: _closed, ...o }) => o);
  }

  /** The gatekeeper's view of one user (the headroom route's figures). */
  headroomView(userId: string, kind: SessionKind = 'standard'): LedgerView {
    return this.viewFor(userId, kind, this.now());
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
    return this.boundJobsWithAge().map((job) => job.jobId);
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
    opts?: PurchaseDetails
  ): Promise<{ applied: boolean; duplicate?: 'session' | 'payment_intent' }> {
    return this.run(async () => {
      if (amountMicro <= 0n) throw new Error(`purchase amount must be positive, got ${amountMicro}`);
      if (opts?.grossCents !== undefined) assertCents('purchase grossCents', opts.grossCents);
      if (opts?.taxCents !== undefined) assertCents('purchase taxCents', opts.taxCents);
      if (this.seenStripeEvents.has(stripeEventId)) return { applied: false };
      // Stripe can emit two event objects (two ids) for one occurrence, and a
      // session can complete as `completed` and as `async_payment_succeeded`.
      // One Checkout Session, or one PaymentIntent, is one purchase.
      const duplicate = this.purchaseKnown({ sessionId: opts?.sessionId, paymentIntentId: opts?.paymentIntentId });
      if (duplicate) return { applied: false, duplicate };
      await this.commit({
        t: 'purchase',
        userId,
        amount: amountMicro.toString(),
        eventId: stripeEventId,
        ...(opts?.paymentIntentId ? { pi: opts.paymentIntentId } : {}),
        at: this.now(),
        // TX1 — journaled only when given, so a purchase without them writes
        // the same line as before.
        ...(opts?.grossCents !== undefined ? { grossCents: String(opts.grossCents) } : {}),
        ...(opts?.taxCents !== undefined ? { taxCents: String(opts.taxCents) } : {}),
        ...(opts?.sessionId ? { cs: opts.sessionId } : {}),
        ...(opts?.billingCountry ? { country: opts.billingCountry } : {}),
        ...(opts?.cardCountry ? { cardCountry: opts.cardCountry } : {}),
        ...(opts?.taxIdPresent !== undefined ? { taxId: opts.taxIdPresent } : {}),
        ...(opts?.consent
          ? { terms: opts.consent.termsVersion, over18: opts.consent.over18, useNow: opts.consent.useNow }
          : {}),
      });
      return { applied: true };
    });
  }

  /** Whether a Stripe event id has already been journaled (any event type). */
  hasStripeEvent(eventId: string): boolean {
    return this.seenStripeEvents.has(eventId);
  }

  /** Whether a purchase (credited or refused) is already journaled for this
   *  Checkout Session or PaymentIntent, and by which of the two. */
  purchaseKnown(ids: { sessionId?: string; paymentIntentId?: string }): 'session' | 'payment_intent' | undefined {
    if (ids.sessionId && this.seenSessions.has(ids.sessionId)) return 'session';
    if (ids.paymentIntentId && (this.charges.has(ids.paymentIntentId) || this.refused.has(ids.paymentIntentId))) {
      return 'payment_intent';
    }
    return undefined;
  }

  /** TX1 §3 — journal a paid Checkout that was refused and refunded in full.
   *  Idempotent per Stripe event id. Nothing is credited. */
  recordRefusedPurchase(refusal: {
    userId: string;
    eventId: string;
    paymentIntentId?: string;
    sessionId?: string;
    grossCents: number;
    taxCents: number;
    billingCountry?: string;
    reason: string;
    refundId?: string;
    /** Whether the charge was refunded. Omitted: true exactly when a refund id is given. */
    refunded?: boolean;
  }): Promise<{ applied: boolean }> {
    return this.run(async () => {
      assertCents('refused purchase grossCents', refusal.grossCents);
      assertCents('refused purchase taxCents', refusal.taxCents);
      if (this.seenStripeEvents.has(refusal.eventId)) return { applied: false };
      if (this.purchaseKnown({ sessionId: refusal.sessionId, paymentIntentId: refusal.paymentIntentId })) {
        return { applied: false };
      }
      await this.commit({
        t: 'purchase-refused',
        userId: refusal.userId,
        eventId: refusal.eventId,
        ...(refusal.paymentIntentId ? { pi: refusal.paymentIntentId } : {}),
        ...(refusal.sessionId ? { cs: refusal.sessionId } : {}),
        grossCents: String(refusal.grossCents),
        taxCents: String(refusal.taxCents),
        ...(refusal.billingCountry ? { country: refusal.billingCountry } : {}),
        reason: refusal.reason,
        ...(refusal.refundId ? { refundId: refusal.refundId } : {}),
        refunded: refusal.refunded ?? Boolean(refusal.refundId),
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

  /** Micro-USDC still refundable against one card charge: the net purchased
   *  under that paymentIntent minus what has already been refunded to it, and
   *  NOTHING while a dispute on it is open or lost (TX1 §3: the disputed
   *  credits must not also be cashed out). */
  private remainingForCharge(paymentIntentId: string): bigint {
    const charge = this.charges.get(paymentIntentId);
    if (!charge) return 0n;
    if (this.disputeBlocks(paymentIntentId)) return 0n;
    const remaining = charge.netMicro - charge.refundedNetMicro;
    return remaining < 0n ? 0n : remaining;
  }

  /** A charge with a dispute that is open or was lost is never refundable. */
  private disputeBlocks(paymentIntentId: string): boolean {
    for (const dispute of this.disputes.values()) {
      if (dispute.pi === paymentIntentId && dispute.state !== 'won') return true;
    }
    return false;
  }

  /** Net already taken from the user for open or lost disputes on a charge. */
  private disputedNet(paymentIntentId: string): bigint {
    let total = 0n;
    for (const dispute of this.disputes.values()) {
      if (dispute.pi === paymentIntentId && dispute.state !== 'won') total += dispute.debitMicro;
    }
    return total;
  }

  private chargeSnapshot(charge: Charge): ChargeSnapshot {
    return {
      paymentIntentId: charge.pi,
      userId: charge.userId,
      netMicro: charge.netMicro,
      grossCents: charge.grossCents,
      taxCents: charge.taxCents,
      atMs: charge.atMs,
      refundedNetMicro: charge.refundedNetMicro,
      refundedGrossCents: charge.refundedGrossCents,
      stripeSeenCents: charge.stripeSeenCents,
      refundFailures: charge.refundFailures,
      remainingNetMicro: this.remainingForCharge(charge.pi),
      disputed: this.disputeBlocks(charge.pi),
      ...(charge.sessionId ? { sessionId: charge.sessionId } : {}),
      ...(charge.country ? { country: charge.country } : {}),
      ...(charge.cardCountry ? { cardCountry: charge.cardCountry } : {}),
    };
  }

  /** One card charge as the ledger holds it (reconciliation, tests). */
  charge(paymentIntentId: string): ChargeSnapshot | undefined {
    const charge = this.charges.get(paymentIntentId);
    return charge ? this.chargeSnapshot(charge) : undefined;
  }

  allCharges(): ChargeSnapshot[] {
    return [...this.charges.values()].map((c) => this.chargeSnapshot(c));
  }

  refusedPurchases(): RefusedPurchaseSnapshot[] {
    return [...this.refused.values()].map((r) => ({
      userId: r.userId,
      eventId: r.eventId,
      ...(r.pi ? { paymentIntentId: r.pi } : {}),
      ...(r.sessionId ? { sessionId: r.sessionId } : {}),
      grossCents: r.grossCents,
      taxCents: r.taxCents,
      ...(r.country ? { country: r.country } : {}),
      reason: r.reason,
      ...(r.refundId ? { refundId: r.refundId } : {}),
      refunded: r.refunded,
      atMs: r.atMs,
    }));
  }

  private legSnapshot(leg: Leg): CashoutLegSnapshot {
    return {
      key: leg.key,
      userId: leg.userId,
      paymentIntentId: leg.pi,
      amountMicro: leg.amountMicro,
      grossCents: leg.grossCents,
      atMs: leg.atMs,
      state: leg.state,
      ...(leg.refundId ? { refundId: leg.refundId } : {}),
    };
  }

  cashoutLeg(key: string): CashoutLegSnapshot | undefined {
    const leg = this.legs.get(key);
    return leg ? this.legSnapshot(leg) : undefined;
  }

  allCashoutLegs(): CashoutLegSnapshot[] {
    return [...this.legs.values()].map((l) => this.legSnapshot(l));
  }

  dispute(disputeId: string): DisputeSnapshot | undefined {
    return this.allDisputes().find((d) => d.disputeId === disputeId);
  }

  allDisputes(): DisputeSnapshot[] {
    return [...this.disputes.values()].map((d) => ({
      disputeId: d.id,
      paymentIntentId: d.pi,
      userId: d.userId,
      debitMicro: d.debitMicro,
      grossCents: d.grossCents,
      atMs: d.atMs,
      state: d.state,
      withdrawnCents: d.withdrawnCents,
      reinstatedCents: d.reinstatedCents,
    }));
  }

  /** The gatekeeper's view of one user, for a request of `kind`, at `nowMs`.
   *  D21: passes holds do not count toward training's concurrency. */
  private viewFor(userId: string, kind: SessionKind, nowMs: number): LedgerView {
    let spentInWindowMicro = 0n;
    let opensInWindow = 0;
    let liveTrainingHolds = 0;
    for (const hold of this.holds.values()) {
      if (hold.userId !== userId) continue;
      const holdKind: SessionKind = hold.kind ?? 'standard';
      // Training's daily cap is its own budget (one training deposit is the
      // size of a day of chat); `passes` shares the STANDARD window (D21), so
      // only holds in the request's window count toward it. The per-minute
      // open rate stays per user, all kinds.
      if (nowMs - hold.atMs < DAY_MS && windowKind(holdKind) === windowKind(kind)) {
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

  /** The gatekeeper's decision for one request at `nowMs`. The caller holds
   *  the serial queue (this must NOT call `run`, which would deadlock).
   *  `previewOpen` and `openHold` share it so the advice and the authority
   *  can never ask different questions: a gate input added here reaches both.
   *  `modelId` reaches the gatekeeper (D10) and is NEVER journaled — the
   *  fingerprint already carries it and standard lines stay byte-identical. */
  private decide(request: GateRequest, gatekeeper: Gatekeeper, nowMs: number): GateDecision {
    // TX1 §3 — a user with an open chargeback opens no new session (their
    // open holds settle normally). Decided here, under the mutex, so the
    // preview and the hold agree. A balance below zero needs no rule of its
    // own: the gatekeeper refuses it as INSUFFICIENT_BALANCE.
    if (this.isFrozen(request.userId)) return { allow: false, reason: 'ACCOUNT_FROZEN' };
    const kind: SessionKind = request.kind ?? 'standard';
    return gatekeeper(this.viewFor(request.userId, kind, nowMs), {
      host: request.host,
      depositMicro: request.depositMicro,
      kind,
      ...(request.modelId !== undefined ? { modelId: request.modelId } : {}),
    });
  }

  /**
   * FT1 D4 — the gatekeeper's decision WITHOUT a hold: advice for the session
   * service so every policy refusal precedes any chain read. Runs on the same
   * serial queue as `openHold`, which re-decides under the mutex and is the
   * only authority; a preview that allows may still be refused at the hold.
   */
  previewOpen(request: GateRequest, gatekeeper: Gatekeeper): Promise<GateDecision> {
    return this.run(async () => this.decide(request, gatekeeper, this.now()));
  }

  /**
   * The ONLY authoriser of a vault spend: runs the gatekeeper and places the
   * hold atomically on the serial queue. Refusal changes nothing.
   */
  openHold(request: GateRequest, gatekeeper: Gatekeeper): Promise<OpenHoldResult> {
    return this.run(async () => {
      const nowMs = this.now();
      const kind: SessionKind = request.kind ?? 'standard';
      const decision = this.decide(request, gatekeeper, nowMs);
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
        // D20 item 2 — the hold↔key link, so R5 can complete or free the key.
        ...(request.key !== undefined ? { key: request.key } : {}),
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
      // D20: after the migration marker the outbox is the record of a create;
      // a hold's own mirrored hash drives recovery only for pre-deploy holds.
      if (hold.state === 'held' && hold.pendingTxHash && hold.preMigration) {
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
      const charge = opts?.paymentIntentId ? this.charges.get(opts.paymentIntentId) : undefined;
      await this.commit({
        t: 'cashout',
        userId,
        amount: amountMicro.toString(),
        ...(opts?.paymentIntentId ? { pi: opts.paymentIntentId } : {}),
        at: this.now(),
        ...(charge
          ? {
              grossCents: (
                grossAtCumNet(charge, charge.refundedNetMicro + amountMicro) - charge.refundedGrossCents
              ).toString(),
            }
          : {}),
      });
      return { ok: true };
    });
  }

  // ---- TX1 §5: refunds across charges, with the VAT paid on them ----

  /** The legs a refund of `amountMicro` credits would take, WITHOUT a commit.
   *  Walks the user's card purchases newest first, inside the refund window,
   *  skipping disputed charges, taking each charge's remaining net until the
   *  amount is covered. Per charge the gross to return is
   *  `floor(gross x cumNetRefunded / net) - refundedGross`, and exactly the
   *  remaining gross on the refund that empties it. The caller holds the
   *  serial queue (or wants a preview: the account page shows the gross
   *  before the customer confirms). */
  private computeLegs(
    userId: string,
    amountMicro: bigint,
    requestId: string,
    windowMs: number,
    nowMs: number
  ): PlanCashoutResult {
    if (amountMicro <= 0n) return { ok: false, reason: 'INVALID_AMOUNT' };
    if (this.isFrozen(userId)) return { ok: false, reason: 'ACCOUNT_FROZEN' };
    const purchases = this.purchasesByUser.get(userId) ?? [];
    const legs: PlannedLeg[] = [];
    const visited = new Set<string>();
    let need = amountMicro;
    let grossCents = 0n;
    let expired = false;
    for (let i = purchases.length - 1; i >= 0 && need > 0n; i--) {
      const pi = purchases[i]!.pi;
      if (!pi || visited.has(pi)) continue;
      visited.add(pi);
      const charge = this.charges.get(pi);
      if (!charge || charge.userId !== userId) continue;
      const remaining = this.remainingForCharge(pi);
      if (remaining <= 0n) continue;
      if (nowMs - charge.atMs > windowMs) {
        expired = true;
        continue;
      }
      const take = remaining < need ? remaining : need;
      const gross = grossAtCumNet(charge, charge.refundedNetMicro + take) - charge.refundedGrossCents;
      if (gross <= 0n) {
        // Cannot happen while gross >= net (tax is never negative); a refund
        // of nothing must never be journaled as a debit.
        throw new Error(`refund leg on ${pi} computes to ${gross} cents for ${take} micro — refusing`);
      }
      legs.push({ key: cashoutLegKey(requestId, pi), paymentIntentId: pi, amountMicro: take, grossCents: gross });
      grossCents += gross;
      need -= take;
    }
    if (need > 0n) {
      if (legs.length === 0) return { ok: false, reason: expired ? 'REFUND_WINDOW_EXPIRED' : 'NO_REFUNDABLE_PURCHASE' };
      return { ok: false, reason: 'EXCEEDS_REFUNDABLE' };
    }
    if (this.availableMicro(userId) < amountMicro) return { ok: false, reason: 'INSUFFICIENT_BALANCE' };
    return { ok: true, legs, grossCents };
  }

  /** What a refund of `amountMicro` would return, VAT included, and against
   *  which charges — advice for the account page; `planCashout` re-decides
   *  under the mutex and is the authority. */
  previewCashout(
    userId: string,
    amountMicro: bigint,
    opts: { windowMs: number; nowMs?: number }
  ): Promise<PlanCashoutResult> {
    return this.run(async () => this.computeLegs(userId, amountMicro, 'preview', opts.windowMs, opts.nowMs ?? this.now()));
  }

  /** The most a user could have refunded now: unspent credits, capped by what
   *  their card charges inside the window still have left. */
  refundableMicro(userId: string, opts: { windowMs: number; nowMs?: number }): bigint {
    if (this.isFrozen(userId)) return 0n;
    const nowMs = opts.nowMs ?? this.now();
    let byCharges = 0n;
    for (const charge of this.charges.values()) {
      if (charge.userId !== userId || nowMs - charge.atMs > opts.windowMs) continue;
      byCharges += this.remainingForCharge(charge.pi);
    }
    const balance = this.availableMicro(userId);
    const capped = byCharges < balance ? byCharges : balance;
    return capped > 0n ? capped : 0n;
  }

  /**
   * DEBIT FIRST (Decision 6): decide the legs and journal every leg's debit
   * as ONE line, inside the serial queue, so concurrent refund requests can
   * never together return more than a card was charged or a balance holds.
   * The caller then issues one Stripe refund per leg under the leg's key and
   * confirms or reverses each.
   */
  planCashout(
    userId: string,
    amountMicro: bigint,
    opts: { requestId: string; windowMs: number; nowMs?: number }
  ): Promise<PlanCashoutResult> {
    return this.run(async () => {
      const plan = this.computeLegs(userId, amountMicro, opts.requestId, opts.windowMs, opts.nowMs ?? this.now());
      if (!plan.ok) return plan;
      for (const leg of plan.legs) {
        if (this.legs.has(leg.key)) throw new Error(`refund request ${opts.requestId} was already journaled`);
      }
      await this.commit({
        t: 'cashout-legs',
        userId,
        rid: opts.requestId,
        legs: plan.legs.map((l) => ({
          pi: l.paymentIntentId,
          amount: l.amountMicro.toString(),
          grossCents: l.grossCents.toString(),
        })),
        at: this.now(),
      });
      return plan;
    });
  }

  /** The Stripe refund for a leg exists. True when newly recorded. */
  confirmCashoutLeg(key: string, refundId: string): Promise<boolean> {
    return this.run(async () => {
      const leg = this.legs.get(key);
      if (!leg || leg.state !== 'pending') return false;
      await this.commit({ t: 'cashout-leg-ok', key, refundId, at: this.now() });
      return true;
    });
  }

  // -------------------------------------------------------------------------
  // THE REFUND INVARIANT. For every charge, once Stripe's events are delivered
  // and no leg is pending:
  //
  //     credits debited for refunds  ==  the net share of cash actually refunded
  //
  // The ledger keeps three figures per charge:
  //   refundedGross  cents the ledger has DEBITED for: own legs that are
  //                  pending or confirmed (never reversed ones) + refunds made
  //                  outside this service that it has learnt of;
  //   refundedNet    the credits taken for exactly those;
  //   stripeSeen     the last trustworthy value of Stripe's cumulative
  //                  `amount_refunded`. It must never OVERSTATE what Stripe
  //                  holds now: `reverseCashoutLeg`'s re-check debits
  //                  `stripeSeen - refundedGross` as an outside refund.
  // and Stripe holds the truth T = refunds made and not failed.
  //
  // How each path keeps it:
  //   own refund succeeds     debited at plan time (before Stripe is called),
  //                           so T's own part never exceeds the ledger's; its
  //                           `charge.refunded` finds nothing unexplained.
  //   Stripe refuses the leg  reversed with NO refund id: nothing was made.
  //   own refund fails        (at once, found failed by the sweep, or reported
  //   before or after its     by `charge.refund.updated`): reversed WITH its
  //   charge.refunded         refund id, which lowers stripeSeen by the leg's
  //                           gross (Stripe's T dropped by it too) and counts a
  //                           failure on the charge. Whichever of the sweep and
  //                           the event comes first does it; the other is a
  //                           no-op (the leg is reversed, the id is recorded).
  //   outside refund          `charge.refunded`: debit T_snapshot -
  //                           refundedGross. A leg pending at that moment can
  //                           hide part of it; it is debited when that leg's
  //                           own `charge.refunded` arrives, or by the re-check
  //                           if the leg is reversed.
  //   outside refund fails    credit back at most what the ledger had debited
  //                           for outside refunds (never an own leg's share).
  //   stale snapshot          T only ever falls when a refund fails. So a
  //                           snapshot is trusted until a failure is recorded
  //                           on the charge; from then on EVERY `charge.refunded`
  //                           for it is replaced by a read of the charge's
  //                           current state (the webhook does the read), for
  //                           debiting AND for stripeSeen.
  //   sweep recovery          a pending leg is confirmed if its refund exists,
  //                           made under the same Idempotency-Key, or reversed
  //                           (with the refund id when one exists and failed).
  //   after ANY failure       the caller (webhook, request path, sweep) reads
  //                           the charge's current state once more and debits
  //                           what the ledger has not: an outside refund whose
  //                           snapshot looked explained by the leg that has
  //                           just been reversed would otherwise never be
  //                           debited, because no further event arrives.
  // Walked for every ordering of these steps, one and two legs, in
  // test/tx1-refund-invariant.test.ts.
  // -------------------------------------------------------------------------

  /**
   * Compensate a pending leg: the debit and the charge's refunded figures
   * return exactly. Two cases, told apart by `refundId`:
   *   - NO refund was made (Stripe refused it, or a look-up found none): no id;
   *   - a refund WAS made and then failed or was cancelled: pass its id, so
   *     Stripe's cumulative figure held here is lowered with it, the failure is
   *     counted on the charge, and the `charge.refund.updated` that follows is
   *     recognised as already handled.
   * A confirmed leg is reversed only by `refundFailed`. True when newly reversed.
   */
  reverseCashoutLeg(key: string, why: string, opts?: { refundId?: string }): Promise<boolean> {
    return this.run(async () => {
      const leg = this.legs.get(key);
      if (!leg || leg.state !== 'pending') return false;
      await this.commit({
        t: 'cashout-leg-reversed',
        key,
        why,
        ...(opts?.refundId ? { refundId: opts.refundId } : {}),
        at: this.now(),
      });
      // A dashboard refund's `charge.refunded` may have arrived while this leg
      // was pending and been (partly) explained by it. With the leg gone, what
      // Stripe reported is unexplained again: debit it now.
      const charge = this.charges.get(leg.pi);
      if (charge && charge.stripeSeenCents > charge.refundedGrossCents) {
        await this.debitUnexplained(charge, charge.stripeSeenCents, `recheck:${key}`);
      }
      return true;
    });
  }

  /** Debit the net share of what Stripe says was refunded beyond what this
   *  ledger has already debited. The caller holds the serial queue. */
  private async debitUnexplained(
    charge: Charge,
    cumulativeCents: bigint,
    eventId: string
  ): Promise<{ debitedMicro: bigint; unexplainedCents: bigint }> {
    const unexplained =
      cumulativeCents > charge.refundedGrossCents ? cumulativeCents - charge.refundedGrossCents : 0n;
    let debit = 0n;
    if (unexplained > 0n) {
      debit = netAtCumGross(charge, charge.refundedGrossCents + unexplained) - charge.refundedNetMicro;
      // Never more than the charge still carries: the net, less what refunds
      // and open or lost disputes have already taken.
      const cap = charge.netMicro - charge.refundedNetMicro - this.disputedNet(charge.pi);
      if (debit > cap) debit = cap;
      if (debit < 0n) debit = 0n;
    }
    if (unexplained > 0n || cumulativeCents > charge.stripeSeenCents) {
      await this.commit({
        t: 'stripe-refund',
        pi: charge.pi,
        userId: charge.userId,
        eventId,
        seenCents: cumulativeCents.toString(),
        amount: debit.toString(),
        grossCents: unexplained.toString(),
        at: this.now(),
      });
    }
    return { debitedMicro: debit, unexplainedCents: unexplained };
  }

  /**
   * TX1 §3 — `charge.refunded`. The single source of truth for refunded
   * amounts is Stripe's CUMULATIVE `amount_refunded` against this ledger's
   * per-charge `refundedGross`. Refunds this service issued are debited before
   * Stripe is called, so they are already inside `refundedGross`; only the
   * unexplained remainder (a refund made in the dashboard) is debited, as its
   * net share. No double debit however the webhook and the API call race.
   */
  chargeRefunded(
    paymentIntentId: string,
    cumulativeRefundedCents: number,
    eventId: string
  ): Promise<
    | { known: false }
    | { known: true; refusedPurchase: true }
    | { known: true; refusedPurchase?: undefined; applied: boolean; userId: string; debitedMicro: bigint; unexplainedCents: bigint; balanceMicro: bigint }
  > {
    return this.run(async () => {
      const charge = this.charges.get(paymentIntentId);
      if (!charge) {
        return this.refused.has(paymentIntentId) ? { known: true as const, refusedPurchase: true as const } : { known: false as const };
      }
      if (this.seenStripeEvents.has(eventId)) {
        return {
          known: true as const,
          applied: false,
          userId: charge.userId,
          debitedMicro: 0n,
          unexplainedCents: 0n,
          balanceMicro: this.availableMicro(charge.userId),
        };
      }
      assertCents(`charge.refunded for ${paymentIntentId}: amount_refunded`, cumulativeRefundedCents);
      const r = await this.debitUnexplained(charge, BigInt(cumulativeRefundedCents), eventId);
      return {
        known: true as const,
        applied: r.unexplainedCents > 0n,
        userId: charge.userId,
        debitedMicro: r.debitedMicro,
        unexplainedCents: r.unexplainedCents,
        balanceMicro: this.availableMicro(charge.userId),
      };
    });
  }

  /**
   * TX1 §3 — a refund that never reached the card (`charge.refund.updated`,
   * status `failed`): credit back what it debited. A refund this service
   * issued is matched by the key it carries in its metadata and reverses
   * exactly its leg; any other reverses its net share. Idempotent per refund.
   */
  refundFailed(failure: {
    refundId: string;
    paymentIntentId: string;
    amountCents: number;
    key?: string;
    eventId: string;
  }): Promise<
    | { kind: 'unknown' }
    | { kind: 'refused-purchase'; userId: string }
    | { kind: 'leg' | 'external'; applied: boolean; userId: string; creditedMicro: bigint }
  > {
    return this.run(async () => {
      assertCents(`failed refund ${failure.refundId} amount`, failure.amountCents);
      const amount = BigInt(failure.amountCents);
      const leg = failure.key ? this.legs.get(failure.key) : undefined;
      if (leg && leg.pi === failure.paymentIntentId && leg.grossCents === amount && this.failedRefunds.has(failure.refundId)) {
        // Already handled: by the sweep or the request path (which reversed the
        // leg WITH this refund's id), or by an earlier delivery of this event.
        return { kind: 'leg' as const, applied: false, userId: leg.userId, creditedMicro: 0n };
      }
      // A leg that is ALREADY reversed, without this refund's id, was reversed
      // because no refund was found; the refund was made afterwards all the
      // same and, if its `charge.refunded` arrived, was debited as a refund
      // made outside this service. Its failure is then handled as one of those
      // (below): credited back only as far as it was debited.
      if (leg && leg.pi === failure.paymentIntentId && leg.grossCents === amount && leg.state !== 'reversed') {
        await this.commit({
          t: 'cashout-leg-reversed',
          key: leg.key,
          why: 'refund_failed',
          refundId: failure.refundId,
          eventId: failure.eventId,
          at: this.now(),
        });
        return { kind: 'leg' as const, applied: true, userId: leg.userId, creditedMicro: leg.amountMicro };
      }
      const charge = this.charges.get(failure.paymentIntentId);
      if (!charge) {
        const refused = this.refused.get(failure.paymentIntentId);
        return refused ? { kind: 'refused-purchase' as const, userId: refused.userId } : { kind: 'unknown' as const };
      }
      if (this.failedRefunds.has(failure.refundId)) {
        return { kind: 'external' as const, applied: false, userId: charge.userId, creditedMicro: 0n };
      }
      // Credit back only what the ledger DEBITED for refunds made outside this
      // service: its refunded figures less its own live legs. A dashboard
      // refund that failed before its `charge.refunded` arrived was never
      // debited, and must not be paid for out of an own leg's share.
      let ownGross = 0n;
      let ownNet = 0n;
      for (const l of this.legs.values()) {
        if (l.pi === charge.pi && l.state !== 'reversed') {
          ownGross += l.grossCents;
          ownNet += l.amountMicro;
        }
      }
      const outsideGross = charge.refundedGrossCents > ownGross ? charge.refundedGrossCents - ownGross : 0n;
      const outsideNet = charge.refundedNetMicro > ownNet ? charge.refundedNetMicro - ownNet : 0n;
      const gross = amount < outsideGross ? amount : outsideGross;
      const newCumGross = charge.refundedGrossCents - gross;
      let credit = gross === outsideGross ? outsideNet : charge.refundedNetMicro - netAtCumGross(charge, newCumGross);
      if (credit > outsideNet) credit = outsideNet;
      if (credit < 0n) credit = 0n;
      await this.commit({
        t: 'stripe-refund-failed',
        pi: charge.pi,
        userId: charge.userId,
        refundId: failure.refundId,
        eventId: failure.eventId,
        amount: credit.toString(),
        grossCents: gross.toString(),
        failedCents: amount.toString(),
        at: this.now(),
      });
      return { kind: 'external' as const, applied: true, userId: charge.userId, creditedMicro: credit };
    });
  }

  // ---- TX1 §3: disputes (chargebacks) ----

  /**
   * `charge.dispute.created`, journaled under the dispute id (idempotent):
   * debit `dispute.amount x net/gross` of that charge — never more than the
   * charge still carries after earlier refunds — and freeze the user. The
   * charge's refundable remainder is 0 from here (see `remainingForCharge`).
   * The balance may go below zero: the credits may already be spent.
   */
  disputeOpened(opened: {
    disputeId: string;
    paymentIntentId: string;
    amountCents: number;
    eventId: string;
  }): Promise<{ known: false } | { known: true; applied: boolean; userId: string; debitedMicro: bigint; balanceMicro: bigint }> {
    return this.run(async () => {
      const charge = this.charges.get(opened.paymentIntentId);
      if (!charge) return { known: false as const };
      const existing = this.disputes.get(opened.disputeId);
      if (existing) {
        return {
          known: true as const,
          applied: false,
          userId: existing.userId,
          debitedMicro: existing.debitMicro,
          balanceMicro: this.availableMicro(existing.userId),
        };
      }
      assertCents(`dispute ${opened.disputeId} amount`, opened.amountCents);
      if (opened.amountCents === 0) throw new Error(`dispute ${opened.disputeId} carries amount 0`);
      let debit = netAtCumGross(charge, BigInt(opened.amountCents));
      const cap = charge.netMicro - charge.refundedNetMicro - this.disputedNet(charge.pi);
      if (debit > cap) debit = cap;
      if (debit < 0n) debit = 0n;
      await this.commit({
        t: 'dispute',
        id: opened.disputeId,
        pi: charge.pi,
        userId: charge.userId,
        eventId: opened.eventId,
        amount: debit.toString(),
        grossCents: String(opened.amountCents),
        at: this.now(),
      });
      return {
        known: true as const,
        applied: true,
        userId: charge.userId,
        debitedMicro: debit,
        balanceMicro: this.availableMicro(charge.userId),
      };
    });
  }

  /**
   * `charge.dispute.closed`: `won` restores exactly the journaled debit (and
   * the charge is refundable again); `lost` keeps it. Either way the dispute
   * is no longer open, so the freeze lifts; after a lost dispute a balance
   * below zero still refuses sessions and refunds until it is made good.
   */
  disputeClosed(closed: {
    disputeId: string;
    outcome: 'won' | 'lost';
    eventId: string;
  }): Promise<
    | { known: false }
    | { known: true; applied: boolean; userId: string; restoredMicro: bigint; refundDebitedMicro: bigint; balanceMicro: bigint }
  > {
    return this.run(async () => {
      const dispute = this.disputes.get(closed.disputeId);
      if (!dispute) return { known: false as const };
      if (dispute.state !== 'open') {
        return {
          known: true as const,
          applied: false,
          userId: dispute.userId,
          restoredMicro: 0n,
          refundDebitedMicro: 0n,
          balanceMicro: this.availableMicro(dispute.userId),
        };
      }
      await this.commit({
        t: 'dispute-closed',
        id: closed.disputeId,
        outcome: closed.outcome,
        eventId: closed.eventId,
        at: this.now(),
      });
      // A refund made while the dispute was open (the usual answer to a card
      // issuer's inquiry is to refund it) was debited only up to what the
      // dispute had not already taken. Now that a win has restored the
      // dispute's debit, that refund's net share is owed: debit it, or the
      // customer keeps both the money and the credits.
      let refundDebitedMicro = 0n;
      const charge = this.charges.get(dispute.pi);
      if (closed.outcome === 'won' && charge) {
        let shortfall = netAtCumGross(charge, charge.refundedGrossCents) - charge.refundedNetMicro;
        const cap = charge.netMicro - charge.refundedNetMicro - this.disputedNet(charge.pi);
        if (shortfall > cap) shortfall = cap;
        if (shortfall > 0n) {
          await this.commit({
            t: 'stripe-refund',
            pi: charge.pi,
            userId: charge.userId,
            eventId: `recheck:${closed.eventId}`,
            seenCents: charge.stripeSeenCents.toString(),
            amount: shortfall.toString(),
            grossCents: '0',
            at: this.now(),
          });
          refundDebitedMicro = shortfall;
        }
      }
      return {
        known: true as const,
        applied: true,
        userId: dispute.userId,
        restoredMicro: closed.outcome === 'won' ? dispute.debitMicro : 0n,
        refundDebitedMicro,
        balanceMicro: this.availableMicro(dispute.userId),
      };
    });
  }

  /** `charge.dispute.funds_withdrawn` / `funds_reinstated`: journaled for
   *  reconciliation only. The dispute fee is a cost line for the accountant,
   *  never a debit on the user. Idempotent per event id. */
  disputeFunds(funds: {
    disputeId: string;
    kind: 'withdrawn' | 'reinstated';
    cents: number;
    eventId: string;
  }): Promise<{ known: boolean; applied: boolean }> {
    return this.run(async () => {
      assertCents(`dispute ${funds.disputeId} funds`, funds.cents);
      if (!this.disputes.has(funds.disputeId)) return { known: false, applied: false };
      if (this.seenStripeEvents.has(funds.eventId)) return { known: true, applied: false };
      await this.commit({
        t: 'dispute-funds',
        id: funds.disputeId,
        kind: funds.kind,
        cents: String(funds.cents),
        eventId: funds.eventId,
        at: this.now(),
      });
      return { known: true, applied: true };
    });
  }

  /** Compensate a cash-out whose Stripe refund failed after the debit. */
  reverseCashout(userId: string, amountMicro: bigint, paymentIntentId: string): Promise<void> {
    return this.run(async () => {
      if (amountMicro <= 0n) throw new Error(`reversal amount must be positive, got ${amountMicro}`);
      const charge = this.charges.get(paymentIntentId);
      await this.commit({
        t: 'cashout-reversal',
        userId,
        amount: amountMicro.toString(),
        pi: paymentIntentId,
        at: this.now(),
        ...(charge
          ? {
              grossCents: (
                charge.refundedGrossCents - grossAtCumNet(charge, charge.refundedNetMicro - amountMicro)
              ).toString(),
            }
          : {}),
      });
    });
  }
}

/** D21 — the daily window a kind's spend counts in: `passes` shares the
 *  standard one (it is not a second daily budget); every other kind has its
 *  own. */
function windowKind(kind: SessionKind): SessionKind {
  return kind === 'passes' ? 'standard' : kind;
}
