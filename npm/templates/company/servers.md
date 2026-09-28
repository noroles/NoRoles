# Servers

The MCP servers an agent may reach, only through `noroles mcp`. NoRoles starts them itself and hands them their keys, so the agent never sees a key.

How each tool is treated:
- a tool marked read-only by its server is open: it runs at once;
- a tool marked destructive needs `destroy`;
- any other tool that can change things outside must be mapped to its permissions below, or it is refused.

Results are checked before the agent sees them: keys are always removed, and personal data (emails, phone numbers, card numbers, IBANs) is removed unless the mandate can `data.export`.

```yaml
servers: {}
#  github:
#    command: npx
#    args: ["-y", "@modelcontextprotocol/server-github"]
#    env: { GITHUB_PERSONAL_ACCESS_TOKEN: github_read }   # env var: credential name from credentials.md
#    tools:
#      create_issue: { permissions: [speak.external] }
#      "*": {}                                            # anything else: follow the server's own hints
#  billing:
#    command: node
#    args: ["billing-mcp.js"]
#    env: { BILLING_KEY: card }
#    tools:
#      pay_invoice: { permissions: [money.spend], amount: amount, to: vendor, currency_default: USD }
```
