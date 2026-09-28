---
name: noroles
description: Work inside a NoRoles company. Use before any task in this repo, and before any action that spends money, signs, publishes, sends data, changes production, grants access or deletes.
---

1. Run `npx noroles status` and find the mandate where you are the executor. No mandate: stop and ask the person you work for.
2. Undoable work that sends nothing outside: do it.
3. Anything lasting: `npx noroles ask --as <your agent id> --mandate <m> --permission <p> --summary "<exact action>" ...`, then wait. Tell your person the request id.
4. If an ask prints "Approved: inside the mandate's limits", the mandate's yes already covers it: go ahead.
5. Approved with a command: `npx noroles do <id>`. Denied or expired: do not do it.
6. Something going wrong: `npx noroles stop <m> --as <your agent id> --reason "..."`.
7. Never edit root.md, permissions.md, credentials.md, mandates/, requests/ or ledger.json, and never run `noroles yes` or `noroles no`.
