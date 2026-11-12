// Terms section 2: "Each purchase is listed on your account page." GET
// /v1/fiat/purchases answers the holder of the account's credential with that
// account's card payments, refunds and automatic refunds, read from the
// ledger alone (no Stripe call on the read path), and nothing of anyone else's.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { GET } from '../app/v1/fiat/purchases/route';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { CreditsLedger, MemoryLedgerStore, cashoutLegKey } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { purchaseHistory } from '../src/lib/purchase-history';
import { middleware } from '../middleware';
import { NextRequest } from 'next/server';

const ADDR = '0x1234567890abcdef1234567890abcdef12345678';
const OTHER = '0xabcdefabcdefabcdefabcdefabcdefabcdefabcd';
const T0 = 1_794_000_000_000;

function get(credential?: string, query = '') {
  return GET(
    new Request(`http://fiat/v1/fiat/purchases${query}`, {
      headers: credential ? { authorization: `Bearer ${credential}` } : {},
    })
  );
}

describe('GET /v1/fiat/purchases', () => {
  let ledger: CreditsLedger;
  let token: string;
  let otherToken: string;
  let now = T0;

  beforeEach(async () => {
    now = T0;
    ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => now });
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    token = await credentials.issue(ADDR, 'browser');
    otherToken = await credentials.issue(OTHER, 'browser');
    // 500 credits for $6.00 (VAT $1.00), then 1,000 credits for $12.00.
    await ledger.purchase(ADDR, 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1', grossCents: 600, taxCents: 100, sessionId: 'cs_1', billingCountry: 'GB' });
    now = T0 + 60_000;
    await ledger.purchase(ADDR, 10_000_000n, 'evt_2', { paymentIntentId: 'pi_2', grossCents: 1200, taxCents: 200, sessionId: 'cs_2', billingCountry: 'GB' });
    await ledger.purchase(OTHER, 7_000_000n, 'evt_3', { paymentIntentId: 'pi_other', grossCents: 840, taxCents: 140 });
    // A refund of 200 credits.
    now = T0 + 120_000;
    // (refunds draw on the newest payment first)
    const plan = await ledger.planCashout(ADDR, 2_000_000n, { requestId: 'r1', windowMs: 90 * 86_400_000 });
    expect(plan.ok).toBe(true);
    await ledger.confirmCashoutLeg(cashoutLegKey('r1', 'pi_2'), 're_1');
    // A payment that could not be credited and was refunded in full.
    now = T0 + 180_000;
    await ledger.recordRefusedPurchase({ userId: ADDR, eventId: 'evt_r', paymentIntentId: 'pi_r', grossCents: 500, taxCents: 0, billingCountry: 'US', reason: 'country', refundId: 're_g', refunded: true });
    await ledger.recordRefusedPurchase({ userId: OTHER, eventId: 'evt_r2', paymentIntentId: 'pi_r_other', grossCents: 900, taxCents: 0, billingCountry: 'FR', reason: 'country', refunded: true });
    setFiatDepsForTest({ ledger, credentials });
  });
  afterEach(() => setFiatDepsForTest(undefined));

  it('refuses without a credential, with a wrong one, and never takes an address', async () => {
    expect((await get()).status).toBe(401);
    expect(await (await get()).json()).toEqual({ error: 'credential_required' });
    expect((await get('fc1_not_a_credential')).status).toBe(401);
    const byAddress = await get(undefined, `?address=${ADDR}`);
    expect(byAddress.status).toBe(401);
    expect(JSON.stringify(await byAddress.json())).not.toMatch(/pi_|600/);
  });

  it("lists the account's card payments, newest first, in dollars and cents with the VAT shown separately", async () => {
    const res = await get(token);
    expect(res.status).toBe(200);
    expect(res.headers.get('cache-control')).toBe('no-store');
    const body = await res.json();
    expect(body.purchases).toEqual([
      { reference: 'pi_2', at: T0 + 60_000, creditsMicro: '10000000', netCents: 1000, taxCents: 200, grossCents: 1200, refundedGrossCents: 240, disputed: false },
      { reference: 'pi_1', at: T0, creditsMicro: '5000000', netCents: 500, taxCents: 100, grossCents: 600, refundedGrossCents: 0, disputed: false },
    ]);
  });

  it('lists refunds with what went back to the card, VAT included, and their state', async () => {
    const { refunds } = await (await get(token)).json();
    expect(refunds).toEqual([{ reference: 'pi_2', at: T0 + 120_000, creditsMicro: '2000000', netCents: 200, taxCents: 40, grossCents: 240, state: 'confirmed' }]);
  });

  it('lists payments that could not be credited and were refunded automatically', async () => {
    const { automaticRefunds } = await (await get(token)).json();
    expect(automaticRefunds).toEqual([{ reference: 'pi_r', at: T0 + 180_000, grossCents: 500, taxCents: 0, reason: 'country', refunded: true }]);
  });

  it('S5: a refusal that was only alarmed (no payment intent, so no refund was made) is NOT shown as refunded', async () => {
    now = T0 + 240_000;
    await ledger.recordRefusedPurchase({ userId: ADDR, eventId: 'evt_alarm', grossCents: 700, taxCents: 0, reason: 'country', refunded: false });
    const { automaticRefunds } = await (await get(token)).json();
    expect(automaticRefunds[0]).toEqual({ at: T0 + 240_000, grossCents: 700, taxCents: 0, reason: 'country', refunded: false });
    expect(automaticRefunds[1].refunded).toBe(true);
    expect(ledger.refusedPurchases().find((r) => r.eventId === 'evt_alarm')!.refunded).toBe(false);
  });

  it('S5: the refunded flag is journaled, and a line from before it is read by whether a refund id was kept', async () => {
    const store = new MemoryLedgerStore();
    const first = await CreditsLedger.open(store, { now: () => T0 });
    await first.recordRefusedPurchase({ userId: ADDR, eventId: 'evt_a', paymentIntentId: 'pi_a', grossCents: 100, taxCents: 0, reason: 'country', refunded: true });
    await first.recordRefusedPurchase({ userId: ADDR, eventId: 'evt_b', grossCents: 200, taxCents: 0, reason: 'country', refunded: false });
    const lines = await store.load();
    expect(lines.filter((l) => l.includes('"purchase-refused"')).map((l) => JSON.parse(l).refunded)).toEqual([true, false]);
    // Lines as written before the flag existed.
    await store.append(JSON.stringify({ t: 'purchase-refused', userId: ADDR, eventId: 'evt_c', pi: 'pi_c', grossCents: '300', taxCents: '0', reason: 'country', refundId: 're_c', at: T0 }));
    await store.append(JSON.stringify({ t: 'purchase-refused', userId: ADDR, eventId: 'evt_d', grossCents: '400', taxCents: '0', reason: 'country', at: T0 }));
    const reopened = await CreditsLedger.open(store, { now: () => T0 });
    const byEvent = Object.fromEntries(reopened.refusedPurchases().map((r) => [r.eventId, r.refunded]));
    expect(byEvent).toEqual({ evt_a: true, evt_b: false, evt_c: true, evt_d: false });
  });

  it("never shows another account's rows", async () => {
    const mine = JSON.stringify(await (await get(token)).json());
    expect(mine).not.toMatch(/pi_other|pi_r_other|840|900/);
    const theirs = await (await get(otherToken)).json();
    expect(theirs.purchases.map((p: { reference: string }) => p.reference)).toEqual(['pi_other']);
    expect(theirs.refunds).toEqual([]);
    expect(theirs.automaticRefunds.map((p: { reference: string }) => p.reference)).toEqual(['pi_r_other']);
    expect(JSON.stringify(theirs)).not.toMatch(/pi_1|pi_2|"pi_r"/);
  });

  it('says plainly that the ledger holds no invoice link, and carries no internal identifiers', async () => {
    const body = await (await get(token)).json();
    expect(body.invoiceLinks).toBe(false);
    expect(JSON.stringify(body)).not.toMatch(/cs_1|evt_|re_g|re_1|userId|0x1234/);
  });

  it('an account with nothing bought gets three empty lists', async () => {
    const empty = await CreditsLedger.open(new MemoryLedgerStore());
    expect(purchaseHistory(empty, ADDR)).toEqual({ purchases: [], refunds: [], automaticRefunds: [], invoiceLinks: false });
  });

  it('503s when the fiat backend is not configured', async () => {
    setFiatDepsForTest(undefined);
    delete process.env.FIAT_VAULT_PRIVATE_KEY;
    const res = await get(token);
    expect(res.status).toBe(503);
    expect(await res.json()).toEqual({ error: 'service_unavailable' });
  });

  it('S5: the balance, headroom and purchases routes give a caller who is not yet known a generic body, and log the detail', async () => {
    const { GET: balance } = await import('../app/v1/fiat/balance/route');
    const { GET: headroom } = await import('../app/v1/fiat/headroom/route');
    setFiatDepsForTest(undefined);
    delete process.env.FIAT_VAULT_PRIVATE_KEY;
    const logged: string[] = [];
    const original = console.error;
    console.error = (...args: unknown[]) => void logged.push(args.map(String).join(' '));
    try {
      const req = (path: string) => new Request(`http://fiat/v1/fiat/${path}`, { headers: { authorization: 'Bearer whatever' } });
      for (const res of [await balance(req('balance')), await headroom(req('headroom')), await GET(req('purchases'))]) {
        expect(res.status).toBe(503);
        const text = await res.text();
        expect(JSON.parse(text)).toEqual({ error: 'service_unavailable' });
        expect(text).not.toMatch(/FIAT_|KEY|env|configured|\//i);
      }
      process.env.FIAT_BALANCE_OPEN = 'on';
      const open = await balance(new Request(`http://fiat/v1/fiat/balance?address=${ADDR}`));
      expect(await open.json()).toEqual({ error: 'service_unavailable' });
    } finally {
      console.error = original;
      delete process.env.FIAT_BALANCE_OPEN;
    }
    expect(logged.length).toBeGreaterThanOrEqual(4);
    expect(logged.join('\n')).toMatch(/\[fiat\/(balance|headroom|purchases)\] backend unavailable: /);
  });

  it('is a browser-facing route: the account page on another origin may read it', () => {
    process.env.FIAT_CORS_ALLOWED_ORIGINS = 'https://site.example';
    try {
      const res = middleware(new NextRequest('http://fiat/v1/fiat/purchases', { method: 'OPTIONS', headers: { origin: 'https://site.example' } }));
      expect(res.headers.get('access-control-allow-origin')).toBe('https://site.example');
    } finally {
      delete process.env.FIAT_CORS_ALLOWED_ORIGINS;
    }
  });
});
