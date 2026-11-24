// POST /v1/fiat/owners/submit (OP1.3a, plan D14): body { userOpHash, signature }.
// The relayer re-checks the prepared operation against the chain, simulates it,
// deposits the prefund and sends handleOps; answers { txHash, ownerIndex }.
import { ownerRoute } from '../../../../../src/lib/owner-routes';

export const dynamic = 'force-dynamic';

export function POST(req: Request): Promise<Response> {
  return ownerRoute(req, 'submit');
}
