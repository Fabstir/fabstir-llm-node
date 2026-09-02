// FC1.1 — the gatekeeper is the security boundary (Decision 3): pure policy
// over a ledger snapshot. Refusal order is deterministic and every refusal is
// a stable machine-readable reason. Config comes from server env; the DEFAULT
// allow-list is EMPTY, so an unconfigured deployment refuses every fiat open.
import { afterEach, describe, expect, it } from 'vitest';
import {
  capsFor,
  describeGatekeeperConfig,
  gatekeeperConfigFromEnv,
  isKindEnabled,
  largestSessionCapMicro,
  makeGatekeeper,
  type GatekeeperConfig,
  type LedgerView,
} from '../src/lib/gatekeeper';

const HOST = '0xAbCd000000000000000000000000000000000001';

const CONFIG: GatekeeperConfig = {
  allowedHosts: [HOST],
  maxDepositPerSessionMicro: 2_000_000n,
  maxDailySpendMicro: 10_000_000n,
  maxOpensPerMinute: 3,
};

function view(overrides: Partial<LedgerView> = {}): LedgerView {
  return {
    availableMicro: 5_000_000n,
    spentInWindowMicro: 0n,
    opensInWindow: 0,
    ...overrides,
  };
}

describe('gatekeeper decisions', () => {
  const gate = makeGatekeeper(CONFIG);

  it('allows a clean request', () => {
    expect(gate(view(), { host: HOST, depositMicro: 500_000n })).toEqual({ allow: true });
  });

  it('checks the allow-list before anything else (no balance probing off-list)', () => {
    const decision = gate(view({ availableMicro: 0n }), { host: '0x9999999999999999999999999999999999999999', depositMicro: 500_000n });
    expect(decision).toEqual({ allow: false, reason: 'HOST_NOT_ALLOWED' });
  });

  it('compares hosts case-insensitively', () => {
    expect(gate(view(), { host: HOST.toUpperCase().replace('0X', '0x'), depositMicro: 500_000n })).toEqual({
      allow: true,
    });
  });

  it('refuses non-positive and over-cap deposits', () => {
    expect(gate(view(), { host: HOST, depositMicro: 0n })).toEqual({ allow: false, reason: 'INVALID_DEPOSIT' });
    expect(gate(view(), { host: HOST, depositMicro: -1n })).toEqual({ allow: false, reason: 'INVALID_DEPOSIT' });
    expect(gate(view(), { host: HOST, depositMicro: 2_000_001n })).toEqual({
      allow: false,
      reason: 'DEPOSIT_OVER_CAP',
    });
  });

  it('refuses over-balance', () => {
    expect(gate(view({ availableMicro: 499_999n }), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'INSUFFICIENT_BALANCE',
    });
  });

  it('refuses when the daily window would be exceeded, counting the requested deposit', () => {
    expect(
      gate(view({ spentInWindowMicro: 9_600_000n }), { host: HOST, depositMicro: 500_000n })
    ).toEqual({ allow: false, reason: 'DAILY_CAP_EXCEEDED' });
    expect(
      gate(view({ spentInWindowMicro: 9_500_000n }), { host: HOST, depositMicro: 500_000n })
    ).toEqual({ allow: true });
  });

  it('refuses when the per-minute open count is reached', () => {
    expect(gate(view({ opensInWindow: 3 }), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'RATE_LIMITED',
    });
  });
});

describe('gatekeeperConfigFromEnv', () => {
  afterEach(() => {
    delete process.env.FIAT_ALLOWED_HOSTS;
    delete process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO;
    delete process.env.FIAT_MAX_DAILY_SPEND_MICRO;
    delete process.env.FIAT_MAX_OPENS_PER_MINUTE;
  });

  it('parses the comma-separated allow-list and numeric caps', () => {
    process.env.FIAT_ALLOWED_HOSTS = `${HOST}, 0xdead000000000000000000000000000000000002`;
    process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO = '1500000';
    process.env.FIAT_MAX_DAILY_SPEND_MICRO = '5000000';
    process.env.FIAT_MAX_OPENS_PER_MINUTE = '5';
    const config = gatekeeperConfigFromEnv();
    expect(config.allowedHosts).toEqual([HOST, '0xdead000000000000000000000000000000000002']);
    expect(config.maxDepositPerSessionMicro).toBe(1_500_000n);
    expect(config.maxDailySpendMicro).toBe(5_000_000n);
    expect(config.maxOpensPerMinute).toBe(5);
  });

  it('defaults to an EMPTY allow-list (unconfigured deployments refuse every open)', () => {
    const config = gatekeeperConfigFromEnv();
    expect(config.allowedHosts).toEqual([]);
    const gate = makeGatekeeper(config);
    expect(gate(view(), { host: HOST, depositMicro: 500_000n })).toEqual({
      allow: false,
      reason: 'HOST_NOT_ALLOWED',
    });
  });

  it('rejects malformed cap values instead of silently defaulting them', () => {
    process.env.FIAT_MAX_SESSION_DEPOSIT_MICRO = 'lots';
    expect(() => gatekeeperConfigFromEnv()).toThrow(/FIAT_MAX_SESSION_DEPOSIT_MICRO/);
  });
});

