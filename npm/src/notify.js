// Tell the person at the computer when something needs them, without them watching a terminal.
// macOS notification centre. Urgent ones play a sound: money, anything flagged, refusals, incidents.
// NOROLES_NOTIFY=0 turns it off; NOROLES_NOTIFY_LOG=<file> writes notifications to a file instead (tests).
import fs from 'node:fs';
import { spawn } from 'node:child_process';

// The text comes from agents, so it is passed as arguments and never spliced into the script.
const SCRIPT = ['on run argv', 'if (item 3 of argv) is "1" then', 'display notification (item 2 of argv) with title (item 1 of argv) sound name "Glass"', 'else', 'display notification (item 2 of argv) with title (item 1 of argv)', 'end if', 'end run'];

export function notify(title, body, { urgent = false } = {}) {
  if (process.env.NOROLES_NOTIFY === '0') return;
  if (process.env.NOROLES_NOTIFY_LOG) { fs.appendFileSync(process.env.NOROLES_NOTIFY_LOG, JSON.stringify({ title, body, urgent }) + '\n'); return; }
  if (process.platform !== 'darwin') return;
  try {
    const args = SCRIPT.flatMap((l) => ['-e', l]).concat([String(title), String(body).slice(0, 240), urgent ? '1' : '0']);
    spawn('osascript', args, { detached: true, stdio: 'ignore' }).unref();
  } catch {}
}
