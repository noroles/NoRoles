#!/usr/bin/env node
// NoRoles command line. https://noroles.com
import path from 'node:path';
import fs from 'node:fs';
import readline from 'node:readline/promises';
import { execFileSync } from 'node:child_process';
import { printManifesto } from '../src/manifesto.js';
import { init } from '../src/init.js';
import { load, check, mandateState, sha, readText, META_FILES } from '../src/company.js';
import { ask, openMandate, proposeMeta, decide, run, doAction, settle, effective, progress, stop, resume, audit, incidents, resolveIncident } from '../src/requests.js';
import { keygen, keyPath } from '../src/keys.js';

const HELP = `NoRoles: permissions instead of roles, for people and AI agents.

  noroles                      print the manifesto
  noroles init [dir]           start a company in dir (default: here)
  noroles check                check the files against the laws
  noroles status               mandates, requests waiting for a yes, problems

  noroles open <mandate>       ask the holders to open a mandate
  noroles ask --mandate <m> --permission <p> --summary "<exact action>"
              [--amount 42 --currency USD --to "<recipient>"] [--tool <key> -- <command...>]
              [--item "<one of several>" ...]   a batch: every item shown, one yes
  noroles yes <id>             approve (people only, in a terminal, signed with your key)
  noroles no <id> [reason]     decline (people only, in a terminal)
  noroles keygen               create your signing key (once per person)
  noroles stop <mandate> --reason "<why>"   break glass: anyone can stop a mandate
  noroles resume <mandate>     ask the holder to lift a stop
  noroles audit                check every signature and find changes made outside NoRoles
  noroles resolve <incident> "<what happened>"
  noroles do <id>              carry out an approved action, once
  noroles run <mandate> --as <executor> -- <command...>
                               run with only the keys the mandate allows
  noroles propose-meta         ask for a rule.change yes after editing root, permissions or credentials

Docs: https://noroles.com`;

const bold = (s) => (process.stdout.isTTY ? `\x1b[1m${s}\x1b[0m` : s);
const dim = (s) => (process.stdout.isTTY ? `\x1b[2m${s}\x1b[0m` : s);
const fail = (msg) => { console.error(`noroles: ${msg}`); process.exit(1); };

function findDir() {
  if (process.env.NOROLES_DIR) return process.env.NOROLES_DIR;
  let d = process.cwd();
  while (true) {
    if (fs.existsSync(path.join(d, 'permissions.md'))) return d;
    const up = path.dirname(d);
    if (up === d) fail('no permissions.md here or above. Run `noroles init` first.');
    d = up;
  }
}

function parse(args) {
  const out = { _: [] };
  const dash = args.indexOf('--');
  const head = dash >= 0 ? args.slice(0, dash) : args;
  out.command = dash >= 0 ? args.slice(dash + 1) : null;
  for (let i = 0; i < head.length; i++) {
    const a = head[i];
    if (a.startsWith('--')) { const k = a.slice(2); const v = head[i + 1] && !head[i + 1].startsWith('--') ? head[++i] : true; (out[k] = out[k] === undefined ? v : [].concat(out[k], v)); }
    else out._.push(a);
  }
  return out;
}

async function secret(question) {
  process.stdout.write(question);
  const stdin = process.stdin;
  stdin.setRawMode(true); stdin.resume(); stdin.setEncoding('utf8');
  return new Promise((resolve) => {
    let v = '';
    const on = (chunk) => {
      for (const ch of chunk) {
        if (ch === '\r' || ch === '\n') { stdin.setRawMode(false); stdin.pause(); stdin.off('data', on); process.stdout.write('\n'); resolve(v); return; }
        if (ch === '\u0003') { process.stdout.write('\n'); process.exit(1); }
        if (ch === '\u007f') v = v.slice(0, -1); else v += ch;
      }
    };
    stdin.on('data', on);
  });
}

const needTerminal = () => {
  if (process.env.NOROLES_EXECUTOR) fail('executors cannot do this: only people');
  if (!process.stdin.isTTY || !process.stdout.isTTY) fail('run this in a terminal: only people can');
};

/** Who is at the keyboard: the person whose email matches git config. */
function me(c) {
  let email = '';
  try { email = execFileSync('git', ['config', 'user.email'], { cwd: c.dir }).toString().trim(); } catch {}
  const id = Object.entries(c.people).find(([, p]) => p.email && p.email.toLowerCase() === email.toLowerCase())?.[0];
  return id || null;
}

function asker(c, flag) {
  if (flag) return flag;
  if (process.env.NOROLES_EXECUTOR) return process.env.NOROLES_EXECUTOR;
  // Outside a terminal it is most likely an agent: never assume it is the person whose git email is set.
  if (!process.stdin.isTTY) fail('say who is asking: pass --as <your agent or person id> (see permissions.md)');
  return me(c) || fail('cannot tell who is asking: pass --as <person or agent>');
}

