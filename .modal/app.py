#!/usr/bin/env python3
"""Control plane for detached Goble runs: the deployed app `sandboxes`.

Deploy once, then start and inspect jobs:

    modal deploy .modal/app.py
    ./.modal/sandboxes start --label release3 --prompt-file .modal/tasks/release.md
    ./.modal/sandboxes status --label release3

A job is a `modal.Sandbox`, not a function call: the agent is the sandbox's
main process, so the container ends when the agent exits and costs nothing after
that. The control app stays small; it only creates sandboxes, reads their state,
and reads the volume they wrote. The agent image is built once by
`.modal/images.py` and referenced here by name.

Each job gets its own volume, `goble-job-<label>`, mounted at `/work`. A volume
has one writer at a time, so parallel labels cannot overwrite each other, and a
re-run on the same volume finds the previous checkout.
"""

from __future__ import annotations

import pathlib
import re
import secrets
import shlex
import time

import modal

APP_NAME = "sandboxes"
IMAGE_NAME = "goble-agent"
JOBS_DICT = "goble-jobs"
VOLUME_PREFIX = "goble-job-"
REPO_URL = "https://github.com/AdrianTuci1/goble.git"
MODEL_SECRET = "staticlabs-sandboxes-llm"
REQUIRED_SECRETS = ("goble-grok-auth", "goble-github-token", MODEL_SECRET)

WORK = "/work"
REPO = f"{WORK}/repo"
SANDBOX_DIR = f"{WORK}/.sandbox"
OUT = f"{WORK}/out"
LOG = f"{SANDBOX_DIR}/process.log"
AGENT_IN_SANDBOX = "/root/.grok/agents/goble-remote.md"
AGENT_IN_CONTROL = "/opt/goble-remote.md"

CPU = 8.0
MEMORY_MIB = 16384
JOB_TIMEOUT = 24 * 60 * 60
CONTROL_TIMEOUT = 15 * 60
DEFAULT_MODEL = "deepseek"
DEFAULT_MAX_TURNS = 240
DEFAULT_BASE = "terminal-first"

LABEL_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,40}$")

app = modal.App(APP_NAME)

# The control image holds the Modal SDK and the agent definition. Editing
# `.grok/agents/goble-remote.md` and redeploying is enough; the sandbox image
# does not change.
control_image = (
    modal.Image.debian_slim(python_version="3.12")
    .add_local_file(
        pathlib.Path(__file__).resolve().parent.parent
        / ".grok"
        / "agents"
        / "goble-remote.md",
        AGENT_IN_CONTROL,
        copy=True,
    )
)

jobs = modal.Dict.from_name(JOBS_DICT, create_if_missing=True)


def job_secrets() -> list[modal.Secret]:
    return [
        modal.Secret.from_name("goble-grok-auth"),
        modal.Secret.from_name("goble-github-token"),
        modal.Secret.from_name(MODEL_SECRET, required_keys=["DEEPSEEK_API_KEY"]),
    ]


def quote(value: str) -> str:
    return shlex.quote(value)


def entrypoint(label: str, model: str, base_branch: str, max_turns: int) -> str:
    """The sandbox's main process: wait for the task, run the agent, export.

    The sandbox starts before the control plane can write the task file, so the
    script waits for it. Output goes to a file on the volume, which is what
    `sandbox_logs` reads after the sandbox is gone.
    """
    task = f"{SANDBOX_DIR}/task-{label}.md"
    branch = f"modal/{label}"
    patch = f"{OUT}/{label}.patch"
    worktree_patch = f"{OUT}/{label}-worktree.patch"
    return f"""set -uo pipefail
mkdir -p {SANDBOX_DIR} {OUT}
bash /opt/bootstrap.sh >{LOG} 2>&1 || {{ echo "bootstrap failed" >>{LOG}; exit 1; }}

if [ ! -d {REPO}/.git ]; then
  git clone --branch {quote(base_branch)} {REPO_URL} {REPO} >>{LOG} 2>&1 || exit 1
fi
git -C {REPO} fetch origin {quote(base_branch)} >>{LOG} 2>&1 || exit 1
if git -C {REPO} show-ref --verify --quiet refs/heads/{quote(branch)}; then
  git -C {REPO} checkout {quote(branch)} >>{LOG} 2>&1 || exit 1
else
  git -C {REPO} checkout -B {quote(branch)} origin/{quote(base_branch)} >>{LOG} 2>&1 || exit 1
fi

for _ in $(seq 1 300); do [ -s {task} ] && break; sleep 1; done
[ -s {task} ] || {{ echo "task file never arrived" >>{LOG}; exit 78; }}

cd {REPO}
grok --agent {AGENT_IN_SANDBOX} --prompt-file {task} \\
  --model {quote(model)} --always-approve --no-auto-update \\
  --max-turns {max_turns} --output-format streaming-json >>{LOG} 2>&1
rc=$?

set +e
base=$(git merge-base HEAD origin/{quote(base_branch)})
if [ -n "$base" ]; then
  git format-patch --binary --stdout "$base"..HEAD > {patch}
  git log --oneline "$base"..HEAD > {OUT}/commits.txt
fi
git status --short > {OUT}/status.txt
git diff --binary > {worktree_patch}
[ -s {worktree_patch} ] || rm -f {worktree_patch}
{{
  echo "# {label}"
  echo
  echo "- exit code: $rc"
  echo "- model: {model}"
  echo "- branch: {branch}"
  echo "- base branch: {base_branch}"
  echo
  echo "## Commits"
  echo
  echo '```'
  cat {OUT}/commits.txt 2>/dev/null
  echo '```'
  echo
  echo "## Working tree"
  echo
  echo '```'
  cat {OUT}/status.txt
  echo '```'
}} > {OUT}/RESULT.md
exit $rc
"""


