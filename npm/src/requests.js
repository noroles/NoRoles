// Asking for a yes, recording it honestly, and acting only on what was approved.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import YAML from 'yaml';
import {
  load, check, mandateCan, mandateState, sha, canonical, readText, META_FILES,
  effective, progress, complete, limitProblem, isApproved, eachFor, validAnswer, principal,
} from './company.js';
import { sign, statement } from './keys.js';
import { flags } from './flags.js';
import { notify } from './notify.js';

const DAY = 86400000;
export const actionHash = (a) => sha(canonical(a));
export { effective, progress };

function save(c, r) {
  fs.mkdirSync(path.join(c.dir, 'requests'), { recursive: true });
  fs.writeFileSync(path.join(c.dir, 'requests', `${r.id}.yaml`), YAML.stringify(r));
  c.requests[r.id] = r;
}

// NoRoles commits only its own record, never other work lying around in the folder.
const RECORD = ['requests', 'incidents', 'log', 'ledger.json'];

export function commit(dir, msg, { all = false } = {}) {
  if (!fs.existsSync(path.join(dir, '.git'))) return;
  const paths = all ? ['-A'] : ['--', ...RECORD.filter((p) => fs.existsSync(path.join(dir, p)))];
  if (!all && paths.length === 1) return;
  execFileSync('git', ['add', ...paths], { cwd: dir });
  const staged = execFileSync('git', ['diff', '--cached', '--name-only', ...(all ? [] : paths)], { cwd: dir }).toString().trim();
  if (!staged) return;
  execFileSync('git', ['commit', '-q', '-m', `noroles: ${msg}`, ...(all ? [] : ['--', ...staged.split('\n')])], { cwd: dir });
}

const newId = (now, p = 'r') => `${p}-${now.toISOString().slice(0, 10).replace(/-/g, '')}-${crypto.randomBytes(3).toString('hex')}`;

function base(kind, { mandate, permissions, asker, action, now }) {
  return { id: newId(now), kind, mandate, permissions, asker, created: now.toISOString(), status: 'pending', action, action_hash: actionHash(action), approvals: [], denials: [] };
}

function finish(c, r) {
  r.status = 'approved';
  if (r.kind === 'meta') {
    c.ledger.meta = { ...c.ledger.meta, ...r.action.hashes };
    fs.writeFileSync(path.join(c.dir, 'ledger.json'), JSON.stringify(c.ledger, null, 2) + '\n');
  }
}

// ---------- asking ----------

export function ask(dir, { mandate, permissions, asker, summary, amount, currency, to, payload, items, tool, command, now = new Date() }) {
  const c = load(dir);
  const m = c.mandates[mandate];
  if (!m) throw new Error(`no mandate "${mandate}"`);
  if (!c.people[asker] && !c.agents[asker]) throw new Error(`"${asker}" is not a known person or agent`);
  if (![].concat(m.executor || []).includes(asker) && m.holder !== asker) throw new Error(`"${asker}" is not an executor or holder of ${mandate}`);
  const state = mandateState(c, mandate, now);
  if (!state.active) throw new Error(`mandate ${mandate} is not active: ${state.why}`);
  const perms = [].concat(permissions || []);
  if (!perms.length) throw new Error('name at least one permission');
  const list = items ? [].concat(items).map(String) : null;
  const action = { summary: String(summary || ''), amount: amount == null ? null : Number(amount), currency: currency || null, to: to || null, payload: payload ?? null, items: list, tool: tool || null, command: command || null };
  const r = base('action', { mandate, permissions: perms, asker, action, now });
  const problem = limitProblem(c, r, now);
  if (problem) throw new Error(problem);
  // each: mandate means the yes that opened the mandate already covers every action inside its limits
  if (perms.every((p) => eachFor(c, m, p) === 'mandate')) {
    const open = Object.values(c.requests).find((o) => o.kind === 'open' && o.mandate === mandate && o.mandate_hash === m.hash && isApproved(c, o, now));
    if (open) { r.covered_by = open.id; r.status = 'approved'; }
  }
  save(c, r);
  commit(dir, `ask ${r.id} ${perms.join(',')} for ${mandate}: ${action.summary}${r.covered_by ? ` (covered by ${r.covered_by})` : ''}`);
  if (r.status === 'pending') {
    const f = flags(c, r);
    const money = perms.some((p) => p.startsWith('money.'));
    notify(`NoRoles: ${asker} needs a yes`, `${perms.join(', ')}: ${action.summary}${action.amount != null ? ` (${action.amount}${action.currency ? ' ' + action.currency : ''})` : ''}${f.length ? `\n! ${f.join('\n! ')}` : ''}\nnoroles yes ${r.id}`, { urgent: money || f.length > 0 });
  }
  return r;
}

