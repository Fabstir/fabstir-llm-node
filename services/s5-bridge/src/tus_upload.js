/**
 * Outbound tus upload for blobs beyond S5's 32 MiB single-shot cap.
 *
 * S5's POST /s5/upload refuses anything over 32 MiB ("This API only supports a
 * maximum size of 32 MiB" — confirmed against the live portal node 2026-07-18,
 * after session 924's 55.8 MB deliverable died on it). The node's tus endpoint
 * carries a 4 GB ceiling and verifies the blake3 hash server-side on completion
 * (mismatches are deleted), so integrity matches the single-shot path.
 *
 * Wire dialect (S5 node http_api, the version this portal runs):
 *   POST  {portal}/s5/upload/tus
 *         Upload-Length:   <total bytes>
 *         Upload-Metadata: hash <b64std(utf8(b64url(multihash)))>
 *           — the node does base64.decode → utf8 → Multihash.fromBase64Url,
 *             so the value is DOUBLY encoded: base64url of the 0x1e-prefixed
 *             blake3 multihash, wrapped once more in standard base64 per tus.
 *         → 201 + Location
 *   PATCH {location}
 *         Upload-Offset: <n>, Content-Type: application/offset+octet-stream
 *         → 204 + Upload-Offset (the final PATCH triggers hash verification)
 */
import { Buffer } from 'node:buffer';
import { BlobIdentifier } from '@julesl23/s5js/dist/src/identifier/blob.js';

export const TUS_THRESHOLD = 32 * 1024 * 1024; // S5's single-shot cap
export const TUS_CHUNK_SIZE = 16 * 1024 * 1024;

const MULTIHASH_BLAKE3 = 0x1e;

/** The doubly-encoded Upload-Metadata value for a multihash (see header note). */
export function tusMetadataHash(multihash) {
  const b64url = Buffer.from(multihash).toString('base64url');
  return Buffer.from(b64url, 'utf8').toString('base64');
}

/** One complete tus upload to one portal. Throws on any failure. */
export async function tusUploadToPortal(portal, blob, multihash, opts = {}) {
  const fetchImpl = opts.fetchImpl ?? fetch;
  const chunkSize = opts.chunkSize ?? TUS_CHUNK_SIZE;
  const auth = portal.headers['Authorization'] || portal.headers['authorization'] || '';

  const createUrl = portal.apiURL('upload/tus');
  const createRes = await fetchImpl(createUrl, {
    method: 'POST',
    headers: {
      Authorization: auth,
      'Tus-Resumable': '1.0.0',
      'Upload-Length': String(blob.size),
      'Upload-Metadata': `hash ${tusMetadataHash(multihash)}`,
    },
  });
  if (createRes.status !== 201) {
    throw new Error(`tus create failed: HTTP ${createRes.status}: ${await createRes.text()}`);
  }
  const location = createRes.headers.get('location');
  if (!location) {
    throw new Error('tus create returned no Location header');
  }
  const uploadUrl = new URL(location, createUrl).toString();

  let offset = 0;
  while (offset < blob.size) {
    const end = Math.min(offset + chunkSize, blob.size);
    const chunk = Buffer.from(await blob.slice(offset, end).arrayBuffer());
    const res = await fetchImpl(uploadUrl, {
      method: 'PATCH',
      headers: {
        Authorization: auth,
        'Tus-Resumable': '1.0.0',
        'Upload-Offset': String(offset),
        'Content-Type': 'application/offset+octet-stream',
      },
      body: chunk,
    });
    if (res.status !== 204) {
      throw new Error(`tus PATCH at offset ${offset} failed: HTTP ${res.status}: ${await res.text()}`);
    }
    const serverOffset = Number(res.headers.get('upload-offset'));
    if (!Number.isFinite(serverOffset) || serverOffset <= offset) {
      throw new Error(
        `tus PATCH did not advance: server offset ${res.headers.get('upload-offset')} from ${offset}`,
      );
    }
    offset = serverOffset;
  }
}

/**
 * Patch an S5APIWithIdentity instance so uploadBlob routes blobs at or beyond
 * the single-shot cap through tus. Small blobs keep the original path (and its
 * read-your-writes cache); large blobs skip the in-memory cache deliberately —
 * downloadBlobAsBytes falls back to the portal by CID, which now has the blob.
 * Covers every consumer on the instance: FS5.put (the node's /s5/fs PUTs) and
 * direct uploadBlob callers alike.
 */
export function installTusLargeUpload(api, opts = {}) {
  const threshold = opts.threshold ?? TUS_THRESHOLD;
  const logger = opts.logger ?? console;
  const orig = api.uploadBlob.bind(api);

  api.uploadBlob = async (blob) => {
    if (blob.size < threshold) {
      return orig(blob);
    }
    const portals = Object.values(api.accountConfigs);
    if (portals.length === 0) {
      throw new Error('No portals available for upload');
    }
    const blake3Hash = await api.crypto.hashBlake3Blob(blob);
    const multihash = new Uint8Array(1 + blake3Hash.length);
    multihash[0] = MULTIHASH_BLAKE3;
    multihash.set(blake3Hash, 1);
    const bid = new BlobIdentifier(multihash, blob.size);
    // The WIRE metadata carries the DEPRECATED type byte 0x1f: the stock S5 node
    // computes its completion hash with lib5's mhashBlake3Default (0x1f) and
    // compares full multihash bytes, so 0x1e is rejected as "Invalid hash found".
    // Storage-wise this matches every single-shot blob already on the portal
    // (vanilla uploadRawFile also keys 0x1f); reads work because s5.js's
    // BlobIdentifier.decode normalises 0x1f→0x1e on the way back (the legacy
    // branch in identifier/blob.js). The identifier we RETURN stays modern 0x1e,
    // exactly what the single-shot path yields post-normalisation.
    const wireMultihash = Uint8Array.from(multihash);
    wireMultihash[0] = 0x1f;

    let lastErr;
    // Mirror the original's retry ladder: three passes over the portal list.
    for (const portal of portals.concat(portals, portals)) {
      try {
        await tusUploadToPortal(portal, blob, wireMultihash, opts);
        logger.log?.(`[tus-upload] ✅ ${blob.size} bytes → ${portal.host} (resumable path)`);
        return bid;
      } catch (e) {
        lastErr = e;
        logger.error?.(`[tus-upload] ${portal.host}: ${e.message}`);
      }
    }
    throw new Error(
      `Failed to tus-upload blob with 3 tries for each available portal: ${lastErr?.message}`,
    );
  };
  api.uploadBlob.__tusInstalled = true;
  return api;
}
