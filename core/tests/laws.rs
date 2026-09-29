// Each test is a law from SPEC.md or an attack found in the paper test. Same cases as npm/test/laws.test.js.
use noroles::company::*;
use noroles::flags::flags;
use noroles::hook;
use noroles::keys;
use noroles::requests::{self, Ask, Proof};
use noroles::util::*;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const PASS: &str = "correct horse battery";
const T0S: &str = "2026-10-01T10:00:00Z";
fn t0() -> i64 { parse_time(T0S).unwrap() }
fn at(h: i64) -> i64 { t0() + h * HOUR }

struct Keys { ana: String, ben: String }
fn keyset() -> &'static Keys {
    static K: OnceLock<Keys> = OnceLock::new();
    K.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("noroles-rs-keys-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        unsafe { std::env::set_var("NOROLES_KEYS", &d); std::env::set_var("NOROLES_NOTIFY", "0"); }
        Keys { ana: keys::keygen("ana", PASS).unwrap(), ben: keys::keygen("ben", PASS).unwrap() }
    })
}

fn tmp(p: &str) -> PathBuf {
    let mut b = [0u8; 6]; getrandom::fill(&mut b).unwrap();
    let d = std::env::temp_dir().join(format!("{p}-{}", hex::encode(b)));
    fs::create_dir_all(&d).unwrap();
    d
}
fn git(dir: &Path, a: &[&str]) -> String { String::from_utf8_lossy(&Command::new("git").args(a).current_dir(dir).output().unwrap().stdout).into_owned() }

fn mandate(dir: &Path, name: &str, extra: V) {
    let mut fm = json!({ "intent": format!("do {name}"), "metric": "done by expiry", "stop_if": "we change plans", "holder": "ana", "executor": "ana-agent", "expires": "2026-10-20" });
    for (k, v) in extra.as_object().unwrap() { fm[k] = v.clone(); }
    fs::write(dir.join("mandates").join(format!("{name}.md")), format!("---\n{}---\nBody.\n", to_yaml(&fm))).unwrap();
}

