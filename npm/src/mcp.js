// `noroles mcp`: NoRoles sits between an agent and its MCP tools, so every call is checked
// whatever the agent decides. Open calls pass through. Lasting calls become a request for a yes;
// the agent calls again with the same arguments once a person has signed it.
// Keys live with the upstream servers NoRoles starts; the agent never sees them.
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import { spawn } from 'node:child_process';
import { load, mandateCan, mandateState, isApproved, sha, canonical } from './company.js';
import { ask, markDone, readSecrets } from './requests.js';

const PROTOCOL = '2025-06-18';
const SEP = '__';

// ---------- a minimal MCP client for the upstream servers ----------

class Upstream {
  constructor(name, def, env) {
    this.name = name;
    this.proc = spawn(def.command, def.args || [], { env, stdio: ['pipe', 'pipe', 'inherit'] });
    this.next = 1;
    this.waiting = new Map();
    readline.createInterface({ input: this.proc.stdout }).on('line', (line) => {
      let m; try { m = JSON.parse(line); } catch { return; }
      const w = this.waiting.get(m.id);
      if (w) { this.waiting.delete(m.id); m.error ? w.reject(new Error(m.error.message)) : w.resolve(m.result); }
    });
    this.proc.on('exit', () => { for (const w of this.waiting.values()) w.reject(new Error(`${name} exited`)); this.waiting.clear(); });
  }
  request(method, params) {
    const id = this.next++;
    this.proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      setTimeout(() => { if (this.waiting.delete(id)) reject(new Error(`${this.name}: ${method} timed out`)); }, 120000);
    });
  }
  notify(method, params) { this.proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n'); }
  async start() {
    await this.request('initialize', { protocolVersion: PROTOCOL, capabilities: {}, clientInfo: { name: 'noroles', version: '0.3' } });
    this.notify('notifications/initialized', {});
    const { tools } = await this.request('tools/list', {});
    this.tools = tools || [];
    return this;
  }
  stop() { try { this.proc.kill(); } catch {} }
}

// ---------- classifying a call ----------

/** Which permissions a tool call needs. [] means open. null means lasting but unmapped: refused.
 * A mapping written in servers.md (a meta file, changed only with rule.change) wins over the server's own hints. */
export function classify(server, tool) {
  const rule = server.tools?.[tool.name] ?? server.tools?.['*'];
  const a = tool.annotations || {};
  if (rule?.permissions) return [...new Set(rule.permissions)];
  if (rule?.open === true) return [];
  if (a.readOnlyHint === true) return [];
  if (a.destructiveHint === true) return ['destroy'];
  // law 1: nothing destroyed and nothing leaves the company, as the server itself declares
  if (a.destructiveHint === false && a.openWorldHint === false) return [];
  return null;
}

// ---------- the output guard ----------

const luhn = (d) => { let s = 0; for (let i = 0; i < d.length; i++) { let n = +d[d.length - 1 - i]; if (i % 2) { n *= 2; if (n > 9) n -= 9; } s += n; } return s % 10 === 0; };
const PATTERNS = [
  { kind: 'secret', re: /\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{10,}\b|\bgh[pousr]_[A-Za-z0-9]{20,}\b|\bAKIA[0-9A-Z]{16}\b|\bxai-[A-Za-z0-9]{20,}\b|\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g, always: true },
  { kind: 'card', re: /\b(?:\d[ -]?){13,19}\b/g, test: (m) => luhn(m.replace(/\D/g, '')) },
  { kind: 'iban', re: /\b[A-Z]{2}\d{2}[A-Z0-9]{11,30}\b/g },
  { kind: 'email', re: /\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b/g },
  { kind: 'phone', re: /\+\d[\d ().-]{7,}\d/g },
];

/** Redact personal data (unless the mandate can data.export) and secrets (always) from tool output. */
export function guard(text, { canExport }) {
  const found = {};
  let out = text;
  for (const p of PATTERNS) {
    if (canExport && !p.always) continue;
    out = out.replace(p.re, (m) => {
      if (p.test && !p.test(m)) return m;
      found[p.kind] = (found[p.kind] || 0) + 1;
      return `[${p.kind} removed by NoRoles]`;
    });
  }
  return { text: out, found };
}

// ---------- the gateway ----------

const envelope = (server, tool, args) => ({ server, tool, arguments: args ?? {} });
const pick = (args, key) => (key && args && args[key] != null ? args[key] : null);

