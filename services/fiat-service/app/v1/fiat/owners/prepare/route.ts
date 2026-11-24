// POST /v1/fiat/owners/prepare (OP1.3a, plan D14): the relayer builds the whole
// unsigned user operation for "Add a passkey" or "Remove" and returns it with
// its hash. Body: { action: 'add', publicKey, firstOwnerPublicKey? } or
// { action: 'remove', index, publicKey, firstOwnerPublicKey? }. Logic and
// rules: src/lib/owner-relayer.ts; who may call: src/lib/owner-routes.ts.
import { ownerRoute } from '../../../../../src/lib/owner-routes';

export const dynamic = 'force-dynamic';

export function POST(req: Request): Promise<Response> {
  return ownerRoute(req, 'prepare');
}