fn company_with(quorum: i64, each: &str) -> PathBuf {
    let k = keyset();
    let dir = tmp("noroles-rs");
    git(&dir, &["init", "-q"]); git(&dir, &["config", "user.email", "ana@acme.test"]); git(&dir, &["config", "user.name", "Ana"]);
    fs::create_dir_all(dir.join("mandates")).unwrap();
    fs::create_dir_all(dir.join(".noroles")).unwrap();
    fs::write(dir.join("root.md"), "```yaml\nroot: [ana]\nobserver: null\n```\n").unwrap();
    fs::write(dir.join("permissions.md"), format!("# Permissions
```yaml
people:
  ana: {{ email: ana@acme.test, key: \"{}\" }}
  ben: {{ email: ben@acme.test, key: \"{}\" }}
agents:
  ana-agent: {{ works_for: ana }}
groups:
  founders: [ana, ben]
permissions:
  money.spend:   {{ holders: [founders], quorum: {quorum}, each: {each}, limits: {{ amount: 500, per_period: 550, period: month }}, respond_within: 48h }}
  money.pay_out: {{ holders: [founders], quorum: 2 }}
  speak.external: {{ holders: [ana] }}
  rule.change:   {{ holders: [founders], quorum: 2 }}
```
", k.ana, k.ben)).unwrap();
    fs::write(dir.join("credentials.md"), "# Credentials\n```yaml\ncredentials:\n  github_read: { env: GITHUB_TOKEN, exercises: [] }\n  card:        { env: CARD_KEY, exercises: [money.spend] }\n  stripe:      { env: STRIPE_KEY, exercises: [money.spend, money.pay_out] }\n```\n").unwrap();
    fs::write(dir.join(".noroles").join("secrets.env"), "GITHUB_TOKEN=gh-open\nCARD_KEY=card-secret\nSTRIPE_KEY=stripe-secret\n").unwrap();
    mandate(&dir, "site", json!({ "can": { "money.spend": { "amount": 100 }, "tools": ["github_read", "card"] }, "owns": ["website"] }));
    let mut meta = Obj::new();
    for f in META_FILES { if let Ok(t) = fs::read_to_string(dir.join(f)) { meta.insert(f.into(), V::from(sha(&t))); } }
    fs::write(dir.join("ledger.json"), json!({ "meta": meta }).to_string()).unwrap();
    requests::commit(&dir, "start", true).unwrap();
    dir
}
fn company() -> PathBuf { company_with(1, "item") }

fn yes(dir: &Path, id: &str, who: &str, now: i64) -> Result<V, String> { requests::decide(dir, id, who, true, None, Proof::Passphrase(PASS), now) }
fn opened_as(dir: &Path, name: &str) {
    let r = requests::open_mandate(dir, name, "ana", t0()).unwrap();
    if status(&r) != "approved" { yes(dir, s(&r, "id").unwrap(), "ana", t0()).unwrap(); }
}
fn opened(dir: &Path) { opened_as(dir, "site") }
fn id(r: &V) -> String { s(r, "id").unwrap().to_string() }
fn c(dir: &Path) -> Company { load(dir).unwrap() }
fn errors(dir: &Path) -> Vec<Problem> { check(&c(dir), t0()).into_iter().filter(|x| x.error).collect() }
fn has_err(dir: &Path, needle: &str) -> bool { errors(dir).iter().any(|e| e.msg.contains(needle) || e.r#where.contains(needle)) }

fn spend_with(dir: &Path, amount: f64, now: i64, f: impl FnOnce(&mut Ask)) -> Result<V, String> {
    let mut a = Ask { mandate: "site".into(), permissions: vec!["money.spend".into()], asker: "ana-agent".into(), summary: format!("buy domain for {amount}"), amount: Some(amount), to: Some("Registrar Inc".into()), payload: V::Null, ..Default::default() };
    f(&mut a);
    requests::ask(dir, a, now)
}
fn spend(dir: &Path, amount: f64) -> Result<V, String> { spend_with(dir, amount, t0(), |_| {}) }
fn err_of<T>(r: Result<T, String>) -> String { match r { Err(e) => e, Ok(_) => panic!("expected an error") } }

fn edit_request(dir: &Path, id: &str, f: impl FnOnce(&mut V)) {
    let p = dir.join("requests").join(format!("{id}.yaml"));
    let mut doc: V = yaml(&fs::read_to_string(&p).unwrap(), "r").unwrap();
    f(&mut doc);
    fs::write(&p, to_yaml(&doc)).unwrap();
}

#[test] fn a_fresh_company_passes_the_laws() { assert!(errors(&company()).is_empty()); }

#[test] fn law4_nothing_lasting_in_an_unopened_mandate() {
    assert!(err_of(spend(&company(), 20.0)).contains("not active: not opened yet"));
}

#[test] fn opening_asks_the_holders_of_every_permission_in_can() {
    let dir = company();
    let r = requests::open_mandate(&dir, "site", "ana", t0()).unwrap();
    assert_eq!(status(&r), "pending");
    assert_eq!(permissions_of(&r), vec!["money.spend"]);
    yes(&dir, &id(&r), "ben", t0()).unwrap();
    assert!(mandate_state(&c(&dir), "site", t0()).0);
}

#[test] fn only_people_say_yes() {
    let dir = company(); opened(&dir);
    let r = spend(&dir, 20.0).unwrap();
    assert!(err_of(yes(&dir, &id(&r), "ana-agent", t0())).contains("not a person"));
}

#[test] fn limits_the_lower_wins() { let dir = company(); opened(&dir); assert!(err_of(spend(&dir, 150.0)).contains("above the limit 100")); }

#[test] fn limits_per_period_summed_across_mandates() {
    let dir = company();
    mandate(&dir, "ads", json!({ "can": { "money.spend": { "amount": 400 } }, "owns": ["ads"] }));
    opened(&dir); opened_as(&dir, "ads");
    let a = requests::ask(&dir, Ask { mandate: "ads".into(), permissions: vec!["money.spend".into()], asker: "ana-agent".into(), summary: "ads".into(), amount: Some(400.0), ..Default::default() }, t0()).unwrap();
    yes(&dir, &id(&a), "ana", t0()).unwrap();
    let b = spend(&dir, 100.0).unwrap();
    yes(&dir, &id(&b), "ana", t0()).unwrap();
    assert!(err_of(spend(&dir, 100.0)).contains("500 already used this month"));
}

