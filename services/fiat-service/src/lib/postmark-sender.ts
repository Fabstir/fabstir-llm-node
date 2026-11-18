// The email delivery provider (legal list 1.12, chosen 2026-11-18): Postmark.
// One plain-text message per call to Postmark's send API, on a transactional
// message stream. Built from the environment and INERT unless
// FIAT_POSTMARK_TOKEN is set: without it `postmarkSenderFromEnv` returns
// undefined and nothing is sent, exactly as before a provider existed.
//
// Two things never reach a log or an alarm from here: the server token, and a
// recipient's address (Postmark's own error text can quote it, so addresses in
// an error are replaced before the error is thrown).
import type { EmailMessage, EmailSender } from './purchase-confirmation';

export const POSTMARK_SEND_URL = 'https://api.postmarkapp.com/email';
/** The send sits inside the Stripe webhook: bounded well under Stripe's wait. */
export const POSTMARK_TIMEOUT_MS = 8_000;
const DEFAULT_STREAM = 'outbound';
const STREAM_RE = /^[a-z0-9][a-z0-9-]{0,62}$/;
/** `name@domain` alone, or `Display Name <name@domain>`; one line. */
const FROM_RE = /^(?:[^\s<>@"]+@[^\s<>@"]+\.[^\s<>@"]+|[^<>\r\n"]{1,80} <[^\s<>@"]+@[^\s<>@"]+\.[^\s<>@"]+>)$/;
const ADDRESS_RE = /[^\s<>@"',;:]+@[^\s<>@"',;:]+/g;

export type PostmarkFetcher = (
  url: string,
  init: { method: 'POST'; headers: Record<string, string>; body: string; signal: AbortSignal; redirect: 'error'; cache: 'no-store' }
) => Promise<Response>;

/** Error text with every email address taken out. */
export function withoutAddresses(text: string): string {
  return text.replace(ADDRESS_RE, '[address]');
}

export interface PostmarkSettings {
  token: string;
  from: string;
  stream: string;
}

/** The settings, `undefined` when no token is set (the sender is off), or a
 *  `problem` when the token is set but the rest cannot be used. */
export function postmarkSettings(env: Record<string, string | undefined> = process.env): PostmarkSettings | undefined | { problem: string } {
  const token = env.FIAT_POSTMARK_TOKEN?.trim();
  if (!token) return undefined;
  const from = env.FIAT_EMAIL_FROM?.trim();
  if (!from) return { problem: 'FIAT_POSTMARK_TOKEN is set but FIAT_EMAIL_FROM is not' };
  if (!FROM_RE.test(from)) return { problem: 'FIAT_EMAIL_FROM must be an address, or a name followed by an address in angle brackets' };
  const stream = env.FIAT_POSTMARK_STREAM?.trim() || DEFAULT_STREAM;
  if (!STREAM_RE.test(stream)) return { problem: 'FIAT_POSTMARK_STREAM is not a message stream id' };
  return { token, from, stream };
}

export function postmarkSender(settings: PostmarkSettings, fetcher: PostmarkFetcher = fetch as unknown as PostmarkFetcher): EmailSender {
  return {
    async send(message: EmailMessage): Promise<void> {
      let res: Response;
      try {
        res = await fetcher(POSTMARK_SEND_URL, {
          method: 'POST',
          headers: {
            Accept: 'application/json',
            'Content-Type': 'application/json',
            'X-Postmark-Server-Token': settings.token,
          },
          body: JSON.stringify({
            From: settings.from,
            To: message.to,
            Subject: message.subject,
            TextBody: message.text,
            MessageStream: settings.stream,
          }),
          signal: AbortSignal.timeout(POSTMARK_TIMEOUT_MS),
          redirect: 'error',
          cache: 'no-store',
        });
      } catch (e) {
        const name = e instanceof Error ? e.name : 'Error';
        throw new Error(name === 'TimeoutError' || name === 'AbortError' ? `Postmark did not answer in ${POSTMARK_TIMEOUT_MS} ms` : `Postmark could not be reached (${name})`);
      }
      let body: { ErrorCode?: unknown; Message?: unknown } = {};
      try {
        body = (await res.json()) as typeof body;
      } catch {
        // A body that is not JSON: judged by the status alone, below.
      }
      if (res.status === 200 && body.ErrorCode === 0) return;
      const code = typeof body.ErrorCode === 'number' ? ` code ${body.ErrorCode}` : '';
      const why = typeof body.Message === 'string' ? `: ${withoutAddresses(body.Message).slice(0, 200)}` : '';
      throw new Error(`Postmark refused the message (HTTP ${res.status}${code})${why}`);
    },
  };
}

let warned = false;
/** The production sender, or undefined when none is configured. A token with
 *  unusable settings sends nothing and says so once in the journal. */
export function postmarkSenderFromEnv(env: Record<string, string | undefined> = process.env): EmailSender | undefined {
  const settings = postmarkSettings(env);
  if (!settings) return undefined;
  if ('problem' in settings) {
    if (!warned) {
      warned = true;
      console.error(`[fiat-email] ALARM: no email will be sent — ${settings.problem}`);
    }
    return undefined;
  }
  return postmarkSender(settings);
}
