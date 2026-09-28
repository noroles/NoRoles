// Creates a company folder from the template.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { sha, readText, META_FILES } from './company.js';
import { commit } from './requests.js';

const TEMPLATE = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'templates', 'company');

const gitConfig = (key) => { try { return execFileSync('git', ['config', key]).toString().trim(); } catch { return ''; } };
const slug = (s) => String(s).toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'me';

function copy(src, dst, vars) {
  for (const e of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, e.name);
    const d = path.join(dst, e.name === 'gitignore.txt' ? '.gitignore' : e.name);
    if (e.isDirectory()) { fs.mkdirSync(d, { recursive: true }); copy(s, d, vars); continue; }
    if (fs.existsSync(d)) continue;
    fs.writeFileSync(d, fs.readFileSync(s, 'utf8').replace(/\{\{(\w+)\}\}/g, (_, k) => vars[k] ?? ''));
  }
}

export function init(dir, { name, email, now = new Date() } = {}) {
  if (fs.existsSync(path.join(dir, 'permissions.md'))) throw new Error(`${dir} already has permissions.md`);
  name ||= gitConfig('user.name') || 'Me';
  email ||= gitConfig('user.email') || '';
  const expires = new Date(now.getTime() + 30 * 86400000).toISOString().slice(0, 10);
  fs.mkdirSync(dir, { recursive: true });
  copy(TEMPLATE, dir, { ID: slug(name.split(' ')[0]), NAME: name, EMAIL: email, EXPIRES: expires });
  fs.mkdirSync(path.join(dir, '.noroles'), { recursive: true });
  const secrets = path.join(dir, '.noroles', 'secrets.env');
  if (!fs.existsSync(secrets)) fs.writeFileSync(secrets, '# KEY=value, one per line. This file never leaves this computer.\n', { mode: 0o600 });
  const meta = {};
  for (const f of META_FILES) if (fs.existsSync(path.join(dir, f))) meta[f] = sha(readText(dir, f));
  fs.writeFileSync(path.join(dir, 'ledger.json'), JSON.stringify({ meta }, null, 2) + '\n');
  if (!fs.existsSync(path.join(dir, '.git'))) execFileSync('git', ['init', '-q'], { cwd: dir });
  commit(dir, 'start the company (genesis: root writes root.md, permissions.md and rules directly)', { all: true });
  return { id: slug(name.split(' ')[0]), name, email };
}