#[test] fn a_request_edited_after_it_was_asked_is_void() {
    let dir = company(); opened(&dir);
    let r = spend(&dir, 20.0).unwrap();
    edit_request(&dir, &id(&r), |d| d["action"]["to"] = V::from("Attacker LLC"));
    assert!(err_of(yes(&dir, &id(&r), "ana", t0())).contains("edited after it was asked"));
}

#[test] fn a_mandate_edited_after_its_yes_is_not_active() {
    let dir = company(); opened(&dir);
    mandate(&dir, "site", json!({ "can": { "money.spend": { "amount": 100 }, "tools": ["github_read", "card"] }, "owns": ["website"], "intent": "something wider" }));
    let (a, why) = mandate_state(&c(&dir), "site", t0());
    assert!(!a); assert!(why.contains("changed since it was approved"));
}

#[test] fn law6_one_owner() { let dir = company(); mandate(&dir, "rival", json!({ "owns": ["website"] })); assert!(has_err(&dir, "claimed by both")); }

#[test] fn law2_side_effects_count() {
    let dir = company();
    mandate(&dir, "site", json!({ "can": { "money.spend": { "amount": 100 }, "tools": ["stripe"] }, "owns": ["website"] }));
    assert!(has_err(&dir, "stripe\" can exercise money.pay_out"));
}

#[test] fn at_most_90_days() { let dir = company(); mandate(&dir, "site", json!({ "owns": ["website"], "opened": "2026-10-01", "expires": "2027-03-01" })); assert!(has_err(&dir, "at most 90")); }

#[test] fn parts_stay_inside_their_parent() {
    let dir = company();
    mandate(&dir, "site", json!({ "can": { "money.spend": { "amount": 100 }, "tools": ["github_read", "card"] }, "owns": ["website"], "parts": ["blog"] }));
    mandate(&dir, "blog", json!({ "can": { "speak.external": {} }, "owns": ["blog"] }));
    assert!(has_err(&dir, "can speak.external, which its parent site does not have"));
    assert!(has_err(&dir, "owns \"blog\", outside its parent site"));
}

#[test] fn needs_no_cycle() {
    let dir = company();
    mandate(&dir, "a", json!({ "needs": ["b"], "owns": ["a"] }));
    mandate(&dir, "b", json!({ "needs": ["a"], "owns": ["b"] }));
    assert!(has_err(&dir, "needs cycle"));
}

