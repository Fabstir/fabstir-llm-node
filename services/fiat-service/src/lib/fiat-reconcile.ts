// R5 / M2 — the reconciliation job. A crash between the on-chain session create
// and the ledger `bindSession` leaves an orphaned `held` hold: the deposit is
// escrowed but the refund can't be re-credited until the hold is bound to its
// jobId. Because `openFiatSession` records the create tx hash the instant it is
// submitted (`recordCreatePending`), recovery is DETERMINISTIC — read the tx
// receipt and bind by the jobId in its SessionJobCreatedForModel event. No
// fuzzy host+deposit matching that could misdirect a refund.
import { JsonRpcProvider } from 'ethers';
import { rpcUrl } from './balance';
import { ESCROW_INTERFACE } from './escrow';
import type { CreditsLedger } from './ledger';
import { createEntryDeposit } from './fiat-vault';
import type { IdempotencyStore } from './idempotency';
import type { SessionKind } from './gatekeeper';
import { LATE_RECEIPT_WINDOW_MS, type VaultOutbox } from './vault-outbox';
import { disputeLedgerState, lazyStripeApi, makeStripeReconcileSource } from './stripe';

export type CreateTxOutcome =
  | { status: 'pending' } // not mined yet — try again next run
  | { status: 'reverted' } // mined but failed / no session created — release the hold
  | { status: 'created'; jobId: bigint }; // session exists — bind the hold

export interface CreateReceiptReader {
  read(txHash: string): Promise<CreateTxOutcome>;
}

interface MinimalReceipt {
  status: number | null | undefined;
  logs: ReadonlyArray<{ topics: readonly string[]; data: string }>;
}

const CREATED_TOPIC = ESCROW_INTERFACE.getEvent('SessionJobCreatedForModel')!.topicHash;

/** Classify a create tx receipt. Pure — no chain — so it unit-tests directly. */
export function parseCreateReceipt(receipt: MinimalReceipt | null): CreateTxOutcome {
  if (!receipt) return { status: 'pending' };
  if (receipt.status === 0) return { status: 'reverted' };
  const log = receipt.logs.find((l) => l.topics[0] === CREATED_TOPIC);
  // Mined successfully but no creation event ⇒ the session was never opened;
  // treat as reverted so the hold is released (the deposit isn't escrowed).
  if (!log) return { status: 'reverted' };
  return { status: 'created', jobId: BigInt(log.topics[1]!) };
}

/**
 * Reconcile every pending-create orphan. Idempotent and safe to run on a
 * schedule / at startup — a no-op when there are none. Never guesses: a hold is
 * bound ONLY to the jobId its own create tx produced.
 */
export async function reconcileOrphans(
  ledger: CreditsLedger,
  reader: CreateReceiptReader,
  onEvent: (message: string) => void
): Promise<{ bound: number; released: number; pending: number }> {
  let bound = 0;
  let released = 0;
  let pending = 0;
  for (const orphan of ledger.pendingCreates()) {
    let outcome: CreateTxOutcome;
    try {
      outcome = await reader.read(orphan.txHash);
    } catch (e) {
      onEvent(`reconcile: receipt read failed for hold ${orphan.holdId} (${orphan.txHash}): ${e instanceof Error ? e.message : String(e)}`);
      continue;
    }
    switch (outcome.status) {
      case 'pending':
        pending += 1;
        break;
      case 'reverted':
        try {
          await ledger.releaseHold(orphan.holdId);
          released += 1;
          onEvent(`reconcile: create ${orphan.txHash} reverted → released hold ${orphan.holdId}, refunded ${orphan.userId}`);
        } catch (e) {
          onEvent(`reconcile: release failed for ${orphan.holdId}: ${e instanceof Error ? e.message : String(e)}`);
        }
        break;
      case 'created':
        try {
          await ledger.bindSession(orphan.holdId, outcome.jobId);
          bound += 1;
          onEvent(`reconcile: orphaned hold ${orphan.holdId} → job ${outcome.jobId} (bound; settlement will refund)`);
        } catch (e) {
          onEvent(`reconcile: bind failed for ${orphan.holdId} → job ${outcome.jobId}: ${e instanceof Error ? e.message : String(e)}`);
        }
        break;
    }
  }
  return { bound, released, pending };
}

