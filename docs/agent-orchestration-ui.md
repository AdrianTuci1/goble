# Goble Agent Orchestration UI/UX Architecture

Date: 2026-08-01
Status: Draft / Implementation started

---

## 1. User journey overview

```mermaid
journey
    title User journey in Goble Desktop
    section Open app
      Launch app: 5: User
      See chat composer: 5: User
    section Configure model
      No model configured: 2: User
      Composer disabled: 2: User
      Navigate to Settings: 3: User
      Add provider + key + model: 4: User
    section First task
      Type vague request in chat: 5: User
      Assistant clarifies: 3: Assistant
      Pick quick reply or type: 4: User
    section Plan and run
      Assistant plans mission: 4: Assistant
      Shows reasoning steps in UI: 4: Assistant
      Deploys agents / MCPs / workflow: 4: Assistant
      Monitors execution: 4: User
    section Inspect agent
      Open Agents page: 5: User
      Click agent card: 5: User
      See drawer with logs + executions: 5: User
```

---

## 2. Mission lifecycle (vague → refined → done)

A mission is not always clear from the first message. The user can refine the goal at any point during the reasoning phase.

```mermaid
stateDiagram-v2
    [*] --> UserInput: user writes in chat

    UserInput --> Clarifying: request is vague
    UserInput --> Planning: request is clear
    Clarifying --> Planning: user answered ask_user
    Planning --> Refinement: assistant detects ambiguity
    Refinement --> Planning: user updates goal
    Planning --> Deploying: plan approved / auto-execute
    Deploying --> Running: agent/workflow on worker
    Running --> Monitoring: logs + events streaming
    Monitoring --> Refinement: user asks to change mission
    Monitoring --> Done: success
    Running --> Error: failure
    Error --> Refinement: user corrects mission
    Error --> Done: aborted
    Done --> [*]

    note right of Refinement
        Mission text can be edited by the user.
        Reasoning steps are re-evaluated from
        the updated goal.
    end note
```

---

## 3. UI state transitions

```mermaid
flowchart TD
    A[Open app] --> B{Model configured?}
    B -->|No| C[Settings page]
    C --> D[Save LLM settings]
    D --> E[Chat page]
    B -->|Yes| E

    E --> F[Normal composer]
    F --> G{User selects an agent?}
    G -->|Yes| H[Agent composer]
    G -->|No| I[User sends message]

    H --> I

    I --> J{Reasoning enabled?}
    J -->|Yes| K[Reasoning loop]
    J -->|No| L[Direct execution]

    K --> M{Need clarification?}
    M -->|Yes| N[AskUser composer]
    M -->|No| O{Plan ready?}
    O -->|No| K
    O -->|Yes| L

    N --> P[User answers]
    P --> K

    L --> Q[Tool execution]
    Q --> R[Agent / MCP / Workflow / Cron]
    R --> S[Worker runs externally]
    S --> T[Observability: logs + executions]
```

---

## 4. Composer variants

```mermaid
flowchart LR
    subgraph Composers
        N[Normal composer]
        NM[No model composer]
        A[Agent composer]
        QR[AskUser quick replies]
        OI[AskUser open input]
        F[Follow-up composer]
    end

    subgraph Triggers
        T1[No model configured]
        T2[User picks agent]
        T3[Assistant asks clarification]
        T4[Mission suspended]
    end

    T1 --> NM
    T2 --> A
    T3 --> QR
    T3 --> OI
    T4 --> F
```

### Composer UI mockups

| Variant | Placeholder | Buttons / extras |
|---------|-------------|------------------|
| Normal | `Ask anything...` | Send, model selector, agent picker |
| No model | `Add a model in Settings first` | Disabled input, link to Settings |
| Agent | `Message to agent {{name}}...` | Agent avatar, clear button, tools hint |
| AskUser quick | Question card shown | Up to 5 quick-reply buttons + "Other" |
| AskUser open | Question as hint | Normal input, focused |
| Follow-up | `Continue the task...` | Resume button, mission status badge |

---

## 5. Reasoning loop architecture

