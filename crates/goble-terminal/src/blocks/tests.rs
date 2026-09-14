use super::*;

use crate::hooks::{
    BootstrappedValue, CommandFinishedValue, HookEvent, InitShellValue, InputBufferValue,
    PrecmdValue, PreexecValue,
};
use crate::screen::ScreenSize;

fn size() -> ScreenSize {
    ScreenSize::new(40, 6)
}

fn init_shell() -> HookEvent {
    HookEvent::InitShell(InitShellValue {
        session_id: Some("s1".into()),
        shell: Some("zsh".into()),
        host: Some("mac".into()),
        cwd: Some("/work".into()),
        honor_ps1: Some(false),
    })
}

fn precmd(cwd: &str) -> HookEvent {
    HookEvent::Precmd(PrecmdValue {
        pwd: Some(cwd.into()),
        git_branch: Some("main".into()),
        rprompt: None,
        session_id: Some("s1".into()),
        virtual_env: None,
        conda_env: None,
        node_version: None,
        honor_ps1: Some(false),
    })
}

fn preexec(command: &str) -> HookEvent {
    HookEvent::Preexec(PreexecValue {
        command: Some(command.into()),
    })
}

fn finished(exit_code: i32) -> HookEvent {
    HookEvent::CommandFinished(CommandFinishedValue {
        exit_code,
        next_block_id: None,
    })
}

fn agent_owner() -> BlockOwner {
    BlockOwner::Agent {
        conversation_id: "conv-1".into(),
        call_id: "call-1".into(),
    }
}

/// A list that has bootstrapped and drawn its first prompt.
fn booted() -> BlockList {
    let mut list = BlockList::new(size());
    list.apply(init_shell());
    list.apply(HookEvent::Bootstrapped(BootstrappedValue {
        version: Some("1".into()),
    }));
    list.apply(precmd("/work"));
    list
}

#[test]
fn starts_with_a_static_preamble() {
    let mut list = BlockList::new(size());
    assert_eq!(list.len(), 1);
    assert_eq!(list.active_block().state(), BlockState::Static);
    list.feed(b"Last login: today\n");
    assert_eq!(list.active_block().text(), "Last login: today");
    assert_eq!(list.active_block().command_text(), "");
}

#[test]
fn bootstrapping_starts_the_first_command_block() {
    let mut list = BlockList::new(size());
    list.feed(b"banner\n");
    list.apply(init_shell());
    let events = list.apply(HookEvent::Bootstrapped(BootstrappedValue::default()));
    assert_eq!(events, vec![BlockEvent::Bootstrapped]);
    assert!(list.bootstrapped());
    assert_eq!(list.len(), 2);
    assert_eq!(list.active_block().state(), BlockState::BeforeExecution);
    // The banner stayed behind in the preamble.
    assert_eq!(list.text(), "banner");
    // Bootstrapping twice does not add a block.
    assert!(list
        .apply(HookEvent::Bootstrapped(BootstrappedValue::default()))
        .is_empty());
    assert_eq!(list.len(), 2);
}

#[test]
fn runs_a_command_from_prompt_to_exit_code() {
    let mut list = booted();
    assert_eq!(list.session().cwd.as_deref(), Some("/work"));

    // The shell's line editor echoes what is typed, then the command runs.
    list.feed(b"$ ls -la\r\n");
    let events = list.apply(preexec("ls -la"));
    let id = list.active_block().id();
    assert_eq!(events, vec![BlockEvent::BlockStarted { id }]);
    assert_eq!(list.active_block().state(), BlockState::Executing);
    assert_eq!(list.active_block().command_text(), "ls -la");

    list.feed(b"total 0\r\nfile.txt\r\n");
    let events = list.apply(finished(0));

    assert_eq!(
        events,
        vec![BlockEvent::BlockFinished {
            id,
            exit_code: 0,
            failed: false
        }]
    );
    let block = list.get(id).expect("the finished block");
    assert_eq!(block.state(), BlockState::DoneWithExecution);
    assert_eq!(block.exit_code(), Some(0));
    assert!(!block.has_failed());
    assert_eq!(block.text(), "$ ls -la\ntotal 0\nfile.txt");
    assert!(block.duration().is_some());
    // A new block is already active, so the next prompt has somewhere to go.
    assert_eq!(list.active_block().state(), BlockState::BeforeExecution);
    assert_ne!(list.active_block().id(), id);
}

