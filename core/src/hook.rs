//! `noroles hook`: NoRoles in the call path of Claude Code, for every tool the agent already uses
//! (Gmail, Slack, Linear, any MCP connector, local or hosted), with no proxy in between.
//!
//! Claude Code runs this before every tool call (a PreToolUse hook) and passes the call as JSON.
//! - Open work gets no answer from us, so the person's own Claude Code permission settings still apply.
//! - A call mapped in tools.md to a permission becomes a request for a yes; the call is denied with
//!   the request id. After the yes, the same call with the same arguments passes, once.
//! - An MCP call that is not mapped and does not read like reading is refused (fail closed).
//! - The record (requests, incidents, log, ledger, meta files) is written only by NoRoles.
use crate::company::*;
use crate::flags::flags;
use crate::notify::notify;
use crate::requests::{self, Ask};
use crate::util::*;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// A tool-name pattern from tools.md: `*` matches any run of characters, `|` separates alternatives.
pub struct Pattern(Vec<String>);
impl Pattern {
    pub fn new(p: &str) -> Result<Self, String> {
        let alts: Vec<String> = p.split('|').map(|x| x.trim().to_string()).collect();
        if alts.iter().any(|a| a.is_empty()) { return Err("empty pattern".into()); }
        Ok(Pattern(alts))
    }
    pub fn matches(&self, name: &str) -> bool { self.0.iter().any(|a| glob(a.as_bytes(), name.as_bytes())) }
}
fn glob(p: &[u8], t: &[u8]) -> bool {
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' { star = Some(pi); pi += 1; mark = ti; }
        else if pi < p.len() && p[pi] == t[ti] { pi += 1; ti += 1; }
        else if let Some(s) = star { pi = s + 1; mark += 1; ti = mark; }
        else { return false; }
    }
    while pi < p.len() && p[pi] == b'*' { pi += 1; }
    pi == p.len()
}

pub enum Class { Open, Needs(Vec<String>, V), Refused(String) }

const READ_VERBS: [&str; 22] = ["get", "list", "search", "read", "fetch", "query", "find", "describe", "count", "lookup", "show", "view", "retrieve", "download", "inspect", "preview", "check", "browse", "explain", "summarize", "whoami", "status"];

/// The words of a tool name: `slack_send_message` -> [slack, send, message]; `getUserEvents` -> [get, user, events].
fn words(name: &str) -> Vec<String> {
    let mut out = vec![]; let mut cur = String::new(); let mut prev_lower = false;
    for ch in name.chars() {
        if !ch.is_alphanumeric() { if !cur.is_empty() { out.push(std::mem::take(&mut cur)); } prev_lower = false; continue; }
        if ch.is_uppercase() && prev_lower && !cur.is_empty() { out.push(std::mem::take(&mut cur)); }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        cur.extend(ch.to_lowercase());
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

/// Which permissions a call needs. The first matching line of tools.md wins; unmapped MCP tools
/// are open only when their name reads like reading or drafting.
pub fn classify(c: &Company, tool: &str) -> Class {
    for (pat, rule) in &c.tools {
        let Ok(p) = Pattern::new(pat) else { continue };
        if !p.matches(tool) { continue; }
        if rule.as_str() == Some("open") || g(rule, "open").as_bool() == Some(true) { return Class::Open; }
        if rule.as_str() == Some("refuse") || g(rule, "refuse").as_bool() == Some(true) { return Class::Refused(format!("tools.md refuses {pat}")); }
        let perms = listk(rule, "permissions");
        if !perms.is_empty() { return Class::Needs(perms, rule.clone()); }
        return Class::Refused(format!("tools.md line {pat} names no permission"));
    }
    if !tool.starts_with("mcp__") { return Class::Open; }
    let short = tool.rsplit("__").next().unwrap_or(tool);
    let w = words(short);
    // connector prefixes like slack_ or gmail_ come first in some tool names: look at the first two words
    let reads = w.iter().take(2).any(|x| READ_VERBS.contains(&x.as_str()));
    let drafts = w.iter().any(|x| x == "draft" || x == "drafts");
    if reads || drafts { return Class::Open; }
    Class::Refused(format!("{tool} can change things outside and tools.md does not say which permission it needs"))
}

fn find_company(start: &Path) -> Option<PathBuf> {
    if let Ok(d) = std::env::var("NOROLES_DIR") { if !d.is_empty() { return Some(PathBuf::from(d)); } }
    let mut d = start.to_path_buf();
    loop {
        if d.join("permissions.md").exists() { return Some(d); }
        if !d.pop() { return None; }
    }
}

fn deny(reason: &str) -> String {
    json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason } }).to_string()
}

fn session_file(dir: &Path, session: &str) -> PathBuf {
    let safe: String = session.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    dir.join(".noroles").join("sessions").join(safe)
}