const show = (r) => {
  const a = r.action;
  const lines = [`${bold(r.id)}  ${r.kind}  ${r.permissions.join(', ') || 'no permission'}${r.mandate ? `  mandate ${r.mandate}` : ''}`, `  asked by ${r.asker} at ${r.created}`];
  if (a.summary) lines.push(`  action:  ${a.summary}`);
  if (a.amount != null) lines.push(`  amount:  ${a.amount}${a.currency ? ' ' + a.currency : ''}`);
  if (a.to) lines.push(`  to:      ${a.to}`);
  if (a.tool) lines.push(`  key:     ${a.tool}`);
  if (a.command) lines.push(`  runs:    ${a.command.join(' ')}`);
  if (a.payload) lines.push(`  payload: ${typeof a.payload === 'string' ? a.payload : JSON.stringify(a.payload)}`);
  if (a.hashes) lines.push(`  files:   ${Object.keys(a.hashes).join(', ')}`);
  if (a.items) a.items.forEach((it, i) => lines.push(`  item ${i + 1}:  ${it}`));
  if (r.covered_by) lines.push(`  covered by the yes that opened the mandate (${r.covered_by})`);
  return lines.join('\n');
};

async function answer(yes, id, reason) {
  const dir = findDir();
  const c = load(dir);
  needTerminal();
  const who = me(c) || fail('your git email is not in permissions.md people');
  const r = c.requests[id] || fail(`no request ${id}`);
  console.log('\n' + show(r) + '\n');
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout, terminal: false });
  const typed = (await rl.question(`${who}, type "${yes ? 'yes' : 'no'}" to ${yes ? 'approve exactly this' : 'decline'}: `)).trim().toLowerCase();
  rl.close();
  if (typed !== (yes ? 'yes' : 'no')) fail('nothing recorded');
  let passphrase;
  if (c.people[who].key) {
    if (!fs.existsSync(keyPath(who))) fail(`your key is not on this computer (${keyPath(who)})`);
    passphrase = await secret('passphrase for your signing key: ');
  }
  const out = decide(dir, { id, who, yes, reason, passphrase });
  console.log(`${out.id}: ${out.status}`);
}

function status() {
  const dir = findDir();
  settle(dir);
  const c = load(dir);
  const now = new Date();
  console.log(bold('Mandates'));
  for (const m of Object.values(c.mandates)) {
    const s = mandateState(c, m.name, now);
    console.log(`  ${s.active ? '●' : '○'} ${bold(m.name)}  ${dim(`holder ${m.holder}, executor ${[].concat(m.executor).join(', ')}, ends ${String(m.expires || m.review).slice(0, 10)}`)}`);
    console.log(`    ${m.intent}${s.active ? '' : dim(`  (${s.why})`)}`);
  }
  const pending = Object.values(c.requests).filter((r) => effective(c, r, now).status === 'pending');
  console.log('\n' + bold(`Waiting for a yes (${pending.length})`));
  for (const r of pending) {
    console.log(show(r));
    for (const p of progress(c, r, now)) console.log(dim(`  ${p.permission}: ${p.got.length}/${p.need} from ${p.can.join(', ') || 'nobody'}${p.wait_until ? `, then wait until ${p.wait_until.toISOString()}` : ''}`));
  }
  const approved = Object.values(c.requests).filter((r) => r.status === 'approved' && r.kind === 'action');
  if (approved.length) { console.log('\n' + bold('Approved, not yet done')); for (const r of approved) console.log(`  ${r.id}  ${r.action.summary}`); }
  const open = incidents(dir).filter((i) => !i.resolved);
  if (open.length) { console.log('\n' + bold(`Incidents (${open.length})`)); for (const i of open) console.log(`  ${i.id}  ${i.what}  ${dim(`for ${i.holder}`)}`); }
  const problems = check(c, now);
  console.log('\n' + bold(problems.length ? `Problems (${problems.length})` : 'No problems'));
  for (const p of problems) console.log(`  ${p.level === 'error' ? '✗' : '!'} ${p.where}: ${p.msg}`);
}

