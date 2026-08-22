// The scenarios the UI developer reproduced, as tests against the real
// openFiatSession: a reload mid-payment, and a tab duplicated mid-payment (which
// copies sessionStorage, so every client-side guard travels with it). Both used
// to open a SECOND vault-funded session for one chat. The rule proven here is
// that the chain is touched exactly once per (user, key), whatever the client does.
import { describe, expect, it } from 'vitest';
import { openFiatSession, type FiatSessionDeps } from '../src/lib/fiat-session-service';
import { IdempotencyStore } from '../src/lib/idempotency';
import { CreditsLedger, type LedgerStore } from '../src/lib/ledger';
import { makeGatekeeper } from '../src/lib/gatekeeper';

const USER = '0xb5e859a491607d8970bbd4d9ddd317d5c3357a80';
const HOST = '0x20f2a5fcdf271a5e6b04383c2915ea980a50948c';
const MODEL = `0x${'cd'.repeat(32)}`;
const CLIENT = '0x1a84ef2650c4299659f522c1961c6be4bc22cb14';

function memoryStore(): LedgerStore {
  const lines: string[] = [];
  return { load: async () => [...lines], append: async (l: string) => void lines.push(l) };
}

async function makeDeps(opts: { failCreate?: boolean } = {}) {
  const ledger = await CreditsLedger.open(memoryStore());
  await ledger.purchase(USER, 5_000_000n, 'seed-event');
  const creates: Array<{ host: string; depositMicro: bigint }> = [];
  let nextJobId = 990n;
  const deps: FiatSessionDeps = {
    ledger,
    credentials: { authenticate: (c: string) => (c === 'good' ? USER : null) } as FiatSessionDeps['credentials'],
    gatekeeper: makeGatekeeper({
      allowedHosts: [HOST],
      maxDepositPerSessionMicro: 2_000_000n,
      maxDailySpendMicro: 10_000_000n,
      maxOpensPerMinute: 100,
    }),
    chain: {
      ensureAllowance: async () => {},
      createSession: async (p) => {
        creates.push({ host: p.host, depositMicro: p.depositMicro });
        if (opts.failCreate) throw new Error('chain says no');
        const jobId = nextJobId++;
        return { jobId, depositor: '0xvault', txHash: `0xtx${jobId}` };
      },
    },
    signAuth: (sessionId, clientAddress) => ({
      scheme: 'fc1-session-auth-v1',
      signature: `0xsig-${sessionId}-${clientAddress}`,
      clientAddress,
    }),
    idempotency: await IdempotencyStore.open(memoryStore()),
  };
  return { deps, creates, ledger };
}

const REQUEST = {
  credential: 'good',
  host: HOST,
  modelId: MODEL,
  depositMicro: 500_000n,
  clientAddress: CLIENT,
  idempotencyKey: 'attempt-1',
};

describe('idempotent session opens (the reload and duplicate-tab cases)', () => {
  it('a reload retry replays the SAME session and escrows once', async () => {
    const { deps, creates, ledger } = await makeDeps();
    const first = await openFiatSession(deps, REQUEST);
    const balanceAfterFirst = ledger.availableMicro(USER);

    // F5 mid-payment: the page comes back knowing nothing, and sends the same key.
    const second = await openFiatSession(deps, REQUEST);

    expect(first.status).toBe('ok');
    expect(second.status).toBe('ok');
    expect(second.status === 'ok' && second.jobId).toBe(first.status === 'ok' && first.jobId);
    expect(second.status === 'ok' && second.replayed).toBe(true);
    expect(creates).toHaveLength(1); // ONE escrow, not two
    expect(ledger.availableMicro(USER)).toBe(balanceAfterFirst); // no second hold
  });

  it('the replayed authorisation is byte-identical, so the client can just use it', async () => {
    const { deps } = await makeDeps();
    const first = await openFiatSession(deps, REQUEST);
    const second = await openFiatSession(deps, REQUEST);
    expect(first.status === 'ok' && second.status === 'ok' && second.authorisation).toEqual(
      first.status === 'ok' ? first.authorisation : null
    );
  });

  it('a duplicated tab racing the original is refused, never double-charged', async () => {
    // Duplicate Tab copies sessionStorage, so the copy carries the same key and
    // none of the in-memory guards. It arrives while the first is mid-flight.
    const { deps, creates } = await makeDeps();
    let releaseCreate: () => void = () => {};
    const gate = new Promise<void>((r) => (releaseCreate = r));
    const slowChain = {
      ensureAllowance: async () => {},
      createSession: async (p: { host: string; modelId: string; depositMicro: bigint }) => {
        creates.push({ host: p.host, depositMicro: p.depositMicro });
        await gate;
        return { jobId: 991n, depositor: '0xvault', txHash: '0xtx991' };
      },
    };
    const slowDeps = { ...deps, chain: slowChain as FiatSessionDeps['chain'] };

    const inFlight = openFiatSession(slowDeps, REQUEST);
    await new Promise((r) => setTimeout(r, 5));
    const duplicate = await openFiatSession(slowDeps, REQUEST);

    expect(duplicate.status).toBe('in_flight'); // wait and ask again, not a second escrow
    releaseCreate();
    expect((await inFlight).status).toBe('ok');
    expect(creates).toHaveLength(1);
  });

  it('the same key with different parameters is an error, not a wrong replay', async () => {
    const { deps } = await makeDeps();
    await openFiatSession(deps, REQUEST);
    const mismatched = await openFiatSession(deps, { ...REQUEST, depositMicro: 900_000n });
    expect(mismatched.status).toBe('key_conflict');
  });

  it('a key freed by a failed create can be retried honestly', async () => {
    const { deps, creates } = await makeDeps({ failCreate: true });
    expect((await openFiatSession(deps, REQUEST)).status).toBe('chain_error');
    // Nothing escrowed, so the same key must work rather than be stuck.
    expect((await openFiatSession(deps, REQUEST)).status).toBe('chain_error');
    expect(creates).toHaveLength(2); // genuinely retried, because no money moved
  });

  it('a refusal frees the key too (no money moved)', async () => {
    const { deps } = await makeDeps();
    const refused = await openFiatSession(deps, { ...REQUEST, host: '0xnot-allowed' });
    expect(refused.status).toBe('refused');
    const retry = await openFiatSession(deps, REQUEST);
    expect(retry.status).toBe('ok'); // key was released, so this is a fresh attempt
  });

  it('keys are scoped per user: one account cannot replay another account session', async () => {
    const { deps } = await makeDeps();
    await openFiatSession(deps, REQUEST);
    const otherUser = {
      ...deps,
      credentials: { authenticate: () => '0xdifferentuser' } as unknown as FiatSessionDeps['credentials'],
    };
    const theirs = await openFiatSession(otherUser, REQUEST);
    expect(theirs.status).toBe('refused'); // their own (empty) balance, not a replay
  });

  it('without a key, behaviour is exactly as before (two opens, two sessions)', async () => {
    const { deps, creates } = await makeDeps();
    const { idempotencyKey: _omitted, ...noKey } = REQUEST;
    await openFiatSession(deps, noKey);
    await openFiatSession(deps, noKey);
    expect(creates).toHaveLength(2);
  });
});