#[test] fn meta_files_need_two_people() {
    let dir = company();
    let f = dir.join("permissions.md");
    fs::write(&f, fs::read_to_string(&f).unwrap().replace("amount: 500", "amount: 50000")).unwrap();
    assert!(errors(&dir).iter().any(|e| e.r#where == "permissions.md"));
    let r = requests::propose_meta(&dir, "ana", t0()).unwrap();
    assert!(err_of(yes(&dir, &id(&r), "ana", t0())).contains("holds none"));
    yes(&dir, &id(&r), "ben", t0()).unwrap();
    requests::settle(&dir, at(12)).unwrap();
    assert!(errors(&dir).iter().any(|e| e.r#where == "permissions.md"), "still waiting");
    requests::settle(&dir, at(25)).unwrap();
    assert!(errors(&dir).is_empty());
}

#[test] fn meta_changed_again_during_the_wait_is_void() {
    let dir = company();
    let f = dir.join("permissions.md");
    fs::write(&f, fs::read_to_string(&f).unwrap().replace("amount: 500", "amount: 600")).unwrap();
    let r = requests::propose_meta(&dir, "ana", t0()).unwrap();
    yes(&dir, &id(&r), "ben", t0()).unwrap();
    fs::write(&f, fs::read_to_string(&f).unwrap().replace("amount: 600", "amount: 99999")).unwrap();
    requests::settle(&dir, at(25)).unwrap();
    assert_eq!(status(&c(&dir).requests[&id(&r)]), "void");
}

#[test] fn quorum_distinct_humans_never_the_asker() {
    let dir = company_with(2, "item");
    let o = requests::open_mandate(&dir, "site", "ana", t0()).unwrap();
    yes(&dir, &id(&o), "ben", t0()).unwrap();
    assert_eq!(status(&c(&dir).requests[&id(&o)]), "pending");
    assert!(err_of(yes(&dir, &id(&o), "ana", t0())).contains("holds none"));
    requests::settle(&dir, at(25)).unwrap();
    assert_eq!(status(&c(&dir).requests[&id(&o)]), "approved");
}

#[test] fn an_agent_asking_counts_as_its_person() {
    let dir = company_with(2, "item");
    let o = requests::open_mandate(&dir, "site", "ben", t0()).unwrap();
    yes(&dir, &id(&o), "ana", t0()).unwrap();
    requests::settle(&dir, at(25)).unwrap();
    let r = spend_with(&dir, 20.0, at(25), |_| {}).unwrap();
    assert!(err_of(yes(&dir, &id(&r), "ana", at(25))).contains("holds none"));
}

#[test] fn silence_passes_to_root_then_is_a_no() {
    let dir = company(); opened(&dir);
    let r = spend(&dir, 20.0).unwrap();
    requests::settle(&dir, at(97)).unwrap();
    assert_eq!(status(&c(&dir).requests[&id(&r)]), "expired");
    let e = err_of(requests::do_action(&dir, &id(&r), at(97)));
    assert!(e.contains("expired") || e.contains("not approved"));
}

#[test] fn do_runs_once_with_only_its_key() {
    let dir = company(); opened(&dir);
    let out = dir.join("out.txt");
    let cmd = vec!["sh".into(), "-c".into(), format!("echo \"card=$CARD_KEY stripe=$STRIPE_KEY gh=$GITHUB_TOKEN\" > {}", out.display())];
    let r = spend_with(&dir, 20.0, t0(), |a| { a.tool = Some("card".into()); a.command = Some(cmd); }).unwrap();
    assert!(err_of(requests::do_action(&dir, &id(&r), t0())).contains("not approved"));
    yes(&dir, &id(&r), "ana", t0()).unwrap();
    assert_eq!(requests::do_action(&dir, &id(&r), t0()).unwrap(), 0);
    assert_eq!(fs::read_to_string(&out).unwrap().trim(), "card=card-secret stripe= gh=");
    assert!(err_of(requests::do_action(&dir, &id(&r), t0())).contains("already carried out"));
}

#[test] fn run_gives_only_open_keys() {
    let dir = company(); opened(&dir);
    let out = dir.join("env.txt");
    unsafe { std::env::set_var("CARD_KEY", "leaked-from-parent"); }
    let code = requests::run(&dir, "site", "ana-agent", &["sh".into(), "-c".into(), format!("echo \"gh=$GITHUB_TOKEN card=$CARD_KEY who=$NOROLES_EXECUTOR\" > {}", out.display())], t0()).unwrap();
    assert_eq!(code, 0);
    assert_eq!(fs::read_to_string(&out).unwrap().trim(), "gh=gh-open card= who=ana-agent");
}

#[test] fn every_step_is_a_commit() {
    let dir = company(); opened(&dir);
    let r = spend(&dir, 20.0).unwrap();
    yes(&dir, &id(&r), "ana", t0()).unwrap();
    let log = git(&dir, &["log", "--format=%s"]);
    assert!(log.contains(&format!("yes {} by ana (approved)", id(&r))));
    assert!(log.contains(&format!("ask {}", id(&r))));
}

