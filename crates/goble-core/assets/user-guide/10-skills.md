# Skills and Personas

Skills and personas give the agent reusable instructions and a consistent voice, and can be tool-adjacent — a skill may come with helper scripts.

---

## Skills

A **skill** is a documented procedure the agent can load into context. A skill lives under `~/.goble/skills/<name>/` (user-installed) or `~/.goble/bundles/skills/<name>/` (bundled), as a `SKILL.md` plus optional reference files and scripts.

Typical structure:

```
~/.goble/bundles/skills/<name>/SKILL.md
~/.goble/bundles/skills/<name>/reference.md
~/.goble/bundles/skills/<name>/scripts/...
```

Skills are loaded as tool-adjacent docs and injected into context so the agent follows the documented workflow. Ask the agent to "use the `<name>` skill" or let it discover one relevant to the task.

## Personas

A **persona** is a configured voice/role — e.g. `implementer`, `reviewer`, `security-auditor`. Personas are stored in `~/.goble/bundles/personas/` (and configurable per agent) and are assembled into an agent's system prompt:

```
persona + config → system prompt
```

## Creating Skills and Personas

Both can be created or edited from natural language:

- "Create a skill for releasing a crate" — the agent scaffolds the `SKILL.md` and any scripts.
- "Create a persona that acts as a security auditor" — it writes the persona and binds it to an agent.

---

## Related

- [Tools](09-tools.md) — how skills relate to the tool set.
- [Agents](08-agents.md) — how a persona binds to an agent.
