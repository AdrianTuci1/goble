use std::path::PathBuf;

use serde_json::json;

use crate::agent_runtime::state::RuntimeState;

const MAX_READ_CHARS: usize = 100_000;

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub workspace_path: PathBuf,
    pub runtime_state: RuntimeState,
    pub console: Vec<String>,
}

impl ToolContext {
    pub fn new(workspace_path: PathBuf) -> Self {
        Self {
            workspace_path,
            runtime_state: RuntimeState::new(),
            console: Vec::new(),
        }
    }

    fn resolve_path(&self, path: &str) -> anyhow::Result<PathBuf> {
        let candidate = self.workspace_path.join(path);
        let canonical = candidate
            .canonicalize()
            .unwrap_or_else(|_| candidate.clone());
        let root = self
            .workspace_path
            .canonicalize()
            .unwrap_or_else(|_| self.workspace_path.clone());
        if canonical.starts_with(&root) {
            Ok(canonical)
        } else {
            Err(anyhow::anyhow!("path escapes workspace: {}", path))
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub output: String,
    pub state: Option<RuntimeState>,
    pub finished: bool,
    pub finish_summary: Option<String>,
}

impl ToolResult {
    pub fn new(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            state: None,
            finished: false,
            finish_summary: None,
        }
    }

    pub fn with_state(output: impl Into<String>, state: RuntimeState) -> Self {
        Self {
            output: output.into(),
            state: Some(state),
            finished: false,
            finish_summary: None,
        }
    }

    pub fn finish(summary: impl Into<String>) -> Self {
        Self {
            output: "finished".into(),
            state: None,
            finished: true,
            finish_summary: Some(summary.into()),
        }
    }
}

pub struct ToolRegistry;

impl ToolRegistry {
    pub fn definitions() -> Vec<goble_core::llm::ToolDefinition> {
        vec![
            Self::console_tool(),
            Self::read_file_tool(),
            Self::edit_file_tool(),
            Self::list_files_tool(),
            Self::thinking_tool(),
            Self::mull_tool(),
            Self::update_state_tool(),
            Self::self_improve_tool(),
            Self::finish_tool(),
        ]
    }

