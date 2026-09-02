// Phase 3: escrow round-trip from the passkey smart account.
//
// The website makes the SAME two calls the Blender helper makes for every
// paid clip: ERC-20 approve, then createSessionJobForModelWithToken on the
// JobMarketplace. The deposit is escrowed in the contract (never sent to the
// host or to us); a zero-token host settlement refunds it in full. Calldata is
// encoded with ethers v6 and sent through the raw passkey provider
// (eth_sendTransaction), so the auth lib's bundled ethers v5 never touches
// transactions.
//
// Session parameters mirror real settled session 931 on Base Sepolia
// (maxDuration 3600, proofInterval 1000, proofTimeoutWindow 300, price 904):
// values proven to pass contract validation against the live host.
import { Interface, type Log } from 'ethers';
import { usdcTokenAddress } from './balance';
import type { SessionKind } from './gatekeeper';

export const ESCROW_INTERFACE = new Interface([
  'function approve(address spender, uint256 amount) returns (bool)',
  'function createSessionJobForModelWithToken(address host, bytes32 modelId, address token, uint256 deposit, uint256 pricePerToken, uint256 maxDuration, uint256 proofInterval, uint256 proofTimeoutWindow)',
  'event SessionJobCreatedForModel(uint256 indexed jobId, address indexed depositor, address indexed host, bytes32 modelId, uint256 deposit)',
]);

// Contract minimum deposit: 0.5 USDC = 50 credits.
export const TEST_DEPOSIT_MICRO = 500_000n;
export const SESSION_MAX_DURATION = 3600n;
export const SESSION_PROOF_INTERVAL = 1000n;
export const SESSION_PROOF_TIMEOUT_WINDOW = 300n;

// A training run lives hours and posts a proof per slice. The node's accept
// gate (DESIGN-TRAINING-M0-INTERFACE A.3) needs remaining lifetime >=
// TRAIN_JOB_TIMEOUT_SECS (12600) + 600 and proofTimeoutWindow >= 3600 (the
// contract maximum, MAX_PROOF_TIMEOUT), so a training session takes the
// SDK's wallet-path shape, proven live by every wallet-paid run. A session
// created with the constants above passes the vault's gate, funds, and then
// fails A.3 after escrow — a funded round trip that cannot succeed.
export const TRAINING_SESSION_MAX_DURATION = 14400n;
export const TRAINING_SESSION_PROOF_INTERVAL = 1000n;
export const TRAINING_SESSION_PROOF_TIMEOUT_WINDOW = 3600n;
/** The contract's MAX_PROOF_TIMEOUT (live probe 2026-08-22). */
export const MAX_PROOF_TIMEOUT_WINDOW = 3600n;

export interface SessionShape {
  maxDuration: bigint;
  proofInterval: bigint;
  proofTimeoutWindow: bigint;
}

/** The standard kind's proof window. The contract's timeout is a permission
 *  any caller gains once proof silence exceeds this window; on 300 s that
 *  opens for every vault chat that idles five minutes before its first proof.
 *  Nothing of ours acts on it before the listener's reclaim delay, so the
 *  window costs the flow nothing and can be raised to the maximum; it is an
 *  env knob rather than a changed default because a 3600/3600 shape has not
 *  yet been created on this contract, and the first one should be a probe. */
function standardProofTimeoutWindow(): bigint {
  const raw = process.env.FIAT_SESSION_PROOF_TIMEOUT_WINDOW;
  if (raw === undefined || raw === '') return SESSION_PROOF_TIMEOUT_WINDOW;
  let value: bigint;
  try {
    value = BigInt(raw);
  } catch {
    throw new Error(`FIAT_SESSION_PROOF_TIMEOUT_WINDOW must be an integer number of seconds, got "${raw}"`);
  }
  if (value <= 0n || value > MAX_PROOF_TIMEOUT_WINDOW) {
    throw new Error(
      `FIAT_SESSION_PROOF_TIMEOUT_WINDOW must be 1..${MAX_PROOF_TIMEOUT_WINDOW} seconds (the contract maximum), got ${value}`
    );
  }
  return value;
}

/** The on-chain session parameters per job kind. The SERVICE owns these:
 *  the vault fronts the money, so three raw numbers from a browser are never
 *  accepted. Absent kind = `standard` = the chat/render shape above. */
export function sessionShapeFor(kind: SessionKind = 'standard'): SessionShape {
  if (kind === 'training') {
    return {
      maxDuration: TRAINING_SESSION_MAX_DURATION,
      proofInterval: TRAINING_SESSION_PROOF_INTERVAL,
      proofTimeoutWindow: TRAINING_SESSION_PROOF_TIMEOUT_WINDOW,
    };
  }
  return {
    maxDuration: SESSION_MAX_DURATION,
    proofInterval: SESSION_PROOF_INTERVAL,
    proofTimeoutWindow: standardProofTimeoutWindow(),
  };
}

function requireEnv(name: string, value: string | undefined): string {
  if (!value) throw new Error(`${name} is not set — copy .env.example to .env.local`);
  return value;
}

export function jobMarketplaceAddress(): string {
  return requireEnv('NEXT_PUBLIC_CONTRACT_JOB_MARKETPLACE', process.env.NEXT_PUBLIC_CONTRACT_JOB_MARKETPLACE);
}

