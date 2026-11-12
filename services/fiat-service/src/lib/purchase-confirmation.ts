// Legal list 2.5 (register B2, B5) — our own confirmation of each purchase, by
// email, carrying the terms the buyer accepted and the cancellation form IN
// ITS TEXT (a link to a web page is not a durable medium), and the buyer's
// request to use the credits straight away when they made one.
//
// Two things are deliberately NOT here, because they are not decided:
//   - the email delivery provider (legal list 1.12). `emailSenderFromEnv`
//     returns undefined until one is wired in, and the webhook then logs that
//     the confirmation was not sent. Nothing else changes when one is added:
//     implement `EmailSender` and return it from `emailSenderFromEnv`.
//   - the final text of the Terms (with the solicitors). The text is read at
//     send time from FIAT_TERMS_DIR/<version>.txt, one plain-text file per
//     Terms version, so no release is needed when the Terms change.
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { SELLER } from './seller';

export interface PurchaseConfirmation {
  to: string;
  /** The account the credits were added to. */
  accountId: string;
  /** Price before VAT, the VAT, and the total, in cents. */
  netCents: number;
  taxCents: number;
  grossCents: number;
  /** ISO currency code, lower case, as Stripe reports it. */
  currency: string;
  purchasedAtMs: number;
  paymentIntentId?: string;
  termsVersion: string;
  /** The buyer ticked "I want to use my credits straight away". */
  useNow: boolean;
}

export interface EmailMessage {
  to: string;
  subject: string;
  text: string;
}

export interface EmailSender {
  send(message: EmailMessage): Promise<void>;
}

export interface TermsSource {
  /** The full plain text of the Terms at `version`, or undefined if not held. */
  text(version: string): Promise<string | undefined>;
}

const MONTHS = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
];

function money(cents: number, currency: string): string {
  const whole = Math.floor(cents / 100).toLocaleString('en-GB');
  const pence = String(cents % 100).padStart(2, '0');
  return currency === 'usd' ? `$${whole}.${pence}` : `${whole}.${pence} ${currency.toUpperCase()}`;
}

