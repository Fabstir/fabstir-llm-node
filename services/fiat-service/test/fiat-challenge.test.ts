// FC2.2 — the ChallengeStore in isolation (the routes exercise it end-to-end;
// this pins its Decision-9 bounds directly).
import { describe, expect, it } from 'vitest';
import { ChallengeStore } from '../src/lib/fiat-challenge';

const A = '0xAbCdef0123456789abcDEF0123456789aBCdEf01';
const A_LC = A.toLowerCase();
const B = '0x1111111111111111111111111111111111111111';

describe('ChallengeStore', () => {
  it('issues idempotently per address (returns the live nonce, never evicts) and bounds size', () => {
    const s = new ChallengeStore();
    const c1 = s.issue(A);
    const c2 = s.issue(A);
    expect(c2.nonce).toBe(c1.nonce); // same live nonce handed back
    expect(s.size()).toBe(1);
    s.issue(B);
    expect(s.size()).toBe(2);
  });

  it('consume burns the nonce (single-use) and frees the address slot', () => {
    const s = new ChallengeStore();
    const c = s.issue(A);
    expect(s.consume(c.nonce)?.address).toBe(A_LC);
    expect(s.consume(c.nonce)).toBeNull(); // already consumed
    expect(s.size()).toBe(0);
    const c2 = s.issue(A); // slot freed → a fresh nonce
    expect(c2.nonce).not.toBe(c.nonce);
  });

  it('expires and sweeps a nonce after the TTL', () => {
    let clock = 1_000;
    const s = new ChallengeStore(() => clock, 1_000, 100); // 1s TTL
    const c = s.issue(A);
    clock += 2_000;
    expect(s.consume(c.nonce)).toBeNull();
    expect(s.size()).toBe(0);
  });

  it('throws past the global cap', () => {
    const s = new ChallengeStore(Date.now, 5 * 60_000, 1); // cap = 1
    s.issue(A);
    expect(() => s.issue(B)).toThrow(/too many/);
  });

  it('rejects a malformed address', () => {
    expect(() => new ChallengeStore().issue('nope')).toThrow();
  });
});
