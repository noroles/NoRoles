# NoRoles

Run a company of people and AI agents on permissions instead of roles. https://noroles.com

```bash
npx noroles init my-company
```

That creates a git repo with:

- `permissions.md`: who can say yes to the few actions that can't be undone (spending, paying people, signing, speaking in public, changing prices or production, sharing personal data, giving access, deleting, changing the rules).
- `mandates/`: one file per piece of work, with a goal, a holder, an executor (a person or an agent), limits and an end date.
- `rules/`: what is allowed and forbidden.
- `credentials.md`: which key can exercise which permission. Values stay in `.noroles/secrets.env`.
- `AGENTS.md`, `CLAUDE.md` and a Claude Code skill, so an agent knows how to work here on its first read.

## Put NoRoles in the call path

```bash
noroles mcp-config first-website --as me-agent > .mcp.json   # Claude Code then reaches its tools only through NoRoles
```

`noroles mcp` is an MCP server that starts the real MCP servers listed in `servers.md` and stands between them and the agent. The agent never gets their keys. Every call is checked, whatever the agent decides:

- a tool its server marks read-only, or non-destructive and closed-world, runs at once;
- a tool mapped in `servers.md` to a permission becomes a request for a yes; nothing reaches the tool until a person signs, and the yes covers only those exact arguments, once;
- a destructive tool needs `destroy`; any other tool that can change things outside and is not mapped is refused;
- results are checked before the agent sees them: keys are always removed, and emails, phone numbers, card numbers and IBANs are removed unless the mandate can `data.export`.

## How work runs

```bash
noroles open first-website          # the holders say yes to the mandate once
noroles ask --mandate first-website --permission money.spend \
  --summary "buy example.com for 1 year" --amount 12 --currency USD --to Namecheap
noroles yes r-20261001-a1b2c3       # a person, in a terminal, sees the exact action and types yes
noroles do  r-20261001-a1b2c3       # runs the approved command once, with only its own key
noroles run first-website --as me-agent -- claude   # an agent gets only the open keys its mandate names
noroles status
```

What the code enforces today:

- Only people answer, and every answer is signed. `noroles keygen` gives each person an ed25519 key locked with a passphrase; `yes` and `no` refuse to run inside `noroles run` or without a terminal. Approval is recomputed from signatures every time, never read from a status field, so a yes written into a file by hand counts for nothing.
- The signer sees the exact action. A request edited after it was asked is void.
- Limits are the lower of the mandate and `permissions.md`; `per_period` is summed across all mandates.
- Quorum counts distinct humans, never the asker or the asker's agent. When fewer people hold a permission than its quorum, each missing yes becomes a 24h wait.
- Silence passes to root, then counts as a no.
- `each`: with `mandate` (the default) the yes that opened a mandate covers every action inside its limits; `batch` shows every item in one request; `item` means one action per request. A mandate may tighten this, never loosen it.
- Break glass: anyone, agent included, can `noroles stop` a mandate. Only its holder or root lifts the stop.
- Missed dues, passed reviews, silent requests and stops open incidents for the holder, who resolves or explains them.
- `noroles audit` checks every signature and hash and flags any commit to the record made outside NoRoles.
- A key that can move money is never handed to an agent. It is used once, by `noroles do`, for an approved action.
- A mandate edited after its yes stops being active. Editing `root.md`, `permissions.md` or `credentials.md` by hand blocks everything until two people accept it with `rule.change`.
- `check` enforces one owner per target, parts inside their parent, no `needs` cycles, at most 90 days per mandate, and keys that match `can`.
- Every step is a git commit.

Not yet: the output guard is pattern-based, so it catches common formats, not every kind of personal data. Also not yet: watching real outside systems (bank, Stripe, social accounts) for changes made around NoRoles, short-lived keys issued per mandate, a web or phone app for answering.

The full model is in [SPEC.md](https://github.com/noroles/noroles/blob/main/SPEC.md). MIT licensed.
