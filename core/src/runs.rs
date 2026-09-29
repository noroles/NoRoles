//! Agents started from the panel: a task, a mandate, and Claude Code running headless in the
//! company folder. NoRoles' hook checks every call as usual. When a call needs a yes the agent
//! stops; when the person answers, the agent is resumed on its own.
//! Runs live in .noroles/runs (never committed): transcripts can hold company data.
use crate::company::*;
use crate::util::*;
use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

static AUTH: Mutex<Option<(i64, bool)>> = Mutex::new(None);

fn runs_dir(dir: &Path) -> PathBuf { dir.join(".noroles").join("runs") }
fn meta_path(dir: &Path, id: &str) -> PathBuf { runs_dir(dir).join(format!("{id}.json")) }
fn log_path(dir: &Path, id: &str) -> PathBuf { runs_dir(dir).join(format!("{id}.jsonl")) }

fn uuid() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn read_meta(dir: &Path, id: &str) -> Option<V> { fs::read_to_string(meta_path(dir, id)).ok().and_then(|t| serde_json::from_str(&t).ok()) }
fn write_meta(dir: &Path, m: &V) {
    let id = s(m, "id").unwrap_or("");
    let _ = fs::create_dir_all(runs_dir(dir));
    let _ = fs::write(meta_path(dir, id), serde_json::to_string_pretty(m).unwrap());
}

/// Whether Claude Code is signed in on this computer (cached for half a minute).
pub fn signed_in() -> bool {
    let now = now_ms();
    if let Some((at, v)) = *AUTH.lock().unwrap() { if now - at < 30_000 { return v; } }
    let v = Command::new("claude").args(["auth", "status"]).output().ok()
        .and_then(|o| serde_json::from_slice::<V>(&o.stdout).ok()).map(|j| g(&j, "loggedIn").as_bool() == Some(true)).unwrap_or(false);
    *AUTH.lock().unwrap() = Some((now, v));
    v
}
pub fn forget_auth() { *AUTH.lock().unwrap() = None; }

/// Opens the Anthropic sign-in page in the browser; the person signs in there themselves.
pub fn login(dir: &Path) -> Result<(), String> {
    let f = dir.join(".noroles").join("login.log");
    let out = fs::File::create(&f).map_err(|e| e.to_string())?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    Command::new("claude").args(["auth", "login", "--claudeai"]).stdin(Stdio::null()).stdout(out).stderr(err)
        .spawn().map(|_| ()).map_err(|e| format!("cannot start claude: {e}. Is Claude Code installed?"))?;
    forget_auth();
    Ok(())
}

/// The sign-in link Claude Code printed, in case the browser did not open by itself.
pub fn login_url(dir: &Path) -> Option<String> {
    let t = fs::read_to_string(dir.join(".noroles").join("login.log")).ok()?;
    t.split_whitespace().find(|w| w.starts_with("https://")).map(|w| w.trim_end_matches(['.', ',', ')']).to_string())
}

fn system_prompt(c: &Company, mandate: &str, agent: &str) -> String {
    let m = c.mandate(mandate).cloned().unwrap_or(V::Null);
    format!("You are {agent}, an AI agent in a company that runs on NoRoles. You work in the mandate \"{mandate}\".
Goal: {}
Measured by: {}
Stop if: {}
{}

How this company works:
- NoRoles checks every tool call. Reading, searching and drafts run at once.
- Sending, paying, deleting and sharing need a yes from a person. When a call is refused with \"needs a yes\" and a request id, do not look for another way to do it. Finish your reply with what you asked for and why, in two or three sentences. You will be resumed after the person answers.
- When you are resumed after a yes, call the same tool again with exactly the same arguments.
- Never edit permissions.md, root.md, tools.md, requests/, incidents/, log/ or ledger.json.
- Facts only: never invent a number, a name or a result. If you cannot find something, say so.
- Answer in the language of the task. Keep the final report short: what you did, what waits for a yes, what you could not do.",
        text(g(&m, "intent")).unwrap_or_default(), text(g(&m, "metric")).unwrap_or_default(), text(g(&m, "stop_if")).unwrap_or_default(),
        text(g(&m, "body")).unwrap_or_default())
}

