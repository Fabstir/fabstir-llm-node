// FC1.2 — the session-open service: credential -> gatekeeper -> vault create
// -> bind -> signed client authorisation. Order is the security property: the
// vault is NEVER touched before the gatekeeper allows, and a failed create
// releases the hold to the penny.
import { describe, expect, it } from 'vitest';
import { openFiatSession, type FiatChain, type FiatSessionDeps } from '../src/lib/fiat-session-service';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { FiatCredentials } from '../src/lib/fiat-credentials';
import { makeGatekeeper } from '../src/lib/gatekeeper';
import type { SessionAuthorisation } from '../src/lib/fiat-vault';

const HOST = '0xabcd000000000000000000000000000000000001';
const MODEL = `0x${'ab'.repeat(32)}`;
const CLIENT = '0x1234567890abcdef1234567890abcdef12345678';
const VAULT = '0x8ba1f109551bD432803012645Ac136ddd64DBA72';

function fakeChain(opts?: { failCreate?: boolean }) {
  const log: string[] = [];
  return {
    log,
    chain: {
      ensureAllowance: async (deposit: bigint) => {
        log.push(`allowance:${deposit}`);
      },
      createSession: async () => {
        log.push('create');
        if (opts?.failCreate) throw new Error('tx reverted');
        return { jobId: 842n, depositor: VAULT, txHash: '0xtx' };
      },
    },
  };
}

const fakeSignAuth = (sessionId: bigint, clientAddress: string): SessionAuthorisation => ({
  scheme: 'fc1-session-auth-v1',
  signature: `0xsig-${sessionId}-${clientAddress}`,
  clientAddress,
});

async function makeDeps(opts?: { failCreate?: boolean; balanceMicro?: bigint }) {
  const ledger = await CreditsLedger.open(new MemoryLedgerStore());
  const credentials = await FiatCredentials.open(new MemoryLedgerStore());
  const token = await credentials.issue('user-1');
  if (opts?.balanceMicro !== 0n) {
    await ledger.purchase('user-1', opts?.balanceMicro ?? 1_000_000n, 'evt_1');
  }
  const { chain, log } = fakeChain(opts);
  const deps: FiatSessionDeps = {
    ledger,
    credentials,
    gatekeeper: makeGatekeeper({
      allowedHosts: [HOST],
      maxDepositPerSessionMicro: 2_000_000n,
      maxDailySpendMicro: 10_000_000n,
      maxOpensPerMinute: 3,
    }),
    chain,
    signAuth: fakeSignAuth,
  };
  return { deps, ledger, token, chainLog: log };
}

const request = (token: string, overrides = {}) => ({
  credential: token,
  host: HOST,
  modelId: MODEL,
  depositMicro: 500_000n,
  clientAddress: CLIENT,
  ...overrides,
});

