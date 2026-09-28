# No Roles: spec v1.0

A company of people and agents with no roles and no managers. Authority comes from **permissions**. Work happens in **mandates**. Everyone follows **rules**. Git is the record. Part 1 is the model everyone reads. Part 2 is what the orchestrator code must do.

**Words:** *root*: the people in `root.md`, who must match the company's legal owners or directors; any change to `root.md` also needs the observer's written confirmation that it matches the legal record; the last stop for anything unanswered. *Orchestrator*: deterministic code, not an AI agent, that runs Part 2. *Yes*: an approval a verified human signs through the orchestrator; a verified written notice from an outside party counts as that party's yes or no. *Incident*: a logged miss its holder must resolve or explain. *Outside party*: anyone not working for the company; a *contractor* is a person paid to work for it without being employed. *Raw sensitive data*: IDs, payment details, intimate content, health, precise location, credentials.

**Start:** root writes the first `root.md`, `permissions.md` and rules directly; nothing else is ever written that way.

# Part 1: the model

## Three things

### Permission (`permissions.md`)
An action with a lasting effect. Each lists:
- `holders`: people or groups who can say yes. An outside party holds one only through a signed contract, and only what it gives them.
- `quorum`: how many different humans listed as holders must say yes, not counting the asker (default 1; a holder may approve their own work at quorum 1). If fewer such humans exist than the quorum, each missing yes becomes a 24-hour wait with notice to the observer named in `root.md` (an accountant, lawyer or board member), who may veto.
- `limits`: ceilings such as `refund <= $50` or `per_period <= $500`, summed across mandates, and `each: mandate | batch | item`: how often a fresh yes is needed (default `mandate`, one yes covering every action inside the limits). `permissions.md` sets each permission's default; a mandate may tighten it, never loosen it. Count caps (`posts <= 20/day`) are limits. An *item* is one action as shown to the signer; for automated sends, the item is the template. A *batch* is any set of items shown to the signer in full in one request (a list of posts with their schedule, uploads, payees, recipients).
- `respond_within`: default 48h, and never more than half the time left to the earliest approved due it blocks; with under 8h left, all holders and root are asked at once. Silence passes it on; at the end of the chain silence is a no.
- `human_only`: needs a named, verified human (signatory, KYC, custodian, bank). Agents prepare; the holder acts. Name a backup who can legally act, a recovery path with a re-verify due, or `backup: none` so requests go out early.

Starter set:
| permission | covers |
|---|---|
| `money.spend` | commit or spend company money: purchases and vendor invoices, and refunds within a mandate's refund limit. Contractor pay is never here |
| `money.pay_out` | pay money we owe that is not a purchase: partner, agency and affiliate payouts, contractor pay (even when a platform bills it), refunds above a mandate's limit. Default `quorum: 2, each: batch` |
| `price.change` | what customers pay or get: prices, trials, plans, credits, entitlements. Default `each: item` |
| `contract.sign` | commit the company in writing. `human_only`, `each: item` |
| `speak.external` | claims, prices, legal or marketing words leaving the company: posts, pages, emails, press, 1:1 messages to partners or vendors. Default `each: batch` under the content rule. Metadata about where content came from (C2PA, EXIF) is a claim |
| `prod.change` | a production change with a lasting effect (law 1) |
| `data.export` | export, disclose or delete personal data; read raw sensitive data outside product use; send personal data to a processor, model or sub-processor not in `rules/processors.md`. Product use is the product serving its own users as its privacy rule describes, including deletions that rule requires |
| `access.change` | grant, widen or revoke access to tools and systems; every grant names the mandate and the person or group it serves. Who is in a holder group is part of `permissions.md` (a meta rule) |
| `people.engage` | start or end work with a person, or change who they work for |
| `destroy` | delete or terminate anything unrecoverable |
| `rule.change` | change a rule or accept a risk no rule covers. `permissions.md`, these laws, `root.md`, the orchestrator's code and config, the identity provider's and git host's configuration (not their user lists, which are `access.change`) are meta rules: `quorum: 2, each: item` |

For both money permissions: a recurring charge (subscription, weekly billing) needs one yes, at the permission's quorum, with a `per_period` limit and a due before each charge; a new or changed payee account (bank, card, PayPal, wallet or crypto address) needs `each: item`, confirmed with the payee through a different channel than the request came from (or by a small test payment the payee confirms back), by someone other than the requester. A refund to the card or account the customer paid from is not a new payee. The due before a recurring charge is a *notice due*: passing it without objection keeps the charge within its limit and is logged, not an incident.

### Mandate (`mandates/<name>.md`)
```yaml
intent:   outcome and why
metric:   a number, or "decision recorded by <date>", or "reviewed by @x"
stop_if:  when to stop
holder:   one person, or several with a quorum
executor: who does the work: an agent, a person, or a mix (a person must accept)
owns:     [surfaces, resources, relationships only this mandate may change]
can:      [permissions and tools, with limits]   # narrows, never widens, permissions.md; may name a required holder
expires:  <date>    # one-off work, at most 90 days out
review:   <date>    # standing duties, at most 90 days out, renewed by root
due:      [dated obligations: renewals, runs, filings, dates promised outside]
parts:    [child mandates]   # their can and owns are subsets of the parent's
needs:    [other mandates]   # no cycles; no due before a due it needs
visible_to: [everyone]       # hides all but name and holder; never hides anything from root or from holders of its `can`
```
Opening or widening a mandate asks every holder of every permission in `can`; loosening `stop_if`, dues or limits counts as widening; a part inherits its parent's yes and `owns`. The holder or root may cancel a mandate, which cancels its parts; open dues pass to the parent or root. A signer may withdraw a yes before the action runs. Root may replace a holder.

