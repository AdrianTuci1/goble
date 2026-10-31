//! Rhai workflow bodies: the script runner a workflow executes when it carries
//! one, with a host seam for agent calls.
//!
//! A script starts with a pure-literal `meta` map — `name`, `description`,
//! `when_to_use` and the `phases` the UI draws — and then orchestrates agent
//! calls with `agent`, `parallel`, `phase`, `log`, `budget` and `complete`.
//! The engine is deterministic by omission: no clock, no randomness, no process
//! control is registered. Agent failure is data (`success: false` on the
//! returned map); an exhausted budget is an error.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, Position, Scope};
use serde::{Deserialize, Serialize};

use crate::{slug, RunStatus, StepExecution, StepId, StepStatus, WorkflowId, WorkflowRun};

/// The default cap on `agent`/`parallel` launches in one run.
pub const DEFAULT_BUDGET: u32 = 128;

/// What a script declares about itself in its leading `meta` map.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    #[serde(default)]
    pub phases: Vec<PhaseMeta>,
}

/// One phase the script draws in the run rail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseMeta {
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// What the script asks for on one agent call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentOpts {
    pub prompt: String,
    pub label: Option<String>,
    pub phase: Option<String>,
    pub agent_type: Option<String>,
}

/// How one agent call settled. `success: false` is data the script may branch
/// on; infrastructure failures surface as script errors instead.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentOutcome {
    pub agent_id: String,
    pub success: bool,
    pub output: String,
    pub tokens_used: u64,
    pub duration_ms: u64,
}

/// The seam a script uses to do work: one agent call and one progress line.
pub trait ScriptHost: Send + Sync {
    fn agent(&self, opts: &AgentOpts) -> AgentOutcome;
    /// Record a `log()` line. The default keeps nothing, for hosts that only
    /// care about the run result.
    fn log(&self, _line: &str) {}
}

/// One recorded agent call, kept on the run for display and debugging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCall {
    pub label: String,
    pub phase: Option<String>,
    pub prompt: String,
    pub success: bool,
    pub output: String,
    pub tokens_used: u64,
    pub duration_ms: u64,
}

/// One phase as the script ran it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseRun {
    pub title: String,
    pub status: StepStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// The settled result of a script body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptRun {
    pub meta: Meta,
    pub status: RunStatus,
    pub phases: Vec<PhaseRun>,
    pub output: Option<serde_json::Value>,
    pub log: Vec<String>,
    pub calls: Vec<AgentCall>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

impl ScriptRun {
    /// The status log the daemon and the run overlay already understand.
    pub fn to_workflow_run(&self, workflow_id: WorkflowId) -> WorkflowRun {
        let steps = self
            .phases
            .iter()
            .enumerate()
            .map(|(index, phase)| StepExecution {
                index,
                step_id: StepId::new(slug(&phase.title)),
                name: phase.title.clone(),
                status: phase.status.clone(),
                started_at: phase.started_at,
                finished_at: phase.finished_at.or(Some(self.finished_at)),
            })
            .collect();
        WorkflowRun {
            workflow_id,
            status: self.status.clone(),
            steps,
            started_at: self.started_at,
            finished_at: Some(self.finished_at),
            output: self.output.clone(),
            log: self.log.clone(),
        }
    }
}

/// What `validate_only` reports: metadata, compile result and the one path the
/// given `args` selects, run against a canned host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    pub meta: Meta,
    pub status: RunStatus,
    pub phases: Vec<PhaseRun>,
    pub calls: usize,
}

/// Reads a script's `meta` header without touching the real host.
///
/// The header must be the script's first statement, so it is evaluated before
/// anything else. The rest of the body runs against stub host functions with a
/// low operation cap: no agent runs, no file or network access, and any error
/// after the header is irrelevant — only the `meta` value is read.
pub fn meta_of(source: &str) -> Result<Meta, String> {
    if let Some(call) = forbidden_call(source) {
        return Err(format!(
            "{call}() is not available in workflow scripts: a run must be deterministic and must never block"
        ));
    }
    let head = source.trim_start();
    let declares_meta = head.starts_with("let meta")
        || head.starts_with("const meta")
        || head.starts_with("export let meta");
    if !declares_meta {
        return Err(
            "the script must start with `let meta = #{ name: ..., phases: [...] }`".to_string(),
        );
    }
    let engine = stub_engine();
    let ast = engine
        .compile(source)
        .map_err(|e| format!("compile failed: {e}"))?;
    let mut scope = Scope::new();
    let _ = engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast);
    let value = scope
        .get_value::<Dynamic>("meta")
        .ok_or_else(|| "the script's `meta` map did not evaluate".to_string())?;
    let json: serde_json::Value = rhai::serde::from_dynamic(&value)
        .map_err(|e| format!("meta must be a map of plain values: {e}"))?;
    serde_json::from_value(json).map_err(|e| format!("meta is malformed: {e}"))
}

