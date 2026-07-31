# Goble Agent Orchestration UI/UX Architecture

Date: 2026-08-01
Status: Draft / Implementation started

## 1. Questions and answers (current design intent)

| Question | Answer |
|----------|--------|
| Open GUI as user, no model configured, write in chat | The composer is disabled or the user is redirected to user settings to add a model. |
| Can chat create an agent, connect MCPs, set cron, call that agent externally | Yes. The local chat is an orchestration surface. Agents, MCPs, workflows with cron triggers, and external runs on workers are all available as tools to the local assistant. |
| Can the agent finish complex workflows | Yes, through the reasoning loop and mission tracking. The assistant can plan, ask the user when information is missing, create agents/MCPs/workflows, deploy them, and resume execution later. |
| Do we have observability over it | Yes. Every action is logged: reasoning steps, tool calls, worker runs, executions. The Agents page shows a per-agent drawer with details, workflows, executions and logs. |
| Multiple composer variants / user follow-up | The composer can show quick-reply variants for `ask_user` or switch to a follow-up input when the assistant needs the user to continue a suspended turn. |

## 2. UI surfaces

### 2.1 Chat (normal mode)
- User types a task.
- If no model is configured for the selected provider, the composer is disabled and the placeholder says: `Add a model in Settings first`.
- Settings can be reached from a link/button in the composer or from the sidebar.

### 2.2 Chat with agent selected
- User can pick an agent from the Agents page or a dropdown in the chat header.
- The composer changes mode: placeholder becomes `Message to agent <name>`, system prompt is the agent spec, tools are filtered by the agent's tools.
- If the agent requires missing information, the composer either renders quick-reply buttons or asks the user to write a follow-up.

### 2.3 Agents page
- Grid of agent cards.
- Clicking a card opens a side drawer with:
  - **Details**: id, prompt, tools.
  - **Workflows**: workflows that use this agent.
  - **Executions**: recent runs, status, worker, timestamp.
  - **Live logs**: streamed when the agent is running.
- The drawer is also the custom render surface for the agent.

### 2.4 Settings page
- LLM provider cards (OpenAI, Anthropic, local, custom).
- Model selection.
- API key / base URL.
- Global default model.

## 3. Core concepts

```text
User -> Chat / Agent drawer
        |
        v
Goble Desktop (Tauri + React)
  - Composer variants
  - Zustand store: chats, agents, workflows, executions, missions, pending asks
  - Tauri commands: run_harness, resume_harness, list_agents, ...
        |
        v
goble-core (Rust)
  - Store (SQLite): missions, reasoning_steps, pending_asks, chat_messages, agents, workflows, executions, mcp_servers, workers, vault
  - Harness: reasoning loop + execution loop
  - LlmProvider: OpenAI / Anthropic / local
  - McpManager: discover/call MCP tools
  - Worker client: deploy agents to workers
        |
        +---> LLM provider (API)
        +---> MCP servers (local / remote)
        +---> Worker (goblin-worker): runs agents, returns logs, execution traces
```

## 4. Entities

### Mission
A user goal that may span multiple turns and tools.

```json
{
  "id": "uuid",
  "chat_id": "uuid",
  "goal": "build a daily report workflow",
  "status": "clarifying | planning | deploying | running | done | error",
  "plan": "...",
  "workflow_id": "...",
  "reasoning_steps": [...],
  "pending_ask": null | { "id", "question", "quick_replies" }
}
```

### ReasoningStep
One thinking step produced by the LLM while planning.

```json
{
  "step": 0,
  "mode": "direct | contemplating | ruminating | baking | reflecting | verifying | debugging | synthesizing | planning",
  "content": "...",
  "decision": "continue | execute | ask_user | done",
  "tool_calls": [...]
}
```

### PendingAsk
Suspended turn waiting for user input.

```json
{
  "id": "uuid",
  "question": "Which database should I query?",
  "quick_replies": ["postgres", "mysql"]
}
```

### Execution
A concrete run of an agent on a worker.

```json
{
  "id": "trace-id",
  "agent_id": "...",
  "worker_id": "...",
  "status": "running | success | error",
  "trace": { "steps": [...], "output": "..." },
  "started_at": "...",
  "finished_at": "..."
}
```

## 5. Reasoning loop (goble-core)

The harness is now split into two phases:

