// R6: the S3 implementation of the vault store (Garage on the portal box).
// Explicit region and path-style addressing, so the client never asks the
// endpoint for a bucket location and never builds a bucket host name (R11: no
// outbound connection other than the configured endpoint).
//
// v0.2 (R6a): reads and writes carry NO timeout of their own. A timeout here
// would let the caller move on while the write is still in flight, and that
// write could then land on top of a newer one. The timeout belongs to
// AccountLocks, which answers the caller but keeps the account locked until
// the call made here has really settled.
import { AsyncLocalStorage } from 'node:async_hooks';
import http from 'node:http';
import https from 'node:https';
import { Client } from 'minio';
import { objectKey, parseRecord, serialiseRecord, type Slot, type VaultRecord, type VaultStore } from './store.js';

export interface S3Settings {
  /** http(s)://host[:port] */
  endpoint: string;
  accessKey: string;
  secretKey: string;
  bucket: string;
  region: string;
}

// A record is the vault text (32 KiB by default) plus a few short fields.
// Refuse to buffer anything far beyond that.
const MAX_OBJECT_BYTES = 4 * 1024 * 1024;

/** What one storage call learned about the HTTP answer it got. The S3 client
 *  reports an error's code but not its HTTP status, so the status is noted
 *  here, per call, by the transport the client is given. */
interface CallNote {
  status: number | undefined;
}

const callNote = new AsyncLocalStorage<CallNote>();

/** R6b (v0.2): "no vault" ONLY when storage answered HTTP 404 with the parsed
 *  S3 error code `NoSuchKey`. Every other answer is a storage error: an empty
 *  404, an HTML 404 from something in front of the storage, `NoSuchBucket`,
 *  403, 5xx. An outage must never be mistaken for an empty account, because
 *  the UI would then walk the user into first-time set-up over a live vault. */
function isMissing(err: unknown, note: CallNote): boolean {
  return note.status === 404 && (err as { code?: unknown } | null)?.code === 'NoSuchKey';
}

function withTimeout<T>(work: Promise<T>, ms: number): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('S3 call timed out')), ms);
    work.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err) => {
        clearTimeout(timer);
        reject(err);
      }
    );
  });
}

export class S3VaultStore implements VaultStore {
  private readonly client: Client;
  private readonly bucket: string;

  constructor(settings: S3Settings) {
    let url: URL;
    try {
      url = new URL(settings.endpoint);
    } catch {
      throw new Error('S3 endpoint must be an http or https URL');
    }
    if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('S3 endpoint must be an http or https URL');
    if (!settings.accessKey || !settings.secretKey || !settings.bucket || !settings.region) {
      throw new Error('S3 access key, secret key, bucket and region are all required');
    }
    const useSSL = url.protocol === 'https:';
    const base = useSSL ? https : http;
    // The note is taken when the request is MADE (inside the call's own async
    // context) and filled in when the answer arrives, so calls running
    // together never read each other's status.
    const transport = {
      request: (options: http.RequestOptions, onResponse?: (res: http.IncomingMessage) => void): http.ClientRequest => {
        const note = callNote.getStore();
        return base.request(options, (res) => {
          if (note) note.status = res.statusCode;
          if (onResponse) onResponse(res);
        });
      },
    };
    this.client = new Client({
      endPoint: url.hostname,
      port: url.port ? Number(url.port) : useSSL ? 443 : 80,
      useSSL,
      accessKey: settings.accessKey,
      secretKey: settings.secretKey,
      region: settings.region,
      pathStyle: true,
      transport: transport as unknown as { request: typeof http.request },
    });
    this.bucket = settings.bucket;
  }

  private async fetchText(key: string): Promise<string> {
    const stream = await this.client.getObject(this.bucket, key);
    const chunks: Buffer[] = [];
    let size = 0;
    for await (const chunk of stream) {
      const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as Uint8Array);
      size += buf.length;
      if (size > MAX_OBJECT_BYTES) {
        stream.destroy();
        throw new Error('stored vault record is too large');
      }
      chunks.push(buf);
    }
    return Buffer.concat(chunks).toString('utf8');
  }

  private async read(chainId: number, address: string, slot: Slot): Promise<VaultRecord | null> {
    const key = objectKey(chainId, address, slot);
    let text: string;
    const note: CallNote = { status: undefined };
    try {
      text = await callNote.run(note, () => this.fetchText(key));
    } catch (err) {
      if (isMissing(err, note)) return null;
      throw err;
    }
    return parseRecord(text, chainId, address);
  }

  private async write(record: VaultRecord, slot: Slot): Promise<void> {
    const key = objectKey(record.chainId, record.address, slot);
    const body = Buffer.from(serialiseRecord(record), 'utf8');
    await this.client.putObject(this.bucket, key, body, body.length, { 'Content-Type': 'application/json' });
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

  async ping(): Promise<boolean> {
    try {
      return await withTimeout(this.client.bucketExists(this.bucket), 3_000);
    } catch {
      return false;
    }
  }
}
