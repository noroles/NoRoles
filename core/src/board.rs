//! The board: every mandate as one card with one status, in plain words.
//! A person writes what should happen; an agent turns it into a mandate; one press starts it.
//! Permissions, requests and signatures stay underneath.
use crate::company::*;
use crate::requests::{self, Proof};
use crate::util::*;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// What each permission means, for people who never read permissions.md.
pub fn label(p: &str) -> String {
    match p {
        "speak.external" => "Send emails and messages outside the company",
        "money.spend" => "Spend money",
        "money.pay_out" => "Pay partners and contractors",
        "ads.change" => "Change ad budgets and campaigns",
        "price.change" => "Change prices",
        "contract.sign" => "Sign contracts",
        "prod.change" => "Change the live product",
        "data.export" => "Share personal data",
        "access.change" => "Share files or invite people",
        "people.engage" => "Hire or engage people",
        "destroy" => "Delete things",
        "rule.change" => "Change the company rules",
        other => return other.to_string(),
    }.to_string()
}

/// The one column a mandate sits in.
pub fn column(c: &Company, dir: &Path, name: &str, runs: &[V], now: i64) -> (&'static str, String) {
    let pending: Vec<&V> = c.requests.values().filter(|r| s(r, "mandate") == Some(name) && effective(c, r, now).0 == "pending").collect();
    if let Some(r) = pending.first() {
        let what = if kind(r) == "open" { "Waiting for your yes to start".to_string() } else { format!("Waiting for your yes: {}", s(g(r, "action"), "summary").unwrap_or("")) };
        return ("needs", what);
    }
    let (active, why) = mandate_state(c, name, now);
    let incident = requests::incidents(dir).into_iter().find(|i| s(i, "mandate") == Some(name) && !truthy(g(i, "resolved")));
    if why.starts_with("stopped") { return ("blocked", why); }
    if let Some(i) = incident { if active { return ("blocked", s(&i, "what").unwrap_or("").to_string()); } }
    if let Some(rest) = why.strip_prefix("done: ") { return ("done", if rest == "done" { "Done".into() } else { rest.to_string() }); }
    if why.starts_with("ended") { return ("done", why); }
    let declined = c.requests.values().filter(|r| kind(r) == "open" && s(r, "mandate") == Some(name)).max_by_key(|r| created(r)).filter(|r| status(r) == "denied" || status(r) == "expired");
    if !active {
        if let Some(d) = declined { return ("done", format!("Not started: {}", if status(d) == "expired" { "nobody answered".to_string() } else { g(d, "denials").get(0).and_then(|x| s(x, "reason")).unwrap_or("declined").to_string() })); }
        return ("planned", if why == "not opened yet" { "Not started yet".into() } else { why });
    }
    let mine: Vec<&V> = runs.iter().filter(|r| s(r, "mandate") == Some(name)).collect();
    if let Some(r) = mine.first() {
        match s(r, "status") {
            Some("working") => return ("working", "The agent is working on it".into()),
            Some("waiting for you") => return ("needs", "The agent waits for your yes".into()),
            Some("failed") => return ("blocked", "The agent stopped with an error".into()),
            _ => {}
        }
        if let Some(res) = s(r, "result") { return ("working", res.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_start_matches(['#', '*', ' ']).chars().take(140).collect()); }
    }
    ("working", "Started".into())
}

pub fn cards(c: &Company, dir: &Path, runs: &[V], now: i64) -> Vec<V> {
    c.mandates.values().map(|m| {
        let name = s(m, "name").unwrap_or("");
        let (col, line) = column(c, dir, name, runs, now);
        let can = mandate_can(m);
        json!({
            "name": name, "title": text(g(m, "title")).unwrap_or_else(|| name.replace('-', " ")),
            "goal": g(m, "intent"), "done_when": g(m, "metric"), "stop_if": g(m, "stop_if"),
            "holder": g(m, "holder"), "executor": listk(m, "executor"),
            "agent": listk(m, "executor").iter().any(|e| c.is_agent(e)),
            "needs_yes": can.perms.iter().map(|(p, l)| json!({ "permission": p, "label": label(p), "amount": g(l, "amount") })).collect::<Vec<_>>(),
            "ends": mandate_end(m).map(iso), "check_in": text(g(m, "check_in")).unwrap_or_else(|| "none".into()),
            "column": col, "line": line, "body": g(m, "body"),
        })
    }).collect()
}

// ---------- from an idea to a mandate ----------

fn slug(x: &str) -> String {
    let mut out = String::new();
    for ch in x.to_lowercase().chars() { if ch.is_ascii_alphanumeric() { out.push(ch) } else if !out.ends_with('-') && !out.is_empty() { out.push('-') } }
    let t: String = out.trim_matches('-').chars().take(40).collect();
    if t.is_empty() { "mandate".into() } else { t.trim_end_matches('-').to_string() }
}

