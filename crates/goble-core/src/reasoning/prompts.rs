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
    // The reasoning tools are only this phase's. Without naming the harness
    // tools the turn executes with, a model reads the short list as its whole
    // capability and answers that it has no delegation tool at all.
    prompt.push_str("The reasoning tools are the planning phase only: execute hands the turn to the harness tools - create_agent, update_agent, create_workflow, create_team, run_command, read_file, write_file, edit_file, codebase_search, search_store, deploy_agent, deploy_workflow, schedule_workflow, memory_read, memory_write, install_mcp_server, discover_mcp_tools, web_search, run_agent and spawn_subagent. A capability that is not in the reasoning list is therefore not missing from the turn: plan the call and make it after execute instead of reporting that you cannot do it.\n");
    prompt.push_str("Rules:\n");
    prompt.push_str("- If you need more information, call ask_user.\n");
    prompt.push_str(
        "- If you need to keep reasoning, call continue_thinking or set_thinking_mode.\n",
    );
    prompt.push_str("- When you are ready to act (call tools, create agents/workflows, deploy, spawn sub-agents), call execute.\n");
    prompt.push_str("- To delegate work to a sub-agent, call execute and then spawn_subagent during execution, with description, subagent_type and prompt. run_in_background: false awaits the child and returns its output as this call's result (a child that outlives the wait moves to the background and the result is its id); run_in_background: true returns the child's id at once and the child keeps running. A child works on its own conversation, tool set and working directory, and may spawn children of its own until the depth limit is reached.\n");
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
    prompt.push_str("\nUse available tools to create agents, workflows, discover MCPs, install/connect MCPs, deploy to workers, schedule workflows, and delegate work to sub-agents with spawn_subagent (run_in_background: false awaits the child and returns its output as the result; run_in_background: true returns its id at once and the child keeps running).\n");
    // `ask_user` is a reasoning-phase tool; it is not in this phase's tool set,
    // so a model told to "ask the user instead" must reply, not call it.
    prompt.push_str("Only call a tool when you have enough information. If still missing info, reply with the question instead of calling a tool: ask_user belongs to the reasoning phase and is not available here.\n");
    prompt
}
