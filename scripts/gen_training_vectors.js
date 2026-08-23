// Generates tests/training/vectors/*.json per DESIGN-TRAINING-M0-INTERFACE.md
// (§Test Vectors; Status-line version is the authority). Run with the helper's
// node_modules: NODE_PATH=platformless-helper/node_modules node scripts/gen_training_vectors.js
// Deterministic: fixed test key, fixed placeholder hashes, no Date/random.
// v3 (converge rounds 1-2): non-power-of-two seed (2^64 was f64-exact, defusing the
// float trap), four-case billing (the worked example alone could not tell floor from
// ceil, and nothing separated floor from round-to-nearest on the gross), the D.1
// shifted-remainder manifest case, and placeholder/sessionId labelling.
const { ethers } = require('ethers');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const OUT = '/workspace/fabstir-llm-node/tests/training/vectors';
fs.mkdirSync(OUT, { recursive: true });

// ---------- canonicalisation: recursively sorted keys, compact, UTF-8 ----------
// Must match the node's checkpoint::delta::sort_json_keys + serde_json compact output.
// SAFE SUBSET ONLY: conformant manifests carry ASCII keys, ASCII strings, and small
// integers — inside that subset JS and serde_json emit identical bytes. Floats,
// non-ASCII keys, or >= 1e21 numbers WOULD diverge (JS "1e+21" vs ryu "1e21"; UTF-16 vs
// UTF-8 key sort) — if a schema ever adds such a field, add a vector for it FIRST.
function canonical(value) {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  if (value !== null && typeof value === 'object') {
    const keys = Object.keys(value).sort();
    return '{' + keys.map(k => JSON.stringify(k) + ':' + canonical(value[k])).join(',') + '}';
  }
  return JSON.stringify(value);
}
const sha256hex = (s) => '0x' + crypto.createHash('sha256').update(Buffer.from(s, 'utf8')).digest('hex');

// ---------- fixed identities ----------
const TEMPLATE_ID = 'train-qlora-qwen38-27b-v1';
const MODEL_ID = ethers.keccak256(ethers.toUtf8Bytes('fabstir/training/' + TEMPLATE_ID)); // the REAL registry id
const TEMPLATE_HASH = ethers.keccak256(ethers.toUtf8Bytes(TEMPLATE_ID + '@vector-placeholder')); // placeholder until T1.4
const ENV_HASH = ethers.keccak256(ethers.toUtf8Bytes('envHash@vector-placeholder')); // PLACEHOLDER (see sig-digest _note)
const TOKENIZER_SHA = '0x' + '11'.repeat(32); // placeholder; real value from gen_counting_fixture.py
// Well-known throwaway test key (hardhat #0) — vectors only, never funds.
const TEST_KEY = '0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80';
const wallet = new ethers.Wallet(TEST_KEY);
// Adversarial seed: > 2^64 AND not a power of two (2^64 itself is f64-exact — the
// round-1 review caught the original vector defusing its own float trap).
const SEED = '18446744073709551629'; // 2^64 + 13
if (BigInt(Number(SEED)) === BigInt(SEED)) throw new Error('seed must NOT survive a float round-trip');
if (BigInt(SEED) <= 0xffffffffffffffffn) throw new Error('seed must exceed u64::MAX');

