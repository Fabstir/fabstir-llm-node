// A scripted Base Sepolia for the owner relayer's unit tests (OP1.3a), served
// through a viem custom transport. It models just enough of EntryPoint v0.6
// and Coinbase Smart Wallet v1.1: getNonce, balanceOf (with a state override
// of the deposit slot), handleOps and simulateHandleOp (validation order and
// FailedOp reasons as the real contract), depositTo, and the account's owner
// views. A user operation "signature" here is fakeSign(userOpHash): the fake
// recomputes the hash from the op it was SENT, so a hash over any other op
// (other gas, other callData) fails with AA24, as on chain.
import {
  concat,
  custom,
  decodeFunctionData,
  encodeAbiParameters,
  encodeErrorResult,
  encodeEventTopics,
  encodeFunctionResult,
  keccak256,
  parseAbi,
  parseTransaction,
  toHex,
  type Hex,
} from 'viem';
import { entryPoint06Abi } from 'viem/account-abstraction';
import { ENTRY_POINT_V06, depositSlot, requiredPrefund, userOpHash, type UserOp } from '../../src/lib/owner-relayer';

const ownerAbi = parseAbi([
  'function execute(address target, uint256 value, bytes data) payable',
  'function addOwnerPublicKey(bytes32 x, bytes32 y)',
  'function removeOwnerAtIndex(uint256 index, bytes owner)',
  'function isOwnerPublicKey(bytes32 x, bytes32 y) view returns (bool)',
  'function ownerAtIndex(uint256 index) view returns (bytes)',
  'function nextOwnerIndex() view returns (uint256)',
  'function removedOwnersCount() view returns (uint256)',
  'event AddOwner(uint256 indexed index, bytes owner)',
]);

export const fakeSign = (hash: Hex): Hex => keccak256(concat([hash, '0x5151']));

type Account = { owners: Hex[]; removed: number };

export type FakeChain = ReturnType<typeof fakeOwnerChain>;

const EP = ENTRY_POINT_V06.toLowerCase();

function revert(data: Hex): never {
  throw Object.assign(new Error('execution reverted'), { code: 3, data });
}

function failedOp(reason: string): never {
  revert(encodeErrorResult({ abi: entryPoint06Abi, errorName: 'FailedOp', args: [0n, reason] }));
}