    pub fn execute(
        ctx: &mut ToolContext,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<ToolResult> {
        match name {
            "console" => Self::console(ctx, args),
            "read_file" => Self::read_file(ctx, args),
            "edit_file" => Self::edit_file(ctx, args),
            "list_files" => Self::list_files(ctx, args),
            "thinking" => Self::thinking(ctx, args),
            "mull" => Self::mull(ctx, args),
            "update_state" => Self::update_state(ctx, args),
            "self_improve" => Self::self_improve(ctx, args),
            "finish" => Self::finish(ctx, args),
            _ => Err(anyhow::anyhow!("unknown tool: {}", name)),
        }
    }

    fn console_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "console".into(),
            description: "Log a message to the runtime console.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": {"type": "string", "description": "message to log"}
                },
                "required": ["message"]
            }),
        }
    }

    fn read_file_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "read_file".into(),
            description: "Read the contents of a file within the workspace.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "relative path inside workspace"}
                },
                "required": ["path"]
            }),
        }
    }

    fn edit_file_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "edit_file".into(),
            description: "Replace old_string with new_string in a workspace file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "relative path inside workspace"},
                    "old_string": {"type": "string", "description": "exact text to replace"},
                    "new_string": {"type": "string", "description": "replacement text"}
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }

    fn list_files_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "list_files".into(),
            description: "List files in a workspace directory (default: workspace root).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "relative directory path, optional"}
                },
                "required": []
            }),
        }
    }

    fn thinking_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "thinking".into(),
            description: "Record a private thought in the execution trace.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "thought": {"type": "string", "description": "thought text"}
                },
                "required": ["thought"]
            }),
        }
    }

    fn mull_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "mull".into(),
            description: "Add a note to the runtime state for reflection.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "topic": {"type": "string", "description": "note text"}
                },
                "required": ["topic"]
            }),
        }
    }

    fn update_state_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "update_state".into(),
            description:
                "Update the runtime state: add a checklist item, mark one done, or add a note."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "add_checklist": {"type": "string", "description": "text for a new checklist item"},
                    "mark_done": {"type": "string", "description": "id of checklist item to mark done"},
                    "add_note": {"type": "string", "description": "note text to append"}
                },
                "required": []
            }),
        }
    }

    fn self_improve_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "self_improve".into(),
            description: "Record self-feedback to improve behavior in future runs.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "feedback": {"type": "string", "description": "self-feedback text"}
                },
                "required": ["feedback"]
            }),
        }
    }

    fn finish_tool() -> goble_core::llm::ToolDefinition {
        goble_core::llm::ToolDefinition {
            name: "finish".into(),
            description: "Signal that the agent has finished its work.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "summary": {"type": "string", "description": "final summary"}
                },
                "required": ["summary"]
            }),
        }
    }

    fn console(ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing message"))?;
        ctx.console.push(message.to_string());
        Ok(ToolResult::new("ok"))
    }

    fn read_file(ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let resolved = ctx.resolve_path(path)?;
        let content = std::fs::read_to_string(&resolved)?;
        if content.len() > MAX_READ_CHARS {
            let truncated = format!(
                "{}\n\n[truncated; {} total characters]",
                &content[..MAX_READ_CHARS],
                content.len()
            );
            return Ok(ToolResult::new(truncated));
        }
        Ok(ToolResult::new(content))
    }

    fn edit_file(ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing old_string"))?;
        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing new_string"))?;
        let resolved = ctx.resolve_path(path)?;
        if old_string.is_empty() {
            if let Some(parent) = resolved.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&resolved, new_string)?;
            return Ok(ToolResult::new("created"));
        }
        let content = std::fs::read_to_string(&resolved)?;
        if !content.contains(old_string) {
            return Err(anyhow::anyhow!(
                "old_string not found in file: {}",
                old_string.chars().take(80).collect::<String>()
            ));
        }
        let updated = content.replacen(old_string, new_string, 1);
        std::fs::write(&resolved, updated)?;
        Ok(ToolResult::new("ok"))
    }

    fn list_files(ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let rel = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let resolved = ctx.resolve_path(rel)?;
        let mut entries: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&resolved)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if entry.file_type()?.is_dir() {
                "dir"
            } else {
                "file"
            };
            entries.push(format!("{} {}", kind, name));
        }
        entries.sort();
        Ok(ToolResult::new(entries.join("\n")))
    }

    fn thinking(ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let thought = args
            .get("thought")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing thought"))?;
        ctx.console.push(format!("[thinking] {}", thought));
        Ok(ToolResult::new("ok"))
    }

    fn mull(ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let topic = args
            .get("topic")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing topic"))?;
        ctx.runtime_state.add_note(topic);
        Ok(ToolResult::with_state("noted", ctx.runtime_state.clone()))
    }

    fn update_state(ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        if let Some(text) = args.get("add_checklist").and_then(|v| v.as_str()) {
            ctx.runtime_state.add_checklist(text);
        }
        if let Some(id) = args.get("mark_done").and_then(|v| v.as_str()) {
            if !ctx.runtime_state.mark_done(id) {
                return Ok(ToolResult::with_state(
                    format!("checklist item {} not found", id),
                    ctx.runtime_state.clone(),
                ));
            }
        }
        if let Some(text) = args.get("add_note").and_then(|v| v.as_str()) {
            ctx.runtime_state.add_note(text);
        }
        Ok(ToolResult::with_state("ok", ctx.runtime_state.clone()))
    }

    fn self_improve(ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let feedback = args
            .get("feedback")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing feedback"))?;
        ctx.runtime_state.add_self_feedback(feedback);
        Ok(ToolResult::with_state("noted", ctx.runtime_state.clone()))
    }

    fn finish(_ctx: &mut ToolContext, args: &serde_json::Value) -> anyhow::Result<ToolResult> {
        let summary = args
            .get("summary")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing summary"))?;
        Ok(ToolResult::finish(summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_read_file_tool() {
        let tmp = tempdir().unwrap();
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        let result =
            ToolRegistry::execute(&mut ctx, "read_file", &json!({"path": "a.txt"})).unwrap();
        assert_eq!(result.output, "hello");
    }

    #[test]
    fn test_edit_file_tool() {
        let tmp = tempdir().unwrap();
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        std::fs::write(tmp.path().join("a.txt"), "hello world").unwrap();
        ToolRegistry::execute(
            &mut ctx,
            "edit_file",
            &json!({"path": "a.txt", "old_string": "hello", "new_string": "hi"}),
        )
        .unwrap();
        let content = std::fs::read_to_string(tmp.path().join("a.txt")).unwrap();
        assert_eq!(content, "hi world");
    }

    #[test]
    fn test_list_files_tool() {
        let tmp = tempdir().unwrap();
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        std::fs::write(tmp.path().join("a.txt"), "").unwrap();
        std::fs::create_dir(tmp.path().join("b")).unwrap();
        let result = ToolRegistry::execute(&mut ctx, "list_files", &json!({})).unwrap();
        assert!(result.output.contains("file a.txt"));
        assert!(result.output.contains("dir b"));
    }

    #[test]
    fn test_console_and_state_tools() {
        let tmp = tempdir().unwrap();
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        ToolRegistry::execute(&mut ctx, "console", &json!({"message": "hi"})).unwrap();
        assert_eq!(ctx.console, vec!["hi"]);

        let result = ToolRegistry::execute(
            &mut ctx,
            "update_state",
            &json!({"add_checklist": "read file", "add_note": "note one"}),
        )
        .unwrap();
        let mut state = result.state.unwrap();
        assert_eq!(state.checklist.len(), 1);
        let id = state.checklist[0].id.clone();
        assert!(state.mark_done(&id));
        assert_eq!(state.notes, vec!["note one"]);
    }

    #[test]
    fn test_path_escape_rejected() {
        let tmp = tempdir().unwrap();
        let mut ctx = ToolContext::new(tmp.path().to_path_buf());
        std::fs::write("/tmp/escape-target.txt", "outside").unwrap();
        let result = ToolRegistry::execute(
            &mut ctx,
            "read_file",
            &json!({"path": "../escape-target.txt"}),
        );
        assert!(result.is_err());
    }
}
