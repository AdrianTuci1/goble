use super::agents::work_call_description;
use super::*;

use goble_core::harness::is_hidden_tool;
use goble_ui::elements::chat_content::format_elapsed;
use goble_ui::{WorkGroup, WorkRow, WorkSection};

/// The pane's work dock, as the groups an agent pane draws under its header.
///
/// The dock is grok-build's: four groups in a fixed order — workflows,
/// sub-agents, tasks and watchers — each listing the work of that kind still
/// in flight. A group with no rows is left out, so an idle pane draws no dock.
/// The fold and `show N more` state live on the app, keyed by
/// [`WorkSection::key`], so a choice survives the per-frame element rebuild.
impl UiState {
    /// Whether `section` lists its rows. An unfolded section is open, which is
    /// how the dock opens on a pane that has never been touched.
    pub fn work_section_open(&self, section: WorkSection) -> bool {
        self.work_dock_open
            .borrow()
            .get(section.key())
            .copied()
            .unwrap_or(true)
    }

    /// Whether `section` has revealed the rows behind `show N more`.
    fn work_section_show_all(&self, section: WorkSection) -> bool {
        self.work_dock_show_all
            .borrow()
            .get(section.key())
            .copied()
            .unwrap_or(false)
    }

    /// Fold or open one group: the header row's own click.
    pub fn toggle_work_section(&mut self, section: WorkSection) {
        let open = !self.work_section_open(section);
        self.work_dock_open
            .borrow_mut()
            .insert(section.key().to_string(), open);
    }

    /// Reveal the rest of one group: the `show N more` row's click.
    pub fn show_all_work_section(&mut self, section: WorkSection) {
        self.work_dock_show_all
            .borrow_mut()
            .insert(section.key().to_string(), true);
    }