#[test] fn a_yes_written_by_hand_does_not_count() {
    let dir = company(); opened(&dir);
    let r = spend_with(&dir, 20.0, t0(), |a| a.command = Some(vec!["true".into()])).unwrap();
    edit_request(&dir, &id(&r), |d| { d["approvals"].as_array_mut().unwrap().push(json!({ "by": "ana", "at": T0S, "sig": "AAAA" })); d["status"] = V::from("approved"); });
    assert!(err_of(requests::do_action(&dir, &id(&r), t0())).contains("not approved"));
    assert!(requests::audit(&dir, t0()).unwrap().iter().any(|x| x.what.contains("not validly signed")));
}

#[test] fn the_wrong_passphrase_signs_nothing() {
    let dir = company(); opened(&dir);
    let r = spend(&dir, 20.0).unwrap();
    assert!(requests::decide(&dir, &id(&r), "ana", true, None, Proof::Passphrase("wrong-passphrase"), t0()).is_err());
    assert_eq!(g(&c(&dir).requests[&id(&r)], "approvals").as_array().unwrap().len(), 0);
}

#[test] fn a_yes_cannot_move_to_another_request() {
    let dir = company(); opened(&dir);
    let a = spend(&dir, 20.0).unwrap();
    let b = spend_with(&dir, 90.0, t0(), |x| x.summary = "something else".into()).unwrap();
    yes(&dir, &id(&a), "ana", t0()).unwrap();
    let y = g(&c(&dir).requests[&id(&a)], "approvals")[0].clone();
    edit_request(&dir, &id(&b), |d| d["approvals"].as_array_mut().unwrap().push(y));
    assert!(err_of(requests::do_action(&dir, &id(&b), t0())).contains("not approved"));
}

#[test] fn hashes_cover_nested_fields() {
    let dir = company(); opened(&dir);
    let r = spend_with(&dir, 20.0, t0(), |a| a.payload = json!({ "invoice": { "iban": "DE00 1111" } })).unwrap();
    edit_request(&dir, &id(&r), |d| d["action"]["payload"]["invoice"]["iban"] = V::from("XX99 6666"));
    assert!(err_of(yes(&dir, &id(&r), "ana", t0())).contains("edited after it was asked"));
}

#[test] fn each_mandate_the_opening_yes_covers() {
    let dir = company_with(1, "mandate"); opened(&dir);
    let out = dir.join("paid.txt");
    let r = spend_with(&dir, 20.0, t0(), |a| { a.tool = Some("card".into()); a.command = Some(vec!["sh".into(), "-c".into(), format!("echo \"$CARD_KEY\" > {}", out.display())]); }).unwrap();
    assert_eq!(status(&r), "approved");
    assert!(s(&r, "covered_by").is_some());
    assert_eq!(requests::do_action(&dir, &id(&r), t0()).unwrap(), 0);
    assert_eq!(fs::read_to_string(&out).unwrap().trim(), "card-secret");
    assert!(err_of(spend(&dir, 150.0)).contains("above the limit"));
}

#[test] fn each_can_tighten_never_loosen() {
    let dir = company_with(1, "item");
    mandate(&dir, "site", json!({ "can": { "money.spend": { "amount": 100, "each": "mandate" }, "tools": ["github_read", "card"] }, "owns": ["website"] }));
    assert!(has_err(&dir, "looser than permissions.md"));
}

#[test] fn each_item_and_batch() {
    let dir = company_with(1, "item"); opened(&dir);
    assert!(err_of(spend_with(&dir, 20.0, t0(), |a| a.items = Some(vec!["a.com".into(), "b.com".into()]))).contains("each: item"));
    let d2 = company_with(1, "batch"); opened(&d2);
    let r = spend_with(&d2, 40.0, t0(), |a| a.items = Some(vec!["a.com for 20".into(), "b.com for 20".into()])).unwrap();
    assert_eq!(listk(g(&r, "action"), "items"), vec!["a.com for 20", "b.com for 20"]);
    assert_eq!(status(&r), "pending");
}

