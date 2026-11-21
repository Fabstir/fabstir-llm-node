// R6: storage. One record per (chainId, address):
//
//   v1/<chainId>/<address>/current.json
//   v1/<chainId>/<address>/previous.json
//
// This module holds the interface, the record codec, the object keys, the
// in-memory implementation used by tests, and the per-account serialiser. The
// S3 implementation is in store-s3.ts, so nothing here loads an S3 client.

export interface VaultRecord {
  v: 1;
  /** 0x + 40 lower-case hex. */
  address: string;
  chainId: number;
  /** 1 for the first vault, then one more with every replace. */
  version: number;
  /** Read tokens carry the epoch they were issued under; every replace increases it. */
  epoch: number;
  /** ISO 8601 UTC. */
  updatedAt: string;
  /** The wrapped vault: opaque text, never inspected, never logged. */
  vault: string;
}

export interface VaultStore {
  readCurrent(chainId: number, address: string): Promise<VaultRecord | null>;
  readPrevious(chainId: number, address: string): Promise<VaultRecord | null>;
  writeCurrent(record: VaultRecord): Promise<void>;
  writePrevious(record: VaultRecord): Promise<void>;
  /** True when the storage answers. Never throws. */
  ping(): Promise<boolean>;
}

const ADDRESS_LC_RE = /^0x[0-9a-f]{40}$/;

/** A stored object exists but is not a record of this account. */
export class CorruptRecordError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CorruptRecordError';
  }
}

/** The caller stopped waiting for the storage. The work may still be running:
 *  see AccountLocks. */
export class StorageTimeoutError extends Error {
  constructor() {
    super('storage call timed out');
    this.name = 'StorageTimeoutError';
  }
}

/** An earlier storage call for this account timed out and has not settled
 *  yet, so nothing more is accepted for the account until it does. */
export class StorageUnsettledError extends Error {
  constructor() {
    super('an earlier storage call for this account has not settled');
    this.name = 'StorageUnsettledError';
  }
}

const KIND_CODE_RE = /^[A-Za-z][A-Za-z0-9_]{0,39}$/;

/** R9 v0.2: the kind of a storage failure, for the log: `timeout`,
 *  `unsettled`, `corrupt`, or the S3 (or socket) error CODE. Never the
 *  message, which could carry request data. */
export function storageFailureKind(err: unknown): string {
  if (err instanceof StorageTimeoutError) return 'timeout';
  if (err instanceof StorageUnsettledError) return 'unsettled';
  if (err instanceof CorruptRecordError) return 'corrupt';
  const code = (err as { code?: unknown } | null)?.code;
  return typeof code === 'string' && KIND_CODE_RE.test(code) ? code : 'error';
}

export type Slot = 'current' | 'previous';

/** The object key for a record. Built only from a checked chain id and a
 *  checked lower-case address, so no caller text can steer the path. */
export function objectKey(chainId: number, address: string, slot: Slot): string {
  if (!Number.isSafeInteger(chainId) || chainId <= 0) throw new Error('chain id must be a positive integer');
  if (typeof address !== 'string' || !ADDRESS_LC_RE.test(address)) throw new Error('address must be 0x and 40 lower-case hex');
  if (slot !== 'current' && slot !== 'previous') throw new Error('unknown record slot');
  return `v1/${chainId}/${address}/${slot}.json`;
}

function isRecordShape(r: unknown): r is VaultRecord {
  if (typeof r !== 'object' || r === null) return false;
  const o = r as Record<string, unknown>;
  return (
    o.v === 1 &&
    typeof o.address === 'string' &&
    ADDRESS_LC_RE.test(o.address) &&
    typeof o.chainId === 'number' &&
    Number.isSafeInteger(o.chainId) &&
    o.chainId > 0 &&
    typeof o.version === 'number' &&
    Number.isSafeInteger(o.version) &&
    o.version >= 1 &&
    typeof o.epoch === 'number' &&
    Number.isSafeInteger(o.epoch) &&
    o.epoch >= 1 &&
    typeof o.updatedAt === 'string' &&
    typeof o.vault === 'string'
  );
}

/** Exactly the documented fields, in the documented order. */
export function serialiseRecord(record: VaultRecord): string {
  if (!isRecordShape(record)) throw new Error('vault record is malformed');
  return JSON.stringify({
    v: 1,
    address: record.address,
    chainId: record.chainId,
    version: record.version,
    epoch: record.epoch,
    updatedAt: record.updatedAt,
    vault: record.vault,
  });
}

/** Parse a stored record and check it belongs where it was found. Errors never
 *  carry the stored text. */
export function parseRecord(text: string, chainId: number, address: string): VaultRecord {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new CorruptRecordError('stored vault record is not JSON');
  }
  if (!isRecordShape(parsed)) throw new CorruptRecordError('stored vault record is malformed');
  if (parsed.address !== address || parsed.chainId !== chainId) {
    throw new CorruptRecordError('stored vault record belongs to another account');
  }
  return {
    v: 1,
    address: parsed.address,
    chainId: parsed.chainId,
    version: parsed.version,
    epoch: parsed.epoch,
    updatedAt: parsed.updatedAt,
    vault: parsed.vault,
  };
}

