# Credentials

Goble keeps secrets (API keys, tokens) so the agent can use them **without ever seeing them**. The model sees only a **name**; the value is substituted at execution time, server-side, so a key never appears in the conversation, in the transcript, or in tool logs.

---

## How It Works

1. You save a credential from an ask-user card or in settings: a **name** (e.g. `github-token`) and the **value**, stored under `~/.goble`'s credentials store.
2. The `credentials` tool lists only **names** — never values.
3. Inside a `run_command`, write `{{credential:<name>}}` anywhere in the command or arguments.
4. At execution time the harness substitutes the stored value into argv and runs the command. The substituted value is **not** part of the tool's arguments or its result.

Example — use a stored GitHub token without exposing it:

```
run_command(command="gh api /user", args=["--header", "Authorization: token {{credential:github-token}}"])
```

The model writes `{{credential:github-token}}`; the harness inserts the real token only when the process is spawned.

---

## Why It Matters

- The **model never sees the key** — only a placeholder name.
- The **transcript stays clean** — the value is never echoed into a message or event.
- **Revocation is centralized** — rotate a value in one place; every reference follows.

---

## Managing Credentials

- **Save** — from an ask-user card (the card has a name field and a masked value field), or from settings.
- **List** — ask the agent; it calls `credentials` and reports the names.
- **Use** — reference `{{credential:<name>}}` inside `run_command`.

---

## Related

- [Tools](09-tools.md) — `credentials` and `run_command`.
- [Principals and Access](05-principals-and-access.md) — credentials are recorded per principal.
