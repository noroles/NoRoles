# NoRoles

**Run a company of people and AI agents on permissions instead of roles.** https://noroles.com

Agents now write, build, sell and pay. Companies still run on titles, managers and approvals, so the work waits. NoRoles replaces the org chart with one open list: who can say yes to the few actions that can't be undone (money leaving, contracts, public words, personal data). Everything else, anyone just does. An agent acts with exactly its person's permissions, never more.

```bash
npx noroles init my-company
```

## What is here

| | |
|---|---|
| [`MANIFESTO.md`](MANIFESTO.md) | eighteen principles: how we think and work |
| [`SPEC.md`](SPEC.md) | the model: permissions, mandates, rules, eight laws, and what the orchestrator must enforce |
| [`npm/`](npm) | the `noroles` command line: start a company, ask for a yes, sign it, run agents with only the keys their mandate allows |

## How work runs

```bash
noroles open first-website       # the holders say yes to a mandate once
noroles ask --as me-agent --mandate first-website --permission money.spend \
  --summary "buy example.com for 1 year" --amount 12 --currency USD --to Namecheap
noroles yes <id>                 # a person, in a terminal, sees the exact action and signs
noroles do <id>                  # runs the approved command once, with only its own key
noroles run first-website --as me-agent -- claude
noroles mcp-config first-website --as me-agent > .mcp.json   # every tool call goes through NoRoles
```

See [`npm/README.md`](npm/README.md) for what the code enforces today and what it does not yet.

## Contributing

Issues and pull requests are welcome. Every change to how approvals work needs a test in `npm/test/laws.test.js` that names the law or attack it covers.

MIT licensed.
