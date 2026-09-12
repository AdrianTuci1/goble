use goble_core::harness::ToolCallStatus;

/// How a tool call's body is currently displayed: the three-state fold the
/// transcript uses, matching grok-build's per-entry `DisplayMode`. The state is
/// owned by the app (next to the reasoning rows' expand map) so it survives the
/// per-frame element rebuild.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToolDisplayMode {
    /// The header alone — the quiet form every tool call opens in.
    Collapsed,
    /// The head and tail of the body, with the middle elided.
    Truncated,
    /// The whole body.
    #[default]
    Expanded,
}

impl ToolDisplayMode {
    /// The next mode in the fold cycle a toggle advances through:
    /// `Collapsed -> Truncated -> Expanded -> Collapsed`.
    pub fn next(self) -> Self {
        match self {
            Self::Collapsed => Self::Truncated,
            Self::Truncated => Self::Expanded,
            Self::Expanded => Self::Collapsed,
        }
    }
}

/// The app-owned fold key for a call: its persisted id, or a stable fallback
/// (`<name>#<index>`) for a row an older build wrote without one. `index` is the
/// call's position in its message's `tool_calls`.
pub fn tool_fold_key(call: &ToolCall, index: usize) -> String {
    if call.id.is_empty() {
        format!("{}#{}", call.name, index)
    } else {
        call.id.clone()
    }
}

/// A single tool invocation recorded on an assistant message. `arguments` is the
/// JSON arguments the tool was called with, rendered alongside the name so the
/// user can see what the agent actually invoked. `status` and `result` are the
/// persisted lifecycle carrier: a re-read renders the terminal state, and an
/// in-flight `running` call is visible while it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub status: ToolCallStatus,
    pub result: Option<String>,
}

impl ToolCall {
    /// The fold a call opens in when the app has no state for it yet. Every
    /// tool shape starts folded — grok-build's tool blocks all default to
    /// `DisplayMode::Collapsed`, so a folded row is what you see first.
    pub fn default_display_mode(&self) -> ToolDisplayMode {
        ToolDisplayMode::Collapsed
    }

    /// Parse tool-call metadata from the harness-produced `tool_calls` JSON column
    /// into renderable calls. Malformed or unknown JSON yields an empty list
    /// rather than failing the whole transcript. Rows written by older builds
    /// carry only `name`/`arguments`; `id`, `status` and `result` default.
    pub fn from_llm_json(json: &str) -> Vec<ToolCall> {
        #[derive(serde::Deserialize)]
        struct Raw {
            #[serde(default)]
            id: String,
            name: String,
            #[serde(default)]
            arguments: serde_json::Value,
            #[serde(default)]
            status: ToolCallStatus,
            #[serde(default)]
            result: Option<String>,
        }
        serde_json::from_str::<Vec<Raw>>(json)
            .unwrap_or_default()
            .into_iter()
            .map(|raw| ToolCall {
                id: raw.id,
                name: raw.name,
                arguments: serde_json::to_string(&raw.arguments).unwrap_or_default(),
                status: raw.status,
                result: raw.result,
            })
            .collect()
    }
}
