//! Asking for a yes, recording it honestly, and acting only on what was approved.
use crate::company::*;
use crate::flags::flags;
use crate::keys;
use crate::notify::notify;
use crate::util::*;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::Command;

type R<T> = Result<T, String>;

pub fn save(c: &mut Company, r: &V) -> R<()> {
    let d = c.dir.join("requests");
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let id = s(r, "id").unwrap_or("").to_string();
    fs::write(d.join(format!("{id}.yaml")), to_yaml(r)).map_err(|e| e.to_string())?;
    c.requests.insert(id, r.clone());
    Ok(())
}

// NoRoles commits only its own record, never other work lying around in the folder.
const RECORD: [&str; 4] = ["requests", "incidents", "log", "ledger.json"];

fn git(dir: &Path, args: &[&str]) -> R<String> {
    let o = Command::new("git").args(args).current_dir(dir).output().map_err(|e| e.to_string())?;
    if !o.status.success() { return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim())); }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

pub fn commit(dir: &Path, msg: &str, all: bool) -> R<()> {
    if !dir.join(".git").exists() { return Ok(()); }
    let paths: Vec<&str> = if all { vec!["-A"] } else { RECORD.iter().copied().filter(|p| dir.join(p).exists()).collect() };
    if !all && paths.is_empty() { return Ok(()); }
    let mut add = vec!["add"]; if !all { add.push("--"); } add.extend(&paths);
    git(dir, &add)?;
    let mut diff = vec!["diff", "--cached", "--name-only"]; if !all { diff.push("--"); diff.extend(&paths); }
    let staged = git(dir, &diff)?;
    let staged: Vec<&str> = staged.lines().filter(|l| !l.is_empty()).collect();
    if staged.is_empty() { return Ok(()); }
    let m = format!("noroles: {msg}");
    let mut args = vec!["commit", "-q", "-m", &m];
    if !all { args.push("--"); args.extend(&staged); }
    git(dir, &args)?;
    Ok(())
}

fn new_id(now: i64, p: &str) -> String {
    let mut b = [0u8; 3];
    let _ = getrandom::fill(&mut b);
    format!("{p}-{}-{}", iso(now)[..10].replace('-', ""), hex::encode(b))
}

fn base(kind: &str, mandate: V, permissions: Vec<String>, asker: &str, action: V, now: i64) -> V {
    let h = sha(&canonical(&action));
    json!({ "id": new_id(now, "r"), "kind": kind, "mandate": mandate, "permissions": permissions, "asker": asker, "created": iso(now), "status": "pending", "action": action, "action_hash": h, "approvals": [], "denials": [] })
}