/** Receipt reader over the real chain. */
export function makeChainReceiptReader(deps?: { provider?: JsonRpcProvider; marketplaceAddress?: string }): CreateReceiptReader {
  const provider = deps?.provider ?? new JsonRpcProvider(rpcUrl());
  return {
    async read(txHash: string): Promise<CreateTxOutcome> {
      const receipt = await provider.getTransactionReceipt(txHash);
      if (!receipt) return { status: 'pending' };
      return parseCreateReceipt({
        status: receipt.status,
        logs: receipt.logs.map((l) => ({ topics: l.topics, data: l.data })),
      });
    },
  };
}

// ---------------------------------------------------------------------------
// D20 item 3 — R5 over the vault outbox. Every key and hold ends, and never
// wrongly, except where an alarm hands it to a person:
//   - a create resolved with a session: its `held` hold is bound and its key
//     completed (the authorisation is re-derived on replay);
//   - a create resolved `reverted`/`dropped`: its `held` hold is released and
//     its key freed; one mined with NO creation event is an alarm (once), its
//     hold left `held` and its key `pending` for a person;
//   - a `held` hold older than 10 min that NO outbox create entry links
//     (nothing was appended, so nothing was broadcast — whatever the hold's
//     own mirrored hash says) is released and its key freed;
//   - a reserved key with no hold older than 10 min is freed; a `pending` key
//     whose hold is no longer `held` follows it (bound/settled → completed,
//     released → freed);
//   - an outbox create whose `link` names no hold: an alarm and, if its receipt
//     shows a session, a `vault-orphan` of the kind with the LONGEST configured
//     reclaim delay (standard, training or passes; training when none given);
//   - a late receipt (within 24 h) for a create whose hold was released: an
//     alarm and a durable `vault-orphan` (kind copied from the hold);
//   - holds and keys in the shared in-flight set (a live request) are skipped;
//   - anything journaled before the `migration` marker: a receipt still binds
//     or releases; otherwise an alarm, raised ONCE, and never an automatic
//     release or free.
// Verdicts are idempotent: the cron route and the 15 s loop may overlap.
// ---------------------------------------------------------------------------

/** A held hold with no outbox create is released after this (and a reserved
 *  key with no hold freed): long enough that no live open can still append. */
export const UNLINKED_RELEASE_MS = 10 * 60 * 1000;

export interface VaultReconcileDeps {
  ledger: CreditsLedger;
  idempotency?: IdempotencyStore;
  outbox: VaultOutbox;
  onAlarm: (message: string) => void;
  now?: () => number;
  /** The sweeper's reclaim delays (the listener's options), so an orphan of
   *  unknown kind waits the longest of them. Absent = training. */
  reclaimAfterMs?: number;
  reclaimAfterMsByKind?: Partial<Record<SessionKind, number>>;
}

/** The kind whose reclaim delay is the longest — what an orphan of unknown
 *  kind must wait so the sweep never fires while its session can be live. A
 *  kind absent from `byKind` waits the standard delay (as the sweeper does);
 *  training wins a tie. */
export function longestReclaimKind(standardMs: number, byKind: Partial<Record<SessionKind, number>> = {}): SessionKind {
  const delay = (k: SessionKind) => (k === 'standard' ? standardMs : (byKind[k] ?? standardMs));
  let longest: SessionKind = 'training';
  for (const k of ['passes', 'standard'] as const) {
    if (delay(k) > delay(longest)) longest = k;
  }
  return longest;
}

