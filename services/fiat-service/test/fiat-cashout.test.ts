// FC1.4 — cash-out: a Stripe refund against the original card only (clean AML
// posture), never USDC. Debit-first with a compensating reversal if Stripe
// fails; the R3 refund window is a placeholder constant for the adviser.
// Plus the solvency invariant: vault holdings >= outstanding ledger money.
import { describe, expect, it, afterEach } from 'vitest';
import { requestCashout, setCashoutServiceForTest, type CashoutDeps } from '../src/lib/fiat-cashout-service';
import { POST as cashoutRoute } from '../app/v1/fiat/cashout/route';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { startSettlementListener } from '../src/lib/settlement-listener';
import { makeGatekeeper } from '../src/lib/gatekeeper';

const DAY_MS = 86_400_000;

describe('ledger purchase provenance (paymentIntentId)', () => {
  it('tracks the newest refundable purchase and its remaining amount across cash-outs', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1' });
    await ledger.purchase('user-1', 3_000_000n, 'evt_2', { paymentIntentId: 'pi_2' });

    expect(ledger.latestRefundablePurchase('user-1')).toMatchObject({
      paymentIntentId: 'pi_2',
      remainingMicro: 3_000_000n,
    });

    await ledger.cashout('user-1', 1_000_000n, { paymentIntentId: 'pi_2' });
    expect(ledger.latestRefundablePurchase('user-1')).toMatchObject({
      paymentIntentId: 'pi_2',
      remainingMicro: 2_000_000n,
    });
    expect(ledger.availableMicro('user-1')).toBe(7_000_000n);
  });

  it('a cashout reversal restores both the balance and the refundable remainder', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1' });
    await ledger.cashout('user-1', 2_000_000n, { paymentIntentId: 'pi_1' });
    await ledger.reverseCashout('user-1', 2_000_000n, 'pi_1');
    expect(ledger.availableMicro('user-1')).toBe(5_000_000n);
    expect(ledger.latestRefundablePurchase('user-1')).toMatchObject({ remainingMicro: 5_000_000n });
  });

  it('purchases without a paymentIntentId are never refundable', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 5_000_000n, 'evt_1');
    expect(ledger.latestRefundablePurchase('user-1')).toBeUndefined();
  });

  it('skips a fully-refunded newest charge and reaches an older one with room (L3)', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 3_000_000n, 'evt_old', { paymentIntentId: 'pi_old' });
    await ledger.purchase('user-1', 2_000_000n, 'evt_new', { paymentIntentId: 'pi_new' });
    // Fully refund the newest charge.
    await ledger.cashout('user-1', 2_000_000n, { paymentIntentId: 'pi_new' });
    // The next refundable is the OLDER charge, not undefined.
    expect(ledger.latestRefundablePurchase('user-1')).toMatchObject({
      paymentIntentId: 'pi_old',
      remainingMicro: 3_000_000n,
    });
  });
});

function fakeStripe(opts?: { fail?: boolean }) {
  const refunds: Array<{ paymentIntentId: string; amountCents: number }> = [];
  return {
    refunds,
    stripe: {
      createRefund: async (paymentIntentId: string, amountCents: number) => {
        if (opts?.fail) throw new Error('stripe down');
        refunds.push({ paymentIntentId, amountCents });
        return { id: `re_${refunds.length}` };
      },
    },
  };
}

async function cashoutDeps(opts?: {
  fail?: boolean;
  nowMs?: number;
  purchaseAtMs?: number;
  windowDays?: number;
}) {
  const nowMs = opts?.nowMs ?? 1_700_000_000_000;
  let clock = opts?.purchaseAtMs ?? nowMs;
  const ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => clock });
  const credentials = await FiatCredentials.open(new MemoryLedgerStore());
  const token = await credentials.issue('user-1');
  await ledger.purchase('user-1', 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1' });
  clock = nowMs; // subsequent ledger writes happen "now"
  const { stripe, refunds } = fakeStripe(opts);
  const deps: CashoutDeps = {
    ledger,
    credentials,
    stripe,
    refundWindowDays: opts?.windowDays ?? 90,
    now: () => nowMs,
  };
  return { deps, ledger, token, refunds };
}

