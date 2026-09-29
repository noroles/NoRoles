//! `noroles serve`: the company on one screen, on this computer only.
//! People answer requests, open and stop mandates, and see who can do what and what agents did.
//! A yes is still signed with the person's key: the page asks for the passphrase every time.
//!
//! Safety: listens on 127.0.0.1 only; every call needs the token printed when the server starts;
//! the Host header must be the local address (no DNS rebinding); the page is served by NoRoles
//! itself and shows the exact action from the signed record, never text an agent formatted.
use crate::company::*;
use crate::flags::flags;
use crate::requests::{self, Proof};
use crate::util::*;
use serde_json::json;
use std::io::Read;
use std::path::{Path, PathBuf};

const PAGE: &str = include_str!("panel.html");

fn read_calls(dir: &Path, n: usize) -> Vec<V> {
    let t = std::fs::read_to_string(dir.join("log").join("calls.jsonl")).unwrap_or_default();
    let mut v: Vec<V> = t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let start = v.len().saturating_sub(n);
    v.drain(..start);
    v.reverse();
    v
}

fn request_view(c: &Company, r: &V, now: i64) -> V {
    let (st, escalated) = effective(c, r, now);
    let prog: Vec<V> = progress(c, r, now).into_iter().map(|p| json!({ "permission": p.permission, "got": p.got, "need": p.need, "can": p.can, "wait_until": p.wait_until.map(iso) })).collect();
    json!({
        "id": g(r, "id"), "kind": g(r, "kind"), "mandate": g(r, "mandate"), "permissions": g(r, "permissions"),
        "asker": g(r, "asker"), "created": g(r, "created"), "status": st, "escalated": escalated,
        "action": g(r, "action"), "flags": flags(c, r), "progress": prog,
        "approvals": g(r, "approvals").as_array().map(|a| a.iter().map(|x| json!({ "by": g(x, "by"), "at": g(x, "at"), "valid": valid_answer(c, r, x, "yes") })).collect::<Vec<_>>()),
        "denials": g(r, "denials").as_array().map(|a| a.iter().map(|x| json!({ "by": g(x, "by"), "at": g(x, "at"), "reason": g(x, "reason") })).collect::<Vec<_>>()),
        "done": g(r, "done"), "covered_by": g(r, "covered_by"),
    })
}

pub fn state(dir: &Path, me: Option<&str>) -> Result<V, String> {
    let now = now_ms();
    requests::settle(dir, now)?;
    let c = load(dir)?;
    let mandates: Vec<V> = c.mandates.values().map(|m| {
        let name = s(m, "name").unwrap_or("");
        let (active, why) = mandate_state(&c, name, now);
        let can = mandate_can(m);
        json!({
            "name": name, "intent": g(m, "intent"), "metric": g(m, "metric"), "stop_if": g(m, "stop_if"),
            "holder": g(m, "holder"), "executor": listk(m, "executor"), "owns": listk(m, "owns"),
            "ends": mandate_end(m).map(iso), "kind": if truthy(g(m, "expires")) { "one-off" } else { "standing" },
            "active": active, "why": why, "can": can.perms, "tools": can.tools, "body": g(m, "body"),
            "opened_pending": c.requests.values().any(|r| kind(r) == "open" && s(r, "mandate") == Some(name) && effective(&c, r, now).0 == "pending"),
        })
    }).collect();
    let mut reqs: Vec<&V> = c.requests.values().collect();
    reqs.sort_by(|a, b| s(b, "created").cmp(&s(a, "created")));
    let pending: Vec<V> = reqs.iter().filter(|r| effective(&c, r, now).0 == "pending").map(|r| request_view(&c, r, now)).collect();
    let recent: Vec<V> = reqs.iter().filter(|r| effective(&c, r, now).0 != "pending").take(40).map(|r| request_view(&c, r, now)).collect();
    let permissions: Vec<V> = c.permissions.iter().map(|(p, def)| json!({
        "name": p, "holders": humans(&c, g(def, "holders")), "quorum": g(def, "quorum").as_i64().unwrap_or(1),
        "each": s(def, "each").unwrap_or("mandate"), "limits": g(def, "limits"), "respond_within": g(def, "respond_within"),
        "human_only": g(def, "human_only"),
    })).collect();
    let people: Vec<V> = c.people.iter().map(|(id, p)| json!({ "id": id, "name": g(p, "name"), "email": g(p, "email"), "signed": s(p, "key").is_some(), "root": c.root.contains(id) })).collect();
    let agents: Vec<V> = c.agents.iter().map(|(id, a)| json!({ "id": id, "works_for": g(a, "works_for") })).collect();
    let incidents: Vec<V> = requests::incidents(dir).into_iter().filter(|i| !truthy(g(i, "resolved"))).collect();
    let problems: Vec<V> = check(&c, now).into_iter().map(|p| json!({ "error": p.error, "where": p.r#where, "msg": p.msg })).collect();
    Ok(json!({
        "company": dir.file_name().map(|x| x.to_string_lossy().into_owned()), "dir": dir.display().to_string(),
        "me": me, "now": iso(now),
        "mandates": mandates, "pending": pending, "recent": recent,
        "permissions": permissions, "people": people, "agents": agents,
        "tools": c.tools.iter().map(|(k, v)| json!({ "pattern": k, "rule": v })).collect::<Vec<_>>(),
        "incidents": incidents, "problems": problems, "calls": read_calls(dir, 60),
    }))
}

fn act(dir: &Path, me: &str, path: &str, body: &V) -> Result<V, String> {
    let now = now_ms();
    let st = |k: &str| s(body, k).unwrap_or("").to_string();
    match path {
        "/api/answer" => {
            let yes = g(body, "yes").as_bool().ok_or("say yes or no")?;
            let pass = st("passphrase");
            let c = load(dir)?;
            let proof = if s(c.people.get(me).unwrap_or(&V::Null), "key").is_some() { Proof::Passphrase(&pass) } else { Proof::None };
            let reason = st("reason");
            let r = requests::decide(dir, &st("id"), me, yes, Some(reason.as_str()).filter(|x| !x.is_empty()), proof, now)?;
            Ok(json!({ "id": g(&r, "id"), "status": g(&r, "status") }))
        }
        "/api/open" => { let r = requests::open_mandate(dir, &st("mandate"), me, now)?; Ok(json!({ "id": g(&r, "id"), "status": g(&r, "status") })) }
        "/api/stop" => { requests::stop(dir, &st("mandate"), me, &st("reason"), now)?; Ok(json!({ "ok": true })) }
        "/api/resume" => { let r = requests::resume(dir, &st("mandate"), me, now)?; Ok(json!({ "id": g(&r, "id") })) }
        "/api/resolve" => {
            // no signature is stored for this, so prove it is the person: their key must open
            let c = load(dir)?;
            if let Some(k) = c.people.get(me).and_then(|p| s(p, "key")) {
                if crate::keys::public_of(me, &st("passphrase")).ok().as_deref() != Some(k) { return Err("wrong passphrase: nothing was recorded".into()); }
            }
            requests::resolve_incident(dir, &st("id"), me, &st("note"), now)?; Ok(json!({ "ok": true }))
        }
        _ => Err("no such action".into()),
    }
}

/// The panel's access token, kept next to the person's keys so a bookmark keeps working.
fn panel_token() -> Result<String, String> {
    let f = crate::keys::key_dir().join("panel-token");
    if let Ok(t) = std::fs::read_to_string(&f) { let t = t.trim().to_string(); if t.len() == 32 { return Ok(t); } }
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    let t = hex::encode(b);
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&f, &t).map_err(|e| e.to_string())?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)); }
    Ok(t)
}