#[test]
fn a_non_zero_exit_code_is_a_failure() {
    let mut list = booted();
    list.apply(preexec("false"));
    list.apply(finished(1));
    assert!(list.blocks()[1].has_failed());
    assert_eq!(list.blocks()[1].exit_code(), Some(1));
}

#[test]
fn a_claimed_preexec_takes_the_owner() {
    let mut list = booted();
    let id = list.active_block().id();
    assert!(list.claim_next_pre_exec(id, agent_owner()));
    assert_eq!(list.pending_claim().map(|claim| claim.block), Some(id));

    list.apply(preexec("ls"));
    assert_eq!(list.active_block().owner(), &agent_owner());
    assert!(list.pending_claim().is_none(), "the claim was consumed");

    // The next unclaimed command on the next block is the user's again.
    list.apply(finished(0));
    list.apply(preexec("pwd"));
    assert_eq!(list.active_block().owner(), &BlockOwner::User);
}

#[test]
fn an_unclaimed_preexec_belongs_to_the_user() {
    let mut list = booted();
    list.apply(preexec("ls"));
    assert_eq!(list.active_block().owner(), &BlockOwner::User);
}

#[test]
fn a_claim_aimed_at_the_wrong_block_is_refused() {
    let mut list = booted();
    // Run one command so there is an earlier, no-longer-active block.
    list.apply(preexec("echo one"));
    list.apply(finished(0));
    let earlier = list.blocks()[1].id();
    let active = list.active_block().id();
    assert_ne!(earlier, active);

    assert!(
        !list.claim_next_pre_exec(earlier, agent_owner()),
        "a finished block cannot be claimed"
    );
    assert!(
        !list.claim_next_pre_exec(BlockId(999), agent_owner()),
        "an unknown block cannot be claimed"
    );
    assert!(list.pending_claim().is_none(), "nothing was accepted");

    // The refusal left the command that runs here as the user's.
    list.apply(preexec("pwd"));
    assert_eq!(list.get(active).unwrap().owner(), &BlockOwner::User);
}

#[test]
fn a_claim_that_lands_on_another_block_is_refused() {
    let mut list = booted();
    let claimed = list.active_block().id();
    assert!(list.claim_next_pre_exec(claimed, agent_owner()));

    // The shell moved on before the claimed command ran, so the `Preexec`
    // that arrives belongs to a different block and the claim is refused.
    list.apply(finished(0));
    let other = list.active_block().id();
    assert_ne!(other, claimed);
    list.apply(preexec("pwd"));
    assert_eq!(list.get(other).unwrap().owner(), &BlockOwner::User);
    assert_eq!(list.get(claimed).unwrap().owner(), &BlockOwner::User);
    assert!(list.pending_claim().is_none());
}

/// Claim the active block and start its command, as P2's protocol does.
fn claimed_command(list: &mut BlockList, command: &str) -> BlockId {
    let id = list.active_block().id();
    assert!(list.claim_next_pre_exec(id, agent_owner()));
    list.apply(preexec(command));
    id
}

fn tool_result_of(events: &[BlockEvent]) -> &ToolResult {
    events
        .iter()
        .find_map(|event| match event {
            BlockEvent::ToolResult(result) => Some(result),
            _ => None,
        })
        .expect("a tool result for the claimed block")
}

