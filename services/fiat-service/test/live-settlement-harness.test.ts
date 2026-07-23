// LIVE settlement harness — proves the production recovery path end to end
// against the REAL Base Sepolia chain, using the REAL production components:
// JsonlLedgerStore, CreditsLedger, makeChainSessionReader,
// makeChainSettlementSource, startSettlementListener. No fakes anywhere.
//
// Inert in normal test runs. To run:
//   LIVE_HARNESS=1 NEXT_PUBLIC_CONTRACT_JOB_MARKETPLACE=0x... \
//     npx vitest run --no-file-parallelism test/live-settlement-harness.test.ts
//
// It reconstructs the deployed server's exact ledger state for the UI dev's
// account (as of the job-965 incident, 2026-07-23), then proves:
//   1. the chain reader sees job 965 as settled with refund 456988
//   2. one real listener tick recovers the refund and alarms the recovery
//   3. the resulting balance is 4,470,964 micro
// and finally reports what the LIVE service says, so any divergence between
// "the code" and "the deployment" is printed in one line.
import { describe, expect, it } from 'vitest';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { CreditsLedger, JsonlLedgerStore } from '../src/lib/ledger';
import { makeGatekeeper } from '../src/lib/gatekeeper';
import {
  makeChainSessionReader,
  makeChainSettlementSource,
  startSettlementListener,
} from '../src/lib/settlement-listener';

const USER = '0xb5e859a491607d8970bbd4d9ddd317d5c3357a80';
const HOST = '0x048afA7126A3B684832886b78e7cC1Dd4019557E';
const DEPOSIT = 500_000n;
const RENDER_REFUND = 456_988n; // 500,000 − 43,012 gross, all three renders

const gate = makeGatekeeper({
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 100_000_000n,
  maxOpensPerMinute: 100,
});

describe.runIf(process.env.LIVE_HARNESS === '1')('LIVE: job-965 recovery, real chain, real code', () => {
  it('reconstructs the server ledger, reads real chain state, recovers 965 in one tick', async () => {
    // ---- 0. Reconstruct the deployed ledger's exact state ----
    const dir = mkdtempSync(join(tmpdir(), 'live-harness-'));
    const ledger = await CreditsLedger.open(new JsonlLedgerStore(join(dir, 'ledger.jsonl')));
    await ledger.purchase(USER, 5_000_000n, 'evt_buy_500');
    // The server's 40-credit cash-out is handled arithmetically (the −400,000n
    // in every assertion below); the money-critical paths — holds, binds,
    // settles, the sweep — all use the real APIs.
    for (const [jobId, settled] of [
      [960n, true],
      [962n, true],
      [965n, false],
    ] as Array<[bigint, boolean]>) {
      const open = await ledger.openHold({ userId: USER, host: HOST, depositMicro: DEPOSIT }, gate);
      if (!open.ok) throw new Error(`open refused: ${open.reason}`);
      await ledger.bindSession(open.holdId, jobId);
      if (settled) await ledger.settle(jobId, RENDER_REFUND);
    }
    const reconstructed = ledger.availableMicro(USER);
    console.log(`[harness] reconstructed balance (before 40-credit cashout): ${reconstructed}`);
    // 5,000,000 − 3×500,000 + 2×456,988 = 4,413,976; server also has −400,000
    // cash-out ⇒ 4,013,976. The delta MUST be exactly the cash-out:
    expect(reconstructed - 400_000n).toBe(4_013_976n);
    expect(ledger.boundJobIds().map(String)).toEqual(['965']);
    console.log('[harness] STEP 0 OK — ledger state matches the server (modulo the 400k cash-out)');

    // ---- 1. The real chain reader on the real chain ----
    const reader = makeChainSessionReader();
    const s = await reader.session(965n);
    console.log(`[harness] chain says job 965: ended=${s?.ended} refundedToUser=${s?.refundedToUser}`);
    expect(s?.ended).toBe(true);
    expect(s?.refundedToUser).toBe(RENDER_REFUND);
    console.log('[harness] STEP 1 OK — executed state readable and correct');

    // ---- 2. One REAL production tick: real source, real reader, real ledger ----
    const alarms: string[] = [];
    let cursorVal: number | undefined = undefined;
    const listener = startSettlementListener({
      ledger,
      source: makeChainSettlementSource(),
      cursor: {
        load: async () => cursorVal,
        save: async (b) => {
          cursorVal = b;
        },
      },
      fromBlock: 44_533_200, // past 965's settlement block — the sweep must not need events
      onAlarm: (m) => {
        alarms.push(m);
        console.log(`[harness] ALARM: ${m}`);
      },
      manual: true,
      safetyLag: 5,
      overlapBlocks: 30,
      tickTimeoutMs: 90_000,
      stateSweep: reader,
    });
    await listener.tick();
    await listener.stop();

    expect(ledger.refundForJob(965n)).toBe(RENDER_REFUND);
    expect(alarms.some((m) => m.includes('state-sweep recovered job 965'))).toBe(true);
    const finalBalance = ledger.availableMicro(USER);
    console.log(`[harness] STEP 2 OK — one tick recovered 965; harness balance ${finalBalance} (server-equivalent ${finalBalance - 400_000n})`);
    expect(finalBalance - 400_000n).toBe(4_470_964n);

    // ---- 3. What does the LIVE deployment say right now? ----
    try {
      const res = await fetch(`https://fiat.fabstir.net/v1/fiat/balance?address=${USER}`);
      const live = (await res.json()) as { availableMicro?: string };
      console.log(`[harness] LIVE service balance: ${live.availableMicro}`);
      if (live.availableMicro === '4470964') {
        console.log('[harness] STEP 3: the deployment AGREES — system healthy end to end');
      } else {
        console.log(
          `[harness] STEP 3 DIVERGENCE: code provably recovers 965 (steps 1-2) but the deployment reads ${live.availableMicro}. ` +
            'The defect is in the DEPLOYMENT (stale build, dead loop, or env), not the code path. ' +
            "Check on the server: (a) the running build's mtime vs the restore, (b) journal for heartbeat/ALARM lines, (c) FIAT_SETTLEMENT_ENABLED in the live process env."
        );
      }
    } catch (e) {
      console.log(`[harness] STEP 3: live service unreachable from here (${e}) — steps 1-2 remain conclusive for the code`);
    }
  }, 120_000);
});
