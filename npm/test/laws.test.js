// Each test is a law from SPEC.md or an attack found in the paper test.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import YAML from 'yaml';
import { init } from '../src/init.js';
import { load, check, mandateState, sha, readText, META_FILES } from '../src/company.js';
import { ask, openMandate, proposeMeta, decide, run, doAction, settle, stop, resume, audit, incidents, resolveIncident } from '../src/requests.js';
import { keygen } from '../src/keys.js';
import { flags } from '../src/flags.js';

const PASS = 'correct horse battery';
const NOTES = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'noroles-notes-')), 'notes.jsonl');
process.env.NOROLES_NOTIFY_LOG = NOTES;
const notes = () => (fs.existsSync(NOTES) ? fs.readFileSync(NOTES, 'utf8').trim().split('\n').map((l) => JSON.parse(l)) : []);
process.env.NOROLES_KEYS = fs.mkdtempSync(path.join(os.tmpdir(), 'noroles-keys-'));
const KEYS = { ana: keygen('ana', PASS), ben: keygen('ben', PASS) };

const H = 3600000;
const T0 = new Date('2026-10-01T10:00:00Z');
const at = (h) => new Date(T0.getTime() + h * H);

function company({ quorum = 1, each = 'item' } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'noroles-'));
  execFileSync('git', ['init', '-q'], { cwd: dir });
  execFileSync('git', ['config', 'user.email', 'ana@acme.test'], { cwd: dir });
  execFileSync('git', ['config', 'user.name', 'Ana'], { cwd: dir });
  init(dir, { name: 'Ana', email: 'ana@acme.test', now: T0 });
  // genesis: root writes the starting files directly
  fs.writeFileSync(path.join(dir, 'permissions.md'), `# Permissions
\`\`\`yaml
people:
  ana: { email: ana@acme.test, key: "${KEYS.ana}" }
  ben: { email: ben@acme.test, key: "${KEYS.ben}" }
agents:
  ana-agent: { works_for: ana }
groups:
  founders: [ana, ben]
permissions:
  money.spend:   { holders: [founders], quorum: ${quorum}, each: ${each}, limits: { amount: 500, per_period: 550, period: month }, respond_within: 48h }
  money.pay_out: { holders: [founders], quorum: 2 }
  speak.external: { holders: [ana] }
  rule.change:   { holders: [founders], quorum: 2 }
\`\`\`
`);
  fs.writeFileSync(path.join(dir, 'credentials.md'), `# Credentials
\`\`\`yaml
credentials:
  github_read: { env: GITHUB_TOKEN, exercises: [] }
  card:        { env: CARD_KEY, exercises: [money.spend] }
  stripe:      { env: STRIPE_KEY, exercises: [money.spend, money.pay_out] }
\`\`\`
`);
  fs.writeFileSync(path.join(dir, '.noroles', 'secrets.env'), 'GITHUB_TOKEN=gh-open\nCARD_KEY=card-secret\nSTRIPE_KEY=stripe-secret\n');
  fs.rmSync(path.join(dir, 'mandates', 'first-website.md'));
  mandate(dir, 'site', { can: { 'money.spend': { amount: 100 }, tools: ['github_read', 'card'] }, owns: ['website'] });
  const meta = {};
  for (const f of META_FILES) meta[f] = sha(readText(dir, f));
  fs.writeFileSync(path.join(dir, 'ledger.json'), JSON.stringify({ meta }));
  return dir;
}

function mandate(dir, name, extra = {}) {
  const fm = { intent: `do ${name}`, metric: 'done by expiry', stop_if: 'we change plans', holder: 'ana', executor: 'ana-agent', expires: '2026-10-20', ...extra };
  fs.writeFileSync(path.join(dir, 'mandates', `${name}.md`), `---\n${YAML.stringify(fm)}---\nBody.\n`);
}

function opened(dir, name = 'site') {
  const r = openMandate(dir, { mandate: name, asker: 'ana', now: T0 });
  if (r.status !== 'approved') decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });
}

const errors = (dir) => check(load(dir), T0).filter((x) => x.level === 'error');
const spend = (dir, amount, extra = {}) => ask(dir, { mandate: 'site', permissions: ['money.spend'], asker: 'ana-agent', summary: `buy domain for ${amount}`, amount, to: 'Registrar Inc', now: T0, ...extra });

test('a fresh company passes the laws', () => {
  assert.deepEqual(errors(company()), []);
});

