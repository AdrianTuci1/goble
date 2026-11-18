#!/usr/bin/env bash
# Live smoke test for the gateway's remote-execution API (docs/gateway-api.md).
#
# Starts a throwaway gateway with a generated PIN and worker token, connects the
# `fake_worker` example as a worker, and drives the whole control plane with
# curl: PIN -> token, session create/reuse, run dispatch, event log, SSE replay
# and keepalive, one-run-per-session 409, and cancel.
#
# Requirements: cargo, curl, jq.
# The gateway listens on 127.0.0.1:18788 by default; override with
# GATEWAY_SMOKE_BIND. Nothing outside $TMPDIR is touched.
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

BIND=${GATEWAY_SMOKE_BIND:-127.0.0.1:18788}
PIN=${GATEWAY_SMOKE_PIN:-424242}
WORKER_TOKEN="gateway-smoke-token"
WORKER_ID="smoke-worker"
SESSION_NAME="smoke-$(date +%s)"

for tool in cargo curl jq; do
  command -v "$tool" >/dev/null || {
    echo "gateway-smoke: missing $tool" >&2
    exit 1
  }
done

tmp=$(mktemp -d)
gateway_pid=""
worker_pid=""
sse_pid=""
cleanup() {
  for pid in "$sse_pid" "$worker_pid" "$gateway_pid"; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  done
  for pid in "$sse_pid" "$worker_pid" "$gateway_pid"; do
    [ -n "$pid" ] && wait "$pid" 2>/dev/null || true
  done
  rm -rf "$tmp"
}
trap cleanup EXIT

fail() {
  echo "gateway-smoke: FAIL: $*" >&2
  echo '--- gateway log ---' >&2
  sed -n '1,120p' "$tmp/gateway.log" >&2 || true
  echo '--- worker log ---' >&2
  sed -n '1,120p' "$tmp/worker.log" >&2 || true
  exit 1
}

step() { printf '\n== %s\n' "$*"; }

step "build"
cargo build -q -p goble-gateway --example fake_worker

printf '%s' "$PIN" >"$tmp/pin"
printf '%s' "$WORKER_TOKEN" >"$tmp/worker-token"

step "start gateway on $BIND"
target/debug/goble-gateway \
  --bind "$BIND" \
  --pin-file "$tmp/pin" \
  --worker-token-file "$tmp/worker-token" \
  --data-dir "$tmp/data" >"$tmp/gateway.log" 2>&1 &
gateway_pid=$!

for _ in $(seq 1 100); do
  curl -fsS "http://$BIND/health" >/dev/null 2>&1 && break
  sleep 0.1
done
curl -fsS "http://$BIND/health" >/dev/null || fail "gateway did not start"
echo "health: $(curl -fsS "http://$BIND/health")"

step "pair"
TOKEN=$(curl -fsS -X POST "http://$BIND/api/v1/session" \
  -H 'content-type: application/json' \
  -d "{\"pin\":\"$PIN\"}" | jq -r .token)
[ -n "$TOKEN" ] && [ "$TOKEN" != "null" ] || fail "pairing did not return a token"
echo "token: ${TOKEN:0:8}…"

auth_get() { curl -fsS -H "authorization: Bearer $TOKEN" "http://$BIND$1"; }
auth_post() { curl -fsS -X POST -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d "$2" "http://$BIND$1"; }
auth_post_code() {
  curl -s -o "$tmp/last-body" -w '%{http_code}' -X POST \
    -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' \
    -d "$2" "http://$BIND$1"
}

step "unauthorized control-plane request"
code=$(curl -s -o "$tmp/unauthorized-body" -w '%{http_code}' "http://$BIND/api/v1/sessions")
[ "$code" = "401" ] || fail "GET /api/v1/sessions without a token returned $code"
jq -e '.error == "login required"' "$tmp/unauthorized-body" >/dev/null || fail "unexpected 401 body"
echo "401: $(cat "$tmp/unauthorized-body")"

step "start the fake worker"
target/debug/examples/fake_worker \
  "ws://$BIND/ws/worker?id=$WORKER_ID" "$WORKER_TOKEN" >"$tmp/worker.log" 2>&1 &
worker_pid=$!

for _ in $(seq 1 100); do
  if auth_get /api/v1/workers | jq -e --arg id "$WORKER_ID" \
    'any(.workers[]; .id == $id)' >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done
auth_get /api/v1/workers | jq -e --arg id "$WORKER_ID" 'any(.workers[]; .id == $id)' >/dev/null ||
  fail "fake worker never connected"

step "create a session (create + reuse)"
session=$(auth_post /api/v1/sessions \
  "{\"name\":\"$SESSION_NAME\",\"worker\":\"$WORKER_ID\"}")
SESSION_ID=$(echo "$session" | jq -r .id)
[ "$(echo "$session" | jq -r .reused)" = "false" ] || fail "first create did not report reused=false"
[ "$(echo "$session" | jq -r .worker_id)" = "$WORKER_ID" ] || fail "session bound to the wrong worker"
echo "session: $SESSION_ID (thread $(echo "$session" | jq -r .thread_id))"

