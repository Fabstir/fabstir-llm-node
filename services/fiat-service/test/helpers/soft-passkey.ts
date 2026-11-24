// A software P-256 passkey (node:crypto only) that answers navigator.credentials.get
// as a browser authenticator does, for viem's toWebAuthnAccount. Made-up keys only.
import { createHash, generateKeyPairSync, randomBytes, sign as ecdsaSign, type KeyObject } from 'node:crypto';
import type { Hex } from 'viem';

const b64url = (b: Uint8Array) => Buffer.from(b).toString('base64url');
const sha256 = (b: Uint8Array) => new Uint8Array(createHash('sha256').update(b).digest());

export type SoftPasskey = {
  publicKey: Hex;
  credentialId: string;
  getFn: (options: any) => Promise<any>;
  signCount: () => number;
};

export function softPasskey(origin = 'http://localhost:3000'): SoftPasskey {
  const { privateKey, publicKey } = generateKeyPairSync('ec', { namedCurve: 'P-256' });
  const jwk = publicKey.export({ format: 'jwk' }) as { x: string; y: string };
  const hexKey = ('0x' + Buffer.from(jwk.x, 'base64url').toString('hex') + Buffer.from(jwk.y, 'base64url').toString('hex')) as Hex;
  const rawId = randomBytes(16);
  const credentialId = b64url(rawId);
  let count = 0;
  const key: KeyObject = privateKey;
  const getFn = async (options: any) => {
    count++;
    const pk = options.publicKey;
    const challenge = new Uint8Array(pk.challenge);
    const rpHash = sha256(new TextEncoder().encode(pk.rpId));
    const authenticatorData = new Uint8Array([...rpHash, 0x05, 0, 0, 0, 1]);
    const clientDataJSON = new TextEncoder().encode(
      JSON.stringify({ type: 'webauthn.get', challenge: b64url(challenge), origin, crossOrigin: false })
    );
    const payload = new Uint8Array([...authenticatorData, ...sha256(clientDataJSON)]);
    const der = new Uint8Array(ecdsaSign('sha256', payload, key));
    const buf = (u: Uint8Array) => u.slice().buffer;
    return {
      id: credentialId,
      rawId: buf(rawId),
      type: 'public-key',
      response: {
        authenticatorData: buf(authenticatorData),
        clientDataJSON: buf(clientDataJSON),
        signature: buf(der),
        userHandle: null,
      },
    };
  };
  return { publicKey: hexKey, credentialId, getFn, signCount: () => count };
}