test('law 4: nothing lasting happens inside a mandate nobody opened', () => {
  const dir = company();
  assert.throws(() => spend(dir, 20), /not active: not opened yet/);
});

test('opening a mandate asks the holders of every permission in can', () => {
  const dir = company();
  const r = openMandate(dir, { mandate: 'site', asker: 'ana', now: T0 });
  assert.equal(r.status, 'pending');
  assert.deepEqual(r.permissions, ['money.spend']);
  decide(dir, { id: r.id, who: 'ben', yes: true, passphrase: PASS, now: T0 });
  assert.equal(mandateState(load(dir), 'site', T0).active, true);
});

test('only people say yes: an agent cannot approve', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 20);
  assert.throws(() => decide(dir, { id: r.id, who: 'ana-agent', yes: true, passphrase: PASS, now: T0 }), /not a person/);
});

test('limits: the lower of the mandate and permissions.md wins', () => {
  const dir = company(); opened(dir);
  assert.throws(() => spend(dir, 150), /above the limit 100/);
});

test('limits: per_period is summed across mandates', () => {
  const dir = company();
  mandate(dir, 'ads', { can: { 'money.spend': { amount: 400 } }, owns: ['ads'] });
  opened(dir); opened(dir, 'ads');
  const a = ask(dir, { mandate: 'ads', permissions: ['money.spend'], asker: 'ana-agent', summary: 'ads', amount: 400, now: T0 });
  decide(dir, { id: a.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });
  const b = spend(dir, 100);
  decide(dir, { id: b.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });
  assert.throws(() => spend(dir, 100), /500 already used this month/);
});

test('a request edited after it was asked is void', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 20);
  const f = path.join(dir, 'requests', `${r.id}.yaml`);
  const doc = YAML.parse(fs.readFileSync(f, 'utf8'));
  doc.action.to = 'Attacker LLC';
  fs.writeFileSync(f, YAML.stringify(doc));
  assert.throws(() => decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: T0 }), /edited after it was asked/);
});

test('a mandate edited after its yes is no longer active', () => {
  const dir = company(); opened(dir);
  mandate(dir, 'site', { can: { 'money.spend': { amount: 100 }, tools: ['github_read', 'card'] }, owns: ['website'], intent: 'something wider' });
  const s = mandateState(load(dir), 'site', T0);
  assert.equal(s.active, false);
  assert.match(s.why, /changed since it was approved/);
});

test('law 6: two mandates cannot own the same thing', () => {
  const dir = company();
  mandate(dir, 'rival', { owns: ['website'] });
  assert.ok(errors(dir).some((e) => /claimed by both/.test(e.msg)));
});

test('law 2: a tool that can exercise a permission needs that permission in can', () => {
  const dir = company();
  mandate(dir, 'site', { can: { 'money.spend': { amount: 100 }, tools: ['stripe'] }, owns: ['website'] });
  assert.ok(errors(dir).some((e) => /stripe" can exercise money.pay_out/.test(e.msg)));
});

test('a mandate lasts at most 90 days', () => {
  const dir = company();
  mandate(dir, 'site', { owns: ['website'], opened: '2026-10-01', expires: '2027-03-01' });
  assert.ok(errors(dir).some((e) => /at most 90/.test(e.msg)));
});

test('parts stay inside their parent', () => {
  const dir = company();
  mandate(dir, 'site', { can: { 'money.spend': { amount: 100 }, tools: ['github_read', 'card'] }, owns: ['website'], parts: ['blog'] });
  mandate(dir, 'blog', { can: { 'speak.external': {} }, owns: ['blog'] });
  const e = errors(dir).map((x) => x.msg).join('\n');
  assert.match(e, /can speak.external, which its parent site does not have/);
  assert.match(e, /owns "blog", outside its parent site/);
});

test('needs cannot form a cycle', () => {
  const dir = company();
  mandate(dir, 'a', { needs: ['b'], owns: ['a'] });
  mandate(dir, 'b', { needs: ['a'], owns: ['b'] });
  assert.ok(errors(dir).some((e) => /needs cycle/.test(e.msg)));
});

test('meta files: editing permissions.md by hand stops everything until two people accept it', () => {
  const dir = company();
  const f = path.join(dir, 'permissions.md');
  fs.writeFileSync(f, fs.readFileSync(f, 'utf8').replace('amount: 500', 'amount: 50000'));
  assert.ok(errors(dir).some((e) => e.where === 'permissions.md'));
  const r = proposeMeta(dir, { asker: 'ana', now: T0 });
  // the proposer never counts: ben's yes plus a 24h wait for the missing second person
  assert.throws(() => decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: T0 }), /holds none/);
  decide(dir, { id: r.id, who: 'ben', yes: true, passphrase: PASS, now: T0 });
  settle(dir, at(12));
  assert.ok(errors(dir).some((e) => e.where === 'permissions.md'), 'still waiting');
  settle(dir, at(25));
  assert.deepEqual(errors(dir), []);
});

