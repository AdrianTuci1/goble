//! Workflow files and immutable revisions.
//!
//! A workflow is a TOML file in `.goble/workflows/`. Deploying it validates the
//! file and stores the exact bytes under a content hash; a run names the hash,
//! so editing the file later cannot change what an in-flight run executes.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("invalid workflow TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("the workflow name is empty")]
    EmptyName,
    #[error("the workflow trigger is empty")]
    EmptyTrigger,
    #[error("the workflow has no steps")]
    NoSteps,
    #[error("step {0} has an empty prompt")]
    EmptyPrompt(String),
    #[error("a step must have a non-empty name")]
    EmptyStepName,
    #[error("step {step} depends on unknown step {dependency}")]
    UnknownDependency { step: String, dependency: String },
    #[error("the workflow steps contain a cycle: {0}")]
    Cycle(String),
    #[error("the target timeout must be greater than zero")]
    InvalidTimeout,
    #[error("the attempt budget must be greater than zero")]
    InvalidAttempts,
    #[error("step {step} has an empty secret reference")]
    EmptySecretRef { step: String },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WorkflowFile {
    pub workflow: WorkflowMeta,
    pub target: TargetPolicy,
    #[serde(default)]
    pub steps: BTreeMap<String, StepConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkflowMeta {
    pub name: String,
    pub on: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TargetPolicy {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub sandbox: Sandbox,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_attempts")]
    pub max_attempts: u32,
    /// Provider-native options. They stay opaque to the hub; the worker's
    /// provider adapter validates them.
    #[serde(default)]
    pub options: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StepConfig {
    pub prompt: String,
    #[serde(default)]
    pub grants: Grant,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub approval: Approval,
    /// Names only. Values are resolved by the worker's vault.
    #[serde(default)]
    pub secret_refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Grant {
    #[default]
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Approval {
    #[default]
    Never,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sandbox {
    #[default]
    None,
    E2b,
    Modal,
    Daytona,
    Box,
    Docker,
    Ssh,
}

impl Sandbox {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::E2b => "e2b",
            Self::Modal => "modal",
            Self::Daytona => "daytona",
            Self::Box => "box",
            Self::Docker => "docker",
            Self::Ssh => "ssh",
        }
    }
}

impl Grant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "ReadOnly",
            Self::ReadWrite => "ReadWrite",
        }
    }
}

impl Approval {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Always => "always",
        }
    }
}

impl WorkflowFile {
    /// Parse and validate a workflow file. Deployment is the only place that
    /// should call this; dispatch reads the stored revision.
    pub fn parse(content: &str) -> Result<Self, WorkflowError> {
        let workflow: Self = toml::from_str(content)?;
        workflow.validate()?;
        Ok(workflow)
    }

    pub fn validate(&self) -> Result<(), WorkflowError> {
        if self.workflow.name.trim().is_empty() {
            return Err(WorkflowError::EmptyName);
        }
        if self.workflow.on.trim().is_empty() {
            return Err(WorkflowError::EmptyTrigger);
        }
        if self.target.timeout_secs == 0 {
            return Err(WorkflowError::InvalidTimeout);
        }
        if self.target.max_attempts == 0 {
            return Err(WorkflowError::InvalidAttempts);
        }
        if self.steps.is_empty() {
            return Err(WorkflowError::NoSteps);
        }
        for (name, step) in &self.steps {
            if name.trim().is_empty() {
                return Err(WorkflowError::EmptyStepName);
            }
            if step.prompt.trim().is_empty() {
                return Err(WorkflowError::EmptyPrompt(name.clone()));
            }
            for reference in &step.secret_refs {
                if reference.trim().is_empty() {
                    return Err(WorkflowError::EmptySecretRef { step: name.clone() });
                }
            }
        }
        self.validate_dependencies()?;
        self.validate_acyclic()?;
        Ok(())
    }

