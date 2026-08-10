// R5 / M2 — the reconciliation job: bind (or release) orphaned holds left when
// the backend crashed between the on-chain create and the ledger bind. It reads
// each pending create's tx RECEIPT (deterministic — no fuzzy matching), then:
//   created  -> bind the hold to its jobId (settlement then refunds normally)
//   reverted -> release the hold (the deposit was never escrowed)
//   pending  -> leave it for the next run
import { describe, expect, it } from 'vitest';
import {
  parseCreateReceipt,
  reconcileOrphans,
  type CreateReceiptReader,
  type CreateTxOutcome,
} from '../src/lib/fiat-reconcile';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { makeGatekeeper } from '../src/lib/gatekeeper';
import { ESCROW_INTERFACE } from '../src/lib/escrow';

const HOST = '0xabcd000000000000000000000000000000000001';
const VAULT = '0x8ba1f109551bd432803012645ac136ddd64dba72';
const MODEL = `0x${'ab'.repeat(32)}`;
const DEPOSIT = 500_000n;

const gate = makeGatekeeper({
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 10_000_000n,
  maxOpensPerMinute: 10,
});

async function ledgerWithPending(txHash: string) {
  const ledger = await CreditsLedger.open(new MemoryLedgerStore());
  await ledger.purchase('user-1', 1_000_000n, 'evt_1');
  const open = await ledger.openHold({ userId: 'user-1', host: HOST, depositMicro: DEPOSIT }, gate);
  if (!open.ok) throw new Error('open refused');
  await ledger.recordCreatePending(open.holdId, txHash);
  return { ledger, holdId: open.holdId };
}

function reader(map: Record<string, CreateTxOutcome>): CreateReceiptReader {
  return { read: async (txHash) => map[txHash] ?? { status: 'pending' } };
}

describe('reconcileOrphans', () => {
  it('binds an orphan whose create tx succeeded, so settlement can refund it', async () => {
    const { ledger, holdId } = await ledgerWithPending('0xcreated');
    const messages: string[] = [];
    const result = await reconcileOrphans(ledger, reader({ '0xcreated': { status: 'created', jobId: 777n } }), (m) => messages.push(m));

    expect(result).toEqual({ bound: 1, released: 0, pending: 0 });
    expect(ledger.userForJob(777n)).toBe('user-1');
    expect(ledger.pendingCreates()).toEqual([]);
    // Now a settlement for that job credits the right user.
    await ledger.settle(777n, 456_988n);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n - (DEPOSIT - 456_988n));
    void holdId;
  });

  it('releases an orphan whose create tx REVERTED (deposit never escrowed) and refunds the balance', async () => {
    const { ledger } = await ledgerWithPending('0xreverted');
    const result = await reconcileOrphans(ledger, reader({ '0xreverted': { status: 'reverted' } }), () => {});
    expect(result).toEqual({ bound: 0, released: 1, pending: 0 });
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n); // fully restored
    expect(ledger.pendingCreates()).toEqual([]);
  });

  it('leaves a still-pending (unmined) create for the next run', async () => {
    const { ledger } = await ledgerWithPending('0xunmined');
    const result = await reconcileOrphans(ledger, reader({ '0xunmined': { status: 'pending' } }), () => {});
    expect(result).toEqual({ bound: 0, released: 0, pending: 1 });
    expect(ledger.pendingCreates()).toHaveLength(1); // still there
  });

  it('is a no-op when there are no pending creates', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    const result = await reconcileOrphans(ledger, reader({}), () => {});
    expect(result).toEqual({ bound: 0, released: 0, pending: 0 });
  });

  it('a receipt-read error leaves the orphan untouched (surfaced, retried next run)', async () => {
    const { ledger } = await ledgerWithPending('0xrpcdown');
    const messages: string[] = [];
    const failing: CreateReceiptReader = {
      read: async () => {
        throw new Error('rpc down');
      },
    };
    const result = await reconcileOrphans(ledger, failing, (m) => messages.push(m));
    expect(result).toEqual({ bound: 0, released: 0, pending: 0 });
    expect(ledger.pendingCreates()).toHaveLength(1); // untouched
    expect(messages.join(' ')).toMatch(/rpc down/);
  });

  it('handles several orphans in one pass (bind + release + leave)', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('u', 3_000_000n, 'evt');
    const opens = [];
    for (const tx of ['0xa', '0xb', '0xc']) {
      const o = await ledger.openHold({ userId: 'u', host: HOST, depositMicro: DEPOSIT }, gate);
      if (!o.ok) throw new Error('refused');
      await ledger.recordCreatePending(o.holdId, tx);
      opens.push(o.holdId);
    }
    const result = await reconcileOrphans(
      ledger,
      reader({ '0xa': { status: 'created', jobId: 10n }, '0xb': { status: 'reverted' }, '0xc': { status: 'pending' } }),
      () => {}
    );
    expect(result).toEqual({ bound: 1, released: 1, pending: 1 });
  });
});

describe('parseCreateReceipt (real receipt shapes, no chain)', () => {
  const createdLog = () => {
    const enc = ESCROW_INTERFACE.encodeEventLog('SessionJobCreatedForModel', [842n, VAULT, HOST, MODEL, DEPOSIT]);
    return { topics: enc.topics, data: enc.data };
  };

  it('null receipt (unmined) → pending', () => {
    expect(parseCreateReceipt(null)).toEqual({ status: 'pending' });
  });

  it('failed receipt (status 0) → reverted', () => {
    expect(parseCreateReceipt({ status: 0, logs: [] })).toEqual({ status: 'reverted' });
  });

  it('success with the creation event → created + jobId', () => {
    expect(parseCreateReceipt({ status: 1, logs: [createdLog()] })).toEqual({ status: 'created', jobId: 842n });
  });

  it('success but NO creation event → reverted (never actually opened)', () => {
    expect(parseCreateReceipt({ status: 1, logs: [] })).toEqual({ status: 'reverted' });
  });
});
