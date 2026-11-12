// Legal list 2.5 — our own confirmation email for each purchase: it carries
// the terms the buyer accepted and the cancellation form in its TEXT, and the
// buyer's request to use the credits straight away when they made one. The
// delivery provider is not chosen yet, so the sender here is a fake; the
// wording and the when-it-is-sent rules are what is under test.
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  buildConfirmationEmail,
  emailSenderFromEnv,
  sendPurchaseConfirmation,
  termsSourceFromEnv,
  type EmailMessage,
} from '../src/lib/purchase-confirmation';
import { CreditsLedger, MemoryLedgerStore } from '../src/lib/ledger';
import { handleStripeEvent, type StripeWebhookDeps } from '../src/lib/stripe-webhook';
import { allowedBillingCountries } from '../src/lib/sales-countries';
import { fakeStripeApi } from './helpers/fake-stripe';
import { USER, checkoutCompleted } from './fixtures/stripe-events';

const TERMS = [
  'TERMS OF SERVICE (test text)',
  '7. Your right to cancel',
  'SCHEDULE 3: MODEL CANCELLATION FORM',
  'I hereby give notice that I cancel my contract for the supply of the following service: Platformless AI credits.',
].join('\n');

const PURCHASE = {
  to: 'buyer@example.com',
  accountId: USER,
  netCents: 1000,
  taxCents: 200,
  grossCents: 1200,
  currency: 'usd',
  purchasedAtMs: Date.UTC(2026, 10, 12, 9, 30),
  paymentIntentId: 'pi_1',
  termsVersion: '2026-10-23.1',
  useNow: true,
};

