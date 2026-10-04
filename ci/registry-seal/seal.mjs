import fs from 'node:fs';
import sodium from 'libsodium-wrappers';

// Retrieved through the authenticated GitHub Actions repository-public-key API
// for william-lbn/control-plane on 2026-10-04. This is a PUBLIC encryption key.
// A key rotation requires a reviewed commit; dispatch cannot change recipients.
const destination = 'william-lbn/control-plane';
const keyId = '3380204578043523366';
const publicKey = 'gdZeDrgme+Jww7A6U2TjKEoJ7SCbDSaXD0LKRNUpIXM=';

if (process.env.GITHUB_ACTOR !== 'william-lbn' ||
    process.env.GITHUB_REPOSITORY !== 'william-lbn/neon' ||
    process.env.GITHUB_REF !== 'refs/heads/main') {
  throw new Error('Only the repository owner can authorize this fixed migration');
}
if (process.env.DOCKERHUB_USERNAME !== 'williamluckyli') {
  throw new Error('Existing registry identity does not match the authorized owner');
}
await sodium.ready;
const recipient = sodium.from_base64(publicKey, sodium.base64_variants.ORIGINAL);
if (recipient.length !== sodium.crypto_box_PUBLICKEYBYTES) {
  throw new Error('Invalid destination public key');
}
const secrets = {};
for (const name of ['DOCKERHUB_USERNAME', 'DOCKERHUB_TOKEN']) {
  if (!process.env[name]) throw new Error('Required existing secret is missing');
  const plaintext = sodium.from_string(process.env[name]);
  delete process.env[name];
  try {
    secrets[name] = {
      key_id: keyId,
      encrypted_value: sodium.to_base64(sodium.crypto_box_seal(plaintext, recipient),
        sodium.base64_variants.ORIGINAL),
    };
  } finally {
    sodium.memzero(plaintext);
  }
}
fs.writeFileSync(process.env.SEALED_OUTPUT, JSON.stringify({
  destination, source: process.env.GITHUB_REPOSITORY,
  run_id: process.env.GITHUB_RUN_ID, source_commit: process.env.GITHUB_SHA,
  key_id: keyId, secrets,
}, null, 2) + '\n', {mode: 0o600, flag: 'wx'});
console.log('Sealed two existing credentials to the fixed destination; no plaintext exported.');
