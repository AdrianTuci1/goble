# Agent 10 — Warp Chat & Composer Patterns (warp-new comparison)

## Scope
Second pass over `~/Projects/warp-new` to understand exactly how Warp renders chat content and how the composer handles custom widgets (API-key selector, model/environment variant selectors).

## Chat formatting is a document model, not plain text

Warp's chat messages are rendered from a `markdown_parser::FormattedText` value, not from a raw string.

Key types (`crates/markdown_parser/src/lib.rs`):

- `FormattedText { lines: VecDeque<FormattedTextLine> }`
- `FormattedTextLine` variants:
  - `Heading`, `Line` (paragraph), `OrderedList`, `UnorderedList`
  - `CodeBlock`, `TaskList`, `LineBreak`, `HorizontalRule`
  - `Embedded(Mapping)` — YAML-backed inline object
  - `Image`, `Table`
- `FormattedTextInline = Vec<FormattedTextFragment>`
- `FormattedTextFragment { text: String, styles: FormattedTextStyles }`
- `FormattedTextStyles` carries weight, italic, underline, strikethrough, inline-code, and an optional hyperlink.

## Custom actions are first-class hyperlinks

The hyperlink slot is not limited to URLs:

```rust
pub enum Hyperlink {
    Url(String),
    Action(Arc<dyn Action>),
}

pub trait Action: Any + Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
}
```

`impl<T> Action for T where T: Any + Debug + Send + Sync` means any concrete payload can be attached to a text span and dispatched on click.

In the renderer (`crates/octomusui_core/src/elements/formatted_text_element.rs`):

- `register_default_click_handlers_with_action_support` takes a callback `Fn(HyperlinkLens, &mut EventContext, &AppContext)`.
- `HyperlinkLens::Action(&dyn Action)` lets the consumer downcast the payload and run the matching logic.
- Hovering an action link sets the pointing-hand cursor and highlights the range; clicking dispatches the action.

This is the mechanism for clickable inline actions inside chat bubbles.

## Embedded objects travel inside markdown

The markdown parser has three special code-block languages used to inject non-text items into `FormattedText`:

- `octomus-embedded-object` → parsed as YAML → becomes `FormattedTextLine::Embedded(Mapping)`.
- `octomus-runnable-command` → runnable command block.
- `octomus-markdown-table` → table data.

So a chat message can be serialized as markdown while still describing custom UI objects. The downstream view decides how to render each `Embedded` line.

## Composer architecture

The composer is **not** a decorated `<textarea>`. It is an `EditorView` (rich text editor) surrounded by a toolbar/footer built from separate view handles.

From `app/src/terminal/input.rs`:

```rust
pub struct Input {
    editor: ViewHandle<EditorView>,
    agent_input_footer: ViewHandle<AgentInputFooter>,
    // ... inline selectors, slash commands, etc.
}
```

`Input::render_input_box` wraps `ChildView::new(&self.editor)` in `Clipped` + `ConstrainedBox`. The editor itself owns the typed text and cursor.

### API-key / variant selectors live in the footer, not inline

`AgentInputFooter` (rendered below the editor) owns:

- `model_selector: ViewHandle<ProfileModelSelector>` — choose the active model.
- `environment_selector: Option<ViewHandle<EnvironmentSelector>>` — choose the cloud/agent environment.
- `auth_secret_selector: ViewHandle<AuthSecretSelector>` — choose/create API keys (button tooltip "API key").
- `file_button`, `mic_button`, `fast_forward_button`, etc.

These are `ActionButton`s that open `Menu`/`DisplayChipMenu` dropdowns. Selections update models, and the footer re-renders to reflect the new state.

In the newer cloud-mode v2 composer (`Input::render_cloud_mode_v2_composing_input` / `AgentInputFooter::render_cloud_mode_v2_footer`), the selectors move to a top row above the editor, but they are still discrete view handles, not inline chips inside the text buffer.

### Inline menus are overlays positioned relative to the cursor

`Input` also owns a set of inline selector views:

- `inline_model_selector_view`
- `inline_profile_selector_view`
- `inline_slash_commands_view`
- `inline_skill_selector_view`
- etc.

They are rendered as overlay `ChildView`s positioned with `OffsetPositioning::relative_to_stack_child` anchored to the cursor save-position. This is the pattern for "type `@` and get a picker" style interactions, not for permanently embedded chips.

### Attachment chips render above the editor

Image/file attachments are rendered as a row of `Chip` components **above** the editor box (`render_attachment_chips`), not inside the text buffer.

## Chat message bubbles

Agent output is rendered in `app/src/ai/blocklist/block/view_impl/output.rs`. Each output item is added to a `Flex::column`. Special message types (`Action`, `Summarization`, `WebSearch`, `Subagent`, etc.) branch to dedicated renderers (`render_send_message`, `render_imported_comments`, etc.). Plain text sections are presumably rendered through `FormattedTextElement`, although this file focuses on structured output cards.

The important takeaway: chat content is a **stream of typed message items**, each deciding its own UI representation, rather than one big text buffer.

## Implications for Goble

1. **Chat content model**: we should introduce a lightweight `ChatContent` / `FormattedText`-like enum for chat bubbles. It can start with just `Text`, `Code`, and `Action` fragments; later add tables/embeds.
2. **Custom actions**: clickable actions inside messages should be hyperlinks that carry a trait object or enum payload, with a renderer callback that dispatches to the domain layer.
3. **Composer**: build `ChatComposer` as a vertical stack:
   - optional attachment chip row,
   - rich text editor (can be a simple multi-line `TextArea` for MVP),
   - footer toolbar with model/API-key/variant selector buttons and a send button.
   Custom selectors are dropdown overlays / separate primitives, not inline chips.
4. **Do not port `markdown_parser` yet**: we only need a tiny subset. Start with a `FormattedText`-style enum and optional inline action fragments. Add markdown parsing only if chat history must be serialized as markdown.
5. **First concrete step**: implement `ChatMessageBubble` and `ChatComposer` primitives plus a minimal `ChatView` that renders a list of mock messages and the composer footer.

## File references in warp-new

- `crates/markdown_parser/src/lib.rs` — `FormattedText`, `FormattedTextLine`, `FormattedTextFragment`, `FormattedTextStyles`, `Hyperlink`, `Action`.
- `crates/octomusui_core/src/elements/formatted_text_element.rs` — `FormattedTextElement`, `register_default_click_handlers_with_action_support`, hover/click dispatch.
- `crates/editor/src/editor.rs` — `EditorView` trait, `EmbeddedItemModel`, `RunnableCommandModel`.
- `app/src/terminal/input.rs` — `Input` struct, `render_input_box`, editor/footer layout.
- `app/src/terminal/input/agent.rs` — `render_agent_input`, `render_cloud_mode_v2_composing_input`, `render_cloud_mode_v2_input_container`.
- `app/src/terminal/input/cli_agent.rs` — `render_cli_agent_input` (rich input + footer).
- `app/src/ai/blocklist/agent_view/agent_input_footer/mod.rs` — `AgentInputFooter`, toolbar items, action buttons, footer rendering.
- `app/src/terminal/view/ambient_agent/auth_secret_selector.rs` — API-key selector button + menu.
- `app/src/terminal/view/ambient_agent/model_selector.rs` — model variant selector.
- `app/src/ai/blocklist/agent_view/agent_input_footer/environment_selector.rs` — environment selector.
- `app/src/ai/blocklist/block/view_impl/output.rs` — agent output message stream / bubble rendering.