// FT1 — per-kind caps (D2), the kind↔model binding (D10), concurrency (D11),
// enabled-kind sizing (D5) and the env parse for the new knobs.
describe('per-kind gatekeeping (FT1)', () => {
  const TRAINING_ID = `0x${'a4'.repeat(32)}`;
  const FT1_CONFIG: GatekeeperConfig = {
    ...CONFIG,
    perKind: { training: { maxDepositPerSessionMicro: 10_000_000n, maxDailySpendMicro: 20_000_000n } },
    trainingModelIds: [TRAINING_ID],
    maxConcurrentTraining: 1,
  };
  const gate = makeGatekeeper(FT1_CONFIG);
  const trainingReq = { host: HOST, depositMicro: 8_600_000n, kind: 'training' as const, modelId: TRAINING_ID };

  it('a training deposit is judged by the training caps, a standard one by the globals', () => {
    expect(gate(view({ availableMicro: 50_000_000n }), trainingReq)).toEqual({ allow: true });
    expect(gate(view({ availableMicro: 50_000_000n }), { host: HOST, depositMicro: 8_600_000n, modelId: `0x${'ab'.repeat(32)}` })).toEqual({
      allow: false,
      reason: 'DEPOSIT_OVER_CAP',
    });
    // the training day
    expect(gate(view({ availableMicro: 50_000_000n, spentInWindowMicro: 12_000_000n }), trainingReq)).toEqual({
      allow: false,
      reason: 'DAILY_CAP_EXCEEDED',
    });
  });

  it('binds kind and model id both ways, and a training kind without a model id is refused', () => {
    const v = view({ availableMicro: 50_000_000n });
    expect(gate(v, { ...trainingReq, modelId: `0x${'ab'.repeat(32)}` })).toEqual({ allow: false, reason: 'MODEL_KIND_MISMATCH' });
    expect(gate(v, { host: HOST, depositMicro: 500_000n, modelId: TRAINING_ID })).toEqual({ allow: false, reason: 'MODEL_KIND_MISMATCH' });
    expect(gate(v, { host: HOST, depositMicro: 500_000n, kind: 'training' })).toEqual({ allow: false, reason: 'MODEL_KIND_MISMATCH' });
    // case-insensitive
    expect(gate(v, { ...trainingReq, modelId: TRAINING_ID.toUpperCase().replace('0X', '0x') })).toEqual({ allow: true });
    // an empty set closes the kind
    expect(makeGatekeeper({ ...FT1_CONFIG, trainingModelIds: [] })(v, trainingReq)).toEqual({ allow: false, reason: 'MODEL_KIND_MISMATCH' });
  });

  it('refuses a second live training hold per user, standard is unlimited', () => {
    const v = view({ availableMicro: 50_000_000n, liveTrainingHolds: 1 });
    expect(gate(v, trainingReq)).toEqual({ allow: false, reason: 'CONCURRENT_CAP_EXCEEDED' });
    expect(gate(v, { host: HOST, depositMicro: 500_000n, modelId: `0x${'ab'.repeat(32)}` })).toEqual({ allow: true });
    expect(makeGatekeeper({ ...FT1_CONFIG, maxConcurrentTraining: 2 })(v, trainingReq)).toEqual({ allow: true });
  });

  it('the largest cap ranges over ENABLED kinds only (D5)', () => {
    expect(largestSessionCapMicro(FT1_CONFIG)).toBe(10_000_000n);
    expect(largestSessionCapMicro({ ...FT1_CONFIG, trainingModelIds: [] })).toBe(2_000_000n);
    expect(isKindEnabled(FT1_CONFIG, 'training')).toBe(true);
    expect(isKindEnabled({ ...FT1_CONFIG, trainingModelIds: undefined }, 'training')).toBe(false);
    expect(isKindEnabled(FT1_CONFIG, 'standard')).toBe(true);
  });

  it('a kind absent from perKind falls back to the standard caps (never looser)', () => {
    expect(capsFor({ ...FT1_CONFIG, perKind: undefined }, 'training')).toEqual({
      maxDepositPerSessionMicro: 2_000_000n,
      maxDailySpendMicro: 10_000_000n,
    });
  });

  describe('env parse', () => {
    afterEach(() => {
      delete process.env.FIAT_TRAINING_MODEL_IDS;
      delete process.env.FIAT_TRAINING_MAX_SESSION_DEPOSIT_MICRO;
      delete process.env.FIAT_TRAINING_MAX_DAILY_SPEND_MICRO;
      delete process.env.FIAT_TRAINING_MAX_CONCURRENT;
    });

    it('parses the training knobs, lowercasing the ids, and defaults to a CLOSED training kind', () => {
      process.env.FIAT_TRAINING_MODEL_IDS = ` ${TRAINING_ID.toUpperCase().replace('0X', '0x')}, 0x${'11'.repeat(32)} `;
      process.env.FIAT_TRAINING_MAX_SESSION_DEPOSIT_MICRO = '12000000';
      process.env.FIAT_TRAINING_MAX_DAILY_SPEND_MICRO = '30000000';
      process.env.FIAT_TRAINING_MAX_CONCURRENT = '2';
      const config = gatekeeperConfigFromEnv();
      expect(config.trainingModelIds).toEqual([TRAINING_ID, `0x${'11'.repeat(32)}`]);
      expect(config.perKind?.training).toEqual({ maxDepositPerSessionMicro: 12_000_000n, maxDailySpendMicro: 30_000_000n });
      expect(config.maxConcurrentTraining).toBe(2);
      const defaults = gatekeeperConfigFromEnv();
      expect(defaults.trainingModelIds).toEqual([TRAINING_ID, `0x${'11'.repeat(32)}`]);
      delete process.env.FIAT_TRAINING_MODEL_IDS;
      const closed = gatekeeperConfigFromEnv();
      expect(closed.trainingModelIds).toEqual([]);
      expect(closed.perKind?.training).toEqual({ maxDepositPerSessionMicro: 12_000_000n, maxDailySpendMicro: 30_000_000n });
      expect(describeGatekeeperConfig(closed)).toContain('training disabled');
      expect(describeGatekeeperConfig(config)).toContain('training ENABLED');
    });
  });
});
