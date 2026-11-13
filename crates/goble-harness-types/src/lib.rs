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

/// A medium is *where* the work runs: local, VM, remote worker, browser, container.
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
    Remote,
    Browser,
    Container,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionChannel {
    HeadlessShell,
    GuiScreen,
    Cdp,
    Voice,
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

/// The prefix a composer entry is typed behind. A harness advertises one list
/// of entries and each says which trigger opens it, so the host never has to
/// know a harness's own idea of commands, skills or mentions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerTrigger {
    /// `/` — a command: the harness runs it, or the host does on its behalf.
    Slash,
    /// `$` — a skill the harness can load into the next turn.
    Skill,
}

impl ComposerTrigger {
    /// The character the entry is typed behind.
    pub fn prefix(self) -> char {
        match self {
            ComposerTrigger::Slash => '/',
            ComposerTrigger::Skill => '$',
        }
    }
}

/// One entry a harness advertises to a composer: the name typed behind its
/// trigger, what it does, and the host action that runs it.
///
/// `action` is the id the host knows the entry by — the same shapes the
/// reference client receives from an ACP session's `available_commands`. A
/// harness that owns the entry end to end leaves it empty, and the host sends
/// the entry's text with the draft instead of running something itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCommand {
    pub name: String,
    pub description: String,
    pub trigger: ComposerTrigger,
    #[serde(default)]
    pub action: String,
}

impl HarnessCommand {
    /// A `/` command the host runs through `action`.
    pub fn slash(
        name: impl Into<String>,
        description: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            trigger: ComposerTrigger::Slash,
            action: action.into(),
        }
    }

    /// A `$` skill the harness loads itself.
    pub fn skill(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            trigger: ComposerTrigger::Skill,
            action: String::new(),
        }
    }
}

/// A model a harness runs, with the thinking levels it accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessModel {
    /// The id a turn runs on.
    pub id: String,
    /// The label the picker shows; the id when the harness names none.
    pub name: String,
    /// The thinking levels the model offers, in the order a picker lists them.
    /// Empty means the model has nothing to choose.
    #[serde(default)]
    pub thinking: Vec<String>,
}

impl HarnessModel {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            thinking: Vec::new(),
        }
    }

    pub fn with_thinking(mut self, levels: Vec<String>) -> Self {
        self.thinking = levels;
        self
    }
}

/// What a harness advertises to a composer: the commands and skills it offers,
/// and the models it can run with the thinking levels each accepts.
///
/// The host reads this instead of keeping a list of its own, so a harness that
/// brings its own commands shows them without the host learning anything about
/// it. The native harness reports the app's own command list and the models the
/// configuration declares.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCatalog {
    #[serde(default)]
    pub commands: Vec<HarnessCommand>,
    #[serde(default)]
    pub models: Vec<HarnessModel>,
}

impl HarnessCatalog {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_commands(mut self, commands: Vec<HarnessCommand>) -> Self {
        self.commands = commands;
        self
    }

    pub fn with_models(mut self, models: Vec<HarnessModel>) -> Self {
        self.models = models;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.models.is_empty()
    }

    /// The entries behind `trigger`, in the order the harness listed them.
    pub fn commands_for(&self, trigger: ComposerTrigger) -> Vec<&HarnessCommand> {
        self.commands
            .iter()
            .filter(|command| command.trigger == trigger)
            .collect()
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

/// The user's decision on a command the harness proposed before running it.
///
/// Shared by the harness seam and the daemon wire so the same decision travels
/// from the composer to the executing harness: `Approve` runs the chosen text
/// verbatim, `Edit` runs the user's edited text, and `Reject` refuses the
/// command (the harness turns it into a failed tool call).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CommandDecision {
    Approve(String),
    Edit(String),
    Reject(String),
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
    fn a_catalog_separates_commands_from_skills() {
        let catalog = HarnessCatalog::empty()
            .with_commands(vec![
                HarnessCommand::slash("new", "Start a conversation", "new"),
                HarnessCommand::skill("deploy", "Ship the current branch"),
            ])
            .with_models(vec![
                HarnessModel::new("deepseek-flash", "Deepseek-V4-Flash")
                    .with_thinking(vec!["off".into(), "on".into()]),
            ]);

        let slash = catalog.commands_for(ComposerTrigger::Slash);
        assert_eq!(slash.len(), 1);
        assert_eq!(slash[0].name, "new");
        assert_eq!(slash[0].action, "new");
        assert_eq!(ComposerTrigger::Slash.prefix(), '/');

        let skills = catalog.commands_for(ComposerTrigger::Skill);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "deploy");
        assert!(skills[0].action.is_empty(), "the harness loads its own skill");
        assert_eq!(ComposerTrigger::Skill.prefix(), '$');

        assert_eq!(catalog.models[0].thinking, vec!["off", "on"]);
        assert!(!catalog.is_empty());
        assert!(HarnessCatalog::empty().is_empty());
    }

    #[test]
    fn a_catalog_roundtrips_through_the_wire() {
        let catalog = HarnessCatalog::empty()
            .with_commands(vec![HarnessCommand::slash("clear", "Clear", "clear")])
            .with_models(vec![HarnessModel::new("m", "M")]);
        let json = serde_json::to_string(&catalog).unwrap();
        let decoded: HarnessCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, catalog);
    }
}
