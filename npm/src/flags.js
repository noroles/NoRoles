// What is unusual about a request, so a person can say yes in seconds by looking only at the flags.
// Computed from the signed history every time it is shown, never stored: editing a request file cannot hide one.
import { isApproved } from './company.js';

const HOUR = 3600000;
const norm = (s) => String(s ?? '').trim().toLowerCase();
const median = (xs) => { const s = [...xs].sort((a, b) => a - b); const i = s.length >> 1; return s.length % 2 ? s[i] : (s[i - 1] + s[i]) / 2; };
const day = (iso) => String(iso).slice(0, 10);

export function flags(c, r) {
  if (r.kind !== 'action') return [];
  const at = new Date(r.created).getTime();
  const before = Object.values(c.requests).filter((o) => o.id !== r.id && o.kind === 'action' && new Date(o.created).getTime() < at);
  const yes = before.filter((o) => isApproved(c, o, new Date(r.created), { ignoreDone: true }));
  const a = r.action;
  const out = [];
  const money = r.permissions.some((p) => p.startsWith('money.'));

  if (a.to && !yes.some((o) => norm(o.action.to) === norm(a.to))) {
    out.push(money
      ? `new payee: ${a.to} was never paid before. Confirm with them through another channel before you say yes`
      : `new recipient: nothing was ever approved for ${a.to} before`);
  }
  for (const p of r.permissions) {
    if (a.amount != null) {
      const past = yes.filter((o) => o.permissions.includes(p) && o.action.amount != null).map((o) => Number(o.action.amount));
      if (past.length >= 3) {
        const m = median(past);
        if (m > 0 && a.amount > 3 * m) out.push(`amount ${a.amount} is ${(a.amount / m).toFixed(1)}× the usual ${p} (median ${m})`);
      }
    }
    if (!yes.some((o) => o.mandate === r.mandate && o.permissions.includes(p))) out.push(`first time mandate ${r.mandate} uses ${p}`);
  }
  const declined = before.find((o) => o.action_hash === r.action_hash && o.status === 'denied');
  if (declined) {
    const d = declined.denials?.[0];
    out.push(`the same action was declined on ${day(d?.at || declined.created)}${d?.by ? ` by ${d.by}` : ''}${d?.reason ? `: ${d.reason}` : ''}`);
  }
  const burst = before.filter((o) => o.asker === r.asker && at - new Date(o.created).getTime() <= HOUR).length;
  if (burst >= 5) out.push(`request ${burst + 1} from ${r.asker} in the last hour`);
  return out;
}