export function fakeOwnerChain(opts: { chainId?: number; baseFee?: bigint } = {}) {
  const chainId = opts.chainId ?? 84532;
  const state = {
    block: 1000n,
    baseFee: opts.baseFee ?? 5_000_000n,
    accounts: new Map<string, Account>(),
    deposits: new Map<string, bigint>(),
    nonces: new Map<string, bigint>(),
    relayerBalance: 10n ** 18n,
    relayerNonce: 0,
    receipts: new Map<string, Record<string, unknown>>(),
    /** every transaction the relayer sent, decoded */
    sent: [] as { to: string; functionName: string; value: bigint }[],
    methods: [] as string[],
    /** the account's inner call fails (UserOperationEvent success = false) */
    failInner: false,
    /** hold sendRawTransaction until this resolves */
    gate: null as Promise<void> | null,
    /** an RPC that is down */
    down: false,
    /** another add to the same account lands in the same block, AFTER ours (another device, another bundler) */
    sameBlockAdd: null as Hex | null,
    /** the receipt of handleOps carries no AddOwner event */
    dropAddOwnerEvent: false,
    /** the RPC goes down as soon as a handleOps receipt has been served (a lagging or failing node after the send) */
    downAfterReceipt: false,
    /** handleOps receipts served so far */
    handleOpsReceipts: new Set<string>(),
  };

  const deployed = (a: string) => state.accounts.has(a.toLowerCase());
  const key = (a: string) => a.toLowerCase();

  function deploy(address: string, firstKey: Hex) {
    state.accounts.set(key(address), { owners: [firstKey.toLowerCase() as Hex], removed: 0 });
  }

  function overrideDeposit(override: any, sender: string): bigint | undefined {
    if (!override) return undefined;
    const ep = override[ENTRY_POINT_V06] ?? override[EP];
    const value = ep?.stateDiff?.[depositSlot(sender as Hex)];
    return value === undefined ? undefined : BigInt(value);
  }

  /** EntryPoint v0.6's validation order, with its FailedOp reasons. */
  function validate(op: UserOp, deposit: bigint, checkSignature: boolean) {
    const isDeployed = deployed(op.sender);
    if (op.initCode !== '0x' && isDeployed) failedOp('AA10 sender already constructed');
    if (op.initCode === '0x' && !isDeployed) failedOp('AA20 account not deployed');
    if ((state.nonces.get(key(op.sender)) ?? 0n) !== op.nonce) failedOp('AA25 invalid account nonce');
    if (deposit < requiredPrefund(op)) failedOp("AA21 didn't pay prefund");
    if (checkSignature && op.signature.toLowerCase() !== fakeSign(userOpHash(op, chainId)).toLowerCase()) failedOp('AA24 signature error');
  }

  function snapshot() {
    return {
      accounts: new Map([...state.accounts].map(([k, v]) => [k, { owners: [...v.owners], removed: v.removed }])),
      nonces: new Map(state.nonces),
    };
  }

  /** The AddOwner events of the last apply(). */
  let lastAdded: { index: bigint; owner: Hex }[] = [];

  /** Apply the op's effect; returns whether the inner call succeeded. */
  function apply(op: UserOp): boolean {
    lastAdded = [];
    if (op.initCode !== '0x') {
      deploy(op.sender, `0x${op.initCode.slice(-128)}` as Hex);
      lastAdded.push({ index: 0n, owner: `0x${op.initCode.slice(-128)}`.toLowerCase() as Hex });
    }
    state.nonces.set(key(op.sender), op.nonce + 1n);
    if (state.failInner) return false;
    const outer = decodeFunctionData({ abi: ownerAbi, data: op.callData });
    if (outer.functionName !== 'execute') return false;
    const [target, , data] = outer.args as [Hex, bigint, Hex];
    if (key(target) !== key(op.sender)) return false;
    const inner = decodeFunctionData({ abi: ownerAbi, data });
    const acct = state.accounts.get(key(op.sender))!;
    if (inner.functionName === 'addOwnerPublicKey') {
      const [x, y] = inner.args as [Hex, Hex];
      const owner = concat([x, y]).toLowerCase() as Hex;
      lastAdded.push({ index: BigInt(acct.owners.length), owner });
      acct.owners.push(owner);
      return true;
    }
    if (inner.functionName === 'removeOwnerAtIndex') {
      const [index, owner] = inner.args as [bigint, Hex];
      const i = Number(index);
      if (acct.owners.length - acct.removed <= 1) return false;
      if (acct.owners[i] !== owner.toLowerCase()) return false;
      acct.owners[i] = '0x';
      acct.removed++;
      return true;
    }
    return false;
  }

  function accountView(address: string, data: Hex): Hex {
    const acct = state.accounts.get(key(address));
    if (!acct) return '0x'; // no code: viem's readContract fails
    const call = decodeFunctionData({ abi: ownerAbi, data });
    switch (call.functionName) {
      case 'isOwnerPublicKey': {
        const [x, y] = call.args as [Hex, Hex];
        const k = concat([x, y]).toLowerCase();
        return encodeFunctionResult({ abi: ownerAbi, functionName: 'isOwnerPublicKey', result: acct.owners.includes(k as Hex) });
      }
      case 'ownerAtIndex':
        return encodeFunctionResult({ abi: ownerAbi, functionName: 'ownerAtIndex', result: acct.owners[Number(call.args[0])] ?? '0x' });
      case 'nextOwnerIndex':
        return encodeFunctionResult({ abi: ownerAbi, functionName: 'nextOwnerIndex', result: BigInt(acct.owners.length) });
      case 'removedOwnersCount':
        return encodeFunctionResult({ abi: ownerAbi, functionName: 'removedOwnersCount', result: BigInt(acct.removed) });
      default:
        throw new Error(`fake account: unexpected ${call.functionName}`);
    }
  }

  function entryPointCall(data: Hex, override: any): Hex {
    const call = decodeFunctionData({ abi: entryPoint06Abi, data });
    switch (call.functionName) {
      case 'getNonce': {
        const [sender] = call.args as [Hex, bigint];
        return encodeFunctionResult({ abi: entryPoint06Abi, functionName: 'getNonce', result: state.nonces.get(key(sender)) ?? 0n });
      }
      case 'balanceOf': {
        const [sender] = call.args as [Hex];
        const value = overrideDeposit(override, sender) ?? state.deposits.get(key(sender)) ?? 0n;
        return encodeFunctionResult({ abi: entryPoint06Abi, functionName: 'balanceOf', result: value });
      }
      case 'handleOps': {
        const [ops] = call.args as unknown as [UserOp[]];
        const op = ops[0];
        validate(op, overrideDeposit(override, op.sender) ?? state.deposits.get(key(op.sender)) ?? 0n, true);
        return '0x';
      }
      case 'simulateHandleOp': {
        const [op, target, targetCallData] = call.args as unknown as [UserOp, Hex, Hex];
        validate(op, overrideDeposit(override, op.sender) ?? state.deposits.get(key(op.sender)) ?? 0n, false);
        const saved = snapshot();
        let result: Hex = '0x';
        let ok = false;
        try {
          apply(op);
          result = accountView(target, targetCallData);
          ok = true;
        } finally {
          state.accounts = saved.accounts;
          state.nonces = saved.nonces;
        }
        revert(
          encodeErrorResult({
            abi: entryPoint06Abi,
            errorName: 'ExecutionResult',
            args: [316_278n, 414_680_000_000n, 0, 0, ok, result],
          })
        );
      }
      // falls through: revert() never returns
      default:
        throw new Error(`fake EntryPoint: unexpected ${call.functionName}`);
    }
  }

  function receiptFor(hash: Hex, status: '0x1' | '0x0', logs: Record<string, unknown>[]) {
    state.block += 1n;
    const blockHash = keccak256(toHex(state.block));
    const r = {
      transactionHash: hash,
      transactionIndex: '0x0',
      blockHash,
      blockNumber: toHex(state.block),
      from: '0x0000000000000000000000000000000000000001',
      to: ENTRY_POINT_V06,
      cumulativeGasUsed: '0x5208',
      gasUsed: '0x5208',
      effectiveGasPrice: toHex(state.baseFee),
      contractAddress: null,
      logs: logs.map((l, i) => ({ ...l, blockHash, blockNumber: toHex(state.block), transactionHash: hash, transactionIndex: '0x0', logIndex: toHex(i), removed: false })),
      logsBloom: '0x' + '00'.repeat(256),
      status,
      type: '0x2',
    };
    state.receipts.set(hash, r);
  }

  async function sendRaw(raw: Hex): Promise<Hex> {
    if (state.gate) await state.gate;
    const tx = parseTransaction(raw as never) as { to?: Hex; data?: Hex; value?: bigint };
    const hash = keccak256(raw);
    state.relayerNonce++;
    const call = decodeFunctionData({ abi: entryPoint06Abi, data: tx.data! });
    state.sent.push({ to: tx.to!, functionName: call.functionName, value: tx.value ?? 0n });
    if (call.functionName === 'depositTo') {
      const [account] = call.args as [Hex];
      state.deposits.set(key(account), (state.deposits.get(key(account)) ?? 0n) + (tx.value ?? 0n));
      receiptFor(hash, '0x1', []);
      return hash;
    }
    if (call.functionName === 'handleOps') {
      const [ops] = call.args as unknown as [UserOp[]];
      const op = ops[0];
      try {
        validate(op, state.deposits.get(key(op.sender)) ?? 0n, true);
      } catch {
        receiptFor(hash, '0x0', []);
        return hash;
      }
      const success = apply(op);
      const accountLogs = state.dropAddOwnerEvent
        ? []
        : lastAdded.map((a) => ({
            address: op.sender,
            topics: encodeEventTopics({ abi: ownerAbi, eventName: 'AddOwner', args: { index: a.index } }),
            data: encodeAbiParameters([{ type: 'bytes' }], [a.owner]),
          }));
      if (state.sameBlockAdd) {
        // Another transaction later in the same block: state moves, OUR receipt does not show it.
        state.accounts.get(key(op.sender))!.owners.push(state.sameBlockAdd.toLowerCase() as Hex);
      }
      const topics = encodeEventTopics({
        abi: entryPoint06Abi,
        eventName: 'UserOperationEvent',
        args: { userOpHash: userOpHash(op, chainId), sender: op.sender, paymaster: '0x0000000000000000000000000000000000000000' },
      });
      const data = encodeAbiParameters(
        [{ type: 'uint256' }, { type: 'bool' }, { type: 'uint256' }, { type: 'uint256' }],
        [op.nonce, success, 414_680_000_000n, 414_680n]
      );
      receiptFor(hash, '0x1', [...accountLogs, { address: ENTRY_POINT_V06, topics, data }]);
      state.handleOpsReceipts.add(hash);
      return hash;
    }
    throw new Error(`fake chain: unexpected transaction ${call.functionName}`);
  }

  const transport = custom({
    async request({ method, params }: { method: string; params?: any }) {
      state.methods.push(method);
      if (state.down) throw new Error('fetch failed');
      switch (method) {
        case 'eth_chainId':
          return toHex(chainId);
        case 'eth_blockNumber':
          return toHex(state.block);
        case 'eth_getBlockByNumber':
          return {
            number: toHex(state.block),
            hash: keccak256(toHex(state.block)),
            parentHash: '0x' + '00'.repeat(32),
            timestamp: '0x6740f000',
            baseFeePerGas: toHex(state.baseFee),
            gasLimit: '0x1c9c380',
            gasUsed: '0x0',
            transactions: [],
            difficulty: '0x0',
            extraData: '0x',
            logsBloom: '0x' + '00'.repeat(256),
            miner: '0x0000000000000000000000000000000000000000',
            nonce: '0x0000000000000000',
            size: '0x0',
            stateRoot: '0x' + '00'.repeat(32),
            receiptsRoot: '0x' + '00'.repeat(32),
            transactionsRoot: '0x' + '00'.repeat(32),
            sha3Uncles: '0x' + '00'.repeat(32),
            uncles: [],
          };
        case 'eth_getCode':
          return deployed(params[0]) ? '0x363d3d373d3d363d7f360894' : '0x';
        case 'eth_getBalance':
          return toHex(state.relayerBalance);
        case 'eth_getTransactionCount':
          return toHex(state.relayerNonce);
        case 'eth_maxPriorityFeePerGas':
          return toHex(1_000_000n);
        case 'eth_gasPrice':
          return toHex(state.baseFee);
        case 'eth_estimateGas':
          return toHex(1_322_063n);
        case 'eth_call': {
          const [{ to, data }, , override] = params;
          if (key(to) === EP) return entryPointCall(data, override);
          return accountView(to, data);
        }
        case 'eth_sendRawTransaction':
          return sendRaw(params[0]);
        case 'eth_getTransactionReceipt': {
          const r = state.receipts.get(params[0]) ?? null;
          if (r && state.downAfterReceipt && state.handleOpsReceipts.has(params[0])) state.down = true;
          return r;
        }
        case 'eth_getTransactionByHash':
          return null;
        default:
          throw Object.assign(new Error(`the method ${method} does not exist`), { code: -32601 });
      }
    },
  }, { retryCount: 0 });

  return { transport, state, deploy };
}
