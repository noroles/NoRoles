//! Tell the person at the computer when something needs them, without them watching a terminal.
//! macOS notification centre. Urgent ones play a sound: money, anything flagged, refusals, incidents.
//! NOROLES_NOTIFY=0 turns it off; NOROLES_NOTIFY_LOG=<file> writes notifications to a file instead (tests).
use std::process::{Command, Stdio};

// The text comes from agents, so it is passed as arguments and never spliced into the script.
const SCRIPT: [&str; 7] = ["on run argv", "if (item 3 of argv) is \"1\" then", "display notification (item 2 of argv) with title (item 1 of argv) sound name \"Glass\"", "else", "display notification (item 2 of argv) with title (item 1 of argv)", "end if", "end run"];

pub fn notify(title: &str, body: &str, urgent: bool) {
    if std::env::var("NOROLES_NOTIFY").as_deref() == Ok("0") { return; }
    if let Ok(f) = std::env::var("NOROLES_NOTIFY_LOG") {
        use std::io::Write;
        if let Ok(mut h) = std::fs::OpenOptions::new().create(true).append(true).open(f) {
            let _ = writeln!(h, "{}", serde_json::json!({ "title": title, "body": body, "urgent": urgent }));
        }
        return;
    }
    if !cfg!(target_os = "macos") { return; }
    let body: String = body.chars().take(240).collect();
    let mut cmd = Command::new("osascript");
    for l in SCRIPT { cmd.arg("-e").arg(l); }
    let _ = cmd.arg(title).arg(body).arg(if urgent { "1" } else { "0" }).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}