async function main() {
  const [cmd, ...rest] = process.argv.slice(2);
  const o = parse(rest);
  try {
    switch (cmd) {
      case undefined: case 'manifesto': printManifesto(); console.log(dim('\nRun `noroles help` for the commands.')); break;
      case 'help': case '--help': case '-h': console.log(HELP); break;
      case 'init': {
        const dir = path.resolve(o._[0] || '.');
        const who = init(dir, { name: o.name, email: o.email });
        console.log(`Started a NoRoles company in ${dir} for ${who.name} (${who.id}).\nNext: read permissions.md, then \`noroles open first-website\`, then tell your agent to read AGENTS.md.`);
        break;
      }
      case 'check': {
        const c = load(findDir());
        const problems = check(c);
        for (const p of problems) console.log(`${p.level === 'error' ? '✗' : '!'} ${p.where}: ${p.msg}`);
        if (!problems.length) console.log('No problems.');
        process.exit(problems.some((p) => p.level === 'error') ? 1 : 0);
      }
      case 'status': status(); break;
      case 'open': {
        const dir = findDir(); const c = load(dir);
        const r = openMandate(dir, { mandate: o._[0] || fail('which mandate?'), asker: asker(c, o.as) });
        console.log(show(r) + `\n${r.status === 'approved' ? 'Open: it needs no permission.' : 'Waiting for its holders: they run `noroles yes ' + r.id + '`.'}`);
        break;
      }
      case 'ask': {
        const dir = findDir(); const c = load(dir);
        const r = ask(dir, { mandate: o.mandate || fail('--mandate is required'), permissions: o.permission || fail('--permission is required'), asker: asker(c, o.as), summary: o.summary || fail('--summary is required: the exact action'), items: o.item, amount: o.amount, currency: o.currency, to: o.to, payload: o.payload, tool: o.tool, command: o.command });
        console.log(show(r) + (r.status === 'approved' ? `\nApproved: inside the mandate's limits, covered by its yes. Go ahead${r.action.command ? ` with \`noroles do ${r.id}\`` : ''}.` : `\nAsked. A holder answers with \`noroles yes ${r.id}\`. Do nothing lasting until then.`));
        break;
      }
      case 'yes': await answer(true, o._[0] || fail('which request?')); break;
      case 'no': await answer(false, o._[0] || fail('which request?'), o._.slice(1).join(' ')); break;
      case 'do': process.exit(doAction(findDir(), { id: o._[0] || fail('which request?') }));
      case 'run': process.exit(run(findDir(), { mandate: o._[0] || fail('which mandate?'), as: o.as || fail('--as <executor> is required'), argv: o.command || fail('put the command after --') }));
      case 'keygen': {
        needTerminal();
        const dir = findDir(); const c = load(dir);
        const who = me(c) || fail('your git email is not in permissions.md people');
        const p1 = await secret('new passphrase (8+ characters): ');
        const p2 = await secret('again: ');
        if (p1 !== p2) fail('passphrases differ');
        const pub = keygen(who, p1);
        const genesis = !Object.keys(c.requests).length;
        const file = fs.readFileSync(path.join(dir, 'permissions.md'), 'utf8');
        const re = new RegExp(`^(\\s*${who}:\\s*\\{)(.*)\\}`, 'm');
        if (genesis && re.test(file) && !/key:/.test(file.match(re)[2])) {
          fs.writeFileSync(path.join(dir, 'permissions.md'), file.replace(re, (_, a, b) => `${a}${b.replace(/\s*$/, '')}, key: "${pub}" }`));
          const meta = {}; for (const f of META_FILES) if (fs.existsSync(path.join(dir, f))) meta[f] = sha(readText(dir, f));
          fs.writeFileSync(path.join(dir, 'ledger.json'), JSON.stringify({ ...c.ledger, meta }, null, 2) + '\n');
          const { commit } = await import('../src/requests.js'); commit(dir, `genesis: signing key for ${who}`, { all: true });
          console.log(`Key saved to ${keyPath(who)} and added to permissions.md.`);
        } else console.log(`Key saved to ${keyPath(who)}. Add this to your entry in permissions.md, then \`noroles propose-meta\`:\n  key: "${pub}"`);
        break;
      }
      case 'stop': { const dir = findDir(); const c = load(dir); const r = stop(dir, { mandate: o._[0] || fail('which mandate?'), asker: asker(c, o.as), reason: o.reason || o._.slice(1).join(' ') }); console.log(`Stopped ${r.mandate}. Its holder reviews within 24h; \`noroles resume ${r.mandate}\` asks them to lift it.`); break; }
      case 'resume': { const dir = findDir(); const c = load(dir); const r = resume(dir, { mandate: o._[0] || fail('which mandate?'), asker: asker(c, o.as) }); console.log(show(r) + `\nThe holder answers with \`noroles yes ${r.id}\`.`); break; }
      case 'audit': { const f = audit(findDir()); for (const x of f) console.log(`✗ ${x.what}`); if (!f.length) console.log('Every answer is validly signed and every change went through NoRoles.'); process.exit(f.length ? 1 : 0); }
      case 'resolve': { needTerminal(); const dir = findDir(); const c = load(dir); const who = me(c) || fail('your git email is not in permissions.md people'); resolveIncident(dir, { id: o._[0] || fail('which incident?'), who, note: o._.slice(1).join(' ') }); console.log('Resolved.'); break; }
      case 'propose-meta': { const dir = findDir(); const c = load(dir); const r = proposeMeta(dir, { asker: asker(c, o.as) }); console.log(show(r)); break; }
      default: fail(`unknown command "${cmd}". Run \`noroles help\`.`);
    }
  } catch (e) { fail(e.message); }
}
main();
