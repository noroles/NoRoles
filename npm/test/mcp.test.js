// The MCP gateway: every tool call goes through NoRoles, whatever the agent decides.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { PassThrough } from 'node:stream';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import YAML from 'yaml';
import { init } from '../src/init.js';
import { sha, readText, META_FILES, load } from '../src/company.js';
import { openMandate, decide } from '../src/requests.js';
import { gateway, guard, classify } from '../src/mcp.js';
import { keygen } from '../src/keys.js';

const FAKE = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures', 'fake-server.js');
const PASS = 'correct horse battery';
process.env.NOROLES_NOTIFY_LOG = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'noroles-notes-')), 'notes.jsonl');
process.env.NOROLES_KEYS = fs.mkdtempSync(path.join(os.tmpdir(), 'noroles-keys-'));
const KEY = keygen('ana', PASS);

function company({ each = 'item', canExport = false } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'noroles-mcp-'));
  execFileSync('git', ['init', '-q'], { cwd: dir });
  execFileSync('git', ['config', 'user.email', 'ana@acme.test'], { cwd: dir });
  execFileSync('git', ['config', 'user.name', 'Ana'], { cwd: dir });
  init(dir, { name: 'Ana', email: 'ana@acme.test' });
  const log = path.join(dir, 'fake.log');
  fs.writeFileSync(path.join(dir, 'permissions.md'), `\`\`\`yaml
people:
  ana: { email: ana@acme.test, key: "${KEY}" }
agents:
  ana-agent: { works_for: ana }
permissions:
  money.spend:    { holders: [ana], each: ${each}, limits: { amount: 500 } }
  speak.external: { holders: [ana], each: item }
  destroy:        { holders: [ana], each: item }
  data.export:    { holders: [ana], each: item }
  rule.change:    { holders: [ana], quorum: 2 }
\`\`\`
`);
  fs.writeFileSync(path.join(dir, 'credentials.md'), '```yaml\ncredentials:\n  fake_key: { env: FAKE_SECRET, exercises: [money.spend] }\n```\n');
  fs.writeFileSync(path.join(dir, 'servers.md'), `\`\`\`yaml
servers:
  fake:
    command: node
    args: ["${FAKE}"]
    env: { FAKE_KEY: fake_key }
    plain_env: { FAKE_LOG: "${log}" }
    tools:
      pay: { permissions: [money.spend], amount: amount, to: vendor }
      send_email: { permissions: [speak.external] }
\`\`\`
`);
  fs.writeFileSync(path.join(dir, '.noroles', 'secrets.env'), 'FAKE_SECRET=the-real-key\n');
  fs.rmSync(path.join(dir, 'mandates', 'first-website.md'));
  const can = { 'money.spend': { amount: 100 }, servers: ['fake'] };
  if (canExport) can['data.export'] = {};
  fs.writeFileSync(path.join(dir, 'mandates', 'ops.md'), `---\n${YAML.stringify({ intent: 'run ops', metric: 'done', stop_if: 'never', holder: 'ana', executor: 'ana-agent', owns: ['ops'], can, expires: new Date(Date.now() + 20 * 864e5).toISOString().slice(0, 10) })}---\n`);
  const meta = {}; for (const f of META_FILES) if (fs.existsSync(path.join(dir, f))) meta[f] = sha(readText(dir, f));
  fs.writeFileSync(path.join(dir, 'ledger.json'), JSON.stringify({ meta }));
  const o = openMandate(dir, { mandate: 'ops', asker: 'ana' });
  decide(dir, { id: o.id, who: 'ana', yes: true, passphrase: PASS });
  return { dir, log };
}

async function connect(dir) {
  const input = new PassThrough(), output = new PassThrough();
  const g = await gateway(dir, { mandate: 'ops', as: 'ana-agent', input, output });
  let id = 0; const waiting = new Map(); let buf = '';
  output.on('data', (d) => { buf += d; let i; while ((i = buf.indexOf('\n')) >= 0) { const m = JSON.parse(buf.slice(0, i)); buf = buf.slice(i + 1); waiting.get(m.id)?.(m); } });
  const rpc = (method, params) => new Promise((res) => { const n = ++id; waiting.set(n, res); input.write(JSON.stringify({ jsonrpc: '2.0', id: n, method, params }) + '\n'); });
  const callTool = async (name, args) => (await rpc('tools/call', { name, arguments: args })).result;
  return { rpc, callTool, stop: g.stop };
}
const reached = (log) => (fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);
const say = (r) => r.content.map((p) => p.text).join('\n');

test('classify: read-only is open, destructive needs destroy, unmapped writes are refused', () => {
  const sv = { tools: { pay: { permissions: ['money.spend'] } } };
  assert.deepEqual(classify(sv, { name: 'read', annotations: { readOnlyHint: true } }), []);
  assert.deepEqual(classify(sv, { name: 'wipe', annotations: { destructiveHint: true } }), ['destroy']);
  assert.equal(classify(sv, { name: 'mystery' }), null);
  assert.deepEqual(classify(sv, { name: 'pay', annotations: { destructiveHint: true } }), ['money.spend'], 'an explicit mapping wins');
  assert.deepEqual(classify(sv, { name: 'mkdir', annotations: { readOnlyHint: false, destructiveHint: false, openWorldHint: false } }), []);
  assert.equal(classify(sv, { name: 'post', annotations: { readOnlyHint: false, destructiveHint: false, openWorldHint: true } }), null);
});

