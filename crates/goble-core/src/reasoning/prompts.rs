use crate::harness::ThinkingMode;

use super::types::ReasoningStep;

pub(super) fn build_reasoning_prompt(
    mode: ThinkingMode,
    goal: &str,
    steps: &[ReasoningStep],
) -> String {
    let mut prompt = format!(
        "You are in thinking mode: {}.\n{}\n\n",
        mode.as_str(),
        mode.prompt()
    );
    prompt.push_str("You are orchestrating a complex task. You may call reasoning tools: set_thinking_mode, continue_thinking, execute, ask_user, create_mission, update_mission.\n");
    prompt.push_str("Rules:\n");
    prompt.push_str("- If you need more information, call ask_user.\n");
    prompt.push_str(
        "- If you need to keep reasoning, call continue_thinking or set_thinking_mode.\n",
    );
    prompt.push_str("- When you are ready to act (call tools, create agents/workflows, deploy), call execute.\n");
    prompt.push_str("- Track the overall goal using create_mission / update_mission.\n\n");
    prompt.push_str(&format!("Current goal: {goal}\n\n"));
    if !steps.is_empty() {
        prompt.push_str("Previous reasoning steps:\n");
        for step in steps.iter().rev().take(4) {
            prompt.push_str(&format!(
                "- [{}] {} -> {}\n",
                step.step,
                step.mode.as_str(),
                serde_json::to_string(&step.decision).unwrap_or_default()
            ));
        }
        prompt.push('\n');
    }
    prompt
}

pub(super) fn build_execution_prompt(goal: &str, reasoning_steps: &[ReasoningStep]) -> String {
    let mut prompt = format!("You are now executing. The goal is: {goal}\n\nReasoning summary:\n");
    for step in reasoning_steps.iter().rev().take(6) {
        let content = step.content.chars().take(200).collect::<String>();
        prompt.push_str(&format!(
            "- [{}] {}: {}\n",
            step.step,
            step.mode.as_str(),
            content
        ));
    }
    prompt.push_str("\nUse available tools to create agents, workflows, discover MCPs, install/connect MCPs, deploy to workers, and schedule workflows.\n");
    prompt.push_str("Only call a tool when you have enough information. If still missing info, ask the user instead.\n");
    prompt
}