#[test]
fn a_claimed_command_hands_back_its_output_and_exit_code() {
    let mut list = booted();
    let id = claimed_command(&mut list, "ls -la");
    list.feed(b"file.txt\r\n");
    let events = list.apply(finished(0));

    // The renderer's event is still there; the result rides alongside it.
    assert!(events.iter().any(|event| matches!(
        event,
        BlockEvent::BlockFinished { id: block, exit_code: 0, .. } if *block == id
    )));
    let result = tool_result_of(&events);
    assert_eq!(result.block, id);
    assert_eq!(result.conversation_id, "conv-1");
    assert_eq!(result.call_id, "call-1");
    assert_eq!(result.command, "ls -la");
    assert_eq!(result.output, "file.txt");
    assert_eq!(result.outcome, ToolOutcome::Finished { exit_code: 0 });
    assert_eq!(result.exit_code(), Some(0));
    assert!(!result.is_failure());
    assert!(!result.is_still_running());
}

#[test]
fn a_claimed_command_that_fails_hands_back_its_exit_code() {
    let mut list = booted();
    claimed_command(&mut list, "false");
    list.feed(b"boom\r\n");
    let events = list.apply(finished(2));

    let result = tool_result_of(&events);
    assert_eq!(result.output, "boom");
    assert_eq!(result.exit_code(), Some(2));
    assert!(result.is_failure());
}

#[test]
fn a_claimed_command_with_no_output_hands_back_an_empty_result() {
    let mut list = booted();
    claimed_command(&mut list, "true");
    let events = list.apply(finished(0));

    let result = tool_result_of(&events);
    assert_eq!(result.output, "");
    assert_eq!(result.outcome, ToolOutcome::Finished { exit_code: 0 });
    assert!(!result.is_failure());
}

#[test]
fn an_interrupted_claimed_command_fails_instead_of_hanging() {
    let mut list = booted();
    claimed_command(&mut list, "sleep 100");
    list.feed(b"partial\r\n");
    // Ctrl-C: the shell still reports the command, with exit code 130.
    let events = list.apply(finished(130));

    let result = tool_result_of(&events);
    assert_eq!(result.output, "partial");
    assert_eq!(result.exit_code(), Some(130));
    assert!(result.is_failure());
    assert!(!result.is_still_running());
}

#[test]
fn a_promoted_claimed_command_hands_back_a_still_running_result() {
    let mut list = booted();
    let id = claimed_command(&mut list, "sleep 100 &");
    list.feed(b"starting\r\n");

    // A prompt while the claimed command still runs: no exit code is coming,
    // so the result is handed back now rather than after a hook that never
    // arrives.
    let events = list.apply(precmd("/work"));
    let result = tool_result_of(&events);
    assert_eq!(result.block, id);
    assert_eq!(result.command, "sleep 100 &");
    assert_eq!(result.output, "starting");
    assert_eq!(result.outcome, ToolOutcome::StillRunning);
    assert!(result.is_still_running());
    assert_eq!(result.exit_code(), None, "no exit code is coming");
    assert!(!result.is_failure());

    // The promoted block is terminal: the finish that belongs to the next
    // block does not produce a second, stale result for the same call.
    let events = list.apply(finished(0));
    assert!(!events
        .iter()
        .any(|event| matches!(event, BlockEvent::ToolResult(_))));
}

#[test]
fn a_users_command_produces_no_tool_result() {
    let mut list = booted();
    list.apply(preexec("ls"));
    list.feed(b"file.txt\r\n");
    let events = list.apply(finished(0));
    assert!(!events
        .iter()
        .any(|event| matches!(event, BlockEvent::ToolResult(_))));
}