/** In-memory store for tests. Goes through the same keys and codec as S3. */
export class MemoryVaultStore implements VaultStore {
  private readonly objects = new Map<string, string>();
  /** Keys written, in order (tests assert "nothing written"). */
  readonly writes: string[] = [];
  /** When true every call fails, as an unreachable S3 would. */
  failing = false;
  /** Test hooks, awaited before each read or write of a key: a test can hold
   *  a call back (a slow S3) or make one fail by throwing. */
  onRead: ((key: string) => void | Promise<void>) | undefined;
  onWrite: ((key: string) => void | Promise<void>) | undefined;

  private guard(): void {
    if (this.failing) throw new Error('storage unavailable (test)');
  }

  private async read(chainId: number, address: string, slot: Slot): Promise<VaultRecord | null> {
    this.guard();
    if (this.onRead) await this.onRead(objectKey(chainId, address, slot));
    const text = this.objects.get(objectKey(chainId, address, slot));
    return text === undefined ? null : parseRecord(text, chainId, address);
  }

  private async write(record: VaultRecord, slot: Slot): Promise<void> {
    this.guard();
    const key = objectKey(record.chainId, record.address, slot);
    if (this.onWrite) await this.onWrite(key);
    this.objects.set(key, serialiseRecord(record));
    this.writes.push(key);
  }

  readCurrent(chainId: number, address: string): Promise<VaultRecord | null> {
    return this.read(chainId, address, 'current');
  }

  readPrevious(chainId: number, address: string): Promise<VaultRecord | null> {
    return this.read(chainId, address, 'previous');
  }

  writeCurrent(record: VaultRecord): Promise<void> {
    return this.write(record, 'current');
  }

  writePrevious(record: VaultRecord): Promise<void> {
    return this.write(record, 'previous');
  }

  /** How often the storage was pinged. */
  pings = 0;

  async ping(): Promise<boolean> {
    this.pings += 1;
    return !this.failing;
  }

  /** Test helper: plant raw stored text, such as a damaged object. */
  putRaw(key: string, text: string): void {
    this.objects.set(key, text);
  }

  /** Test helper: what an operator removing a record on request does. */
  remove(chainId: number, address: string, slot?: Slot): void {
    if (slot !== 'previous') this.objects.delete(objectKey(chainId, address, 'current'));
    if (slot !== 'current') this.objects.delete(objectKey(chainId, address, 'previous'));
  }
}

/** What the work is told about its caller. */
export interface LockContext {
  /** True once the caller has been answered with a timeout. Work that has not
   *  yet written should then stop instead of writing for nobody. */
  readonly abandoned: boolean;
}

/** Serialises work per key within this process (R6: reads and writes for one
 *  account are serialised; the service runs as a single instance).
 *
 *  v0.2 (R6a). The CALLER is answered when the timeout passes, but the
 *  lock is held until the work has really settled, and until then the key is
 *  "unsettled": every further call for it is refused at once. A storage write
 *  that outlives its timeout can therefore never land on top of a newer,
 *  acknowledged one, because no newer one is accepted while it is in flight.
 *  Work that times out while still queued is never started. */
export class AccountLocks {
  private readonly tails = new Map<string, Promise<unknown>>();
  private readonly unsettled = new Set<string>();

  isUnsettled(key: string): boolean {
    return this.unsettled.has(key);
  }

  run<T>(key: string, fn: (ctx: LockContext) => Promise<T>, timeoutMs: number): Promise<T> {
    // The timeout is REQUIRED (round 2). There is no silent "no timeout": a
    // caller that forgot it would wait on a hung storage call for ever.
    if (typeof timeoutMs !== 'number' || !Number.isFinite(timeoutMs) || timeoutMs <= 0) {
      return Promise.reject(new TypeError('AccountLocks.run needs a storage timeout in milliseconds (a positive number)'));
    }
    if (this.unsettled.has(key)) return Promise.reject(new StorageUnsettledError());

    const before = this.tails.get(key) ?? Promise.resolve();
    let state: 'queued' | 'running' | 'settled' = 'queued';
    let abandoned = false;
    const ctx: LockContext = {
      get abandoned() {
        return abandoned;
      },
    };

    const result = new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        abandoned = true;
        if (state === 'running') this.unsettled.add(key);
        reject(new StorageTimeoutError());
      }, timeoutMs);
      const work = before.then(async () => {
        if (abandoned) return; // timed out while queued: never started
        state = 'running';
        try {
          const value = await fn(ctx);
          resolve(value); // ignored if the caller was already answered
        } catch (err) {
          reject(err);
        } finally {
          state = 'settled';
          clearTimeout(timer);
          this.unsettled.delete(key);
        }
      });
      // The tail is the WORK, not the answer to the caller: the next in line
      // waits for the storage call itself to settle.
      const tail = work.then(
        () => undefined,
        () => undefined
      );
      this.tails.set(key, tail);
      void tail.then(() => {
        if (this.tails.get(key) === tail) this.tails.delete(key);
      });
    });
    return result;
  }

  size(): number {
    return this.tails.size;
  }
}