1. **Reasoning phase** (only enabled when the user explicitly turns on complex task mode or selects an agent in chat)
   - The LLM receives a system prompt describing the thinking mode.
   - It can call: `set_thinking_mode`, `continue_thinking`, `execute`, `ask_user`, `create_mission`, `update_mission`.
   - Each step is persisted as a `reasoning_step` row.
   - When the LLM calls `execute`, the harness moves to the execution phase.
   - When the LLM calls `ask_user`, the harness emits `HarnessEvent::AskUser`, persists a `pending_ask`, and stops the stream.

2. **Execution phase**
   - The LLM receives the reasoning summary + execution system prompt.
   - It can call all harness tools: `create_agent`, `install_mcp_server`, `create_workflow`, `schedule_workflow`, `deploy_agent`, `call_mcp_tool`, etc.
   - Tool results are saved as chat messages so the LLM can iterate.
   - The loop stops when the LLM produces a final answer without tool calls.

3. **Resume**
   - UI calls `resume_harness(chat_id, response)`.
   - The pending ask is resolved, the user's answer is inserted as a chat message, and the harness re-runs from reasoning/execution.

## 6. Composer variants

| Mode | UI |
|------|----|
| Normal | Text input, send button, model selector. |
| No model configured | Input disabled, placeholder `Add a model in Settings`, link to settings. |
| Agent selected | Text input, placeholder `Message to <agent>`, agent avatar, clear button. |
| AskUser with quick replies | Input hidden, question card, up to N quick-reply buttons + "Other" button that switches to text input. |
| AskUser open | Text input focused, prompt prefixed with the question context. |
| Follow-up after suspension | Input enabled, label `Continue the task...`. |

## 7. Observability

- **Agent drawer**: details, workflows, executions, live logs from worker.
- **Chat area**: tool call start/finish events, reasoning started/done events, mission status updates, error messages.
- **Worker logs**: global log panel (`worker_logs` command), `agent:log` events, `agent:started` / `agent:finished` events.
- **Store**: all reasoning steps, missions, executions, pending asks are persisted in SQLite.

## 8. New Tauri commands to expose

```rust
#[tauri::command]
fn resume_harness(
    req: ResumeHarnessRequest, // chat_id, response
    state: tauri::State<'_, Arc<DesktopState>>,
) -> Result<(), String>;

#[tauri::command]
fn list_missions(
    state: tauri::State<'_, Arc<DesktopState>>,
) -> Result<Vec<MissionInfo>, String>;

#[tauri::command]
fn get_mission(
    mission_id: String,
    state: tauri::State<'_, Arc<DesktopState>>,
) -> Result<MissionInfo, String>;
```

## 9. Next implementation steps

1. **Desktop backend**:
   - Add `resume_harness` command (reuse `run_harness` with `resume_turn`).
   - Add `list_missions` / `get_mission` commands.
   - Wire `HarnessEvent::AskUser`, `ReasoningStarted`, `ReasoningDone`, `MissionUpdated` to the frontend.

2. **Desktop frontend store**:
   - Add `missions`, `pendingAsk` to Zustand.
   - Handle `harness:event` AskUser by switching the composer to quick-reply or follow-up mode.

3. **AgentsPage**:
   - Keep the agent drawer that is already started.
   - Add `Run` tab with manual input + trigger.
   - Add real-time log viewer for the selected agent.

4. **ChatArea**:
   - Disable composer when no model is configured and show a settings link.
   - Add agent selector in chat header.
   - Render `AskUser` events as cards with quick replies.
   - Add `Resume` button when a suspended ask is present.

5. **SettingsPage**:
   - Ensure a global default model is selected; chats inherit it if not set per chat.

## 10. Decisions made during this session

- Reasoning mode is **opt-in** for the current implementation to keep existing chat and e2e tests stable. It is enabled via `Harness::with_reasoning(true)` and triggered by orchestration keywords or an existing mission/pending ask.
- `ask_user` is a **blocking** suspension. The mission persists; the frontend resumes with `resume_turn`.
- The agent drawer is the primary observability surface, not the chat, but chat also shows high-level events.
- Composer variants are the frontend's responsibility based on the active mission state and pending ask.

---

## Files touched in this session

- `crates/goble-core/src/store.rs` — added `missions`, `reasoning_steps`, `pending_asks` tables and CRUD.
- `crates/goble-core/src/harness.rs` — exposed `with_reasoning`, `run_turn`/`resume_turn` delegate to `reasoning.rs`.
- `crates/goble-core/src/reasoning.rs` — new reasoning loop, `ask_user` suspend/resume, mission tracking.
- `crates/goble-core/src/lib.rs` — added `pub mod reasoning`.
- `crates/goble-desktop/src/pages/AgentsPage.tsx` — agent drawer (details, workflows, executions).

