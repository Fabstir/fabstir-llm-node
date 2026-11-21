// R10: configuration fails closed. Each missing or malformed required value
// refuses to start, and the refusal names the variable, never its value.
import { describe, expect, it } from 'vitest';
import { ConfigError, loadConfig, REQUIRED_VARIABLES } from '../src/config.js';

const SECRET_HEX = '00112233445566778899aabbccddeeff'.repeat(2);
const FULL: Record<string, string> = {
  VAULT_REGION: 'uk',
  VAULT_PORTAL_HOST: 'uk.portal.example',
  VAULT_CHAIN_ID: '84532',
  VAULT_RPC_URLS: 'https://rpc-one.example, https://rpc-two.example/path',
  VAULT_TOKEN_SECRET: SECRET_HEX,
  VAULT_S3_ENDPOINT: 'http://127.0.0.1:3900',
  VAULT_S3_ACCESS_KEY: 'GK-test-access',
  VAULT_S3_SECRET_KEY: 'test-secret-key-value',
  VAULT_S3_BUCKET: 's5-vault',
  VAULT_S3_REGION: 'us-east-1',
};

function problemsOf(env: Record<string, string | undefined>): string[] {
  try {
    loadConfig(env);
  } catch (e) {
    expect(e).toBeInstanceOf(ConfigError);
    return (e as ConfigError).problems;
  }
  throw new Error('expected loadConfig to refuse');
}