#[test]
fn a_tool_result_carries_output_that_scrolled_past_the_screen() {
    let mut list = booted();
    claimed_command(&mut list, "seq 1 10");
    let output: String = (1..=10).map(|line| format!("{line}\r\n")).collect();
    list.feed(output.as_bytes());
    let events = list.apply(finished(0));

    // Ten rows in a six-line screen: the result is the whole output, not
    // only the rows still visible.
    let result = tool_result_of(&events);
    assert_eq!(result.output, "1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
}

#[test]
fn metadata_is_recorded_on_the_block() {
    let list = booted();
    let block = list.active_block();
    assert_eq!(block.metadata().pwd.as_deref(), Some("/work"));
    assert_eq!(block.metadata().git_branch.as_deref(), Some("main"));
    assert!(!block.honor_ps1());
}

#[test]
fn output_before_a_command_belongs_to_the_command_line() {
    let mut list = booted();
    // The shell echoes the typed line before the command runs.
    list.feed(b"$ ls");
    assert_eq!(list.active_block().command().text(), "$ ls");
    assert_eq!(list.active_block().output().text(), "");

    list.apply(preexec("ls"));
    list.feed(b"file.txt");
    assert_eq!(list.active_block().command().text(), "$ ls");
    assert_eq!(list.active_block().output().text(), "file.txt");
}

#[test]
fn a_finished_block_never_grows_again() {
    let mut list = booted();
    let id = list.active_block().id();
    list.apply(preexec("echo hi"));
    list.feed(b"hi\r\n");
    list.apply(finished(0));
    let before = list.get(id).unwrap().text();

    // The next prompt, and output from a background job, arrive afterwards.
    list.feed(b"next prompt");
    assert_eq!(list.get(id).unwrap().text(), before);
    assert_eq!(
        list.get(id).unwrap().dropped_bytes(),
        0,
        "routed to the new block"
    );
    assert_eq!(list.active_block().command().text(), "next prompt");
}

#[test]
fn a_frozen_block_keeps_the_shell_out_of_it() {
    let mut list = booted();
    let id = list.active_block().id();
    list.apply(preexec("printf out"));
    list.feed(b"out");
    list.apply(finished(0));
    let block = list.get(id).unwrap();
    assert!(block.output().is_frozen());
    assert_eq!(block.output().text(), "out");
    assert_eq!(block.output_lines().len(), 1);
}

#[test]
fn a_block_keeps_output_taller_than_the_screen() {
    let mut list = booted();
    let id = list.active_block().id();
    list.apply(preexec("seq 1 50"));
    let output: String = (1..=50).map(|line| format!("{line}\r\n")).collect();
    list.feed(output.as_bytes());
    list.apply(finished(0));

    let block = list.get(id).unwrap();
    assert_eq!(
        block.output_lines().len(),
        50,
        "every row the command printed"
    );
    assert_eq!(
        block.output().history_size(),
        45,
        "50 rows plus the newline, in a 6 line screen"
    );
    assert_eq!(block.output_lines()[0].text(), "1");
    assert_eq!(block.output_lines()[49].text(), "50");
}

#[test]
fn a_prompt_during_execution_promotes_the_command_to_background() {
    let mut list = booted();
    let running = list.active_block().id();
    list.apply(preexec("sleep 100 &"));

    let events = list.apply(precmd("/work"));
    let promoted = list.get(running).unwrap();
    assert_eq!(promoted.state(), BlockState::Background);
    assert!(promoted.exit_code().is_none());
    assert_eq!(events.len(), 1);
    assert_ne!(list.active_block().id(), running);
    assert_eq!(list.active_block().state(), BlockState::BeforeExecution);
}

#[test]
fn an_empty_prompt_is_done_with_no_execution() {
    let mut list = booted();
    list.apply(preexec(""));
    list.apply(finished(0));
    assert_eq!(list.blocks()[1].state(), BlockState::DoneWithNoExecution);
    // Output without a command is still an execution.
    let mut list = booted();
    list.apply(preexec(""));
    list.feed(b"something");
    list.apply(finished(0));
    assert_eq!(list.blocks()[1].state(), BlockState::DoneWithExecution);
}

#[test]
fn clear_drops_earlier_blocks_and_marks_a_gap() {
    let mut list = booted();
    for index in 0..3 {
        list.apply(preexec(&format!("echo {index}")));
        list.feed(format!("{index}\r\n").as_bytes());
        list.apply(finished(0));
    }
    assert_eq!(list.len(), 5);

    list.apply(preexec("clear"));
    list.feed(b"stale output");
    let events = list.apply(HookEvent::Clear);
    assert_eq!(events, vec![BlockEvent::Cleared { removed: 4 }]);
    assert_eq!(list.len(), 1);
    assert_eq!(list.active_index(), 0);
    // The `clear` command itself stays visible; what it printed does not.
    assert_eq!(list.active_block().command_text(), "clear");
    assert_eq!(list.active_block().output().text(), "");
    assert_eq!(list.gaps(), &[list.active_block().id()]);

    // The block keeps running and can still finish normally.
    list.apply(finished(0));
    assert_eq!(list.blocks()[0].state(), BlockState::DoneWithExecution);
}

#[test]
fn input_buffer_is_reported() {
    let mut list = booted();
    let events = list.apply(HookEvent::InputBuffer(InputBufferValue {
        buffer: "git st".into(),
        cursor: Some(6),
    }));
    assert_eq!(
        events,
        vec![BlockEvent::InputBufferChanged {
            buffer: "git st".into(),
            cursor: Some(6)
        }]
    );
    assert_eq!(list.input_buffer(), "git st");
    assert_eq!(list.input_cursor(), Some(6));
}

#[test]
fn resize_reaches_every_block() {
    let mut list = booted();
    list.apply(preexec("echo wrap"));
    list.feed(b"a rather long line that has to wrap once the screen is narrow");
    list.apply(finished(0));
    list.resize(ScreenSize::new(20, 4));
    for block in list.blocks() {
        assert_eq!(block.command().size(), ScreenSize::new(20, 4));
        assert_eq!(block.output().size(), ScreenSize::new(20, 4));
    }
    let block = list.get(list.blocks()[1].id()).unwrap();
    assert!(
        block.text().contains("wrap"),
        "text after reflow: {:?}",
        block.text()
    );
}

#[test]
fn lines_are_tagged_with_their_block() {
    let mut list = booted();
    list.feed(b"$ echo one\r\n");
    list.apply(preexec("echo one"));
    list.feed(b"one\r\n");
    list.apply(finished(0));
    let lines = list.lines();
    assert_eq!(lines.len(), list.total_lines());
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].line.text(), "$ echo one");
    assert_eq!(lines[0].block, list.blocks()[1].id());
    assert_eq!(lines[1].line.text(), "one");
    assert_eq!(list.text(), "$ echo one\none");
}