export function openMandate(dir, { mandate, asker, now = new Date() }) {
  const c = load(dir);
  const m = c.mandates[mandate];
  if (!m) throw new Error(`no mandate "${mandate}"`);
  const problems = check(c, now).filter((x) => x.level === 'error' && (x.where.includes(`mandates/${mandate}.md`) || x.where.startsWith('owns')));
  if (problems.length) throw new Error(`fix these first:\n${problems.map((p) => `  ${p.where}: ${p.msg}`).join('\n')}`);
  const perms = Object.keys(mandateCan(m).perms);
  const action = { summary: `open mandate ${mandate}: ${m.intent}`, mandate_hash: m.hash, can: m.can || {} };
  const r = { ...base('open', { mandate, permissions: perms, asker, action, now }), mandate_hash: m.hash };
  if (!perms.length) r.status = 'approved';
  save(c, r);
  commit(dir, `open ${mandate} (${r.id})`);
  return r;
}

export function proposeMeta(dir, { asker, now = new Date() }) {
  const c = load(dir);
  const hashes = {};
  for (const f of META_FILES) if (fs.existsSync(path.join(dir, f))) hashes[f] = sha(readText(dir, f));
  const changed = Object.keys(hashes).filter((f) => hashes[f] !== c.ledger.meta?.[f]);
  if (!changed.length) throw new Error('no meta file has changed');
  const r = base('meta', { mandate: null, permissions: ['rule.change'], asker, action: { summary: `accept changes to ${changed.join(', ')}`, hashes }, now });
  save(c, r);
  commit(dir, `propose meta change ${r.id}: ${changed.join(', ')}`);
  return r;
}

/** Break glass: anyone may stop a mandate to limit harm. It never switches a control off. */
export function stop(dir, { mandate, asker, reason, now = new Date() }) {
  const c = load(dir);
  if (!c.mandates[mandate]) throw new Error(`no mandate "${mandate}"`);
  if (!c.people[asker] && !c.agents[asker]) throw new Error(`"${asker}" is not a known person or agent`);
  if (!reason) throw new Error('say why: the holder reviews every stop within 24h');
  const r = { ...base('stop', { mandate, permissions: [], asker, action: { summary: String(reason) }, now }), status: 'approved' };
  save(c, r);
  openIncident(c, { key: `stop-${r.id}`, mandate, what: `stopped by ${asker}: ${reason}. Holder reviews within 24h.`, now });
  commit(dir, `stop ${mandate} by ${asker}: ${reason}`);
  return r;
}

/** Only the mandate's holder (or root) can undo a stop. */
export function resume(dir, { mandate, asker, now = new Date() }) {
  const c = load(dir);
  if (!c.mandates[mandate]) throw new Error(`no mandate "${mandate}"`);
  const r = base('resume', { mandate, permissions: [], asker, action: { summary: `resume ${mandate}` }, now });
  save(c, r);
  commit(dir, `ask to resume ${mandate} (${r.id})`);
  return r;
}

// ---------- answering ----------

