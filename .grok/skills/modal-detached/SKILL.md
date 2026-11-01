---
name: modal-detached
description: >
  Run scoped Goble work in a detached Modal sandbox and collect the patch later:
  build the `goble-agent` image once, deploy the small `sandboxes` control
  plane, start jobs as `modal.Sandbox` instances, and read their state,
  transcript, and patch from a per-job volume.
  Use when the user asks to delegate a task to a Modal agent, run something
  detached or remotely, check on a detached job, or collect its patch.
  Triggers: "modal", "detached", "trimite la agent", "preia update-urile",
  "rulează în sandbox", "/modal-detached".
---

# Detached Goble runs on Modal

Modal apps are the control plane; work runs in **sandboxes** (`modal.Sandbox`).
A job is not a function call and not an ephemeral app: the agent is the
sandbox's main process, so the container ends when the agent exits and costs
nothing afterwards.

## Two steps, each done once

```bash
modal run .modal/images.py     # build and publish the image as `goble-agent`
modal deploy .modal/app.py     # deploy the `sandboxes` control plane
```

The image build is separate on purpose. Modal's sandbox guide asks for named
images (`Image.build(app).publish(name)` then `Image.from_name`), so starting a
job never blocks on a build. Rebuild the image after editing `.modal/Dockerfile`
or `.modal/grok-config.toml`; redeploy the app after editing `app.py` or
`.grok/agents/goble-remote.md` (the control image carries the agent file).

## Client

| Command | Result |
|---|---|
| `./.modal/sandboxes start --label <label> --prompt-file <path>` | creates the sandbox, returns at once |
| `./.modal/sandboxes status --label <label>` | `running`, `finished`, or `failed`, exit code, commits, transcript size |
| `./.modal/sandboxes logs --label <label> [--tail N] [--raw]` | transcript tail, rendered unless `--raw` |
| `./.modal/sandboxes summary --label <label>` | `out/RESULT.md` and the agent's `summary.md` |
| `./.modal/sandboxes stop --label <label>` | terminates the sandbox, keeps the volume |
| `./.modal/sandboxes list` | every job recorded by the app |
| `./.modal/sandboxes profiles` | what `start` accepts |

`start` also takes `--model`, `--base-branch`, `--max-turns`.

## What `start` does

1. Checks the label (lowercase, single use) and looks up `goble-agent`.
2. Creates the sandbox with 8 CPUs, 16 GiB, a 24-hour timeout, the three
   secrets, and the volume `goble-job-<label>` mounted at `/work`.
3. Writes the agent definition and the task into the sandbox. The task file is
   written last because the entrypoint waits for it.
4. Records the sandbox id in `modal.Dict goble-jobs`.

The entrypoint clones `terminal-first`, checks out `modal/<label>`, runs
`grok` on the task, then writes `out/<label>.patch`, `out/commits.txt`,
`out/status.txt`, and `out/RESULT.md`. The raw streaming-json transcript is
`/work/.sandbox/process.log`.

## The model

The image carries `/root/.grok/config.toml` with the alias `deepseek` →
`deepseek-flash` on `https://api.deepseek.com` (`env_key = "DEEPSEEK_API_KEY"`),
so `--model deepseek` resolves; without that file the CLI advertises only
`grok-4.7` and the alias fails with `unknown model id`. The key comes from the
Modal secret `staticlabs-sandboxes-llm`.

## Files

| Path | Purpose |
|---|---|
| `images.py` | builds and publishes `goble-agent` |
| `app.py` | the `sandboxes` control plane (`sandbox_*` functions) |
| `client.py` | local CLI, calls the deployed functions |
| `sandboxes` | shell wrapper around `client.py` |
| `Dockerfile` | Python 3.12, Rust stable, Grok CLI, git, ripgrep, Node |
| `grok-config.toml` | the DeepSeek alias, copied to `/root/.grok/config.toml` |
| `bootstrap.sh` | auth, git identity, folder-trust grant |
| `grok_tail.py` | renders the transcript locally |
| `tasks/*.md` | task prompts |

Each job volume holds:

```text
repo                      clone, branch modal/<label>
.sandbox/task-<label>.md   the task
.sandbox/process.log       raw streaming-json transcript
out/<label>.patch          commits since the merge base
out/commits.txt
out/status.txt
out/RESULT.md
```

## Collect

```bash
./.modal/sandboxes summary --label release3
modal volume get goble-job-release3 out /tmp/release3-out
git am /tmp/release3-out/release3.patch
```

## Secrets

```bash
modal secret create goble-grok-auth "GROK_AUTH_JSON=$(cat ~/.grok/auth.json)"
modal secret create goble-github-token "GITHUB_TOKEN=$(gh auth token)" "GH_TOKEN=$(gh auth token)"
```

`staticlabs-sandboxes-llm` must exist with `DEEPSEEK_API_KEY`. Refresh
`goble-grok-auth` with `modal secret create --force ...` when the local login
rotates, or every job fails at the first model call.

## Rules

- One label per workstream, one volume per label. A label is claimed in
  `modal.Dict goble-jobs` and cannot be started twice.
- Jobs clone the base branch from GitHub, so push local work first.
- Jobs commit locally and never push. Review the patch here.
- The agent must never build or launch the GUI, runs targeted tests only, and
  adds no lint suppressions.
- The sandbox has open outbound network, which the agent needs for crates,
  npm, and the model API.
