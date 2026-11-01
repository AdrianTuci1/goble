# Agent image for detached Goble runs.
#
# Built once and published as `goble-agent` by `.modal/run.py::build`; jobs
# refer to it by name. Rebuild with `modal run .modal/run.py::build` after
# changing anything in this file.
FROM python:3.12-slim-bookworm

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH=/root/.grok/bin:/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin
ENV CARGO_TERM_COLOR=always

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        build-essential \
        pkg-config \
        libssl-dev \
        git \
        ripgrep \
        zsh \
        tmux \
        jq \
        less \
        openssh-client \
        nodejs \
        npm \
        file \
        unzip \
        xz-utils \
        libsqlite3-dev \
        libxkbcommon-dev \
        libwayland-dev \
        libx11-dev \
        libxi-dev \
        libxcursor-dev \
        libxrandr-dev \
        libxinerama-dev \
        libgl1-mesa-dev \
        libegl1-mesa-dev \
    && rm -rf /var/lib/apt/lists/*

# The Grok CLI used by every job.
RUN curl -fsSL https://x.ai/cli/install.sh | bash \
    && grok version

# Rust stable; the workspace pins edition 2021 and rust-version 1.86.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustc --version \
    && cargo --version

COPY grok-config.toml /root/.grok/config.toml
COPY harness/grok-workflow /usr/local/bin/grok-workflow
RUN chmod +x /usr/local/bin/grok-workflow \
    && git config --system user.name "Adrian Tucicovenco" \
    && git config --system user.email "adrian.tucicovenco@gmail.com" \
    && git config --system init.defaultBranch main \
    && git config --system --add safe.directory '*'

WORKDIR /work
