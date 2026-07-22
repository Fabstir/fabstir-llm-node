// FC1.5 — operator issuance of Decision-9 credentials. Guarded by a server-only
// operator token (FIAT_ADMIN_TOKEN): at launch, fiat accounts are provisioned
// by the operator; a self-serve issuance flow needs the site's own login story
// and is out of FC1 scope. The raw credential is returned exactly ONCE — only
// its hash is stored (see fiat-credentials.ts).
import { createHash, timingSafeEqual } from 'node:crypto';
import { getFiatDeps } from '../../../../src/lib/fiat-session-service';
import { fiatUserId } from '../../../../src/lib/fiat-identity';

function operatorAuthorised(req: Request): boolean | undefined {
  const configured = process.env.FIAT_ADMIN_TOKEN;
  if (!configured) return undefined; // endpoint disabled
  const header = req.headers.get('authorization');
  if (!header?.startsWith('Bearer ')) return false;
  const given = createHash('sha256').update(header.slice('Bearer '.length)).digest();
  const expected = createHash('sha256').update(configured).digest();
  return timingSafeEqual(given, expected);
}

export async function POST(req: Request): Promise<Response> {
  const authorised = operatorAuthorised(req);
  if (authorised === undefined) {
    return Response.json(
      { error: 'FIAT_ADMIN_TOKEN is not set — credential issuance is disabled' },
      { status: 503 }
    );
  }
  if (!authorised) return Response.json({ error: 'unauthorised' }, { status: 401 });

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: 'body must be JSON' }, { status: 400 });
  }
  // fc1UserId is the user's smart-account address (fiat-identity.ts), so a
  // credential is bound to the SAME id a card purchase credits.
  let userId: string;
  try {
    userId = fiatUserId((body ?? {}).userId as string);
  } catch {
    return Response.json({ error: 'userId must be the user\'s address' }, { status: 400 });
  }

  const { credentials } = await getFiatDeps();
  // Decision 8: the operator provisions the HELPER's credential (explicit purpose
  // so a later browser 'browser' mint can never evict it).
  const credential = await credentials.issue(userId, 'helper');
  return Response.json({ credential });
}
