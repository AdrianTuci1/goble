---
name: modal-detached
description: >
  Run scoped Goble work in a detached Modal sandbox through the grok harness:
  build the `goble-agent` image once, start a job with `.modal/run.py`, open an
  interactive `grok` session on the same volume, and read the workflow's
  `SUMMARY.md` and exported patch from the job volume.
  Use when the user asks to delegate a task to a Modal agent, run something
  detached or remotely, open an interactive Grok session in the sandbox, resume
  one, check on a detached job, or collect its patch.
  Triggers: "modal", "detached", "trimite la agent", "preia update-urile",
  "rulează în sandbox", "sesiune interactivă", "reia sesiunea",
  "/modal-detached".
---

# Detached Goble runs on Modal

The runtime is the same one the core-infra `harness-grok` profile uses. A job is
a `modal.Sandbox` whose main process is `grok-workflow`, the ACP client that
starts exactly one Rhai workflow and keeps the ACP session open until the
workflow reaches a final state. Headless `grok -p` is not used: it closes the
session at the end of the first turn and cancels a background workflow before it
calls its first agent.

`.modal/run.py` has two local entrypoints and nothing deployed:

| Part | Role |
|---|---|
| `build` | builds `.modal/Dockerfile`, publishes `goble-agent` |
| `start` | creates the volume and sandbox, writes the workflow and task |

## Build, once

```bash
modal run .modal/run.py::build     # build and publish `goble-agent`
```

Rebuild the image after editing `Dockerfile`, `grok-config.toml`, or
`harness/grok-workflow`. A workflow edit needs no rebuild: `start` reads it from
this machine.

## Start

```bash
modal run .modal/run.py::start --label release3 --task .modal/tasks/release.md
```

Optional: `--workflow` (default `.grok/workflows/goble-remote.rhai`) and
`--base-branch` (default `terminal-first`). The command returns at once with the
sandbox id, volume, branch, and the model keys it sent.

## Interactive session

`shell` starts a long-lived sandbox with the same image, secret, and volume. The
Grok home is `/work/.grok` on the volume, so sessions survive the container:

```bash
modal run .modal/run.py::shell --label dev --hours 4
modal shell sb-...            # id printed by the command
```

Inside, `cd /work/goble`, then:

| Command | Effect |
|---|---|
| `grok` | new session with the DeepSeek default |
| `grok -c` | continue the most recent session for `/work/goble` |
| `grok -r <id>` | resume a specific session |

To keep grok running when the laptop closes, start it under
`tmux new -A -s grok`; detach with `Ctrl-b d`, reconnect with
`modal shell sb-...` then `tmux attach -t grok`. A detached tmux session is not
an active exec, so the sandbox still stops after `--idle-minutes`; pass
`--idle-minutes 0` to keep it until `--hours` instead. Without tmux, exiting
grok or the shell leaves the session on disk.

If the sandbox reaches its timeout, run `shell` again with the same label and
`grok -c`: the session store is in `/work/.grok`, mounted from
`goble-job-<label>`. Job sandboxes use `/root/.grok` and stay stateless. The
shell sandbox defaults to 4 CPUs, 8 GiB, and 4 hours (`--hours`); it is also
stopped after 30 minutes without an active exec (`--idle-minutes`, `0` disables
it). An attached `modal shell` counts as activity, so the sandbox stays up while
you are in it and stops soon after you leave.

## Task and workflow contract

1. `start` creates the sandbox with 8 CPUs, 16 GiB, a 24-hour timeout, the model
   secret, and the volume `goble-job-<label>` at `/work`.
2. It writes the workflow to `/work/.sandbox/workflows/goble-remote.rhai` and the
   task to `/work/.sandbox/task-<label>.md`, the task last.
3. The entrypoint clones `terminal-first`, checks out `modal/<label>`, waits for
   both files, exports `GROK_TASK_FILE`, and runs `grok-workflow goble-remote`.
4. The workflow reads the task from `args.task`, works in `/work/goble`, commits
   locally, and writes `/work/SUMMARY.md`.
