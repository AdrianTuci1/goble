# Authentication

Goble connects to **model providers** (the LLMs the agent calls) and maintains a **principal identity** for the machine. These are two separate concerns: providers are where the agent sends requests; the identity is who the workspace belongs to.

---

## Model Providers

A provider is an LLM endpoint, configured with the model name, base URL and an API key. Goble lets you set one or more providers. On first launch you add a provider and its key; the key is stored as a **credential** (see [Credentials](04-credentials.md)).

Provider settings live in the settings view:

- **Model** — the model id the agent uses for turns.
- **Provider** — the endpoint (OpenAI-compatible, or a hosted provider).
- **API key** — saved as a credential name; the value never enters the conversation.

The composer's model selector switches the active model. Changing it mid-conversation applies to the next turn.

---

## Principal Identity

Every machine has a `principal_id` under `~/.goble/principal_id`, the identity of the workspace's primary principal. `~/.goble/auth.json` holds session credentials for that principal.

A **principal** is any identity that can reach this workspace: the local user, a remote operator, or a service account. Goble records all of them and what they may do — see [Principals and Access](05-principals-and-access.md).

---

## Where Credentials Live

Secrets are never written into the transcript. Goble stores them under their **name** and substitutes the value server-side when a tool runs. See [Credentials](04-credentials.md) for how to reference one.

---

## Troubleshooting

- **"No model provider configured"** — Open settings and add a provider + API key.
- **401 from a provider** — The key was revoked or expired; update the credential in settings.
- **Wrong model answering** — Check the composer's model selector; changing it goes into effect next turn.