/// Ask an agent to turn an idea (typed, dictated, or pointing at a document) into a mandate draft.
pub fn draft(dir: &Path, idea: &str, who: &str, me: &str) -> Result<V, String> {
    if idea.trim().is_empty() { return Err("write what should happen first".into()); }
    let c = load(dir)?;
    let perms: Vec<String> = c.permissions.keys().filter(|p| *p != "rule.change").map(|p| format!("- {p}: {}", label(p))).collect();
    let people: Vec<String> = c.people.iter().map(|(id, p)| format!("- {id}: {}", text(g(p, "name")).unwrap_or_default())).collect();
    let today = &iso(now_ms())[..10];
    let prompt = format!(r#"Turn this idea into a work mandate for a company that runs on NoRoles. Today is {today}. The person asking is {me}.

The idea, in their words:
"""
{idea}
"""
Who should do it: {who}

If the idea points to a document, a page, a thread or a ticket, read it first with your tools (read only: never send, create or change anything). Use only facts you found or were given; never invent numbers, names or dates. If something is unclear, say so in "unclear".

Things that need a person's yes, and their keys (use only these keys):
{}

People:
{}

Reply with ONLY one JSON object, no other text:
{{"title": "2-4 words, plain", "goal": "one sentence: the outcome", "done_when": "how anyone can tell it is done", "stop_if": "when to stop and ask", "needs_yes": ["keys from the list the work will really need"], "limits": {{"money.spend": 0}}, "kind": "one-off or ongoing", "days": 30, "check_in": "none, daily or weekly", "first_step": "what the agent does first, as an instruction", "may_do_alone": ["short things it can do without asking"], "unclear": "what you had to assume, or empty", "language": "the language of the idea"}}
Write every text field in the language of the idea. "days" is at most 90. Leave "limits" empty unless money is involved and the idea names an amount."#, perms.join("\n"), people.join("\n"));
    let out = Command::new("claude")
        .arg("-p").arg(&prompt)
        .args(["--model", "sonnet", "--output-format", "json", "--permission-mode", "bypassPermissions", "--disallowedTools", "Bash,Write,Edit,NotebookEdit", "--max-budget-usd", "1"])
        .current_dir(dir).stdin(Stdio::null()).output().map_err(|e| format!("cannot start claude: {e}"))?;
    let v: V = serde_json::from_slice(&out.stdout).map_err(|_| format!("the agent gave no answer{}", String::from_utf8_lossy(&out.stderr).lines().last().map(|l| format!(": {l}")).unwrap_or_default()))?;
    let result = s(&v, "result").unwrap_or("");
    let (a, b) = (result.find('{'), result.rfind('}'));
    let (Some(a), Some(b)) = (a, b) else { return Err(format!("the agent did not return a draft: {}", result.chars().take(300).collect::<String>())) };
    let mut d: V = serde_json::from_str(&result[a..=b]).map_err(|e| format!("the draft was not readable: {e}"))?;
    let keep: Vec<V> = listk(&d, "needs_yes").into_iter().filter(|p| c.permissions.contains_key(p) && p != "rule.change").map(V::from).collect();
    d["needs_yes"] = V::from(keep);
    d["who"] = V::from(who);
    Ok(d)
}

/// Write the mandate, sign the yes that opens it, and hand it to the agent. One press.
pub fn create(dir: &Path, me: &str, d: &V, passphrase: &str, start: bool) -> Result<V, String> {
    let c = load(dir)?;
    let title = s(d, "title").filter(|t| !t.trim().is_empty()).ok_or("give it a name")?.trim().to_string();
    let goal = s(d, "goal").filter(|t| !t.trim().is_empty()).ok_or("say what the goal is")?;
    let mut name = slug(&title);
    let base = name.clone();
    let mut i = 2;
    while c.mandates.contains_key(&name) || dir.join("mandates").join(format!("{name}.md")).exists() { name = format!("{base}-{i}"); i += 1; }
    let who = s(d, "who").unwrap_or("agent");
    let executor = if who == "agent" {
        c.agents.iter().find(|(_, a)| s(a, "works_for") == Some(me)).map(|(id, _)| id.clone()).ok_or("you have no agent in permissions.md")?
    } else if c.is_person(who) { who.to_string() } else { return Err(format!("\"{who}\" is not in the company")); };
    let mut can = Obj::new();
    for p in listk(d, "needs_yes") {
        if !c.permissions.contains_key(&p) { continue; }
        let amount = g(g(d, "limits"), &p).as_f64().filter(|a| *a > 0.0);
        can.insert(p, amount.map(|a| json!({ "amount": num(a) })).unwrap_or(json!({})));
    }
    let days = g(d, "days").as_f64().unwrap_or(30.0).clamp(1.0, 90.0) as i64;
    let end = iso(now_ms() + days * DAY)[..10].to_string();
    let ongoing = s(d, "kind").is_some_and(|k| k.starts_with("ongoing"));
    let mut fm = json!({
        "title": title, "intent": goal, "metric": s(d, "done_when").filter(|x| !x.trim().is_empty()).unwrap_or(goal),
        "stop_if": s(d, "stop_if").filter(|x| !x.trim().is_empty()).unwrap_or("something is unclear or would need more than this mandate allows"),
        "holder": me, "executor": executor, "owns": [name], "can": can,
        "check_in": s(d, "check_in").filter(|x| ["daily", "weekly"].contains(x)).unwrap_or("none"),
    });
    fm[if ongoing { "review" } else { "expires" }] = V::from(end);
    let mut body = String::new();
    if let Some(f) = s(d, "first_step").filter(|x| !x.trim().is_empty()) { body.push_str(&format!("First: {f}\n\n")); }
    let alone = listk(d, "may_do_alone");
    if !alone.is_empty() { body.push_str(&format!("Without asking: {}.\n", alone.join("; "))); }
    fs::create_dir_all(dir.join("mandates")).map_err(|e| e.to_string())?;
    fs::write(dir.join("mandates").join(format!("{name}.md")), format!("---\n{}---\n{body}", to_yaml(&fm))).map_err(|e| e.to_string())?;
    if dir.join(".git").exists() {
        let f = format!("mandates/{name}.md");
        let _ = Command::new("git").args(["add", "--", &f]).current_dir(dir).output();
        let _ = Command::new("git").args(["commit", "-q", "-m", &format!("noroles: new mandate {name}"), "--", &f]).current_dir(dir).output();
    }
    if !start { return Ok(json!({ "name": name, "status": "planned" })); }
    open(dir, me, &name, passphrase)
}

/// Open a mandate and, when an agent does it, give the agent its first step.
pub fn open(dir: &Path, me: &str, name: &str, passphrase: &str) -> Result<V, String> {
    let now = now_ms();
    let r = requests::open_mandate(dir, name, me, now)?;
    if status(&r) == "pending" {
        let c = load(dir)?;
        let proof = if c.people.get(me).and_then(|p| s(p, "key")).is_some() { Proof::Passphrase(passphrase) } else { Proof::None };
        requests::decide(dir, s(&r, "id").unwrap_or(""), me, true, None, proof, now)
            .map_err(|e| if e.starts_with("wrong passphrase") { "Wrong passphrase: the mandate is saved but not started.".to_string() } else { e })?;
    }
    let c = load(dir)?;
    let m = c.mandate(name).cloned().unwrap_or(V::Null);
    let agent = listk(&m, "executor").iter().any(|e| c.is_agent(e));
    if agent && crate::runs::signed_in() {
        let first = text(g(&m, "body")).and_then(|b| b.lines().find_map(|l| l.strip_prefix("First: ").map(String::from))).unwrap_or_else(|| format!("Start working toward the goal: {}", text(g(&m, "intent")).unwrap_or_default()));
        crate::runs::start(dir, name, &first)?;
    }
    Ok(json!({ "name": name, "status": "started" }))
}

// ---------- check-ins ----------

/// Mandates that ask for check-ins get one from their agent: daily or weekly, from 8 in the morning.
pub fn check_ins(dir: &Path) {
    let now = now_ms();
    let Ok(c) = load(dir) else { return };
    if !crate::runs::signed_in() { return; }
    use chrono::Timelike;
    if chrono::Local::now().hour() < 8 { return; }
    let runs = crate::runs::list(dir);
    for m in c.mandates.values() {
        let name = s(m, "name").unwrap_or("");
        let every = match s(m, "check_in") { Some("daily") => DAY - HOUR, Some("weekly") => 7 * DAY - HOUR, _ => continue };
        if !mandate_state(&c, name, now).0 || !listk(m, "executor").iter().any(|e| c.is_agent(e)) { continue; }
        let mine: Vec<&V> = runs.iter().filter(|r| s(r, "mandate") == Some(name)).collect();
        if mine.iter().any(|r| matches!(s(r, "status"), Some("working") | Some("waiting for you"))) { continue; }
        let last = mine.iter().filter_map(|r| time_of(g(r, "started"))).max().unwrap_or(0);
        if now - last < every { continue; }
        let _ = crate::runs::start(dir, name, "Check-in. Go on with the work toward the goal. Then report in three short lines: what got done since the last check-in, what is next, what is blocked or waits for a yes.");
    }
}