/// The engine `meta_of` evaluates with: every host call is a harmless stub.
fn stub_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(50_000);
    engine.register_fn("agent", |_prompt: &str| stub_agent());
    engine.register_fn("agent", |_prompt: &str, _opts: Map| stub_agent());
    engine.register_fn("parallel", |calls: Array| -> Array {
        calls.iter().map(|_| Dynamic::from(stub_agent())).collect()
    });
    engine.register_fn("phase", |_title: &str| {});
    engine.register_fn("log", |_line: &str| {});
    engine.register_fn("budget", || -> Map { Map::new() });
    engine.register_fn("complete", |_value: Dynamic| {});
    engine.register_fn("json_encode", |_value: Dynamic| String::new());
    engine
}

/// The name of a clock or blocking call the source uses, skipping strings and
/// line comments so a mention inside a message is not rejected.
fn forbidden_call(source: &str) -> Option<&'static str> {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &source[start..i];
            if word == "sleep" || word == "timestamp" {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if bytes.get(j) == Some(&b'(') {
                    return Some(if word == "sleep" {
                        "sleep"
                    } else {
                        "timestamp"
                    });
                }
            }
            continue;
        }
        i += 1;
    }
    None
}

fn stub_agent() -> Map {
    let mut map = Map::new();
    map.insert("agent_id".into(), "stub".into());
    map.insert("success".into(), true.into());
    map.insert("output".into(), String::new().into());
    map.insert("tokens_used".into(), 0i64.into());
    map.insert("duration_ms".into(), 0i64.into());
    map
}

/// Runs workflow scripts with a per-run agent budget.
#[derive(Debug, Clone)]
pub struct ScriptEngine {
    budget: u32,
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptEngine {
    pub fn new() -> Self {
        Self {
            budget: DEFAULT_BUDGET,
        }
    }

    pub fn with_budget(mut self, budget: u32) -> Self {
        self.budget = budget.clamp(1, 1024);
        self
    }

    pub fn budget(&self) -> u32 {
        self.budget
    }

    /// Compile the script and run the single path `args` selects against a host
    /// that answers every agent call successfully. Nothing external runs.
    pub fn validate(
        &self,
        source: &str,
        args: &serde_json::Value,
    ) -> Result<ValidationReport, String> {
        let host = Arc::new(CannedHost::default());
        let run = self.run(source, args, host)?;
        Ok(ValidationReport {
            meta: run.meta,
            status: run.status,
            phases: run.phases,
            calls: run.calls.len(),
        })
    }

    /// Run `source` with `args` bound to the script's `args` variable.
    pub fn run(
        &self,
        source: &str,
        args: &serde_json::Value,
        host: Arc<dyn ScriptHost>,
    ) -> Result<ScriptRun, String> {
        let meta = meta_of(source)?;
        let started_at = Utc::now();
        let inner = Arc::new(Mutex::new(RunState::new(started_at, self.budget)));
        let engine = build_engine(Arc::clone(&inner), host);
        let ast = engine
            .compile(source)
            .map_err(|e| format!("compile failed: {e}"))?;

        let mut scope = Scope::new();
        let args_dynamic = rhai::serde::to_dynamic(args).unwrap_or(Dynamic::UNIT);
        scope.push_dynamic("args", args_dynamic);

        let outcome = engine.eval_ast_with_scope::<Dynamic>(&mut scope, &ast);
        let mut state = inner.lock().expect("script state poisoned");
        match outcome {
            Ok(_) => {}
            Err(error) if matches!(*error, EvalAltResult::ErrorTerminated(..)) => {}
            Err(error) => {
                state.failure.get_or_insert_with(|| format!("{error}"));
            }
        }
        state.close_phase(None);
        let finished_at = Utc::now();
        let settled = std::mem::replace(&mut *state, RunState::new(finished_at, self.budget));
        Ok(settled.settle(meta, finished_at))
    }
}

/// A host for `validate`: every agent succeeds with a fixed small output.
#[derive(Default)]
struct CannedHost {
    calls: Mutex<u32>,
}

impl ScriptHost for CannedHost {
    fn agent(&self, _opts: &AgentOpts) -> AgentOutcome {
        let mut calls = self.calls.lock().expect("canned host poisoned");
        *calls += 1;
        AgentOutcome {
            agent_id: format!("validate-{calls}"),
            success: true,
            output: "ok".to_string(),
            tokens_used: 0,
            duration_ms: 0,
        }
    }
}

struct PhaseState {
    title: String,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    status: StepStatus,
}

struct RunState {
    started_at: DateTime<Utc>,
    budget: u32,
    spent: u32,
    log: Vec<String>,
    calls: Vec<AgentCall>,
    phases: Vec<PhaseState>,
    open: Option<usize>,
    completed: Option<serde_json::Value>,
    failure: Option<String>,
}

impl RunState {
    fn new(started_at: DateTime<Utc>, budget: u32) -> Self {
        Self {
            started_at,
            budget,
            spent: 0,
            log: Vec::new(),
            calls: Vec::new(),
            phases: Vec::new(),
            open: None,
            completed: None,
            failure: None,
        }
    }

