#!/bin/bash
# Per-container setup for detached Goble agent runs on Modal.
# Credentials, git identity, and the trusted-folder grant the headless agent
# needs before it will edit a repository it has not seen before.
set -euo pipefail

mkdir -p /root/.grok /work/logs /work/out
if [ -n "${GROK_AUTH_JSON:-}" ]; then
  printf '%s' "$GROK_AUTH_JSON" > /root/.grok/auth.json
  chmod 600 /root/.grok/auth.json
fi

git config --global user.name "${GIT_USER_NAME:-Adrian Tucicovenco}"
git config --global user.email "${GIT_USER_EMAIL:-adrian.tucicovenco@gmail.com}"
git config --global init.defaultBranch main
git config --global --add safe.directory '*'

# Headless runs still consult the folder-trust store. Grant the job's checkout
# explicitly; the path is passed in by the caller.
if [ -n "${TRUSTED_REPO:-}" ]; then
  printf '\n[folders."%s"]\ntrusted = true\ndecided_at = 0\n' "$TRUSTED_REPO" \
    >> /root/.grok/trusted_folders.toml
fi

echo "== $(nproc) cpu, $(awk '/MemTotal/ {printf "%.0f", $2/1048576}' /proc/meminfo) GiB ram, grok $(grok version | head -1) =="
rustc --version
cargo --version
