// FC1 — GET /api/fiat/balance: the account page's balance read.
import { afterEach, describe, expect, it } from 'vitest';
import { GET } from '../app/v1/fiat/balance/route';
import { setFiatDepsForTest } from '../src/lib/fiat-session-service';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';

const ADDR = '0x1234567890abcDEF1234567890abcdef12345678';
const ADDR_LC = ADDR.toLowerCase();

afterEach(() => setFiatDepsForTest(undefined));

function get(query: string) {
  return GET(new Request(`http://site/api/fiat/balance${query}`));
}

describe('GET /api/fiat/balance', () => {
  it('returns the balance for the address (matched case-insensitively to the fc1UserId)', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase(ADDR_LC, 5_000_000n, 'evt_1');
    setFiatDepsForTest({ ledger });

    const res = await get(`?address=${ADDR}`); // mixed-case query still resolves
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ availableMicro: '5000000' });
  });

  it('returns zero for an unknown address', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    setFiatDepsForTest({ ledger });
    const res = await get(`?address=${ADDR}`);
    expect(await res.json()).toEqual({ availableMicro: '0' });
  });

  it('400s on a missing or malformed address', async () => {
    setFiatDepsForTest({ ledger: await CreditsLedger.open(new MemoryLedgerStore()) });
    expect((await get('')).status).toBe(400);
    expect((await get('?address=nope')).status).toBe(400);
  });

  it('503s when the fiat backend is not configured', async () => {
    setFiatDepsForTest(undefined);
    delete process.env.FIAT_VAULT_PRIVATE_KEY;
    expect((await get(`?address=${ADDR}`)).status).toBe(503);
  });
});
