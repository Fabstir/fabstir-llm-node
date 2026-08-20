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
  // FC2.8: the retry key travels as a header (Stripe's convention) or in the
  // body, whichever suits the caller. Bounded so a key cannot be used as a
  // storage channel; absent = today's behaviour, no dedupe.
  const rawKey = req.headers.get('idempotency-key') ?? (body as { idempotencyKey?: unknown })?.idempotencyKey;
  if (rawKey !== undefined && rawKey !== null) {
    if (typeof rawKey !== 'string' || rawKey.length === 0 || rawKey.length > 200) {
      return Response.json(
        { error: 'idempotencyKey must be a string of 1..200 characters' },
        { status: 400 }
      );
    }
  }
  const idempotencyKey = typeof rawKey === 'string' ? rawKey : undefined;
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
      ...(idempotencyKey ? { idempotencyKey } : {}),
    });
    switch (outcome.status) {
      case 'ok':
        return Response.json(
          {
            sessionId: outcome.sessionId.toString(),
            jobId: outcome.jobId.toString(),
            authorisation: outcome.authorisation,
            // Tells the caller this was a replay of an earlier attempt, not a
            // new escrow — useful in their logs, ignorable in their code.
            ...(outcome.replayed ? { replayed: true } : {}),
          },
          { headers: idempotencyKey ? { 'idempotency-replayed': outcome.replayed ? 'true' : 'false' } : {} }
        );
      case 'in_flight':
        // 409 + Retry-After: an identical attempt is mid-flight and may already
        // have escrowed. Waiting is the only answer that cannot double-charge.
        return Response.json(
          { error: 'in_flight', message: 'an attempt with this idempotency key is still in progress' },
          { status: 409, headers: { 'retry-after': '5' } }
        );
      case 'key_conflict':
        return Response.json(
          {
            error: 'key_conflict',
            message: 'this idempotency key was first used with different session parameters',
          },
          { status: 422 }
        );
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
