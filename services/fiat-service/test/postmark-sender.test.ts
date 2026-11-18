// The Postmark sender (legal list 1.12): what it sends, when it is off, and
// that neither the token nor a recipient's address can reach an alarm.
import { describe, expect, it, vi } from 'vitest';
import {
  POSTMARK_SEND_URL,
  postmarkSender,
  postmarkSenderFromEnv,
  postmarkSettings,
  withoutAddresses,
  type PostmarkFetcher,
} from '../src/lib/postmark-sender';
import { emailSenderFromEnv, sendPurchaseConfirmation } from '../src/lib/purchase-confirmation';

const TOKEN = 'tok-0000-secret';
const SETTINGS = { token: TOKEN, from: 'Platformless AI <support@fabstir.com>', stream: 'outbound' };
const MESSAGE = { to: 'buyer@example.com', subject: 'Your Platformless AI credits: payment confirmed', text: 'line one\nline two' };

const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
function recording(answer: () => Response | Promise<Response>) {
  const calls: Array<{ url: string; init: Parameters<PostmarkFetcher>[1] }> = [];
  const fetcher: PostmarkFetcher = async (url, init) => {
    calls.push({ url, init });
    return answer();
  };
  return { calls, fetcher };
}

describe('settings from the environment', () => {
  it('no token: the sender is off, whatever else is set', () => {
    expect(postmarkSettings({})).toBeUndefined();
    expect(postmarkSettings({ FIAT_POSTMARK_TOKEN: '  ', FIAT_EMAIL_FROM: 'support@fabstir.com' })).toBeUndefined();
    expect(postmarkSenderFromEnv({ FIAT_EMAIL_FROM: 'support@fabstir.com' })).toBeUndefined();
    expect(emailSenderFromEnv({})).toBeUndefined();
  });

  it('a token with a From address gives a sender on the transactional stream by default', () => {
    expect(postmarkSettings({ FIAT_POSTMARK_TOKEN: TOKEN, FIAT_EMAIL_FROM: ' support@fabstir.com ' })).toEqual({ token: TOKEN, from: 'support@fabstir.com', stream: 'outbound' });
    expect(postmarkSettings({ FIAT_POSTMARK_TOKEN: TOKEN, FIAT_EMAIL_FROM: SETTINGS.from, FIAT_POSTMARK_STREAM: 'orders-1' })).toEqual({ ...SETTINGS, stream: 'orders-1' });
    expect(emailSenderFromEnv({ FIAT_POSTMARK_TOKEN: TOKEN, FIAT_EMAIL_FROM: SETTINGS.from })).toBeDefined();
  });

  it('a token with an unusable From or stream sends nothing and says why, without the token', () => {
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      for (const env of [
        { FIAT_POSTMARK_TOKEN: TOKEN },
        { FIAT_POSTMARK_TOKEN: TOKEN, FIAT_EMAIL_FROM: 'not an address' },
        { FIAT_POSTMARK_TOKEN: TOKEN, FIAT_EMAIL_FROM: 'a@b.com\nBcc: x@y.com' },
        { FIAT_POSTMARK_TOKEN: TOKEN, FIAT_EMAIL_FROM: 'support@fabstir.com', FIAT_POSTMARK_STREAM: 'Bad Stream' },
      ]) {
        expect(postmarkSettings(env)).toHaveProperty('problem');
        expect(postmarkSenderFromEnv(env)).toBeUndefined();
      }
      expect(err).toHaveBeenCalledTimes(1); // said once, not on every webhook
      expect(String(err.mock.calls[0]![0])).toMatch(/ALARM: no email will be sent/);
      expect(JSON.stringify(err.mock.calls)).not.toContain(TOKEN);
    } finally {
      err.mockRestore();
    }
  });
});