    fn phase_status(&mut self, title: &str) -> usize {
        if let Some(index) = self.open {
            if self.phases[index].title == title {
                return index;
            }
            self.close_phase(None);
        }
        self.phases.push(PhaseState {
            title: title.to_string(),
            started_at: Utc::now(),
            finished_at: None,
            status: StepStatus::Succeeded,
        });
        let index = self.phases.len() - 1;
        self.open = Some(index);
        index
    }

    fn close_phase(&mut self, status: Option<StepStatus>) {
        if let Some(index) = self.open.take() {
            let phase = &mut self.phases[index];
            if let Some(status) = status {
                phase.status = status;
            }
            phase.finished_at = Some(Utc::now());
        }
    }

    fn fail_here(&mut self, message: &str) {
        let index = match self.open {
            Some(index) => Some(index),
            None => {
                self.phase_status("Run");
                self.open
            }
        };
        if let Some(index) = index {
            if matches!(self.phases[index].status, StepStatus::Succeeded) {
                self.phases[index].status = StepStatus::Failed(message.to_string());
            }
        }
    }

    fn settle(mut self, meta: Meta, finished_at: DateTime<Utc>) -> ScriptRun {
        self.close_phase(None);
        let status = if let Some(failure) = &self.failure {
            RunStatus::Failed(failure.clone())
        } else if let Some(first) = self.calls.iter().find(|call| !call.success) {
            RunStatus::Failed(format!("{}: agent call failed", first.label))
        } else {
            RunStatus::Succeeded
        };
        if self.phases.is_empty() {
            self.phases.push(PhaseState {
                title: if meta.name.trim().is_empty() {
                    "Run".to_string()
                } else {
                    meta.name.clone()
                },
                started_at: self.started_at,
                finished_at: Some(finished_at),
                status: match &status {
                    RunStatus::Succeeded => StepStatus::Succeeded,
                    RunStatus::Failed(message) => StepStatus::Failed(message.clone()),
                    RunStatus::Running => StepStatus::Succeeded,
                },
            });
        }
        ScriptRun {
            meta,
            status,
            phases: self
                .phases
                .into_iter()
                .map(|phase| PhaseRun {
                    title: phase.title,
                    status: phase.status,
                    started_at: phase.started_at,
                    finished_at: phase.finished_at,
                })
                .collect(),
            output: self.completed,
            log: self.log,
            calls: self.calls,
            started_at: self.started_at,
            finished_at,
        }
    }
}

fn runtime_error(message: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        message.into().into(),
        Position::NONE,
    ))
}

fn option_string(value: &Dynamic) -> Option<String> {
    value.clone().into_string().ok()
}

fn map_option(map: &Map, key: &str) -> Option<String> {
    map.get(key).and_then(option_string)
}