### Rule (`rules/*.md`)
Binds every mandate touching its subject: content, privacy, approved processors, card-network rules, platform terms, a partner's contract terms, coding norms. A rule says what it allows and forbids; empty rules do not count. One rule may cover a class of party. A signed contract is itself the rule for that party. Where a rule protects an outside party's own consent (likeness, their contract terms), that party is a holder, their yes comes only through the channel written in their signed contract, confirmed by a human, and their silence is a no that is never passed on; using a right the contract gives us (ending on notice) needs only our holders; if it is unclear whether the contract gives us a right that touches that party's consent, it does not, until they agree through the channel in their contract; whether it is unclear is decided by our holders of that rule, and any one of them finding it unclear is enough. Rules may carry `visible_to`, hiding terms and outside holders from everyone but root and the rule's holders. Meetings, DMs and files are visible only to their participants unless the privacy rule or a participant (or that participant's own agent) shares them, and anything derived from them keeps that. Changing a non-meta rule needs `rule.change` at its own quorum.

## Eight laws
1. **Only lasting effects need a yes.** An action is open when it can be undone in minutes and leaves nothing lasting outside the company: no money moved, no words or data sent, nothing anyone relies on. A product change users see but can roll back instantly (layout, features, labels, plain error messages) is open when `can` names it and a `stop_if` is watched; words that cite law, a partner, a country or a price are claims. Always `prod.change`, flag or not: how money is charged, pricing, who may see restricted content (age, login, tier, region or consent gates), and switching off any safety, security, compliance or spend control.
2. **Every lasting effect needs its holders**, for every permission it touches, side effects included (a config that raises spend touches `money.spend`).
3. **Nothing is unheld or unanswered.** Every permission, duty and open question has a holder: a question with no permission goes to the holder of the mandate it blocks or that owns what it concerns. A due blocked by a no or by silence becomes an incident, never a yes. An unanswered question about a live safety, security or compliance risk is an incident for root at once.
4. **Executors act only inside a mandate**, with only what `can` gives them. Agents never write mandates, permissions, rules, `root.md` or yes records; people change them only through the orchestrator.
5. **Duties stand, obligations are watched.** Access ends when the mandate it serves ends or its person or group leaves it. Every date promised outside is a due; a promise about another mandate's work adds it to `needs`; dues stay with the work when it changes hands. A missed due (other than a notice due) or passed review is an incident, first for the holder, then root. Past review, a standing mandate keeps only what its dues need.
6. **One owner per target.** No two active mandates outside the same family share an `owns` item or contradict each other or a rule; a contradiction the code cannot see goes to both holders, then root. Claiming an `owns` item needs root's yes and ends with the mandate; anyone else changing an owned item needs its holder's yes. Strategy that binds several mandates is a rule.
7. **Break glass lowers exposure only.** Anyone may stop, pause, hide, roll back, turn a protective gate on, or freeze an agent's access, to limit harm; a human account may be frozen only by root or a holder of `access.change`, with the evidence logged; without a yes, if the action can be undone. It never switches a control off, freezes a healthy human, changes a quorum count, or edits a yes record; once the holder has restored a target, it may be used there again only for a newly logged harm. It may miss an outside due only to stop harm to a person, a legal breach or money loss, and then tells root at once. The holder reviews within 24h, then root.
8. **Same law for people.** People follow mandates and rules too. A person must accept being named; taking them off a mandate needs its holder. Before a mandate touches an outside party or platform (sends to, signs with, pays, or holds an account with it), a rule or contract must cover it; reading public information and declining an offer are not touching.

# Part 2: what the orchestrator must do
1. **Classify by effect.** A request carrying personal or raw sensitive data counts as a send. Read-only calls with no personal data, to tools named in `can`, are covered by the mandate's yes; reading public information is open, including through a logged-in tool the orchestrator limits to allowlisted read-only pages. Map each credential to the permissions it can exercise; a tool that acts on outside systems counts as those permissions on every call. A deploy is open only when every change it ships is open. `destroy` and delete keys are never open.
2. **Give only `can`.** An agent executor starts with only the tools and keys its mandate names. A person executor gets the same: the orchestrator grants their accounts only what `can` names, for the mandate's life. Credentials are short-lived and revoked when the mandate ends or the executor leaves it.
3. **Collect yeses honestly.** Show the signer the exact action (payload, amount, recipient), not an agent's summary. Record only human-signed yeses; count quorum by distinct listed humans. A mandate that can read raw sensitive data sends it outside only `each: item`, except to its own subject, or to a listed processor or payment rail for its listed purpose. Agents reach outside only destinations named in their mandate's `can`.
4. **Check before activating:** conflicts (law 6), `needs` cycles, parts within parents, a rule or contract for every outside party (law 8), limits within ceilings.
5. **Watch time:** every due, review, expiry and `respond_within`; when a due moves, flag every due that needs it; route per laws 3 and 5.
6. **Watch reality:** compare real systems with the repo; open an incident for anything done outside it; void commits it did not make, except the genesis commit and a valid recovery.
7. **Record everything** as commits and keep a mirror. Recovery is only for an orchestrator that is down or corrupted: every root member signs out of band, it restores the last good state and nothing else (never `root.md`), the observer may veto within 24h, and an incident opens.