```mermaid
sequenceDiagram
    participant UI as Desktop UI
    participant TC as Tauri Commands
    participant CO as goble-core
    participant ST as SQLite Store
    participant LLM as LLM Provider
    participant WK as Worker
    participant MCP as MCP Server

    UI ->> TC: run_harness(chat_id, prompt, provider, model)
    TC ->> CO: run_turn()
    CO ->> ST: insert user message
    CO ->> ST: create mission
    CO ->> LLM: reasoning phase prompt
    LLM -->> CO: tool call: set_thinking_mode
    CO ->> CO: persist reasoning step
    LLM -->> CO: tool call: ask_user
    CO ->> ST: save pending_ask
    CO ->> UI: HarnessEvent::AskUser
    CO ->> UI: stream suspended

    UI ->> UI: render AskUser composer
    UI ->> TC: resume_harness(chat_id, response)
    TC ->> CO: resume_turn()
    CO ->> ST: insert user answer
    CO ->> ST: clear pending_ask
    CO ->> LLM: continue reasoning
    LLM -->> CO: tool call: execute
    CO ->> CO: switch to execution phase

    CO ->> LLM: execution phase prompt
    LLM -->> CO: tool call: create_agent
    CO ->> ST: insert agent
    LLM -->> CO: tool call: install_mcp_server
    CO ->> MCP: install / configure
    MCP -->> CO: summary
    LLM -->> CO: tool call: create_workflow
    CO ->> ST: insert workflow
    LLM -->> CO: tool call: deploy_agent
    CO ->> WK: DesktopMessage::RunAgent
    WK -->> CO: AgentStarted / AgentLog / AgentFinished
    CO ->> ST: insert execution
    CO ->> UI: HarnessEvent::AgentStarted/Log/Done
    LLM -->> CO: final answer
    CO ->> ST: insert assistant message
    CO ->> UI: HarnessEvent::AssistantDelta + Done
```

---

## 6. Mission refinement loop

```mermaid
sequenceDiagram
    participant U as User
    participant UI as Desktop UI
    participant CO as goble-core
    participant DB as SQLite Store

    U ->> UI: vague request
    UI ->> CO: run_turn(reasoning=true)
    CO ->> DB: create mission(goal=initial)
    CO ->> CO: reasoning step 1
    CO ->> U: AskUser: "What exactly do you need?"
    U ->> UI: answers / edits goal
    UI ->> CO: update_mission(new_goal)
    CO ->> DB: update mission goal
    CO ->> CO: reasoning step 2 (re-plan from new goal)
    CO ->> U: AskUser: "Which database?"
    U ->> UI: reply
    UI ->> CO: resume_turn()
    CO ->> CO: finalize plan
    CO ->> CO: execute
    CO ->> DB: mission status = done
```

---

## 7. Component architecture

```mermaid
flowchart TB
    subgraph Desktop["Goble Desktop (Tauri + React)"]
        Z[Zustand store]
        R[React UI]
        TC[Tauri commands]
    end

    subgraph Core["goble-core (Rust)"]
        H[Harness]
        RL[Reasoning loop]
        EX[Execution loop]
        ST[SQLite Store]
        MM[McpManager]
        LLM[LlmProvider]
        WC[Worker client]
    end

    subgraph Ext["External"]
        LLM_API[LLM API]
        MCP_S[MCP servers]
        GW[goblin-worker]
    end

    R --> Z
    Z --> TC
    TC --> H
    H --> RL
    H --> EX
    RL --> ST
    EX --> ST
    EX --> MM
    EX --> WC
    MM --> MCP_S
    LLM --> LLM_API
    WC --> GW
    H --> LLM
    ST --> H
```

---

## 8. Database entity model

```mermaid
erDiagram
    CHAT ||--o{ CHAT_MESSAGE : contains
    CHAT ||--o{ MISSION : has
    MISSION ||--o{ REASONING_STEP : produces
    MISSION ||--o| PENDING_ASK : suspends_with
    MISSION ||--o| WORKFLOW : creates
    MISSION ||--o{ EXECUTION : tracks
    AGENT ||--o{ WORKFLOW_STEP : used_by
    WORKFLOW ||--o{ WORKFLOW_STEP : contains
    AGENT ||--o{ EXECUTION : runs_as
    WORKER ||--o{ EXECUTION : runs_on
    MCP_SERVER ||--o{ MCP_TOOL : exposes

    CHAT {
        string id PK
        string title
        string provider
        string model
        string agent_id
        string worker_id
        string updated_at
    }

    CHAT_MESSAGE {
        string id PK
        string chat_id FK
        string role
        string content
        string created_at
    }

    MISSION {
        string id PK
        string chat_id FK
        string goal
        string status
        string plan
        string workflow_id FK
        string created_at
        string updated_at
    }

    REASONING_STEP {
        string id PK
        string mission_id FK
        int step_index
        string mode
        string content
        string decision
        string created_at
    }

    PENDING_ASK {
        string id PK
        string mission_id FK
        string question
        string quick_replies
        string created_at
    }

    AGENT {
        string id PK
        string name
        string prompt
        string tools
        string created_at
        string updated_at
    }

    WORKFLOW {
        string id PK
        string name
        string trigger
        string enabled
        string created_at
        string updated_at
    }

    WORKFLOW_STEP {
        string workflow_id FK
        int step_index
        string agent_id
        string prompt
    }

    EXECUTION {
        string id PK
        string agent_id FK
        string worker_id FK
        string status
        string trace
        string started_at
        string finished_at
    }

    WORKER {
        string id PK
        string name
        string url
        string status
    }

    MCP_SERVER {
        string id PK
        string name
        string source
        string enabled
    }

    MCP_TOOL {
        string server_id FK
        string name
        string schema
    }
```

