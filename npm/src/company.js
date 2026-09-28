// Reads a NoRoles company from its folder and checks it against the spec laws.
// Files: root.md, permissions.md, credentials.md (YAML in a ```yaml block),
// mandates/*.md (YAML front matter), rules/*.md, requests/*.yaml, ledger.json.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import YAML from 'yaml';
import { verify, statement } from './keys.js';

export const META_FILES = ['root.md', 'permissions.md', 'credentials.md', 'servers.md'];
const DAY = 86400000;

export const sha = (s) => crypto.createHash('sha256').update(s).digest('hex');
export { canonical } from './keys.js';
import { canonical } from './keys.js';
export const readText = (dir, f) => fs.readFileSync(path.join(dir, f), 'utf8');

export function yamlBlock(text, file) {
  const m = /```ya?ml\n([\s\S]*?)```/.exec(text);
  if (!m) throw new Error(`${file}: no \`\`\`yaml block found`);
  return YAML.parse(m[1]) || {};
}

export function frontMatter(text, file) {
  const m = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/.exec(text);
  if (!m) throw new Error(`${file}: no front matter (--- ... ---) found`);
  return { data: YAML.parse(m[1]) || {}, body: m[2].trim() };
}

export function parseDuration(s, fallback = '48h') {
  const m = /^(\d+(?:\.\d+)?)\s*(m|h|d)$/.exec(String(s ?? fallback).trim());
  if (!m) throw new Error(`bad duration: ${s}`);
  return Number(m[1]) * { m: 60000, h: 3600000, d: DAY }[m[2]];
}

const asDate = (v) => (v instanceof Date ? v : v ? new Date(String(v)) : null);

export function load(dir) {
  const root = yamlBlock(readText(dir, 'root.md'), 'root.md');
  const perm = yamlBlock(readText(dir, 'permissions.md'), 'permissions.md');
  const creds = fs.existsSync(path.join(dir, 'credentials.md'))
    ? yamlBlock(readText(dir, 'credentials.md'), 'credentials.md') : {};
  const servers = fs.existsSync(path.join(dir, 'servers.md'))
    ? yamlBlock(readText(dir, 'servers.md'), 'servers.md').servers || {} : {};
  const mandates = {};
  const mdir = path.join(dir, 'mandates');
  for (const f of fs.existsSync(mdir) ? fs.readdirSync(mdir).filter((f) => f.endsWith('.md')).sort() : []) {
    const text = readText(dir, path.join('mandates', f));
    const { data, body } = frontMatter(text, `mandates/${f}`);
    mandates[f.replace(/\.md$/, '')] = { ...data, name: f.replace(/\.md$/, ''), body, hash: sha(text) };
  }
  const rules = fs.existsSync(path.join(dir, 'rules'))
    ? fs.readdirSync(path.join(dir, 'rules')).filter((f) => f.endsWith('.md')).map((f) => f.replace(/\.md$/, '')) : [];
  const requests = {};
  const rdir = path.join(dir, 'requests');
  for (const f of fs.existsSync(rdir) ? fs.readdirSync(rdir).filter((f) => f.endsWith('.yaml')).sort() : []) {
    const r = YAML.parse(readText(dir, path.join('requests', f)));
    requests[r.id] = r;
  }
  const ledgerPath = path.join(dir, 'ledger.json');
  const ledger = fs.existsSync(ledgerPath) ? JSON.parse(fs.readFileSync(ledgerPath, 'utf8')) : { meta: {} };
  return {
    dir,
    root: { members: root.root || [], observer: root.observer || null },
    people: perm.people || {},
    agents: perm.agents || {},
    groups: perm.groups || {},
    permissions: perm.permissions || {},
    credentials: creds.credentials || {},
    servers,
    mandates, rules, requests, ledger,
  };
}

