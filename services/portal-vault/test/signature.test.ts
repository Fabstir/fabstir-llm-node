// R1: on-chain signature verification, ported from the fiat service's
// fiat-signature.test.ts. The tests run the REAL viem code path and stub only
// the transport (a viem `custom()` transport whose eth_call returns the
// ABI-encoded bool the universal validator would return: 0x..01 valid,
// 0x..00 invalid). What this module adds to viem, surfacing an RPC outage
// instead of silently reporting "invalid", is pinned by the transport tests;
// the list of RPC URLs tried in order is pinned by the second-URL tests.
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { afterEach, describe, expect, it } from 'vitest';
import { HttpRequestError, RpcRequestError, custom, hashMessage, type Transport } from 'viem';
import {
  SignatureCheckUnavailableError,
  SUPPORTED_CHAIN_IDS,
  MAX_SIGNATURE_HEX_CHARS,
  createSignatureVerifier,
  isExecutionReverted,
  isWellFormedSignature,
  withConcurrencyLimit,
  type SignatureVerifier,
  type TypedDataOwnershipProof,
} from '../src/signature.js';

const ADDR = '0x1234567890abcDEF1234567890abcdef12345678';
const ADDR_LC = ADDR.toLowerCase();
// A plausibly shaped ERC-6492-wrapped signature. Its bytes are irrelevant: the
// stubbed validator decides; only the hex shape has to pass input validation.
const SIG = '0x' + 'ab'.repeat(210) + '6492'.repeat(8);
const VALID = '0x0000000000000000000000000000000000000000000000000000000000000001';
const INVALID = '0x0000000000000000000000000000000000000000000000000000000000000000';

type RpcCall = { method: string; params: unknown };

/** A viem transport whose eth_call is driven by `handler`. retryCount 0 keeps a
 *  thrown error from being retried, so the call log is deterministic. */
function stub(handler: (method: string, params: unknown) => Promise<unknown>): { transport: Transport; calls: RpcCall[] } {
  const calls: RpcCall[] = [];
  const transport = custom(
    {
      request: ({ method, params }: { method: string; params?: unknown }) => {
        calls.push({ method, params });
        return handler(method, params);
      },
    },
    { retryCount: 0 }
  );
  return { transport, calls };
}

const answering = (answer: string) =>
  stub(async (m) => (m === 'eth_call' ? answer : Promise.reject(new Error(`unexpected ${m}`))));
const alwaysValid = () => answering(VALID);
const alwaysInvalid = () => answering(INVALID);
const down = () =>
  stub(async () => {
    throw new Error('ECONNREFUSED');
  });

const verifierOver = (...stubs: { transport: Transport }[]) =>
  createSignatureVerifier({ chainId: 84532, transports: stubs.map((s) => s.transport) });

const TYPED: TypedDataOwnershipProof['typedData'] = {
  domain: { name: 'Platformless AI Vault', version: '1', chainId: 84532 },
  types: {
    VaultRead: [
      { name: 'wallet', type: 'address' },
      { name: 'nonce', type: 'string' },
    ],
  },
  primaryType: 'VaultRead',
  message: { wallet: ADDR_LC, nonce: 'abc123' },
};

describe('signature verifier: message (personal_sign) path', () => {
  it('returns true when the on-chain validator confirms, binding the address and the message hash into the call', async () => {
    const rpc = alwaysValid();
    expect(await verifierOver(rpc)({ address: ADDR, message: 'hello world', signature: SIG })).toBe(true);

    const ethCall = rpc.calls.find((c) => c.method === 'eth_call');
    expect(ethCall).toBeDefined();
    // The deployless verifier is called with (address, messageHash, signature)
    // ABI-encoded into the call data, so the exact address and the exact
    // message hash the server chose MUST appear in it.
    const data = String((ethCall!.params as [{ data: string }])[0].data).toLowerCase();
    expect(data).toContain(ADDR_LC.slice(2));
    expect(data).toContain(hashMessage('hello world').slice(2));
  });

  it('returns false for a wrong-signer signature (validator says invalid)', async () => {
    expect(await verifierOver(alwaysInvalid())({ address: ADDR, message: 'hello world', signature: SIG })).toBe(false);
  });

  it('accepts any casing of the address', async () => {
    for (const address of [ADDR, ADDR_LC, '0x' + ADDR.slice(2).toUpperCase()]) {
      const rpc = alwaysValid();
      expect(await verifierOver(rpc)({ address, message: 'hello world', signature: SIG })).toBe(true);
      const data = String((rpc.calls[0]!.params as [{ data: string }])[0].data).toLowerCase();
      expect(data).toContain(ADDR_LC.slice(2));
    }
  });

  it('THROWS unavailable (never reports invalid, never passes) when the RPC transport fails', async () => {
    await expect(verifierOver(down())({ address: ADDR, message: 'hello world', signature: SIG })).rejects.toBeInstanceOf(
      SignatureCheckUnavailableError
    );
  });
});