#[test] fn break_glass() {
    let dir = company(); opened(&dir);
    requests::stop(&dir, "site", "ana-agent", "charges look wrong", t0()).unwrap();
    assert!(mandate_state(&c(&dir), "site", t0()).1.contains("stopped by ana-agent"));
    assert!(err_of(requests::run(&dir, "site", "ana-agent", &["true".into()], t0())).contains("not active"));
    assert!(requests::incidents(&dir).iter().any(|i| s(i, "what").unwrap_or("").contains("charges look wrong")));
    let r = requests::resume(&dir, "site", "ana-agent", t0()).unwrap();
    assert!(err_of(yes(&dir, &id(&r), "ben", t0())).contains("holds none"));
    yes(&dir, &id(&r), "ana", t0()).unwrap();
    assert!(mandate_state(&c(&dir), "site", t0()).0);
}

#[test] fn law5_missed_due() {
    let dir = company();
    mandate(&dir, "site", json!({ "can": { "money.spend": { "amount": 100 }, "tools": ["github_read", "card"] }, "owns": ["website"], "due": [{ "date": "2026-10-05", "what": "renew the domain" }] }));
    requests::settle(&dir, parse_time("2026-10-06T00:00:00Z").unwrap()).unwrap();
    let inc = requests::incidents(&dir).into_iter().find(|i| s(i, "what").unwrap_or("").contains("renew the domain")).unwrap();
    assert_eq!(s(&inc, "holder"), Some("ana"));
    let iid = s(&inc, "id").unwrap().to_string();
    assert!(err_of(requests::resolve_incident(&dir, &iid, "ben", "x", t0())).contains("only ana or root"));
    requests::resolve_incident(&dir, &iid, "ana", "renewed a day late", t0()).unwrap();
    assert!(requests::incidents(&dir).iter().any(|i| s(i, "id") == Some(&iid) && truthy(g(i, "resolved"))));
}

#[test] fn audit_outside_commit() {
    let dir = company(); opened(&dir);
    let r = spend(&dir, 20.0).unwrap();
    let p = dir.join("requests").join(format!("{}.yaml", id(&r)));
    fs::write(&p, fs::read_to_string(&p).unwrap() + "# hi\n").unwrap();
    git(&dir, &["commit", "-qam", "tidy up"]);
    assert!(requests::audit(&dir, t0()).unwrap().iter().any(|x| x.what.contains("outside NoRoles")));
}

#[test] fn commits_only_its_own_files() {
    let dir = company(); opened(&dir);
    fs::write(dir.join("draft.html"), "<h1>work in progress</h1>").unwrap();
    let r = spend(&dir, 20.0).unwrap();
    assert_eq!(git(&dir, &["show", "--name-only", "--format=", "HEAD"]).trim(), format!("requests/{}.yaml", id(&r)));
    assert!(git(&dir, &["status", "--porcelain"]).contains("?? draft.html"));
}

// ---------- flags ----------

#[test] fn flags_new_payee_then_not() {
    let dir = company(); opened(&dir);
    let first = spend(&dir, 10.0).unwrap();
    let f = flags(&c(&dir), &first);
    assert!(f.iter().any(|x| x.contains("new payee: Registrar Inc")));
    assert!(f.iter().any(|x| x.contains("first time mandate site uses money.spend")));
    yes(&dir, &id(&first), "ana", t0()).unwrap();
    let second = spend_with(&dir, 10.0, at(1), |_| {}).unwrap();
    assert!(flags(&c(&dir), &second).is_empty());
}

#[test] fn flags_amount_far_above_usual() {
    let dir = company(); opened(&dir);
    for i in 0..3 { let r = spend_with(&dir, 10.0, at(i), |_| {}).unwrap(); yes(&dir, &id(&r), "ana", t0()).unwrap(); }
    let big = spend_with(&dir, 40.0, at(4), |_| {}).unwrap();
    assert!(flags(&c(&dir), &big).iter().any(|x| x.contains("4.0× the usual money.spend (median 10)")));
}