/** Humans a holder list resolves to (people and groups; agents never count). */
export function humans(c, list) {
  const out = new Set();
  for (const h of list || []) {
    if (c.groups[h]) for (const p of c.groups[h]) if (c.people[p]) out.add(p);
    if (c.people[h]) out.add(h);
  }
  return [...out];
}

/** The person an executor or asker answers to: a person is themselves, an agent is its person. */
export function principal(c, who) {
  if (c.people[who]) return who;
  if (c.agents[who]) return c.agents[who].works_for;
  return null;
}

export function mandateCan(m) {
  const can = m.can || {};
  const perms = {};
  for (const [k, v] of Object.entries(can)) if (k !== 'tools' && k !== 'servers') perms[k] = v || {};
  return { perms, tools: can.tools || [], servers: can.servers || [] };
}

function family(c, name) {
  const fam = new Set([name]);
  const parentOf = {};
  for (const m of Object.values(c.mandates)) for (const p of m.parts || []) parentOf[p] = m.name;
  let top = name;
  while (parentOf[top]) top = parentOf[top];
  const walk = (n) => { fam.add(n); for (const p of c.mandates[n]?.parts || []) walk(p); };
  walk(top);
  return fam;
}

const EACH = { item: 0, batch: 1, mandate: 2 };
const PERIOD = { day: DAY, week: 7 * DAY, month: 30 * DAY };

/** How often a fresh yes is needed for p inside mandate m: the tighter of permissions.md and the mandate. */
export function eachFor(c, m, p) {
  const base = c.permissions[p]?.each || 'mandate';
  const own = m ? mandateCan(m).perms[p]?.each : null;
  return own && EACH[own] < EACH[base] ? own : base;
}

function respondWithin(c, r) {
  const list = r.permissions.length ? r.permissions : ['_'];
  return Math.min(...list.map((p) => parseDuration(c.permissions[p]?.respond_within)));
}

/** Pending requests pass to root when their holders are silent, and become a no when root is silent too. */
export function effective(c, r, now = new Date()) {
  if (r.status !== 'pending') return { status: r.status, escalated: false };
  const age = now - new Date(r.created);
  const rw = respondWithin(c, r);
  if (age > 2 * rw) return { status: 'expired', escalated: true };
  return { status: 'pending', escalated: age > rw };
}

export function quorumFor(c, r, p) {
  if (r.kind === 'resume') return 1;
  const q = c.permissions[p]?.quorum ?? 1;
  return r.kind === 'meta' ? Math.max(2, q) : q;
}

export function eligible(c, r, p, now = new Date()) {
  let hs = r.kind === 'resume' ? [c.mandates[r.mandate]?.holder].filter(Boolean) : humans(c, c.permissions[p]?.holders);
  if (effective(c, r, now).escalated || r.kind === 'resume') hs = [...new Set([...hs, ...c.root.members])];
  const asker = principal(c, r.asker);
  if (quorumFor(c, r, p) > 1) hs = hs.filter((h) => h !== asker);
  return hs;
}

/** An answer counts only if it is signed by the person's key (when they have one) over this exact action. */
export function validAnswer(c, r, a, answer) {
  const person = c.people[a.by];
  if (!person) return false;
  if (!person.key) return true;
  return verify(person.key, statement(r, answer, a.at), a.sig);
}

/** Per permission: who can answer, who validly did, and how many are needed. When fewer humans hold a
 * permission than its quorum, each missing yes becomes a 24h wait (the observer's window to veto). */
export function progress(c, r, now = new Date()) {
  const perms = r.kind === 'resume' ? ['holder'] : r.permissions;
  const yes = (r.approvals || []).filter((a) => validAnswer(c, r, a, 'yes')).map((a) => a.by);
  return perms.map((p) => {
    const can = eligible(c, r, p, now);
    const got = [...new Set(yes)].filter((b) => can.includes(b));
    const quorum = quorumFor(c, r, p);
    const short = Math.max(0, quorum - can.length);
    return { permission: p, need: quorum - short, quorum, got, can, wait_until: short ? new Date(new Date(r.created).getTime() + short * DAY) : null };
  });
}

