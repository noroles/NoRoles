# How to work in this company

This company runs on NoRoles (https://noroles.com). There are no roles and no managers. Authority comes only from permissions.

Before you do anything:
1. Run `npx noroles status`. It lists the mandates, who each one is for, and what is waiting for a yes.
2. Work only inside a mandate where you are the executor. The mandate says the goal, the limits and when it ends.

While you work:
- If an action can be undone in minutes and sends nothing outside (no money, no words, no data), just do it.
- If it can't, ask first and wait for the answer:
  `npx noroles ask --as <your agent id> --mandate <name> --permission <permission> --summary "<exact action>" [--amount 42 --to "<recipient>"] [--tool <key> -- <command>]`
  Put the exact action in the request: amount, recipient, text, command. The person sees exactly that.
- When it is approved, run `npx noroles do <request-id>` if it has a command, or do exactly what was approved.
- Run tools through `npx noroles run <mandate> --as <you> -- <command>` so you only get the keys your mandate allows.
- If something is going wrong (money, words or data leaving that should not), stop the mandate at once:
  `npx noroles stop <mandate> --as <your agent id> --reason "<what you saw>"`. Stopping is always allowed. Only the holder resumes it.

If your tools come through the `noroles` MCP server, it checks every call itself. When a result says a yes is needed, stop, tell your person the request id, and call again with exactly the same arguments after they approve.

Never:
- edit `root.md`, `permissions.md`, `credentials.md`, anything in `mandates/`, `requests/` or `ledger.json`;
- run `noroles yes` or `noroles no`: only people answer;
- use a key that is not given to you by `noroles run` or `noroles do`.

If something is unclear, ask the holder of your mandate. Write every request so a stranger would understand it.