test('meta files: a file changed again during the wait voids the change', () => {
  const dir = company();
  const f = path.join(dir, 'permissions.md');
  fs.writeFileSync(f, fs.readFileSync(f, 'utf8').replace('amount: 500', 'amount: 600'));
  const r = proposeMeta(dir, { asker: 'ana', now: T0 });
  decide(dir, { id: r.id, who: 'ben', yes: true, passphrase: PASS, now: T0 });
  fs.writeFileSync(f, fs.readFileSync(f, 'utf8').replace('amount: 600', 'amount: 99999'));
  settle(dir, at(25));
  assert.equal(load(dir).requests[r.id].status, 'void');
  assert.ok(errors(dir).some((e) => e.where === 'permissions.md'));
});

test('quorum counts distinct humans and never the asker', () => {
  const dir = company({ quorum: 2 });
  const o = openMandate(dir, { mandate: 'site', asker: 'ana', now: T0 });
  // ana asked, so at quorum 2 only ben counts; one founder short means a 24h wait
  decide(dir, { id: o.id, who: 'ben', yes: true, passphrase: PASS, now: T0 });
  assert.equal(load(dir).requests[o.id].status, 'pending');
  assert.throws(() => decide(dir, { id: o.id, who: 'ana', yes: true, passphrase: PASS, now: T0 }), /holds none/);
  settle(dir, at(25));
  assert.equal(load(dir).requests[o.id].status, 'approved');
});

test('an agent asking counts as its person: at quorum 2 that person cannot approve', () => {
  const dir = company({ quorum: 2 });
  const o = openMandate(dir, { mandate: 'site', asker: 'ben', now: T0 });
  decide(dir, { id: o.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });
  settle(dir, at(25));
  const r = spend(dir, 20, { now: at(25) });
  assert.throws(() => decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: at(25) }), /holds none/);
});

test('silence passes to root, then counts as a no', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 20);
  settle(dir, at(97));
  assert.equal(load(dir).requests[r.id].status, 'expired');
  assert.throws(() => doAction(dir, { id: r.id, now: at(97) }), /expired|not approved/);
});

test('do: an approved action runs once, with only its own key', () => {
  const dir = company(); opened(dir);
  const out = path.join(dir, 'out.txt');
  const r = spend(dir, 20, { tool: 'card', command: ['sh', '-c', `echo "card=$CARD_KEY stripe=$STRIPE_KEY gh=$GITHUB_TOKEN" > ${out}`] });
  assert.throws(() => doAction(dir, { id: r.id, now: T0 }), /not approved/);
  decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });
  assert.equal(doAction(dir, { id: r.id, now: T0 }), 0);
  assert.equal(fs.readFileSync(out, 'utf8').trim(), 'card=card-secret stripe= gh=');
  assert.throws(() => doAction(dir, { id: r.id, now: T0 }), /already carried out/);
});

test('run: an executor gets only the open keys of its mandate, never a paying one', () => {
  const dir = company(); opened(dir);
  const out = path.join(dir, 'env.txt');
  process.env.CARD_KEY = 'leaked-from-parent';
  const code = run(dir, { mandate: 'site', as: 'ana-agent', argv: ['sh', '-c', `echo "gh=$GITHUB_TOKEN card=$CARD_KEY who=$NOROLES_EXECUTOR" > ${out}`], now: T0 });
  delete process.env.CARD_KEY;
  assert.equal(code, 0);
  assert.equal(fs.readFileSync(out, 'utf8').trim(), 'gh=gh-open card= who=ana-agent');
});

test('the record is git: every step is a commit', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 20);
  decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });
  const log = execFileSync('git', ['log', '--format=%s'], { cwd: dir }).toString();
  assert.match(log, new RegExp(`yes ${r.id} by ana \\(approved\\)`));
  assert.match(log, new RegExp(`ask ${r.id}`));
});

