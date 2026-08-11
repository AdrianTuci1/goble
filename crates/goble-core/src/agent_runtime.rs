use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const RUNTIME_STATE_FILE: &str = ".runtime_state.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeState {
    pub version: u32,
    pub checklist: Vec<ChecklistItem>,
    pub notes: Vec<String>,
    pub self_feedback: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub id: String,
    pub text: String,
    pub done: bool,
}

impl ChecklistItem {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            text: text.into(),
            done: false,
        }
    }
}

impl RuntimeState {
    pub fn new() -> Self {
        Self {
            version: 1,
            checklist: Vec::new(),
            notes: Vec::new(),
            self_feedback: Vec::new(),
        }
    }

    pub fn load(workspace_path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = Self::state_path(&workspace_path);
        if !path.exists() {
            return Ok(Self::new());
        }
        let data = std::fs::read_to_string(&path)?;
        let mut state: Self = serde_json::from_str(&data)?;
        if state.version == 0 {
            state.version = 1;
        }
        Ok(state)
    }

    pub fn save(&self, workspace_path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = Self::state_path(&workspace_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn add_checklist(&mut self, text: impl Into<String>) -> String {
        let item = ChecklistItem::new(text);
        let id = item.id.clone();
        self.checklist.push(item);
        id
    }

    pub fn mark_done(&mut self, id: &str) -> bool {
        if let Some(item) = self.checklist.iter_mut().find(|i| i.id == id) {
            item.done = true;
            true
        } else {
            false
        }
    }

    pub fn add_note(&mut self, text: impl Into<String>) {
        self.notes.push(text.into());
    }

    pub fn add_self_feedback(&mut self, text: impl Into<String>) {
        self.self_feedback.push(text.into());
    }

    fn state_path(workspace_path: impl AsRef<Path>) -> PathBuf {
        workspace_path.as_ref().join(RUNTIME_STATE_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_runtime_state_checklist() {
        let mut state = RuntimeState::new();
        let id = state.add_checklist("read file");
        assert!(state.mark_done(&id));
        assert!(state.checklist[0].done);
        assert!(!state.mark_done("missing"));
    }

    #[test]
    fn test_runtime_state_persistence() {
        let tmp = tempdir().unwrap();
        let mut state = RuntimeState::new();
        let id = state.add_checklist("write tests");
        state.add_note("note one");
        state.add_self_feedback("avoid infinite loops");
        state.save(tmp.path()).unwrap();

        let loaded = RuntimeState::load(tmp.path()).unwrap();
        assert_eq!(loaded.checklist.len(), 1);
        assert_eq!(loaded.checklist[0].id, id);
        assert_eq!(loaded.notes, vec!["note one"]);
        assert_eq!(loaded.self_feedback, vec!["avoid infinite loops"]);
    }
}