    /// The groups `pane_id`'s work dock draws, in the dock's order. Empty when
    /// the pane has nothing in flight.
    pub fn work_dock_groups(&self, pane_id: u64) -> Vec<WorkGroup> {
        let group = |section: WorkSection, rows: Vec<WorkRow>| {
            (!rows.is_empty()).then(|| {
                WorkGroup::new(section, rows, self.work_section_open(section))
                    .with_show_all(self.work_section_show_all(section))
            })
        };
        [
            // Workflows and scheduled loops belong to the workspace, not to a
            // pane, so only the active pane lists them: every sibling would
            // otherwise draw the same rows.
            (pane_id == self.active_pane_id)
                .then(|| group(WorkSection::Workflows, self.workflow_work_rows()))
                .flatten(),
            group(WorkSection::Subagents, self.subagent_work_rows(pane_id)),
            group(WorkSection::Tasks, self.task_work_rows(pane_id)),
            group(WorkSection::Watchers, self.watcher_work_rows(pane_id)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// The dock's spinner phase for `pane_id`: the pane's first live work sets
    /// the clock, so a row keeps turning while its work does.
    pub fn pane_work_dock_phase(&self, pane_id: u64) -> f32 {
        self.pane_runtime
            .get(&pane_id)
            .map(PaneRuntime::work_dock_phase)
            .unwrap_or(0.0)
    }

    /// A running workflow run, named by its definition's own name and stating
    /// the phase it is on and how long it has run.
    fn workflow_work_rows(&self) -> Vec<WorkRow> {
        self.workflow_runs
            .iter()
            .filter(|run| run.status == "running")
            .map(|run| {
                let name = self
                    .workflows
                    .iter()
                    .find(|workflow| workflow.id == run.workflow_id)
                    .map(|workflow| workflow.name.clone())
                    .unwrap_or_else(|| run.workflow_id.clone());
                let mut row = WorkRow::new(run.id.clone(), "Workflow", name)
                    .spinning(true)
                    .openable(true);
                if let Some(phase) = run
                    .steps
                    .iter()
                    .rev()
                    .find(|step| step.status == "running")
                    .map(|step| step.name.clone())
                {
                    row = row.with_activity(format!("phase {phase}"));
                }
                if let Some(elapsed) = run.elapsed {
                    row = row.with_meta(format_elapsed(elapsed));
                }
                row
            })
            .collect()
    }

    /// The pane's live children, each with the activity it is doing and its own
    /// elapsed clock. A running child can be stopped; an ended one only opened.
    fn subagent_work_rows(&self, pane_id: u64) -> Vec<WorkRow> {
        self.pane_sub_agents(pane_id)
            .into_iter()
            .map(|record| {
                let running = record.is_running();
                let mut row = WorkRow::new(record.child_id.clone(), "Subagent", record.description)
                    .spinning(running)
                    .openable(!record.child_id.is_empty())
                    .with_meta(format_elapsed(record.elapsed));
                if running && !record.activity.is_empty() {
                    row = row.with_activity(record.activity);
                }
                if running {
                    row = row.killable(true);
                }
                row
            })
            .collect()
    }

    /// The pane's in-flight tool calls, named by the row their family gives
    /// them. A call whose child accounts for it, one the harness hides, or a
    /// watcher (which has its own group) is not a task of its own.
    fn task_work_rows(&self, pane_id: u64) -> Vec<WorkRow> {
        self.call_work_rows(pane_id, |call| !is_watcher_call(call))
    }

    /// The pane's live watchers: in-flight calls of a monitor/watch tool. Only
    /// a call that is running now is listed, so the group empties when the
    /// watch ends instead of pinning a scheduled loop above the transcript.
    fn watcher_work_rows(&self, pane_id: u64) -> Vec<WorkRow> {
        self.call_work_rows(pane_id, is_watcher_call)
    }

    /// The in-flight calls `wanted` accepts, each as the dock's row with the
    /// call's own elapsed clock.
    fn call_work_rows(
        &self,
        pane_id: u64,
        wanted: impl Fn(&ToolCall) -> bool,
    ) -> Vec<WorkRow> {
        let Some(rt) = self.pane_runtime.get(&pane_id) else {
            return Vec::new();
        };
        let spawned_by_running: HashSet<&str> = rt
            .sub_agents
            .values()
            .filter(|record| record.row.is_running())
            .map(|record| record.parent_call_id.as_str())
            .collect();
        let mut calls: Vec<&ToolCall> = rt
            .in_flight_tools
            .values()
            .filter(|call| !is_hidden_tool(&call.name))
            .filter(|call| !spawned_by_running.contains(call.id.as_str()))
            .filter(|call| wanted(call))
            .collect();
        calls.sort_by(|a, b| a.id.cmp(&b.id));
        calls
            .into_iter()
            .map(|call| {
                // A watcher row leads with the tool that is watching, because
                // the group above it already says "Watchers"; a task row leads
                // with the word "Task" and the verb its family gives it.
                let (kind, description) = if is_watcher_call(call) {
                    (
                        tool_kind_word(&call.name),
                        watcher_subject(call),
                    )
                } else {
                    ("Task".to_string(), work_call_description(call))
                };
                let mut row = WorkRow::new(call.id.clone(), kind, description).spinning(true);
                if let Some(started) = rt.in_flight_since.get(&call.id) {
                    row = row.with_meta(format_elapsed(started.elapsed()));
                }
                row
            })
            .collect()
    }
}

/// A tool name as the dock's kind word: the display name with its first letter
/// capitalised, so `monitor` reads `Monitor` beside a `Task` row.
fn tool_kind_word(name: &str) -> String {
    let words = goble_core::harness::tool_display_name(name);
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => words,
    }
}

/// What a watch is watching: the call's own description, or the operand the
/// tool family uses. A watch with neither is named by its tool alone, so the
/// row never draws a dangling separator.
fn watcher_subject(call: &ToolCall) -> String {
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    for key in ["description", "pattern", "path", "command", "url"] {
        if let Some(value) = arguments
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return value.to_string();
        }
    }
    String::new()
}

/// Whether a call is a watch rather than ordinary work: the harness's monitor
/// tools. They get their own group in the dock; every other call is a task.
fn is_watcher_call(call: &ToolCall) -> bool {
    matches!(call.name.to_lowercase().as_str(), "monitor" | "watch" | "watch_process")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::types::SubAgentRecord;
    use crate::ui::{WorkflowEntry, WorkflowRunEntry, WorkflowRunStepEntry};
    use goble_core::harness::ToolCallStatus;
    use goble_ui::{SubAgentRow, SubAgentRowStatus};
    use std::time::{Duration, Instant};

    fn running_call(state: &mut UiState, id: &str, name: &str, arguments: &str) {
        let rt = state.pane_runtime.entry(1).or_default();
        rt.in_flight_tools.insert(
            id.to_string(),
            ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: arguments.to_string(),
                status: ToolCallStatus::Running,
                result: None,
                diff: None,
            },
        );
        rt.in_flight_since.insert(id.to_string(), Instant::now());
    }

