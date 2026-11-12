// TX1 §3/§7 — the journal: lines written before TX1 replay unchanged, every
// TX1 entry replays to the same projection, an entry type this binary does not
// know stops it (fail closed), and balances below zero never hide a shortfall
// from the solvency alarm.
import { describe, expect, it } from 'vitest';
import { CreditsLedger, JOURNAL_FORMAT, MemoryLedgerStore, cashoutLegKey } from '../src/lib/ledger';
import { startSettlementListener } from '../src/lib/settlement-listener';

const DAY_MS = 86_400_000;

async function storeWith(lines: Array<Record<string, unknown>>): Promise<MemoryLedgerStore> {
  const store = new MemoryLedgerStore();
  for (const line of lines) await store.append(JSON.stringify(line));
  return store;
}

describe('TX1 §3 journal lines written before TX1 replay unchanged', () => {
  // The exact shapes the pre-TX1 binary wrote: no gross, tax or country on a
  // purchase; no gross on a cashout or its reversal.
  const OLD = [
    { t: 'purchase', userId: 'u', amount: '5000000', eventId: 'evt_1', pi: 'pi_1', at: 1000 },
    { t: 'purchase', userId: 'u', amount: '2000000', eventId: 'evt_2', at: 2000 },
    { t: 'cashout', userId: 'u', amount: '1500000', pi: 'pi_1', at: 3000 },
    { t: 'cashout', userId: 'u', amount: '1000000', pi: 'pi_1', at: 4000 },
    { t: 'cashout-reversal', userId: 'u', amount: '1000000', pi: 'pi_1', at: 5000 },
    { t: 'migration', at: 6000 },
  ];

  it('balances and refundable remainders are what the old binary computed', async () => {
    const ledger = await CreditsLedger.open(await storeWith(OLD));
    expect(ledger.availableMicro('u')).toBe(5_500_000n);
    expect(ledger.latestRefundablePurchase('u')).toMatchObject({ paymentIntentId: 'pi_1', remainingMicro: 3_500_000n });
    expect(ledger.hasMigrationMarker()).toBe(true);
    // No tax then: the gross is the net in cents, and so is what was refunded.
    expect(ledger.charge('pi_1')).toMatchObject({
      netMicro: 5_000_000n,
      grossCents: 500n,
      taxCents: 0n,
      refundedNetMicro: 1_500_000n,
      refundedGrossCents: 150n,
    });
  });

  it('an old charge refunds cent for cent under the TX1 rules (no VAT was paid on it)', async () => {
    const ledger = await CreditsLedger.open(await storeWith(OLD), { now: () => 7000 });
    const plan = await ledger.planCashout('u', 3_500_000n, { requestId: 'r1', windowMs: 90 * DAY_MS });
    expect(plan).toMatchObject({ ok: true, grossCents: 350n });
    expect(ledger.charge('pi_1')).toMatchObject({ refundedGrossCents: 500n, remainingNetMicro: 0n });
  });

  it('a purchase journaled with no TX1 details writes the same line as before', async () => {
    const store = new MemoryLedgerStore();
    const ledger = await CreditsLedger.open(store, { now: () => 1000 });
    await ledger.purchase('u', 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1' });
    expect((await store.load()).map((l) => JSON.parse(l))).toEqual([
      { t: 'purchase', userId: 'u', amount: '5000000', eventId: 'evt_1', pi: 'pi_1', at: 1000 },
    ]);
  });
});

describe('TX1 §3 every TX1 entry replays to the same projection', () => {
  it('a journal with tax, multi-leg refunds, a failed refund, a dashboard refund and disputes reopens identically', async () => {
    const store = new MemoryLedgerStore();
    let clock = 1_794_000_000_000;
    const ledger = await CreditsLedger.open(store, { now: () => clock });
    await ledger.writeFormatMarker();
    await ledger.purchase('u', 9_990_000n, 'evt_a', { paymentIntentId: 'pi_a', grossCents: 1199, taxCents: 200, billingCountry: 'GB', cardCountry: 'GB' });
    clock += 1000;
    await ledger.purchase('u', 3_000_000n, 'evt_b', { paymentIntentId: 'pi_b', grossCents: 360, taxCents: 60, billingCountry: 'GB' });
    await ledger.purchase('v', 5_000_000n, 'evt_c', { paymentIntentId: 'pi_c', grossCents: 600, taxCents: 100 });
    await ledger.recordRefusedPurchase({ userId: 'w', eventId: 'evt_r', paymentIntentId: 'pi_r', grossCents: 500, taxCents: 0, billingCountry: 'US', reason: 'country', refundId: 're_g' });
    const plan = await ledger.planCashout('u', 4_110_000n, { requestId: 'r1', windowMs: 90 * DAY_MS });
    expect(plan.ok).toBe(true);
    await ledger.confirmCashoutLeg(cashoutLegKey('r1', 'pi_b'), 're_1');
    await ledger.reverseCashoutLeg(cashoutLegKey('r1', 'pi_a'), 'stripe_refused');
    await ledger.planCashout('u', 1_110_000n, { requestId: 'r2', windowMs: 90 * DAY_MS });
    await ledger.confirmCashoutLeg(cashoutLegKey('r2', 'pi_a'), 're_2');
    await ledger.refundFailed({ refundId: 're_2', paymentIntentId: 'pi_a', amountCents: 133, key: cashoutLegKey('r2', 'pi_a'), eventId: 'evt_f' });
    await ledger.chargeRefunded('pi_a', 401, 'evt_d');
    await ledger.refundFailed({ refundId: 're_dash', paymentIntentId: 'pi_a', amountCents: 100, eventId: 'evt_f2' });
    await ledger.disputeOpened({ disputeId: 'dp_1', paymentIntentId: 'pi_c', amountCents: 600, eventId: 'evt_dp' });
    await ledger.disputeFunds({ disputeId: 'dp_1', kind: 'withdrawn', cents: 600, eventId: 'evt_fw' });
    await ledger.disputeClosed({ disputeId: 'dp_1', outcome: 'lost', eventId: 'evt_dc' });
    await ledger.disputeOpened({ disputeId: 'dp_2', paymentIntentId: 'pi_a', amountCents: 300, eventId: 'evt_dp2' });

    const reopened = await CreditsLedger.open(store, { now: () => clock });
    for (const user of ['u', 'v', 'w']) {
      expect(reopened.availableMicro(user)).toBe(ledger.availableMicro(user));
      expect(reopened.isFrozen(user)).toBe(ledger.isFrozen(user));
    }
    expect(reopened.allCharges()).toEqual(ledger.allCharges());
    expect(reopened.allCashoutLegs()).toEqual(ledger.allCashoutLegs());
    expect(reopened.allDisputes()).toEqual(ledger.allDisputes());
    expect(reopened.refusedPurchases()).toEqual(ledger.refusedPurchases());
    expect(reopened.outstandingMicro()).toBe(ledger.outstandingMicro());
    expect(reopened.receivableMicro()).toBe(ledger.receivableMicro());
    expect(reopened.journalFormat()).toBe(JOURNAL_FORMAT);
    // And the replayed events stay idempotent after a restart.
    expect((await reopened.chargeRefunded('pi_a', 401, 'evt_d')).known).toBe(true);
    expect(reopened.availableMicro('u')).toBe(ledger.availableMicro('u'));
    expect(await reopened.disputeOpened({ disputeId: 'dp_1', paymentIntentId: 'pi_c', amountCents: 600, eventId: 'evt_dp' })).toMatchObject({ applied: false });
    expect(reopened.availableMicro('v')).toBe(ledger.availableMicro('v'));
  });
});

describe('TX1 §7 journal safety: fail closed', () => {
  it('an unknown entry type refuses to open the ledger, naming the line', async () => {
    const store = await storeWith([
      { t: 'purchase', userId: 'u', amount: '5000000', eventId: 'evt_1', at: 1000 },
      { t: 'loyalty-bonus', userId: 'u', amount: '9000000', at: 2000 },
    ]);
    await expect(CreditsLedger.open(store)).rejects.toThrow(/line 2.*unknown journal entry type "loyalty-bonus"/);
  });

  it('an unknown entry type on the LAST line is not mistaken for a torn tail', async () => {
    const store = await storeWith([{ t: 'something-new', at: 1 }]);
    await expect(CreditsLedger.open(store)).rejects.toThrow(/unknown journal entry type/);
  });

  it('a journal format newer than this binary refuses to open', async () => {
    const store = await storeWith([{ t: 'format', v: JOURNAL_FORMAT + 1, at: 1 }]);
    await expect(CreditsLedger.open(store)).rejects.toThrow(/newer than this binary/);
  });

  it('the format marker is written once and survives a reopen', async () => {
    const store = new MemoryLedgerStore();
    const ledger = await CreditsLedger.open(store, { now: () => 5 });
    expect(ledger.journalFormat()).toBe(1);
    await ledger.writeFormatMarker();
    await ledger.writeFormatMarker();
    expect((await store.load()).map((l) => JSON.parse(l))).toEqual([{ t: 'format', v: JOURNAL_FORMAT, at: 5 }]);
    expect((await CreditsLedger.open(store)).journalFormat()).toBe(JOURNAL_FORMAT);
  });
});

describe('TX1 §3 balances below zero and the solvency alarm', () => {
  async function twoUsers() {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('good', 3_000_000n, 'evt_g', { paymentIntentId: 'pi_g', grossCents: 360, taxCents: 60 });
    await ledger.purchase('debtor', 5_000_000n, 'evt_d', { paymentIntentId: 'pi_d', grossCents: 600, taxCents: 100 });
    // The debtor spends everything, then charges the purchase back.
    await ledger.cashout('debtor', 5_000_000n);
    await ledger.disputeOpened({ disputeId: 'dp', paymentIntentId: 'pi_d', amountCents: 600, eventId: 'evt_dp' });
    return ledger;
  }

  it('outstanding is the sum of max(balance, 0); the debt is a separate receivable', async () => {
    const ledger = await twoUsers();
    expect(ledger.availableMicro('debtor')).toBe(-5_000_000n);
    // NOT 3,000,000 - 5,000,000: one user's debt never reduces what we owe another.
    expect(ledger.outstandingMicro()).toBe(3_000_000n);
    expect(ledger.receivableMicro()).toBe(5_000_000n);
    expect(ledger.negativeBalances()).toEqual([{ userId: 'debtor', owedMicro: 5_000_000n }]);
  });

  it('the listener alarms on a shortfall a netted sum would have hidden, and on the receivable itself', async () => {
    const ledger = await twoUsers();
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor: { load: async () => 5, save: async () => {} },
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      // The vault holds 1 USDC: short of the 3 owed to `good`. Netting the
      // debtor's -5 in would have read as "owes -2" and stayed silent.
      solvency: { holdings: async () => 1_000_000n },
      manual: true,
    });
    await listener.tick();
    await listener.stop();
    expect(alarms.some((m) => /solvency breach/.test(m) && /3000000/.test(m))).toBe(true);
    expect(alarms.some((m) => /below zero/i.test(m) && /5000000/.test(m))).toBe(true);
  });

  it('a hold release and a settlement refund net against a balance below zero', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('u', 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1', grossCents: 600, taxCents: 100 });
    const gate = () => ({ allow: true as const });
    const a = await ledger.openHold({ userId: 'u', host: '0xh', depositMicro: 2_000_000n }, gate);
    const b = await ledger.openHold({ userId: 'u', host: '0xh', depositMicro: 2_000_000n }, gate);
    if (!a.ok || !b.ok) throw new Error('holds must open');
    await ledger.bindSession(b.holdId, 77n);
    await ledger.chargeRefunded('pi_1', 600, 'evt_full'); // the whole purchase refunded in the dashboard
    expect(ledger.availableMicro('u')).toBe(-4_000_000n);
    await ledger.releaseHold(a.holdId);
    expect(ledger.availableMicro('u')).toBe(-2_000_000n);
    await ledger.settle(77n, 1_500_000n);
    expect(ledger.availableMicro('u')).toBe(-500_000n);
  });
});
