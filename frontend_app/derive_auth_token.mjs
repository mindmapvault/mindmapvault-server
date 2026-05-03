import { argon2id } from 'hash-wasm';
import { hkdf } from '@noble/hashes/hkdf';
import { sha256 } from '@noble/hashes/sha256';

const [password, saltB64, mCost, tCost, pCost] = process.argv.slice(2);

const salt = Uint8Array.from(Buffer.from(saltB64, 'base64'));
const masterKey = await argon2id({
  password,
  salt,
  parallelism: Number(pCost),
  iterations: Number(tCost),
  memorySize: Number(mCost),
  hashLength: 32,
  outputType: 'binary',
});
const authBytes = hkdf(sha256, masterKey, undefined, 'crypt-mind-auth-v1', 32);
process.stdout.write(Buffer.from(authBytes).toString('hex'));