export const complete = (prog, now) => prog.every((p) => p.got.length >= p.need && (!p.wait_until || now >= p.wait_until));

function spentInPeriod(c, p, period, since, exceptId, now) {
  return Object.values(c.requests)
    .filter((r) => r.id !== exceptId && r.kind === 'action' && r.permissions.includes(p) && isApproved(c, r, now, { ignoreDone: true }))
    .filter((r) => new Date(r.created) >= since)
    .reduce((s, r) => s + (Number(r.action.amount) || 0), 0);
}

/** Why an action is outside its mandate's limits right now, or null. */
export function limitProblem(c, r, now = new Date()) {
  const m = c.mandates[r.mandate];
  const { perms, tools } = mandateCan(m);
  for (const p of r.permissions) {
    if (!perms[p]) return `mandate ${r.mandate} cannot ${p}`;
    const def = c.permissions[p] || {};
    const amount = Number(r.action.amount) || 0;
    const cap = Math.min(perms[p].amount ?? Infinity, def.limits?.amount ?? Infinity);
    if (amount > cap) return `${p}: ${amount} is above the limit ${cap}`;
    const pp = Math.min(perms[p].per_period ?? Infinity, def.limits?.per_period ?? Infinity);
    if (pp !== Infinity) {
      const period = perms[p].period || def.limits?.period || 'month';
      const spent = spentInPeriod(c, p, period, new Date(new Date(r.created).getTime() - (PERIOD[period] || PERIOD.month)), r.id, now);
      if (spent + amount > pp) return `${p}: ${spent} already used this ${period}, ${amount} more would pass ${pp}`;
    }
    const each = eachFor(c, m, p);
    if (each === 'item' && (r.action.items?.length || 0) > 1) return `${p} is each: item: one action per request`;
  }
  if (r.action.tool) {
    if (!tools.includes(r.action.tool)) return `mandate ${r.mandate} does not have tool ${r.action.tool}`;
    for (const p of c.credentials[r.action.tool]?.exercises || []) if (!r.permissions.includes(p)) return `tool ${r.action.tool} can also ${p}, which this request does not ask for (law 2: side effects count)`;
  }
  return null;
}

/** Whether a request is approved, recomputed from signed answers and hashes, never from its status field. */
export function isApproved(c, r, now = new Date(), { ignoreDone = false } = {}) {
  if (!r || ['denied', 'expired', 'void'].includes(r.status)) return false;
  if (!ignoreDone && r.status === 'done' && r.kind === 'action') return false;
  if (sha(canonical(r.action)) !== r.action_hash) return false;
  if ((r.denials || []).some((d) => validAnswer(c, r, d, 'no'))) return false;
  if (r.kind === 'stop') return true;
  if (r.kind === 'open' && !Object.keys(mandateCan(c.mandates[r.mandate] || {}).perms).length) return true;
  if (r.covered_by) {
    const open = c.requests[r.covered_by];
    return !!open && open.kind === 'open' && open.mandate === r.mandate && isApproved(c, open, now)
      && r.permissions.every((p) => eachFor(c, c.mandates[r.mandate], p) === 'mandate');
  }
  if (!(r.approvals || []).length) return false;
  return complete(progress(c, r, now), now);
}

function stoppedBy(c, name) {
  const events = Object.values(c.requests).filter((r) => r.mandate === name && (r.kind === 'stop' || r.kind === 'resume')).sort((a, b) => a.created.localeCompare(b.created) || (a.kind === 'stop' ? -1 : 1));
  let stop = null;
  for (const e of events) {
    if (e.kind === 'stop' && isApproved(c, e)) stop = e;
    if (e.kind === 'resume' && stop && e.created >= stop.created && isApproved(c, e)) stop = null;
  }
  return stop;
}

/** A mandate is active when its current file was opened by a validly approved "open" request,
 * it has not ended, and nobody has broken the glass on it since. */
