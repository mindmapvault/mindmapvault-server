#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const thisFile = fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(thisFile), '..');

const allowlist = new Set([
  '.env.example',
  '.env.ci.example',
]);

const patterns = [
  { name: 'provider-secret-key-pattern-a', regex: /\bsk_(live|test)_[0-9A-Za-z]{16,}\b/ },
  { name: 'provider-secret-key-pattern-b', regex: /\bwhsec_[0-9A-Za-z]{16,}\b/ },
  { name: 'private-key-block', regex: /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/ },
  { name: 'aws-access-key-id', regex: /\bAKIA[0-9A-Z]{16}\b/ },
  { name: 'api-token-assignment', regex: /\b([A-Z0-9_]*(API_TOKEN|TOKEN))\s*=\s*(?!replace_with|changeme|xxx|YOUR_)[A-Za-z0-9_-]{20,}\b/ },
  { name: 'secret-assignment', regex: /\b([A-Z0-9_]*(SECRET|KEY))\s*=\s*(?!replace_with|changeme|xxx|YOUR_)[A-Za-z0-9_+/=]{20,}/ },
  { name: 'jwt-secret-assignment', regex: /\b(JWT_SECRET|REFRESH_TOKEN_SECRET)\s*=\s*(?!replace_with|changeme|xxx|YOUR_)[^\s#]{20,}/ },
];

function isBinary(buffer) {
  const len = Math.min(buffer.length, 2048);
  for (let i = 0; i < len; i++) {
    if (buffer[i] === 0) return true;
  }
  return false;
}

function getTrackedFiles() {
  const res = spawnSync('git', ['ls-files'], {
    cwd: repoRoot,
    encoding: 'utf8',
  });
  if (res.status !== 0) {
    console.error(res.stderr || 'Failed to read git tracked files');
    process.exit(2);
  }
  return res.stdout
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean);
}

const findings = [];
for (const rel of getTrackedFiles()) {
  if (allowlist.has(rel)) continue;
  const abs = path.join(repoRoot, rel);
  if (!fs.existsSync(abs)) continue;
  const stat = fs.statSync(abs);
  if (!stat.isFile()) continue;

  const buf = fs.readFileSync(abs);
  if (isBinary(buf)) continue;
  const text = buf.toString('utf8');

  for (const p of patterns) {
    if (p.regex.test(text)) {
      findings.push(`${p.name} detected in ${rel}`);
    }
  }
}

if (findings.length) {
  console.error('Potential committed secrets detected:');
  for (const f of findings) console.error(`- ${f}`);
  process.exit(1);
}

console.log('No committed secret patterns detected in tracked files.');
