# `.modal` — detached agent runs

Goble work runs in a `modal.Sandbox`, launched by the small control plane
deployed as the app `sandboxes`. The agent is the sandbox's main process, so the
container stops when the agent exits. Each job gets its own volume,
`goble-job-<label>`, mounted at `/work`:

```text
/work/repo                  clone, branch modal/<label>
/work/.sandbox/process.log  raw streaming-json transcript
/work/.sandbox/task-<label>.md
/work/out                   patch, commits, status, RESULT.md
```

## Build and deploy, once

```bash
modal run .modal/images.py     # build and publish the image as `goble-agent`
modal deploy .modal/app.py     # deploy the `sandboxes` control plane
```

The image build is separate so a job never waits on a build. Rebuild the image
after editing `Dockerfile` or `grok-config.toml`; redeploy the app after editing
`app.py` or `.grok/agents/goble-remote.md`.

## Pieces

| Path | Purpose |
|---|---|
| `images.py` | builds and publishes `goble-agent` |
| `app.py` | the `sandboxes` control plane (`sandbox_*` functions) |
| `client.py` | local CLI, calls the deployed functions |
| `sandboxes` | wrapper around `client.py` |
| `Dockerfile` | Python 3.12, Rust stable, Grok CLI, git, ripgrep, Node |
| `grok-config.toml` | the DeepSeek alias copied to `/root/.grok/config.toml` |
| `bootstrap.sh` | per-sandbox auth, git identity, folder-trust grant |
| `grok_tail.py` | renders transcripts locally |
| `tasks/*.md` | task prompts |

The model and the house rules live in `.grok/agents/goble-remote.md`.

## Secrets

```bash
modal secret create goble-grok-auth "GROK_AUTH_JSON=$(cat ~/.grok/auth.json)"
modal secret create goble-github-token "GITHUB_TOKEN=$(gh auth token)" "GH_TOKEN=$(gh auth token)"
```

`staticlabs-sandboxes-llm` must exist with `DEEPSEEK_API_KEY`. Refresh
`goble-grok-auth` with `--force` when the local login rotates.

## Run

```bash
./.modal/sandboxes start --label release3 --prompt-file .modal/tasks/release.md
./.modal/sandboxes status --label release3
./.modal/sandboxes logs --label release3 --tail 40
./.modal/sandboxes summary --label release3
./.modal/sandboxes list
./.modal/sandboxes stop --label release3
```

Collect the patch after the job returns:

```bash
modal volume get goble-job-release3 out /tmp/release3-out
git am /tmp/release3-out/release3.patch
```

## Limits

- One sandbox per label, one volume per label; parallel labels cannot collide.
- Each job clones the base branch from GitHub, so commit or push local work
  first.
- Jobs commit locally and never push or open a pull request.
- 8 CPUs, 16 GiB RAM, 24-hour timeout per sandbox.

The full workflow is in `.grok/skills/modal-detached/SKILL.md`.