#[test]
fn unknown_hook_orders_do_not_panic() {
    // Hooks can arrive out of order after a shell reload; nothing here may
    // panic or invent a block.
    let mut list = BlockList::new(size());
    list.apply(finished(0));
    list.apply(preexec("orphan"));
    list.apply(HookEvent::Clear);
    list.apply(precmd("/"));
    list.feed(b"still alive");
    assert!(!list.is_empty());
    assert_eq!(list.active_index(), list.len() - 1);
}

fn ids(blocks: Vec<&Block>) -> Vec<BlockId> {
    blocks.iter().map(|block| block.id()).collect()
}

#[test]
fn a_conversation_block_sits_between_shell_blocks() {
    let mut list = booted();
    list.apply(preexec("echo before"));
    list.feed(b"before\r\n");
    list.apply(finished(0));
    let shell = list.active_block().id();

    let card = list.push_agent_view_block("conv-1", "Fix the build");

    // The card does not steal the active block: the shell keeps the screen.
    assert_eq!(list.active_block().id(), shell);
    let block = list.get(card).expect("the card");
    assert_eq!(block.conversation_id(), Some("conv-1"));
    assert_eq!(block.label(), Some("Fix the build"));
    assert!(!block.kind().is_shell());
    assert_eq!(block.text(), "", "a conversation block holds no output");

    // The next command lands after the card, so the card keeps its place in
    // the history.
    list.apply(preexec("echo after"));
    list.feed(b"after\r\n");
    list.apply(finished(0));
    let after = list.active_block().id();
    let order: Vec<BlockId> = ids(list.blocks().iter().collect());
    let at = |id: BlockId| order.iter().position(|candidate| *candidate == id).unwrap();
    assert!(at(shell) < at(card));
    assert!(at(card) < at(after));
    assert_eq!(list.get(card).unwrap().text(), "");
}