export function mandateState(c, name, now = new Date()) {
  const m = c.mandates[name];
  if (!m) return { active: false, why: 'no such mandate' };
  const end = asDate(m.expires || m.review);
  if (end && now > new Date(end.getTime() + DAY)) return { active: false, why: `ended ${end.toISOString().slice(0, 10)}` };
  const stop = stoppedBy(c, name);
  if (stop) return { active: false, why: `stopped by ${stop.asker}: ${stop.action.summary}` };
  const opens = Object.values(c.requests).filter((r) => r.kind === 'open' && r.mandate === name);
  if (opens.some((r) => r.mandate_hash === m.hash && isApproved(c, r, now))) return { active: true, why: 'open' };
  for (const p of Object.values(c.mandates)) {
    if ((p.parts || []).includes(name) && mandateState(c, p.name, now).active) return { active: true, why: `part of ${p.name}` };
  }
  return { active: false, why: opens.some((r) => isApproved(c, r, now)) ? 'changed since it was approved: needs a new yes' : 'not opened yet' };
}

/** Law checks. Returns a list of { level: 'error' | 'warn', where, msg }. */
export function check(c, now = new Date()) {
  const out = [];
  const err = (where, msg) => out.push({ level: 'error', where, msg });
  const warn = (where, msg) => out.push({ level: 'warn', where, msg });

  if (!c.root.members.length) err('root.md', 'root lists no one');
  for (const r of c.root.members) if (!c.people[r]) err('root.md', `root member "${r}" is not in people`);
  if (!c.root.observer) warn('root.md', 'no observer named: nobody can veto when a quorum cannot be met');

  for (const [id, p] of Object.entries(c.people)) if (!p.key) warn('permissions.md', `${id} has no signing key: their yes can be forged by editing a file. Run \`noroles keygen\``);
  for (const [a, def] of Object.entries(c.agents)) {
    if (!c.people[def.works_for]) err('permissions.md', `agent "${a}" works for "${def.works_for}", who is not in people`);
  }
  for (const [g, list] of Object.entries(c.groups)) {
    for (const p of list) if (!c.people[p]) err('permissions.md', `group "${g}" lists "${p}", who is not a person (agents cannot hold permissions)`);
  }
  for (const [p, def] of Object.entries(c.permissions)) {
    const hs = humans(c, def.holders);
    const q = def.quorum ?? 1;
    if (!hs.length) err(`permissions.md ${p}`, 'no human holds this permission (law 3: nothing is unheld)');
    else if (hs.length < q) warn(`permissions.md ${p}`, `quorum ${q} but only ${hs.length} holder(s): each missing yes becomes a 24h wait for the observer`);
    for (const h of def.holders || []) if (c.agents[h]) err(`permissions.md ${p}`, `agent "${h}" is listed as a holder: agents never hold permissions`);
  }
  for (const [name, sv] of Object.entries(c.servers)) {
    if (!sv.command) err(`servers.md ${name}`, 'no command');
    for (const cred of Object.values(sv.env || {})) if (!c.credentials[cred]) err(`servers.md ${name}`, `env uses unknown credential "${cred}"`);
    for (const [t, rule] of Object.entries(sv.tools || {})) for (const p of rule?.permissions || []) if (!c.permissions[p]) err(`servers.md ${name}.${t}`, `unknown permission "${p}"`);
  }
  for (const [name, cr] of Object.entries(c.credentials)) {
    for (const p of cr.exercises || []) if (!c.permissions[p]) err(`credentials.md ${name}`, `exercises unknown permission "${p}"`);
    if (!cr.env) err(`credentials.md ${name}`, 'no env variable named');
  }

  const owners = {};
  for (const m of Object.values(c.mandates)) {
    const at = `mandates/${m.name}.md`;
    for (const f of ['intent', 'metric', 'stop_if', 'holder', 'executor']) if (!m[f]) err(at, `missing "${f}"`);
    if (m.holder && !c.people[m.holder]) err(at, `holder "${m.holder}" is not a person`);
    for (const e of [].concat(m.executor || [])) if (!c.people[e] && !c.agents[e]) err(at, `executor "${e}" is not a known person or agent`);
    const end = asDate(m.expires || m.review);
    if (!end) err(at, 'needs "expires" (one-off work) or "review" (standing duty)');
    else {
      const from = asDate(m.opened) || now;
      if ((end - from) / DAY > 90) err(at, `ends ${Math.round((end - from) / DAY)} days after it opens: at most 90`);
    }
    const { perms, tools } = mandateCan(m);
    for (const [p, lim] of Object.entries(perms)) {
      const def = c.permissions[p];
      if (!def) { err(at, `can names unknown permission "${p}"`); continue; }
      if (lim.each && EACH[lim.each] > EACH[def.each || 'mandate']) err(at, `${p}.each = ${lim.each} is looser than permissions.md (${def.each || 'mandate'})`);
      for (const [k, v] of Object.entries(lim)) {
        const ceiling = def.limits?.[k];
        if (typeof v === 'number' && typeof ceiling === 'number' && v > ceiling) err(at, `${p}.${k} = ${v} is above the permissions.md ceiling ${ceiling} (a mandate narrows, never widens)`);
      }
    }
    for (const t of tools) {
      const cr = c.credentials[t];
      if (!cr) { err(at, `tool "${t}" is not in credentials.md`); continue; }
      for (const p of cr.exercises || []) if (!perms[p]) err(at, `tool "${t}" can exercise ${p}, but can does not include ${p}`);
    }
    for (const sv of mandateCan(m).servers) if (!c.servers[sv]) err(at, `server "${sv}" is not in servers.md`);
    for (const n of m.needs || []) if (!c.mandates[n]) err(at, `needs unknown mandate "${n}"`);
    for (const part of m.parts || []) {
      const child = c.mandates[part];
      if (!child) { err(at, `part "${part}" does not exist`); continue; }
      const pc = mandateCan(m), cc = mandateCan(child);
      for (const p of Object.keys(cc.perms)) if (!pc.perms[p]) err(`mandates/${part}.md`, `can ${p}, which its parent ${m.name} does not have`);
      for (const t of cc.tools) if (!pc.tools.includes(t)) err(`mandates/${part}.md`, `tool ${t}, which its parent ${m.name} does not have`);
      for (const o of child.owns || []) if (!(m.owns || []).includes(o)) err(`mandates/${part}.md`, `owns "${o}", outside its parent ${m.name}`);
    }
    if (mandateState(c, m.name, now).active || !end || now <= end) {
      for (const o of m.owns || []) (owners[o] ||= []).push(m.name);
    }
  }
  for (const [o, list] of Object.entries(owners)) {
    for (let i = 0; i < list.length; i++) for (let j = i + 1; j < list.length; j++) {
      if (!family(c, list[i]).has(list[j])) err(`owns "${o}"`, `claimed by both ${list[i]} and ${list[j]} (law 6: one owner per target)`);
    }
  }
  // needs cycles
  const seen = {}, stack = [];
  const visit = (n) => {
    if (seen[n] === 1) { err(`mandates/${n}.md`, `needs cycle: ${[...stack, n].join(' -> ')}`); return; }
    if (seen[n] === 2 || !c.mandates[n]) return;
    seen[n] = 1; stack.push(n);
    for (const x of c.mandates[n].needs || []) visit(x);
    stack.pop(); seen[n] = 2;
  };
  for (const n of Object.keys(c.mandates)) visit(n);

  // meta files may only change through an approved rule.change
  for (const f of META_FILES) {
    if (!fs.existsSync(path.join(c.dir, f))) continue;
    const h = sha(readText(c.dir, f));
    const known = c.ledger.meta?.[f];
    if (known && known !== h) err(f, 'changed outside NoRoles. Run `noroles propose-meta` and get a rule.change yes');
  }
  return out;
}