describe('signature verifier: typed-data (eth_signTypedData_v4) path', () => {
  it('returns true when the validator confirms a typed-data proof', async () => {
    const rpc = alwaysValid();
    expect(await verifierOver(rpc)({ address: ADDR, typedData: TYPED, signature: SIG })).toBe(true);
    expect(rpc.calls.some((c) => c.method === 'eth_call')).toBe(true);
  });

  it('returns false when the validator rejects a typed-data proof', async () => {
    expect(await verifierOver(alwaysInvalid())({ address: ADDR, typedData: TYPED, signature: SIG })).toBe(false);
  });

  it('throws unavailable when the transport fails', async () => {
    await expect(verifierOver(down())({ address: ADDR, typedData: TYPED, signature: SIG })).rejects.toBeInstanceOf(
      SignatureCheckUnavailableError
    );
  });
});

describe('signature verifier: input validation (before any RPC)', () => {
  it('rejects a malformed address and a malformed signature without touching the transport', async () => {
    const rpc = alwaysValid();
    const verify = verifierOver(rpc);
    await expect(verify({ address: '0xnope', message: 'm', signature: SIG })).rejects.toThrow();
    await expect(verify({ address: ADDR, message: 'm', signature: 'not-hex' })).rejects.toThrow();
    await expect(verify({ address: ADDR, message: 'm', signature: '0xabc' })).rejects.toThrow(); // odd length
    await expect(verify({ address: ADDR, message: 'm', signature: '0x' })).rejects.toThrow();
    expect(rpc.calls.length).toBe(0);
  });

  it('an input error is NOT reported as a transport outage', async () => {
    await expect(verifierOver(alwaysValid())({ address: '0xnope', message: 'm', signature: SIG })).rejects.not.toBeInstanceOf(
      SignatureCheckUnavailableError
    );
  });

  it('an input error never echoes the signature', async () => {
    try {
      await verifierOver(alwaysValid())({ address: ADDR, message: 'm', signature: '0xSECRETSIG' });
      expect.unreachable();
    } catch (e) {
      expect(String((e as Error).message)).not.toContain('SECRETSIG');
    }
  });
});

describe('signature verifier: RPC URLs tried in order', () => {
  it('uses the second RPC when the first fails', async () => {
    const first = down();
    const second = alwaysValid();
    expect(await verifierOver(first, second)({ address: ADDR, message: 'hello world', signature: SIG })).toBe(true);
    expect(first.calls.length).toBeGreaterThan(0);
    expect(second.calls.some((c) => c.method === 'eth_call')).toBe(true);
  });

  it('reports invalid from the second RPC when the first fails', async () => {
    expect(await verifierOver(down(), alwaysInvalid())({ address: ADDR, message: 'hello world', signature: SIG })).toBe(false);
  });

  it('does not ask the second RPC when the first gives a clean answer, valid or invalid', async () => {
    for (const first of [alwaysValid(), alwaysInvalid()]) {
      const second = alwaysValid();
      await verifierOver(first, second)({ address: ADDR, message: 'hello world', signature: SIG });
      expect(second.calls.length).toBe(0);
    }
  });

  it('throws unavailable only when every RPC fails', async () => {
    const first = down();
    const second = down();
    await expect(verifierOver(first, second)({ address: ADDR, message: 'hello world', signature: SIG })).rejects.toBeInstanceOf(
      SignatureCheckUnavailableError
    );
    expect(first.calls.length).toBeGreaterThan(0);
    expect(second.calls.length).toBeGreaterThan(0);
  });
});

describe('signature verifier: chain', () => {
  it('supports Base Sepolia and Base only', () => {
    expect([...SUPPORTED_CHAIN_IDS].sort()).toEqual([8453, 84532]);
    expect(() => createSignatureVerifier({ chainId: 8453, rpcUrls: ['http://127.0.0.1:1'] })).not.toThrow();
    expect(() => createSignatureVerifier({ chainId: 84532, rpcUrls: ['http://127.0.0.1:1'] })).not.toThrow();
    expect(() => createSignatureVerifier({ chainId: 1, rpcUrls: ['http://127.0.0.1:1'] })).toThrow();
    expect(() => createSignatureVerifier({ chainId: 5611, rpcUrls: ['http://127.0.0.1:1'] })).toThrow();
  });

  it('refuses to be built with no RPC at all', () => {
    expect(() => createSignatureVerifier({ chainId: 84532, rpcUrls: [] })).toThrow();
    expect(() => createSignatureVerifier({ chainId: 84532 })).toThrow();
  });
});