#[test]
fn a_conversation_block_never_takes_pty_bytes() {
    let mut list = booted();
    let card = list.push_agent_view_block("conv-1", "Fix the build");
    // Even mid-command, bytes belong to the shell block.
    list.apply(preexec("sleep 5"));
    list.feed(b"partial output");
    assert_eq!(list.get(card).unwrap().text(), "");
    assert_eq!(list.get(card).unwrap().dropped_bytes(), 0);
    assert_eq!(list.active_block().output().text(), "partial output");
}

#[test]
fn each_view_sees_only_its_own_blocks() {
    let mut list = booted();
    list.apply(preexec("echo shell"));
    list.feed(b"shell\r\n");
    list.apply(finished(0));
    let shell = list.blocks()[1].id();

    let card = list.push_agent_view_block("conv-1", "Fix the build");

    let conversation = BlockView::Agent {
        conversation_id: "conv-1".into(),
    };
    // Nothing is in the conversation yet, and its own card is hidden while
    // it is open — the view is the conversation.
    assert!(list.visible_blocks(&conversation).is_empty());
    assert!(list.visible_lines(&conversation).is_empty());
    assert!(ids(list.visible_blocks(&BlockView::Terminal)).contains(&card));

    // A command run inside the conversation shows in both views.
    assert!(list.associate_with_conversation(shell, "conv-1"));
    assert_eq!(ids(list.visible_blocks(&conversation)), vec![shell]);
    assert!(ids(list.visible_blocks(&BlockView::Terminal)).contains(&shell));
    let lines = list.visible_lines(&conversation);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].line.text(), "shell");
    assert_eq!(lines[0].block, shell);

    // Another conversation has nothing of its own, and still sees its card
    // in the terminal.
    let other = BlockView::Agent {
        conversation_id: "conv-2".into(),
    };
    assert!(list.visible_blocks(&other).is_empty());
    assert!(ids(list.visible_blocks(&BlockView::Terminal)).contains(&card));
    assert_eq!(list.visible_lines(&BlockView::Terminal)[0], list.lines()[0]);
}

#[test]
fn associating_a_block_hides_it_from_the_terminal_when_asked() {
    let mut list = booted();
    list.apply(preexec("echo one"));
    let id = list.active_block().id();

    assert!(list.associate_with_conversation(id, "conv-1"));
    let view = BlockView::Agent {
        conversation_id: "conv-1".into(),
    };
    assert!(list.visible_blocks(&view).iter().any(|b| b.id() == id));
    assert!(
        list.visible_blocks(&BlockView::Terminal)
            .iter()
            .any(|b| b.id() == id),
        "a command run in the conversation is still terminal output"
    );

    assert!(list.set_visibility(id, BlockVisibility::agent("conv-1")));
    assert!(!list
        .visible_blocks(&BlockView::Terminal)
        .iter()
        .any(|b| b.id() == id));
    assert!(
        list.visible_blocks(&view).iter().any(|b| b.id() == id),
        "hidden from the terminal, kept in the conversation"
    );

    assert!(!list.associate_with_conversation(BlockId(999), "conv-1"));
    assert!(!list.set_visibility(BlockId(999), BlockVisibility::terminal()));
}

#[test]
fn ids_are_unique_and_ordered() {
    let mut list = booted();
    let mut ids: Vec<BlockId> = list.blocks().iter().map(Block::id).collect();
    for _ in 0..3 {
        list.apply(preexec("true"));
        list.apply(finished(0));
        ids.push(list.active_block().id());
    }
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "ids repeat: {ids:?}");
}
