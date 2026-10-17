// Generates (or inspects) the Chromium extension key that pins the extension id.
//
//   node scripts/gen-key.mjs            -> prints a fresh `key` and the id it yields
//   node scripts/gen-key.mjs --id       -> prints the id of the key already in manifest/chrome.json
//
// Only the public key goes into the manifest; the id is the first 128 bits of SHA-256 over the
// SPKI DER, hex-encoded with a-p instead of 0-9a-f. No private key is needed for unpacked loads
// (the stores sign their own packages).
import { createHash, generateKeyPairSync } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export function extensionIdFromKey(keyBase64) {
  const der = Buffer.from(keyBase64, 'base64');
  const hex = createHash('sha256').update(der).digest('hex').slice(0, 32);
  return hex.replace(/[0-9a-f]/g, (c) => String.fromCharCode('a'.charCodeAt(0) + parseInt(c, 16)));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const root = dirname(dirname(fileURLToPath(import.meta.url)));
  if (process.argv.includes('--id')) {
    const manifest = JSON.parse(readFileSync(join(root, 'manifest', 'chrome.json'), 'utf8'));
    console.log(extensionIdFromKey(manifest.key));
  } else {
    const { publicKey } = generateKeyPairSync('rsa', {
      modulusLength: 2048,
      publicKeyEncoding: { type: 'spki', format: 'der' },
      privateKeyEncoding: { type: 'pkcs8', format: 'pem' },
    });
    const key = publicKey.toString('base64');
    console.log(JSON.stringify({ key, id: extensionIdFromKey(key) }, null, 2));
  }
}