export async function gateway(dir, { mandate, as, input = process.stdin, output = process.stdout, log = () => {} }) {
  let c = load(dir);
  const m = c.mandates[mandate];
  if (!m) throw new Error(`no mandate "${mandate}"`);
  if (![].concat(m.executor || []).includes(as)) throw new Error(`${as} is not an executor of ${mandate}`);
  const secrets = readSecrets(dir);
  const allowed = mandateCan(m).servers;
  const ups = {};
  for (const name of allowed) {
    const def = c.servers[name];
    const env = { PATH: process.env.PATH, HOME: process.env.HOME, ...(def.plain_env || {}) };
    for (const [k, cred] of Object.entries(def.env || {})) {
      const cr = c.credentials[cred];
      if (secrets[cr.env] != null) env[k] = secrets[cr.env];
    }
    ups[name] = await new Upstream(name, def, env).start();
  }
  const send = (msg) => output.write(JSON.stringify(msg) + '\n');
  const text = (t, isError = false) => ({ content: [{ type: 'text', text: t }], ...(isError ? { isError: true } : {}) });
  const callLog = path.join(dir, 'log', 'calls.jsonl');
  const record = (e) => { fs.mkdirSync(path.dirname(callLog), { recursive: true }); fs.appendFileSync(callLog, JSON.stringify({ at: new Date().toISOString(), mandate, as, ...e }) + '\n'); };

  async function call(fullName, args) {
    c = load(dir);
    const state = mandateState(c, mandate);
    if (!state.active) return text(`NoRoles: mandate ${mandate} is not active (${state.why}). Nothing was done.`, true);
    const [sv, ...rest] = fullName.split(SEP);
    const toolName = rest.join(SEP);
    const up = ups[sv];
    const tool = up?.tools.find((t) => t.name === toolName);
    if (!tool) return text(`NoRoles: no tool ${fullName} for mandate ${mandate}.`, true);
    const def = c.servers[sv];
    const perms = classify(def, tool);
    const env = envelope(sv, toolName, args);

    if (perms === null) {
      record({ tool: fullName, decision: 'refused', why: 'lasting, unmapped' });
      return text(`NoRoles: ${fullName} can change things outside and servers.md does not say which permission it needs, so it is refused. Ask your person to map it in servers.md.`, true);
    }
    let requestId = null;
    if (perms.length) {
      const hash = sha(canonical(env));
      const prior = Object.values(c.requests).find((r) => r.kind === 'action' && r.mandate === mandate && r.status !== 'done'
        && r.action.payload && sha(canonical(r.action.payload)) === hash);
      if (prior && isApproved(c, prior)) requestId = prior.id;
      else if (prior && ['pending'].includes(prior.status)) return text(`NoRoles: still waiting for a yes on ${prior.id}. Nothing was done. Call again with the same arguments after your person runs \`noroles yes ${prior.id}\`.`);
      else {
        const rule = def.tools?.[toolName] ?? def.tools?.['*'] ?? {};
        let r;
        try {
          r = ask(dir, { mandate, permissions: perms, asker: as, summary: `${sv}.${toolName} ${canonical(args ?? {})}`, amount: pick(args, rule.amount), currency: pick(args, rule.currency) || rule.currency_default || null, to: pick(args, rule.to), payload: env });
        } catch (e) {
          record({ tool: fullName, decision: 'refused', why: e.message });
          return text(`NoRoles: refused: ${e.message}. Nothing was done.`, true);
        }
        if (r.status !== 'approved') {
          record({ tool: fullName, decision: 'asked', request: r.id });
          return text(`NoRoles: ${fullName} needs a yes (${perms.join(', ')}). Asked as ${r.id}; nothing was done yet. Tell your person to run \`noroles yes ${r.id}\`, then call this tool again with exactly the same arguments.`);
        }
        requestId = r.id;
      }
    }
    const res = await up.request('tools/call', { name: toolName, arguments: args ?? {} });
    if (requestId) markDone(dir, { id: requestId, result: 'called through the gateway' });
    const canExport = !!mandateCan(m).perms['data.export'];
    const found = {};
    const content = (res.content || []).map((p) => {
      if (p.type !== 'text') return p;
      const g = guard(p.text, { canExport });
      for (const [k, v] of Object.entries(g.found)) found[k] = (found[k] || 0) + v;
      return { ...p, text: g.text };
    });
    if (Object.keys(found).length) content.push({ type: 'text', text: `NoRoles removed ${Object.entries(found).map(([k, v]) => `${v} ${k}`).join(', ')} from this result${canExport ? '' : ' (this mandate cannot data.export)'}.` });
    record({ tool: fullName, decision: requestId ? `approved ${requestId}` : 'open', removed: found });
    return { ...res, content };
  }

  const rl = readline.createInterface({ input });
  rl.on('line', async (line) => {
    if (!line.trim()) return;
    let msg; try { msg = JSON.parse(line); } catch { return; }
    if (msg.id === undefined) return;
    try {
      if (msg.method === 'initialize') return send({ jsonrpc: '2.0', id: msg.id, result: { protocolVersion: msg.params?.protocolVersion || PROTOCOL, capabilities: { tools: {} }, serverInfo: { name: 'noroles', version: '0.3' }, instructions: `You work inside the NoRoles mandate "${mandate}". Open calls run at once. Lasting calls ask a person for a yes first: when a result says a yes is needed, stop, tell your person the request id, and call again with the same arguments after they approve.` } });
      if (msg.method === 'ping') return send({ jsonrpc: '2.0', id: msg.id, result: {} });
      if (msg.method === 'tools/list') {
        const tools = [];
        for (const [name, up] of Object.entries(ups)) for (const t of up.tools) {
          const perms = classify(c.servers[name], t);
          const note = perms === null ? ' [NoRoles: refused, not mapped]' : perms.length ? ` [NoRoles: needs a yes: ${perms.join(', ')}]` : '';
          tools.push({ ...t, name: `${name}${SEP}${t.name}`, description: `${t.description || ''}${note}` });
        }
        return send({ jsonrpc: '2.0', id: msg.id, result: { tools } });
      }
      if (msg.method === 'tools/call') return send({ jsonrpc: '2.0', id: msg.id, result: await call(msg.params.name, msg.params.arguments) });
      send({ jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: `NoRoles does not handle ${msg.method}` } });
    } catch (e) {
      log(e);
      send({ jsonrpc: '2.0', id: msg.id, result: text(`NoRoles: ${e.message}`, true) });
    }
  });
  return { stop: () => { rl.close(); for (const u of Object.values(ups)) u.stop(); } };
}
