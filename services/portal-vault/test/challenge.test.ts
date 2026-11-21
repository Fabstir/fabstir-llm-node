// Spec 3.1, R2, R3: the challenge store in isolation. Ported from the fiat
// service's store, with the one deliberate difference of R2: looking a nonce up
// (peek) never burns it; only consume does, and handlers call consume only after
// a verified signature.
import { describe, expect, it } from 'vitest';
import { ChallengeClientLimitError, ChallengeStore, ChallengeStoreFullError } from '../src/challenge.js';

const A = '0xAbCdef0123456789abcDEF0123456789aBCdEf01';
const A_LC = A.toLowerCase();
const B = '0x1111111111111111111111111111111111111111';

function make(opts: { ttlMs?: number; max?: number } = {}) {
  const clock = { t: 1_800_000_000_000 };
  const store = new ChallengeStore({ now: () => clock.t, ttlMs: opts.ttlMs ?? 300_000, max: opts.max ?? 100 });
  return { store, clock };
}

describe('ChallengeStore', () => {
  it('issues a 48 lower-case hex nonce with a five minute life and a lower-case address', () => {
    const { store, clock } = make();
    const c = store.issue(A, 84532, 'read');
    expect(c.nonce).toMatch(/^[0-9a-f]{48}$/);
    expect(c.address).toBe(A_LC);
    expect(c.chainId).toBe(84532);
    expect(c.purpose).toBe('read');
    expect(c.expiresAt).toBe(clock.t + 300_000);
  });

  it('issues idempotently: the same nonce and expiry within the life, whatever the address casing', () => {
    const { store, clock } = make();
    const c1 = store.issue(A, 84532, 'read');
    clock.t += 60_000;
    const c2 = store.issue(A_LC, 84532, 'read');
    expect(c2.nonce).toBe(c1.nonce);
    expect(c2.expiresAt).toBe(c1.expiresAt);
    expect(store.size()).toBe(1);
  });

  it('keeps separate slots per purpose, per chain and per address', () => {
    const { store } = make();
    const read = store.issue(A, 84532, 'read');
    const replace = store.issue(A, 84532, 'replace');
    const otherChain = store.issue(A, 8453, 'read');
    const otherAddress = store.issue(B, 84532, 'read');
    expect(new Set([read.nonce, replace.nonce, otherChain.nonce, otherAddress.nonce]).size).toBe(4);
    expect(store.size()).toBe(4);
    expect(store.issue(A, 84532, 'replace').nonce).toBe(replace.nonce);
    expect(store.issue(A, 8453, 'read').nonce).toBe(otherChain.nonce);
  });

  it('peek returns the live challenge and does NOT consume it', () => {
    const { store } = make();
    const c = store.issue(A, 84532, 'read');
    for (let i = 0; i < 5; i++) expect(store.peek(c.nonce)).toEqual(c);
    expect(store.size()).toBe(1);
    expect(store.issue(A, 84532, 'read').nonce).toBe(c.nonce);
  });

  it('consume returns the challenge once, then the nonce is gone and the slot is free', () => {
    const { store } = make();
    const c = store.issue(A, 84532, 'read');
    expect(store.consume(c.nonce)).toEqual(c);
    expect(store.consume(c.nonce)).toBeNull();
    expect(store.peek(c.nonce)).toBeNull();
    expect(store.size()).toBe(0);
    expect(store.issue(A, 84532, 'read').nonce).not.toBe(c.nonce);
  });

  it('consuming one purpose leaves the other purpose live', () => {
    const { store } = make();
    const read = store.issue(A, 84532, 'read');
    const replace = store.issue(A, 84532, 'replace');
    store.consume(read.nonce);
    expect(store.peek(replace.nonce)).toEqual(replace);
  });

  it('expires a nonce at the end of its life, for peek and for consume, and issues a fresh one after', () => {
    const { store, clock } = make({ ttlMs: 1_000 });
    const c = store.issue(A, 84532, 'read');
    clock.t += 999;
    expect(store.peek(c.nonce)).not.toBeNull();
    clock.t += 1;
    expect(store.peek(c.nonce)).toBeNull();
    expect(store.consume(c.nonce)).toBeNull();
    expect(store.size()).toBe(0);
    expect(store.issue(A, 84532, 'read').nonce).not.toBe(c.nonce);
  });

  it('refuses new challenges past the global cap, but still returns a live one', () => {
    const { store, clock } = make({ max: 2 });
    const first = store.issue(A, 84532, 'read');
    store.issue(A, 84532, 'replace');
    expect(() => store.issue(B, 84532, 'read')).toThrow(ChallengeStoreFullError);
    expect(store.issue(A, 84532, 'read').nonce).toBe(first.nonce);
    clock.t += 300_000;
    expect(store.issue(B, 84532, 'read').nonce).toMatch(/^[0-9a-f]{48}$/);
  });

  it('returns null for an unknown, empty or non-string nonce', () => {
    const { store } = make();
    store.issue(A, 84532, 'read');
    for (const bad of ['', 'ab'.repeat(24), 'constructor', '__proto__', 12, null, undefined, {}]) {
      expect(store.peek(bad)).toBeNull();
      expect(store.consume(bad)).toBeNull();
    }
    expect(store.size()).toBe(1);
  });

  it('rejects a malformed address, an unknown purpose and a bad chain id', () => {
    const { store } = make();
    expect(() => store.issue('nope', 84532, 'read')).toThrow();
    expect(() => store.issue(A, 84532, 'delete' as never)).toThrow();
    expect(() => store.issue(A, 1.5, 'read')).toThrow();
    expect(store.size()).toBe(0);
  });

  it('never repeats a nonce across issues', () => {
    const { store } = make({ max: 1000 });
    const seen = new Set<string>();
    for (let i = 0; i < 200; i++) {
      const c = store.issue(A, 84532, 'read');
      seen.add(c.nonce);
      store.consume(c.nonce);
    }
    expect(seen.size).toBe(200);
  });
});