export function decide(dir, { id, who, yes, reason, passphrase, sig, now = new Date() }) {
  const c = load(dir);
  const r = c.requests[id];
  if (!r) throw new Error(`no request ${id}`);
  const person = c.people[who];
  if (!person) throw new Error(`"${who}" is not a person: only people say yes or no`);
  const eff = effective(c, r, now);
  if (eff.status !== 'pending') throw new Error(`${id} is ${eff.status}`);
  if (actionHash(r.action) !== r.action_hash) throw new Error(`${id} was edited after it was asked: void`);
  const prog = progress(c, r, now);
  if (!prog.some((p) => p.can.includes(who))) throw new Error(`${who} holds none of ${r.permissions.join(', ') || 'this mandate'} for this request`);
  if (r.approvals.some((a) => a.by === who) || r.denials.some((a) => a.by === who)) throw new Error(`${who} already answered ${id}`);
  const answer = yes ? 'yes' : 'no';
  const at = now.toISOString();
  const entry = { by: who, at };
  if (person.key) {
    entry.sig = sig || (passphrase != null ? sign(who, passphrase, statement(r, answer, at)) : null);
    if (!validAnswer(c, r, entry, answer)) throw new Error(`${who}: the signature does not match the key in permissions.md`);
  }
  if (!yes) {
    r.denials.push({ ...entry, reason: reason || null });
    r.status = 'denied';
    save(c, r);
    commit(dir, `no ${id} by ${who}${reason ? `: ${reason}` : ''}`);
    return r;
  }
  if (r.kind === 'action') {
    const st = mandateState(c, r.mandate, now);
    if (!st.active) throw new Error(`mandate ${r.mandate} is not active: ${st.why}`);
    const p = limitProblem(c, r, now); if (p) throw new Error(p);
  }
  if (r.kind === 'open' && c.mandates[r.mandate]?.hash !== r.mandate_hash) throw new Error(`mandate ${r.mandate} changed after it was asked: ask again`);
  if (r.kind === 'meta') for (const [f, h] of Object.entries(r.action.hashes)) if (sha(readText(dir, f)) !== h) throw new Error(`${f} changed again after this was asked: propose again`);
  r.approvals.push(entry);
  if (complete(progress(c, r, now), now)) finish(c, r);
  save(c, r);
  commit(dir, `yes ${id} by ${who}${r.status === 'approved' ? ' (approved)' : ''}`);
  return r;
}

// ---------- time: waits, silence, dues, reviews ----------

function openIncident(c, { key, mandate, what, now }) {
  const f = path.join(c.dir, 'incidents', `${key}.yaml`);
  if (fs.existsSync(f)) return false;
  const m = c.mandates[mandate];
  fs.mkdirSync(path.dirname(f), { recursive: true });
  fs.writeFileSync(f, YAML.stringify({ id: key, mandate, what, opened: now.toISOString(), holder: m?.holder || c.root.members[0] || null, then: c.root.members, resolved: null }));
  notify('NoRoles: incident', `${mandate ? mandate + ': ' : ''}${what}`, { urgent: true });
  return true;
}

export function incidents(dir) {
  const d = path.join(dir, 'incidents');
  return fs.existsSync(d) ? fs.readdirSync(d).filter((f) => f.endsWith('.yaml')).map((f) => YAML.parse(fs.readFileSync(path.join(d, f), 'utf8'))) : [];
}

export function resolveIncident(dir, { id, who, note, now = new Date() }) {
  const c = load(dir);
  const f = path.join(dir, 'incidents', `${id}.yaml`);
  if (!fs.existsSync(f)) throw new Error(`no incident ${id}`);
  const inc = YAML.parse(fs.readFileSync(f, 'utf8'));
  if (!c.people[who]) throw new Error('only people resolve incidents');
  if (who !== inc.holder && !c.root.members.includes(who)) throw new Error(`only ${inc.holder} or root can resolve ${id}`);
  if (!note) throw new Error('say what happened: resolve or explain');
  inc.resolved = { by: who, at: now.toISOString(), note };
  fs.writeFileSync(f, YAML.stringify(inc));
  commit(dir, `resolve ${id} by ${who}: ${note}`);
  return inc;
}