describe('openFiatSession', () => {
  // The double-bind race, live on 2026-07-26 as job 987. The reconcile sweep
  // runs on the settlement listener's tick; when a tick lands inside our
  // confirmation wait it resolves the same receipt and binds first. bindSession
  // is deliberately strict about rebinding, so this call threw and the route
  // 500'd — on a session that had been created and paid for. The customer saw
  // a failure, the deposit was gone, and the session was stranded.
  it('succeeds when the reconcile sweep binds the hold first (lost race is not a failure)', async () => {
    const { deps, ledger, token } = await makeDeps();
    const realCreate = deps.chain.createSession;
    const racingChain: FiatChain = {
      ...deps.chain,
      // Bind from "the reconciler" during the confirmation wait, exactly as the
      // sweep does, after onSubmitted has recorded the pending create.
      createSession: async (args) => {
        const created = await realCreate(args);
        const orphan = ledger.unboundHolds()[0];
        if (orphan) await ledger.bindSession(orphan.holdId, created.jobId);
        return created;
      },
    };

    const outcome = await openFiatSession({ ...deps, chain: racingChain }, request(token));

    expect(outcome.status).toBe('ok');
    if (outcome.status !== 'ok') return;
    expect(outcome.jobId).toBe(842n);
    // Bound exactly once, to us, and the money moved exactly once.
    expect(ledger.userForJob(842n)).toBe('user-1');
    expect(ledger.availableMicro('user-1')).toBe(500_000n);
  });

  it('still fails loudly if the jobId was bound to somebody else\'s hold', async () => {
    const { deps, ledger, token } = await makeDeps();
    const realCreate = deps.chain.createSession;
    const stealingChain: FiatChain = {
      ...deps.chain,
      createSession: async (args) => {
        const created = await realCreate(args);
        // A DIFFERENT hold claims our jobId — a genuine misbind, never benign.
        await ledger.purchase('user-2', 1_000_000n, 'evt_2');
        const other = await ledger.openHold(
          { userId: 'user-2', host: HOST, depositMicro: 500_000n },
          deps.gatekeeper
        );
        if (other.ok) await ledger.bindSession(other.holdId, created.jobId);
        return created;
      },
    };

    await expect(openFiatSession({ ...deps, chain: stealingChain }, request(token))).rejects.toThrow();
  });

  it('happy path: holds, creates from the vault, binds the jobId, returns the signed authorisation', async () => {
    const { deps, ledger, token, chainLog } = await makeDeps();
    const outcome = await openFiatSession(deps, request(token));
    expect(outcome).toEqual({
      status: 'ok',
      sessionId: 842n,
      jobId: 842n,
      authorisation: fakeSignAuth(842n, CLIENT),
    });
    expect(chainLog).toEqual(['allowance:500000', 'create']);
    expect(ledger.availableMicro('user-1')).toBe(500_000n);
    expect(ledger.userForJob(842n)).toBe('user-1');
  });

  it('a bad credential is refused before the ledger or vault is touched', async () => {
    const { deps, ledger, chainLog } = await makeDeps();
    const outcome = await openFiatSession(deps, request('fc1_' + '00'.repeat(32)));
    expect(outcome).toEqual({ status: 'unauthorised' });
    expect(chainLog).toEqual([]);
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n);
  });

  it('a revoked credential is refused', async () => {
    const { deps, token } = await makeDeps();
    await deps.credentials.revokeAll('user-1');
    expect(await openFiatSession(deps, request(token))).toEqual({ status: 'unauthorised' });
  });

  it('a gatekeeper refusal never reaches the chain (no allowance, no create)', async () => {
    const { deps, token, chainLog } = await makeDeps({ balanceMicro: 400_000n });
    const outcome = await openFiatSession(deps, request(token));
    expect(outcome).toEqual({ status: 'refused', reason: 'INSUFFICIENT_BALANCE' });
    expect(chainLog).toEqual([]);
  });

  it('an off-allow-list host is refused before the chain', async () => {
    const { deps, token, chainLog } = await makeDeps();
    const outcome = await openFiatSession(
      deps,
      request(token, { host: '0x9999999999999999999999999999999999999999' })
    );
    expect(outcome).toEqual({ status: 'refused', reason: 'HOST_NOT_ALLOWED' });
    expect(chainLog).toEqual([]);
  });

  it('a failed create releases the hold exactly and reports chain_error', async () => {
    const { deps, ledger, token } = await makeDeps({ failCreate: true });
    const outcome = await openFiatSession(deps, request(token));
    expect(outcome).toEqual({ status: 'chain_error', message: 'tx reverted' });
    expect(ledger.availableMicro('user-1')).toBe(1_000_000n);
    expect(ledger.outstandingMicro()).toBe(1_000_000n);
  });

  it('the authorisation is signed over the RETURNED sessionId and the REQUESTED client address', async () => {
    const { deps, token } = await makeDeps();
    const outcome = await openFiatSession(deps, request(token));
    if (outcome.status !== 'ok') throw new Error(`unexpected ${outcome.status}`);
    expect(outcome.authorisation.signature).toBe(`0xsig-842-${CLIENT}`);
  });

  it('records a pending-create marker the instant the create tx is submitted, then clears it on bind (M2)', async () => {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    await ledger.purchase('user-1', 1_000_000n, 'evt_1');
    const credentials = await FiatCredentials.open(new MemoryLedgerStore());
    const token = await credentials.issue('user-1');

    let pendingAtSubmit: ReturnType<CreditsLedger['pendingCreates']> = [];
    const chain: FiatChain = {
      ensureAllowance: async () => {},
      createSession: async (p) => {
        await p.onSubmitted?.('0xsubmitted'); // vault submits the create tx
        pendingAtSubmit = ledger.pendingCreates(); // snapshot the recoverable orphan BEFORE bind
        return { jobId: 842n, depositor: VAULT, txHash: '0xsubmitted' };
      },
    };
    const deps: FiatSessionDeps = {
      ledger,
      credentials,
      gatekeeper: makeGatekeeper({
        allowedHosts: [HOST],
        maxDepositPerSessionMicro: 2_000_000n,
        maxDailySpendMicro: 10_000_000n,
        maxOpensPerMinute: 3,
      }),
      chain,
      signAuth: fakeSignAuth,
    };

    const outcome = await openFiatSession(deps, request(token));
    expect(outcome.status).toBe('ok');
    // Between submit and bind the hold was a recoverable pending orphan…
    expect(pendingAtSubmit).toEqual([
      expect.objectContaining({ txHash: '0xsubmitted', userId: 'user-1', host: HOST }),
    ]);
    // …and once bound, the marker is cleared and the job maps to the user.
    expect(ledger.pendingCreates()).toEqual([]);
    expect(ledger.userForJob(842n)).toBe('user-1');
  });
});
