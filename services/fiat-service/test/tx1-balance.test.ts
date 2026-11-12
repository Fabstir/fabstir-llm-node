// Legal list 2.13 — the balance read is closed: it answers the holder of the
// account's credential, not anyone who knows the address. The open lookup
// stays available behind FIAT_BALANCE_OPEN=on for a client that has not moved
// to the credential yet.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { GET } from '../app/v1/fiat/balance/route';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';

const ADDR = '0x1234567890abcdef1234567890abcdef12345678';
const OTHER = '0xabcdefabcdefabcdefabcdefabcdefabcdefabcd';
const NOW = 1_794_000_000_000;

function get(query: string, credential?: string) {
  return GET(
    new Request(`http://fiat/v1/fiat/balance${query}`, {
      headers: credential ? { authorization: `Bearer ${credential}` } : {},
    })
  );
}

describe('GET /v1/fiat/balance (closed by default)', () => {
  let ledger: CreditsLedger;
  let token: string;

  beforeEach(async () => {
    ledger = await CreditsLedger.open(new MemoryLedgerStore(), { now: () => NOW });
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    token = await credentials.issue(ADDR, 'browser');
    await ledger.purchase(ADDR, 5_000_000n, 'evt_1', { paymentIntentId: 'pi_1', grossCents: 600, taxCents: 100 });
    await ledger.purchase(OTHER, 7_000_000n, 'evt_2');
    setFiatDepsForTest({ ledger, credentials });
  });
  afterEach(() => {
    setFiatDepsForTest(undefined);
    delete process.env.FIAT_BALANCE_OPEN;
  });

  it('refuses a lookup by address alone: knowing an address reveals nothing', async () => {
    const res = await get(`?address=${ADDR}`);
    expect(res.status).toBe(401);
    const body = await res.json();
    expect(body).toEqual({ error: 'credential_required' });
    expect(JSON.stringify(body)).not.toContain('5000000');
  });

  it("answers the credential's own account, with what could be refunded now", async () => {
    const res = await get('', token);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      availableMicro: '5000000',
      owedMicro: '0',
      refundableMicro: '5000000',
      frozen: false,
    });
  });

  it("a credential never reads another account, whatever address is asked for", async () => {
    const res = await get(`?address=${OTHER}`, token);
    expect(await res.json()).toMatchObject({ availableMicro: '5000000' }); // ADDR's, not OTHER's 7,000,000
  });

  it('rejects a bad credential', async () => {
    expect((await get('', 'fc1_wrong')).status).toBe(401);
  });

  it('a balance below zero reads as 0 available and the amount owed', async () => {
    await ledger.cashout(ADDR, 5_000_000n);
    await ledger.chargeRefunded('pi_1', 600, 'evt_dash');
    const body = await (await get('', token)).json();
    expect(body).toEqual({ availableMicro: '0', owedMicro: '5000000', refundableMicro: '0', frozen: false });
  });

  it('reports a frozen account', async () => {
    await ledger.disputeOpened({ disputeId: 'dp', paymentIntentId: 'pi_1', amountCents: 300, eventId: 'evt_dp' });
    expect(await (await get('', token)).json()).toMatchObject({ frozen: true, refundableMicro: '0' });
  });

  it('FIAT_BALANCE_OPEN=on restores the lookup by address for a client not yet on the credential', async () => {
    process.env.FIAT_BALANCE_OPEN = 'on';
    const res = await get(`?address=${ADDR}`);
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ availableMicro: '5000000' });
  });
});
