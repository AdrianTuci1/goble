use crate::llm::ToolDefinition;

pub fn build_reasoning_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "set_thinking_mode".to_string(),
            description: "Switch the thinking mode for the next reasoning step.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["direct", "contemplating", "ruminating", "baking", "reflecting", "verifying", "debugging", "synthesizing", "planning"] }
                },
                "required": ["mode"]
            }),
        },
        ToolDefinition {
            name: "continue_thinking".to_string(),
            description: "Continue reasoning for another step. Optionally provide a focus prompt."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "focus": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "execute".to_string(),
            description: "Stop reasoning and proceed to execute the plan or reply.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "ask_user".to_string(),
            description: "Ask the user for clarification before continuing.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "quick_replies": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["question"]
            }),
        },
        ToolDefinition {
            name: "create_mission".to_string(),
            description: "Create or update a mission tracking a complex goal.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "goal": { "type": "string" },
                    "status": { "type": "string", "enum": ["clarifying", "planning", "deploying", "running", "done", "error"] },
                    "plan": { "type": "string" },
                    "workflow_id": { "type": "string" }
                },
                "required": ["goal"]
            }),
        },
        ToolDefinition {
            name: "update_mission".to_string(),
            description: "Update mission status or plan.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "status": { "type": "string", "enum": ["clarifying", "planning", "deploying", "running", "done", "error"] },
                    "plan": { "type": "string" },
                    "workflow_id": { "type": "string" }
                },
                "required": ["id"]
            }),
        },
    ]
}
