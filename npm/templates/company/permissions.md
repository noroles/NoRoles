# Permissions

A permission is the right to say yes to one kind of action that can't be undone.
It is the only authority in this company. Agents never hold one: an agent acts with its person's permissions, never more.

Everything not listed here can be undone in minutes, so anyone just does it.

```yaml
people:
  {{ID}}: { name: "{{NAME}}", email: "{{EMAIL}}" }

agents:
  {{ID}}-agent: { works_for: {{ID}} }

groups:
  founders: [{{ID}}]

permissions:
  money.spend:     { holders: [founders], limits: { amount: 500, per_period: 2000, period: month }, respond_within: 48h }
  money.pay_out:   { holders: [founders], quorum: 2, each: batch, respond_within: 48h }
  price.change:    { holders: [founders], each: item }
  contract.sign:   { holders: [founders], human_only: true, each: item }
  speak.external:  { holders: [founders], each: batch }
  prod.change:     { holders: [founders], each: item }
  data.export:     { holders: [founders], each: item }
  access.change:   { holders: [founders] }
  people.engage:   { holders: [founders] }
  destroy:         { holders: [founders], each: item }
  rule.change:     { holders: [founders], quorum: 2, each: item }
```