/** Promote requests whose waits passed, close silent ones as a no, and open incidents for missed dues and reviews. */
export function settle(dir, now = new Date()) {
  const c = load(dir);
  const changed = [];
  for (const r of Object.values(c.requests)) {
    if (r.status !== 'pending') continue;
    if (effective(c, r, now).status === 'expired') {
      r.status = 'expired'; save(c, r); changed.push(`${r.id} expired: silence is a no`);
      if (r.kind === 'action') openIncident(c, { key: `silent-${r.id}`, mandate: r.mandate, what: `nobody answered ${r.id} (${r.action.summary})`, now });
      continue;
    }
    if (!r.approvals.length || !complete(progress(c, r, now), now)) continue;
    const moved = r.kind === 'meta' && Object.entries(r.action.hashes).some(([f, h]) => sha(readText(dir, f)) !== h);
    if (moved || actionHash(r.action) !== r.action_hash) { r.status = 'void'; save(c, r); changed.push(`${r.id} void: changed during the wait`); continue; }
    finish(c, r); save(c, r); changed.push(`${r.id} approved after the wait`);
  }
  for (const m of Object.values(c.mandates)) {
    for (const [i, d] of (m.due || []).entries()) {
      const date = new Date(String(d.date ?? d));
      if (d.notice || !(date < now)) continue;
      if (openIncident(c, { key: `due-${m.name}-${i}-${date.toISOString().slice(0, 10)}`, mandate: m.name, what: `missed due ${date.toISOString().slice(0, 10)}: ${d.what || d}`, now })) changed.push(`${m.name}: missed due`);
    }
    if (m.review && new Date(String(m.review)) < now && mandateState(c, m.name, now).why !== 'no such mandate') {
      if (openIncident(c, { key: `review-${m.name}-${String(m.review).slice(0, 10)}`, mandate: m.name, what: `review date ${String(m.review).slice(0, 10)} passed: root renews or ends it`, now })) changed.push(`${m.name}: review passed`);
    }
  }
  if (changed.length) commit(dir, changed.join('; '));
  return changed;
}

// ---------- acting ----------

