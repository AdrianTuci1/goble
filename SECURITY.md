# Security policy

Goble runs agents with access to your files, shell and credentials. A vulnerability
here can mean code execution, credential disclosure or a sandbox escape, so reports
are handled privately first.

## Supported versions

| Version | Supported |
|---|---|
| `0.1.x` (latest release) | Yes |
| `main` builds | Yes, until the next release |
| Older releases | No |

## Reporting a vulnerability

Use GitHub's private advisory form:

<https://github.com/AdrianTuci1/goble/security/advisories/new>

Do not open a public issue, discussion or PR for a vulnerability. The form keeps the
report private until an advisory is published.

Include what you can:

- The version or commit, the platform, and the medium (local, remote worker, web client).
- The steps to reproduce and the impact you expect.
- A proof of concept, log excerpt or screenshot (remove real credentials first).
- Whether the issue is already public elsewhere.

## What to expect

- Acknowledgement within 3 business days.
- A triage decision and a severity estimate within 7 days.
- A fix, a workaround or a documented mitigation for confirmed issues; the advisory is
  published after a release contains the fix, with credit unless you ask otherwise.

## Scope

In scope:

- The daemon, the worker, the control socket and the MCP surface.
- The sandbox, command approval and secret handling.
- The remote transports (mTLS, pairing, the gateway) and the update channel.
- Installers and release artifacts.

Out of scope:

- A compromised machine or an attacker who already controls the user's account.
- Prompts that make a model produce wrong or unsafe output without a runtime flaw.
- Vulnerabilities in a harness you connected through BYOH, unless Goble's adapter
  weakens its isolation.
- Reports from automated scanners without a reproducible impact.

## Safe harbor

Good-faith research on your own deployment is welcome. Do not access other users'
data, degrade a shared service, or publish details before an advisory is released.
