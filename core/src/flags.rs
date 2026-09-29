//! What is unusual about a request, so a person can say yes in seconds by looking only at the flags.
//! Computed from the signed history every time it is shown, never stored: editing a request file cannot hide one.
use crate::company::*;
use crate::util::*;

fn norm(v: &V) -> String { text(v).unwrap_or_default().trim().to_lowercase() }
fn median(xs: &mut [f64]) -> f64 { xs.sort_by(|a, b| a.partial_cmp(b).unwrap()); let i = xs.len() / 2; if xs.len() % 2 == 1 { xs[i] } else { (xs[i - 1] + xs[i]) / 2.0 } }

pub fn flags(c: &Company, r: &V) -> Vec<String> {
    if kind(r) != "action" { return vec![]; }
    let at = created(r);
    let id = s(r, "id").unwrap_or("");
    let before: Vec<&V> = c.requests.values().filter(|o| s(o, "id") != Some(id) && kind(o) == "action" && created(o) < at).collect();
    let yes: Vec<&V> = before.iter().copied().filter(|o| is_approved(c, o, at, true)).collect();
    let a = g(r, "action");
    let perms = permissions_of(r);
    let mandate = s(r, "mandate");
    let mut out = vec![];
    let money = perms.iter().any(|p| p.starts_with("money."));
    if let Some(to) = text(g(a, "to")).filter(|t| !t.is_empty()) {
        if !yes.iter().any(|o| norm(g(g(o, "action"), "to")) == norm(g(a, "to"))) {
            out.push(if money { format!("new payee: {to} was never paid before. Confirm with them through another channel before you say yes") }
                     else { format!("new recipient: nothing was ever approved for {to} before") });
        }
    }
    for p in &perms {
        if let Some(amount) = f64_of(g(a, "amount")) {
            let mut past: Vec<f64> = yes.iter().filter(|o| permissions_of(o).contains(p)).filter_map(|o| f64_of(g(g(o, "action"), "amount"))).collect();
            if past.len() >= 3 {
                let m = median(&mut past);
                if m > 0.0 && amount > 3.0 * m { out.push(format!("amount {} is {:.1}× the usual {p} (median {})", fmt_num(amount), amount / m, fmt_num(m))); }
            }
        }
        if !yes.iter().any(|o| s(o, "mandate") == mandate && permissions_of(o).contains(p)) { out.push(format!("first time mandate {} uses {p}", mandate.unwrap_or(""))); }
    }
    if let Some(d) = before.iter().find(|o| s(o, "action_hash") == s(r, "action_hash") && status(o) == "denied") {
        let first = g(d, "denials").get(0).cloned().unwrap_or(V::Null);
        let when = text(g(&first, "at")).or_else(|| text(g(d, "created"))).unwrap_or_default();
        let by = s(&first, "by").map(|b| format!(" by {b}")).unwrap_or_default();
        let why = s(&first, "reason").map(|x| format!(": {x}")).unwrap_or_default();
        out.push(format!("the same action was declined on {}{by}{why}", &when[..when.len().min(10)]));
    }
    let asker = s(r, "asker");
    let burst = before.iter().filter(|o| s(o, "asker") == asker && at - created(o) <= HOUR).count();
    if burst >= 5 { out.push(format!("request {} from {} in the last hour", burst + 1, asker.unwrap_or(""))); }
    out
}
