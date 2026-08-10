// The external-tick endpoint: auth, disabled states, and that a kick actually
// runs one listener tick. The endpoint is the money path's external heartbeat
// (see the route header for the four-freeze history); these tests pin its gate.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { POST } from '../app/v1/fiat/settlement/tick/route';
import { setProductionSettlementListenerForTest } from '../src/lib/settlement-listener';

const savedToken = process.env.FIAT_ADMIN_TOKEN;

function req(auth?: string): Request {
  return new Request('http://svc/v1/fiat/settlement/tick', {
    method: 'POST',
    headers: auth ? { authorization: auth } : {},
  });
}

beforeEach(() => {
  process.env.FIAT_ADMIN_TOKEN = 'tick-token';
});
afterEach(() => {
  if (savedToken === undefined) delete process.env.FIAT_ADMIN_TOKEN;
  else process.env.FIAT_ADMIN_TOKEN = savedToken;
  setProductionSettlementListenerForTest(undefined);
});

describe('POST /v1/fiat/settlement/tick', () => {
  it('503 when the admin token is unset (endpoint disabled)', async () => {
    delete process.env.FIAT_ADMIN_TOKEN;
    expect((await POST(req('Bearer whatever'))).status).toBe(503);
  });

  it('401 without the exact bearer token', async () => {
    expect((await POST(req())).status).toBe(401);
    expect((await POST(req('Bearer wrong'))).status).toBe(401);
  });

  it('503 when the listener is not running', async () => {
    expect((await POST(req('Bearer tick-token'))).status).toBe(503);
  });

  it('runs exactly one tick and reports it, uncacheable', async () => {
    let ticks = 0;
    setProductionSettlementListenerForTest({
      tick: async () => {
        ticks += 1;
      },
      stop: async () => {},
    });
    const res = await POST(req('Bearer tick-token'));
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ ticked: true });
    expect(res.headers.get('cache-control')).toBe('no-store');
    expect(ticks).toBe(1);
  });
});
