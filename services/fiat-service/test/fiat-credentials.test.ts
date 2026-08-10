// FC1.2 — Decision 9 credentials: backend-issued per-user bearer tokens the
// helper presents to open fiat sessions. The journal stores only SHA-256
// hashes (a leaked journal must not leak spendable tokens); revocation is
// server-side and immediate.
import { describe, expect, it } from 'vitest';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { JsonlLedgerStore, MemoryLedgerStore } from '../src/lib/ledger';

describe('FiatCredentials', () => {
  it('issues a token that authenticates to the issuing user', async () => {
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await creds.issue('user-1');
    expect(creds.authenticate(token)).toBe('user-1');
  });

  it('rejects unknown, malformed, and empty tokens', async () => {
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    await creds.issue('user-1');
    expect(creds.authenticate('fc1_' + 'ab'.repeat(32))).toBeNull();
    expect(creds.authenticate('garbage')).toBeNull();
    expect(creds.authenticate('')).toBeNull();
  });

  it('issues unique high-entropy tokens', async () => {
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    const a = await creds.issue('user-1');
    const b = await creds.issue('user-1');
    expect(a).not.toBe(b);
    expect(a).toMatch(/^fc1_[0-9a-f]{64}$/);
  });

  it('revokeAll cuts off every credential of that user across purposes and reports the count', async () => {
    // FC2 Decision 8: two live credentials for one user means two DIFFERENT
    // purposes (a second same-purpose mint would evict the first — covered
    // below). revokeAll kills them all.
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    const a = await creds.issue('user-1', 'helper');
    const b = await creds.issue('user-1', 'browser');
    const other = await creds.issue('user-2');
    expect(await creds.revokeAll('user-1')).toBe(2);
    expect(creds.authenticate(a)).toBeNull();
    expect(creds.authenticate(b)).toBeNull();
    expect(creds.authenticate(other)).toBe('user-2');
  });

  // FC2 Decision 8 — keep-newest-per-purpose.
  it('a second mint of the SAME purpose revokes the first (keep-newest)', async () => {
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    const first = await creds.issue('user-1', 'helper');
    const second = await creds.issue('user-1', 'helper');
    expect(creds.authenticate(first)).toBeNull(); // evicted
    expect(creds.authenticate(second)).toBe('user-1'); // survives
  });

  it('a browser mint NEVER evicts the live helper credential (the rendering-break we must avoid)', async () => {
    const creds = await FiatCredentials.open(new MemoryLedgerStore());
    const helper = await creds.issue('user-1', 'helper'); // minted first (paired to Blender)
    const browser1 = await creds.issue('user-1', 'browser'); // cash-out re-mint
    const browser2 = await creds.issue('user-1', 'browser'); // another cash-out, later
    expect(creds.authenticate(helper)).toBe('user-1'); // helper still alive
    expect(creds.authenticate(browser1)).toBeNull(); // old browser evicted
    expect(creds.authenticate(browser2)).toBe('user-1'); // newest browser alive
  });

  it('keep-newest-per-purpose survives a restart', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'fc2-creds-'));
    const path = join(dir, 'credentials.jsonl');
    const creds = await FiatCredentials.open(new JsonlLedgerStore(path));
    await creds.issue('user-1', 'helper');
    const helperNew = await creds.issue('user-1', 'helper'); // evicts the first helper
    const browser = await creds.issue('user-1', 'browser');

    const reopened = await FiatCredentials.open(new JsonlLedgerStore(path));
    expect(reopened.authenticate(helperNew)).toBe('user-1');
    expect(reopened.authenticate(browser)).toBe('user-1');
  });

  it('never writes the raw token to the journal (hashes only)', async () => {
    const store = new MemoryLedgerStore();
    const creds = await FiatCredentials.open(store);
    const token = await creds.issue('user-1');
    const journal = (await store.load()).join('\n');
    expect(journal).not.toContain(token);
    expect(journal).not.toContain(token.replace(/^fc1_/, ''));
  });

  it('survives a restart: issued tokens still authenticate, revoked stay revoked', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'fc1-creds-'));
    const path = join(dir, 'credentials.jsonl');
    const creds = await FiatCredentials.open(new JsonlLedgerStore(path));
    const keep = await creds.issue('user-1');
    const gone = await creds.issue('user-2');
    await creds.revokeAll('user-2');

    const reopened = await FiatCredentials.open(new JsonlLedgerStore(path));
    expect(reopened.authenticate(keep)).toBe('user-1');
    expect(reopened.authenticate(gone)).toBeNull();
  });
});