test('signatures: a yes written into the file by hand does not count', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 20, { command: ['true'] });
  const f = path.join(dir, 'requests', `${r.id}.yaml`);
  const doc = YAML.parse(fs.readFileSync(f, 'utf8'));
  doc.approvals.push({ by: 'ana', at: T0.toISOString(), sig: 'AAAA' });
  doc.status = 'approved';
  fs.writeFileSync(f, YAML.stringify(doc));
  assert.throws(() => doAction(dir, { id: r.id, now: T0 }), /not approved/);
  assert.ok(audit(dir, T0).some((x) => /not validly signed/.test(x.what)));
});

test('signatures: the wrong passphrase signs nothing', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 20);
  assert.throws(() => decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: 'wrong-passphrase', now: T0 }));
  assert.equal(load(dir).requests[r.id].approvals.length, 0);
});

test('signatures: a yes cannot be moved to another request', () => {
  const dir = company(); opened(dir);
  const a = spend(dir, 20);
  const b = spend(dir, 90, { summary: 'something else' });
  decide(dir, { id: a.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });
  const yes = load(dir).requests[a.id].approvals[0];
  const f = path.join(dir, 'requests', `${b.id}.yaml`);
  const doc = YAML.parse(fs.readFileSync(f, 'utf8'));
  doc.approvals.push(yes);
  fs.writeFileSync(f, YAML.stringify(doc));
  assert.throws(() => doAction(dir, { id: b.id, now: T0 }), /not approved/);
});

test('hashes cover nested fields: editing a payload deep inside is caught', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 20, { payload: { invoice: { iban: 'DE00 1111' } } });
  const f = path.join(dir, 'requests', `${r.id}.yaml`);
  const doc = YAML.parse(fs.readFileSync(f, 'utf8'));
  doc.action.payload.invoice.iban = 'XX99 6666';
  fs.writeFileSync(f, YAML.stringify(doc));
  assert.throws(() => decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: T0 }), /edited after it was asked/);
});

test('each: mandate: the yes that opened the mandate covers actions inside its limits', () => {
  const dir = company({ each: 'mandate' }); opened(dir);
  const out = path.join(dir, 'paid.txt');
  const r = spend(dir, 20, { tool: 'card', command: ['sh', '-c', `echo "$CARD_KEY" > ${out}`] });
  assert.equal(r.status, 'approved');
  assert.ok(r.covered_by);
  assert.equal(doAction(dir, { id: r.id, now: T0 }), 0);
  assert.equal(fs.readFileSync(out, 'utf8').trim(), 'card-secret');
  assert.throws(() => spend(dir, 150), /above the limit/);
});

test('each: a mandate may tighten how often a yes is needed, never loosen it', () => {
  const dir = company({ each: 'item' });
  mandate(dir, 'site', { can: { 'money.spend': { amount: 100, each: 'mandate' }, tools: ['github_read', 'card'] }, owns: ['website'] });
  assert.ok(errors(dir).some((e) => /looser than permissions.md/.test(e.msg)));
});

test('each: item means one action per request; batch shows every item', () => {
  const dir = company({ each: 'item' }); opened(dir);
  assert.throws(() => spend(dir, 20, { items: ['a.com', 'b.com'] }), /each: item/);
  const dir2 = company({ each: 'batch' }); opened(dir2);
  const r = spend(dir2, 40, { items: ['a.com for 20', 'b.com for 20'] });
  assert.deepEqual(r.action.items, ['a.com for 20', 'b.com for 20']);
  assert.equal(r.status, 'pending');
});

test('break glass: anyone can stop a mandate; only its holder lifts the stop', () => {
  const dir = company(); opened(dir);
  stop(dir, { mandate: 'site', asker: 'ana-agent', reason: 'charges look wrong', now: T0 });
  assert.match(mandateState(load(dir), 'site', T0).why, /stopped by ana-agent/);
  assert.throws(() => run(dir, { mandate: 'site', as: 'ana-agent', argv: ['true'], now: T0 }), /not active/);
  assert.ok(incidents(dir).some((i) => /charges look wrong/.test(i.what)));
  const r = resume(dir, { mandate: 'site', asker: 'ana-agent', now: T0 });
  assert.throws(() => decide(dir, { id: r.id, who: 'ben', yes: true, passphrase: PASS, now: T0 }), /holds none/);
  decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });
  assert.equal(mandateState(load(dir), 'site', T0).active, true);
});

