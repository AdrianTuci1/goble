use crate::elements::terminal_block::TerminalData;
use goble_core::harness::ToolCallStatus;
use super::*;

#[test]
fn groups_inline_fragments_into_paragraph() {
    let fragments = vec![
        ChatFragment::text("Hello "),
        ChatFragment::bold("world"),
        ChatFragment::code("code"),
        ChatFragment::terminal(TerminalData::new(
            "cargo run",
            vec![crate::elements::terminal_block::TerminalLine::command("cargo run")],
        )),
        ChatFragment::text("Done"),
    ];
    let blocks = group_fragments_into_blocks(&fragments);
    assert_eq!(
        blocks,
        vec![
            ChatBlock::Paragraph(vec![
                InlineSpan::plain("Hello "),
                InlineSpan::bold("world"),
                InlineSpan::code("code"),
            ]),
            ChatBlock::Terminal(TerminalData::new(
                "cargo run",
                vec![crate::elements::terminal_block::TerminalLine::command("cargo run")],
            )),
            ChatBlock::Paragraph(vec![InlineSpan::plain("Done")]),
        ]
    );
}

#[test]
fn link_fragment_stays_in_the_paragraph() {
    let fragments = vec![
        ChatFragment::text("see "),
        ChatFragment::link("Goble", "https://goble.dev"),
        ChatFragment::text(" for details"),
    ];
    let blocks = group_fragments_into_blocks(&fragments);
    assert_eq!(
        blocks,
        vec![ChatBlock::Paragraph(vec![
            InlineSpan::plain("see "),
            InlineSpan::link("Goble", "https://goble.dev"),
            InlineSpan::plain(" for details"),
        ])]
    );
}

#[test]
fn heading_and_list_are_their_own_blocks() {
    let fragments = vec![
        ChatFragment::heading(1, "Title"),
        ChatFragment::list(
            vec![
                ListItem::new(vec![ChatFragment::text("a")]),
                ListItem::new(vec![ChatFragment::text("b")]),
            ],
            Some(3),
        ),
    ];
    let blocks = group_fragments_into_blocks(&fragments);
    assert_eq!(
        blocks,
        vec![
            ChatBlock::Heading {
                level: 1,
                text: "Title".to_string(),
            },
            ChatBlock::List {
                items: vec![
                    ListItem::new(vec![ChatFragment::text("a")]),
                    ListItem::new(vec![ChatFragment::text("b")]),
                ],
                start: Some(3),
            },
        ]
    );
}

#[test]
fn reasoning_fragment_is_its_own_block() {
    let fragments = vec![
        ChatFragment::text("answer: "),
        ChatFragment::reasoning("c1:0", "contemplating", "weighing options", false),
    ];
    let blocks = group_fragments_into_blocks(&fragments);
    assert_eq!(
        blocks,
        vec![
            ChatBlock::Paragraph(vec![InlineSpan::plain("answer: ")]),
            ChatBlock::Reasoning {
                key: "c1:0".to_string(),
                mode: "contemplating".to_string(),
                text: "weighing options".to_string(),
                done: false,
            },
        ]
    );
}

#[test]
fn tool_call_parses_harness_json() {
    let calls = ToolCall::from_llm_json(
        r#"[{"id":"call_1","name":"ls","arguments":{"path":"/tmp"},"status":"running"}]"#,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].name, "ls");
    assert!(calls[0].arguments.contains("/tmp"));
    assert_eq!(calls[0].status, ToolCallStatus::Running);
    assert_eq!(calls[0].result, None);

    // Malformed JSON is tolerated (empty list), never panics a transcript.
    assert!(ToolCall::from_llm_json("not json").is_empty());
    assert!(ToolCall::from_llm_json("").is_empty());
}

#[test]
fn tool_call_carries_status_and_result() {
    let calls = ToolCall::from_llm_json(
        r#"[{"id":"call_2","name":"credentials","arguments":{},"status":"finished","result":"no credentials stored"}]"#,
    );
    assert_eq!(calls[0].status, ToolCallStatus::Finished);
    assert_eq!(calls[0].result.as_deref(), Some("no credentials stored"));

    let failed = ToolCall::from_llm_json(
        r#"[{"id":"call_3","name":"no_such_tool","arguments":{},"status":"error","result":"unknown tool"}]"#,
    );
    assert_eq!(failed[0].status, ToolCallStatus::Error);
}

/// Rows persisted before the status carrier existed carry only
/// `id`/`name`/`arguments`; they must still parse.
#[test]
fn tool_call_row_from_older_build_still_parses() {
    let calls =
        ToolCall::from_llm_json(r#"[{"id":"call_1","name":"ls","arguments":{"path":"/tmp"}}]"#);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "ls");
    assert_eq!(calls[0].status, ToolCallStatus::Pending);
    assert_eq!(calls[0].result, None);
}