describe('signature verifier: "execution reverted" is a verdict only as a JSON-RPC error response (R1 v0.2, round 2)', () => {
  // Over real HTTP: the service's own http transport against a stub node on
  // loopback, because the distinction is between an HTTP-level failure and an
  // error object the node returned in a successful HTTP answer.
  type Answer = { status: number; body: string; contentType?: string; hang?: boolean };
  const servers: http.Server[] = [];
  async function node(answer: Answer): Promise<{ url: string; hits: () => number }> {
    let hits = 0;
    const server = http.createServer((req, res) => {
      const chunks: Buffer[] = [];
      req.on('data', (c: Buffer) => chunks.push(c));
      req.on('end', () => {
        hits++;
        if (answer.hang) return; // never answers: the client times out
        let id: unknown = 1;
        try {
          id = JSON.parse(Buffer.concat(chunks).toString('utf8')).id;
        } catch {
          /* keep 1 */
        }
        res.writeHead(answer.status, { 'content-type': answer.contentType ?? 'application/json' });
        res.end(answer.body.replace('"__ID__"', JSON.stringify(id)));
      });
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    servers.push(server);
    return { url: `http://127.0.0.1:${(server.address() as AddressInfo).port}`, hits: () => hits };
  }
  afterEach(async () => {
    for (const s of servers.splice(0)) {
      s.closeAllConnections();
      await new Promise((r) => s.close(r));
    }
  });

  const rpcError = (code: number, message: string): Answer => ({
    status: 200,
    body: JSON.stringify({ jsonrpc: '2.0', id: '__ID__', error: { code, message } }),
  });
  const rpcResult = (result: string): Answer => ({ status: 200, body: JSON.stringify({ jsonrpc: '2.0', id: '__ID__', result }) });
  const over = (...urls: string[]) => createSignatureVerifier({ chainId: 84532, rpcUrls: urls, timeoutMs: 400 });
  const proof = { address: ADDR, message: 'hello world', signature: SIG };

  it('the stub node is a faithful node: a result of true verifies, a result of false does not', async () => {
    expect(await over((await node(rpcResult(VALID))).url)(proof)).toBe(true);
    expect(await over((await node(rpcResult(INVALID))).url)(proof)).toBe(false);
  });

  it('JSON-RPC error code 3 is invalid', async () => {
    expect(await over((await node(rpcError(3, 'some node wording'))).url)(proof)).toBe(false);
  });

  it('a JSON-RPC error with the phrase in its message is invalid, whatever its code', async () => {
    expect(await over((await node(rpcError(-32000, 'execution reverted'))).url)(proof)).toBe(false);
    expect(await over((await node(rpcError(-32000, 'Execution Reverted: custom error'))).url)(proof)).toBe(false);
    expect(await over((await node(rpcError(3, 'execution reverted'))).url)({ address: ADDR, typedData: TYPED, signature: SIG })).toBe(false);
  });

  it('a revert verdict is final: the second RPC is not asked', async () => {
    const first = await node(rpcError(3, 'execution reverted'));
    const second = await node(rpcResult(VALID));
    expect(await over(first.url, second.url)(proof)).toBe(false);
    expect(second.hits()).toBe(0);
  });

  it('HTTP 502 with the phrase in its body is unavailable, not a verdict', async () => {
    const texts = [
      'Bad Gateway: upstream said execution reverted',
      JSON.stringify({ jsonrpc: '2.0', id: 1, error: { code: 3, message: 'execution reverted' } }),
    ];
    for (const body of texts) {
      const n = await node({ status: 502, body, contentType: 'text/plain' });
      await expect(over(n.url)(proof)).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
      expect(n.hits()).toBeGreaterThan(0);
    }
  });

  it('HTTP 429 with the phrase in its body is unavailable, not a verdict', async () => {
    const n = await node({ status: 429, body: JSON.stringify({ jsonrpc: '2.0', id: 1, error: { code: 3, message: 'execution reverted' } }) });
    await expect(over(n.url)(proof)).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
  });

  it('other HTTP failures, a timeout and a refused connection are unavailable', async () => {
    for (const answer of [
      { status: 500, body: 'execution reverted' },
      { status: 403, body: '{"error":"execution reverted"}' },
      { status: 200, body: '', hang: true },
    ] as Answer[]) {
      await expect(over((await node(answer)).url)(proof)).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
    }
    await expect(over('http://127.0.0.1:1')(proof)).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
  });

  it('other JSON-RPC errors stay unavailable', async () => {
    for (const answer of [rpcError(-32005, 'rate limit exceeded'), rpcError(-32603, 'internal error'), rpcError(-32000, 'header not found')]) {
      await expect(over((await node(answer)).url)(proof)).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
    }
  });

  it('with two RPC URLs, an HTTP failure on the first still fails over to the second, even when its body carries the phrase', async () => {
    const first = await node({ status: 502, body: 'execution reverted', contentType: 'text/plain' });
    const good = await node(rpcResult(VALID));
    expect(await over(first.url, good.url)(proof)).toBe(true);
    expect(first.hits()).toBeGreaterThan(0);
    expect(good.hits()).toBeGreaterThan(0);

    const first2 = await node({ status: 429, body: JSON.stringify({ error: { code: 3, message: 'execution reverted' } }) });
    const reverting = await node(rpcError(3, 'execution reverted'));
    expect(await over(first2.url, reverting.url)(proof)).toBe(false);
    expect(reverting.hits()).toBeGreaterThan(0);
  });

  it('an error that merely looks like a revert but is not a JSON-RPC error response is unavailable', async () => {
    const lookalikes = [
      Object.assign(new Error('execution reverted'), { code: 3 }),
      Object.assign(new Error('wrapped'), { cause: { code: 3, message: 'execution reverted' } }),
      new HttpRequestError({ url: 'http://rpc.invalid', status: 502, details: 'execution reverted', body: { note: 'execution reverted' } }),
    ];
    for (const error of lookalikes) {
      const rpc = stub(async () => {
        throw error;
      });
      await expect(verifierOver(rpc)(proof)).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
      expect(isExecutionReverted(error)).toBe(false);
    }
    const real = new RpcRequestError({ url: 'http://rpc.invalid', body: { method: 'eth_call' }, error: { code: 3, message: 'execution reverted' } });
    expect(isExecutionReverted(real)).toBe(true);
    expect(await verifierOver(stub(async () => { throw real; }))(proof)).toBe(false);
  });
});

describe('signature size (R1 v0.2)', () => {
  it('accepts up to 8 KiB and refuses anything longer, before any RPC call', async () => {
    expect(MAX_SIGNATURE_HEX_CHARS).toBe(16384);
    const atLimit = '0x' + 'ab'.repeat(8192);
    const over = '0x' + 'ab'.repeat(8193);
    expect(isWellFormedSignature(atLimit)).toBe(true);
    expect(isWellFormedSignature(over)).toBe(false);
    const rpc = alwaysValid();
    expect(await verifierOver(rpc)({ address: ADDR, message: 'm', signature: atLimit })).toBe(true);
    const calls = rpc.calls.length;
    await expect(verifierOver(rpc)({ address: ADDR, message: 'm', signature: over })).rejects.toThrow();
    await expect(verifierOver(rpc)({ address: ADDR, message: 'm', signature: over })).rejects.not.toBeInstanceOf(SignatureCheckUnavailableError);
    expect(rpc.calls.length).toBe(calls);
  });
});

describe('withConcurrencyLimit (R1 v0.2)', () => {
  it('runs at most the given number at once and answers "unavailable" beyond it, without calling the verifier', async () => {
    let running = 0;
    let peak = 0;
    let calls = 0;
    const release: Array<() => void> = [];
    const slow: SignatureVerifier = async () => {
      calls++;
      running++;
      peak = Math.max(peak, running);
      await new Promise<void>((r) => release.push(r));
      running--;
      return true;
    };
    const limited = withConcurrencyLimit(slow, 16);
    const proof = { address: ADDR, message: 'm', signature: SIG };
    const first = Array.from({ length: 16 }, () => limited(proof));
    await expect(limited(proof)).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
    await expect(limited(proof)).rejects.toBeInstanceOf(SignatureCheckUnavailableError);
    expect(calls).toBe(16);
    release.splice(0).forEach((r) => r());
    expect(await Promise.all(first)).toEqual(new Array(16).fill(true));
    expect(peak).toBe(16);
    // Slots are free again, also after a verifier that throws.
    const throwing = withConcurrencyLimit(async () => {
      throw new Error('boom');
    }, 1);
    await expect(throwing(proof)).rejects.toThrow('boom');
    await expect(throwing(proof)).rejects.toThrow('boom');
    const again = limited(proof);
    release.splice(0).forEach((r) => r());
    expect(await again).toBe(true);
  });

  it('refuses a nonsense limit', () => {
    expect(() => withConcurrencyLimit(async () => true, 0)).toThrow();
  });
});