describe('loadConfig', () => {
  it('accepts a full environment and applies the documented defaults', () => {
    const c = loadConfig(FULL);
    expect(c.region).toBe('uk');
    expect(c.portalHost).toBe('uk.portal.example');
    expect(c.chainId).toBe(84532);
    expect(c.rpcUrls).toEqual(['https://rpc-one.example', 'https://rpc-two.example/path']);
    expect(c.tokenSecret.equals(Buffer.from(SECRET_HEX, 'hex'))).toBe(true);
    expect(c.s3).toEqual({
      endpoint: 'http://127.0.0.1:3900',
      accessKey: 'GK-test-access',
      secretKey: 'test-secret-key-value',
      bucket: 's5-vault',
      region: 'us-east-1',
    });
    expect(c.bind).toBe('127.0.0.1');
    expect(c.port).toBe(5052);
    expect(c.readTokenDays).toBe(30);
    expect(c.maxVaultBytes).toBe(32768);
    expect(c.allowedOrigins).toEqual([]);
    expect(c.ratePerMin).toBe(60);
    expect(c.challengeMax).toBe(10000);
  });

  it('lists exactly the ten required variables', () => {
    expect([...REQUIRED_VARIABLES].sort()).toEqual(Object.keys(FULL).sort());
  });

  for (const name of Object.keys(FULL)) {
    it(`refuses to start without ${name}, and says so`, () => {
      for (const value of [undefined, '', '   ']) {
        const problems = problemsOf({ ...FULL, [name]: value });
        expect(problems).toHaveLength(1);
        expect(problems[0]).toContain(name);
      }
    });
  }

  it('names every missing variable at once for an empty environment', () => {
    const problems = problemsOf({});
    for (const name of Object.keys(FULL)) expect(problems.some((p) => p.includes(name))).toBe(true);
    expect(problems).toHaveLength(10);
  });

  it('has no default for any required value: unrelated variables do not help', () => {
    expect(problemsOf({ CHAIN_ID: '84532', BASE_SEPOLIA_RPC_URL: 'https://x.example', VAULT_PORT: '5052' })).toHaveLength(10);
  });

  it('refuses a token secret that is short or not hex', () => {
    for (const bad of [SECRET_HEX.slice(2), 'zz'.repeat(32), SECRET_HEX + 'a', 'not-hex']) {
      const problems = problemsOf({ ...FULL, VAULT_TOKEN_SECRET: bad });
      expect(problems.join(' ')).toContain('VAULT_TOKEN_SECRET');
    }
    expect(loadConfig({ ...FULL, VAULT_TOKEN_SECRET: '00112233445566778899AABBCCDDEEFF'.repeat(3) }).tokenSecret).toHaveLength(48);
  });

  it('refuses a chain it cannot verify on, and a malformed chain id', () => {
    for (const bad of ['1', '5611', '0', '-1', '84532.0', 'base', '0x14a34']) {
      expect(problemsOf({ ...FULL, VAULT_CHAIN_ID: bad }).join(' ')).toContain('VAULT_CHAIN_ID');
    }
    expect(loadConfig({ ...FULL, VAULT_CHAIN_ID: '8453' }).chainId).toBe(8453);
  });

  it('refuses RPC URLs and an S3 endpoint that are not http or https URLs', () => {
    for (const bad of ['ftp://x', 'rpc.example', 'https://ok.example,,', 'https://ok.example,nonsense', ',']) {
      expect(problemsOf({ ...FULL, VAULT_RPC_URLS: bad }).join(' ')).toContain('VAULT_RPC_URLS');
    }
    for (const bad of ['ftp://x', '127.0.0.1:3900', 'nonsense']) {
      expect(problemsOf({ ...FULL, VAULT_S3_ENDPOINT: bad }).join(' ')).toContain('VAULT_S3_ENDPOINT');
    }
  });

  it('refuses a token secret with fewer than 8 distinct byte values (R10 v0.2)', () => {
    for (const weak of ['00'.repeat(32), 'a1'.repeat(32), '0001020304050607'.slice(0, 14).repeat(5), '00010203040506'.repeat(8)]) {
      const problems = problemsOf({ ...FULL, VAULT_TOKEN_SECRET: weak });
      expect(problems).toHaveLength(1);
      expect(problems[0]).toContain('VAULT_TOKEN_SECRET');
      expect(problems[0]).not.toContain(weak);
    }
    expect(loadConfig({ ...FULL, VAULT_TOKEN_SECRET: '0001020304050607'.repeat(4) }).tokenSecret).toHaveLength(32);
  });

  it('refuses an S3 endpoint that is not https unless its host is loopback (R10 v0.2)', () => {
    for (const bad of ['http://s3.example:3900', 'http://10.0.0.5:3900', 'http://192.168.1.2', 'http://128.0.0.1:3900', 'http://localhost.example', 'http://[2001:db8::1]:3900']) {
      expect(problemsOf({ ...FULL, VAULT_S3_ENDPOINT: bad }).join(' ')).toContain('VAULT_S3_ENDPOINT');
    }
    for (const good of ['https://s3.example', 'http://127.0.0.1:3900', 'http://127.8.9.10:3900', 'http://localhost:3900', 'http://[::1]:3900', 'https://10.0.0.5:3900']) {
      expect(loadConfig({ ...FULL, VAULT_S3_ENDPOINT: good }).s3.endpoint).toBe(good);
    }
  });

  it('refuses any RPC URL that is not https unless its host is loopback (R10 v0.2)', () => {
    for (const bad of ['http://rpc.example', 'https://ok.example,http://rpc.example', 'http://10.0.0.5:8545']) {
      expect(problemsOf({ ...FULL, VAULT_RPC_URLS: bad }).join(' ')).toContain('VAULT_RPC_URLS');
    }
    expect(loadConfig({ ...FULL, VAULT_RPC_URLS: 'http://127.0.0.1:8545,https://rpc.example,http://localhost:8545,http://[::1]:8545' }).rpcUrls).toHaveLength(4);
  });

  it('refuses a region or portal host that could not sit on one line of the signed message', () => {
    for (const bad of ['uk\nportal: evil', 'u k', 'UK!', 'x'.repeat(40)]) {
      expect(problemsOf({ ...FULL, VAULT_REGION: bad }).join(' ')).toContain('VAULT_REGION');
    }
    for (const bad of ['https://uk.portal.example', 'uk.portal.example/path', 'a b', 'host\nx', 'UK.Portal.Example']) {
      expect(problemsOf({ ...FULL, VAULT_PORTAL_HOST: bad }).join(' ')).toContain('VAULT_PORTAL_HOST');
    }
  });

  it('reads the optional values', () => {
    const c = loadConfig({
      ...FULL,
      VAULT_BIND: '0.0.0.0',
      VAULT_PORT: '6000',
      VAULT_READ_TOKEN_DAYS: '7',
      VAULT_MAX_BYTES: '1024',
      VAULT_ALLOWED_ORIGINS: 'https://app.example, http://localhost:3000',
      VAULT_RATE_PER_MIN: '120',
      VAULT_CHALLENGE_MAX: '50',
    });
    expect(c.bind).toBe('0.0.0.0');
    expect(c.port).toBe(6000);
    expect(c.readTokenDays).toBe(7);
    expect(c.maxVaultBytes).toBe(1024);
    expect(c.allowedOrigins).toEqual(['https://app.example', 'http://localhost:3000']);
    expect(c.ratePerMin).toBe(120);
    expect(c.challengeMax).toBe(50);
  });

  it('refuses malformed optional values instead of falling back silently', () => {
    const cases: Array<[string, string]> = [
      ['VAULT_BIND', 'localhost'],
      ['VAULT_PORT', '0'],
      ['VAULT_PORT', '70000'],
      ['VAULT_PORT', 'abc'],
      ['VAULT_READ_TOKEN_DAYS', '0'],
      ['VAULT_READ_TOKEN_DAYS', '1.5'],
      ['VAULT_MAX_BYTES', '-1'],
      ['VAULT_MAX_BYTES', '999999999'],
      ['VAULT_ALLOWED_ORIGINS', '*'],
      ['VAULT_ALLOWED_ORIGINS', 'https://app.example/path'],
      ['VAULT_ALLOWED_ORIGINS', 'app.example'],
      ['VAULT_RATE_PER_MIN', '0'],
      ['VAULT_CHALLENGE_MAX', 'many'],
    ];
    for (const [name, value] of cases) {
      expect(problemsOf({ ...FULL, [name]: value }).join(' ')).toContain(name);
    }
  });

  it('never puts a secret value in the refusal', () => {
    const problems = problemsOf({
      ...FULL,
      VAULT_TOKEN_SECRET: 'deadbeefdeadbeef',
      VAULT_S3_SECRET_KEY: '',
      VAULT_S3_ACCESS_KEY: 'GK-visible-access',
      VAULT_CHAIN_ID: '1',
    });
    const text = problems.join('\n') + String(new ConfigError(problems).message);
    expect(text).not.toContain('deadbeefdeadbeef');
    expect(text).not.toContain('GK-visible-access');
  });
});