fn spawn_claude(dir: PathBuf, id: String, prompt: String, resume: bool) -> Result<(), String> {
    let c = load(&dir)?;
    let meta = read_meta(&dir, &id).ok_or("no such run")?;
    let mandate = s(&meta, "mandate").unwrap_or("").to_string();
    let agent = s(&meta, "agent").unwrap_or("").to_string();
    let sys = system_prompt(&c, &mandate, &agent);
    let mut cmd = Command::new("claude");
    cmd.arg("-p").arg(&prompt);
    if resume { cmd.args(["--resume", &id]); } else { cmd.args(["--session-id", &id]); }
    cmd.args(["--output-format", "stream-json", "--verbose", "--permission-mode", "bypassPermissions", "--disallowedTools", "Bash", "--max-budget-usd", "3"])
        .arg("--append-system-prompt").arg(sys)
        .current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .env_remove("NOROLES_EXECUTOR").env_remove("NOROLES_DIR");
    let mut child = cmd.spawn().map_err(|e| format!("cannot start claude: {e}"))?;
    let mut m = meta.clone();
    m["status"] = V::from("running");
    m["pid"] = V::from(child.id());
    let mut turns = g(&m, "turns").as_array().cloned().unwrap_or_default();
    turns.push(json!({ "at": iso(now_ms()), "prompt": prompt }));
    m["turns"] = V::from(turns);
    write_meta(&dir, &m);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    std::thread::spawn(move || {
        let mut log = fs::OpenOptions::new().create(true).append(true).open(log_path(&dir, &id)).ok();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(f) = log.as_mut() { let _ = writeln!(f, "{line}"); }
        }
        let mut err = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stderr), &mut err);
        let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
        if let Some(mut m) = read_meta(&dir, &id) {
            m["status"] = V::from(if code == 0 { "finished" } else { "failed" });
            m["exit"] = V::from(code);
            if !err.trim().is_empty() { m["stderr"] = V::from(err.chars().rev().take(2000).collect::<String>().chars().rev().collect::<String>()); }
            m["ended"] = V::from(iso(now_ms()));
            write_meta(&dir, &m);
        }
    });
    Ok(())
}

pub fn start(dir: &Path, mandate: &str, task: &str) -> Result<V, String> {
    if task.trim().is_empty() { return Err("write the task first".into()); }
    let c = load(dir)?;
    let m = c.mandate(mandate).ok_or_else(|| format!("no mandate \"{mandate}\""))?;
    let (active, why) = mandate_state(&c, mandate, now_ms());
    if !active { return Err(format!("mandate {mandate} is not active: {why}")); }
    let agent = listk(m, "executor").into_iter().find(|e| c.is_agent(e)).ok_or_else(|| format!("mandate {mandate} has no agent as executor"))?;
    if !signed_in() { return Err("Claude Code is not signed in on this computer: press Sign in first".into()); }
    let id = uuid();
    let sess = dir.join(".noroles").join("sessions");
    fs::create_dir_all(&sess).map_err(|e| e.to_string())?;
    fs::write(sess.join(&id), mandate).map_err(|e| e.to_string())?;
    let meta = json!({ "id": id, "mandate": mandate, "agent": agent, "task": task, "started": iso(now_ms()), "status": "starting", "turns": [] });
    write_meta(dir, &meta);
    spawn_claude(dir.to_path_buf(), id.clone(), task.to_string(), false)?;
    Ok(json!({ "id": id }))
}

pub fn resume(dir: &Path, id: &str, note: &str) -> Result<(), String> {
    let m = read_meta(dir, id).ok_or("no such run")?;
    if s(&m, "status") == Some("running") && alive(&m) { return Err("the agent is still working".into()); }
    spawn_claude(dir.to_path_buf(), id.to_string(), note.to_string(), true)
}

fn alive(m: &V) -> bool {
    g(m, "pid").as_u64().is_some_and(|p| Command::new("kill").args(["-0", &p.to_string()]).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false))
}

pub fn stop(dir: &Path, id: &str) -> Result<(), String> {
    let mut m = read_meta(dir, id).ok_or("no such run")?;
    if let Some(p) = g(&m, "pid").as_u64() { let _ = Command::new("kill").arg(p.to_string()).status(); }
    m["status"] = V::from("stopped");
    write_meta(dir, &m);
    Ok(())
}

fn request_ids(t: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let b = t.as_bytes();
    let mut i = 0;
    while let Some(p) = t[i..].find("r-2") {
        let st = i + p;
        let cand = &t[st..(st + 17).min(t.len())];
        let ok = cand.len() == 17 && cand[2..10].bytes().all(|x| x.is_ascii_digit()) && b[st + 10] == b'-' && cand[11..].bytes().all(|x| x.is_ascii_hexdigit());
        if ok && !out.contains(&cand.to_string()) { out.push(cand.to_string()); }
        i = st + 3;
    }
    out
}

fn clip(s: &str, n: usize) -> String { if s.chars().count() > n { format!("{}…", s.chars().take(n).collect::<String>()) } else { s.to_string() } }

