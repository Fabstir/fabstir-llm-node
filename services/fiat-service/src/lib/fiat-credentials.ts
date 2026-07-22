// FC1.2 / FC2 — backend-issued per-user credentials the helper (or the browser)
// presents to spend a user's fiat balance. The journal stores SHA-256 hashes
// only (a leaked journal must not leak spendable tokens); a stolen token exposes
// one user's capped ledger balance and dies on revocation, never the vault.
//
// FC2 Decision 8 — keep-newest-PER-PURPOSE. Self-serve minting re-mints on
// demand (page reloads, each cash-out), and every live token is a spending
// secret, so accumulation must be bounded. A naive "cap N, revoke oldest" would
// be WRONG: the helper's credential is minted FIRST (at "enable card-paid
// rendering") and browser cash-out re-mints AFTER it, so oldest-first eviction
// would silently revoke the helper's credential and break Blender rendering.
// Instead every mint carries a `purpose` ('helper' | 'browser') and we keep only
// the NEWEST live credential per (user, purpose), revoking any prior of the SAME
// purpose. Result: at most two live per user; browser activity can NEVER evict
// the helper's credential. `purpose` defaults to 'helper' (the operator/helper
// path), and legacy journal entries with no purpose replay as 'helper' — so FC1
// data and tests are unaffected.
import { createHash, randomBytes } from 'node:crypto';
import type { LedgerStore } from './ledger';

export type CredentialPurpose = 'helper' | 'browser';

type CredEntry =
  | { t: 'cred-issue'; userId: string; purpose?: CredentialPurpose; tokenHash: string; at: number }
  | { t: 'cred-revoke'; tokenHash: string; at: number };

function hashToken(token: string): string {
  return createHash('sha256').update(token).digest('hex');
}

export class FiatCredentials {
  // tokenHash -> which user + purpose it belongs to (authenticate + eviction).
  private byHash = new Map<string, { userId: string; purpose: CredentialPurpose }>();
  // `${userId}\x00${purpose}` -> the single live tokenHash for that pair.
  private liveByUserPurpose = new Map<string, string>();
  private queue: Promise<unknown> = Promise.resolve();

  private constructor(
    private readonly store: LedgerStore,
    private readonly now: () => number
  ) {}

  static async open(store: LedgerStore, opts?: { now?: () => number }): Promise<FiatCredentials> {
    const creds = new FiatCredentials(store, opts?.now ?? Date.now);
    for (const line of await store.load()) creds.apply(JSON.parse(line) as CredEntry);
    return creds;
  }

  private key(userId: string, purpose: CredentialPurpose): string {
    return `${userId}\x00${purpose}`;
  }

  private apply(entry: CredEntry): void {
    if (entry.t === 'cred-issue') {
      const purpose = entry.purpose ?? 'helper'; // legacy entries → helper
      this.byHash.set(entry.tokenHash, { userId: entry.userId, purpose });
      this.liveByUserPurpose.set(this.key(entry.userId, purpose), entry.tokenHash);
    } else {
      const meta = this.byHash.get(entry.tokenHash);
      this.byHash.delete(entry.tokenHash);
      if (meta) {
        const k = this.key(meta.userId, meta.purpose);
        if (this.liveByUserPurpose.get(k) === entry.tokenHash) this.liveByUserPurpose.delete(k);
      }
    }
  }

  private run<T>(op: () => Promise<T>): Promise<T> {
    const result = this.queue.then(op);
    this.queue = result.catch(() => undefined);
    return result;
  }

  private async commit(entry: CredEntry): Promise<void> {
    await this.store.append(JSON.stringify(entry));
    this.apply(entry);
  }

  /** Mint a credential for `userId` under `purpose` (default 'helper'). Revokes
   *  the prior live credential of the SAME (user, purpose) first, so at most one
   *  per purpose survives. The raw token is returned ONCE and never stored. */
  issue(userId: string, purpose: CredentialPurpose = 'helper'): Promise<string> {
    return this.run(async () => {
      const prior = this.liveByUserPurpose.get(this.key(userId, purpose));
      if (prior) await this.commit({ t: 'cred-revoke', tokenHash: prior, at: this.now() });
      const token = `fc1_${randomBytes(32).toString('hex')}`;
      await this.commit({ t: 'cred-issue', userId, purpose, tokenHash: hashToken(token), at: this.now() });
      return token;
    });
  }

  /** Constant-shape check: hash the presented token, look the hash up. */
  authenticate(token: string): string | null {
    if (typeof token !== 'string' || token.length === 0) return null;
    return this.byHash.get(hashToken(token))?.userId ?? null;
  }

  /** Server-side kill switch: revoke every credential of one user, all purposes. */
  revokeAll(userId: string): Promise<number> {
    return this.run(async () => {
      const hashes = [...this.byHash.entries()].filter(([, m]) => m.userId === userId).map(([h]) => h);
      for (const tokenHash of hashes) {
        await this.commit({ t: 'cred-revoke', tokenHash, at: this.now() });
      }
      return hashes.length;
    });
  }
}
