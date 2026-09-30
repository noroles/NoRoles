//! Reads a NoRoles company from its folder and checks it against the spec laws.
//! Files: root.md, permissions.md, credentials.md, servers.md, tools.md (YAML in a ```yaml block),
//! mandates/*.md (YAML front matter), rules/*.md, requests/*.yaml, ledger.json.
use crate::keys;
use crate::util::*;
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const META_FILES: [&str; 5] = ["root.md", "permissions.md", "credentials.md", "servers.md", "tools.md"];

pub struct Company {
    pub dir: PathBuf,
    pub root: Vec<String>,
    pub observer: Option<String>,
    pub people: Obj,
    pub agents: Obj,
    pub groups: Obj,
    pub permissions: Obj,
    pub credentials: Obj,
    pub servers: Obj,
    pub tools: Vec<(String, V)>,
    pub mandates: Obj,
    pub rules: Vec<String>,
    pub requests: Obj,
    pub ledger: V,
}

pub fn read_text(dir: &Path, f: &str) -> Result<String, String> {
    fs::read_to_string(dir.join(f)).map_err(|e| format!("{f}: {e}"))
}

pub fn yaml_block(text: &str, file: &str) -> Result<V, String> {
    let start = text.find("```yaml\n").map(|i| i + 8).or_else(|| text.find("```yml\n").map(|i| i + 7))
        .ok_or_else(|| format!("{file}: no ```yaml block found"))?;
    let end = text[start..].find("```").map(|i| start + i).ok_or_else(|| format!("{file}: no ```yaml block found"))?;
    yaml(&text[start..end], file)
}

pub fn front_matter(text: &str, file: &str) -> Result<(V, String), String> {
    let err = || format!("{file}: no front matter (--- ... ---) found");
    let rest = text.strip_prefix("---\n").ok_or_else(err)?;
    let end = rest.find("\n---").ok_or_else(err)?;
    let after = &rest[end + 4..];
    if !(after.is_empty() || after.starts_with('\n')) { return Err(err()); }
    Ok((yaml(&rest[..end], file)?, after.trim().to_string()))
}

fn listdir(d: &Path, ext: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(d).map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| f.ends_with(ext)).collect()).unwrap_or_default();
    v.sort();
    v
}

