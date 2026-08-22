// Idempotent session opens. Without a key, a retried POST /session opens a
// SECOND paid session: the browser cannot tell "the request never landed" from
// "it landed and the reply was lost", and a page reload or a duplicated tab
// destroys whatever it was holding in memory. The server is the only place that
// can answer honestly, because only the server knows whether money moved.
//
// The rule: same (user, key) -> the ORIGINAL session, replayed. Never a second
// escrow. A key still in flight is refused rather than guessed at, and a key
// reused with different parameters is an error, not a silent replay.
import { describe, expect, it } from 'vitest';
import { IdempotencyStore, requestFingerprint } from '../src/lib/idempotency';
import type { LedgerStore } from '../src/lib/ledger';

function memoryStore(): LedgerStore & { lines: string[] } {
  const lines: string[] = [];
  return { lines, load: async () => [...lines], append: async (l: string) => void lines.push(l) };
}

const USER = '0xabc0000000000000000000000000000000000001';
const OTHER = '0xabc0000000000000000000000000000000000002';
const REQ = { host: '0xhost', modelId: '0xmodel', depositMicro: 500_000n, clientAddress: '0xclient' };

describe('IdempotencyStore', () => {
  it('a fresh key reserves and reports no prior attempt', async () => {
    const store = await IdempotencyStore.open(memoryStore());
    expect(await store.lookup(USER, 'k1')).toBeNull();
    await store.reserve(USER, 'k1', requestFingerprint(REQ));
    const found = await store.lookup(USER, 'k1');
    expect(found).toEqual({ state: 'pending', fingerprint: requestFingerprint(REQ) });
  });

  it('replays the original jobId after completion — never a second escrow', async () => {
    const store = await IdempotencyStore.open(memoryStore());
    await store.reserve(USER, 'k1', requestFingerprint(REQ));
    await store.complete(USER, 'k1', 990n, '0xclient');
    expect(await store.lookup(USER, 'k1')).toEqual({
      state: 'done',
      jobId: 990n,
      clientAddress: '0xclient',
      fingerprint: requestFingerprint(REQ),
    });
  });

  it('scopes keys per user: the same key from another account is untouched', async () => {
    const store = await IdempotencyStore.open(memoryStore());
    await store.reserve(USER, 'k1', requestFingerprint(REQ));
    await store.complete(USER, 'k1', 990n, '0xclient');
    expect(await store.lookup(OTHER, 'k1')).toBeNull();
  });

  it('survives a restart: the journal is the record, not memory', async () => {
    const backing = memoryStore();
    const first = await IdempotencyStore.open(backing);
    await first.reserve(USER, 'k1', requestFingerprint(REQ));
    await first.complete(USER, 'k1', 990n, '0xclient');
    const reloaded = await IdempotencyStore.open(backing);
    const record = await reloaded.lookup(USER, 'k1');
    expect(record?.state).toBe('done');
    expect(record?.state === 'done' && record.jobId).toBe(990n);
  });

  it('a crash between reserve and complete leaves the key PENDING, not free', async () => {
    // The dangerous state: the chain call may have escrowed. A retry must be
    // refused, not silently re-run, or the user pays twice.
    const backing = memoryStore();
    const first = await IdempotencyStore.open(backing);
    await first.reserve(USER, 'k1', requestFingerprint(REQ));
    const reloaded = await IdempotencyStore.open(backing);
    expect((await reloaded.lookup(USER, 'k1'))?.state).toBe('pending');
  });

  it('releases a key when the attempt failed before any money moved', async () => {
    const store = await IdempotencyStore.open(memoryStore());
    await store.reserve(USER, 'k1', requestFingerprint(REQ));
    await store.release(USER, 'k1');
    expect(await store.lookup(USER, 'k1')).toBeNull(); // safe to retry
  });

  it('prunes records past the retention window, keeping recent ones', async () => {
    let now = 1_000_000;
    const store = await IdempotencyStore.open(memoryStore(), { now: () => now, retentionMs: 1000 });
    await store.reserve(USER, 'old', requestFingerprint(REQ));
    await store.complete(USER, 'old', 1n, '0xclient');
    now += 5000;
    await store.reserve(USER, 'new', requestFingerprint(REQ));
    await store.complete(USER, 'new', 2n, '0xclient');
    expect(await store.lookup(USER, 'old')).toBeNull();
    const kept = await store.lookup(USER, 'new');
    expect(kept?.state === 'done' && kept.jobId).toBe(2n);
  });
});

describe('requestFingerprint', () => {
  it('is stable for the same parameters and differs for any change', () => {
    const base = requestFingerprint(REQ);
    expect(requestFingerprint({ ...REQ })).toBe(base);
    expect(requestFingerprint({ ...REQ, depositMicro: 600_000n })).not.toBe(base);
    expect(requestFingerprint({ ...REQ, host: '0xother' })).not.toBe(base);
    expect(requestFingerprint({ ...REQ, modelId: '0xother' })).not.toBe(base);
    expect(requestFingerprint({ ...REQ, clientAddress: '0xother' })).not.toBe(base);
  });
});