// ---------- 4. manifests.json (built first: commitment vector consumes datasetManifestSha256) ----------
const SHARD_MAX = 25161728;
const datasetManifest = {
  schema: 'dataset-manifest-v1',
  format: 'jsonl-text-v1',
  countingRecipe: 'count-v1',
  tokenizerSha256: TOKENIZER_SHA,
  samples: 5000,
  declaredTokens: 3200000,
  totalBytes: 18900000,
  shards: [{ cid: 'uVECTORDATASETSHARD0PLACEHOLDER0000000000000000000', sha256: '0x' + '22'.repeat(32), sizeBytes: 18900000 }],
};
const adapterManifest = {
  schema: 'artifact-manifest-v1',
  kind: 'adapter',
  files: [
    {
      name: 'adapter_model.safetensors', sha256: '0x' + '33'.repeat(32), sizeBytes: 167772160,
      shards: [
        { cid: 'uVECTORADPTSHARD0PLACEHOLDER00000000000000000000000', sha256: '0x' + '44'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORADPTSHARD1PLACEHOLDER00000000000000000000000', sha256: '0x' + '45'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORADPTSHARD2PLACEHOLDER00000000000000000000000', sha256: '0x' + '46'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORADPTSHARD3PLACEHOLDER00000000000000000000000', sha256: '0x' + '47'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORADPTSHARD4PLACEHOLDER00000000000000000000000', sha256: '0x' + '48'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORADPTSHARD5PLACEHOLDER00000000000000000000000', sha256: '0x' + '49'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORADPTSHARD6PLACEHOLDER00000000000000000000000', sha256: '0x' + '4a'.repeat(32), sizeBytes: 16801792 },
      ],
    },
    {
      name: 'adapter.gguf', sha256: '0x' + '55'.repeat(32), sizeBytes: 90000000,
      shards: [
        { cid: 'uVECTORGGUFSHARD0PLACEHOLDER00000000000000000000000', sha256: '0x' + '66'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORGGUFSHARD1PLACEHOLDER00000000000000000000000', sha256: '0x' + '67'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORGGUFSHARD2PLACEHOLDER00000000000000000000000', sha256: '0x' + '68'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORGGUFSHARD3PLACEHOLDER00000000000000000000000', sha256: '0x' + '69'.repeat(32), sizeBytes: 14514816 },
      ],
    },
  ],
};
// The D.1 shifted-remainder branch, pinned as bytes (v0.3.2 disambiguation): file
// = 1×MAX + 524,288 (an exact 2×262,144 multiple), so the splitter emits
// (remainder − 1) then a trailing 1-byte shard: [MAX, 524287, 1].
const shiftedRemainderManifest = {
  schema: 'artifact-manifest-v1',
  kind: 'checkpoint',
  sliceIndex: 4,
  files: [
    {
      name: 'optimizer.bin', sha256: '0x' + '88'.repeat(32), sizeBytes: SHARD_MAX + 524288,
      shards: [
        { cid: 'uVECTORSHIFTSHARD0PLACEHOLDER0000000000000000000000', sha256: '0x' + '99'.repeat(32), sizeBytes: SHARD_MAX },
        { cid: 'uVECTORSHIFTSHARD1PLACEHOLDER0000000000000000000000', sha256: '0x' + '9a'.repeat(32), sizeBytes: 524287 },
        { cid: 'uVECTORSHIFTSHARD2PLACEHOLDER0000000000000000000000', sha256: '0x' + '9b'.repeat(32), sizeBytes: 1 },
      ],
    },
  ],
};
// sanity: shard sums equal file sizes; no shard is an exact chunk multiple
for (const m of [adapterManifest, shiftedRemainderManifest]) {
  for (const f of m.files) {
    const sum = f.shards.reduce((a, s) => a + s.sizeBytes, 0);
    if (sum !== f.sizeBytes) throw new Error(`shard sum mismatch for ${f.name}: ${sum} != ${f.sizeBytes}`);
    for (const s of f.shards) if (s.sizeBytes % 262144 === 0) throw new Error(`chunk-multiple shard in ${f.name}: ${s.sizeBytes}`);
  }
}
if (datasetManifest.shards[0].sizeBytes % 262144 === 0) throw new Error('dataset shard is a chunk multiple');
const datasetCanonical = canonical(datasetManifest);
const adapterCanonical = canonical(adapterManifest);
const shiftedCanonical = canonical(shiftedRemainderManifest);
const datasetManifestSha256 = sha256hex(datasetCanonical);
const adapterManifestSha256 = sha256hex(adapterCanonical);
const shiftedManifestSha256 = sha256hex(shiftedCanonical);

// ---------- 1. input-commitment.json ----------
const job = {
  templateId: TEMPLATE_ID,
  templateHash: TEMPLATE_HASH,
  dataset: { manifestCID: 'uVECTORDATASETMANIFESTPLACEHOLDER000000000000000000', manifestSha256: datasetManifestSha256, declaredTokens: 3200000, samples: 5000 },
  epochs: 3,
  hyper: { rank: 16, alpha: 32, lr: '0.000200', seed: SEED, seqLen: 2048 },
  output: 'adapter-v1',
};
const COMMIT_TYPES = ['bytes32', 'bytes32', 'uint256', 'uint32', 'uint32', 'uint32', 'string', 'uint256', 'uint32'];
const commitValues = [
  job.templateHash, job.dataset.manifestSha256, BigInt(job.dataset.declaredTokens),
  job.epochs, job.hyper.rank, job.hyper.alpha, job.hyper.lr, BigInt(job.hyper.seed), job.hyper.seqLen,
];
const abiBytes = ethers.AbiCoder.defaultAbiCoder().encode(COMMIT_TYPES, commitValues);
const inputCommitment = ethers.keccak256(abiBytes);

// ---------- 2. slice-schedule.json ----------
const SLICE_TOKENS = 1000000;
function schedule(declaredTokens, epochs) {
  const total = declaredTokens * epochs;
  const slices = Math.max(1, Math.floor(total / SLICE_TOKENS));
  const deltas = [];
  for (let i = 0; i < slices - 1; i++) deltas.push(SLICE_TOKENS);
  deltas.push(total - (slices - 1) * SLICE_TOKENS);
  return { declaredTokens, epochs, sliceTokens: SLICE_TOKENS, totalTokens: total, slices, deltas };
}
const scheduleCases = {
  worked: schedule(3200000, 3),          // 9 slices: 8×1M + 1.6M
  remainderFree: schedule(1000000, 2),   // 2 slices: 1M + 1M
  tinyRemainder: schedule(333350, 3),    // 1,000,050 → 1 slice
  subSliceTokens: schedule(10000, 1),    // 10,000 → 1 slice (the max(1,·) floor binds)
};
const expect = (c, slices, last) => { if (c.slices !== slices || c.deltas[c.deltas.length - 1] !== last) throw new Error('schedule drift'); };
expect(scheduleCases.worked, 9, 1600000);
expect(scheduleCases.remainderFree, 2, 1000000);
expect(scheduleCases.tinyRemainder, 1, 1000050);
expect(scheduleCases.subSliceTokens, 1, 10000);

// ---------- 3. sig-digest.json ----------
const DIGEST_TYPES = ['bytes32', 'bytes32', 'bytes32', 'bytes32', 'bytes32', 'uint256', 'uint256', 'uint256', 'address', 'uint256'];
const digestValues = [
  MODEL_ID, TEMPLATE_HASH, ENV_HASH, inputCommitment, '0x' + '77'.repeat(32) /* checkpointManifestSha256 */,
  0n /* sliceIndex */, BigInt(scheduleCases.worked.deltas[0]) /* tokensDelta = worked delta[0] */,
  12345n /* sessionId */, wallet.address, 1790000000n /* timestamp */,
];
const digestAbiBytes = ethers.AbiCoder.defaultAbiCoder().encode(DIGEST_TYPES, digestValues);
const sigDigest = ethers.keccak256(digestAbiBytes);

// ---------- 6. billing.json — four cases (rounds 1-2: the worked example alone divides
// evenly everywhere, so floor/ceil/round implementations were indistinguishable) ----------
const PRICE = 904n;
function billing(tokens) {
  const gross = (tokens * PRICE) / 1000n;                        // floor
  const deposit = ((gross * 105n) + 99n) / 100n;                 // ceil(×1.05)
  const dep = deposit > 500000n ? deposit : 500000n;             // 0.5 USDC floor
  return { trainingTokens: Number(tokens), pricePerToken: Number(PRICE), grossMicroUsdc: Number(gross), depositMicroUsdc: Number(dep) };
}
const billingCases = {
  worked: billing(9600000n),        // even everywhere: 8,678,400 / 9,112,320
  roundingBites: billing(1000050n), // gross floors (904,045.2 → 904,045); ceil ≠ floor (949,248 vs 949,247)
  roundVsFloor: billing(1000053n),  // gross fraction .912 → floor 904,047; round-to-nearest would give 904,048 (round-2 gap)
  minFloorBinds: billing(10000n),   // 9,040 gross → padded 9,492 → deposit = 500,000
};
const pinB = (c, g, d) => { if (c.grossMicroUsdc !== g || c.depositMicroUsdc !== d) throw new Error('billing drift'); };
pinB(billingCases.worked, 8678400, 9112320);
pinB(billingCases.roundingBites, 904045, 949248);
pinB(billingCases.roundVsFloor, 904047, 949250);
pinB(billingCases.minFloorBinds, 9040, 500000);
if ((1000053n * PRICE) % 1000n < 500n) throw new Error('roundVsFloor case must have gross fraction >= .5');

async function main() {
  const signature = await wallet.signMessage(ethers.getBytes(sigDigest)); // EIP-191 personal_sign
  if (ethers.verifyMessage(ethers.getBytes(sigDigest), signature) !== wallet.address) throw new Error('sig roundtrip failed');

  const files = {
    'input-commitment.json': {
      _note: 'Interface B.4. abiEncoded = abi.encode over commitTypes/values; inputCommitment = keccak256(abiEncoded). lr is committed byte-for-byte as sent (trailing zeros deliberate); seed = 2^64+13 (> u64::MAX AND float-lossy — deliberately NOT a power of two). PLACEHOLDERS: templateHash (real one arrives with the T1.4 template), tokenizerSha256, all CIDs, shard sha256s. REAL: datasetManifestSha256 (= manifests.json dataset), declaredTokens/epochs (= slice-schedule worked case), and the registry modelId in sig-digest.json.',
      job,
      commitTypes: COMMIT_TYPES,
      abiEncoded: abiBytes,
      inputCommitment,
    },
    'slice-schedule.json': {
      _note: 'Interface B.1 floor rule: slices = max(1, floor(total/sliceTokens)); last slice absorbs the remainder. Every delta >= min(total, sliceTokens) >= 10000.',
      cases: scheduleCases,
    },
    'sig-digest.json': {
      _note: 'Interface B.5. sigDigest = keccak256(abi.encode(digestTypes/values)); signature = EIP-191 personal_sign of the 32 digest bytes by the throwaway vector key (hardhat #0 — public test key, never funds). Recover: verifyMessage(getBytes(sigDigest), signature) === host. PLACEHOLDERS: templateHash, envHash (keccak of "envHash@vector-placeholder" — the REAL M0 envHash is the LTX empty-environment constant, a DIFFERENT value; never pin this one), checkpointManifestSha256 (0x77…). sessionId NOTE: shown here as the JSON number 12345 for the uint256 digest input; a real B.3 attestation carries sessionId as a 0x-hex STRING ("0x3039" for this value) per the wire ground rules — parse it to uint256 before encoding.',
      digestTypes: DIGEST_TYPES,
      values: {
        modelId: MODEL_ID, templateHash: TEMPLATE_HASH, envHash: ENV_HASH, inputCommitment,
        checkpointManifestSha256: '0x' + '77'.repeat(32), sliceIndex: 0,
        tokensDelta: scheduleCases.worked.deltas[0],
        sessionId: 12345, host: wallet.address, timestamp: 1790000000,
      },
      abiEncoded: digestAbiBytes,
      sigDigest,
      signer: { address: wallet.address, privateKey: TEST_KEY },
      signature,
    },
    'manifests.json': {
      _note: 'Interface D.2/D.3. canonicalBytes = recursively-key-sorted compact UTF-8 JSON; manifestSha256 = SHA256(exact canonical bytes). The STORED manifest bytes must equal canonicalBytes (verify fetched manifests by hashing RAW bytes, never by re-canonicalising). Sizes respect D.1: non-final shards exactly 25,161,728; shiftedRemainder pins the exact-multiple branch — remainder 524,288 (= 2×262,144) splits into 524,287 + 1.',
      dataset: { object: datasetManifest, canonicalBytes: datasetCanonical, manifestSha256: datasetManifestSha256 },
      adapter: { object: adapterManifest, canonicalBytes: adapterCanonical, manifestSha256: adapterManifestSha256 },
      shiftedRemainder: { _note: 'File list abbreviated to the splitter-relevant file; a REAL checkpoint manifest also carries adapter_model.safetensors + trainer_state.json (D.3) — this entry pins the D.1 splitter branch, not the full checkpoint shape.', object: shiftedRemainderManifest, canonicalBytes: shiftedCanonical, manifestSha256: shiftedManifestSha256 },
    },
    'billing.json': {
      _note: 'Interface C.1 at sample pricePerToken 904 (the registered price is set at T7). gross = floor(tokens*price/1000); deposit = max(500000, ceil(gross*1.05)) computed as (gross*105+99)/100. Four cases: worked (divides evenly), roundingBites (floor AND ceil both bite — 949,248 vs a floored 949,247), roundVsFloor (gross fraction .912 — a round-to-nearest gross gives 904,048, floor gives 904,047), minFloorBinds (the 0.5 USDC floor decides).',
      minDepositMicroUsdc: 500000,
      cases: billingCases,
    },
  };
  for (const [name, obj] of Object.entries(files)) {
    fs.writeFileSync(path.join(OUT, name), JSON.stringify(obj, null, 2) + '\n');
    console.log('wrote', name);
  }
  console.log('modelId (REAL registry id):', MODEL_ID);
  console.log('inputCommitment:', inputCommitment);
  console.log('sigDigest:', sigDigest);
}
main().catch(e => { console.error(e); process.exit(1); });