describe('requestCashout', () => {
  it('debits the ledger and issues the Stripe refund in cents against the original card', async () => {
    const { deps, ledger, token, refunds } = await cashoutDeps();
    const outcome = await requestCashout(deps, { credential: token, amountMicro: 2_000_000n });
    expect(outcome).toEqual({ status: 'ok', refundId: 're_1', amountMicro: 2_000_000n });
    expect(refunds).toEqual([{ paymentIntentId: 'pi_1', amountCents: 200 }]);
    expect(ledger.availableMicro('user-1')).toBe(3_000_000n);
  });

  it('rejects bad credentials and bad amounts before anything moves', async () => {
    const { deps, refunds } = await cashoutDeps();
    expect(await requestCashout(deps, { credential: 'nope', amountMicro: 1_000_000n })).toEqual({
      status: 'unauthorised',
    });
    const { deps: d2, token } = await cashoutDeps();
    expect(await requestCashout(d2, { credential: token, amountMicro: 0n })).toEqual({
      status: 'refused',
      reason: 'INVALID_AMOUNT',
    });
    expect(await requestCashout(d2, { credential: token, amountMicro: 15_000n })).toEqual({
      status: 'refused',
      reason: 'NOT_CENT_ALIGNED',
    });
    expect(refunds).toHaveLength(0);
  });

  it('enforces the R3 refund window against the purchase date', async () => {
    const nowMs = 1_700_000_000_000;
    const { deps, token, refunds } = await cashoutDeps({
      nowMs,
      purchaseAtMs: nowMs - 91 * DAY_MS,
      windowDays: 90,
    });
    expect(await requestCashout(deps, { credential: token, amountMicro: 1_000_000n })).toEqual({
      status: 'refused',
      reason: 'REFUND_WINDOW_EXPIRED',
    });
    expect(refunds).toHaveLength(0);
  });

  it('refuses more than the original charge has left to refund', async () => {
    const { deps, token } = await cashoutDeps();
    // Balance can exceed the newest charge (settled render refunds accrue) —
    // top the balance up beyond pi_1's 5 USDC.
    await deps.ledger.purchase('user-1', 2_000_000n, 'evt_extra'); // no paymentIntent
    expect(await requestCashout(deps, { credential: token, amountMicro: 6_000_000n })).toEqual({
      status: 'refused',
      reason: 'EXCEEDS_REFUNDABLE',
    });
  });

  it('refuses over-balance even when the charge would allow it', async () => {
    const { deps, token } = await cashoutDeps();
    await deps.ledger.cashout('user-1', 4_000_000n, { paymentIntentId: 'pi_1' });
    // Balance now 1 USDC, charge remainder 1 USDC — ask for 1.5 USDC.
    expect(await requestCashout(deps, { credential: token, amountMicro: 1_500_000n })).toEqual({
      status: 'refused',
      reason: 'EXCEEDS_REFUNDABLE',
    });
  });

  it('a Stripe failure reverses the debit exactly and reports stripe_error', async () => {
    const { deps, ledger, token } = await cashoutDeps({ fail: true });
    const outcome = await requestCashout(deps, { credential: token, amountMicro: 2_000_000n });
    expect(outcome).toMatchObject({ status: 'stripe_error' });
    expect(ledger.availableMicro('user-1')).toBe(5_000_000n);
    expect(ledger.latestRefundablePurchase('user-1')).toMatchObject({ remainingMicro: 5_000_000n });
  });
});

describe('POST /api/fiat/cashout', () => {
  afterEach(() => setCashoutServiceForTest(undefined));

  function post(body: unknown, credential?: string) {
    return cashoutRoute(
      new Request('http://site/api/fiat/cashout', {
        method: 'POST',
        headers: credential ? { authorization: `Bearer ${credential}` } : {},
        body: JSON.stringify(body),
      })
    );
  }

  it('maps outcomes to stable statuses', async () => {
    setCashoutServiceForTest({
      request: async () => ({ status: 'ok', refundId: 're_9', amountMicro: 1_000_000n }),
    });
    const ok = await post({ amountMicro: '1000000' }, 'fc1_t');
    expect(ok.status).toBe(200);
    expect(await ok.json()).toEqual({ refundId: 're_9', amountMicro: '1000000' });

    setCashoutServiceForTest({ request: async () => ({ status: 'refused', reason: 'REFUND_WINDOW_EXPIRED' }) });
    const refused = await post({ amountMicro: '1000000' }, 'fc1_t');
    expect(refused.status).toBe(403);
    expect(await refused.json()).toEqual({ error: 'refused', reason: 'REFUND_WINDOW_EXPIRED' });
  });

  it('401s without a credential and 400s on bad amounts, before the service', async () => {
    let called = 0;
    setCashoutServiceForTest({
      request: async () => {
        called += 1;
        return { status: 'unauthorised' };
      },
    });
    expect((await post({ amountMicro: '1000000' })).status).toBe(401);
    expect((await post({ amountMicro: 'x' }, 'fc1_t')).status).toBe(400);
    expect((await post({}, 'fc1_t')).status).toBe(400);
    expect(called).toBe(0);
  });
});

describe('solvency invariant (FC1.4 reconciliation)', () => {
  it('alarms when vault holdings drop below outstanding ledger money', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 5_000_000n, 'evt_1');
    const alarms: string[] = [];
    let holdings = 5_000_000n;
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor: { load: async () => 5, save: async () => {} },
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      solvency: { holdings: async () => holdings },
      manual: true,
    });
    await listener.tick();
    expect(alarms).toEqual([]);
    holdings = 4_999_999n;
    await listener.tick();
    expect(alarms).toHaveLength(1);
    expect(alarms[0]).toMatch(/solvency/i);
    await listener.stop();
  });

  it('does NOT alarm on an open session, when its deposit is in escrow (M1)', async () => {
    const gate = makeGatekeeper({
      allowedHosts: ['0xabcd000000000000000000000000000000000001'],
      maxDepositPerSessionMicro: 2_000_000n,
      maxDailySpendMicro: 10_000_000n,
      maxOpensPerMinute: 10,
    });
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 5_000_000n, 'evt_1');
    const open = await ledger.openHold(
      { userId: 'user-1', host: '0xabcd000000000000000000000000000000000001', depositMicro: 500_000n },
      gate
    );
    if (!open.ok) throw new Error('open refused');
    await ledger.bindSession(open.holdId, 900n);

    // The vault now holds 4.5 USDC (0.5 went into escrow); outstanding is still
    // 5.0 (4.5 available + 0.5 bound). backing = 4.5 + 0.5 escrow == 5.0 -> no alarm.
    const alarms: string[] = [];
    const listener = startSettlementListener({
      ledger,
      source: { latestBlock: async () => 10, query: async () => [] },
      cursor: { load: async () => 5, save: async () => {} },
      fromBlock: 0,
      onAlarm: (m) => alarms.push(m),
      solvency: { holdings: async () => 4_500_000n },
      manual: true,
    });
    await listener.tick();
    expect(alarms).toEqual([]);
    await listener.stop();
  });
});