/// The record NoRoles alone writes. An agent editing these could not forge a yes (signatures), but
/// it could lose history or block the company, so the hook refuses it.
fn protected(dir: &Path, file: &str) -> bool {
    let p = Path::new(file);
    let abs = if p.is_absolute() { p.to_path_buf() } else { dir.join(p) };
    let (Some(abs), Ok(root)) = (real(&abs), dir.canonicalize()) else { return false };
    let Ok(rel) = abs.strip_prefix(&root) else { return false };
    let rel = rel.to_string_lossy().replace('\\', "/");
    META_FILES.contains(&rel.as_str()) || rel == "ledger.json" || ["requests/", "incidents/", "log/", ".noroles/"].iter().any(|x| rel.starts_with(x))
}

/// The real path of a file that may not exist yet: resolve its deepest existing folder, keep the rest.
fn real(p: &Path) -> Option<PathBuf> {
    let mut rest = vec![];
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(r) = cur.canonicalize() { let mut out = r; for x in rest.iter().rev() { out.push(x); } return Some(out); }
        rest.push(cur.file_name()?.to_os_string());
        if !cur.pop() { return None; }
    }
}

/// Pick the mandate a lasting call belongs to: the one this session works in, or the only active
/// mandate where the agent is executor and that can do everything the call needs.
fn pick_mandate(c: &Company, dir: &Path, session: &str, asker: &str, perms: &[String], now: i64) -> Result<String, String> {
    if let Ok(m) = fs::read_to_string(session_file(dir, session)) {
        let m = m.trim().to_string();
        if c.mandate(&m).is_some() { return Ok(m); }
    }
    let fit: Vec<String> = c.mandates.values()
        .filter(|m| listk(m, "executor").iter().any(|e| e == asker))
        .filter(|m| { let can = mandate_can(m); perms.iter().all(|p| can.perms.contains_key(p)) })
        .filter_map(|m| s(m, "name").map(String::from))
        .filter(|n| mandate_state(c, n, now).0).collect();
    match fit.len() {
        1 => Ok(fit[0].clone()),
        0 => Err(format!("no open mandate lets {asker} {}. Ask your person to open one (`noroles status` lists them).", perms.join(" and "))),
        _ => Err(format!("several mandates could cover this ({}). Say which one first: run `noroles work <mandate>`, then call again.", fit.join(", "))),
    }
}