5. After `grok-workflow` exits, the entrypoint exports `out/<label>.patch`,
   `out/commits.txt`, `out/status.txt`, and `out/SUMMARY.md`.

## The model

The image carries `/root/.grok/config.toml` with the alias `deepseek` →
`deepseek-flash` on `https://api.deepseek.com` (`env_key = "DEEPSEEK_API_KEY"`),
so workflow agents resolve the default model. Without that file the CLI
advertises only `grok-4.7` and the alias fails with `unknown model id`.

## Files

| Path | Purpose |
|---|---|
| `run.py` | build and start; nothing deployed |
| `Dockerfile` | Python 3.12, Rust stable, Grok CLI, `grok-workflow`, git, Node |
| `harness/grok-workflow` | the ACP client, vendored from core-infra |
| `grok-config.toml` | the DeepSeek alias, copied to `/root/.grok/config.toml` |
| `tasks/*.md` | task prompts |

The job's house rules live in `.grok/workflows/goble-remote.rhai`.

Volume layout:

```text
goble/                      checkout, branch modal/<label>
.sandbox/task-<label>.md     the task, read as args.task
.sandbox/workflows/          the workflow the harness runs
.sandbox/process.log         grok-workflow output
.sandbox/workflow-state.json the final workflow state
SUMMARY.md                   the workflow's report
out/                         patch, commits, status, SUMMARY.md
```

## Collect

```bash
mkdir -p /tmp/release3-out
modal volume get goble-job-release3 out /tmp/release3-out
git am /tmp/release3-out/out/release3.patch
tail -40 /tmp/release3-out/out/SUMMARY.md
```

`modal volume get` puts the directory inside the destination, hence `out/`.
Download `.sandbox/process.log` the same way when a job failed.

## Smoke test

Two tasks cover the harness without touching product code:

| Task | What it proves |
|---|---|
| `tasks/smoke.md` | the agent runs, reads git state, writes `/work/SUMMARY.md` |
| `tasks/smoke-write.md` | the agent writes a file, reads it back, commits it |

Run the write test:

```bash
modal run .modal/run.py::start --label smoke-write1 --task .modal/tasks/smoke-write.md
# wait for the sandbox to exit (dashboard, or Sandbox.from_id(...).poll())
mkdir -p /tmp/smoke-write1-out
modal volume get goble-job-smoke-write1 out /tmp/smoke-write1-out
cat /tmp/smoke-write1-out/out/commits.txt      # test(smoke): write through the harness
head -20 /tmp/smoke-write1-out/out/SUMMARY.md  # Model section: deepseek-flash
```

The patch `out/smoke-write1.patch` creates `SMOKE-WRITE.md`. To confirm the model
independently, read `.sandbox/workflow.json` from the volume: `result._meta.modelId`
is `deepseek-flash`, the alias in `grok-config.toml`.

## Secrets

One box, shared with core-infra's `harness-grok`:

```bash
modal secret create staticlabs-sandboxes-llm \
  "DEEPSEEK_API_KEY=..." "XAI_API_KEY=..." "GROK_AUTH_JSON=$(cat ~/.grok/auth.json)"
```

`DEEPSEEK_API_KEY` is required; the other two are fallbacks. Refresh the box
with `modal secret create --force ...` when the local login rotates, or every
job fails at the first model call.

The sandbox gets the box by name; it holds only the three names above, so no
other secret reaches the container. This is the same box core-infra's
`credentials.llm_values` reads. No GitHub credential is sent: the repository is
public and jobs never push.

## Rules

- One volume per label, one writer at a time. Do not start a label while it runs.
- Jobs clone the base branch from GitHub, so push local work first.
- Jobs commit locally and never push. Review the patch here.
- The workflow's house rules tell the agent to never build or launch the GUI,
  to run targeted tests only, and to add no lint suppressions.
- The sandbox has open outbound network, which the agent needs for crates, npm,
  and the model API.