pub fn load(dir: &Path) -> Result<Company, String> {
    let root = yaml_block(&read_text(dir, "root.md")?, "root.md")?;
    let perm = yaml_block(&read_text(dir, "permissions.md")?, "permissions.md")?;
    let opt = |f: &str| -> Result<V, String> { if dir.join(f).exists() { yaml_block(&read_text(dir, f)?, f) } else { Ok(json!({})) } };
    let creds = opt("credentials.md")?;
    let servers = opt("servers.md")?;
    let tools_v = opt("tools.md")?;
    let tools = g(&tools_v, "tools").as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    let mut mandates = Obj::new();
    for f in listdir(&dir.join("mandates"), ".md") {
        let t = read_text(dir, &format!("mandates/{f}"))?;
        let (data, body) = front_matter(&t, &format!("mandates/{f}"))?;
        let name = f.trim_end_matches(".md").to_string();
        let mut m = data.as_object().cloned().unwrap_or_default();
        m.insert("name".into(), V::from(name.clone()));
        m.insert("body".into(), V::from(body));
        m.insert("hash".into(), V::from(sha(&t)));
        mandates.insert(name, V::Object(m));
    }
    let rules = listdir(&dir.join("rules"), ".md").into_iter().map(|f| f.trim_end_matches(".md").to_string()).collect();
    let mut requests = Obj::new();
    for f in listdir(&dir.join("requests"), ".yaml") {
        let r = yaml(&read_text(dir, &format!("requests/{f}"))?, &format!("requests/{f}"))?;
        if let Some(id) = s(&r, "id") { requests.insert(id.to_string(), r.clone()); }
    }
    let ledger = fs::read_to_string(dir.join("ledger.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({ "meta": {} }));
    Ok(Company {
        dir: dir.to_path_buf(),
        root: listk(&root, "root"),
        observer: text(g(&root, "observer")),
        people: obj(&perm, "people"),
        agents: obj(&perm, "agents"),
        groups: obj(&perm, "groups"),
        permissions: obj(&perm, "permissions"),
        credentials: obj(&creds, "credentials"),
        servers: obj(&servers, "servers"),
        tools,
        mandates, rules, requests, ledger,
    })
}

impl Company {
    pub fn perm(&self, p: &str) -> &V { self.permissions.get(p).unwrap_or(&V::Null) }
    pub fn mandate(&self, m: &str) -> Option<&V> { self.mandates.get(m) }
    pub fn request(&self, id: &str) -> Option<&V> { self.requests.get(id) }
    pub fn is_person(&self, who: &str) -> bool { self.people.contains_key(who) }
    pub fn is_agent(&self, who: &str) -> bool { self.agents.contains_key(who) }
}

/// Humans a holder list resolves to (people and groups; agents never count).
pub fn humans(c: &Company, holders: &V) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut add = |p: String| if !out.contains(&p) { out.push(p) };
    for h in list(holders) {
        if let Some(gr) = c.groups.get(&h) { for p in list(gr) { if c.is_person(&p) { add(p) } } }
        if c.is_person(&h) { add(h) }
    }
    out
}

/// The person an executor or asker answers to: a person is themselves, an agent is its person.
pub fn principal(c: &Company, who: &str) -> Option<String> {
    if c.is_person(who) { return Some(who.into()); }
    c.agents.get(who).and_then(|a| s(a, "works_for")).map(String::from)
}

pub struct Can { pub perms: Obj, pub tools: Vec<String>, pub servers: Vec<String> }
pub fn mandate_can(m: &V) -> Can {
    let can = obj(m, "can");
    let mut perms = Obj::new();
    for (k, v) in &can { if k != "tools" && k != "servers" { perms.insert(k.clone(), if truthy(v) { v.clone() } else { json!({}) }); } }
    Can { perms, tools: list(can.get("tools").unwrap_or(&V::Null)), servers: list(can.get("servers").unwrap_or(&V::Null)) }
}

fn family(c: &Company, name: &str) -> HashSet<String> {
    let mut parent: BTreeMap<String, String> = BTreeMap::new();
    for m in c.mandates.values() { for p in listk(m, "parts") { parent.insert(p, s(m, "name").unwrap_or("").into()); } }
    let mut top = name.to_string();
    for _ in 0..1000 { match parent.get(&top) { Some(p) => top = p.clone(), None => break } }
    fn walk(c: &Company, n: &str, fam: &mut HashSet<String>) {
        fam.insert(n.to_string());
        if let Some(m) = c.mandate(n) { for p in listk(m, "parts") { if !fam.contains(&p) { walk(c, &p, fam) } } }
    }
    let mut fam = HashSet::from([name.to_string()]);
    walk(c, &top, &mut fam);
    fam
}

fn each_rank(e: &str) -> i32 { match e { "item" => 0, "batch" => 1, "mandate" => 2, _ => 2 } }
const PERIOD_MONTH: i64 = 30 * DAY;
fn period_ms(p: &str) -> i64 { match p { "day" => DAY, "week" => 7 * DAY, _ => PERIOD_MONTH } }

/// How often a fresh yes is needed for p inside mandate m: the tighter of permissions.md and the mandate.
pub fn each_for(c: &Company, m: Option<&V>, p: &str) -> String {
    let base = s(c.perm(p), "each").unwrap_or("mandate").to_string();
    let own = m.and_then(|m| mandate_can(m).perms.get(p).and_then(|x| s(x, "each")).map(String::from));
    match own { Some(o) if each_rank(&o) < each_rank(&base) => o, _ => base }
}

pub fn permissions_of(r: &V) -> Vec<String> { listk(r, "permissions") }

fn respond_within(c: &Company, r: &V) -> i64 {
    let mut ps = permissions_of(r);
    if ps.is_empty() { ps.push("_".into()); }
    ps.iter().map(|p| parse_duration(g(c.perm(p), "respond_within"), "48h").unwrap_or(48 * HOUR)).min().unwrap_or(48 * HOUR)
}

pub fn created(r: &V) -> i64 { time_of(g(r, "created")).unwrap_or(0) }
pub fn status(r: &V) -> &str { s(r, "status").unwrap_or("") }
pub fn kind(r: &V) -> &str { s(r, "kind").unwrap_or("") }

/// Pending requests pass to root when their holders are silent, and become a no when root is silent too.
pub fn effective(c: &Company, r: &V, now: i64) -> (String, bool) {
    if status(r) != "pending" { return (status(r).into(), false); }
    let age = now - created(r);
    let rw = respond_within(c, r);
    if age > 2 * rw { return ("expired".into(), true); }
    ("pending".into(), age > rw)
}

pub fn quorum_for(c: &Company, r: &V, p: &str) -> i64 {
    if kind(r) == "resume" { return 1; }
    let q = g(c.perm(p), "quorum").as_i64().unwrap_or(1);
    if kind(r) == "meta" { q.max(2) } else { q }
}

pub fn eligible(c: &Company, r: &V, p: &str, now: i64) -> Vec<String> {
    let mut hs = if kind(r) == "resume" {
        c.mandate(s(r, "mandate").unwrap_or("")).and_then(|m| s(m, "holder")).map(|h| vec![h.to_string()]).unwrap_or_default()
    } else { humans(c, g(c.perm(p), "holders")) };
    if effective(c, r, now).1 || kind(r) == "resume" { for m in &c.root { if !hs.contains(m) { hs.push(m.clone()) } } }
    let asker = principal(c, s(r, "asker").unwrap_or(""));
    if quorum_for(c, r, p) > 1 { hs.retain(|h| Some(h) != asker.as_ref()); }
    hs
}

pub fn action_hash(r: &V) -> String { sha(&canonical(g(r, "action"))) }

/// An answer counts only if it is signed by the person's key (when they have one) over this exact action.
pub fn valid_answer(c: &Company, r: &V, a: &V, answer: &str) -> bool {
    let Some(by) = s(a, "by") else { return false };
    let Some(person) = c.people.get(by) else { return false };
    let Some(key) = s(person, "key") else { return true };
    let at = text(g(a, "at")).unwrap_or_default();
    let st = keys::statement(s(r, "id").unwrap_or(""), s(r, "action_hash").unwrap_or(""), answer, &at);
    keys::verify(key, &st, s(a, "sig").unwrap_or(""))
}

pub struct Prog { pub permission: String, pub need: i64, pub quorum: i64, pub got: Vec<String>, pub can: Vec<String>, pub wait_until: Option<i64> }

/// Per permission: who can answer, who validly did, and how many are needed. When fewer humans hold a
/// permission than its quorum, each missing yes becomes a 24h wait (the observer's window to veto).
pub fn progress(c: &Company, r: &V, now: i64) -> Vec<Prog> {
    let perms = if kind(r) == "resume" { vec!["holder".to_string()] } else { permissions_of(r) };
    let mut yes: Vec<String> = vec![];
    for a in g(r, "approvals").as_array().cloned().unwrap_or_default() {
        if valid_answer(c, r, &a, "yes") { if let Some(b) = s(&a, "by") { if !yes.iter().any(|y| y == b) { yes.push(b.into()) } } }
    }
    perms.into_iter().map(|p| {
        let can = eligible(c, r, &p, now);
        let got: Vec<String> = yes.iter().filter(|b| can.contains(b)).cloned().collect();
        let quorum = quorum_for(c, r, &p);
        let short = (quorum - can.len() as i64).max(0);
        Prog { permission: p, need: quorum - short, quorum, got, can, wait_until: if short > 0 { Some(created(r) + short * DAY) } else { None } }
    }).collect()
}

pub fn complete(prog: &[Prog], now: i64) -> bool { prog.iter().all(|p| p.got.len() as i64 >= p.need && p.wait_until.is_none_or(|w| now >= w)) }

fn spent_in_period(c: &Company, p: &str, since: i64, except: &str, now: i64) -> f64 {
    c.requests.values()
        .filter(|r| s(r, "id") != Some(except) && kind(r) == "action" && permissions_of(r).iter().any(|x| x == p) && is_approved(c, r, now, true))
        .filter(|r| created(r) >= since)
        .map(|r| f64_of(g(g(r, "action"), "amount")).unwrap_or(0.0)).sum()
}

fn limit_num(v: &V, k: &str) -> f64 { g(v, k).as_f64().unwrap_or(f64::INFINITY) }
pub fn fmt_num(f: f64) -> String { if f.is_infinite() { "Infinity".into() } else { js_number(&serde_json::Number::from_f64(f).unwrap_or(0.into())) } }

/// Why an action is outside its mandate's limits right now, or None.
pub fn limit_problem(c: &Company, r: &V, now: i64) -> Option<String> {
    let mname = s(r, "mandate").unwrap_or("");
    let m = c.mandate(mname)?;
    let can = mandate_can(m);
    let a = g(r, "action");
    for p in permissions_of(r) {
        let Some(own) = can.perms.get(&p) else { return Some(format!("mandate {mname} cannot {p}")) };
        let def = c.perm(&p);
        let lim = g(def, "limits");
        let amount = f64_of(g(a, "amount")).unwrap_or(0.0);
        let cap = limit_num(own, "amount").min(limit_num(lim, "amount"));
        if amount > cap { return Some(format!("{p}: {} is above the limit {}", fmt_num(amount), fmt_num(cap))); }
        let pp = limit_num(own, "per_period").min(limit_num(lim, "per_period"));
        if pp.is_finite() {
            let period = s(own, "period").or(s(lim, "period")).unwrap_or("month").to_string();
            let spent = spent_in_period(c, &p, created(r) - period_ms(&period), s(r, "id").unwrap_or(""), now);
            if spent + amount > pp { return Some(format!("{p}: {} already used this {period}, {} more would pass {}", fmt_num(spent), fmt_num(amount), fmt_num(pp))); }
        }
        if each_for(c, Some(m), &p) == "item" && g(a, "items").as_array().map(|x| x.len()).unwrap_or(0) > 1 { return Some(format!("{p} is each: item: one action per request")); }
    }
    if let Some(tool) = s(a, "tool") {
        if !can.tools.iter().any(|t| t == tool) { return Some(format!("mandate {mname} does not have tool {tool}")); }
        for p in c.credentials.get(tool).map(|x| listk(x, "exercises")).unwrap_or_default() {
            if !permissions_of(r).contains(&p) { return Some(format!("tool {tool} can also {p}, which this request does not ask for (law 2: side effects count)")); }
        }
    }
    None
}

/// Whether a request is approved, recomputed from signed answers and hashes, never from its status field.
pub fn is_approved(c: &Company, r: &V, now: i64, ignore_done: bool) -> bool {
    if ["denied", "expired", "void"].contains(&status(r)) { return false; }
    if !ignore_done && status(r) == "done" && kind(r) == "action" { return false; }
    if action_hash(r) != s(r, "action_hash").unwrap_or("") { return false; }
    if g(r, "denials").as_array().map(|d| d.iter().any(|x| valid_answer(c, r, x, "no"))).unwrap_or(false) { return false; }
    if kind(r) == "stop" { return true; }
    if kind(r) == "close" { return c.mandate(s(r, "mandate").unwrap_or("")).and_then(|m| s(m, "holder")) == s(r, "asker") || c.root.iter().any(|x| Some(x.as_str()) == s(r, "asker")); }
    let mname = s(r, "mandate").unwrap_or("");
    if kind(r) == "open" && mandate_can(c.mandate(mname).unwrap_or(&V::Null)).perms.is_empty() { return true; }
    if let Some(cov) = s(r, "covered_by") {
        let Some(open) = c.request(cov) else { return false };
        return kind(open) == "open" && s(open, "mandate") == Some(mname) && is_approved(c, open, now, false)
            && permissions_of(r).iter().all(|p| each_for(c, c.mandate(mname), p) == "mandate");
    }
    if g(r, "approvals").as_array().map(|a| a.is_empty()).unwrap_or(true) { return false; }
    complete(&progress(c, r, now), now)
}

fn stopped_by<'a>(c: &'a Company, name: &str, now: i64) -> Option<&'a V> {
    let mut ev: Vec<&V> = c.requests.values().filter(|r| s(r, "mandate") == Some(name) && (kind(r) == "stop" || kind(r) == "resume")).collect();
    ev.sort_by(|a, b| s(a, "created").unwrap_or("").cmp(s(b, "created").unwrap_or("")).then_with(|| if kind(a) == "stop" { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }));
    let mut stop: Option<&V> = None;
    for e in ev {
        if kind(e) == "stop" && is_approved(c, e, now, false) { stop = Some(e); }
        if kind(e) == "resume" { if let Some(st) = stop { if s(e, "created") >= s(st, "created") && is_approved(c, e, now, false) { stop = None; } } }
    }
    stop
}

