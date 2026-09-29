# Tools

Which tool calls need a yes, when the agent works through `noroles hook` in Claude Code.
It covers every tool the agent has: local MCP servers and connectors added in claude.ai alike.

How a call is treated:
- the first line below whose pattern matches the tool name decides (`*` is any text, `|` separates alternatives);
- `open` runs at once; `{ permissions: [...] }` asks for a yes, and the yes covers only those exact arguments, once;
- an MCP tool that matches no line is open only when its name reads like reading (get, list, search, read...) or drafting; anything else is refused;
- Claude Code's own tools (files, shell, web) are not listed here; the company record is protected either way.

Tool names look like `mcp__<server>__<tool>`, for example `mcp__claude_ai_Gmail__send_message`.
`to`, `amount` and `currency` name the argument that holds the recipient, amount or currency, so the person sees them and flags work. A list means the first of those arguments the call has.

```yaml
tools:
  # words that leave the company
  "*__send_message|*__send_email|*__reply|*__reply_all|*__forward|*__send_draft": { permissions: [speak.external], to: [to, recipient, recipients, email, thread_id, message_id] }
  "*slack_send_message|*slack_schedule_message|*__post_message|*__chat_post*": { permissions: [speak.external], to: [channel_id, channel, user_id, to] }
  "*__publish*|*__post_*|*__create_post|*__tweet*": { permissions: [speak.external] }
  "*__share*|*__invite*|*__add_member*|*__add_permission*": { permissions: [access.change] }
  # money
  "*__pay*|*__create_payment*|*__transfer*|*__refund*|*__create_charge*|*__buy_*|*__purchase*": { permissions: [money.spend], amount: [amount, total, value], currency: currency, to: [to, recipient, vendor, payee, customer] }
  # drafts are undone in a second
  "*__create_draft|*__update_draft|*__save_draft|*__delete_draft": open
  # things that are hard to undo
  "*__delete*|*__remove*|*__trash*|*__archive*|*__destroy*|*__drop*": { permissions: [destroy] }
  # everyday work inside the company, undone in minutes
  "*__save_issue|*__save_comment|*__create_issue|*__update_issue|*__create_comment": open
  "*__create_event|*__update_event|*__respond_to_event": open
  "*__notion-create-*|*__notion-update-*|*__create_page*|*__update_page*": open
  "*__create_file|*__update_file|*__copy_file": open
  "*__add_reaction|*__slack_add_reaction|*__label_*|*__unlabel_*|*__mark_*": open
```
