#!/bin/bash
set -euo pipefail

# Goblin Worker VPS installer
# Usage: ./install-goblin.sh [goblin-binary-path]

BINARY="${1:-/root/goble/target/debug/goblin}"
USER="${GOBLIN_USER:-root}"
GROUP="${GOBLIN_GROUP:-root}"

if [[ ! -f "$BINARY" ]]; then
    echo "error: goblin binary not found at $BINARY"
    echo "build with: cd /root/goble && cargo build -p goblin-worker"
    exit 1
fi

INSTALL_DIR="/opt/goblin"
DATA_DIR="/var/goblin"
LOG_DIR="/var/log/goblin"
PID_FILE="/var/run/goblin.pid"

mkdir -p "$INSTALL_DIR" "$DATA_DIR" "$LOG_DIR"

cp "$BINARY" "$INSTALL_DIR/goblin"
chmod +x "$INSTALL_DIR/goblin"

cat > /etc/systemd/system/goblin.service <<EOF
[Unit]
Description=Goblin Worker
After=network.target

[Service]
Type=simple
User=$USER
Group=$GROUP
WorkingDirectory=$DATA_DIR
ExecStart=$INSTALL_DIR/goblin --bind 0.0.0.0:8787 --workspace-root $DATA_DIR/workspaces --task-store $DATA_DIR/tasks.db --vault-path $DATA_DIR/vault.json
Restart=always
RestartSec=5
Environment="RUST_LOG=info"

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable goblin.service

# If already running, restart; otherwise start.
if systemctl is-active --quiet goblin; then
    systemctl restart goblin
else
    systemctl start goblin
fi

sleep 2

if systemctl is-active --quiet goblin; then
    echo "goblin worker is running"
    curl -s http://localhost:8787/health || true
    echo
else
    echo "error: goblin worker failed to start"
    systemctl status goblin --no-pager
    exit 1
fi
