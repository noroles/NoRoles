// Signed answers. Each person has an ed25519 key; the private half stays on their computer,
// encrypted with a passphrase, so an agent running on the same machine cannot sign for them.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';

export const keyDir = () => process.env.NOROLES_KEYS || path.join(os.homedir(), '.noroles', 'keys');
export const keyPath = (id) => path.join(keyDir(), `${id}.pem`);

/** Canonical JSON: keys sorted at every level, so a hash covers every nested field. */
export function canonical(v) {
  if (Array.isArray(v)) return `[${v.map(canonical).join(',')}]`;
  if (v && typeof v === 'object') return `{${Object.keys(v).sort().map((k) => `${JSON.stringify(k)}:${canonical(v[k])}`).join(',')}}`;
  return JSON.stringify(v ?? null);
}

export function keygen(id, passphrase) {
  if (!passphrase || passphrase.length < 8) throw new Error('use a passphrase of at least 8 characters');
  const file = keyPath(id);
  if (fs.existsSync(file)) throw new Error(`${file} already exists`);
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519', {
    publicKeyEncoding: { type: 'spki', format: 'der' },
    privateKeyEncoding: { type: 'pkcs8', format: 'pem', cipher: 'aes-256-cbc', passphrase },
  });
  fs.mkdirSync(keyDir(), { recursive: true, mode: 0o700 });
  fs.writeFileSync(file, privateKey, { mode: 0o600 });
  return `ed25519:${publicKey.toString('base64')}`;
}

/** What a person signs: the request, the exact action it approves, and the answer. */
export const statement = (r, answer, at) => `noroles/1\n${r.id}\n${r.action_hash}\n${answer}\n${at}`;

export function sign(id, passphrase, text) {
  let key;
  try { key = crypto.createPrivateKey({ key: fs.readFileSync(keyPath(id), 'utf8'), passphrase }); }
  catch (e) { throw new Error(e.code === 'ENOENT' ? `no signing key for ${id} on this computer` : 'wrong passphrase: nothing was signed'); }
  return crypto.sign(null, Buffer.from(text), key).toString('base64');
}

export function verify(pub, text, sig) {
  if (!pub || !sig || !String(pub).startsWith('ed25519:')) return false;
  try {
    const key = crypto.createPublicKey({ key: Buffer.from(pub.slice(8), 'base64'), format: 'der', type: 'spki' });
    return crypto.verify(null, Buffer.from(text), key, Buffer.from(sig, 'base64'));
  } catch { return false; }
}