fn build_engine(inner: Arc<Mutex<RunState>>, host: Arc<dyn ScriptHost>) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(5_000_000);

    // `agent(prompt)` and `agent(prompt, #{ label, phase, agent_type })`
    let one = Arc::clone(&inner);
    let host_one = Arc::clone(&host);
    engine.register_fn(
        "agent",
        move |prompt: &str| -> Result<Dynamic, Box<EvalAltResult>> {
            call_agent(&one, &host_one, prompt, AgentOpts::default())
        },
    );
    let two = Arc::clone(&inner);
    let host_two = Arc::clone(&host);
    engine.register_fn(
        "agent",
        move |prompt: &str, opts: Map| -> Result<Dynamic, Box<EvalAltResult>> {
            call_agent(&two, &host_two, prompt, opts_from(&opts))
        },
    );

    // `parallel([...])`: one sequential pass with atomic budget admission,
    // results in input order. A failed call yields `()`.
    let par = Arc::clone(&inner);
    let host_par = Arc::clone(&host);
    engine.register_fn("parallel", move |calls: Array| -> Array {
        calls
            .iter()
            .map(|call| {
                let opts = match call.clone().try_cast::<Map>() {
                    Some(map) => {
                        let prompt = map_option(&map, "prompt").unwrap_or_default();
                        opts_from(&map).with_prompt(prompt)
                    }
                    None => AgentOpts {
                        prompt: call.clone().into_string().unwrap_or_default(),
                        ..Default::default()
                    },
                };
                let prompt = opts.prompt.clone();
                call_agent(&par, &host_par, &prompt, opts).unwrap_or(Dynamic::UNIT)
            })
            .collect()
    });

    let phase = Arc::clone(&inner);
    engine.register_fn("phase", move |title: &str| {
        let mut state = phase.lock().expect("script state poisoned");
        state.phase_status(title);
    });

    let log = Arc::clone(&inner);
    let host_log = Arc::clone(&host);
    engine.register_fn("log", move |line: &str| {
        host_log.log(line);
        let mut state = log.lock().expect("script state poisoned");
        state.log.push(line.to_string());
    });

    let budget = Arc::clone(&inner);
    engine.register_fn("budget", move || -> Map {
        let state = budget.lock().expect("script state poisoned");
        let mut map = Map::new();
        map.insert("total".into(), (state.budget as i64).into());
        map.insert("spent".into(), (state.spent as i64).into());
        map.insert("reserved".into(), 0i64.into());
        map.insert(
            "remaining".into(),
            ((state.budget.saturating_sub(state.spent)) as i64).into(),
        );
        map
    });

    let complete = Arc::clone(&inner);
    engine.register_fn(
        "complete",
        move |value: Dynamic| -> Result<(), Box<EvalAltResult>> {
            let mut state = complete.lock().expect("script state poisoned");
            state.completed = rhai::serde::from_dynamic::<serde_json::Value>(&value).ok();
            Err(Box::new(EvalAltResult::ErrorTerminated(
                Dynamic::UNIT,
                Position::NONE,
            )))
        },
    );

    engine.register_fn("json_encode", |value: Dynamic| -> String {
        rhai::serde::from_dynamic::<serde_json::Value>(&value)
            .map(|json| json.to_string())
            .unwrap_or_else(|_| "null".to_string())
    });

    engine
}

fn opts_from(map: &Map) -> AgentOpts {
    AgentOpts {
        prompt: map_option(map, "prompt").unwrap_or_default(),
        label: map_option(map, "label"),
        phase: map_option(map, "phase"),
        agent_type: map_option(map, "agent_type"),
    }
}

impl AgentOpts {
    fn with_prompt(mut self, prompt: String) -> Self {
        self.prompt = prompt;
        self
    }
}