pub fn mandate_end(m: &V) -> Option<i64> { let e = g(m, "expires"); time_of(if truthy(e) { e } else { g(m, "review") }) }

/// A mandate is active when its current file was opened by a validly approved "open" request,
/// it has not ended, and nobody has broken the glass on it since.
pub fn mandate_state(c: &Company, name: &str, now: i64) -> (bool, String) {
    mandate_state_d(c, name, now, 0)
}
fn mandate_state_d(c: &Company, name: &str, now: i64, depth: usize) -> (bool, String) {
    let Some(m) = c.mandate(name) else { return (false, "no such mandate".into()) };
    if let Some(end) = mandate_end(m) { if now > end + DAY { return (false, format!("ended {}", &iso(end)[..10])); } }
    if let Some(st) = stopped_by(c, name, now) { return (false, format!("stopped by {}: {}", s(st, "asker").unwrap_or(""), s(g(st, "action"), "summary").unwrap_or(""))); }
    let opens: Vec<&V> = c.requests.values().filter(|r| kind(r) == "open" && s(r, "mandate") == Some(name)).collect();
    // a mandate its holder marked done stays closed until it is opened again
    let last_open = opens.iter().filter(|r| s(r, "mandate_hash") == s(m, "hash") && is_approved(c, r, now, false)).map(|r| created(r)).max();
    if let Some(cl) = c.requests.values().filter(|r| kind(r) == "close" && s(r, "mandate") == Some(name)).max_by_key(|r| created(r)) {
        if last_open.is_none_or(|o| created(cl) >= o) && is_approved(c, cl, now, false) { return (false, format!("done: {}", s(g(cl, "action"), "summary").unwrap_or(""))); }
    }
    if last_open.is_some() { return (true, "open".into()); }
    if depth < 50 {
        for p in c.mandates.values() {
            if listk(p, "parts").iter().any(|x| x == name) && mandate_state_d(c, s(p, "name").unwrap_or(""), now, depth + 1).0 { return (true, format!("part of {}", s(p, "name").unwrap_or(""))); }
        }
    }
    (false, if opens.iter().any(|r| is_approved(c, r, now, false)) { "changed since it was approved: needs a new yes".into() } else { "not opened yet".into() })
}

