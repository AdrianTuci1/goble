//! Model-facing types for a harness turn.
//!
//! This crate is the `types` layer of a `types <- protocol <- runtime` split
//! (mirroring the tool-protocol split used in the reference architecture). It
//! owns only model-shaped data: ids, grants, interaction hints, schedules. No
//! transport, no execution — dependencies point one way, out of this crate.

use serde::{Deserialize, Serialize};

/// Identifies a harness implementation (internal, or an external CLI).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HarnessId(pub String);

impl HarnessId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for HarnessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for HarnessId {
    fn default() -> Self {
        Self::new("internal")
    }
}

/// A project is the directory / mounted workspace work happens in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectId(pub String);

impl ProjectId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// A session is one conversation / run on a project.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// A medium is *where* the work runs: local, VM, remote (xrdp), browser, container.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediumId(pub String);

impl MediumId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediumKind {
    Local,
    Vm,
    RemoteXrdp,
    Browser,
    Container,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionChannel {
    HeadlessShell,
    GuiScreen,
    Xrdp,
    Cdp,
    Voice,
}

/// How to reach a remote RDP desktop the harness asks the host to open when it
/// hands off to an interactive GUI session. This is the *model-shape* of the
/// handoff request; the runtime layer [`ScreenCapturer`]/[`ScreenController`]
/// pair that actually streams and drives it lives in `goble-screen-sdk`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteScreenConfig {
    /// Hostname or IP of the remote (e.g. `"vm.example.com"`).
    pub host: String,
    /// RDP port (default 3389).
    pub port: u16,
    /// RDP account username.
    pub username: String,
    /// RDP account password.
    pub password: String,
    /// Requested desktop width in pixels.
    pub width: u16,
    /// Requested desktop height in pixels.
    pub height: u16,
}

impl RemoteScreenConfig {
    /// A config for the default RDP port and a 1280x720 desktop.
    pub fn new(
        host: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port: 3389,
            username: username.into(),
            password: password.into(),
            width: 1280,
            height: 720,
        }
    }
}

/// Hint the harness sees so it can adapt its reporting. Voice is the *reporting*
/// channel, not the work channel: the agent works silently, then speaks a short
/// summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionHint {
    Text,
    Voice { short_answers: bool },
    Auto,
}

/// Shepherd-style grant: the task signature is the permission surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Grant {
    pub repo: String,
    pub mode: GrantMode,
}

impl Grant {
    pub fn read_only(repo: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            mode: GrantMode::ReadOnly,
        }
    }

    pub fn read_write(repo: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            mode: GrantMode::ReadWrite,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxLevel {
    None,
    AllowList,
    Hardened,
}

/// What a harness declares it can do. Lets the host choose a harness for a
/// given interaction (voice, screen) and sandbox requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCapabilities {
    pub voice: bool,
    pub screen: bool,
    pub sandbox_level: SandboxLevel,
    pub tools: Vec<String>,
    /// Whether the harness can snapshot/restore its own execution state (see
    /// [`HarnessRuntime::snapshot`] / [`HarnessRuntime::restore`]). Reversibility
    /// is available to every harness at the transcript level either way; this
    /// flag marks the optional environment-level restore.
    #[serde(default)]
    pub reversible: bool,
}

impl HarnessCapabilities {
    pub fn internal() -> Self {
        Self {
            voice: false,
            screen: false,
            sandbox_level: SandboxLevel::AllowList,
            tools: Vec::new(),
            reversible: false,
        }
    }
}

/// An opaque snapshot of a harness's execution state, taken at a point so a
/// session can be rewound there without re-executing. The host carries it
/// opaquely (e.g. over the CLI wire or into persistence); only the harness that
/// produced it knows how to interpret `data`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessSnapshot {
    /// A harness-chosen tag describing the snapshot format.
    pub kind: String,
    /// Harness-specific serialized state.
    pub data: serde_json::Value,
}

impl HarnessSnapshot {
    pub fn new(kind: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            data,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Schedule {
    Manual,
    Cron { expression: String },
    Http { path: String },
    Heartbeat { interval_seconds: u64 },
}

/// The request handed to a harness. One harness run = one `HarnessTurn`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessTurn {
    pub harness_id: HarnessId,
    pub project_id: ProjectId,
    pub session_id: SessionId,
    pub medium_id: MediumId,
    pub goal: String,
    pub context: Vec<ChatMessage>,
    pub permissions: Vec<Grant>,
    pub interaction: InteractionHint,
    pub schedule: Option<Schedule>,
}

impl HarnessTurn {
    pub fn new(harness_id: HarnessId, session_id: SessionId, goal: impl Into<String>) -> Self {
        Self {
            harness_id,
            project_id: ProjectId::new("default"),
            session_id,
            medium_id: MediumId::new("local"),
            goal: goal.into(),
            context: Vec::new(),
            permissions: Vec::new(),
            interaction: InteractionHint::Auto,
            schedule: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_turn_roundtrip() {
        let mut turn = HarnessTurn::new(HarnessId::new("cli"), SessionId::new("s1"), "fix bug");
        turn.project_id = ProjectId::new("p1");
        turn.medium_id = MediumId::new("remote");
        turn.permissions = vec![Grant::read_write("backend/")];
        turn.interaction = InteractionHint::Voice { short_answers: true };
        turn.context.push(ChatMessage::new(ChatRole::User, "please fix the tests"));
        turn.schedule = Some(Schedule::Cron {
            expression: "0 8 * * *".into(),
        });

        let json = serde_json::to_string(&turn).unwrap();
        let decoded: HarnessTurn = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, turn);
        assert_eq!(decoded.permissions[0].mode, GrantMode::ReadWrite);
    }

    #[test]
    fn grants_builders() {
        assert_eq!(Grant::read_only("docs").mode, GrantMode::ReadOnly);
        assert_eq!(Grant::read_write("src").mode, GrantMode::ReadWrite);
    }

    #[test]
    fn remote_screen_config_roundtrip() {
        let cfg = RemoteScreenConfig::new("vm.example.com", "user", "pass");
        assert_eq!(cfg.port, 3389);
        assert_eq!(cfg.width, 1280);
        assert_eq!(cfg.height, 720);

        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: RemoteScreenConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, cfg);
    }
}
