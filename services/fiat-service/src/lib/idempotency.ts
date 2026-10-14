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
  /** `ca` (D20 item 2): the client address, journaled at reserve so R5 can
   *  complete the key from the chain alone. Absent on every pre-deploy line. */
  | { t: 'reserve'; userId: string; key: string; fp: string; at: number; ca?: string }
  | { t: 'complete'; userId: string; key: string; jobId: string; clientAddress: string; at: number }
  | { t: 'release'; userId: string; key: string; at: number };

interface Row {
  userId: string;
  key: string;
  fingerprint: string;
  at: number;
  jobId?: bigint;
  clientAddress?: string;
}

/** A key still `pending` (reserved, never completed or freed) — R5's work-list. */
export interface PendingKey {
  userId: string;
  key: string;
  fingerprint: string;
  atMs: number;
  /** Absent = reserved before the deploy (no `ca`): alarm, never freed. */
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
    opts: { now?: () => number; retentionMs?: number; onAlarm?: (message: string) => void } = {}
  ): Promise<IdempotencyStore> {
    const s = new IdempotencyStore(store, opts.now ?? Date.now, opts.retentionMs ?? DEFAULT_RETENTION_MS);
    const alarm = opts.onAlarm ?? ((m: string) => console.error(`[fiat-idempotency] ALARM: ${m}`));
    const lines = await store.load();
    for (let i = 0; i < lines.length; i++) {
      let e: Entry;
      try {
        e = JSON.parse(lines[i]!) as Entry;
      } catch {
        // A torn FINAL line (a crash mid-append, never acknowledged) is
        // skipped silently. A corrupt line anywhere else may have been a
        // reserve row (a key that stands for a live session): alarm, and keep
        // loading the rest so the service still boots.
        if (i < lines.length - 1) {
          alarm(
            `idempotency journal line ${i + 1} of ${lines.length} is corrupt and was skipped — it may have been a reserve ` +
              `(a key standing for a live session); inspect it and repair the journal by hand: ${lines[i]!.slice(0, 120)}`
          );
        }
        continue;
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
      this.rows.set(id, {
        userId: e.userId,
        key: e.key,
        fingerprint: e.fp,
        at: e.at,
        ...(e.ca !== undefined ? { clientAddress: e.ca } : {}),
      });
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

  /** D20 item 4 — only DONE keys age out. A pending key may stand for a
   *  session whose create is still unresolved; pruning it would let a retry
   *  with the same key open a second session. R5 ends every pending key. */
  private prune(): void {
    const cutoff = this.now() - this.retentionMs;
    for (const [id, row] of this.rows) {
      if (row.jobId !== undefined && row.at <= cutoff) this.rows.delete(id);
    }
  }

  /** The prior attempt for this key, or null if there is none (or it aged out). */
  async lookup(userId: string, key: string): Promise<IdempotencyRecord | null> {
    return this.peek(userId, key);
  }

  /** Claim the key BEFORE the chain call, so a crash leaves it pending. The
   *  client address rides along (D20 item 2) so R5 can complete the key. */
  async reserve(userId: string, key: string, fingerprint: string, clientAddress?: string): Promise<void> {
    await this.write({
      t: 'reserve',
      userId,
      key,
      fp: fingerprint,
      at: this.now(),
      ...(clientAddress !== undefined ? { ca: clientAddress } : {}),
    });
  }

  /** D20 — the ATOMIC claim: the check and the in-memory reserve run in one
   *  synchronous step (before the journal write's first await), so two
   *  same-key requests that both passed an earlier `lookup` (e.g. across the
   *  boot nonce wait's RPC reads) can never both reserve. Answers the prior
   *  record when the key is already taken, or null when this call claimed it. */
  async claim(userId: string, key: string, fingerprint: string, clientAddress?: string): Promise<IdempotencyRecord | null> {
    const prior = this.peek(userId, key);
    if (prior) return prior;
    const id = IdempotencyStore.id(userId, key);
    const writing = this.reserve(userId, key, fingerprint, clientAddress); // applies in memory synchronously
    const mine = this.rows.get(id);
    try {
      await writing;
    } catch (e) {
      // The journal write failed, so nothing was claimed: drop the in-memory
      // row too, or the key would answer `in_flight` until a restart. (If the
      // line did reach the disk, R5 frees a reserved key with no hold after
      // 10 min — no hold is ever opened under a failed claim.)
      if (this.rows.get(id) === mine) this.rows.delete(id);
      throw e;
    }
    return null;
  }

  private peek(userId: string, key: string): IdempotencyRecord | null {
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

  /** Every key still pending, for R5. */
  pendingKeys(): PendingKey[] {
    const out: PendingKey[] = [];
    for (const row of this.rows.values()) {
      if (row.jobId !== undefined) continue;
      out.push({
        userId: row.userId,
        key: row.key,
        fingerprint: row.fingerprint,
        atMs: row.at,
        ...(row.clientAddress !== undefined ? { clientAddress: row.clientAddress } : {}),
      });
    }
    return out;
  }

  /** R5's idempotent completion: only a still-pending key is completed. */
  async completeIfPending(userId: string, key: string, jobId: bigint, clientAddress: string): Promise<boolean> {
    const row = this.rows.get(IdempotencyStore.id(userId, key));
    if (!row || row.jobId !== undefined) return false;
    await this.complete(userId, key, jobId, clientAddress);
    return true;
  }

  /** R5's idempotent free: only a still-pending key is freed (never a done one). */
  async releaseIfPending(userId: string, key: string): Promise<boolean> {
    const row = this.rows.get(IdempotencyStore.id(userId, key));
    if (!row || row.jobId !== undefined) return false;
    await this.release(userId, key);
    return true;
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