export function readSecrets(dir) {
  const f = path.join(dir, '.noroles', 'secrets.env');
  const out = {};
  if (!fs.existsSync(f)) return out;
  for (const line of fs.readFileSync(f, 'utf8').split('\n')) {
    const m = /^\s*([A-Z0-9_]+)\s*=\s*(.*)\s*$/.exec(line);
    if (m) out[m[1]] = m[2].replace(/^["']|["']$/g, '');
  }
  return out;
}

/** Environment with every known credential removed, plus only the ones given. */
function cleanEnv(c, give, extra) {
  const env = { ...process.env };
  for (const cr of Object.values(c.credentials)) delete env[cr.env];
  const secrets = readSecrets(c.dir);
  for (const t of give) {
    const cr = c.credentials[t];
    if (secrets[cr.env] == null) throw new Error(`no value for ${cr.env} in .noroles/secrets.env`);
    env[cr.env] = secrets[cr.env];
  }
  return { ...env, ...extra };
}

function lawErrors(c, now) {
  const errors = check(c, now).filter((x) => x.level === 'error');
  if (errors.length) throw new Error(`the company files break the laws; run \`noroles check\`:\n${errors.map((e) => `  ${e.where}: ${e.msg}`).join('\n')}`);
}

/** Run an executor inside a mandate with only the open tools its mandate names. */
export function run(dir, { mandate, as, argv, now = new Date() }) {
  settle(dir, now);
  const c = load(dir);
  const m = c.mandates[mandate];
  const state = mandateState(c, mandate, now);
  if (!state.active) throw new Error(`mandate ${mandate} is not active: ${state.why}`);
  if (![].concat(m.executor || []).includes(as)) throw new Error(`${as} is not an executor of ${mandate}`);
  lawErrors(c, now);
  const open = mandateCan(m).tools.filter((t) => !(c.credentials[t].exercises || []).length);
  const env = cleanEnv(c, open, { NOROLES_EXECUTOR: as, NOROLES_MANDATE: mandate, NOROLES_DIR: dir });
  logRun(dir, { at: now.toISOString(), mandate, as, argv, tools: open });
  const res = spawnSync(argv[0], argv.slice(1), { stdio: 'inherit', env });
  return res.status ?? 1;
}

/** Carry out one approved action, once, with the single credential it was approved for. */
export function doAction(dir, { id, now = new Date() }) {
  settle(dir, now);
  const c = load(dir);
  const r = c.requests[id];
  if (!r || r.kind !== 'action') throw new Error(`no action request ${id}`);
  if (r.status === 'done') throw new Error(`${id} was already carried out`);
  if (actionHash(r.action) !== r.action_hash) throw new Error(`${id} was edited after it was asked: void`);
  if (!isApproved(c, r, now)) throw new Error(`${id} is ${effective(c, r, now).status}, not approved`);
  const state = mandateState(c, r.mandate, now);
  if (!state.active) throw new Error(`mandate ${r.mandate} is not active: ${state.why}`);
  lawErrors(c, now);
  if (!r.action.command?.length) throw new Error(`${id} has no command to run; the approved action is done by hand`);
  const env = cleanEnv(c, r.action.tool ? [r.action.tool] : [], { NOROLES_REQUEST: id, NOROLES_MANDATE: r.mandate });
  r.status = 'done';
  r.done = { at: new Date().toISOString(), exit: null };
  save(c, r);
  commit(dir, `doing ${id}`);
  const res = spawnSync(r.action.command[0], r.action.command.slice(1), { stdio: 'inherit', env });
  r.done.exit = res.status;
  save(c, r);
  commit(dir, `did ${id} (exit ${res.status})`);
  return res.status ?? 1;
}

function logRun(dir, entry) {
  fs.mkdirSync(path.join(dir, 'log'), { recursive: true });
  fs.appendFileSync(path.join(dir, 'log', 'runs.jsonl'), JSON.stringify(entry) + '\n');
  commit(dir, `run ${entry.mandate} as ${entry.as}: ${entry.argv.join(' ')}`);
}

// ---------- reality ----------

/** Check every recorded answer against its signature and every request against its hash, and find
 * commits to the record that NoRoles did not make. Opens an incident for each finding. */
export function audit(dir, now = new Date()) {
  const c = load(dir);
  const found = [];
  for (const r of Object.values(c.requests)) {
    if (actionHash(r.action) !== r.action_hash) found.push({ key: `tamper-${r.id}`, mandate: r.mandate, what: `${r.id} was edited after it was asked` });
    for (const a of r.approvals || []) if (!validAnswer(c, r, a, 'yes')) found.push({ key: `forged-${r.id}-${a.by}`, mandate: r.mandate, what: `${r.id}: a yes from ${a.by} is not validly signed` });
    if (r.status === 'approved' && !isApproved(c, r, now, { ignoreDone: true })) found.push({ key: `status-${r.id}`, mandate: r.mandate, what: `${r.id} says approved but its signed answers do not add up` });
    if (r.status === 'done' && !isApproved(c, r, now, { ignoreDone: true })) found.push({ key: `undue-${r.id}`, mandate: r.mandate, what: `${r.id} was carried out without a valid yes` });
  }
  if (fs.existsSync(path.join(dir, '.git'))) {
    const log = execFileSync('git', ['log', '--format=@@%H%x09%s', '--name-only'], { cwd: dir }).toString();
    const genesis = execFileSync('git', ['rev-list', '--max-parents=0', 'HEAD'], { cwd: dir }).toString().trim().split('\n');
    for (const block of log.split('@@').filter(Boolean)) {
      const [head, ...rest] = block.split('\n');
      const [hash, subject = ''] = head.split('\t');
      if (genesis.includes(hash) || subject.startsWith('noroles:')) continue;
      const touched = rest.map((f) => f.trim()).filter((f) => f.startsWith('requests/') || f.startsWith('incidents/') || f === 'ledger.json');
      if (touched.length) found.push({ key: `outside-${hash.slice(0, 10)}`, mandate: null, what: `commit ${hash.slice(0, 10)} "${subject}" changed ${touched.join(', ')} outside NoRoles` });
    }
  }
  let opened = 0;
  for (const f of found) if (openIncident(c, { ...f, now })) opened++;
  if (opened) commit(dir, `audit: ${opened} new incident(s)`);
  return found;
}

/** Record that an approved action was carried out (used by the MCP gateway). */
export function markDone(dir, { id, result, now = new Date() }) {
  const c = load(dir);
  const r = c.requests[id];
  r.status = 'done';
  r.done = { at: now.toISOString(), result: result ?? null };
  save(c, r);
  commit(dir, `did ${id} through the gateway`);
}