fn call_agent(
    inner: &Arc<Mutex<RunState>>,
    host: &Arc<dyn ScriptHost>,
    prompt: &str,
    opts: AgentOpts,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let opts = opts.with_prompt(prompt.to_string());
    {
        let mut state = inner.lock().expect("script state poisoned");
        if state.spent >= state.budget {
            let message = format!(
                "workflow budget exhausted: {} agent calls allowed",
                state.budget
            );
            state.failure = Some(message.clone());
            state.fail_here(&message);
            return Err(runtime_error(message));
        }
        state.spent += 1;
        if let Some(phase) = &opts.phase {
            state.phase_status(phase);
        }
    }

    let outcome = host.agent(&opts);
    let label = opts
        .label
        .clone()
        .unwrap_or_else(|| prompt.lines().next().unwrap_or_default().to_string());
    {
        let mut state = inner.lock().expect("script state poisoned");
        state.calls.push(AgentCall {
            label: label.clone(),
            phase: opts.phase.clone(),
            prompt: prompt.to_string(),
            success: outcome.success,
            output: outcome.output.clone(),
            tokens_used: outcome.tokens_used,
            duration_ms: outcome.duration_ms,
        });
        if !outcome.success {
            let message = format!("agent call failed: {label}");
            state.fail_here(&message);
        }
    }

    let mut map = Map::new();
    map.insert("agent_id".into(), outcome.agent_id.into());
    map.insert("success".into(), outcome.success.into());
    map.insert("output".into(), outcome.output.into());
    map.insert("tokens_used".into(), (outcome.tokens_used as i64).into());
    map.insert("duration_ms".into(), (outcome.duration_ms as i64).into());
    Ok(map.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecordingHost {
        calls: Mutex<Vec<AgentOpts>>,
        fail_on: Option<String>,
        lines: AtomicUsize,
    }

    impl ScriptHost for RecordingHost {
        fn agent(&self, opts: &AgentOpts) -> AgentOutcome {
            self.calls.lock().unwrap().push(opts.clone());
            let failed = self
                .fail_on
                .as_deref()
                .is_some_and(|needle| opts.prompt.contains(needle));
            AgentOutcome {
                agent_id: format!("child-{}", self.calls.lock().unwrap().len()),
                success: !failed,
                output: if failed {
                    "boom".to_string()
                } else {
                    format!("done: {}", opts.prompt)
                },
                tokens_used: 7,
                duration_ms: 12,
            }
        }

        fn log(&self, _line: &str) {
            self.lines.fetch_add(1, Ordering::SeqCst);
        }
    }

    const SCRIPT: &str = r#"
let meta = #{
    name: "release",
    description: "Ship a build",
    phases: [#{ title: "Check" }, #{ title: "Ship" }]
};

phase("Check");
let a = agent("run the tests", #{ label: "tests" });
log("tests ok: " + a.output);
phase("Ship");
let b = agent("publish", #{ label: "publish" });
complete(#{ shipped: b.success });
"#;

    #[test]
    fn meta_is_read_without_running_the_body() {
        let meta = meta_of(SCRIPT).unwrap();
        assert_eq!(meta.name, "release");
        assert_eq!(meta.description, "Ship a build");
        assert_eq!(meta.phases.len(), 2);
        assert_eq!(meta.phases[0].title, "Check");
    }

    #[test]
    fn a_missing_meta_header_is_reported() {
        let err = meta_of("let x = 1;").unwrap_err();
        assert!(err.contains("must start with"), "{err}");
    }

    #[test]
    fn a_compile_error_is_an_error_not_a_panic() {
        assert!(meta_of("let meta = #{ name: };").is_err());
        let engine = ScriptEngine::new();
        assert!(engine
            .run(
                "let meta = #{ name: \"x\" }; oops(",
                &serde_json::json!({}),
                Arc::new(RecordingHost::default())
            )
            .is_err());
    }

    #[test]
    fn phases_and_agents_run_in_order() {
        let host = Arc::new(RecordingHost::default());
        let run = ScriptEngine::new()
            .run(SCRIPT, &serde_json::json!({}), host.clone())
            .unwrap();
        assert_eq!(run.status, RunStatus::Succeeded);
        assert_eq!(
            run.phases
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Check", "Ship"]
        );
        assert!(run.phases.iter().all(|p| p.status == StepStatus::Succeeded));
        let prompts: Vec<_> = host
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|c| c.prompt.clone())
            .collect();
        assert_eq!(prompts, vec!["run the tests", "publish"]);
        assert_eq!(run.output, Some(serde_json::json!({"shipped": true})));
        assert_eq!(run.log, vec!["tests ok: done: run the tests"]);
    }

    #[test]
    fn a_failed_agent_marks_the_phase_and_the_run() {
        let host = Arc::new(RecordingHost {
            fail_on: Some("publish".to_string()),
            ..Default::default()
        });
        let run = ScriptEngine::new()
            .run(SCRIPT, &serde_json::json!({}), host)
            .unwrap();
        assert!(matches!(run.status, RunStatus::Failed(_)));
        assert_eq!(run.phases[0].status, StepStatus::Succeeded);
        assert!(matches!(run.phases[1].status, StepStatus::Failed(_)));
    }

    #[test]
    fn the_budget_stops_launches_without_calling_the_host() {
        let host = Arc::new(RecordingHost::default());
        let script = r#"
let meta = #{ name: "cap", phases: [#{ title: "Loop" }] };
phase("Loop");
agent("one");
agent("two");
"#;
        let run = ScriptEngine::new()
            .with_budget(1)
            .run(script, &serde_json::json!({}), host.clone())
            .unwrap();
        assert_eq!(host.calls.lock().unwrap().len(), 1);
        assert!(matches!(run.status, RunStatus::Failed(_)));
        let failure = match &run.status {
            RunStatus::Failed(message) => message.clone(),
            _ => unreachable!(),
        };
        assert!(failure.contains("budget exhausted"), "{failure}");
    }

    #[test]
    fn parallel_returns_results_in_input_order() {
        let host = Arc::new(RecordingHost::default());
        let script = r#"
let meta = #{ name: "fan" };
let results = parallel([
    #{ prompt: "a", label: "A" },
    #{ prompt: "b", label: "B" }
]);
log(results[0].output + "|" + results[1].output);
"#;
        let run = ScriptEngine::new()
            .run(script, &serde_json::json!({}), host)
            .unwrap();
        assert_eq!(run.status, RunStatus::Succeeded);
        assert_eq!(run.calls.len(), 2);
        assert_eq!(run.log, vec!["done: a|done: b"]);
    }

    #[test]
    fn args_are_visible_to_the_script() {
        let host = Arc::new(RecordingHost::default());
        let script = r#"
let meta = #{ name: "args" };
agent(args.topic);
"#;
        let run = ScriptEngine::new()
            .run(
                script,
                &serde_json::json!({"topic": "schema"}),
                host.clone(),
            )
            .unwrap();
        assert_eq!(host.calls.lock().unwrap()[0].prompt, "schema");
        assert_eq!(run.status, RunStatus::Succeeded);
    }

    #[test]
    fn determinism_guards_are_not_registered() {
        let engine = ScriptEngine::new();
        let host = Arc::new(RecordingHost::default());
        for call in ["timestamp()", "sleep(1)", "rand()"] {
            let script = format!("let meta = #{{ name: \"x\" }}; {call};");
            match engine.run(&script, &serde_json::json!({}), host.clone()) {
                Err(_) => {}
                Ok(run) => assert!(
                    matches!(run.status, RunStatus::Failed(_)),
                    "{call} must not resolve, got {:?}",
                    run.status
                ),
            }
        }
    }

    #[test]
    fn a_forbidden_name_inside_a_string_or_comment_is_fine() {
        let script = r#"
let meta = #{ name: "x" };
log("we never sleep( or timestamp( here");
// sleep(1); timestamp();
"#;
        assert!(meta_of(script).is_ok());
    }

    #[test]
    fn validate_runs_the_selected_path_against_a_canned_host() {
        let report = ScriptEngine::new()
            .validate(SCRIPT, &serde_json::json!({}))
            .unwrap();
        assert_eq!(report.meta.name, "release");
        assert_eq!(report.status, RunStatus::Succeeded);
        assert_eq!(report.phases.len(), 2);
        assert_eq!(report.calls, 2);
    }

    #[test]
    fn a_script_run_converts_to_the_shared_status_log() {
        let run = ScriptEngine::new()
            .run(
                SCRIPT,
                &serde_json::json!({}),
                Arc::new(RecordingHost::default()),
            )
            .unwrap();
        let wf = run.to_workflow_run(WorkflowId::new("wf"));
        assert_eq!(wf.workflow_id, WorkflowId::new("wf"));
        assert_eq!(wf.status, RunStatus::Succeeded);
        assert_eq!(wf.steps.len(), 2);
        assert_eq!(wf.steps[0].step_id, StepId::new("check"));
        assert_eq!(wf.output, run.output);
    }

    #[test]
    fn a_script_workflow_derives_its_steps_from_the_phases() {
        let wf = crate::Workflow::new(WorkflowId::new("wf"), "placeholder")
            .with_script(SCRIPT)
            .unwrap();
        assert_eq!(wf.name, "release");
        assert_eq!(wf.steps.len(), 2);
        assert_eq!(wf.steps[0].name, "Check");
        assert!(wf.validate().is_ok());
        assert!(wf.script.is_some());
    }
}