    fn validate_dependencies(&self) -> Result<(), WorkflowError> {
        for (name, step) in &self.steps {
            for dependency in &step.needs {
                if !self.steps.contains_key(dependency) {
                    return Err(WorkflowError::UnknownDependency {
                        step: name.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn validate_acyclic(&self) -> Result<(), WorkflowError> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            Unvisited,
            Visiting,
            Done,
        }
        let mut marks: HashMap<&str, Mark> = self
            .steps
            .keys()
            .map(|name| (name.as_str(), Mark::Unvisited))
            .collect();
        for start in self.steps.keys() {
            if marks[start.as_str()] == Mark::Done {
                continue;
            }
            let mut stack = vec![(start.as_str(), 0usize)];
            while let Some((name, next)) = stack.pop() {
                if marks[name] == Mark::Unvisited {
                    marks.insert(name, Mark::Visiting);
                }
                let needs = &self.steps[name].needs;
                if next < needs.len() {
                    stack.push((name, next + 1));
                    let dependency = needs[next].as_str();
                    match marks[dependency] {
                        Mark::Visiting => {
                            let mut cycle: Vec<&str> =
                                stack.iter().map(|(name, _)| *name).collect();
                            cycle.push(dependency);
                            return Err(WorkflowError::Cycle(cycle.join(" -> ")));
                        }
                        Mark::Done => {}
                        Mark::Unvisited => stack.push((dependency, 0)),
                    }
                } else {
                    marks.insert(name, Mark::Done);
                }
            }
        }
        Ok(())
    }

    /// Steps with no unmet dependencies, in step-name order.
    pub fn ready_steps<'a>(&'a self, completed: &HashSet<&str>) -> Vec<&'a str> {
        self.steps
            .iter()
            .filter(|(name, step)| {
                !completed.contains(name.as_str())
                    && step
                        .needs
                        .iter()
                        .all(|need| completed.contains(need.as_str()))
            })
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// The immutable identity of a deployed file: SHA-256 over the exact bytes.
pub fn content_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn default_timeout() -> u64 {
    1800
}

fn default_attempts() -> u32 {
    3
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
[workflow]
name = "triage-issue"
on = "github.issues.opened"
description = "Read, then fix"

[target]
tags = ["builder", "linux"]
sandbox = "modal"
timeout_secs = 1800

[target.options]
cpu = 4

[steps.triage]
prompt = "Read the issue and name the riskiest change."
grants = "ReadOnly"
secret_refs = ["github_token"]

[steps.fix]
prompt = "Implement the fix and open a draft PR."
grants = "ReadWrite"
needs = ["triage"]
approval = "always"
"#;

    #[test]
    fn a_valid_file_parses() {
        let workflow = WorkflowFile::parse(VALID).unwrap();
        assert_eq!(workflow.workflow.name, "triage-issue");
        assert_eq!(workflow.workflow.on, "github.issues.opened");
        assert_eq!(workflow.target.tags, vec!["builder", "linux"]);
        assert_eq!(workflow.target.sandbox, Sandbox::Modal);
        assert_eq!(workflow.target.timeout_secs, 1800);
        assert_eq!(workflow.target.max_attempts, 3);
        assert_eq!(workflow.steps.len(), 2);
        assert_eq!(workflow.steps["fix"].grants, Grant::ReadWrite);
        assert_eq!(workflow.steps["fix"].approval, Approval::Always);
        assert_eq!(workflow.steps["triage"].secret_refs, vec!["github_token"]);
    }

    #[test]
    fn defaults_keep_a_step_read_only_and_unapproved() {
        let workflow = WorkflowFile::parse(
            r#"
[workflow]
name = "minimal"
on = "manual"

[target]
tags = ["any"]

[steps.one]
prompt = "do the thing"
"#,
        )
        .unwrap();
        let step = &workflow.steps["one"];
        assert_eq!(step.grants, Grant::ReadOnly);
        assert_eq!(step.approval, Approval::Never);
        assert_eq!(workflow.target.sandbox, Sandbox::None);
        assert_eq!(workflow.target.max_attempts, 3);
    }

    #[test]
    fn an_empty_step_list_is_refused() {
        let error = WorkflowFile::parse(
            r#"
[workflow]
name = "empty"
on = "manual"
[target]
tags = ["any"]
"#,
        )
        .unwrap_err();
        assert!(matches!(error, WorkflowError::NoSteps));
    }

    #[test]
    fn an_unknown_dependency_is_refused() {
        let error = WorkflowFile::parse(
            r#"
[workflow]
name = "broken"
on = "manual"
[target]
tags = ["any"]
[steps.one]
prompt = "one"
needs = ["missing"]
"#,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            WorkflowError::UnknownDependency { step, dependency }
                if step == "one" && dependency == "missing"
        ));
    }

    #[test]
    fn a_cycle_is_refused() {
        let error = WorkflowFile::parse(
            r#"
[workflow]
name = "cycle"
on = "manual"
[target]
tags = ["any"]
[steps.one]
prompt = "one"
needs = ["three"]
[steps.two]
prompt = "two"
needs = ["one"]
[steps.three]
prompt = "three"
needs = ["two"]
"#,
        )
        .unwrap_err();
        assert!(matches!(error, WorkflowError::Cycle(_)));
    }

    #[test]
    fn a_self_cycle_is_refused() {
        let error = WorkflowFile::parse(
            r#"
[workflow]
name = "self"
on = "manual"
[target]
tags = ["any"]
[steps.one]
prompt = "one"
needs = ["one"]
"#,
        )
        .unwrap_err();
        assert!(matches!(error, WorkflowError::Cycle(_)));
    }

    #[test]
    fn empty_prompts_and_names_are_refused() {
        let error = WorkflowFile::parse(
            r#"
[workflow]
name = "empty-prompt"
on = "manual"
[target]
tags = ["any"]
[steps.one]
prompt = "   "
"#,
        )
        .unwrap_err();
        assert!(matches!(error, WorkflowError::EmptyPrompt(_)));

        let error = WorkflowFile::parse(
            r#"
[workflow]
name = "  "
on = "manual"
[target]
tags = ["any"]
[steps.one]
prompt = "one"
"#,
        )
        .unwrap_err();
        assert!(matches!(error, WorkflowError::EmptyName));
    }

    #[test]
    fn zero_timeout_and_zero_attempts_are_refused() {
        let timeout = VALID.replace("timeout_secs = 1800", "timeout_secs = 0");
        assert!(matches!(
            WorkflowFile::parse(&timeout).unwrap_err(),
            WorkflowError::InvalidTimeout
        ));
        let attempts = VALID.replace("timeout_secs = 1800", "timeout_secs = 1\nmax_attempts = 0");
        assert!(matches!(
            WorkflowFile::parse(&attempts).unwrap_err(),
            WorkflowError::InvalidAttempts
        ));
    }

    #[test]
    fn an_unknown_sandbox_is_refused_by_the_parser() {
        let unknown = VALID.replace("sandbox = \"modal\"", "sandbox = \"lambda\"");
        assert!(matches!(
            WorkflowFile::parse(&unknown),
            Err(WorkflowError::Toml(_))
        ));
    }

    #[test]
    fn ready_steps_follow_the_dependency_graph() {
        let workflow = WorkflowFile::parse(VALID).unwrap();
        let mut completed = HashSet::new();
        assert_eq!(workflow.ready_steps(&completed), vec!["triage"]);
        completed.insert("triage");
        assert_eq!(workflow.ready_steps(&completed), vec!["fix"]);
        completed.insert("fix");
        assert!(workflow.ready_steps(&completed).is_empty());
    }

    #[test]
    fn the_content_hash_is_stable_and_content_sensitive() {
        let hash = content_hash(VALID);
        assert_eq!(hash, content_hash(VALID));
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(hash, content_hash(&format!("{VALID}\n")));
    }

    #[test]
    fn the_grant_and_sandbox_names_are_stable_for_storage() {
        assert_eq!(Grant::ReadOnly.as_str(), "ReadOnly");
        assert_eq!(Grant::ReadWrite.as_str(), "ReadWrite");
        assert_eq!(Sandbox::E2b.as_str(), "e2b");
        assert_eq!(Sandbox::None.as_str(), "none");
        assert_eq!(Approval::Always.as_str(), "always");
    }
}
