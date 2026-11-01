# `.modal` — detached agent runs

Goble work runs in a `modal.Sandbox` launched from one script, `.modal/run.py`.
The sandbox's main process is `grok-workflow`, the same ACP client the core-infra
`harness-grok` profile runs: it starts one Rhai workflow, passes the task file to
it as `args.task`, and keeps the session alive until the workflow reaches a final
state. Headless `grok -p` would cancel the background workflow at the end of the
first turn, so the harness exists for exactly this.

| Part | Role |
|---|---|
| `build` (local) | builds `.modal/Dockerfile`, publishes the image as `goble-agent` |
| `start` (local) | creates the volume and sandbox, writes the workflow and task |

Nothing is deployed. `start` creates the sandbox from this machine and returns
at once; there is no job registry and no status/logs client. The command prints
the sandbox id, the dashboard shows state, and the result stays on the volume.

## Build, once

```bash
modal run .modal/run.py::build     # build and publish `goble-agent`
```

Rebuild the image after editing `Dockerfile`, `grok-config.toml`, or
`harness/grok-workflow`. A workflow edit needs no rebuild: `start` reads
`.grok/workflows/goble-remote.rhai` from this machine and sends it with the
request.

## Start

```bash
modal run .modal/run.py::start --label release3 --task .modal/tasks/release.md
```

`--workflow` (default `.grok/workflows/goble-remote.rhai`) and `--base-branch`
(default `terminal-first`) are optional. The command returns at once with the
sandbox id, volume, branch, and the model keys it sent.

The sandbox starts before the task is written, waits up to 300 s for the task
and workflow files, clones `terminal-first`, checks out `modal/<label>`, runs the
workflow, then exports the patch.

`.modal/tasks/smoke.md` is a read-only check; `.modal/tasks/smoke-write.md`
proves the agent can write and commit through the harness. The verification
commands are in the skill.

## Interactive session

`shell` starts a long-lived sandbox with the same image, secret, and volume, and
keeps the Grok home on the volume at `/work/.grok`:

```bash
modal run .modal/run.py::shell --label dev --hours 4
modal shell sb-...            # id printed by the command
```

Inside, `cd /work/goble`, then run `grok` for a new session, `grok -c` to
continue the most recent session for that directory, or `grok -r <id>` for a
specific one. To keep grok alive when you close the laptop, run it under
`tmux new -A -s grok`; detach with `Ctrl-b d`, reconnect with
`modal shell sb-...` and `tmux attach -t grok`. A detached tmux session is not
an active exec, so the sandbox still stops after `--idle-minutes`; pass
`--idle-minutes 0` to keep it until `--hours` instead.

If the sandbox reaches its timeout, start `shell` again with the same label: the
session store is on the volume, so `grok -c` resumes where you stopped. The
shell sandbox uses 4 CPUs, 8 GiB, and 4 hours by default; `--hours` changes the
maximum lifetime. It is also stopped after 30 minutes without an active exec
(`--idle-minutes`, `0` disables it): while `modal shell` is attached, the exec
counts as activity, so the sandbox stays up. Job sandboxes keep their Grok home
in the container (`/root/.grok`) and stay stateless.

## Collect

```bash
mkdir -p /tmp/release3-out
modal volume get goble-job-release3 out /tmp/release3-out
git am /tmp/release3-out/out/release3.patch
tail -40 /tmp/release3-out/out/SUMMARY.md
```

`modal volume get` puts the directory inside the destination, hence the nested
`out/`. The same volume also holds the harness log at
`.sandbox/process.log`; download it the same way when a job fails.

## Volume layout

```text
/work/goble                    checkout, branch modal/<label>
/work/.sandbox/task-<label>.md the task
/work/.sandbox/workflows/      the workflow the job runs
/work/.sandbox/process.log     harness output
/work/SUMMARY.md               the workflow's report
/work/out                      patch, commits, status, SUMMARY.md
```

## Secrets

One box, shared with core-infra's `harness-grok`:

```bash
modal secret create staticlabs-sandboxes-llm \
  "DEEPSEEK_API_KEY=..." "XAI_API_KEY=..." "GROK_AUTH_JSON=$(cat ~/.grok/auth.json)"
```

`DEEPSEEK_API_KEY` is required (the default model alias uses it);
`XAI_API_KEY` and `GROK_AUTH_JSON` are optional fallbacks. Refresh the box with
`--force` when the local login rotates.

The sandbox gets the box by name; it holds only the three names above, so no
other secret reaches the container. This is the same box core-infra's
`credentials.llm_values` reads. No GitHub credential is sent: the repository is
public and jobs never push.

## Limits

- One volume per label, one writer at a time. Do not start a label while it runs.
- Each job clones the base branch from GitHub, so commit or push local work
  first.
- Jobs commit locally and never push or open a pull request.
- 8 CPUs, 16 GiB RAM, 24-hour timeout per sandbox.

The full workflow is in `.grok/skills/modal-detached/SKILL.md`.