test('law 5: a missed due opens an incident for the holder, who resolves or explains it', () => {
  const dir = company();
  mandate(dir, 'site', { can: { 'money.spend': { amount: 100 }, tools: ['github_read', 'card'] }, owns: ['website'], due: [{ date: '2026-10-05', what: 'renew the domain' }] });
  settle(dir, new Date('2026-10-06T00:00:00Z'));
  const inc = incidents(dir).find((i) => /renew the domain/.test(i.what));
  assert.ok(inc);
  assert.equal(inc.holder, 'ana');
  assert.throws(() => resolveIncident(dir, { id: inc.id, who: 'ben', note: 'x' }), /only ana or root/);
  resolveIncident(dir, { id: inc.id, who: 'ana', note: 'renewed a day late' });
  assert.ok(incidents(dir).find((i) => i.id === inc.id).resolved);
});

test('audit: a commit to the record made outside NoRoles is an incident', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 20);
  fs.appendFileSync(path.join(dir, 'requests', `${r.id}.yaml`), '# hi\n');
  execFileSync('git', ['commit', '-qam', 'tidy up'], { cwd: dir });
  assert.ok(audit(dir, T0).some((x) => /outside NoRoles/.test(x.what)));
});

test('the record: NoRoles commits only its own files, never other work in the folder', () => {
  const dir = company(); opened(dir);
  fs.writeFileSync(path.join(dir, 'draft.html'), '<h1>work in progress</h1>');
  const r = spend(dir, 20);
  const files = execFileSync('git', ['show', '--name-only', '--format=', 'HEAD'], { cwd: dir }).toString().trim().split('\n');
  assert.deepEqual(files, [`requests/${r.id}.yaml`]);
  assert.match(execFileSync('git', ['status', '--porcelain'], { cwd: dir }).toString(), /\?\? draft.html/);
});

// ---------- flags: what is unusual, so a yes takes seconds ----------

const yesTo = (dir, r) => decide(dir, { id: r.id, who: 'ana', yes: true, passphrase: PASS, now: T0 });

test('flags: a payee never paid before is flagged; once paid, it is not', () => {
  const dir = company(); opened(dir);
  const first = spend(dir, 10);
  assert.ok(flags(load(dir), first).some((f) => /new payee: Registrar Inc/.test(f)));
  assert.ok(flags(load(dir), first).some((f) => /first time mandate site uses money.spend/.test(f)));
  yesTo(dir, first);
  const second = spend(dir, 10, { now: at(1) });
  assert.deepEqual(flags(load(dir), second), []);
});

test('flags: an amount far above the usual is flagged', () => {
  const dir = company(); opened(dir);
  for (let i = 0; i < 3; i++) yesTo(dir, spend(dir, 10, { now: at(i) }));
  const big = spend(dir, 40, { now: at(4) });
  assert.ok(flags(load(dir), big).some((f) => /4\.0× the usual money.spend \(median 10\)/.test(f)));
});

test('flags: asking again for exactly what was declined is flagged', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 10);
  decide(dir, { id: r.id, who: 'ana', yes: false, reason: 'wrong registrar', passphrase: PASS, now: T0 });
  const again = spend(dir, 10, { now: at(1) });
  assert.ok(flags(load(dir), again).some((f) => /declined on 2026-10-01 by ana: wrong registrar/.test(f)));
});

test('flags: a burst of requests from one agent is flagged', () => {
  const dir = company(); opened(dir);
  let r;
  for (let i = 0; i < 6; i++) r = spend(dir, 1, { now: new Date(T0.getTime() + i * 60000) });
  assert.ok(flags(load(dir), r).some((f) => /request 6 from ana-agent in the last hour/.test(f)));
});

test('flags cannot be hidden by editing the request file', () => {
  const dir = company(); opened(dir);
  const r = spend(dir, 10);
  const f = path.join(dir, 'requests', `${r.id}.yaml`);
  fs.writeFileSync(f, fs.readFileSync(f, 'utf8') + 'flags: []\n');
  assert.ok(flags(load(dir), load(dir).requests[r.id]).length > 0);
});

test('notify: a request that waits for a yes tells the person; money is urgent', () => {
  const dir = company(); opened(dir);
  const before = notes().length;
  const r = spend(dir, 10);
  const n = notes().slice(before);
  assert.equal(n.length, 1);
  assert.match(n[0].title, /ana-agent needs a yes/);
  assert.match(n[0].body, new RegExp(`noroles yes ${r.id}`));
  assert.match(n[0].body, /! new payee/);
  assert.equal(n[0].urgent, true);
});
