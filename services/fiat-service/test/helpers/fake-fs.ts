// An in-memory node:fs/promises for the power-cut tests (D20, BM1.2c). It
// records every write and every FileHandle.sync(), and a simulated power cut
// drops every byte written to a file since that file's last sync — so the REAL
// JsonlLedgerStore and outbox journal run over it, and an fsync the code skips
// is a line the "disk" loses.
type FileState = { data: string; synced: number };

const files = new Map<string, FileState>();
export const fsLog: string[] = [];

function enoent(path: string): Error {
  const e = new Error(`ENOENT: no such file or directory, open '${path}'`) as NodeJS.ErrnoException;
  e.code = 'ENOENT';
  return e;
}

function fileFor(path: string): FileState {
  let f = files.get(path);
  if (!f) {
    f = { data: '', synced: 0 };
    files.set(path, f);
  }
  return f;
}

export const fakeDisk = {
  /** The next handle append writes this fragment (a partial line) and throws
   *  EIO, as a full disk or an I/O error mid-write would. */
  failNextAppend: undefined as string | undefined,
  /** Each handle append writes its data in TWO chunks with `beforeOp('append-rest')`
   *  between them (a write still in progress). */
  chunkedAppends: false,
  /** Awaited before 'truncate' and between chunked append halves: a test gate. */
  beforeOp: undefined as ((op: 'truncate' | 'append-rest', path: string) => void | Promise<void>) | undefined,
  reset(): void {
    files.clear();
    fsLog.length = 0;
    this.failNextAppend = undefined;
    this.chunkedAppends = false;
    this.beforeOp = undefined;
  },
  /** The machine loses power: every unsynced byte is gone. */
  powerCut(): void {
    for (const f of files.values()) f.data = f.data.slice(0, f.synced);
    fsLog.push('POWER CUT');
  },
  /** A power cut in the middle of an append: `fragment` (a partial line with
   *  no trailing newline) DID reach the platter, the rest of the line did not. */
  tearTail(path: string, fragment: string): void {
    const f = fileFor(path);
    f.data = f.data.slice(0, f.synced) + fragment;
    f.synced = f.data.length;
    fsLog.push(`TORN ${path}`);
  },
  read(path: string): string {
    return files.get(path)?.data ?? '';
  },
  syncedText(path: string): string {
    const f = files.get(path);
    return f ? f.data.slice(0, f.synced) : '';
  },
};

export const fakeFsModule = {
  async readFile(path: string): Promise<string> {
    const f = files.get(String(path));
    if (!f) throw enoent(String(path));
    return f.data;
  },
  async mkdir(): Promise<void> {},
  async writeFile(path: string, data: string): Promise<void> {
    const f = fileFor(String(path));
    f.data = String(data);
    f.synced = 0;
    fsLog.push(`write ${path}`);
  },
  async appendFile(path: string, data: string): Promise<void> {
    fileFor(String(path)).data += String(data);
    fsLog.push(`append ${path}`);
  },
  async open(path: string) {
    const p = String(path);
    const f = fileFor(p);
    return {
      async appendFile(data: string): Promise<void> {
        const text = String(data);
        if (fakeDisk.failNextAppend !== undefined) {
          f.data += fakeDisk.failNextAppend;
          fakeDisk.failNextAppend = undefined;
          fsLog.push(`append ${p} (partial, EIO)`);
          throw Object.assign(new Error('EIO: i/o error, write'), { code: 'EIO' });
        }
        if (fakeDisk.chunkedAppends) {
          const half = Math.floor(text.length / 2);
          f.data += text.slice(0, half);
          await fakeDisk.beforeOp?.('append-rest', p);
          f.data += text.slice(half);
        } else {
          f.data += text;
        }
        fsLog.push(`append ${p}`);
      },
      async write(data: string): Promise<void> {
        f.data += String(data);
        fsLog.push(`append ${p}`);
      },
      async truncate(len = 0): Promise<void> {
        await fakeDisk.beforeOp?.('truncate', p);
        f.data = f.data.slice(0, len);
        f.synced = Math.min(f.synced, len);
        fsLog.push(`truncate ${p} ${len}`);
      },
      async sync(): Promise<void> {
        f.synced = f.data.length;
        fsLog.push(`sync ${p}`);
      },
      async datasync(): Promise<void> {
        f.synced = f.data.length;
        fsLog.push(`sync ${p}`);
      },
      async close(): Promise<void> {},
    };
  },
};