---

## 9. Agents page: card → drawer

```mermaid
flowchart LR
    A[Agents page] --> B[Agent cards grid]
    B --> C{Click card}
    C --> D[Drawer opens]
    D --> E[Details tab]
    D --> F[Workflows tab]
    D --> G[Executions tab]
    D --> H[Live logs tab]
    D --> I[Run tab]
    I --> J[Manual input]
    J --> K[Trigger agent]
    K --> L[Execution appears in Executions tab]
    L --> H
```

---

## 10. Questions and answers (current design intent)

| Question | Answer |
|----------|--------|
| Open GUI as user, no model configured, write in chat | The composer is disabled or the user is redirected to user settings to add a model. |
| Can chat create an agent, connect MCPs, set cron, call that agent externally | Yes. The local chat is an orchestration surface. Agents, MCPs, workflows with cron triggers, and external runs on workers are all available as tools to the local assistant. |
| Can the agent finish complex workflows | Yes, through the reasoning loop and mission tracking. The assistant can plan, ask the user when information is missing, create agents/MCPs/workflows, deploy them, and resume execution later. |
| Do we have observability over it | Yes. Every action is logged: reasoning steps, tool calls, worker runs, executions. The Agents page shows a per-agent drawer with details, workflows, executions and logs. |
| Multiple composer variants / user follow-up | The composer can show quick-reply variants for `ask_user` or switch to a follow-up input when the assistant needs the user to continue a suspended turn. |
| Mission can be vague and refined | Yes. The mission goal is stored separately and can be updated by the user or the assistant during the reasoning phase. Each update triggers a re-plan. |

---

## 11. Composer variants detailed

| Mode | UI |
|------|----|
| Normal | Text input, send button, model selector. |
| No model configured | Input disabled, placeholder `Add a model in Settings`, link to Settings. |
| Agent selected | Text input, placeholder `Message to <agent>`, agent avatar, clear button. |
| AskUser with quick replies | Input hidden, question card, up to N quick-reply buttons + "Other" button that switches to text input. |
| AskUser open | Text input focused, prompt prefixed with the question context. |
| Follow-up after suspension | Input enabled, label `Continue the task...`. |

---

## 12. Core reasoning flow

```mermaid
flowchart TD
    A[User message] --> B[Create or load mission]
    B --> C{Reasoning enabled?}
    C -->|No| J[Direct execution]
    C -->|Yes| D[Run reasoning phase]
    D --> E{LLM decision}
    E -->|continue_thinking| D
    E -->|set_thinking_mode| D
    E -->|ask_user| F[Persist pending_ask]
    F --> G[Emit AskUser event]
    G --> H[Stream suspended]
    H --> I[User answers]
    I --> D
    E -->|execute| J
    J --> K[Execution phase]
    K --> L{Tool call}
    L -->|create_agent| M[(SQLite)]
    L -->|install_mcp_server| N[MCP server]
    L -->|create_workflow| M
    L -->|deploy_agent| O[Worker]
    L -->|call_mcp_tool| N
    L -->|final answer| P[Assistant message]
    M --> K
    N --> K
    O --> K
    K --> P
    P --> Q[Done]
```

---

## 13. Next implementation steps

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

---

## 14. Files touched

- `crates/goble-core/src/store.rs` — added `missions`, `reasoning_steps`, `pending_asks` tables and CRUD.
- `crates/goble-core/src/harness.rs` — exposed `with_reasoning`, `run_turn`/`resume_turn` delegate to `reasoning.rs`.
- `crates/goble-core/src/reasoning.rs` — new reasoning loop, `ask_user` suspend/resume, mission tracking.
- `crates/goble-core/src/lib.rs` — added `pub mod reasoning`.
- `crates/goble-desktop/src/pages/AgentsPage.tsx` — agent drawer (details, workflows, executions).

---

## 15. How to view the diagrams

This document uses **Mermaid** diagrams. You can view them in:
- GitHub / GitLab (rendered automatically in markdown previews)
- Obsidian with the Mermaid plugin enabled
- Any Mermaid live editor (https://mermaid.live)
- VS Code with the `Markdown Preview Mermaid Support` extension

To render all diagrams locally, copy the `.md` file into any Mermaid-compatible viewer.