pub struct Problem { pub error: bool, pub r#where: String, pub msg: String }

/// Law checks.
pub fn check(c: &Company, now: i64) -> Vec<Problem> {
    let out: std::cell::RefCell<Vec<Problem>> = Default::default();
    let err = |w: &str, m: String| out.borrow_mut().push(Problem { error: true, r#where: w.into(), msg: m });
    let warn = |w: &str, m: String| out.borrow_mut().push(Problem { error: false, r#where: w.into(), msg: m });
    if c.root.is_empty() { err("root.md", "root lists no one".into()); }
    for r in &c.root { if !c.is_person(r) { err("root.md", format!("root member \"{r}\" is not in people")); } }
    if c.observer.is_none() { warn("root.md", "no observer named: nobody can veto when a quorum cannot be met".into()); }
    for (id, p) in &c.people { if s(p, "key").is_none() { warn("permissions.md", format!("{id} has no signing key: their yes can be forged by editing a file. Run `noroles keygen`")); } }
    for (a, def) in &c.agents { let w = s(def, "works_for").unwrap_or(""); if !c.is_person(w) { err("permissions.md", format!("agent \"{a}\" works for \"{w}\", who is not in people")); } }
    for (gname, l) in &c.groups { for p in list(l) { if !c.is_person(&p) { err("permissions.md", format!("group \"{gname}\" lists \"{p}\", who is not a person (agents cannot hold permissions)")); } } }
    for (p, def) in &c.permissions {
        let hs = humans(c, g(def, "holders"));
        let q = g(def, "quorum").as_i64().unwrap_or(1);
        let at = format!("permissions.md {p}");
        if hs.is_empty() { err(&at, "no human holds this permission (law 3: nothing is unheld)".into()); }
        else if (hs.len() as i64) < q { warn(&at, format!("quorum {q} but only {} holder(s): each missing yes becomes a 24h wait for the observer", hs.len())); }
        for h in listk(def, "holders") { if c.is_agent(&h) { err(&at, format!("agent \"{h}\" is listed as a holder: agents never hold permissions")); } }
    }
    for (name, sv) in &c.servers {
        let at = format!("servers.md {name}");
        if s(sv, "command").is_none() { err(&at, "no command".into()); }
        for cred in obj(sv, "env").values().filter_map(text) { if !c.credentials.contains_key(&cred) { err(&at, format!("env uses unknown credential \"{cred}\"")); } }
        for (t, rule) in obj(sv, "tools") { for p in listk(&rule, "permissions") { if !c.permissions.contains_key(&p) { err(&format!("servers.md {name}.{t}"), format!("unknown permission \"{p}\"")); } } }
    }
    for (pat, rule) in &c.tools {
        if let Err(e) = crate::hook::Pattern::new(pat) { err(&format!("tools.md {pat}"), e); }
        for p in listk(rule, "permissions") { if !c.permissions.contains_key(&p) { err(&format!("tools.md {pat}"), format!("unknown permission \"{p}\"")); } }
    }
    for (name, cr) in &c.credentials {
        for p in listk(cr, "exercises") { if !c.permissions.contains_key(&p) { err(&format!("credentials.md {name}"), format!("exercises unknown permission \"{p}\"")); } }
        if s(cr, "env").is_none() { err(&format!("credentials.md {name}"), "no env variable named".into()); }
    }
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for m in c.mandates.values() {
        let name = s(m, "name").unwrap_or("").to_string();
        let at = format!("mandates/{name}.md");
        for f in ["intent", "metric", "stop_if", "holder", "executor"] { if !truthy(g(m, f)) { err(&at, format!("missing \"{f}\"")); } }
        if let Some(h) = s(m, "holder") { if !c.is_person(h) { err(&at, format!("holder \"{h}\" is not a person")); } }
        for e in listk(m, "executor") { if !c.is_person(&e) && !c.is_agent(&e) { err(&at, format!("executor \"{e}\" is not a known person or agent")); } }
        let end = mandate_end(m);
        match end {
            None => err(&at, "needs \"expires\" (one-off work) or \"review\" (standing duty)".into()),
            Some(e) => { let from = time_of(g(m, "opened")).unwrap_or(now); let days = (e - from) as f64 / DAY as f64; if days > 90.0 { err(&at, format!("ends {} days after it opens: at most 90", days.round())); } }
        }
        let can = mandate_can(m);
        for (p, lim) in &can.perms {
            let Some(def) = c.permissions.get(p) else { err(&at, format!("can names unknown permission \"{p}\"")); continue };
            let base = s(def, "each").unwrap_or("mandate");
            if let Some(e) = s(lim, "each") { if each_rank(e) > each_rank(base) { err(&at, format!("{p}.each = {e} is looser than permissions.md ({base})")); } }
            if let Some(o) = lim.as_object() {
                for (k, v) in o {
                    if let (Some(v), Some(ceil)) = (v.as_f64(), g(g(def, "limits"), k).as_f64()) {
                        if v > ceil { err(&at, format!("{p}.{k} = {} is above the permissions.md ceiling {} (a mandate narrows, never widens)", fmt_num(v), fmt_num(ceil))); }
                    }
                }
            }
        }
        for t in &can.tools {
            let Some(cr) = c.credentials.get(t) else { err(&at, format!("tool \"{t}\" is not in credentials.md")); continue };
            for p in listk(cr, "exercises") { if !can.perms.contains_key(&p) { err(&at, format!("tool \"{t}\" can exercise {p}, but can does not include {p}")); } }
        }
        for sv in &can.servers { if !c.servers.contains_key(sv) { err(&at, format!("server \"{sv}\" is not in servers.md")); } }
        for n in listk(m, "needs") { if !c.mandates.contains_key(&n) { err(&at, format!("needs unknown mandate \"{n}\"")); } }
        for part in listk(m, "parts") {
            let Some(child) = c.mandate(&part) else { err(&at, format!("part \"{part}\" does not exist")); continue };
            let cc = mandate_can(child);
            let pat = format!("mandates/{part}.md");
            for p in cc.perms.keys() { if !can.perms.contains_key(p) { err(&pat, format!("can {p}, which its parent {name} does not have")); } }
            for t in &cc.tools { if !can.tools.contains(t) { err(&pat, format!("tool {t}, which its parent {name} does not have")); } }
            for o in listk(child, "owns") { if !listk(m, "owns").contains(&o) { err(&pat, format!("owns \"{o}\", outside its parent {name}")); } }
        }
        if mandate_state(c, &name, now).0 || end.is_none_or(|e| now <= e) { for o in listk(m, "owns") { owners.entry(o).or_default().push(name.clone()); } }
    }
    for (o, l) in &owners {
        for i in 0..l.len() { for j in i + 1..l.len() { if !family(c, &l[i]).contains(&l[j]) { err(&format!("owns \"{o}\""), format!("claimed by both {} and {} (law 6: one owner per target)", l[i], l[j])); } } }
    }
    // needs cycles
    let mut seen: BTreeMap<String, u8> = BTreeMap::new();
    let mut stack: Vec<String> = vec![];
    fn visit(c: &Company, n: &str, seen: &mut BTreeMap<String, u8>, stack: &mut Vec<String>, err: &mut dyn FnMut(&str, String)) {
        match seen.get(n) {
            Some(1) => { let mut p = stack.clone(); p.push(n.into()); err(&format!("mandates/{n}.md"), format!("needs cycle: {}", p.join(" -> "))); return }
            Some(2) => return,
            _ => {}
        }
        let Some(m) = c.mandate(n) else { return };
        seen.insert(n.into(), 1); stack.push(n.into());
        for x in listk(m, "needs") { visit(c, &x, seen, stack, err); }
        stack.pop(); seen.insert(n.into(), 2);
    }
    let mut e2 = |w: &str, m: String| err(w, m);
    for n in c.mandates.keys() { visit(c, n, &mut seen, &mut stack, &mut e2); }
    for f in META_FILES {
        let Ok(t) = read_text(&c.dir, f) else { continue };
        if let Some(known) = s(g(&c.ledger, "meta"), f) { if known != sha(&t) { err(f, "changed outside NoRoles. Run `noroles propose-meta` and get a rule.change yes".into()); } }
    }
    out.into_inner()
}