/// A run as the panel shows it: status, what the agent said and did, and what waits for a yes.
fn view(c: &Company, dir: &Path, m: &V, full: bool) -> V {
    let id = s(m, "id").unwrap_or("");
    let raw = fs::read_to_string(log_path(dir, id)).unwrap_or_default();
    let mut events: Vec<V> = vec![];
    let mut cost = 0.0;
    let mut result: Option<String> = None;
    for line in raw.lines() {
        let Ok(e) = serde_json::from_str::<V>(line) else { continue };
        match s(&e, "type") {
            Some("assistant") => for part in g(g(&e, "message"), "content").as_array().cloned().unwrap_or_default() {
                match s(&part, "type") {
                    Some("text") => { let t = s(&part, "text").unwrap_or("").trim().to_string(); if !t.is_empty() { events.push(json!({ "kind": "say", "text": t })); } }
                    Some("tool_use") => events.push(json!({ "kind": "tool", "name": g(&part, "name"), "input": clip(&g(&part, "input").to_string(), 300) })),
                    _ => {}
                }
            },
            Some("user") => for part in g(g(&e, "message"), "content").as_array().cloned().unwrap_or_default() {
                if s(&part, "type") != Some("tool_result") { continue; }
                let content = g(&part, "content");
                let t = content.as_str().map(String::from).unwrap_or_else(|| content.as_array().map(|a| a.iter().filter_map(|x| s(x, "text")).collect::<Vec<_>>().join("\n")).unwrap_or_default());
                if t.contains("NoRoles") { events.push(json!({ "kind": "noroles", "text": clip(&t, 600) })); }
                else if g(&part, "is_error").as_bool() == Some(true) { events.push(json!({ "kind": "error", "text": clip(&t, 300) })); }
                else { events.push(json!({ "kind": "result", "text": clip(&t, 160) })); }
            },
            Some("result") => { cost += g(&e, "total_cost_usd").as_f64().unwrap_or(0.0); result = s(&e, "result").map(String::from); }
            _ => {}
        }
    }
    let now = now_ms();
    let asked: Vec<V> = request_ids(&raw).into_iter().filter_map(|rid| c.request(&rid).map(|r| json!({ "id": rid, "status": effective(c, r, now).0, "approved": is_approved(c, r, now, true) }))).collect();
    let running = s(m, "status") == Some("running") && alive(m);
    let waiting = !running && asked.iter().any(|a| s(a, "status") == Some("pending"));
    let status = if running { "working" } else if waiting { "waiting for you" } else { match s(m, "status") { Some("failed") => "failed", Some("stopped") => "stopped", _ => "done" } };
    let n = events.len();
    let shown: Vec<V> = if full { events } else { events.into_iter().skip(n.saturating_sub(6)).collect() };
    json!({
        "id": id, "mandate": g(m, "mandate"), "agent": g(m, "agent"), "task": g(m, "task"), "started": g(m, "started"), "ended": g(m, "ended"),
        "status": status, "asked": asked, "events": shown, "event_count": n, "result": result, "cost": (cost * 100.0).round() / 100.0,
        "error": if s(m, "status") == Some("failed") { g(m, "stderr").clone() } else { V::Null },
    })
}

pub fn list(dir: &Path) -> Vec<V> {
    let Ok(c) = load(dir) else { return vec![] };
    let mut metas: Vec<V> = fs::read_dir(runs_dir(dir)).map(|it| it.filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|e| fs::read_to_string(e.path()).ok()).filter_map(|t| serde_json::from_str::<V>(&t).ok()).collect()).unwrap_or_default();
    metas.sort_by(|a, b| s(b, "started").cmp(&s(a, "started")));
    metas.iter().take(30).enumerate().map(|(i, m)| view(&c, dir, m, i < 3)).collect()
}

pub fn get(dir: &Path, id: &str) -> Result<V, String> {
    let c = load(dir)?;
    Ok(view(&c, dir, &read_meta(dir, id).ok_or("no such run")?, true))
}

/// After a person answers a request, resume every finished run that was waiting on it.
pub fn answered(dir: &Path, request: &str, yes: bool, reason: &str) {
    let Ok(rd) = fs::read_dir(runs_dir(dir)) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json") else { continue };
        let raw = fs::read_to_string(log_path(dir, id)).unwrap_or_default();
        if !raw.contains(request) { continue; }
        let Some(m) = read_meta(dir, id) else { continue };
        if s(&m, "status") == Some("running") && alive(&m) { continue; }
        let note = if yes { format!("Ilia approved {request}. Call the same tool again with exactly the same arguments, then finish the task.") }
                   else { format!("Ilia declined {request}{}. Do not do it. Finish the task without it and say what you would do instead.", if reason.is_empty() { String::new() } else { format!(": {reason}") }) };
        let _ = resume(dir, id, &note);
    }
}