def read_text(volume: str, relative: str) -> str | None:
    """Read a file from a job's volume. Works without mounting it."""
    try:
        vol = modal.Volume.from_name(volume)
        try:
            vol.reload()
        except Exception:
            pass
        return b"".join(vol.read_file(relative)).decode("utf-8", "replace")
    except Exception:
        return None


def record_for(label: str) -> dict:
    record = jobs.get(label)
    if record is None:
        raise ValueError(f"no job {label!r}; `.modal/sandboxes list` shows what ran")
    return record


def save(record: dict) -> None:
    jobs[record["label"]] = record


def reconcile(record: dict) -> dict:
    """Ask Modal whether the sandbox is still running, and keep that state."""
    if record.get("state") not in ("running", "unknown"):
        return record
    try:
        sandbox = modal.Sandbox.from_id(record["sandbox_id"])
        code = sandbox.poll()
    except modal.exception.NotFoundError:
        record.update(state="failed", exit_code=None, reason="sandbox is gone")
        save(record)
        return record
    except modal.exception.Error as exc:
        record.update(state="failed", reason=f"{type(exc).__name__}: {exc}")
        save(record)
        return record

    if code is not None:
        record.update(
            state="finished" if code == 0 else "failed",
            exit_code=code,
            ended_at=now(),
        )
        save(record)
    return record


def now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


@app.function(
    image=control_image,
    secrets=job_secrets(),
    timeout=CONTROL_TIMEOUT,
)
def sandbox_start(
    label: str,
    prompt: str,
    model: str = DEFAULT_MODEL,
    base_branch: str = DEFAULT_BASE,
    max_turns: int = DEFAULT_MAX_TURNS,
) -> dict:
    """Create one sandbox and return at once. The agent runs as its main process."""
    if not LABEL_RE.match(label):
        return {
            "ok": False,
            "error": "label must be lowercase letters, digits, '.', '_' or '-', at most 41 chars",
        }
    if not prompt.strip():
        return {"ok": False, "error": "the prompt is empty"}
    if jobs.get(label):
        return {"ok": False, "error": f"label {label!r} already exists"}

    try:
        image = modal.Image.from_name(IMAGE_NAME)
    except modal.exception.NotFoundError:
        return {
            "ok": False,
            "error": f"image {IMAGE_NAME!r} is not published; run `modal run .modal/images.py`",
        }

    volume_name = VOLUME_PREFIX + label
    volume = modal.Volume.from_name(volume_name, create_if_missing=True)
    agent_text = pathlib.Path(AGENT_IN_CONTROL).read_text()

    try:
        sandbox = modal.Sandbox.create(
            "bash",
            "-c",
            entrypoint(label, model, base_branch, max_turns),
            app=modal.App.lookup(APP_NAME, create_if_missing=True),
            name=f"goble-{label}-{secrets.token_hex(3)}",
            image=image,
            env={"TRUSTED_REPO": REPO, "GROK_HOME": "/root/.grok"},
            secrets=job_secrets(),
            volumes={WORK: volume},
            cpu=CPU,
            memory=MEMORY_MIB,
            timeout=JOB_TIMEOUT,
            workdir=WORK,
            tags={"job": label, "model": model, "base": base_branch},
        )
    except modal.exception.Error as exc:
        return {"ok": False, "error": f"Modal refused the sandbox: {exc}"}

    sandbox.filesystem.make_directory(SANDBOX_DIR)
    sandbox.filesystem.make_directory(OUT)
    sandbox.filesystem.make_directory("/root/.grok/agents")
    sandbox.filesystem.write_text(agent_text, AGENT_IN_SANDBOX)
    # The task file is what the entrypoint waits for, so it is written last.
    sandbox.filesystem.write_text(prompt, f"{SANDBOX_DIR}/task-{label}.md")

    record = {
        "label": label,
        "sandbox_id": sandbox.object_id,
        "volume": volume_name,
        "image": IMAGE_NAME,
        "model": model,
        "base_branch": base_branch,
        "max_turns": max_turns,
        "started_at": now(),
        "state": "running",
        "prompt_file": f"{SANDBOX_DIR}/task-{label}.md",
    }
    save(record)
    return {"ok": True, **record}