export async function reconcileVault(d: VaultReconcileDeps): Promise<void> {
  const now = (d.now ?? Date.now)();
  const { ledger, outbox, idempotency, onAlarm } = d;
  const inFlight = outbox.inFlight;
  const say = (m: string) => onAlarm(`reconcile: ${m}`);
  const orphanKind: SessionKind =
    d.reclaimAfterMs === undefined ? 'training' : longestReclaimKind(d.reclaimAfterMs, d.reclaimAfterMsByKind);

  // The late-receipt watch: a create resolved `dropped` keeps being checked
  // for 24 h; a receipt that turns up re-resolves it (handled just below).
  for (const entry of outbox.all()) {
    if (entry.kind !== 'create' || entry.outcome !== 'dropped' || entry.resolvedAt === undefined) continue;
    if (now - entry.resolvedAt >= LATE_RECEIPT_WINDOW_MS) continue;
    try {
      await outbox.checkLateReceipt(entry);
    } catch (e) {
      say(`late-receipt read failed for ${entry.hash}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  // Outbox create entries → their holds.
  const linked = new Set<string>();
  for (const entry of outbox.all()) {
    if (entry.kind !== 'create') continue;
    linked.add(entry.link);
    if (inFlight.holds.has(entry.link)) continue;
    const hold = ledger.hold(entry.link);
    try {
      if (!hold) {
        outbox.alarmOnce(
          `unlinked-create:${entry.hash}`,
          `outbox create ${entry.hash} (nonce ${entry.nonce}) links no hold ("${entry.link}") — nobody is credited; if it mined, the vault reclaims it`
        );
        if (entry.outcome === 'mined' && entry.jobId !== undefined) {
          const recorded = await ledger.recordVaultOrphan({
            holdId: entry.link === 'unlinked' ? entry.hash : entry.link,
            jobId: entry.jobId,
            // The longest reclaim delay: whatever this session was, the sweep
            // must not fire while it can still be live.
            kind: orphanKind,
            amountMicro: createEntryDeposit(entry.raw),
          });
          if (recorded) onAlarm(`vault-orphan: session ${entry.jobId} from unlinked create ${entry.hash} — reclaim scheduled`);
        }
        continue;
      }
      if (hold.preMigration || !entry.outcome) continue;
      if (entry.outcome === 'mined' && entry.jobId === undefined) {
        // Mined with status 1 but NO SessionJobCreatedForModel event: an event
        // inventory mismatch (createSession stops on the same receipt). Only a
        // revert or a drop proves no session exists, so a person decides: the
        // hold stays `held` and its key `pending` (the key loop leaves a key
        // whose hold is held).
        outbox.alarmOnce(
          `mined-no-event:${entry.hash}`,
          `outbox create ${entry.hash} (nonce ${entry.nonce}) mined with no SessionJobCreatedForModel event — ` +
            `hold ${hold.holdId} kept held and its key pending; read the chain and append the matching bind or release by hand`
        );
        continue;
      }
      if (entry.outcome === 'mined' && entry.jobId !== undefined) {
        if (hold.state === 'held') {
          if (await ledger.bindIfHeld(hold.holdId, entry.jobId)) {
            say(`create ${entry.hash} → job ${entry.jobId}: bound hold ${hold.holdId}`);
          }
        } else if (hold.state === 'released') {
          const recorded = await ledger.recordVaultOrphan({
            holdId: hold.holdId,
            jobId: entry.jobId,
            kind: hold.kind,
            amountMicro: hold.amountMicro,
          });
          if (recorded) {
            onAlarm(
              `vault-orphan: late receipt — create ${entry.hash} mined session ${entry.jobId} for RELEASED hold ${hold.holdId}; ` +
                `the user is untouched, the sweeper reclaims the deposit`
            );
          }
        }
      } else if (hold.state === 'held') {
        if (await ledger.releaseIfHeld(hold.holdId)) {
          say(`create ${entry.hash} ${entry.outcome} → released hold ${hold.holdId}, refunded ${hold.userId}`);
        }
      }
    } catch (e) {
      say(`create ${entry.hash} verdict not applied: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  // Held holds no outbox entry links.
  for (const hold of ledger.allHolds()) {
    if (hold.state !== 'held' || inFlight.holds.has(hold.holdId)) continue;
    try {
      if (hold.preMigration) {
        await reconcilePreMigrationHold(outbox, ledger, hold.holdId, hold.pendingTxHash, say);
        continue;
      }
      if (linked.has(hold.holdId)) continue;
      if (now - hold.atMs < UNLINKED_RELEASE_MS) continue;
      // An outbox append failed in this process: the index may lack a line
      // that is on disk, so "no entry" does not prove "nothing sent". The
      // restart's replay decides (the outbox alarm already asked for it).
      if (outbox.appendFailed()) continue;
      if (await ledger.releaseIfHeld(hold.holdId)) {
        say(`hold ${hold.holdId} has no outbox create after 10 min (nothing was sent) → released`);
      }
    } catch (e) {
      say(`hold ${hold.holdId} not resolved: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  // Keys.
  if (!idempotency) return;
  for (const k of idempotency.pendingKeys()) {
    const id = `${k.userId.toLowerCase()} ${k.key}`;
    if (inFlight.keys.has(id)) continue;
    if (k.clientAddress === undefined) {
      outbox.alarmOnce(
        `pre-migration-key:${id}`,
        `pending key ${k.key} of ${k.userId} was reserved before the outbox deploy — it may stand for a live session; ` +
          `read the chain and append the matching complete/release line by hand`
      );
      continue;
    }
    const hold = ledger.holdForKey(k.userId, k.key);
    try {
      if (!hold) {
        if (now - k.atMs >= UNLINKED_RELEASE_MS) {
          if (await idempotency.releaseIfPending(k.userId, k.key)) say(`key ${k.key} reserved with no hold → freed`);
        }
      } else if ((hold.state === 'bound' || hold.state === 'settled') && hold.jobId !== undefined) {
        if (await idempotency.completeIfPending(k.userId, k.key, hold.jobId, k.clientAddress)) {
          say(`key ${k.key} → job ${hold.jobId} completed`);
        }
      } else if (hold.state === 'released') {
        if (await idempotency.releaseIfPending(k.userId, k.key)) say(`key ${k.key} follows released hold ${hold.holdId} → freed`);
      }
    } catch (e) {
      say(`key ${k.key} not resolved: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
}

/** Pre-deploy holds carry no key link, and the old `onSubmitted` ran after the
 *  send, so a hashless hold may stand for a live session: only a receipt acts. */
async function reconcilePreMigrationHold(
  outbox: VaultOutbox,
  ledger: CreditsLedger,
  holdId: string,
  txHash: string | undefined,
  say: (m: string) => void
): Promise<void> {
  if (!txHash) {
    outbox.alarmOnce(
      `pre-migration-hold:${holdId}`,
      `hold ${holdId} predates the outbox and carries no create hash — never released automatically; read the chain and append release/bind by hand`
    );
    return;
  }
  const receipt = await outbox.timed(outbox.rpc.receipt(txHash), 'receipt read');
  // Mined (not status 0) with NO creation event: an event inventory mismatch,
  // not a revert (`parseCreateReceipt` would call it one). Only a status-0
  // receipt proves no session exists, so a person decides, as for C1.
  if (receipt && receipt.status !== 0 && !receipt.logs.some((l) => l.topics[0] === CREATED_TOPIC)) {
    outbox.alarmOnce(
      `pre-migration-no-event:${holdId}`,
      `hold ${holdId}'s pre-deploy create ${txHash} mined with no SessionJobCreatedForModel event — kept held; ` +
        `read the chain and append the matching bind or release by hand`
    );
    return;
  }
  const outcome = parseCreateReceipt(receipt);
  if (outcome.status === 'created') {
    if (await ledger.bindIfHeld(holdId, outcome.jobId)) say(`pre-deploy create ${txHash} → job ${outcome.jobId}: bound hold ${holdId}`);
  } else if (outcome.status === 'reverted') {
    if (await ledger.releaseIfHeld(holdId)) say(`pre-deploy create ${txHash} reverted → released hold ${holdId}`);
  } else {
    outbox.alarmOnce(
      `pre-migration-create:${holdId}`,
      `hold ${holdId}'s pre-deploy create ${txHash} has no receipt — never released automatically; read the chain and append the matching line by hand`
    );
  }
}

// ---------------------------------------------------------------------------
// TX1 §8 — Stripe against the journal, daily. Stripe is the record of what
// cards paid and got back; the journal is the record of what users were
// credited and debited. They are compared per payment intent:
//   - every paid Checkout Session of ours is in the journal with the same
//     gross, tax and net (or was refused and refunded in full);
//   - what Stripe has refunded on a charge equals what the journal says was
//     refunded, in BOTH directions (a refund whose webhook never arrived; a
//     debit whose refund never happened);
//   - a refund leg still `pending` (the process died between the debit and
//     Stripe's answer) is surfaced with what Stripe shows for its key;
//   - every dispute Stripe has on a charge of ours is in the journal, in the
//     same state;
//   - the total of balances below zero.
// Every finding is an alarm; nothing is changed (a person decides). The Stripe
// account is shared with Fabstir v2, so anything not tied to this ledger or to
// a session carrying fc1UserId is left alone.
// ---------------------------------------------------------------------------

export interface StripeSessionRow {
  sessionId: string;
  paymentIntentId?: string;
  /** `metadata.fc1UserId`; absent = not a session of this product. */
  userId?: string;
  grossCents: number;
  taxCents: number;
  currency: string;
  /** Unix seconds. */
  createdS: number;
}

export interface StripeRefundRow {
  refundId: string;
  paymentIntentId?: string;
  amountCents: number;
  status: string;
  /** `metadata.fc1Key` / `metadata.fc1Origin`, when this service issued it. */
  key?: string;
  origin?: string;
}

export interface StripeDisputeRow {
  disputeId: string;
  paymentIntentId?: string;
  amountCents: number;
  status: string;
}

export interface StripeReconcileSource {
  /** Checkout Sessions with payment_status `paid`, created at or after `sinceS`. */
  listPaidSessions(sinceS: number): Promise<StripeSessionRow[]>;
  listRefunds(sinceS: number): Promise<StripeRefundRow[]>;
  listDisputes(sinceS: number): Promise<StripeDisputeRow[]>;
}

export interface StripeReconcileDeps {
  ledger: CreditsLedger;
  source: StripeReconcileSource;
  onAlarm: (message: string) => void;
  now?: () => number;
  /** How far back to compare. Must cover the refund window: a refund can only
   *  follow its purchase, so every refund of a charge inside the look-back is
   *  inside it too. */
  lookbackMs: number;
  /** A session, or a pending refund leg, younger than this is still in
   *  flight (its webhook or Stripe's answer may not have arrived). */
  graceMs?: number;
}

export interface StripeReconcileReport {
  sessions: number;
  refunds: number;
  disputes: number;
  receivableMicro: bigint;
  findings: string[];
}

const RECONCILE_GRACE_MS = 15 * 60 * 1000;
const CENT_MICRO = 10_000n;

export async function reconcileStripe(d: StripeReconcileDeps): Promise<StripeReconcileReport> {
  const now = (d.now ?? Date.now)();
  const sinceMs = now - d.lookbackMs;
  const sinceS = Math.floor(sinceMs / 1000);
  const grace = d.graceMs ?? RECONCILE_GRACE_MS;
  const findings: string[] = [];
  const find = (message: string) => {
    findings.push(message);
    d.onAlarm(`stripe reconcile: ${message}`);
  };

  const [sessions, refunds, disputes] = await Promise.all([
    d.source.listPaidSessions(sinceS),
    d.source.listRefunds(sinceS),
    d.source.listDisputes(sinceS),
  ]);

  const charges = new Map(d.ledger.allCharges().map((c) => [c.paymentIntentId, c]));
  const refused = new Map(
    d.ledger
      .refusedPurchases()
      .filter((r) => r.paymentIntentId !== undefined)
      .map((r) => [r.paymentIntentId!, r])
  );

  // Charges: gross, tax and net per payment intent.
  for (const s of sessions) {
    if (!s.userId || !s.paymentIntentId) continue; // not ours
    if (now - s.createdS * 1000 < grace) continue; // its webhook may be on the way
    const charge = charges.get(s.paymentIntentId);
    if (!charge) {
      if (!refused.has(s.paymentIntentId)) {
        find(
          `paid session ${s.sessionId} (${s.paymentIntentId}, ${s.grossCents} cents, user ${s.userId}) is not in the ledger: ` +
            `never credited and never refused — its webhook was lost; redeliver the event from the Stripe dashboard`
        );
      }
      continue;
    }
    const net = BigInt(s.grossCents - s.taxCents) * CENT_MICRO;
    if (charge.grossCents !== BigInt(s.grossCents) || charge.taxCents !== BigInt(s.taxCents) || charge.netMicro !== net) {
      find(
        `${s.paymentIntentId}: Stripe has gross ${s.grossCents}, tax ${s.taxCents}, net ${net} micro; ` +
          `the ledger has gross ${charge.grossCents}, tax ${charge.taxCents}, net ${charge.netMicro} micro`
      );
    }
  }

  // Refunds, summed per payment intent (a failed or cancelled one returned nothing).
  const refundedAtStripe = new Map<string, bigint>();
  const refundByKey = new Map<string, StripeRefundRow>();
  for (const r of refunds) {
    if (r.key) refundByKey.set(r.key, r);
    if (!r.paymentIntentId || r.status === 'failed' || r.status === 'canceled') continue;
    refundedAtStripe.set(r.paymentIntentId, (refundedAtStripe.get(r.paymentIntentId) ?? 0n) + BigInt(r.amountCents));
  }

  // Refund legs still pending past the grace period, and the charges they
  // make uncertain (their figures are reported once, here, not twice).
  const uncertain = new Set<string>();
  for (const leg of d.ledger.allCashoutLegs()) {
    if (leg.state !== 'pending') continue;
    uncertain.add(leg.paymentIntentId);
    if (now - leg.atMs < grace) continue;
    const atStripe = refundByKey.get(leg.key);
    find(
      `refund leg ${leg.key} (${leg.grossCents} cents, ${leg.amountMicro} micro, user ${leg.userId}) has been pending for ` +
        `${Math.round((now - leg.atMs) / 60_000)} min: ` +
        (atStripe
          ? `Stripe HAS refund ${atStripe.refundId} (${atStripe.status}) under its key — confirm the leg`
          : `Stripe has no refund under its key — the credits are debited and nothing was returned; reverse the leg`)
    );
  }

  for (const charge of charges.values()) {
    if (charge.atMs < sinceMs || uncertain.has(charge.paymentIntentId)) continue;
    const atStripe = refundedAtStripe.get(charge.paymentIntentId) ?? 0n;
    if (atStripe !== charge.refundedGrossCents) {
      find(
        `${charge.paymentIntentId}: refunded at Stripe ${atStripe} cents, in the ledger ${charge.refundedGrossCents} cents — ` +
          (atStripe > charge.refundedGrossCents
            ? `a refund reached the card that the ledger never debited (user ${charge.userId})`
            : `the ledger debited a refund that Stripe does not show (user ${charge.userId})`)
      );
    }
  }
  for (const r of refused.values()) {
    if (r.atMs < sinceMs) continue;
    const atStripe = refundedAtStripe.get(r.paymentIntentId!) ?? 0n;
    if (atStripe !== r.grossCents) {
      find(
        `${r.paymentIntentId}: a refused purchase (${r.reason}) of ${r.grossCents} cents shows ${atStripe} cents refunded at Stripe — ` +
          `user ${r.userId} has no credits and must have the whole amount back`
      );
    }
  }

  // Disputes.
  const seenDisputes = new Set<string>();
  for (const dispute of disputes) {
    if (!dispute.paymentIntentId || !charges.has(dispute.paymentIntentId)) continue; // not ours
    seenDisputes.add(dispute.disputeId);
    const inLedger = d.ledger.dispute(dispute.disputeId);
    if (!inLedger) {
      find(
        `dispute ${dispute.disputeId} on ${dispute.paymentIntentId} (${dispute.amountCents} cents, ${dispute.status}) is not in the ledger — ` +
          `the disputed credits were never debited; redeliver its events`
      );
      continue;
    }
    // The webhook's own rule (stripe.ts), so the two cannot disagree.
    const expected = disputeLedgerState(dispute.status);
    if (inLedger.state !== expected) {
      find(
        `dispute ${dispute.disputeId} on ${dispute.paymentIntentId}: Stripe says ${dispute.status} (${expected}), the ledger has it ${inLedger.state}`
      );
    }
  }
  for (const dispute of d.ledger.allDisputes()) {
    if (dispute.atMs < sinceMs || seenDisputes.has(dispute.disputeId)) continue;
    find(`dispute ${dispute.disputeId} on ${dispute.paymentIntentId} is in the ledger (${dispute.state}) but Stripe does not list it`);
  }

  // The receivable.
  const receivableMicro = d.ledger.receivableMicro();
  if (receivableMicro > 0n) {
    const debtors = d.ledger.negativeBalances();
    find(
      `balances below zero: ${receivableMicro} micro owed by ${debtors.length} user(s): ` +
        debtors.map((x) => `${x.userId} ${x.owedMicro}`).join(', ')
    );
  }

  return { sessions: sessions.length, refunds: refunds.length, disputes: disputes.length, receivableMicro, findings };
}

type ReconcileOverride = {
  source: StripeReconcileSource;
  onAlarm?: (message: string) => void;
  /** The client the pending-leg recovery uses (the route runs it first). */
  stripe?: import('./stripe').StripeApi;
};

/** What the reconcile route's pending-leg recovery runs with. */
export function productionLegRecovery(ledger: CreditsLedger): {
  ledger: CreditsLedger;
  stripe: import('./stripe').StripeApi;
  onAlarm: (message: string) => void;
  graceMs: number;
} {
  return {
    ledger,
    stripe: reconcileOverride?.stripe ?? lazyStripeApi(),
    onAlarm: reconcileOverride?.onAlarm ?? ((message) => console.error(`[fiat-reconcile] ALARM: ${message}`)),
    graceMs: RECONCILE_GRACE_MS,
  };
}
let reconcileOverride: ReconcileOverride | undefined;

/** Tests inject the Stripe side and the alarm sink the route uses. */
export function setStripeReconcileForTest(o: ReconcileOverride | undefined): void {
  reconcileOverride = o;
}

/** The route's deps: the look-back is FIAT_STRIPE_RECONCILE_DAYS (default 100
 *  days: the 90-day refund window and a margin). */
export function productionStripeReconcile(ledger: CreditsLedger): StripeReconcileDeps {
  const raw = process.env.FIAT_STRIPE_RECONCILE_DAYS;
  const days = raw === undefined || raw === '' ? 100 : Number(raw);
  if (!Number.isInteger(days) || days <= 0) {
    throw new Error(`FIAT_STRIPE_RECONCILE_DAYS must be a positive integer, got "${raw}"`);
  }
  return {
    ledger,
    source: reconcileOverride?.source ?? makeStripeReconcileSource(),
    onAlarm: reconcileOverride?.onAlarm ?? ((message) => console.error(`[fiat-reconcile] ALARM: ${message}`)),
    lookbackMs: days * 24 * 60 * 60 * 1000,
  };
}
