//! NoRoles command line. https://noroles.com
use noroles::company::*;
use noroles::requests::{self, Ask, Proof};
use noroles::{flags, hook, keys, serve};
use std::collections::BTreeMap;
use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::exit;
use noroles::util::*;

const HELP: &str = "NoRoles: permissions instead of roles, for people and AI agents.

  noroles                      print the manifesto
  noroles init [dir]           start a company in dir (default: here)
  noroles check                check the files against the laws
  noroles status               mandates, requests waiting for a yes, problems

  noroles open <mandate>       ask the holders to open a mandate
  noroles work <mandate>       say which mandate this agent session works in
  noroles ask --mandate <m> --permission <p> --summary \"<exact action>\"
              [--amount 42 --currency USD --to \"<recipient>\"] [--tool <key> -- <command...>]
              [--item \"<one of several>\" ...]   a batch: every item shown, one yes
  noroles yes <id>             approve (people only, in a terminal, signed with your key)
  noroles no <id> [reason]     decline (people only, in a terminal)
  noroles keygen               create your signing key (once per person)
  noroles stop <mandate> --reason \"<why>\"   break glass: anyone can stop a mandate
  noroles resume <mandate>     ask the holder to lift a stop
  noroles serve                the panel: answer requests, open and stop mandates, see everything
  noroles serve install        start the panel with this computer and keep it on (macOS)
  noroles hook install --as <agent>   check every Claude Code tool call in this company
  noroles audit                check every signature and find changes made outside NoRoles
  noroles resolve <incident> \"<what happened>\"
  noroles do <id>              carry out an approved action, once
  noroles run <mandate> --as <executor> -- <command...>
                               run with only the keys the mandate allows
  noroles propose-meta         ask for a rule.change yes after editing root, permissions, credentials, servers or tools

Docs: https://noroles.com";

const MANIFESTO: &str = include_str!("manifesto.txt");

fn tty() -> bool { std::io::stdout().is_terminal() }
fn bold(s: &str) -> String { if tty() { format!("\x1b[1m{s}\x1b[0m") } else { s.into() } }
fn dim(s: &str) -> String { if tty() { format!("\x1b[2m{s}\x1b[0m") } else { s.into() } }
fn warn(s: &str) -> String { if tty() { format!("\x1b[1;31m{s}\x1b[0m") } else { s.into() } }
fn fail(msg: &str) -> ! { eprintln!("noroles: {msg}"); exit(1) }

struct Args { pos: Vec<String>, flags: BTreeMap<String, Vec<String>>, command: Option<Vec<String>> }
impl Args {
    fn parse(a: &[String]) -> Args {
        let dash = a.iter().position(|x| x == "--");
        let head = &a[..dash.unwrap_or(a.len())];
        let command = dash.map(|i| a[i + 1..].to_vec());
        let (mut pos, mut flags) = (vec![], BTreeMap::<String, Vec<String>>::new());
        let mut i = 0;
        while i < head.len() {
            if let Some(k) = head[i].strip_prefix("--") {
                let v = if i + 1 < head.len() && !head[i + 1].starts_with("--") { i += 1; head[i].clone() } else { "true".into() };
                flags.entry(k.into()).or_default().push(v);
            } else { pos.push(head[i].clone()); }
            i += 1;
        }
        Args { pos, flags, command }
    }
    fn one(&self, k: &str) -> Option<String> { self.flags.get(k).and_then(|v| v.first().cloned()) }
    fn all(&self, k: &str) -> Option<Vec<String>> { self.flags.get(k).cloned() }
    fn p(&self, i: usize) -> Option<String> { self.pos.get(i).cloned() }
}

fn find_dir() -> PathBuf {
    if let Ok(d) = std::env::var("NOROLES_DIR") { if !d.is_empty() { return PathBuf::from(d); } }
    let mut d = std::env::current_dir().unwrap_or_default();
    loop {
        if d.join("permissions.md").exists() { return d; }
        if !d.pop() { fail("no permissions.md here or above. Run `noroles init` first."); }
    }
}
fn company(dir: &Path) -> Company { load(dir).unwrap_or_else(|e| fail(&e)) }
fn ok<T>(r: Result<T, String>) -> T { r.unwrap_or_else(|e| fail(&e)) }

fn need_terminal() {
    if std::env::var("NOROLES_EXECUTOR").is_ok() { fail("executors cannot do this: only people"); }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() { fail("run this in a terminal: only people can"); }
}

/// Who is at the keyboard: the person whose email matches git config.
fn me(c: &Company) -> Option<String> {
    let email = std::process::Command::new("git").args(["config", "user.email"]).current_dir(&c.dir).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_lowercase()).unwrap_or_default();
    c.people.iter().find(|(_, p)| s(p, "email").map(|e| e.to_lowercase()) == Some(email.clone()) && !email.is_empty()).map(|(id, _)| id.clone())
}

fn asker(c: &Company, flag: Option<String>) -> String {
    if let Some(f) = flag { return f; }
    if let Ok(e) = std::env::var("NOROLES_EXECUTOR") { return e; }
    // Outside a terminal it is most likely an agent: never assume it is the person whose git email is set.
    if !std::io::stdin().is_terminal() { fail("say who is asking: pass --as <your agent or person id> (see permissions.md)"); }
    me(c).unwrap_or_else(|| fail("cannot tell who is asking: pass --as <person or agent>"))
}

fn show(r: &V, c: &Company) -> String {
    let a = g(r, "action");
    let perms = permissions_of(r);
    let mut l = vec![format!("{}  {}  {}{}", bold(s(r, "id").unwrap_or("")), kind(r), if perms.is_empty() { "no permission".into() } else { perms.join(", ") }, s(r, "mandate").map(|m| format!("  mandate {m}")).unwrap_or_default())];
    l.push(format!("  asked by {} at {}", s(r, "asker").unwrap_or(""), text(g(r, "created")).unwrap_or_default()));
    if let Some(x) = s(a, "summary").filter(|x| !x.is_empty()) { l.push(format!("  action:  {x}")); }
    if let Some(x) = f64_of(g(a, "amount")) { l.push(format!("  amount:  {}{}", fmt_num(x), s(a, "currency").map(|c| format!(" {c}")).unwrap_or_default())); }
    if let Some(x) = text(g(a, "to")).filter(|x| !x.is_empty()) { l.push(format!("  to:      {x}")); }
    if let Some(x) = s(a, "tool") { l.push(format!("  key:     {x}")); }
    let cmd = listk(a, "command"); if !cmd.is_empty() { l.push(format!("  runs:    {}", cmd.join(" "))); }
    let p = g(a, "payload"); if truthy(p) { l.push(format!("  payload: {}", p.as_str().map(String::from).unwrap_or_else(|| p.to_string()))); }
    if let Some(h) = g(a, "hashes").as_object() { l.push(format!("  files:   {}", h.keys().cloned().collect::<Vec<_>>().join(", "))); }
    for (i, it) in listk(a, "items").iter().enumerate() { l.push(format!("  item {}:  {it}", i + 1)); }
    if let Some(cv) = s(r, "covered_by") { l.push(format!("  covered by the yes that opened the mandate ({cv})")); }
    for f in flags::flags(c, r) { l.push(warn(&format!("  ! {f}"))); }
    l.join("\n")
}

fn read_line(q: &str) -> String {
    print!("{q}"); let _ = std::io::stdout().flush();
    let mut s = String::new(); let _ = std::io::stdin().lock().read_line(&mut s); s
}
fn secret(q: &str) -> String { rpassword::prompt_password(q).unwrap_or_else(|_| fail("could not read the passphrase")) }

fn answer(yes: bool, id: &str, reason: Option<String>) {
    let dir = find_dir();
    let c = company(&dir);
    need_terminal();
    let who = me(&c).unwrap_or_else(|| fail("your git email is not in permissions.md people"));
    let r = c.request(id).cloned().unwrap_or_else(|| fail(&format!("no request {id}")));
    println!("\n{}\n", show(&r, &c));
    let word = if yes { "yes" } else { "no" };
    let typed = read_line(&format!("{who}, type \"{word}\" to {}: ", if yes { "approve exactly this" } else { "decline" })).trim().to_lowercase();
    if typed != word { fail("nothing recorded"); }
    let mut pass = None;
    if s(&c.people[&who], "key").is_some() {
        if !keys::key_path(&who).exists() { fail(&format!("your key is not on this computer ({})", keys::key_path(&who).display())); }
        pass = Some(secret("passphrase for your signing key: "));
    }
    let proof = pass.as_deref().map(Proof::Passphrase).unwrap_or(Proof::None);
    let out = ok(requests::decide(&dir, id, &who, yes, reason.as_deref().filter(|x| !x.is_empty()), proof, now_ms()));
    println!("{}: {}", s(&out, "id").unwrap_or(""), status(&out));
}

fn status_cmd() {
    let dir = find_dir();
    let now = now_ms();
    ok(requests::settle(&dir, now));
    let c = company(&dir);
    println!("{}", bold("Mandates"));
    for m in c.mandates.values() {
        let name = s(m, "name").unwrap_or("");
        let (active, why) = mandate_state(&c, name, now);
        let end = text(if truthy(g(m, "expires")) { g(m, "expires") } else { g(m, "review") }).unwrap_or_default();
        println!("  {} {}  {}", if active { "●" } else { "○" }, bold(name), dim(&format!("holder {}, executor {}, ends {}", s(m, "holder").unwrap_or(""), listk(m, "executor").join(", "), &end[..end.len().min(10)])));
        println!("    {}{}", text(g(m, "intent")).unwrap_or_default(), if active { String::new() } else { dim(&format!("  ({why})")) });
    }
    let pending: Vec<&V> = c.requests.values().filter(|r| effective(&c, r, now).0 == "pending").collect();
    println!("\n{}", bold(&format!("Waiting for a yes ({})", pending.len())));
    for r in pending {
        println!("{}", show(r, &c));
        for p in progress(&c, r, now) {
            println!("{}", dim(&format!("  {}: {}/{} from {}{}", p.permission, p.got.len(), p.need, if p.can.is_empty() { "nobody".into() } else { p.can.join(", ") }, p.wait_until.map(|w| format!(", then wait until {}", iso(w))).unwrap_or_default())));
        }
    }
    let approved: Vec<&V> = c.requests.values().filter(|r| status(r) == "approved" && kind(r) == "action").collect();
    if !approved.is_empty() { println!("\n{}", bold("Approved, not yet done")); for r in approved { println!("  {}  {}", s(r, "id").unwrap_or(""), s(g(r, "action"), "summary").unwrap_or("")); } }
    let open: Vec<V> = requests::incidents(&dir).into_iter().filter(|i| !truthy(g(i, "resolved"))).collect();
    if !open.is_empty() { println!("\n{}", bold(&format!("Incidents ({})", open.len()))); for i in open { println!("  {}  {}  {}", s(&i, "id").unwrap_or(""), s(&i, "what").unwrap_or(""), dim(&format!("for {}", s(&i, "holder").unwrap_or("")))); } }
    let problems = check(&c, now);
    println!("\n{}", bold(&if problems.is_empty() { "No problems".to_string() } else { format!("Problems ({})", problems.len()) }));
    for p in problems { println!("  {} {}: {}", if p.error { "✗" } else { "!" }, p.r#where, p.msg); }
}

// ---------- init: the company template ----------

const TEMPLATE: [(&str, &str); 11] = [
    ("root.md", include_str!("../../npm/templates/company/root.md")),
    ("permissions.md", include_str!("../../npm/templates/company/permissions.md")),
    ("credentials.md", include_str!("../../npm/templates/company/credentials.md")),
    ("servers.md", include_str!("../../npm/templates/company/servers.md")),
    ("tools.md", include_str!("../templates/tools.md")),
    ("mandates/first-website.md", include_str!("../../npm/templates/company/mandates/first-website.md")),
    ("rules/content.md", include_str!("../../npm/templates/company/rules/content.md")),
    ("AGENTS.md", include_str!("../../npm/templates/company/AGENTS.md")),
    ("CLAUDE.md", include_str!("../../npm/templates/company/CLAUDE.md")),
    (".claude/skills/noroles/SKILL.md", include_str!("../../npm/templates/company/.claude/skills/noroles/SKILL.md")),
    (".gitignore", include_str!("../../npm/templates/company/gitignore.txt")),
];

fn git_config(k: &str) -> String {
    std::process::Command::new("git").args(["config", k]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}
fn slug(x: &str) -> String {
    let mut out = String::new();
    for ch in x.to_lowercase().chars() { if ch.is_ascii_alphanumeric() { out.push(ch) } else if !out.ends_with('-') { out.push('-') } }
    let t = out.trim_matches('-').to_string();
    if t.is_empty() { "me".into() } else { t }
}

fn init(dir: &Path, name: Option<String>, email: Option<String>) -> (String, String) {
    if dir.join("permissions.md").exists() { fail(&format!("{} already has permissions.md", dir.display())); }
    let name = name.filter(|x| !x.is_empty()).unwrap_or_else(|| { let n = git_config("user.name"); if n.is_empty() { "Me".into() } else { n } });
    let email = email.unwrap_or_else(|| git_config("user.email"));
    let id = slug(name.split(' ').next().unwrap_or("me"));
    let expires = iso(now_ms() + 30 * DAY)[..10].to_string();
    for (f, t) in TEMPLATE {
        let p = dir.join(f);
        if p.exists() { continue; }
        std::fs::create_dir_all(p.parent().unwrap()).unwrap_or_else(|e| fail(&e.to_string()));
        let body = t.replace("{{ID}}", &id).replace("{{NAME}}", &name).replace("{{EMAIL}}", &email).replace("{{EXPIRES}}", &expires);
        std::fs::write(&p, body).unwrap_or_else(|e| fail(&e.to_string()));
    }
    let sec = dir.join(".noroles").join("secrets.env");
    std::fs::create_dir_all(sec.parent().unwrap()).ok();
    if !sec.exists() {
        std::fs::write(&sec, "# KEY=value, one per line. This file never leaves this computer.\n").ok();
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&sec, std::fs::Permissions::from_mode(0o600)); }
    }
    write_ledger(dir, &serde_json::json!({}));
    if !dir.join(".git").exists() { let _ = std::process::Command::new("git").args(["init", "-q"]).current_dir(dir).status(); }
    ok(requests::commit(dir, "start the company (genesis: root writes root.md, permissions.md and rules directly)", true));
    (id, name)
}

/// Genesis only: record the current meta files as the starting point.
fn write_ledger(dir: &Path, base: &V) {
    let mut meta = Obj::new();
    for f in META_FILES { if let Ok(t) = read_text(dir, f) { meta.insert(f.into(), V::from(sha(&t))); } }
    let mut l = if base.is_object() { base.clone() } else { serde_json::json!({}) };
    l["meta"] = V::Object(meta);
    std::fs::write(dir.join("ledger.json"), serde_json::to_string_pretty(&l).unwrap() + "\n").unwrap_or_else(|e| fail(&e.to_string()));
}

fn file_has_key(dir: &Path, who: &str, public: &str) -> bool {
    read_text(dir, "permissions.md").map(|t| t.lines().any(|l| l.trim_start().starts_with(&format!("{who}:")) && l.contains(public))).unwrap_or(false)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cmd = argv.first().cloned();
    let o = Args::parse(if argv.is_empty() { &[] } else { &argv[1..] });
    let now = now_ms();
    match cmd.as_deref() {
        None | Some("manifesto") => {
            println!();
            for l in MANIFESTO.lines() { let b = l == "NO ROLES" || l.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())); println!("{}", if b { bold(l) } else { l.into() }); }
            println!("{}", dim("\nRun `noroles help` for the commands."));
        }
        Some("help" | "--help" | "-h") => println!("{HELP}"),
        Some("--version" | "-V" | "version") => println!("{}", env!("CARGO_PKG_VERSION")),
        Some("init") => {
            let dir = std::path::absolute(o.p(0).unwrap_or(".".into())).unwrap_or_default();
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| fail(&e.to_string()));
            let email = o.one("email");
            let (id, name) = init(&dir, o.one("name"), email);
            println!("Started a NoRoles company in {} for {name} ({id}).\nNext: read permissions.md, then `noroles open first-website`, then tell your agent to read AGENTS.md.", dir.display());
        }
        Some("check") => {
            let c = company(&find_dir());
            let p = check(&c, now);
            for x in &p { println!("{} {}: {}", if x.error { "✗" } else { "!" }, x.r#where, x.msg); }
            if p.is_empty() { println!("No problems."); }
            exit(if p.iter().any(|x| x.error) { 1 } else { 0 });
        }
        Some("status") => status_cmd(),
        Some("open") => {
            let dir = find_dir(); let c = company(&dir);
            let m = o.p(0).unwrap_or_else(|| fail("which mandate?"));
            let r = ok(requests::open_mandate(&dir, &m, &asker(&c, o.one("as")), now));
            let id = s(&r, "id").unwrap_or("").to_string();
            println!("{}\n{}", show(&r, &c), if status(&r) == "approved" { "Open: it needs no permission.".to_string() } else { format!("Waiting for its holders: they run `noroles yes {id}`.") });
        }
        Some("work") => {
            let dir = find_dir(); let c = company(&dir);
            let m = o.p(0).unwrap_or_else(|| fail("which mandate?"));
            let md = c.mandate(&m).unwrap_or_else(|| fail(&format!("no mandate \"{m}\"")));
            let (active, why) = mandate_state(&c, &m, now);
            println!("Working in {}: {}\n  can: {}\n  stop if: {}{}", bold(&m), text(g(md, "intent")).unwrap_or_default(), { let k: Vec<String> = mandate_can(md).perms.keys().cloned().collect(); if k.is_empty() { "nothing lasting".into() } else { k.join(", ") } }, text(g(md, "stop_if")).unwrap_or_default(), if active { String::new() } else { format!("\n  not active: {why}. Lasting calls will be refused until it is opened.") });
        }
        Some("ask") => {
            let dir = find_dir(); let c = company(&dir);
            let a = Ask {
                mandate: o.one("mandate").unwrap_or_else(|| fail("--mandate is required")),
                permissions: o.all("permission").unwrap_or_else(|| fail("--permission is required")),
                asker: asker(&c, o.one("as")),
                summary: o.one("summary").unwrap_or_else(|| fail("--summary is required: the exact action")),
                amount: o.one("amount").map(|x| x.parse().unwrap_or_else(|_| fail("--amount must be a number"))),
                currency: o.one("currency"), to: o.one("to"),
                payload: o.one("payload").map(V::from).unwrap_or(V::Null),
                items: o.all("item"), tool: o.one("tool"), command: o.command.clone(),
            };
            let r = ok(requests::ask(&dir, a, now));
            let id = s(&r, "id").unwrap_or("").to_string();
            let has_cmd = !listk(g(&r, "action"), "command").is_empty();
            println!("{}\n{}", show(&r, &c), if status(&r) == "approved" { format!("Approved: inside the mandate's limits, covered by its yes. Go ahead{}.", if has_cmd { format!(" with `noroles do {id}`") } else { String::new() }) } else { format!("Asked. A holder answers with `noroles yes {id}`. Do nothing lasting until then.") });
        }
        Some("yes") => answer(true, &o.p(0).unwrap_or_else(|| fail("which request?")), None),
        Some("no") => answer(false, &o.p(0).unwrap_or_else(|| fail("which request?")), Some(o.pos[1.min(o.pos.len())..].join(" "))),
        Some("do") => exit(ok(requests::do_action(&find_dir(), &o.p(0).unwrap_or_else(|| fail("which request?")), now))),
        Some("run") => {
            let m = o.p(0).unwrap_or_else(|| fail("which mandate?"));
            let who = o.one("as").unwrap_or_else(|| fail("--as <executor> is required"));
            let argv = o.command.clone().filter(|x| !x.is_empty()).unwrap_or_else(|| fail("put the command after --"));
            exit(ok(requests::run(&find_dir(), &m, &who, &argv, now)));
        }
        Some("keygen") => {
            need_terminal();
            let dir = find_dir(); let c = company(&dir);
            let who = me(&c).unwrap_or_else(|| fail("your git email is not in permissions.md people"));
            let public = if keys::key_path(&who).exists() {
                println!("You already have a signing key ({}). Using it here.", keys::key_path(&who).display());
                ok(keys::public_of(&who, &secret("its passphrase: ")))
            } else {
                let p1 = secret("new passphrase (8+ characters): ");
                let p2 = secret("again: ");
                if p1 != p2 { fail("passphrases differ"); }
                ok(keys::keygen(&who, &p1))
            };
            if file_has_key(&dir, &who, &public) { println!("permissions.md already has this key."); return; }
            let genesis = c.requests.is_empty();
            let file = ok(read_text(&dir, "permissions.md"));
            let line = file.lines().find(|l| l.trim_start().starts_with(&format!("{who}:")) && l.contains('{') && l.trim_end().ends_with('}')).map(String::from);
            match line {
                Some(l) if genesis && !l.contains("key:") => {
                    let body = l.trim_end().trim_end_matches('}').trim_end();
                    let new = format!("{body}, key: \"{public}\" }}");
                    std::fs::write(dir.join("permissions.md"), file.replacen(&l, &new, 1)).unwrap_or_else(|e| fail(&e.to_string()));
                    write_ledger(&dir, &c.ledger);
                    ok(requests::commit(&dir, &format!("genesis: signing key for {who}"), true));
                    println!("Key saved to {} and added to permissions.md.", keys::key_path(&who).display());
                }
                _ => println!("Key saved to {}. Add this to your entry in permissions.md, then `noroles propose-meta`:\n  key: \"{public}\"", keys::key_path(&who).display()),
            }
        }
        Some("stop") => {
            let dir = find_dir(); let c = company(&dir);
            let m = o.p(0).unwrap_or_else(|| fail("which mandate?"));
            let reason = o.one("reason").unwrap_or_else(|| o.pos[1.min(o.pos.len())..].join(" "));
            ok(requests::stop(&dir, &m, &asker(&c, o.one("as")), &reason, now));
            println!("Stopped {m}. Its holder reviews within 24h; `noroles resume {m}` asks them to lift it.");
        }
        Some("resume") => {
            let dir = find_dir(); let c = company(&dir);
            let r = ok(requests::resume(&dir, &o.p(0).unwrap_or_else(|| fail("which mandate?")), &asker(&c, o.one("as")), now));
            println!("{}\nThe holder answers with `noroles yes {}`.", show(&r, &c), s(&r, "id").unwrap_or(""));
        }
        Some("audit") => {
            let f = ok(requests::audit(&find_dir(), now));
            for x in &f { println!("✗ {}", x.what); }
            if f.is_empty() { println!("Every answer is validly signed and every change went through NoRoles."); }
            exit(if f.is_empty() { 0 } else { 1 });
        }
        Some("resolve") => {
            need_terminal();
            let dir = find_dir(); let c = company(&dir);
            let who = me(&c).unwrap_or_else(|| fail("your git email is not in permissions.md people"));
            ok(requests::resolve_incident(&dir, &o.p(0).unwrap_or_else(|| fail("which incident?")), &who, &o.pos[1.min(o.pos.len())..].join(" "), now));
            println!("Resolved.");
        }
        Some("propose-meta") => {
            let dir = find_dir(); let c = company(&dir);
            let r = ok(requests::propose_meta(&dir, &asker(&c, o.one("as")), now));
            println!("{}", show(&r, &c));
        }
        Some("hook") => {
            if o.p(0).as_deref() == Some("install") {
                let dir = find_dir(); let c = company(&dir);
                let who = o.one("as").unwrap_or_else(|| fail("--as <agent> is required: the agent the hook speaks for"));
                if !c.is_agent(&who) && !c.is_person(&who) { fail(&format!("\"{who}\" is not in permissions.md")); }
                let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "noroles".into());
                let f = ok(hook::install(&dir, &who, &exe));
                println!("Every Claude Code tool call in {} now goes through NoRoles ({}).\nStart a new Claude Code session there for it to take effect.", dir.display(), f.display());
                return;
            }
            let who = o.one("as").unwrap_or_else(|| fail("--as <agent> is required"));
            let mut input = String::new();
            let _ = std::io::stdin().read_to_string(&mut input);
            let v: V = serde_json::from_str(&input).unwrap_or(V::Null);
            if v.is_null() { return; }
            if s(&v, "hook_event_name").unwrap_or("PreToolUse") != "PreToolUse" { return; }
            let out = hook::pre_tool_use(&v, &who, now);
            if !out.is_empty() { println!("{out}"); }
        }
        Some("serve" | "panel") => {
            if std::env::var("NOROLES_EXECUTOR").is_ok() { fail("executors cannot do this: only people"); }
            let dir = find_dir(); let c = company(&dir);
            let who = me(&c).unwrap_or_else(|| fail("your git email is not in permissions.md people"));
            if o.p(0).as_deref() == Some("uninstall") { ok(serve::uninstall()); println!("The panel no longer starts with this computer."); return; }
            if o.p(0).as_deref() == Some("install") {
                let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "noroles".into());
                let port = o.one("port").map(|p| p.parse().unwrap_or_else(|_| fail("--port must be a number"))).unwrap_or(7707);
                let url = ok(serve::install(&dir, &exe, port));
                println!("The panel for {} now starts with this computer and stays on.\nBookmark it: {url}", dir.display());
                if o.one("no-open").is_none() { std::thread::sleep(std::time::Duration::from_millis(700)); let _ = std::process::Command::new("open").arg(&url).status(); }
                return;
            }
            let port = o.one("port").map(|p| p.parse().unwrap_or_else(|_| fail("--port must be a number"))).unwrap_or(7707);
            ok(serve::serve(dir, who, port, o.one("no-open").is_none()));
        }
        Some("mcp" | "mcp-config") => fail("the MCP gateway is still in the JavaScript version for now: `npx noroles@0.4 mcp ...`. For Claude Code, `noroles hook install` covers every tool, local or hosted."),
        Some(other) => fail(&format!("unknown command \"{other}\". Run `noroles help`.")),
    }
}