@app.function(image=control_image, timeout=CONTROL_TIMEOUT)
def sandbox_status(label: str) -> dict:
    """Running, finished, or failed; plus commits and the transcript size."""
    try:
        record = reconcile(record_for(label))
    except ValueError as exc:
        return {"ok": False, "error": str(exc)}

    reflog = read_text(record["volume"], f"repo/.git/logs/refs/heads/modal/{label}") or ""
    commits = [
        line.split("commit: ", 1)[1].strip()
        for line in reflog.splitlines()
        if "commit: " in line
    ]
    transcript = read_text(record["volume"], ".sandbox/process.log") or ""

    return {
        "ok": True,
        "label": label,
        "state": record["state"],
        "exit_code": record.get("exit_code"),
        "reason": record.get("reason"),
        "sandbox_id": record["sandbox_id"],
        "volume": record["volume"],
        "started_at": record.get("started_at"),
        "ended_at": record.get("ended_at"),
        "commits": commits[-5:],
        "commit_count": len(commits),
        "transcript_bytes": len(transcript.encode()),
    }


@app.function(image=control_image, timeout=CONTROL_TIMEOUT)
def sandbox_logs(label: str, tail: int = 120) -> dict:
    """The raw streaming-json transcript, last `tail` lines."""
    try:
        record = record_for(label)
    except ValueError as exc:
        return {"ok": False, "error": str(exc)}

    text = read_text(record["volume"], ".sandbox/process.log")
    if text is None:
        return {"ok": False, "error": f"no transcript on volume {record['volume']}"}
    lines = text.splitlines()
    return {
        "ok": True,
        "label": label,
        "state": record["state"],
        "bytes": len(text.encode()),
        "lines": len(lines),
        "tail": lines[-max(1, tail) :],
    }


@app.function(image=control_image, timeout=CONTROL_TIMEOUT)
def sandbox_summary(label: str) -> dict:
    """`RESULT.md` and the agent's own `summary.md`, once they exist."""
    try:
        record = reconcile(record_for(label))
    except ValueError as exc:
        return {"ok": False, "error": str(exc)}

    result = read_text(record["volume"], "out/RESULT.md")
    summary = read_text(record["volume"], "out/summary.md")
    if result is None and summary is None:
        return {
            "ok": False,
            "label": label,
            "state": record["state"],
            "error": "nothing written yet; `sandbox_logs` shows progress",
        }
    return {
        "ok": True,
        "label": label,
        "state": record["state"],
        "result": result,
        "summary": summary,
    }


@app.function(image=control_image, timeout=CONTROL_TIMEOUT)
def sandbox_stop(label: str) -> dict:
    """Terminate the sandbox. Its volume stays."""
    try:
        record = record_for(label)
    except ValueError as exc:
        return {"ok": False, "error": str(exc)}

    try:
        modal.Sandbox.from_id(record["sandbox_id"]).terminate()
    except modal.exception.NotFoundError:
        record.update(state="finished", reason="sandbox had already exited")
        save(record)
        return {"ok": True, "label": label, "already_finished": True}
    except modal.exception.Error as exc:
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}

    record.update(state="stopped", ended_at=now(), reason="terminated on request")
    save(record)
    return {"ok": True, "label": label, "sandbox_id": record["sandbox_id"]}


@app.function(image=control_image, timeout=CONTROL_TIMEOUT)
def sandbox_list() -> dict:
    """Every job recorded by this app, newest first."""
    records = [record for record in jobs.values() if isinstance(record, dict)]
    records.sort(key=lambda r: r.get("started_at") or "", reverse=True)
    return {"ok": True, "jobs": records}


@app.function(image=control_image, timeout=CONTROL_TIMEOUT)
def sandbox_profiles() -> dict:
    """What `sandbox_start` accepts, so a caller does not guess."""
    return {
        "ok": True,
        "profiles": [
            {
                "name": "goble-remote",
                "description": "Rust workspace agent on branch modal/<label>",
                "image": IMAGE_NAME,
                "cpu": CPU,
                "memory_mib": MEMORY_MIB,
                "timeout_hours": JOB_TIMEOUT // 3600,
                "agent": AGENT_IN_SANDBOX,
                "model": DEFAULT_MODEL,
                "base_branch": DEFAULT_BASE,
                "secrets": list(REQUIRED_SECRETS),
            }
        ],
    }
