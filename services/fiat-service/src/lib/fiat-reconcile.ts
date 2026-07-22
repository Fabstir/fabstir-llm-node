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