describe('buildConfirmationEmail', () => {
  it('states the seller, the amounts with VAT, and carries the whole terms text with the cancellation form', () => {
    const mail = buildConfirmationEmail(PURCHASE, TERMS);
    expect(mail.to).toBe('buyer@example.com');
    expect(mail.subject).toBe('Your Platformless AI credits: payment confirmed');
    expect(mail.text).toContain('Fabstir Services Limited');
    expect(mail.text).toContain('company number 17451342');
    expect(mail.text).toContain('VAT number GB530097903');
    expect(mail.text).toContain('Credits bought: 1,000');
    expect(mail.text).toContain('Price before VAT: $10.00');
    expect(mail.text).toContain('VAT: $2.00');
    expect(mail.text).toContain('Total paid: $12.00 (US dollars)');
    expect(mail.text).toContain('Date: 12 November 2026');
    expect(mail.text).toContain('version 2026-10-23.1');
    // The terms themselves, not a link to them.
    expect(mail.text).toContain(TERMS);
    expect(mail.text).toContain('MODEL CANCELLATION FORM');
    expect(mail.text).not.toMatch(/https?:\/\//);
  });

  it('records the request to use the credits straight away only when it was made', () => {
    expect(buildConfirmationEmail(PURCHASE, TERMS).text).toContain('You asked to use your credits straight away.');
    expect(buildConfirmationEmail({ ...PURCHASE, useNow: false }, TERMS).text).not.toContain('straight away');
  });

  it('is plain UK English for a card customer: no dash used as a pause, nothing about wallets or crypto', () => {
    const { text, subject } = buildConfirmationEmail(PURCHASE, TERMS);
    const ours = `${subject}\n${text.replace(TERMS, '')}`;
    expect(ours).not.toMatch(/[—–]| - /);
    expect(ours).not.toMatch(/wallet|USDC|crypto|blockchain/i);
  });
});

describe('sendPurchaseConfirmation', () => {
  const terms = { text: async (v: string) => (v === '2026-10-23.1' ? TERMS : undefined) };

  it('sends through the provider and says what it could not send, never throwing', async () => {
    const sent: EmailMessage[] = [];
    const alarms: string[] = [];
    const deps = { sender: { send: async (m: EmailMessage) => void sent.push(m) }, terms, onAlarm: (m: string) => alarms.push(m) };
    expect(await sendPurchaseConfirmation(deps, PURCHASE)).toBe('sent');
    expect(sent).toHaveLength(1);

    expect(await sendPurchaseConfirmation(deps, { ...PURCHASE, to: undefined })).toBe('no_address');
    expect(await sendPurchaseConfirmation(deps, { ...PURCHASE, termsVersion: undefined })).toBe('no_consent');
    expect(await sendPurchaseConfirmation(deps, { ...PURCHASE, termsVersion: '1999-01-01' })).toBe('no_terms_text');
    const failing = { ...deps, sender: { send: async () => Promise.reject(new Error('provider down')) } };
    expect(await sendPurchaseConfirmation(failing, PURCHASE)).toBe('failed');
    expect(sent).toHaveLength(1);
    expect(alarms).toHaveLength(4);
    expect(alarms.join('\n')).toMatch(/provider down/);
  });

  it('with no provider configured nothing is sent (the provider is not chosen yet)', async () => {
    expect(emailSenderFromEnv()).toBeUndefined();
    expect(await sendPurchaseConfirmation({ terms, onAlarm: () => {} }, PURCHASE)).toBe('no_sender');
  });
});

describe('termsSourceFromEnv', () => {
  afterEach(() => {
    delete process.env.FIAT_TERMS_DIR;
  });

  it('reads <version>.txt from FIAT_TERMS_DIR and never lets a version name a path', async () => {
    expect(termsSourceFromEnv()).toBeUndefined();
    const dir = await mkdtemp(join(tmpdir(), 'fiat-terms-'));
    await writeFile(join(dir, '2026-10-23.1.txt'), TERMS);
    process.env.FIAT_TERMS_DIR = dir;
    const source = termsSourceFromEnv()!;
    expect(await source.text('2026-10-23.1')).toBe(TERMS);
    expect(await source.text('2027-01-01.1')).toBeUndefined();
    expect(await source.text('../2026-10-23.1')).toBeUndefined();
    expect(await source.text('..')).toBeUndefined();
  });
});

describe('the webhook sends the confirmation once per credited purchase', () => {
  async function harness() {
    const ledger = await CreditsLedger.open(new MemoryLedgerStore());
    const sent: EmailMessage[] = [];
    const deps: StripeWebhookDeps = {
      ledger,
      stripe: fakeStripeApi().api,
      onAlarm: () => {},
      allowedCountries: allowedBillingCountries({}),
      email: {
        sender: { send: async (m) => void sent.push(m) },
        terms: { text: async () => TERMS },
      },
      log: () => {},
    };
    return { deps, ledger, sent };
  }

  it('to the email Stripe collected, with the amounts of that purchase; not again on a replay', async () => {
    const h = await harness();
    const event = checkoutCompleted({ credits: 2500, taxCents: 500, email: 'film@example.com', consent: { useNow: '0' } });
    await handleStripeEvent(h.deps, event);
    await handleStripeEvent(h.deps, event);
    expect(h.sent).toHaveLength(1);
    expect(h.sent[0]!.to).toBe('film@example.com');
    expect(h.sent[0]!.text).toContain('Credits bought: 2,500');
    expect(h.sent[0]!.text).toContain('Total paid: $30.00');
    expect(h.sent[0]!.text).not.toContain('straight away');
  });

  it('a refused purchase gets no confirmation (there is no contract) but is told it was refunded in full', async () => {
    const h = await harness();
    await handleStripeEvent(h.deps, checkoutCompleted({ country: 'US', taxCents: 0, credits: 1000, email: 'abroad@example.com' }));
    expect(h.sent).toHaveLength(1);
    const [mail] = h.sent;
    expect(mail!.to).toBe('abroad@example.com');
    expect(mail!.subject).toBe('Your Platformless AI payment has been refunded');
    expect(mail!.text).toContain('no credits were added and your payment of $10.00 has been refunded in full');
    expect(mail!.text).toContain('billing address is in the United Kingdom.');
    expect(mail!.text).not.toContain('confirms your contract');
    expect(mail!.text).not.toMatch(/[—–]| - /);
    // Replayed: told once.
    expect(h.sent).toHaveLength(1);
  });

  it('a provider failure never stops the credit', async () => {
    const h = await harness();
    h.deps.email = { sender: { send: async () => Promise.reject(new Error('down')) }, terms: { text: async () => TERMS } };
    const res = await handleStripeEvent(h.deps, checkoutCompleted({}));
    expect(res.status).toBe(200);
    expect(h.ledger.availableMicro(USER)).toBe(10_000_000n);
  });
});
