---
intent: Put a one-page website live so people can find us
metric: page live at our domain by the expiry date
stop_if: we decide on a different name
holder: {{ID}}
executor: {{ID}}-agent
owns: [website]
can:
  money.spend: { amount: 30 }
  tools: [github_read]
expires: {{EXPIRES}}
---

Write the page, pick a hosting plan and buy the domain.
Writing and building are open work. Paying for the domain is money.spend, so ask for a yes first.