#[test] fn flags_declined_before() {
    let dir = company(); opened(&dir);
    let r = spend(&dir, 10.0).unwrap();
    requests::decide(&dir, &id(&r), "ana", false, Some("wrong registrar"), Proof::Passphrase(PASS), t0()).unwrap();
    let again = spend_with(&dir, 10.0, at(1), |_| {}).unwrap();
    assert!(flags(&c(&dir), &again).iter().any(|x| x.contains("declined on 2026-10-01 by ana: wrong registrar")));
}

#[test] fn flags_burst() {
    let dir = company(); opened(&dir);
    let mut last = V::Null;
    for i in 0..6 { last = spend_with(&dir, 1.0, t0() + i * 60_000, |_| {}).unwrap(); }
    assert!(flags(&c(&dir), &last).iter().any(|x| x.contains("request 6 from ana-agent in the last hour")));
}

#[test] fn flags_cannot_be_hidden_by_editing() {
    let dir = company(); opened(&dir);
    let r = spend(&dir, 10.0).unwrap();
    let p = dir.join("requests").join(format!("{}.yaml", id(&r)));
    fs::write(&p, fs::read_to_string(&p).unwrap() + "flags: []\n").unwrap();
    let cc = c(&dir);
    assert!(!flags(&cc, &cc.requests[&id(&r)]).is_empty());
}

// ---------- the hook: every Claude Code tool call ----------

fn hook_company() -> PathBuf {
    let dir = company();
    let f = dir.join("permissions.md");
    fs::write(&f, fs::read_to_string(&f).unwrap().replace("speak.external: { holders: [ana] }", "speak.external: { holders: [ana], each: item }")).unwrap();
    fs::write(dir.join("tools.md"), "```yaml\ntools:\n  \"*__send_message\": { permissions: [speak.external], to: to }\n  \"*__pay_invoice\": { permissions: [money.spend], amount: amount, to: vendor }\n  \"*__create_draft\": open\n```\n").unwrap();
    mandate(&dir, "mail", json!({ "can": { "speak.external": {} }, "owns": ["mail"] }));
    let mut meta = Obj::new();
    for f in META_FILES { if let Ok(t) = fs::read_to_string(dir.join(f)) { meta.insert(f.into(), V::from(sha(&t))); } }
    fs::write(dir.join("ledger.json"), json!({ "meta": meta }).to_string()).unwrap();
    requests::commit(&dir, "tools", true).unwrap();
    opened(&dir); opened_as(&dir, "mail");
    dir
}
fn call(dir: &Path, tool: &str, input: V, now: i64) -> String {
    hook::pre_tool_use(&json!({ "session_id": "s1", "cwd": dir, "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": input }), "ana-agent", now)
}
fn denied(out: &str) -> Option<String> {
    let v: V = serde_json::from_str(out).ok()?;
    (s(g(&v, "hookSpecificOutput"), "permissionDecision") == Some("deny")).then(|| s(g(&v, "hookSpecificOutput"), "permissionDecisionReason").unwrap_or("").to_string())
}

#[test] fn hook_reads_and_drafts_pass_without_a_decision() {
    let dir = hook_company();
    assert_eq!(call(&dir, "mcp__claude_ai_Gmail__search_threads", json!({ "q": "x" }), t0()), "");
    assert_eq!(call(&dir, "mcp__claude_ai_Gmail__create_draft", json!({ "to": "bob@x.test" }), t0()), "");
    assert_eq!(call(&dir, "Read", json!({ "file_path": dir.join("root.md") }), t0()), "");
}

