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