reused=$(auth_post /api/v1/sessions "{\"name\":\"$SESSION_NAME\"}")
[ "$(echo "$reused" | jq -r .reused)" = "true" ] || fail "second create did not reuse"
[ "$(echo "$reused" | jq -r .id)" = "$SESSION_ID" ] || fail "reuse returned a different session"

step "dispatch a run and settle it"
run=$(auth_post "/api/v1/sessions/$SESSION_ID/runs" \
  '{"task":"say smoke ok","timeout_s":30}')
RUN_ID=$(echo "$run" | jq -r .id)
[ "$(echo "$run" | jq -r .status)" = "running" ] || fail "run did not start: $run"
echo "run: $RUN_ID"

for _ in $(seq 1 100); do
  status=$(auth_get "/api/v1/runs/$RUN_ID" | jq -r .status)
  [ "$status" = "succeeded" ] && break
  sleep 0.1
done
[ "$status" = "succeeded" ] || fail "run never succeeded (last status $status)"
final_text=$(auth_get "/api/v1/runs/$RUN_ID" | jq -r .final_text)
[ "$final_text" = "smoke ok" ] || fail "unexpected final_text: $final_text"
echo "run settled: status=succeeded final_text=$final_text"

events=$(auth_get "/api/v1/runs/$RUN_ID/events")
echo "$events" | jq -e '.next_index > 0' >/dev/null || fail "event log is empty"
echo "$events" | jq -e 'any(.events[]; .type == "text")' >/dev/null || fail "no text event"
echo "$events" | jq -e 'any(.events[]; .type == "tool_result" and .status == "completed")' >/dev/null ||
  fail "no completed tool_result event"
echo "$events" | jq -e '.events[-1].type == "final" and .events[-1].reason.kind == "completed"' >/dev/null ||
  fail "event log does not end with a completed final event"
echo "events: $(echo "$events" | jq -r .next_index) stored (indexes 0..$(( $(echo "$events" | jq -r .next_index) - 1 )))"

step "SSE replay from_index + keepalive, 409 while running, cancel"
slow=$(auth_post "/api/v1/sessions/$SESSION_ID/runs" \
  '{"task":"slow turn","timeout_s":60}')
SLOW_ID=$(echo "$slow" | jq -r .id)
[ "$(echo "$slow" | jq -r .status)" = "running" ] || fail "slow run did not start"
curl -sN --max-time 5 -H "authorization: Bearer $TOKEN" \
  "http://$BIND/api/v1/runs/$SLOW_ID/stream?from_index=0" >"$tmp/sse.txt" &
sse_pid=$!
for _ in $(seq 1 100); do
  grep -q ': keepalive' "$tmp/sse.txt" 2>/dev/null && break
  sleep 0.1
done
grep -q 'data: ' "$tmp/sse.txt" || fail "SSE stream delivered no data events"
grep -q ': keepalive' "$tmp/sse.txt" || fail "SSE stream sent no keepalive"
echo "sse: $(grep -c '^data: ' "$tmp/sse.txt") data event(s) + keepalive"

busy_code=$(auth_post_code "/api/v1/sessions/$SESSION_ID/runs" '{"task":"second"}')
[ "$busy_code" = "409" ] || fail "second run while busy returned $busy_code (expected 409)"
jq -e '.error == "session_busy"' "$tmp/last-body" >/dev/null || fail "unexpected 409 body"
echo "409: $(cat "$tmp/last-body")"

cancelled=$(auth_post "/api/v1/sessions/$SESSION_ID/cancel" '{}')
[ "$(echo "$cancelled" | jq -r .ok)" = "true" ] || fail "cancel failed: $cancelled"
[ "$(echo "$cancelled" | jq -r .run_id)" = "$SLOW_ID" ] || fail "cancel named the wrong run"
wait "$sse_pid" 2>/dev/null || true
sse_pid=""
grep -q '"reason":{"kind":"cancelled"}' "$tmp/sse.txt" || fail "SSE did not deliver the cancelled final event"
[ "$(auth_get "/api/v1/runs/$SLOW_ID" | jq -r .status)" = "cancelled" ] || fail "run is not cancelled"
echo "cancel: run_id=$(echo "$cancelled" | jq -r .run_id) stream closed with a cancelled final event"

step "session list + persisted bindings"
auth_get /api/v1/sessions | jq -e --arg id "$SESSION_ID" 'any(.sessions[]; .id == $id)' >/dev/null ||
  fail "session is missing from the list"
[ -f "$tmp/data/sessions.json" ] || fail "sessions.json was not written"
[ "$(auth_get "/api/v1/sessions/$SESSION_ID" | jq -r .status)" = "idle" ] || fail "session is not idle"
echo "bindings: $(jq -r '.sessions | length' "$tmp/data/sessions.json") persisted session(s)"

printf '\ngateway smoke test passed: auth, sessions, dispatch, events, SSE, 409, cancel, persistence\n'