test('gateway: tools are listed with what they need', async () => {
  const { dir } = company();
  const g = await connect(dir);
  const { result } = await g.rpc('tools/list', {});
  const d = Object.fromEntries(result.tools.map((t) => [t.name, t.description]));
  assert.match(d.fake__read_note, /Read a note$/);
  assert.match(d.fake__pay, /needs a yes: money.spend/);
  assert.match(d.fake__mystery, /refused, not mapped/);
  g.stop();
});

test('gateway: an open call runs at once', async () => {
  const { dir, log } = company();
  const g = await connect(dir);
  assert.equal(say(await g.callTool('fake__read_note', {})), 'read_note ok');
  assert.equal(reached(log).length, 1);
  g.stop();
});

test('gateway: a lasting call does not reach the tool until a person signs; then it runs once', async () => {
  const { dir, log } = company();
  const g = await connect(dir);
  const first = say(await g.callTool('fake__pay', { amount: 40, vendor: 'Registrar Inc' }));
  const id = /Asked as (r-[\w-]+)/.exec(first)[1];
  assert.equal(reached(log).length, 0, 'nothing reached the tool before the yes');
  assert.match(say(await g.callTool('fake__pay', { amount: 40, vendor: 'Registrar Inc' })), /still waiting/);
  const r = load(dir).requests[id];
  assert.equal(r.action.amount, 40);
  assert.equal(r.action.to, 'Registrar Inc');
  decide(dir, { id, who: 'ana', yes: true, passphrase: PASS });
  assert.equal(say(await g.callTool('fake__pay', { amount: 40, vendor: 'Registrar Inc' })), 'pay ok');
  const calls = reached(log);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].key, 'the-real-key', 'the upstream server got its key; the agent never did');
  assert.equal(load(dir).requests[id].status, 'done');
  // the same arguments again are a new request, not a reuse of the old yes
  assert.match(say(await g.callTool('fake__pay', { amount: 40, vendor: 'Registrar Inc' })), /needs a yes/);
  g.stop();
});

test('gateway: a yes covers only the exact arguments that were asked', async () => {
  const { dir, log } = company();
  const g = await connect(dir);
  const id = /Asked as (r-[\w-]+)/.exec(say(await g.callTool('fake__pay', { amount: 40, vendor: 'Registrar Inc' })))[1];
  decide(dir, { id, who: 'ana', yes: true, passphrase: PASS });
  assert.match(say(await g.callTool('fake__pay', { amount: 40, vendor: 'Attacker LLC' })), /needs a yes/);
  assert.equal(reached(log).length, 0);
  g.stop();
});

test('gateway: limits still apply, and unmapped writes are refused outright', async () => {
  const { dir, log } = company();
  const g = await connect(dir);
  assert.match(say(await g.callTool('fake__pay', { amount: 150, vendor: 'X' })), /above the limit 100/);
  assert.match(say(await g.callTool('fake__mystery', {})), /refused/);
  assert.match(say(await g.callTool('fake__wipe', {})), /cannot destroy/);
  assert.equal(reached(log).length, 0);
  g.stop();
});

test('gateway: with each: mandate, calls inside the limits run without asking again', async () => {
  const { dir, log } = company({ each: 'mandate' });
  const g = await connect(dir);
  assert.equal(say(await g.callTool('fake__pay', { amount: 30, vendor: 'Registrar Inc' })), 'pay ok');
  assert.equal(reached(log).length, 1);
  g.stop();
});

test('gateway: a stopped mandate stops every call', async () => {
  const { dir, log } = company();
  const { stop } = await import('../src/requests.js');
  const g = await connect(dir);
  stop(dir, { mandate: 'ops', asker: 'ana-agent', reason: 'looks wrong' });
  assert.match(say(await g.callTool('fake__read_note', {})), /not active/);
  assert.equal(reached(log).length, 0);
  g.stop();
});

test('output guard: personal data is removed unless the mandate can data.export; keys always are', async () => {
  const { dir } = company();
  const g = await connect(dir);
  const out = say(await g.callTool('fake__get_customer', {}));
  assert.doesNotMatch(out, /ana\.lima@example\.com|4242 4242|GB82WEST|7946|sk_live_/);
  assert.match(out, /Ana Lima/);
  assert.match(out, /removed 1 secret, 1 card, 1 iban, 1 email, 1 phone/);
  g.stop();
  const e = company({ canExport: true });
  const g2 = await connect(e.dir);
  const out2 = say(await g2.callTool('fake__get_customer', {}));
  assert.match(out2, /ana\.lima@example\.com/);
  assert.doesNotMatch(out2, /sk_live_/);
  g2.stop();
});

test('output guard: a number that fails the card checksum is left alone', () => {
  assert.equal(guard('order 1234 5678 9012 3456', { canExport: false }).text, 'order 1234 5678 9012 3456');
});

test('output guard: structured results are checked too, and unknown fields are dropped', async () => {
  const { dir } = company();
  const g = await connect(dir);
  const r = await g.callTool('fake__get_customer_json', {});
  const all = JSON.stringify(r);
  assert.doesNotMatch(all, /ana\.lima@example\.com|sk_live_/);
  assert.equal(r.structuredContent.result[0].name, 'Ana Lima');
  assert.equal(r._meta, undefined);
  g.stop();
});