    fn group<'a>(groups: &'a [WorkGroup], section: WorkSection) -> Option<&'a WorkGroup> {
        groups.iter().find(|group| group.section == section)
    }

    #[test]
    fn a_running_call_is_a_task_row_with_its_clock() {
        let mut state = UiState::mock();
        running_call(
            &mut state,
            "call_1",
            "run_terminal_command",
            r#"{"command":"cargo test"}"#,
        );
        let groups = state.work_dock_groups(1);
        let tasks = group(&groups, WorkSection::Tasks).expect("the call lists a task group");
        assert_eq!(tasks.rows.len(), 1);
        assert_eq!(tasks.rows[0].kind, "Task");
        assert!(tasks.rows[0].description.contains("cargo test"));
        assert!(tasks.rows[0].spinning);
        assert!(!tasks.rows[0].meta.is_empty(), "the row states its clock");
    }

    #[test]
    fn a_running_child_is_a_subagent_row_with_both_controls() {
        let mut state = UiState::mock();
        state.pane_runtime.entry(1).or_default().sub_agents.insert(
            "conv-child-1".to_string(),
            SubAgentRecord {
                parent_call_id: "call_spawn".to_string(),
                row: SubAgentRow {
                    child_id: "conv-child-1".to_string(),
                    subagent_type: "reviewer".to_string(),
                    description: "audit the migration".to_string(),
                    status: SubAgentRowStatus::Running,
                    activity: "reading the schema".to_string(),
                    elapsed: Duration::from_millis(4500),
                    turns: 1,
                    tool_calls: 2,
                    tokens: 300,
                    outcome: None,
                    background: true,
                },
            },
        );
        let groups = state.work_dock_groups(1);
        let subagents =
            group(&groups, WorkSection::Subagents).expect("the child lists a group");
        let row = &subagents.rows[0];
        assert_eq!(row.kind, "Subagent");
        assert_eq!(row.description, "audit the migration");
        assert_eq!(row.activity.as_deref(), Some("reading the schema"));
        assert_eq!(row.meta, "4.5s");
        assert!(row.openable && row.killable);
    }

    #[test]
    fn a_running_workflow_is_named_by_its_definition() {
        let mut state = UiState::mock();
        state.workflows = vec![WorkflowEntry {
            id: "wf-1".to_string(),
            name: "Nightly digest".to_string(),
            description: String::new(),
            steps: Vec::new(),
            trigger: "manual".to_string(),
            enabled: true,
            created_at: "just now".to_string(),
        }];
        state.workflow_runs = vec![WorkflowRunEntry {
            id: "run-1".to_string(),
            workflow_id: "wf-1".to_string(),
            status: "running".to_string(),
            error: None,
            steps: vec![WorkflowRunStepEntry {
                name: "Digest".to_string(),
                status: "running".to_string(),
                detail: None,
            }],
            started_at: "1 min ago".to_string(),
            elapsed: Some(Duration::from_secs(65)),
            finished_at: None,
            output: None,
            log: Vec::new(),
        }];
        let groups = state.work_dock_groups(1);
        let workflows =
            group(&groups, WorkSection::Workflows).expect("the run lists a group");
        let row = &workflows.rows[0];
        assert_eq!(row.kind, "Workflow");
        assert_eq!(row.description, "Nightly digest");
        assert_eq!(row.activity.as_deref(), Some("phase Digest"));
        assert_eq!(row.meta, "1m05s");
        assert!(row.openable);
    }

    #[test]
    fn a_monitor_call_is_a_watcher_row() {
        let mut state = UiState::mock();
        running_call(
            &mut state,
            "call_watch",
            "monitor",
            r#"{"description":"watch the dev instance"}"#,
        );
        running_call(
            &mut state,
            "call_task",
            "run_terminal_command",
            r#"{"command":"cargo test"}"#,
        );
        let groups = state.work_dock_groups(1);
        let watchers = group(&groups, WorkSection::Watchers).expect("the watch lists a group");
        assert_eq!(watchers.rows.len(), 1);
        assert_eq!(watchers.rows[0].id, "call_watch");
        assert_eq!(watchers.rows[0].kind, "Monitor", "the tool that watches");
        assert_eq!(watchers.rows[0].description, "watch the dev instance");
        assert!(watchers.rows[0].spinning);
        let tasks = group(&groups, WorkSection::Tasks).expect("the call lists a task group");
        assert_eq!(tasks.rows.len(), 1, "a watch is not also a task");
        assert_eq!(tasks.rows[0].id, "call_task");
    }

    #[test]
    fn a_scheduled_loop_does_not_pin_a_watcher_group() {
        let mut state = UiState::mock();
        state.crons = vec![
            crate::ui::CronEntry::new("cron-1", "Morning digest", "0 9 * * *", "2h ago"),
        ];
        let groups = state.work_dock_groups(1);
        assert!(
            group(&groups, WorkSection::Watchers).is_none(),
            "an idle scheduled loop is not a live watcher"
        );
    }

    #[test]
    fn folding_a_group_survives_the_snapshot_and_show_all_reveals_the_rest() {
        let mut state = UiState::mock();
        for index in 0..6 {
            running_call(
                &mut state,
                &format!("call_{index}"),
                "read_file",
                r#"{"path":"src/lib.rs"}"#,
            );
        }
        let groups = state.work_dock_groups(1);
        let tasks = group(&groups, WorkSection::Tasks).expect("six calls list a group");
        assert_eq!(tasks.rows.len(), 6, "the group keeps every row");
        assert!(!tasks.show_all, "the group starts folded to its first rows");

        state.show_all_work_section(WorkSection::Tasks);
        let groups = state.work_dock_groups(1);
        let tasks = group(&groups, WorkSection::Tasks).expect("the group survives");
        assert!(tasks.show_all, "show all is app-owned and survives");

        state.toggle_work_section(WorkSection::Tasks);
        let groups = state.work_dock_groups(1);
        let tasks = group(&groups, WorkSection::Tasks).expect("a folded group keeps its header");
        assert!(!tasks.open, "the fold survives the snapshot");
    }
}
