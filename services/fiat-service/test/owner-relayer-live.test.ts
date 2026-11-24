// OP1.3a gated live test (LIVE_CHAIN=1): read-only against Base Sepolia. NO
// funds, NO transactions. The relayer's key is a fresh random one holding
// nothing; the account's EntryPoint deposit comes from a state override.
//
//  - prepare for a fresh software-passkey account builds an op (initCode +
//    execute(addOwnerPublicKey)) whose handleOps simulation SUCCEEDS with a
//    valid passkey signature, and whose simulateHandleOp shows the new key
//    as an owner afterwards;
//  - a signature by a key that is not an owner FAILS (AA24);
//  - submit gets as far as the money step and stops there (no funds:
//    relayer_low_balance), sending nothing.
import { describe, expect, it } from 'vitest';
import { createPublicClient, http, type Hex } from 'viem';
import { generatePrivateKey } from 'viem/accounts';
import { toCoinbaseSmartAccount, toWebAuthnAccount } from 'viem/account-abstraction';
import { baseSepolia } from 'viem/chains';
import { MemoryLedgerStore } from '../src/lib/ledger';
import { OwnerRelayer, accountAddressFor, requiredPrefund, type UserOp, type WireUserOp } from '../src/lib/owner-relayer';
import { softPasskey, type SoftPasskey } from './helpers/soft-passkey';

const RPC = process.env.LIVE_RPC_URL || 'https://base-sepolia-rpc.publicnode.com';
const RP_ID = 'localhost';

function fromWire(w: WireUserOp): UserOp {
  return {
    ...w,
    nonce: BigInt(w.nonce),
    callGasLimit: BigInt(w.callGasLimit),
    verificationGasLimit: BigInt(w.verificationGasLimit),
    preVerificationGas: BigInt(w.preVerificationGas),
    maxFeePerGas: BigInt(w.maxFeePerGas),
    maxPriorityFeePerGas: BigInt(w.maxPriorityFeePerGas),
  };
}

async function signAs(passkey: SoftPasskey, address: `0x${string}`, op: UserOp): Promise<Hex> {
  const client = createPublicClient({ chain: baseSepolia, transport: http(RPC) });
  const owner = toWebAuthnAccount({ credential: { id: passkey.credentialId, publicKey: passkey.publicKey }, rpId: RP_ID, getFn: passkey.getFn as never });
  const account = await toCoinbaseSmartAccount({ client, owners: [owner], version: '1.1', address });
  const { signature: _ignored, ...unsigned } = op;
  return account.signUserOperation({ ...unsigned, chainId: baseSepolia.id } as never);
}

describe.skipIf(!process.env.LIVE_CHAIN)('owner relayer against Base Sepolia (LIVE_CHAIN=1, read-only, no funds)', () => {
  it('a passkey-signed add simulates successfully, a wrong signature fails, submit stops before any send', async () => {
    const first = softPasskey();
    const second = softPasskey();
    const stranger = softPasskey();
    const sender = accountAddressFor(first.publicKey);
    const alarms: string[] = [];
    const relayer = await OwnerRelayer.open({
      transport: http(RPC),
      chainId: 84532,
      privateKey: generatePrivateKey(),
      store: new MemoryLedgerStore(),
      log: () => undefined,
      alarm: (l) => alarms.push(l),
    });
    const userId = sender.toLowerCase();

    const prep = await relayer.prepare(userId, { action: 'add', publicKey: second.publicKey, firstOwnerPublicKey: first.publicKey });
    expect(prep.status, JSON.stringify(prep.body)).toBe(200);
    const op = fromWire(prep.body.userOperation as WireUserOp);
    expect(op.sender).toBe(sender);
    expect(op.initCode.length).toBeGreaterThan(2); // fresh account: deployed by this op
    expect(op.nonce).toBe(0n);

    const action = { action: 'add' as const, publicKey: second.publicKey };
    const good = await relayer.simulate({ ...op, signature: await signAs(first, sender, op) }, action);
    expect(good.ok, JSON.stringify(good, (_k, v) => (typeof v === 'bigint' ? v.toString() : v))).toBe(true);
    if (good.ok) {
      console.log(
        `[live] gas: preOpGas ${good.preOpGas}, paid ${good.paid} wei at maxFee ${op.maxFeePerGas}, handleOps estimateGas ${good.handleOpsGas ?? 'n/a'}, prefund ${requiredPrefund(op)} wei`
      );
    }

    const bad = await relayer.simulate({ ...op, signature: await signAs(stranger, sender, op) }, action);
    expect(bad.ok).toBe(false);
    if (!bad.ok) expect(bad.reason).toMatch(/AA24/);

    // submit with the right signature: every check and the simulation pass, then the
    // unfunded wallet stops it before depositTo/handleOps are sent.
    const sub = await relayer.submit(userId, { userOpHash: prep.body.userOpHash, signature: await signAs(first, sender, op) });
    expect(sub).toEqual({ status: 503, body: { error: 'relayer_low_balance' } });
    expect(alarms.some((a) => a.includes('ALARM'))).toBe(true);
  }, 120_000);
});
