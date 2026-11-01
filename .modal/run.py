#!/usr/bin/env python3
"""One detached Goble job: a Modal sandbox running the Grok harness.

Build the image once, then start jobs:

    modal run .modal/run.py::build
    modal run .modal/run.py::start --label release3 --task .modal/tasks/release.md

Nothing is deployed: `start` creates the sandbox from this machine and returns
at once. The sandbox outlives the command, the dashboard shows whether it still
runs, and the result stays on the volume:

    mkdir -p /tmp/release3-out
    modal volume get goble-job-release3 out /tmp/release3-out
    git am /tmp/release3-out/out/release3.patch

For an interactive Grok session on the same volume:

    modal run .modal/run.py::shell --label dev --hours 4
    modal shell <sandbox-id>

Sessions live in `/work/.grok`, so `grok -c` resumes after the sandbox is
replaced. One volume per label; do not reuse a label while it runs.
"""

from __future__ import annotations

import json
import re
import secrets
import shlex
from pathlib import Path

import modal

APP = "goble-sandboxes"
IMAGE = "goble-agent"
VOLUME_PREFIX = "goble-job-"
REPO_URL = "https://github.com/AdrianTuci1/goble.git"

# The same box core-infra's `harness-grok` reads. It holds only the three model
# keys, so the sandbox can receive it whole; there is no other secret in it.
MODEL_SECRET = "staticlabs-sandboxes-llm"

WORK = "/work"
REPO = f"{WORK}/goble"
SANDBOX_DIR = f"{WORK}/.sandbox"
WORKFLOW_DIR = f"{SANDBOX_DIR}/workflows"
OUT = f"{WORK}/out"
LOG = f"{SANDBOX_DIR}/process.log"
SUMMARY = f"{WORK}/SUMMARY.md"
DEFAULT_WORKFLOW = "goble-remote"

CPU = 8.0
MEMORY_MIB = 16384
JOB_TIMEOUT = 24 * 60 * 60
DEFAULT_BASE = "terminal-first"
LABEL_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,40}$")

# Interactive sessions: smaller than a job, because the agent is the only load.
SHELL_CPU = 4.0
SHELL_MEMORY_MIB = 8192
SHELL_HOURS = 4.0
# Stop the shell sandbox when no exec is attached. Sessions survive on the
# volume, so this only ends the container, not the conversation.
SHELL_IDLE_MINUTES = 30

app = modal.App(APP)
HERE = Path(__file__).resolve().parent


def quote(value: str) -> str:
    return shlex.quote(value)


def entrypoint(label: str, base_branch: str) -> str:
    """The sandbox's main process: clone, wait for the task, run the workflow.

    The sandbox starts before the task and the workflow are written, so the
    script waits for both. Output goes to the volume, which stays readable after
    the sandbox is gone.
    """
    task = f"{SANDBOX_DIR}/task-{label}.md"
    workflow = f"{WORKFLOW_DIR}/{DEFAULT_WORKFLOW}.rhai"
    branch = f"modal/{label}"
    patch = f"{OUT}/{label}.patch"
    return f"""set -uo pipefail
mkdir -p {SANDBOX_DIR} {WORKFLOW_DIR} {OUT}
if [ ! -d {REPO}/.git ]; then
  git clone --branch {quote(base_branch)} {REPO_URL} {REPO} >>{LOG} 2>&1 || exit 1
fi
git -C {REPO} fetch origin {quote(base_branch)} >>{LOG} 2>&1 || exit 1
if git -C {REPO} show-ref --verify --quiet refs/heads/{quote(branch)}; then
  git -C {REPO} checkout {quote(branch)} >>{LOG} 2>&1 || exit 1
else
  git -C {REPO} checkout -B {quote(branch)} origin/{quote(base_branch)} >>{LOG} 2>&1 || exit 1
fi

for _ in $(seq 1 300); do [ -s {task} ] && [ -s {workflow} ] && break; sleep 1; done
[ -s {task} ] || {{ echo "task file never arrived" >>{LOG}; exit 78; }}
[ -s {workflow} ] || {{ echo "workflow file never arrived" >>{LOG}; exit 78; }}

export GROK_TASK_FILE={task}
export GROK_HOME=/root/.grok
export GROK_DISABLE_AUTOUPDATER=1
export GROK_MEMORY=0
export GROK_WORKFLOW_TIMEOUT={max(600, JOB_TIMEOUT - 600)}
cd {WORK}
grok-workflow {quote(DEFAULT_WORKFLOW)} >>{LOG} 2>&1
rc=$?

cd {REPO}
base=$(git merge-base HEAD origin/{quote(base_branch)}) || base=""
if [ -n "$base" ]; then
  git format-patch --binary --stdout "$base"..HEAD > {patch}
  git log --oneline "$base"..HEAD > {OUT}/commits.txt
fi
git status --short > {OUT}/status.txt
[ -f {SUMMARY} ] && cp {SUMMARY} {OUT}/SUMMARY.md
exit $rc
"""