describe('sending', () => {
  it('posts one plain-text message to Postmark with the token in its header only', async () => {
    const { calls, fetcher } = recording(() => json(200, { ErrorCode: 0, Message: 'OK', MessageID: 'm-1' }));
    await postmarkSender(SETTINGS, fetcher).send(MESSAGE);
    expect(calls).toHaveLength(1);
    const { url, init } = calls[0]!;
    expect(url).toBe(POSTMARK_SEND_URL);
    expect(url).toBe('https://api.postmarkapp.com/email');
    expect(init.method).toBe('POST');
    expect(init.redirect).toBe('error');
    expect(init.cache).toBe('no-store');
    expect(init.signal).toBeInstanceOf(AbortSignal);
    expect(init.headers).toEqual({ Accept: 'application/json', 'Content-Type': 'application/json', 'X-Postmark-Server-Token': TOKEN });
    expect(JSON.parse(init.body)).toEqual({
      From: 'Platformless AI <support@fabstir.com>',
      To: 'buyer@example.com',
      Subject: MESSAGE.subject,
      TextBody: 'line one\nline two',
      MessageStream: 'outbound',
    });
    expect(init.body).not.toContain(TOKEN);
  });

  it('a refusal by Postmark throws with its code, and never with the address or the token', async () => {
    const { fetcher } = recording(() => json(422, { ErrorCode: 406, Message: 'You tried to send to recipient(s) that have been marked as inactive. Found inactive addresses: buyer@example.com.' }));
    const failure = await postmarkSender(SETTINGS, fetcher).send(MESSAGE).then(() => undefined, (e: Error) => e);
    expect(failure).toBeInstanceOf(Error);
    expect(failure!.message).toMatch(/Postmark refused the message \(HTTP 422 code 406\)/);
    expect(failure!.message).toContain('[address]');
    expect(failure!.message).not.toContain('buyer@example.com');
    expect(failure!.message).not.toContain(TOKEN);
  });

  it('HTTP 200 is not enough: an error code in the body is a failure', async () => {
    const { fetcher } = recording(() => json(200, { ErrorCode: 300, Message: 'Invalid email request' }));
    await expect(postmarkSender(SETTINGS, fetcher).send(MESSAGE)).rejects.toThrow(/HTTP 200 code 300/);
  });

  it('a wrong token (401) and a body that is not JSON are failures', async () => {
    await expect(postmarkSender(SETTINGS, recording(() => json(401, { ErrorCode: 10, Message: 'Bad or missing Server API token.' })).fetcher).send(MESSAGE)).rejects.toThrow(/HTTP 401 code 10/);
    await expect(postmarkSender(SETTINGS, recording(() => new Response('<html>bad gateway</html>', { status: 502 })).fetcher).send(MESSAGE)).rejects.toThrow(/HTTP 502\)/);
  });

  it('a timeout or a network fault is named without leaking the request', async () => {
    const timeout: PostmarkFetcher = async () => {
      throw Object.assign(new Error(`aborted while sending to ${MESSAGE.to} with ${TOKEN}`), { name: 'TimeoutError' });
    };
    const t = await postmarkSender(SETTINGS, timeout).send(MESSAGE).then(() => undefined, (e: Error) => e);
    expect(t!.message).toMatch(/Postmark did not answer in 8000 ms/);
    const down: PostmarkFetcher = async () => {
      throw new TypeError(`fetch failed for ${MESSAGE.to}`);
    };
    const d = await postmarkSender(SETTINGS, down).send(MESSAGE).then(() => undefined, (e: Error) => e);
    expect(d!.message).toBe('Postmark could not be reached (TypeError)');
    for (const m of [t!.message, d!.message]) {
      expect(m).not.toContain(MESSAGE.to);
      expect(m).not.toContain(TOKEN);
    }
  });

  it('withoutAddresses takes out every address and leaves the rest', () => {
    expect(withoutAddresses('Found inactive addresses: a.b+c@example.co.uk, d@e.org. Try again')).toBe('Found inactive addresses: [address], [address] Try again');
    expect(withoutAddresses('nothing to hide')).toBe('nothing to hide');
  });
});

describe('through the confirmation path', () => {
  const purchase = { to: 'buyer@example.com', accountId: '0xabc', credits: 500, netCents: 432, taxCents: 86, grossCents: 518, currency: 'gbp', purchasedAtMs: Date.UTC(2026, 10, 18), termsVersion: '2026-11-14.1', useNow: true };
  const terms = { text: async () => 'TERMS (test)\nSCHEDULE 3: MODEL CANCELLATION FORM' };

  it('a credited purchase is confirmed through Postmark, with the terms in the text', async () => {
    const { calls, fetcher } = recording(() => json(200, { ErrorCode: 0 }));
    const alarms: string[] = [];
    const outcome = await sendPurchaseConfirmation({ sender: postmarkSender(SETTINGS, fetcher), terms, onAlarm: (m) => alarms.push(m) }, purchase);
    expect(outcome).toBe('sent');
    expect(alarms).toEqual([]);
    const body = JSON.parse(calls[0]!.init.body) as { To: string; TextBody: string };
    expect(body.To).toBe('buyer@example.com');
    expect(body.TextBody).toContain('Total paid: £5.18');
    expect(body.TextBody).toContain('SCHEDULE 3: MODEL CANCELLATION FORM');
  });

  it('a refused send is alarmed without the address, and the purchase path does not throw', async () => {
    const { fetcher } = recording(() => json(422, { ErrorCode: 406, Message: 'Found inactive addresses: buyer@example.com.' }));
    const alarms: string[] = [];
    const outcome = await sendPurchaseConfirmation({ sender: postmarkSender(SETTINGS, fetcher), terms, onAlarm: (m) => alarms.push(m) }, purchase);
    expect(outcome).toBe('failed');
    expect(alarms).toHaveLength(1);
    expect(alarms[0]).toMatch(/confirmation email failed — Postmark refused the message \(HTTP 422 code 406\)/);
    expect(alarms[0]).not.toContain('buyer@example.com');
  });
});