pub fn serve(dir: PathBuf, me: String, port: u16, open_browser: bool) -> Result<(), String> {
    let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;
    let port = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(port);
    let token = panel_token()?;
    let url = format!("http://127.0.0.1:{port}/#{token}");
    println!("NoRoles panel for {} ({me}): {url}\nIt runs on this computer only. Close this window or press Ctrl-C to stop it.", dir.display());
    if open_browser && cfg!(target_os = "macos") { let _ = std::process::Command::new("open").arg(&url).status(); }
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    for mut req in server.incoming_requests() {
        let host_ok = req.headers().iter().any(|h| h.field.equiv("Host") && hosts.contains(&h.value.as_str().to_string()));
        let token_ok = req.headers().iter().any(|h| h.field.equiv("X-NoRoles-Token") && h.value.as_str() == token);
        let path = req.url().split('?').next().unwrap_or("/").to_string();
        let reply = |code: u16, ctype: &str, body: String| {
            tiny_http::Response::from_string(body).with_status_code(code)
                .with_header(tiny_http::Header::from_bytes("Content-Type", ctype).unwrap())
                .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap())
                .with_header(tiny_http::Header::from_bytes("X-Frame-Options", "DENY").unwrap())
                .with_header(tiny_http::Header::from_bytes("Content-Security-Policy", "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src data:; connect-src 'self'; frame-ancestors 'none'").unwrap())
        };
        if !host_ok { let _ = req.respond(reply(403, "text/plain", "wrong host".into())); continue; }
        if path == "/" && req.method() == &tiny_http::Method::Get { let _ = req.respond(reply(200, "text/html; charset=utf-8", PAGE.into())); continue; }
        if !token_ok { let _ = req.respond(reply(401, "application/json", json!({ "error": "open the panel from the link noroles serve printed" }).to_string())); continue; }
        let res = if path == "/api/state" && req.method() == &tiny_http::Method::Get {
            state(&dir, Some(&me))
        } else if req.method() == &tiny_http::Method::Post && path.starts_with("/api/") {
            let mut body = String::new();
            let _ = req.as_reader().take(1 << 20).read_to_string(&mut body);
            let v: V = serde_json::from_str(&body).unwrap_or(json!({}));
            act(&dir, &me, &path, &v)
        } else { Err("not found".into()) };
        let (code, body) = match res { Ok(v) => (200, v.to_string()), Err(e) => (400, json!({ "error": e }).to_string()) };
        let _ = req.respond(reply(code, "application/json", body));
    }
    Ok(())
}