describe('ChallengeStore per client (R7 v0.2)', () => {
  const addr = (i: number) => '0x' + i.toString(16).padStart(40, '0');
  function makeFor(perClientMax: number, max = 1000) {
    const clock = { t: 1_800_000_000_000 };
    return { store: new ChallengeStore({ now: () => clock.t, max, perClientMax }), clock };
  }

  it('allows 20 live challenges per client by default and refuses the next', () => {
    const clock = { t: 1 };
    const store = new ChallengeStore({ now: () => clock.t, max: 1000 });
    for (let i = 1; i <= 20; i++) store.issue(addr(i), 84532, 'read', 'client-a');
    expect(() => store.issue(addr(21), 84532, 'read', 'client-a')).toThrow(ChallengeClientLimitError);
    expect(store.size()).toBe(20);
  });

  it('client A at its cap does not stop client B, even for an address A already asked for', () => {
    const { store } = makeFor(2);
    const a1 = store.issue(addr(1), 84532, 'read', 'client-a');
    store.issue(addr(2), 84532, 'read', 'client-a');
    expect(() => store.issue(addr(3), 84532, 'read', 'client-a')).toThrow(ChallengeClientLimitError);
    expect(store.issue(addr(3), 84532, 'read', 'client-b').nonce).toMatch(/^[0-9a-f]{48}$/);
    // The owner of address 1, on another client, is handed the live nonce and it costs them nothing.
    expect(store.issue(addr(1), 84532, 'read', 'client-b').nonce).toBe(a1.nonce);
    expect(store.issue(addr(4), 84532, 'read', 'client-b').nonce).toMatch(/^[0-9a-f]{48}$/);
    expect(() => store.issue(addr(5), 84532, 'read', 'client-b')).toThrow(ChallengeClientLimitError);
  });

  it('a client at its cap is still handed its own live challenges', () => {
    const { store } = makeFor(1);
    const c = store.issue(addr(1), 84532, 'read', 'client-a');
    expect(store.issue(addr(1), 84532, 'read', 'client-a').nonce).toBe(c.nonce);
  });

  it('frees a client slot when a challenge is consumed or expires', () => {
    const { store, clock } = makeFor(1);
    const c = store.issue(addr(1), 84532, 'read', 'client-a');
    expect(() => store.issue(addr(2), 84532, 'read', 'client-a')).toThrow(ChallengeClientLimitError);
    store.consume(c.nonce);
    store.issue(addr(2), 84532, 'read', 'client-a');
    expect(() => store.issue(addr(3), 84532, 'read', 'client-a')).toThrow(ChallengeClientLimitError);
    clock.t += 300_000;
    expect(store.issue(addr(3), 84532, 'read', 'client-a').nonce).toMatch(/^[0-9a-f]{48}$/);
  });

  it('the limit is on the client, never on the account address alone', () => {
    const { store } = makeFor(1);
    // Many clients asking for one address all get the one live nonce; nobody is refused.
    const first = store.issue(addr(1), 84532, 'read', 'client-0');
    for (let i = 1; i < 50; i++) expect(store.issue(addr(1), 84532, 'read', `client-${i}`).nonce).toBe(first.nonce);
  });

  it('does not expose the client key in the challenge', () => {
    const { store } = makeFor(5);
    const c = store.issue(addr(1), 84532, 'read', 'client-a');
    expect(Object.keys(c).sort()).toEqual(['address', 'chainId', 'expiresAt', 'nonce', 'purpose']);
    expect(Object.keys(store.peek(c.nonce)!).sort()).toEqual(['address', 'chainId', 'expiresAt', 'nonce', 'purpose']);
  });

  it('never does a full sweep on a call: lookups among 5000 live challenges cost a constant each', () => {
    const { store } = makeFor(100000, 5000);
    for (let i = 1; i <= 5000; i++) store.issue(addr(i), 84532, 'read', `c${i}`);
    const before = store.examined;
    for (let i = 0; i < 500; i++) store.peek('0'.repeat(48));
    for (let i = 0; i < 100; i++) expect(() => store.issue(addr(9000 + i), 84532, 'read', 'x')).toThrow(ChallengeStoreFullError);
    expect(store.examined - before).toBeLessThanOrEqual(600 * 2);
  });

  it('sweep() releases every expired challenge; the timer does not keep the process alive', async () => {
    const { store, clock } = makeFor(100);
    for (let i = 1; i <= 10; i++) store.issue(addr(i), 84532, 'read', 'c');
    clock.t += 300_000;
    expect(store.size()).toBe(10);
    expect(store.sweep()).toBe(10);
    expect(store.size()).toBe(0);
    store.issue(addr(1), 84532, 'read', 'c');
    clock.t += 300_000;
    const timer = store.startSweeping(5);
    expect(timer.hasRef()).toBe(false);
    await new Promise((r) => setTimeout(r, 40));
    clearInterval(timer);
    expect(store.size()).toBe(0);
  });
});