/// Handle one PreToolUse event. Returns what to print on stdout (nothing means: no decision).
pub fn pre_tool_use(input: &V, asker: &str, now: i64) -> String {
    let tool = s(input, "tool_name").unwrap_or("").to_string();
    let args = g(input, "tool_input").clone();
    let session = s(input, "session_id").unwrap_or("").to_string();
    let cwd = PathBuf::from(s(input, "cwd").unwrap_or("."));
    let Some(dir) = find_company(&cwd) else { return String::new() };

    // `noroles work <mandate>` names the mandate this session works in
    if tool == "Bash" {
        let cmd = s(&args, "command").unwrap_or("");
        let w: Vec<&str> = cmd.split_whitespace().collect();
        if let Some(i) = w.iter().position(|x| *x == "work") {
            if i > 0 && w[i - 1].ends_with("noroles") { if let Some(m) = w.get(i + 1) {
                let f = session_file(&dir, &session);
                let _ = fs::create_dir_all(f.parent().unwrap());
                let _ = fs::write(&f, m);
            } }
        }
        return String::new();
    }
    if ["Write", "Edit", "MultiEdit", "NotebookEdit"].contains(&tool.as_str()) {
        let file = s(&args, "file_path").or(s(&args, "notebook_path")).unwrap_or("");
        if protected(&dir, file) { return deny(&format!("NoRoles: {file} is part of the company record, which only NoRoles and people write. Use `noroles ask`, `noroles stop` or `noroles propose-meta` instead.")); }
        return String::new();
    }

    let c = match load(&dir) {
        Ok(c) => c,
        Err(e) => return if tool.starts_with("mcp__") { deny(&format!("NoRoles cannot read the company files ({e}), so nothing that changes things outside runs. Tell your person.")) } else { String::new() },
    };
    let class = classify(&c, &tool);
    let record = |decision: &str, extra: V| {
        let mut e = json!({ "at": iso(now), "as": asker, "tool": tool, "decision": decision, "session": session });
        if let (Some(o), Some(x)) = (e.as_object_mut(), extra.as_object()) { for (k, v) in x { o.insert(k.clone(), v.clone()); } }
        let _ = requests::log_line(&dir, "calls.jsonl", &e);
    };
    let (perms, rule) = match class {
        Class::Open => { if tool.starts_with("mcp__") { record("open", json!({})); } return String::new(); }
        Class::Refused(why) => {
            record("refused", json!({ "why": why }));
            notify(&format!("NoRoles: refused {asker}"), &format!("tried {tool}: {why}"), true);
            return deny(&format!("NoRoles: refused. {why}. Nothing was done. Ask your person to map it in tools.md if it should be allowed."));
        }
        Class::Needs(p, r) => (p, r),
    };
    let mandate = match pick_mandate(&c, &dir, &session, asker, &perms, now) {
        Ok(m) => m,
        Err(e) => { record("refused", json!({ "why": e })); return deny(&format!("NoRoles: {tool} needs a yes ({}), but {e}", perms.join(", "))); }
    };
    let payload = json!({ "tool": tool, "arguments": args });
    let hash = sha(&canonical(&payload));
    let prior = c.requests.values().find(|r| kind(r) == "action" && s(r, "mandate") == Some(&mandate) && status(r) != "done"
        && truthy(g(g(r, "action"), "payload")) && sha(&canonical(g(g(r, "action"), "payload"))) == hash).cloned();
    if let Some(p) = &prior {
        let id = s(p, "id").unwrap_or("").to_string();
        if is_approved(&c, p, now, false) {
            let _ = requests::mark_done(&dir, &id, "allowed through the hook", now);
            record("approved", json!({ "request": id, "mandate": mandate }));
            return String::new();
        }
        if effective(&c, p, now).0 == "pending" {
            return deny(&format!("NoRoles: still waiting for a yes on {id}. Nothing was done. Call again with exactly the same arguments after your person runs `noroles yes {id}`."));
        }
    }
    // a rule names the argument that holds the recipient or amount; a list means the first one present
    let pick = |k: &str| listk(&rule, k).iter().find_map(|key| text(g(&args, key)).filter(|x| !x.is_empty()));
    let summary = format!("{} {}", short_name(&tool), canonical(&args));
    let ask = Ask {
        mandate: mandate.clone(), permissions: perms.clone(), asker: asker.into(), summary,
        amount: pick("amount").and_then(|x| x.parse().ok()),
        currency: pick("currency").or_else(|| s(&rule, "currency_default").map(String::from)),
        to: pick("to"), payload, ..Default::default()
    };
    match requests::ask(&dir, ask, now) {
        Err(e) => {
            record("refused", json!({ "why": e, "mandate": mandate }));
            notify(&format!("NoRoles: refused {asker}"), &format!("{mandate}: {tool}: {e}"), true);
            deny(&format!("NoRoles: refused: {e}. Nothing was done."))
        }
        Ok(r) => {
            let id = s(&r, "id").unwrap_or("").to_string();
            if status(&r) == "approved" {
                let _ = requests::mark_done(&dir, &id, "allowed through the hook", now);
                record("approved", json!({ "request": id, "mandate": mandate }));
                return String::new();
            }
            record("asked", json!({ "request": id, "mandate": mandate }));
            let f = load(&dir).map(|c2| flags(&c2, &r)).unwrap_or_default();
            let seen = if f.is_empty() { String::new() } else { format!(" Your person will see: {}.", f.join("; ")) };
            deny(&format!("NoRoles: this needs a yes ({}) in mandate {mandate}. Asked as {id}; nothing was done yet.{seen} Tell your person to run `noroles yes {id}`, then call this tool again with exactly the same arguments.", perms.join(", ")))
        }
    }
}

/// `mcp__claude_ai_Gmail__send_message` -> `Gmail.send_message`, for summaries a person reads.
fn short_name(tool: &str) -> String {
    let parts: Vec<&str> = tool.split("__").collect();
    if parts.len() >= 3 && parts[0] == "mcp" {
        let server = parts[1].trim_start_matches("claude.ai_").trim_start_matches("claude_ai_");
        return format!("{server}.{}", parts[2..].join("__"));
    }
    tool.to_string()
}

/// Add the hook to the company's .claude/settings.json, keeping everything already there.
pub fn install(dir: &Path, asker: &str, exe: &str) -> Result<PathBuf, String> {
    let f = dir.join(".claude").join("settings.json");
    let mut v: V = fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    let command = format!("{} hook --as {asker}", shell_quote(exe));
    let entry = json!({ "matcher": ".*", "hooks": [{ "type": "command", "command": command, "timeout": 30 }] });
    let hooks = v.as_object_mut().ok_or("settings.json is not an object")?.entry("hooks").or_insert(json!({}));
    let pre = hooks.as_object_mut().ok_or("hooks is not an object")?.entry("PreToolUse").or_insert(json!([]));
    let arr = pre.as_array_mut().ok_or("hooks.PreToolUse is not a list")?;
    arr.retain(|e| !g(e, "hooks").as_array().map(|h| h.iter().any(|x| s(x, "command").is_some_and(|c| c.contains(" hook --as ")))).unwrap_or(false));
    arr.push(entry);
    fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&f, serde_json::to_string_pretty(&v).unwrap() + "\n").map_err(|e| e.to_string())?;
    Ok(f)
}

fn shell_quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)) { s.into() } else { format!("'{}'", s.replace('\'', "'\\''")) }
}