export function escrowTestHost(): string {
  return requireEnv('NEXT_PUBLIC_TEST_HOST', process.env.NEXT_PUBLIC_TEST_HOST);
}

export function escrowModelId(): string {
  return requireEnv('NEXT_PUBLIC_LTX_MODEL_ID', process.env.NEXT_PUBLIC_LTX_MODEL_ID);
}

export function escrowPricePerToken(): bigint {
  return BigInt(requireEnv('NEXT_PUBLIC_SESSION_PRICE_PER_TOKEN', process.env.NEXT_PUBLIC_SESSION_PRICE_PER_TOKEN));
}

export function encodeApprove(spender: string, amountMicro: bigint): string {
  return ESCROW_INTERFACE.encodeFunctionData('approve', [spender, amountMicro]);
}

export function encodeCreateSession(params: {
  host: string;
  modelId: string;
  token: string;
  depositMicro: bigint;
  pricePerToken: bigint;
}): string {
  return ESCROW_INTERFACE.encodeFunctionData('createSessionJobForModelWithToken', [
    params.host,
    params.modelId,
    params.token,
    params.depositMicro,
    params.pricePerToken,
    SESSION_MAX_DURATION,
    SESSION_PROOF_INTERVAL,
    SESSION_PROOF_TIMEOUT_WINDOW,
  ]);
}

/** Extract the new session's jobId from a receipt's logs, or null if absent. */
export function decodeSessionCreated(logs: ReadonlyArray<Pick<Log, 'topics' | 'data'>>): bigint | null {
  const topic = ESCROW_INTERFACE.getEvent('SessionJobCreatedForModel')!.topicHash;
  for (const log of logs) {
    if (log.topics[0] === topic) return BigInt(log.topics[1]);
  }
  return null;
}

type Eip1193Provider = { request(args: { method: string; params?: unknown }): Promise<unknown> };

const BASE_SEPOLIA_CHAIN_HEX = '0x14a34'; // 84532

/** The approve + create-session pair as an EIP-5792 calls array. */
export function buildEscrowCalls(): Array<{ to: string; data: string }> {
  const marketplace = jobMarketplaceAddress();
  const token = usdcTokenAddress();
  return [
    { to: token, data: encodeApprove(marketplace, TEST_DEPOSIT_MICRO) },
    {
      to: marketplace,
      data: encodeCreateSession({
        host: escrowTestHost(),
        modelId: escrowModelId(),
        token,
        depositMicro: TEST_DEPOSIT_MICRO,
        pricePerToken: escrowPricePerToken(),
      }),
    },
  ];
}

type CallsStatus = {
  status?: unknown;
  receipts?: Array<{ logs?: Array<{ topics: string[]; data: string }> }>;
};

function isConfirmed(status: unknown): boolean {
  return status === 'CONFIRMED' || status === 200; // EIP-5792 v1 / v2 encodings
}

function isFailed(status: unknown): boolean {
  if (typeof status === 'number') return status >= 400;
  return status === 'FAILED' || status === 'REVERTED';
}

/** Poll wallet_getCallsStatus until the batch confirms, fails, or times out. */
export async function waitForCallsStatus(
  provider: Eip1193Provider,
  callsId: string,
  timeoutMs = 120_000
): Promise<CallsStatus | null> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const res = (await provider.request({ method: 'wallet_getCallsStatus', params: [callsId] })) as CallsStatus;
    if (isConfirmed(res?.status)) return res;
    if (isFailed(res?.status)) {
      throw new Error(`The escrow batch failed on-chain: ${JSON.stringify(res?.status)}`);
    }
    await new Promise((r) => setTimeout(r, 2500));
  }
  return null;
}

/** Submit an EIP-5792 atomic batch through the passkey wallet; returns the calls id. */
export async function sendCalls(
  provider: Eip1193Provider,
  from: string,
  calls: Array<{ to: string; data: string }>
): Promise<string> {
  const response = await provider.request({
    method: 'wallet_sendCalls',
    params: [{ version: '1.0', chainId: BASE_SEPOLIA_CHAIN_HEX, from, calls }],
  });
  const callsId = typeof response === 'string' ? response : (response as { id?: string })?.id ?? '';
  if (!callsId) throw new Error(`Unexpected wallet response: ${JSON.stringify(response)}`);
  return callsId;
}

/**
 * The full test flow as ONE atomic wallet request (EIP-5792 wallet_sendCalls):
 * approve + createSessionJobForModelWithToken in a single popup and a single
 * confirmation. A two-transaction flow fails on smart wallets because the
 * second popup opens after the user-gesture window has expired (observed
 * live: code 4001 with no second popup).
 */
export async function runEscrowTest(
  provider: Eip1193Provider,
  from: string,
  onStatus: (s: string) => void
): Promise<{ jobId: bigint | null; callsId: string }> {
  onStatus('Confirm the escrow in the wallet popup (one confirmation)…');
  const callsId = await sendCalls(provider, from, buildEscrowCalls());

  onStatus('Waiting for the batch to land…');
  const status = await waitForCallsStatus(provider, callsId);
  const logs = (status?.receipts ?? []).flatMap((r) => r.logs ?? []);
  return { jobId: decodeSessionCreated(logs), callsId };
}