fn finish(c: &mut Company, r: &mut V) -> R<()> {
    r["status"] = V::from("approved");
    if kind(r) == "meta" {
        let mut meta = obj(&c.ledger, "meta");
        for (k, v) in obj(g(r, "action"), "hashes") { meta.insert(k, v); }
        c.ledger["meta"] = V::Object(meta);
        fs::write(c.dir.join("ledger.json"), serde_json::to_string_pretty(&c.ledger).unwrap() + "\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Default)]
pub struct Ask { pub mandate: String, pub permissions: Vec<String>, pub asker: String, pub summary: String, pub amount: Option<f64>, pub currency: Option<String>, pub to: Option<String>, pub payload: V, pub items: Option<Vec<String>>, pub tool: Option<String>, pub command: Option<Vec<String>> }

pub fn ask(dir: &Path, a: Ask, now: i64) -> R<V> {
    let mut c = load(dir)?;
    let m = c.mandate(&a.mandate).ok_or_else(|| format!("no mandate \"{}\"", a.mandate))?.clone();
    if !c.is_person(&a.asker) && !c.is_agent(&a.asker) { return Err(format!("\"{}\" is not a known person or agent", a.asker)); }
    if !listk(&m, "executor").contains(&a.asker) && s(&m, "holder") != Some(&a.asker) { return Err(format!("\"{}\" is not an executor or holder of {}", a.asker, a.mandate)); }
    let (active, why) = mandate_state(&c, &a.mandate, now);
    if !active { return Err(format!("mandate {} is not active: {why}", a.mandate)); }
    if a.permissions.is_empty() { return Err("name at least one permission".into()); }
    let action = json!({
        "summary": a.summary, "amount": a.amount.map(num).unwrap_or(V::Null), "currency": a.currency, "to": a.to,
        "payload": a.payload, "items": a.items, "tool": a.tool, "command": a.command,
    });
    let mut r = base("action", V::from(a.mandate.clone()), a.permissions.clone(), &a.asker, action, now);
    if let Some(p) = limit_problem(&c, &r, now) { return Err(p); }
    // each: mandate means the yes that opened the mandate already covers every action inside its limits
    if a.permissions.iter().all(|p| each_for(&c, Some(&m), p) == "mandate") {
        let open = c.requests.values().find(|o| kind(o) == "open" && s(o, "mandate") == Some(&a.mandate) && s(o, "mandate_hash") == s(&m, "hash") && is_approved(&c, o, now, false)).and_then(|o| s(o, "id")).map(String::from);
        if let Some(o) = open { r["covered_by"] = V::from(o); r["status"] = V::from("approved"); }
    }
    save(&mut c, &r)?;
    let summary = s(g(&r, "action"), "summary").unwrap_or("").to_string();
    let cov = s(&r, "covered_by").map(|x| format!(" (covered by {x})")).unwrap_or_default();
    commit(dir, &format!("ask {} {} for {}: {summary}{cov}", s(&r, "id").unwrap(), a.permissions.join(","), a.mandate), false)?;
    if status(&r) == "pending" {
        let f = flags(&c, &r);
        let money = a.permissions.iter().any(|p| p.starts_with("money."));
        let amt = a.amount.map(|x| format!(" ({}{})", fmt_num(x), a.currency.as_ref().map(|c| format!(" {c}")).unwrap_or_default())).unwrap_or_default();
        let fl = if f.is_empty() { String::new() } else { format!("\n! {}", f.join("\n! ")) };
        notify(&format!("NoRoles: {} needs a yes", a.asker), &format!("{}: {summary}{amt}{fl}\nnoroles yes {}", a.permissions.join(", "), s(&r, "id").unwrap()), money || !f.is_empty());
    }
    Ok(r)
}

fn problems_for(c: &Company, mandate: &str, now: i64) -> Vec<Problem> {
    check(c, now).into_iter().filter(|x| x.error && (x.r#where.contains(&format!("mandates/{mandate}.md")) || x.r#where.starts_with("owns"))).collect()
}

pub fn open_mandate(dir: &Path, mandate: &str, asker: &str, now: i64) -> R<V> {
    let mut c = load(dir)?;
    let m = c.mandate(mandate).ok_or_else(|| format!("no mandate \"{mandate}\""))?.clone();
    let p = problems_for(&c, mandate, now);
    if !p.is_empty() { return Err(format!("fix these first:\n{}", p.iter().map(|x| format!("  {}: {}", x.r#where, x.msg)).collect::<Vec<_>>().join("\n"))); }
    let perms: Vec<String> = mandate_can(&m).perms.keys().cloned().collect();
    let action = json!({ "summary": format!("open mandate {mandate}: {}", text(g(&m, "intent")).unwrap_or_default()), "mandate_hash": g(&m, "hash"), "can": if truthy(g(&m, "can")) { g(&m, "can").clone() } else { json!({}) } });
    let mut r = base("open", V::from(mandate), perms.clone(), asker, action, now);
    r["mandate_hash"] = g(&m, "hash").clone();
    if perms.is_empty() { r["status"] = V::from("approved"); }
    save(&mut c, &r)?;
    commit(dir, &format!("open {mandate} ({})", s(&r, "id").unwrap()), false)?;
    Ok(r)
}

pub fn propose_meta(dir: &Path, asker: &str, now: i64) -> R<V> {
    let mut c = load(dir)?;
    let mut hashes = Obj::new();
    for f in META_FILES { if let Ok(t) = read_text(dir, f) { hashes.insert(f.into(), V::from(sha(&t))); } }
    let known = g(&c.ledger, "meta").clone();
    let changed: Vec<String> = hashes.iter().filter(|(f, h)| s(&known, f) != h.as_str()).map(|(f, _)| f.clone()).collect();
    if changed.is_empty() { return Err("no meta file has changed".into()); }
    let r = base("meta", V::Null, vec!["rule.change".into()], asker, json!({ "summary": format!("accept changes to {}", changed.join(", ")), "hashes": hashes }), now);
    save(&mut c, &r)?;
    commit(dir, &format!("propose meta change {}: {}", s(&r, "id").unwrap(), changed.join(", ")), false)?;
    Ok(r)
}

/// Break glass: anyone may stop a mandate to limit harm. It never switches a control off.
pub fn stop(dir: &Path, mandate: &str, asker: &str, reason: &str, now: i64) -> R<V> {
    let mut c = load(dir)?;
    if c.mandate(mandate).is_none() { return Err(format!("no mandate \"{mandate}\"")); }
    if !c.is_person(asker) && !c.is_agent(asker) { return Err(format!("\"{asker}\" is not a known person or agent")); }
    if reason.trim().is_empty() { return Err("say why: the holder reviews every stop within 24h".into()); }
    let mut r = base("stop", V::from(mandate), vec![], asker, json!({ "summary": reason }), now);
    r["status"] = V::from("approved");
    save(&mut c, &r)?;
    open_incident(&c, &format!("stop-{}", s(&r, "id").unwrap()), Some(mandate), &format!("stopped by {asker}: {reason}. Holder reviews within 24h."), now)?;
    commit(dir, &format!("stop {mandate} by {asker}: {reason}"), false)?;
    Ok(r)
}

/// Only the mandate's holder (or root) can undo a stop.
pub fn resume(dir: &Path, mandate: &str, asker: &str, now: i64) -> R<V> {
    let mut c = load(dir)?;
    if c.mandate(mandate).is_none() { return Err(format!("no mandate \"{mandate}\"")); }
    let r = base("resume", V::from(mandate), vec![], asker, json!({ "summary": format!("resume {mandate}") }), now);
    save(&mut c, &r)?;
    commit(dir, &format!("ask to resume {mandate} ({})", s(&r, "id").unwrap()), false)?;
    Ok(r)
}

// ---------- answering ----------

pub enum Proof<'a> { Passphrase(&'a str), Sig(String), None }

pub fn decide(dir: &Path, id: &str, who: &str, yes: bool, reason: Option<&str>, proof: Proof, now: i64) -> R<V> {
    let mut c = load(dir)?;
    let mut r = c.request(id).cloned().ok_or_else(|| format!("no request {id}"))?;
    let person = c.people.get(who).cloned().ok_or_else(|| format!("\"{who}\" is not a person: only people say yes or no"))?;
    let (st, _) = effective(&c, &r, now);
    if st != "pending" { return Err(format!("{id} is {st}")); }
    if action_hash(&r) != s(&r, "action_hash").unwrap_or("") { return Err(format!("{id} was edited after it was asked: void")); }
    let prog = progress(&c, &r, now);
    if !prog.iter().any(|p| p.can.iter().any(|x| x == who)) {
        let ps = permissions_of(&r);
        return Err(format!("{who} holds none of {} for this request", if ps.is_empty() { "this mandate".into() } else { ps.join(", ") }));
    }
    let answered = |k: &str| g(&r, k).as_array().map(|a| a.iter().any(|x| s(x, "by") == Some(who))).unwrap_or(false);
    if answered("approvals") || answered("denials") { return Err(format!("{who} already answered {id}")); }
    let answer = if yes { "yes" } else { "no" };
    let at = iso(now);
    let mut entry = json!({ "by": who, "at": at });
    if s(&person, "key").is_some() {
        let st = keys::statement(id, s(&r, "action_hash").unwrap_or(""), answer, &at);
        let sig = match proof { Proof::Sig(x) => Some(x), Proof::Passphrase(p) => Some(keys::sign(who, p, &st)?), Proof::None => None };
        entry["sig"] = sig.map(V::from).unwrap_or(V::Null);
        if !valid_answer(&c, &r, &entry, answer) { return Err(format!("{who}: the signature does not match the key in permissions.md")); }
    }
    if !yes {
        entry["reason"] = reason.map(V::from).unwrap_or(V::Null);
        r["denials"].as_array_mut().unwrap().push(entry);
        r["status"] = V::from("denied");
        save(&mut c, &r)?;
        commit(dir, &format!("no {id} by {who}{}", reason.map(|x| format!(": {x}")).unwrap_or_default()), false)?;
        return Ok(r);
    }
    let mname = s(&r, "mandate").unwrap_or("").to_string();
    if kind(&r) == "action" {
        let (active, why) = mandate_state(&c, &mname, now);
        if !active { return Err(format!("mandate {mname} is not active: {why}")); }
        if let Some(p) = limit_problem(&c, &r, now) { return Err(p); }
    }
    if kind(&r) == "open" && c.mandate(&mname).and_then(|m| s(m, "hash")) != s(&r, "mandate_hash") { return Err(format!("mandate {mname} changed after it was asked: ask again")); }
    if kind(&r) == "meta" {
        for (f, h) in obj(g(&r, "action"), "hashes") { if read_text(dir, &f).map(|t| sha(&t)).ok().as_deref() != h.as_str() { return Err(format!("{f} changed again after this was asked: propose again")); } }
    }
    r["approvals"].as_array_mut().unwrap().push(entry);
    if complete(&progress(&c, &r, now), now) { finish(&mut c, &mut r)?; }
    save(&mut c, &r)?;
    commit(dir, &format!("yes {id} by {who}{}", if status(&r) == "approved" { " (approved)" } else { "" }), false)?;
    Ok(r)
}

// ---------- time: waits, silence, dues, reviews ----------

pub fn open_incident(c: &Company, key: &str, mandate: Option<&str>, what: &str, now: i64) -> R<bool> {
    let f = c.dir.join("incidents").join(format!("{key}.yaml"));
    if f.exists() { return Ok(false); }
    let holder = mandate.and_then(|m| c.mandate(m)).and_then(|m| s(m, "holder")).map(String::from).or_else(|| c.root.first().cloned());
    fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    let inc = json!({ "id": key, "mandate": mandate, "what": what, "opened": iso(now), "holder": holder, "then": c.root, "resolved": null });
    fs::write(&f, to_yaml(&inc)).map_err(|e| e.to_string())?;
    notify("NoRoles: incident", &format!("{}{what}", mandate.map(|m| format!("{m}: ")).unwrap_or_default()), true);
    Ok(true)
}

pub fn incidents(dir: &Path) -> Vec<V> {
    let d = dir.join("incidents");
    let mut names: Vec<String> = fs::read_dir(&d).map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| f.ends_with(".yaml")).collect()).unwrap_or_default();
    names.sort();
    names.iter().filter_map(|f| fs::read_to_string(d.join(f)).ok()).filter_map(|t| yaml(&t, "incident").ok()).collect()
}

pub fn resolve_incident(dir: &Path, id: &str, who: &str, note: &str, now: i64) -> R<V> {
    let c = load(dir)?;
    let f = dir.join("incidents").join(format!("{id}.yaml"));
    let mut inc = fs::read_to_string(&f).ok().and_then(|t| yaml(&t, id).ok()).ok_or_else(|| format!("no incident {id}"))?;
    if !c.is_person(who) { return Err("only people resolve incidents".into()); }
    let holder = s(&inc, "holder").unwrap_or("").to_string();
    if who != holder && !c.root.iter().any(|r| r == who) { return Err(format!("only {holder} or root can resolve {id}")); }
    if note.trim().is_empty() { return Err("say what happened: resolve or explain".into()); }
    inc["resolved"] = json!({ "by": who, "at": iso(now), "note": note });
    fs::write(&f, to_yaml(&inc)).map_err(|e| e.to_string())?;
    commit(dir, &format!("resolve {id} by {who}: {note}"), false)?;
    Ok(inc)
}

/// Promote requests whose waits passed, close silent ones as a no, and open incidents for missed dues and reviews.
pub fn settle(dir: &Path, now: i64) -> R<Vec<String>> {
    let mut c = load(dir)?;
    let mut changed = vec![];
    let ids: Vec<String> = c.requests.keys().cloned().collect();
    for id in ids {
        let mut r = c.requests[&id].clone();
        if status(&r) != "pending" { continue; }
        if effective(&c, &r, now).0 == "expired" {
            r["status"] = V::from("expired"); save(&mut c, &r)?; changed.push(format!("{id} expired: silence is a no"));
            if kind(&r) == "action" { open_incident(&c, &format!("silent-{id}"), s(&r, "mandate"), &format!("nobody answered {id} ({})", s(g(&r, "action"), "summary").unwrap_or("")), now)?; }
            continue;
        }
        if g(&r, "approvals").as_array().map(|a| a.is_empty()).unwrap_or(true) || !complete(&progress(&c, &r, now), now) { continue; }
        let moved = kind(&r) == "meta" && obj(g(&r, "action"), "hashes").iter().any(|(f, h)| read_text(dir, f).map(|t| sha(&t)).ok().as_deref() != h.as_str());
        if moved || action_hash(&r) != s(&r, "action_hash").unwrap_or("") { r["status"] = V::from("void"); save(&mut c, &r)?; changed.push(format!("{id} void: changed during the wait")); continue; }
        finish(&mut c, &mut r)?; save(&mut c, &r)?; changed.push(format!("{id} approved after the wait"));
    }
    let ms: Vec<V> = c.mandates.values().cloned().collect();
    for m in ms {
        let name = s(&m, "name").unwrap_or("").to_string();
        for (i, d) in g(&m, "due").as_array().cloned().unwrap_or_default().iter().enumerate() {
            let date_v = if d.is_object() { g(d, "date").clone() } else { d.clone() };
            let Some(date) = time_of(&date_v) else { continue };
            if truthy(g(d, "notice")) || date >= now { continue; }
            let day = &iso(date)[..10];
            let what = if d.is_object() { text(g(d, "what")).unwrap_or_else(|| d.to_string()) } else { text(d).unwrap_or_default() };
            if open_incident(&c, &format!("due-{name}-{i}-{day}"), Some(&name), &format!("missed due {day}: {what}"), now)? { changed.push(format!("{name}: missed due")); }
        }
        if let Some(rv) = time_of(g(&m, "review")) {
            if rv < now {
                let d = text(g(&m, "review")).unwrap_or_default();
                let d = &d[..d.len().min(10)];
                if open_incident(&c, &format!("review-{name}-{d}"), Some(&name), &format!("review date {d} passed: root renews or ends it"), now)? { changed.push(format!("{name}: review passed")); }
            }
        }
    }
    if !changed.is_empty() { commit(dir, &changed.join("; "), false)?; }
    Ok(changed)
}

// ---------- acting ----------

pub fn read_secrets(dir: &Path) -> Obj {
    let mut out = Obj::new();
    let Ok(t) = fs::read_to_string(dir.join(".noroles").join("secrets.env")) else { return out };
    for line in t.lines() {
        let l = line.trim();
        let Some((k, v)) = l.split_once('=') else { continue };
        let k = k.trim();
        if k.is_empty() || !k.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_') { continue; }
        let v = v.trim();
        let v = v.strip_prefix(['"', '\'']).unwrap_or(v);
        let v = v.strip_suffix(['"', '\'']).unwrap_or(v);
        out.insert(k.into(), V::from(v));
    }
    out
}

/// Environment with every known credential removed, plus only the ones given.
fn clean_env(c: &Company, give: &[String], extra: &[(&str, String)]) -> R<Vec<(String, String)>> {
    let secrets = read_secrets(&c.dir);
    let known: Vec<String> = c.credentials.values().filter_map(|cr| s(cr, "env").map(String::from)).collect();
    let mut env: Vec<(String, String)> = std::env::vars().filter(|(k, _)| !known.contains(k)).collect();
    for t in give {
        let e = c.credentials.get(t).and_then(|cr| s(cr, "env")).unwrap_or("").to_string();
        let v = secrets.get(&e).and_then(|x| x.as_str()).ok_or_else(|| format!("no value for {e} in .noroles/secrets.env"))?;
        env.push((e, v.into()));
    }
    for (k, v) in extra { env.push((k.to_string(), v.clone())); }
    Ok(env)
}

fn law_errors(c: &Company, now: i64) -> R<()> {
    let e: Vec<Problem> = check(c, now).into_iter().filter(|x| x.error).collect();
    if e.is_empty() { return Ok(()); }
    Err(format!("the company files break the laws; run `noroles check`:\n{}", e.iter().map(|x| format!("  {}: {}", x.r#where, x.msg)).collect::<Vec<_>>().join("\n")))
}

fn spawn(argv: &[String], env: Vec<(String, String)>) -> i32 {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).env_clear().envs(env);
    cmd.status().ok().and_then(|s| s.code()).unwrap_or(1)
}

/// Run an executor inside a mandate with only the open tools its mandate names.
pub fn run(dir: &Path, mandate: &str, who: &str, argv: &[String], now: i64) -> R<i32> {
    settle(dir, now)?;
    let c = load(dir)?;
    let (active, why) = mandate_state(&c, mandate, now);
    if !active { return Err(format!("mandate {mandate} is not active: {why}")); }
    let m = c.mandate(mandate).unwrap();
    if !listk(m, "executor").iter().any(|e| e == who) { return Err(format!("{who} is not an executor of {mandate}")); }
    law_errors(&c, now)?;
    let open: Vec<String> = mandate_can(m).tools.into_iter().filter(|t| c.credentials.get(t).map(|cr| listk(cr, "exercises").is_empty()).unwrap_or(false)).collect();
    let env = clean_env(&c, &open, &[("NOROLES_EXECUTOR", who.into()), ("NOROLES_MANDATE", mandate.into()), ("NOROLES_DIR", dir.display().to_string())])?;
    log_line(dir, "runs.jsonl", &json!({ "at": iso(now), "mandate": mandate, "as": who, "argv": argv, "tools": open }))?;
    commit(dir, &format!("run {mandate} as {who}: {}", argv.join(" ")), false)?;
    Ok(spawn(argv, env))
}

/// Carry out one approved action, once, with the single credential it was approved for.
pub fn do_action(dir: &Path, id: &str, now: i64) -> R<i32> {
    settle(dir, now)?;
    let mut c = load(dir)?;
    let mut r = c.request(id).filter(|r| kind(r) == "action").cloned().ok_or_else(|| format!("no action request {id}"))?;
    if status(&r) == "done" { return Err(format!("{id} was already carried out")); }
    if action_hash(&r) != s(&r, "action_hash").unwrap_or("") { return Err(format!("{id} was edited after it was asked: void")); }
    if !is_approved(&c, &r, now, false) { return Err(format!("{id} is {}, not approved", effective(&c, &r, now).0)); }
    let mname = s(&r, "mandate").unwrap_or("").to_string();
    let (active, why) = mandate_state(&c, &mname, now);
    if !active { return Err(format!("mandate {mname} is not active: {why}")); }
    law_errors(&c, now)?;
    let cmdv = listk(g(&r, "action"), "command");
    if cmdv.is_empty() { return Err(format!("{id} has no command to run; the approved action is done by hand")); }
    let give: Vec<String> = s(g(&r, "action"), "tool").map(|t| vec![t.to_string()]).unwrap_or_default();
    let env = clean_env(&c, &give, &[("NOROLES_REQUEST", id.into()), ("NOROLES_MANDATE", mname.clone())])?;
    r["status"] = V::from("done");
    r["done"] = json!({ "at": iso(now_ms()), "exit": null });
    save(&mut c, &r)?;
    commit(dir, &format!("doing {id}"), false)?;
    let code = spawn(&cmdv, env);
    r["done"]["exit"] = V::from(code);
    save(&mut c, &r)?;
    commit(dir, &format!("did {id} (exit {code})"), false)?;
    Ok(code)
}

pub fn log_line(dir: &Path, file: &str, entry: &V) -> R<()> {
    use std::io::Write;
    let d = dir.join("log");
    fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(d.join(file)).map_err(|e| e.to_string())?;
    writeln!(f, "{}", serde_json::to_string(entry).unwrap()).map_err(|e| e.to_string())
}

// ---------- reality ----------

pub struct Finding { pub key: String, pub mandate: Option<String>, pub what: String }

/// Check every recorded answer against its signature and every request against its hash, and find
/// commits to the record that NoRoles did not make. Opens an incident for each finding.
pub fn audit(dir: &Path, now: i64) -> R<Vec<Finding>> {
    let c = load(dir)?;
    let mut found = vec![];
    for r in c.requests.values() {
        let id = s(r, "id").unwrap_or("");
        let mandate = s(r, "mandate").map(String::from);
        let mut add = |k: String, w: String| found.push(Finding { key: k, mandate: mandate.clone(), what: w });
        if action_hash(r) != s(r, "action_hash").unwrap_or("") { add(format!("tamper-{id}"), format!("{id} was edited after it was asked")); }
        for a in g(r, "approvals").as_array().cloned().unwrap_or_default() {
            if !valid_answer(&c, r, &a, "yes") { let by = s(&a, "by").unwrap_or(""); add(format!("forged-{id}-{by}"), format!("{id}: a yes from {by} is not validly signed")); }
        }
        if status(r) == "approved" && !is_approved(&c, r, now, true) { add(format!("status-{id}"), format!("{id} says approved but its signed answers do not add up")); }
        if status(r) == "done" && !is_approved(&c, r, now, true) { add(format!("undue-{id}"), format!("{id} was carried out without a valid yes")); }
    }
    if dir.join(".git").exists() {
        let log = git(dir, &["log", "--format=@@%H%x09%s", "--name-only"])?;
        let genesis = git(dir, &["rev-list", "--max-parents=0", "HEAD"])?;
        let genesis: Vec<&str> = genesis.lines().collect();
        for block in log.split("@@").filter(|b| !b.is_empty()) {
            let mut lines = block.split('\n');
            let head = lines.next().unwrap_or("");
            let (hash, subject) = head.split_once('\t').unwrap_or((head, ""));
            if genesis.contains(&hash) || subject.starts_with("noroles:") { continue; }
            let touched: Vec<&str> = lines.map(|f| f.trim()).filter(|f| f.starts_with("requests/") || f.starts_with("incidents/") || *f == "ledger.json").collect();
            if !touched.is_empty() { found.push(Finding { key: format!("outside-{}", &hash[..10.min(hash.len())]), mandate: None, what: format!("commit {} \"{subject}\" changed {} outside NoRoles", &hash[..10.min(hash.len())], touched.join(", ")) }); }
        }
    }
    let mut opened = 0;
    for f in &found { if open_incident(&c, &f.key, f.mandate.as_deref(), &f.what, now)? { opened += 1; } }
    if opened > 0 { commit(dir, &format!("audit: {opened} new incident(s)"), false)?; }
    Ok(found)
}

/// Record that an approved action was carried out (used by the gateway and the hook).
pub fn mark_done(dir: &Path, id: &str, result: &str, now: i64) -> R<()> {
    let mut c = load(dir)?;
    let mut r = c.request(id).cloned().ok_or_else(|| format!("no request {id}"))?;
    r["status"] = V::from("done");
    r["done"] = json!({ "at": iso(now), "result": result });
    save(&mut c, &r)?;
    commit(dir, &format!("did {id} through the {}", if result.contains("hook") { "hook" } else { "gateway" }), false)
}
