// Idempotent session opens (FC2.8). A browser cannot distinguish "the request
// never landed" from "it landed, escrowed, and the reply was lost": a page
// reload destroys whatever it was holding, and a duplicated tab starts life
// with a copy of the pending state and none of the in-memory guards. Only the
// server knows whether money moved, so only the server can answer honestly.
//
// The contract: the caller supplies a key, and (userId, key) maps to at most
// ONE escrow, for ever. A completed key REPLAYS its original session; a key
// still in flight is REFUSED rather than guessed at (the chain call may be
// mid-air); a key reused with different parameters is an error, because that is
// a client bug and silently replaying the wrong session would hide it.
//
// The journal is the record, not memory: a crash between reserve and complete
// must leave the key PENDING, since the escrow may exist. Fail closed on money.
import { createHash } from 'node:crypto';
import type { SessionKind } from './gatekeeper';
import type { LedgerStore } from './ledger';

export interface FingerprintInput {
  host: string;
  modelId: string;
  depositMicro: bigint;
  clientAddress: string;
  /** Absent = standard. A key first used for a training open and replayed
   *  as a standard one (or vice versa) is a DIFFERENT session: key_conflict. */
  kind?: SessionKind;
}

/** A stable digest of the parameters a key was first used with, so a key reused
 *  for a DIFFERENT session is caught instead of replaying the wrong one. */
export function requestFingerprint(r: FingerprintInput): string {
  return createHash('sha256')
    .update(
      [
        r.host.toLowerCase(),
        r.modelId.toLowerCase(),
        r.depositMicro.toString(),
        r.clientAddress.toLowerCase(),
        // Appended ONLY for a non-standard kind, so every fingerprint journaled
        // before `kind` existed still matches its own replay byte for byte.
        ...(r.kind && r.kind !== 'standard' ? [r.kind] : []),
      ].join('|')
    )
    .digest('hex')
    .slice(0, 32);
}

export type IdempotencyRecord =
  | { state: 'pending'; fingerprint: string }
  | { state: 'done'; jobId: bigint; clientAddress: string; fingerprint: string };

type Entry =
  | { t: 'reserve'; userId: string; key: string; fp: string; at: number }
  | { t: 'complete'; userId: string; key: string; jobId: string; clientAddress: string; at: number }
  | { t: 'release'; userId: string; key: string; at: number };

interface Row {
  fingerprint: string;
  at: number;
  jobId?: bigint;
  clientAddress?: string;
}

const DEFAULT_RETENTION_MS = 24 * 60 * 60 * 1000;

export class IdempotencyStore {
  private rows = new Map<string, Row>();

  private constructor(
    private readonly store: LedgerStore,
    private readonly now: () => number,
    private readonly retentionMs: number
  ) {}

  static async open(
    store: LedgerStore,
    opts: { now?: () => number; retentionMs?: number } = {}
  ): Promise<IdempotencyStore> {
    const s = new IdempotencyStore(store, opts.now ?? Date.now, opts.retentionMs ?? DEFAULT_RETENTION_MS);
    for (const line of await store.load()) {
      let e: Entry;
      try {
        e = JSON.parse(line) as Entry;
      } catch {
        continue; // a torn final line from a crash mid-append: skip, never throw
      }
      s.apply(e);
    }
    return s;
  }

  private static id(userId: string, key: string): string {
    return `${userId.toLowerCase()} ${key}`;
  }

  private apply(e: Entry): void {
    const id = IdempotencyStore.id(e.userId, e.key);
    if (e.t === 'reserve') {
      this.rows.set(id, { fingerprint: e.fp, at: e.at });
    } else if (e.t === 'complete') {
      const row = this.rows.get(id);
      if (row) {
        row.jobId = BigInt(e.jobId);
        row.clientAddress = e.clientAddress;
        row.at = e.at;
      }
    } else {
      this.rows.delete(id);
    }
  }

  private async write(e: Entry): Promise<void> {
    this.apply(e);
    await this.store.append(JSON.stringify(e));
  }

  private prune(): void {
    const cutoff = this.now() - this.retentionMs;
    for (const [id, row] of this.rows) if (row.at <= cutoff) this.rows.delete(id);
  }

  /** The prior attempt for this key, or null if there is none (or it aged out). */
  async lookup(userId: string, key: string): Promise<IdempotencyRecord | null> {
    this.prune();
    const row = this.rows.get(IdempotencyStore.id(userId, key));
    if (!row) return null;
    if (row.jobId === undefined) return { state: 'pending', fingerprint: row.fingerprint };
    return {
      state: 'done',
      jobId: row.jobId,
      clientAddress: row.clientAddress!,
      fingerprint: row.fingerprint,
    };
  }

  /** Claim the key BEFORE the chain call, so a crash leaves it pending. */
  async reserve(userId: string, key: string, fingerprint: string): Promise<void> {
    await this.write({ t: 'reserve', userId, key, fp: fingerprint, at: this.now() });
  }

  /** Bind the key to the session that was actually created. */
  async complete(userId: string, key: string, jobId: bigint, clientAddress: string): Promise<void> {
    await this.write({
      t: 'complete',
      userId,
      key,
      jobId: jobId.toString(),
      clientAddress,
      at: this.now(),
    });
  }

  /** Free the key after a failure that provably moved no money, so an honest
   *  retry is not blocked by our own bookkeeping. */
  async release(userId: string, key: string): Promise<void> {
    await this.write({ t: 'release', userId, key, at: this.now() });
  }
}