@app.local_entrypoint()
def start(
    label: str,
    task: str,
    workflow: str = ".grok/workflows/goble-remote.rhai",
    base_branch: str = DEFAULT_BASE,
) -> None:
    """Create one sandbox and return at once. The workflow is its main process."""
    if not LABEL_RE.match(label):
        raise SystemExit(
            "label must be lowercase letters, digits, '.', '_' or '-', at most 41 chars"
        )
    task_text = Path(task).read_text()
    workflow_text = Path(workflow).read_text()
    if not task_text.strip():
        raise SystemExit(f"the task file {task!r} is empty")
    if not workflow_text.strip():
        raise SystemExit(f"the workflow file {workflow!r} is empty")

    try:
        image = modal.Image.from_name(IMAGE)
    except modal.exception.NotFoundError:
        raise SystemExit(
            f"image {IMAGE!r} is not published; run `modal run .modal/run.py::build`"
        ) from None

    volume_name = VOLUME_PREFIX + label
    volume = modal.Volume.from_name(volume_name, create_if_missing=True)
    sandbox = modal.Sandbox.create(
        "bash",
        "-c",
        entrypoint(label, base_branch),
        app=modal.App.lookup(APP, create_if_missing=True),
        name=f"goble-{label}-{secrets.token_hex(3)}",
        image=image,
        env={"GROK_HOME": "/root/.grok"},
        secrets=[
            modal.Secret.from_name(MODEL_SECRET, required_keys=["DEEPSEEK_API_KEY"])
        ],
        volumes={WORK: volume},
        cpu=CPU,
        memory=MEMORY_MIB,
        timeout=JOB_TIMEOUT,
        workdir=WORK,
        tags={"job": label, "base": base_branch},
    )

    try:
        sandbox.filesystem.make_directory(SANDBOX_DIR)
        sandbox.filesystem.make_directory(WORKFLOW_DIR)
        sandbox.filesystem.make_directory(OUT)
        sandbox.filesystem.write_text(workflow_text, f"{WORKFLOW_DIR}/{DEFAULT_WORKFLOW}.rhai")
        # The task file is what the entrypoint waits for, so it is written last.
        sandbox.filesystem.write_text(task_text, f"{SANDBOX_DIR}/task-{label}.md")
    except modal.exception.Error:
        sandbox.terminate()
        raise

    print(
        json.dumps(
            {
                "label": label,
                "sandbox_id": sandbox.object_id,
                "volume": volume_name,
                "branch": f"modal/{label}",
                "base_branch": base_branch,
            },
            indent=2,
        )
    )


@app.local_entrypoint()
def build() -> None:
    """Build and publish the image once; jobs reference it by name."""
    image = modal.Image.from_dockerfile(
        str(HERE / "Dockerfile"), context_dir=str(HERE)
    )
    image.build(modal.App.lookup(APP, create_if_missing=True)).publish(IMAGE)


def shell_entrypoint(base_branch: str) -> str:
    """Prepare a persistent Grok home on the volume, then stay alive for execs."""
    return f"""set -uo pipefail
mkdir -p {SANDBOX_DIR} {WORK}/.grok
if [ ! -f {WORK}/.grok/config.toml ]; then
  cp /root/.grok/config.toml {WORK}/.grok/config.toml
fi
if [ -n "${{GROK_AUTH_JSON:-}}" ] && [ ! -f {WORK}/.grok/auth.json ]; then
  printf '%s' "$GROK_AUTH_JSON" > {WORK}/.grok/auth.json
  chmod 600 {WORK}/.grok/auth.json
fi
if [ ! -d {REPO}/.git ]; then
  git clone --branch {quote(base_branch)} {REPO_URL} {REPO} >>{LOG} 2>&1 || true
fi
exec sleep infinity
"""


@app.local_entrypoint()
def shell(
    label: str,
    base_branch: str = DEFAULT_BASE,
    hours: float = SHELL_HOURS,
    idle_minutes: float = SHELL_IDLE_MINUTES,
) -> None:
    """Start a long-lived sandbox for an interactive `grok` session.

    The session store lives in `/work/.grok` on the volume, so a later sandbox
    with the same label resumes it with `grok -c` or `grok -r <id>`. The sandbox
    is terminated after `idle_minutes` without an active exec (0 disables that),
    or at `hours` at the latest.
    """
    if not LABEL_RE.match(label):
        raise SystemExit(
            "label must be lowercase letters, digits, '.', '_' or '-', at most 41 chars"
        )
    if hours <= 0 or hours > JOB_TIMEOUT / 3600:
        raise SystemExit(f"hours must be between 0 and {JOB_TIMEOUT // 3600}")
    if idle_minutes < 0 or idle_minutes > hours * 60:
        raise SystemExit("idle_minutes must be between 0 and hours * 60")

    try:
        image = modal.Image.from_name(IMAGE)
    except modal.exception.NotFoundError:
        raise SystemExit(
            f"image {IMAGE!r} is not published; run `modal run .modal/run.py::build`"
        ) from None

    volume_name = VOLUME_PREFIX + label
    sandbox = modal.Sandbox.create(
        "bash",
        "-c",
        shell_entrypoint(base_branch),
        app=modal.App.lookup(APP, create_if_missing=True),
        name=f"goble-shell-{label}-{secrets.token_hex(3)}",
        image=image,
        env={"GROK_HOME": f"{WORK}/.grok", "GROK_DISABLE_AUTOUPDATER": "1"},
        secrets=[
            modal.Secret.from_name(MODEL_SECRET, required_keys=["DEEPSEEK_API_KEY"])
        ],
        volumes={WORK: modal.Volume.from_name(volume_name, create_if_missing=True)},
        cpu=SHELL_CPU,
        memory=SHELL_MEMORY_MIB,
        timeout=int(hours * 3600),
        idle_timeout=int(idle_minutes * 60) if idle_minutes else None,
        workdir=WORK,
        tags={"job": label, "mode": "shell"},
    )
    print(
        json.dumps(
            {
                "label": label,
                "sandbox_id": sandbox.object_id,
                "volume": volume_name,
                "hours": hours,
                "idle_minutes": idle_minutes,
                "attach": f"modal shell {sandbox.object_id}",
                "resume_last": "cd /work/goble && grok -c",
                "resume_one": "cd /work/goble && grok -r <session-id>",
                "keep_alive": "tmux new -A -s grok",
            },
            indent=2,
        )
    )