function longDate(ms: number): string {
  const d = new Date(ms);
  return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]} ${d.getUTCFullYear()}`;
}

/** The confirmation, as plain text. Pure, so the wording is tested. */
export function buildConfirmationEmail(p: PurchaseConfirmation, termsText: string): EmailMessage {
  const credits = p.netCents.toLocaleString('en-GB');
  const lines = [
    'Thank you for your purchase.',
    '',
    `This email confirms your contract with ${SELLER.name} for the credits below.`,
    '',
    `Credits bought: ${credits}`,
    `Price before VAT: ${money(p.netCents, p.currency)}`,
    `VAT: ${money(p.taxCents, p.currency)}`,
    `Total paid: ${money(p.grossCents, p.currency)}${p.currency === 'usd' ? ' (US dollars)' : ''}`,
    `Date: ${longDate(p.purchasedAtMs)}`,
    ...(p.paymentIntentId ? [`Payment reference: ${p.paymentIntentId}`] : []),
    `Account reference: ${p.accountId}`,
    '',
    'Stripe, our payment provider, sends you a VAT invoice for this payment separately.',
    '',
  ];
  if (p.useNow) {
    lines.push(
      'You asked to use your credits straight away. Once a clip has been delivered to you, you cannot cancel the payment for that clip. Unused credits stay refundable.',
      ''
    );
  }
  lines.push(
    'Your right to cancel, and a cancellation form you can use, are set out in the terms below.',
    `To cancel, or for anything else, email ${SELLER.contactEmail}.`,
    '',
    `The terms you accepted (version ${p.termsVersion}) follow in full.`,
    '',
    '----------------------------------------',
    termsText.trim(),
    '----------------------------------------',
    '',
    `${SELLER.name}, company number ${SELLER.companyNumber}, VAT number ${SELLER.vatNumber}.`,
    `Registered office: ${SELLER.registeredOffice}.`,
    SELLER.contactEmail
  );
  return {
    to: p.to,
    subject: 'Your Platformless AI credits: payment confirmed',
    text: lines.join('\n'),
  };
}

const TERMS_FILE_RE = /^[A-Za-z0-9._:-]{1,40}$/;

/** Terms texts from FIAT_TERMS_DIR, one `<version>.txt` per version. */
export function termsSourceFromEnv(): TermsSource | undefined {
  const dir = process.env.FIAT_TERMS_DIR;
  if (!dir) return undefined;
  return {
    async text(version: string): Promise<string | undefined> {
      // The version came from request metadata: never let it name a path.
      if (!TERMS_FILE_RE.test(version) || version.includes('..')) return undefined;
      try {
        const text = await readFile(join(dir, `${version}.txt`), 'utf8');
        return text.trim().length > 0 ? text : undefined;
      } catch (e) {
        if ((e as NodeJS.ErrnoException).code === 'ENOENT') return undefined;
        throw e;
      }
    },
  };
}

/** No delivery provider is chosen yet (legal list 1.12): undefined. */
export function emailSenderFromEnv(): EmailSender | undefined {
  return undefined;
}

export type ConfirmationOutcome = 'sent' | 'no_sender' | 'no_address' | 'no_consent' | 'no_terms_text' | 'failed';

/** Send the confirmation for one credited purchase. Never throws: a purchase
 *  is credited whether or not the email goes; what did not go is alarmed. */
export async function sendPurchaseConfirmation(
  deps: { sender?: EmailSender; terms?: TermsSource; onAlarm: (message: string) => void },
  purchase: Omit<PurchaseConfirmation, 'to' | 'termsVersion' | 'useNow'> & {
    to?: string;
    termsVersion?: string;
    useNow?: boolean;
  }
): Promise<ConfirmationOutcome> {
  const ref = purchase.paymentIntentId ?? purchase.accountId;
  if (!deps.sender) return 'no_sender';
  if (!purchase.to) {
    deps.onAlarm(`purchase ${ref}: no confirmation email sent — Stripe gave no customer email`);
    return 'no_address';
  }
  if (!purchase.termsVersion) {
    deps.onAlarm(`purchase ${ref}: no confirmation email sent — the purchase recorded no Terms version`);
    return 'no_consent';
  }
  try {
    const termsText = await deps.terms?.text(purchase.termsVersion);
    if (!termsText) {
      deps.onAlarm(
        `purchase ${ref}: no confirmation email sent — no Terms text for version ${purchase.termsVersion} (FIAT_TERMS_DIR/<version>.txt)`
      );
      return 'no_terms_text';
    }
    await deps.sender.send(
      buildConfirmationEmail(
        { ...purchase, to: purchase.to, termsVersion: purchase.termsVersion, useNow: purchase.useNow === true },
        termsText
      )
    );
    return 'sent';
  } catch (e) {
    deps.onAlarm(`purchase ${ref}: confirmation email failed — ${e instanceof Error ? e.message : String(e)}`);
    return 'failed';
  }
}

// ---------------------------------------------------------------------------
// A purchase that was REFUSED and refunded (billing address outside the area
// we sell to, or a payment that could not be credited): tell the buyer, by the
// same send rules. Not sent until a provider is wired in.
// ---------------------------------------------------------------------------

export interface PurchaseRefusal {
  to: string;
  grossCents: number;
  currency: string;
  /** `country` = billing address outside the area we sell to. */
  reason: string;
  /** How the pages name the area we sell to, e.g. "the United Kingdom". */
  salesArea: string;
  paymentIntentId?: string;
}

export function buildRefusalEmail(p: PurchaseRefusal): EmailMessage {
  const lines = [
    `We could not complete your purchase of Platformless AI credits, so no credits were added and your payment of ${money(p.grossCents, p.currency)} has been refunded in full to your card.`,
    'Your bank may take a few days to show the refund.',
    '',
  ];
  if (p.reason === 'country') {
    lines.push(`We sell only to customers whose billing address is in ${p.salesArea}.`, '');
  }
  lines.push(
    `If you think this is a mistake, email ${SELLER.contactEmail}.`,
    ...(p.paymentIntentId ? ['', `Payment reference: ${p.paymentIntentId}`] : []),
    '',
    `${SELLER.name}, company number ${SELLER.companyNumber}, VAT number ${SELLER.vatNumber}.`,
    `Registered office: ${SELLER.registeredOffice}.`,
    SELLER.contactEmail
  );
  return { to: p.to, subject: 'Your Platformless AI payment has been refunded', text: lines.join('\n') };
}

/** Never throws; a refusal is journaled and refunded whether or not this goes. */
export async function sendRefusalNotice(
  deps: { sender?: EmailSender; onAlarm: (message: string) => void },
  refusal: Omit<PurchaseRefusal, 'to'> & { to?: string }
): Promise<'sent' | 'no_sender' | 'no_address' | 'failed'> {
  if (!deps.sender) return 'no_sender';
  if (!refusal.to) return 'no_address';
  try {
    await deps.sender.send(buildRefusalEmail({ ...refusal, to: refusal.to }));
    return 'sent';
  } catch (e) {
    deps.onAlarm(
      `refused purchase ${refusal.paymentIntentId ?? ''}: the refund notice email failed — ${e instanceof Error ? e.message : String(e)}`
    );
    return 'failed';
  }
}