#[test] fn hook_a_lasting_call_waits_for_a_yes_then_passes_once() {
    let dir = hook_company();
    let msg = json!({ "to": "bob@partner.test", "body": "Hi Bob" });
    let first = denied(&call(&dir, "mcp__claude_ai_Gmail__send_message", msg.clone(), t0())).expect("denied until a yes");
    assert!(first.contains("mandate mail"));
    let rid = first.split("Asked as ").nth(1).unwrap().split(';').next().unwrap().to_string();
    assert!(denied(&call(&dir, "mcp__claude_ai_Gmail__send_message", msg.clone(), t0())).unwrap().contains("still waiting"));
    assert_eq!(s(g(&c(&dir).requests[&rid], "action"), "to"), Some("bob@partner.test"));
    yes(&dir, &rid, "ana", t0()).unwrap();
    assert_eq!(call(&dir, "mcp__claude_ai_Gmail__send_message", msg.clone(), t0()), "", "passes after the yes");
    assert_eq!(status(&c(&dir).requests[&rid]), "done");
    assert!(denied(&call(&dir, "mcp__claude_ai_Gmail__send_message", msg, t0())).unwrap().contains("needs a yes"), "once only");
}

#[test] fn hook_a_yes_covers_only_the_exact_arguments() {
    let dir = hook_company();
    let first = denied(&call(&dir, "mcp__claude_ai_Gmail__send_message", json!({ "to": "bob@partner.test", "body": "Hi" }), t0())).unwrap();
    let rid = first.split("Asked as ").nth(1).unwrap().split(';').next().unwrap().to_string();
    yes(&dir, &rid, "ana", t0()).unwrap();
    assert!(denied(&call(&dir, "mcp__claude_ai_Gmail__send_message", json!({ "to": "attacker@evil.test", "body": "Hi" }), t0())).is_some());
}

#[test] fn hook_unmapped_writes_are_refused_and_limits_hold() {
    let dir = hook_company();
    assert!(denied(&call(&dir, "mcp__stripe__frobnicate", json!({}), t0())).unwrap().contains("refused"));
    assert!(denied(&call(&dir, "mcp__billing__pay_invoice", json!({ "amount": 150, "vendor": "X" }), t0())).unwrap().contains("above the limit 100"));
}

#[test] fn hook_the_record_is_protected() {
    let dir = hook_company();
    for f in ["ledger.json", "permissions.md", "tools.md", "requests/r-x.yaml", "incidents/i.yaml"] {
        assert!(denied(&call(&dir, "Write", json!({ "file_path": dir.join(f), "content": "x" }), t0())).is_some(), "{f}");
    }
    assert_eq!(call(&dir, "Write", json!({ "file_path": dir.join("notes.md"), "content": "x" }), t0()), "");
}

#[test] fn hook_a_stopped_mandate_stops_lasting_calls() {
    let dir = hook_company();
    requests::stop(&dir, "mail", "ana-agent", "looks wrong", t0()).unwrap();
    assert!(denied(&call(&dir, "mcp__claude_ai_Gmail__send_message", json!({ "to": "a@b.test" }), t0())).unwrap().contains("no open mandate"));
}

#[test] fn hook_work_names_the_mandate_for_the_session() {
    let dir = hook_company();
    // both site and mail could not send; give site speak.external too, so the choice is ambiguous
    mandate(&dir, "site", json!({ "can": { "money.spend": { "amount": 100 }, "speak.external": {}, "tools": ["github_read", "card"] }, "owns": ["website"] }));
    opened(&dir);
    assert!(denied(&call(&dir, "mcp__x__send_message", json!({ "to": "a@b.test" }), t0())).unwrap().contains("several mandates"));
    call(&dir, "Bash", json!({ "command": "noroles work site" }), t0());
    assert!(denied(&call(&dir, "mcp__x__send_message", json!({ "to": "a@b.test" }), t0())).unwrap().contains("in mandate site"));
}

#[test] fn hook_patterns() {
    let p = hook::Pattern::new("*__send_message|*slack_send*").unwrap();
    assert!(p.matches("mcp__claude_ai_Gmail__send_message"));
    assert!(p.matches("mcp__abc__slack_send_message"));
    assert!(!p.matches("mcp__abc__send_messages_later"));
}

#[test] fn canonical_matches_javascript() {
    assert_eq!(canonical(&json!({ "b": 1, "a": [2.5, null, true, "x\n\"y\""], "c": { "z": 1e21, "y": 40.0 } })), r#"{"a":[2.5,null,true,"x\n\"y\""],"b":1,"c":{"y":40,"z":1e+21}}"#);
}
