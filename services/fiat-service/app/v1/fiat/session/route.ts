// FC1.2 — POST /v1/fiat/session: the fiat session-open endpoint. Thin HTTP
// shell: bearer credential + strict validation, then openFiatSession does the
// gatekeeping and signing. Bigints cross the wire as decimal strings.
import { isAddress } from 'ethers';
import { getFiatSessionService } from '../../../../src/lib/fiat-session-service';

const MODEL_ID_RE = /^0x[0-9a-fA-F]{64}$/;
const DIGITS_RE = /^\d+$/;

export async function POST(req: Request): Promise<Response> {
  const header = req.headers.get('authorization');
  if (!header?.startsWith('Bearer ')) {
    return Response.json({ error: 'missing bearer credential' }, { status: 401 });
  }
  const credential = header.slice('Bearer '.length);

  let body: Record<string, unknown>;
  try {
    body = await req.json();
  } catch {
    return Response.json({ error: 'body must be JSON' }, { status: 400 });
  }
  const { host, modelId, depositMicro, clientAddress } = body ?? {};
  if (typeof host !== 'string' || !isAddress(host)) {
    return Response.json({ error: 'host must be an address' }, { status: 400 });
  }
  if (typeof clientAddress !== 'string' || !isAddress(clientAddress)) {
    return Response.json({ error: 'clientAddress must be an address' }, { status: 400 });
  }
  if (typeof modelId !== 'string' || !MODEL_ID_RE.test(modelId)) {
    return Response.json({ error: 'modelId must be a 32-byte 0x hex id' }, { status: 400 });
  }
  if (typeof depositMicro !== 'string' || !DIGITS_RE.test(depositMicro)) {
    return Response.json(
      { error: 'depositMicro must be a decimal string of USDC micro-units' },
      { status: 400 }
    );
  }

  let service;
  try {
    service = await getFiatSessionService();
  } catch (e) {
    return Response.json(
      { error: e instanceof Error ? e.message : 'fiat backend unavailable' },
      { status: 503 }
    );
  }

  try {
    const outcome = await service.open({
      credential,
      host,
      modelId,
      depositMicro: BigInt(depositMicro),
      clientAddress,
    });
    switch (outcome.status) {
      case 'ok':
        return Response.json({
          sessionId: outcome.sessionId.toString(),
          jobId: outcome.jobId.toString(),
          authorisation: outcome.authorisation,
        });
      case 'unauthorised':
        return Response.json({ error: 'unauthorised' }, { status: 401 });
      case 'refused':
        return Response.json({ error: 'refused', reason: outcome.reason }, { status: 403 });
      case 'chain_error':
        return Response.json({ error: 'chain_error', message: outcome.message }, { status: 502 });
    }
  } catch (e) {
    // A create that succeeded but could not be recorded lands here — loud, not silent.
    return Response.json(
      { error: 'internal', message: e instanceof Error ? e.message : String(e) },
      { status: 500 }
    );
  }
}
