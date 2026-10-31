use std::rc::Rc;

use crate::elements::chat_content::{tool_fold_key, ChatFragmentKind, ToolDisplayMode};
use crate::elements::chat_message_bubble::{tool_fold_targets, ToolFoldTarget};
use crate::elements::terminal_block::{terminal_filter_bar, TerminalFilter};
use crate::elements::{
    command_approval_card, slash_menu_open, AppContext, AskUserCard, Axis, Border, Button,
    ButtonVariant, ChatComposer, ChatMessageBubble, CommandApprovalState, ConstrainedBox,
    Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Empty, Expanded, Fill, Flex,
    HoverRow, Icon, MainAxisAlignment, MainAxisSize, ModelMenu, Padding, QuickActionButton,
    Scrollable, ShortcutHints, SlashMenu, Spacer, Stack, Text, Tooltip, TooltipPosition,
    TurnStatusFooter,
};
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

use super::sticky::StickyTurn;
use super::ChatView;

/// How much lower the instruction strip sits than the block's own inset put
/// it: the strip belongs to the input under it, and the caps read as that
/// input's row only when they hug the separator over it.
pub(super) const STRIP_RISE: f32 = 6.0;

/// What the whole-transcript filter bar's field says when it is empty.
const TRANSCRIPT_FILTER_PLACEHOLDER: &str = "Filter terminal output";

/// Cap on the empty state's frame. A pane wider than this keeps the box
/// readable instead of stretching it across the whole window.
pub(super) const EMPTY_FRAME_MAX_WIDTH: f32 = 720.0;

/// How far the frame's border is mixed from the pane background toward the
/// theme's border, so the box reads as a soft edge rather than a hard rule.
const EMPTY_FRAME_BORDER_MIX: f32 = 0.45;

/// Group a token count in threes, so a long conversation's totals stay
/// readable (`1234567` reads as `1,234,567`).
fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The short form of a failed turn's reason: the first non-empty line, with a
/// provider's JSON body reduced to the message it carries, then capped so a
/// long error cannot take over the transcript. The full message stays
/// available to the caller's tooltip.
fn brief_failure(message: &str) -> String {
    const MAX_CHARS: usize = 180;
    const MESSAGE_KEY: &str = "\"message\":\"";
    let first = message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("The turn ended without a reply.");
    // A provider error is often `<prefix>: 400 Bad Request {"error":{...}}`.
    // Keep the prefix and the message it names, and drop the rest of the body.
    let condensed = match (first.find('{'), first.find(MESSAGE_KEY)) {
        (Some(brace), Some(at)) if at > brace => {
            let value: String = first[at + MESSAGE_KEY.len()..]
                .chars()
                .take_while(|ch| *ch != '"')
                .collect();
            let head = first[..brace]
                .trim_end_matches(|ch: char| ch == ':' || ch == ' ')
                .trim_end();
            format!("{head} — {}", value.replace("\\n", " "))
        }
        _ => first.to_string(),
    };
    if condensed.chars().count() <= MAX_CHARS {
        return condensed;
    }
    let mut shortened: String = condensed.chars().take(MAX_CHARS - 1).collect();
    shortened.push('…');
    shortened
}

/// The block that closes a failed turn: the heading and the brief reason.
///
/// The reason has no line cap: it wraps to whatever width the transcript gives
/// it, and the block's height follows the wrapped text, so a narrow pane reads
/// the whole brief instead of clipping it. The full provider message stays on
/// the tooltip.
fn failure_block(error: &str, app: &AppContext) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    let reason = Tooltip::new(
        Text::new(brief_failure(error))
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(12.0)
            .finish(),
        error.to_string(),
    )
    .with_position(TooltipPosition::Above)
    .finish();
    Container::new(
        // Stretch, so the reason is measured against the transcript's width
        // and wraps there. A start-aligned column hands its children an
        // unbounded width, which is what let the line run past the pane.
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm)
            .with_child(
                Text::new("Agent turn failed")
                    .with_theme_color(ColorToken::Error, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_child(reason)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_padding(EdgeInsets::uniform(md))
    .finish()
}

#[cfg(test)]
mod failure_tests {
    use super::{brief_failure, failure_block};
    use crate::elements::{AppContext, LayoutContext, SizeConstraint};
    use crate::geometry::vec2f;

    #[test]
    fn a_provider_error_keeps_its_prefix_and_message() {
        let error = "LLM request failed: 400 Bad Request {\"error\":{\"message\":\"The reasoning_content in the assistant message is not supported\"}}";
        assert_eq!(
            brief_failure(error),
            "LLM request failed: 400 Bad Request — The reasoning_content in the assistant message is not supported"
        );
    }

    #[test]
    fn a_plain_error_is_its_first_line_and_a_long_one_is_capped() {
        assert_eq!(
            brief_failure("\n  network unreachable  \nmore"),
            "network unreachable"
        );
        let long = "x".repeat(400);
        let brief = brief_failure(&long);
        assert_eq!(brief.chars().count(), 180);
        assert!(brief.ends_with('…'));
    }

    /// The reason is not clipped at a fixed number of lines: a narrow block
    /// wraps it and grows, and a wide one is shorter.
    #[test]
    fn the_reason_wraps_to_the_width_it_is_given() {
        let app = AppContext::default();
        let error = "LLM request failed: 400 Bad Request {\"error\":{\"message\":\"The reasoning_content in the assistant message is not supported by this model\"}}";
        let mut narrow = failure_block(error, &app);
        let narrow_size = narrow.layout(
            SizeConstraint::loose(vec2f(280.0, 4000.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut wide = failure_block(error, &app);
        let wide_size = wide.layout(
            SizeConstraint::loose(vec2f(900.0, 4000.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(
            narrow_size.y > wide_size.y,
            "the block grows with the wrapped reason: {narrow_size:?} against {wide_size:?}"
        );
        assert!(
            narrow_size.y > 14.4 * 3.0,
            "the narrow block is taller than the two-line clip it replaced: {narrow_size:?}"
        );
        assert_eq!(
            narrow_size.x, 280.0,
            "the block takes the width it is given"
        );
    }
}

impl ChatView {
    /// The slash-command list, when the draft the composer holds is a command:
    /// the items the host filtered for that draft, drawn over the whole input
    /// block. `None` when the draft is not a command (or Escape put the list
    /// away), so a closed list takes no space at all.
    fn slash_menu(&self) -> Option<Box<dyn Element>> {
        // The approval card owns the keyboard while it waits, so the command
        // list has no query to follow and draws nothing.
        if self.composer_proposal.is_some() {
            return None;
        }
        if !slash_menu_open(
            &self.composer_value.borrow(),
            *self.composer_slash_dismissed.borrow(),
        ) {
            return None;
        }
        let mut menu = SlashMenu::new(
            self.composer_slash_items.clone(),
            self.composer_slash_index.clone(),
        );
        if let Some(cb) = self.on_composer_slash_move.clone() {
            menu = menu.with_on_move(move |index| (cb.borrow_mut())(index));
        }
        if let Some(cb) = self.on_composer_slash_accept.clone() {
            menu = menu.with_on_accept(move |index| (cb.borrow_mut())(index));
        }
        Some(menu.finish())
    }

    /// The models the pane can run, when the composer's model control opened
    /// them: the same band the command list is drawn in, in the same slot over
    /// the input, so the two read as one list that the draft's commands win
    /// (see [`Self::slash_menu`] and the slot rule in `rebuild`). `None` while
    /// it is closed, so a closed list takes no space at all.
    fn model_menu(&self) -> Option<Box<dyn Element>> {
        if self.composer_proposal.is_some() {
            return None;
        }
        if !*self.composer_model_menu_open.borrow() {
            return None;
        }
        let accept = self.on_select_model_item.clone()?;
        let open = self.composer_model_menu_open.clone();
        let mut menu = ModelMenu::new(
            self.composer_model_items.clone(),
            self.composer_model_index.clone(),
        )
        .with_on_accept(move |index| {
            *open.borrow_mut() = false;
            (accept.borrow_mut())(index);
        });
        // The pointer moves the same selection the arrow keys do (the composer
        // routes those), so the highlight is the row either one would take.
        let index = self.composer_model_index.clone();
        let len = self.composer_model_items.len();
        menu = menu.with_on_move(move |row| {
            *index.borrow_mut() = row.min(len.saturating_sub(1));
        });
        Some(menu.finish())
    }

    /// What a pane with nothing in its transcript shows: the title and the
    /// invitation, centered in a rounded bordered frame.
    ///
    /// The frame spans the pane minus a margin on each side (capped at
    /// [`EMPTY_FRAME_MAX_WIDTH`]) and sits a third of the free height above the
    /// composer, which is where the reference tool's welcome box puts it.
    fn build_empty_state(&self, app: &AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let padding = app.theme.spacing_px(SpacingToken::Lg);
        let margin = app.theme.spacing_px(SpacingToken::Xl);
        let title = self
            .empty_title
            .clone()
            .unwrap_or_else(|| "New conversation".to_string());
        let subtitle = self
            .empty_subtitle
            .clone()
            .unwrap_or_else(|| "Ask anything to get started.".to_string());

        let content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(
                Text::new(title)
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_child(
                Text::new(subtitle)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();

        // The frame's width comes from this `Max` row: it takes whatever the
        // constraint offers, so the border spans the pane instead of hugging
        // the two lines of text.
        let border = app
            .theme
            .color(ColorToken::Bg)
            .mix(&app.theme.color(ColorToken::Border), EMPTY_FRAME_BORDER_MIX);
        let frame = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_child(content)
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(padding))
        .with_border(Border::all(1.0).with_border_color(border))
        .with_corner_radius(app.theme.radius_px())
        .finish();

        // Two spacers in a 1:2 ratio put the frame a third of the slack down
        // the pane and leave the rest below it. The outer flex claims the
        // height it was given so the composer stays pinned to the bottom.
        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Spacer::new().with_flex(1.0).finish())
            .with_child(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::Center)
                    .with_child(
                        Container::new(
                            ConstrainedBox::new(frame)
                                .with_max_width(EMPTY_FRAME_MAX_WIDTH)
                                .finish(),
                        )
                        .with_padding(EdgeInsets::new(margin, 0.0, margin, 0.0))
                        .finish(),
                    )
                    .finish(),
            )
            .with_child(Spacer::new().with_flex(2.0).finish())
            .finish()
    }

    /// The conversation's own footer, at the end of its transcript: fork it
    /// into a new one, and the tokens it has spent behind a disclosure.
    ///
    /// Both are hover affordances — no fill at rest, a band under the pointer,
    /// the shape the reference tool's response footer gives them — rather than
    /// the raised controls a button draws. The counts are the provider's own,
    /// summed over the conversation's model calls: the prompt tokens that
    /// missed the cache plus the output, which is what the conversation spent
    /// rather than the size of the calls (the cached share is billed at a
    /// discount). Nothing here is a price. A
    /// conversation whose provider has reported nothing yet says so rather than
    /// drawing a zero, and its disclosure does not open.
    ///
    /// The row is the closing child of the message column, i.e. part of the
    /// scrolled transcript rather than a sibling of it: `rebuild` hands it to
    /// the same `Scrollable` the messages go into, so the column's own spacing
    /// puts it directly under the last message and scrolling carries it away
    /// with that message instead of stranding it over the composer.
    fn build_conversation_footer(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        if self.messages.is_empty() {
            return None;
        }
        // A turn that is still in flight — including one suspended on a
        // command decision — has no closing row yet: forking the conversation
        // and its spend are read after the agent has finished, not while it is
        // still working.
        if self.turn_status.is_in_flight() || self.composer_proposal.is_some() {
            return None;
        }
        let has_usage = !self.usage.is_empty();
        let has_fork = self.on_fork.is_some();
        if !has_fork && !has_usage {
            return None;
        }
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let xs = app.theme.spacing_px(SpacingToken::Xs);
        let padding = EdgeInsets::new(sm, xs, sm, xs);
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm);

        if let Some(cb) = self.on_fork.clone() {
            let fork = HoverRow::new(
                Flex::row()
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("branch")
                            .with_size(13.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new("Fork")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(11.0)
                            .finish(),
                    )
                    .finish(),
            )
            .with_padding(padding)
            .with_on_click(move || (cb.borrow_mut())())
            .finish();
            row = row.with_child(
                Tooltip::new(fork, "Fork this conversation into a new one")
                    .with_position(TooltipPosition::Above)
                    .finish(),
            );
        }

        if has_usage {
            let open = self.usage_open.clone();
            let is_open = *open.borrow();
            let label = format!("{} tokens", group_digits(self.usage.spend()));
            let usage = HoverRow::new(
                Flex::row()
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("activity")
                            .with_size(13.0)
                            .with_theme_color(ColorToken::Accent, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new(label)
                            .with_theme_color(ColorToken::Accent, app)
                            .with_font_size(11.0)
                            .finish(),
                    )
                    .with_child(
                        Icon::new(if is_open {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        })
                        .with_size(12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                    )
                    .finish(),
            )
            .with_padding(padding)
            .with_on_click(move || {
                let mut open = open.borrow_mut();
                *open = !*open;
            })
            .finish();
            row = row.with_child(
                Tooltip::new(
                    usage,
                    "Show this conversation's token spend (cache hits excluded)",
                )
                .with_position(TooltipPosition::Above)
                .finish(),
            );
        } else {
            // Nothing reported yet: the slot stays, saying so, so the footer of
            // a conversation that has run turns reads the same as one that has
            // not — with no number invented to fill it.
            let usage = HoverRow::new(
                Flex::row()
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("activity")
                            .with_size(13.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new("no usage yet")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(11.0)
                            .finish(),
                    )
                    .finish(),
            )
            .with_padding(padding)
            .finish();
            row = row.with_child(
                Tooltip::new(
                    usage,
                    "The provider reports the token counts on each model call; \
                     none have been reported for this conversation yet",
                )
                .with_position(TooltipPosition::Above)
                .finish(),
            );
        }

        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(app.theme.spacing_px(SpacingToken::Xs))
            .with_child(row.finish());
        if has_usage && *self.usage_open.borrow() {
            column = column.with_child(
                Text::new(self.usage_detail())
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(11.0)
                    .finish(),
            );
        }
        // The room the transcript keeps under its own last row belongs to this
        // row, since it is what the anchored content ends on: without it the
        // affordances are pressed onto the input's separator, which reads as
        // the input's chrome rather than as the tail of the message above them.
        Some(
            Padding::new(
                column.finish(),
                EdgeInsets::new(0.0, 0.0, 0.0, app.theme.spacing_px(SpacingToken::Md)),
            )
            .finish(),
        )
    }

    /// The expanded usage line, shaped like the reference tool's readout: the
    /// prompt side carries its cached share in parentheses, because the cache is
    /// a subset of the input rather than a bucket beside it, so the three
    /// numbers never read as a sum. Then the generated side and the spend the
    /// meter shows: the prompt tokens that missed the cache plus the output.
    fn usage_detail(&self) -> String {
        let input = match self.usage.cached {
            Some(cached) => format!(
                "Input {} ({} cached)",
                group_digits(self.usage.input),
                group_digits(cached)
            ),
            None => format!("Input {}", group_digits(self.usage.input)),
        };
        format!(
            "{input}  ·  Output {}  ·  Spent {}",
            group_digits(self.usage.output),
            group_digits(self.usage.spend())
        )
    }

    /// Whether the transcript contains at least one terminal output block (so
    /// the whole-transcript filter is worth showing).
    fn has_terminal_blocks(&self) -> bool {
        self.messages.iter().any(|m| {
            m.fragments
                .iter()
                .any(|f| matches!(f.kind, ChatFragmentKind::Terminal(_)))
        })
    }

    /// How many of the transcript's terminal lines the wired whole-transcript
    /// filter keeps, and how many there are — what its bar counts. A fragment
    /// that draws no lines contributes nothing.
    fn terminal_line_counts(&self, filter: &TerminalFilter) -> (usize, usize) {
        let mut matched = 0;
        let mut total = 0;
        for message in &self.messages {
            for fragment in &message.fragments {
                let ChatFragmentKind::Terminal(data) = &fragment.kind else {
                    continue;
                };
                total += data.lines.len();
                matched += data
                    .lines
                    .iter()
                    .filter(|line| filter.matches(line.kind, &line.text))
                    .count();
            }
        }
        (matched, total)
    }

    /// The whole-transcript filter bar, drawn over the transcript while the
    /// wired filter is open. `None` when no filter is wired up or the user has
    /// not raised one, so a closed bar takes no space at all.
    fn build_global_filter_bar(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let filter = self.global_terminal_filter.clone()?;
        if !filter.is_open() {
            return None;
        }
        let (matched, total) = self.terminal_line_counts(&filter);
        Some(
            Container::new(terminal_filter_bar(
                &filter,
                TRANSCRIPT_FILTER_PLACEHOLDER,
                matched,
                total,
                app,
            ))
            .with_padding(EdgeInsets::new(0.0, 0.0, 4.0, 0.0))
            .finish(),
        )
    }

    /// Rebuild the tree. `header_height` is the height the pane's topbar took
    /// this frame; the column reserves it so the transcript cannot run under
    /// the floating bar.
    pub(super) fn rebuild(&mut self, app: &AppContext, header_height: f32) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(spacing);

        if header_height > 0.0 && self.header.is_some() {
            column = column.with_child(
                Empty::new()
                    .with_size(Vector2F::new(0.0, header_height))
                    .finish(),
            );
        }

        let message_area: Box<dyn Element> = if self.messages.is_empty()
            && self.pending_ask.is_none()
            && self.composer_proposal.is_none()
            && self.notice.is_none()
            && self.turn_error.is_none()
        {
            // The empty state claims the remaining height itself, so the
            // composer stays pinned to the bottom of the window.
            self.build_empty_state(app)
        } else {
            let mut message_column = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            // An inline notice (e.g. "No API key configured") leads the
            // transcript instead of floating over the app as a dialog.
            if let Some(notice) = self.notice.take() {
                message_column = message_column.with_child(notice);
            }
            // The rows the sticky turn header reads: the latest user message and
            // the reply after it, recorded as they paint.
            let sticky = StickyTurn::for_messages(&self.messages);
            for (index, message) in self.messages.iter().enumerate() {
                let on_action = self.on_action.clone();
                let fragments = message.fragments.clone();
                let mut bubble = ChatMessageBubble::new(message.role, fragments)
                    .with_tool_calls(message.tool_calls.clone())
                    .with_terminal_filters(self.terminal_filters.clone())
                    .with_reasoning_expanded(self.reasoning_expanded.clone())
                    .with_tool_fold(self.tool_fold.clone())
                    .with_sub_agents(self.sub_agents.clone())
                    .with_global_terminal_filter(self.global_terminal_filter.clone())
                    .with_on_copy_terminal(self.on_copy_terminal.clone())
                    .with_on_action(move |action| {
                        if let Some(cb) = on_action.as_ref() {
                            (cb.borrow_mut())(action);
                        }
                    });
                if self.transcript_selection.is_some() {
                    bubble = bubble.with_text_selection(self.text_recording.clone());
                }
                let bubble = bubble.finish();
                message_column = message_column.with_child(sticky.probe(index, bubble));
            }
            // A failed turn says why under the messages it belongs to. The
            // inbox names the conversation; the transcript carries the reason,
            // where the user is already looking when the agent goes quiet.
            if let Some(error) = self.turn_error.clone() {
                message_column = message_column.with_child(failure_block(&error, app));
            }
            // The conversation's own footer closes the transcript: fork it, and
            // the tokens it has spent.
            if let Some(footer) = self.build_conversation_footer(app) {
                message_column = message_column.with_child(footer);
            }
            // A pending ask sits inline at the end of the transcript (warp-new
            // model) so it scrolls with the conversation rather than pinning to
            // the bottom of the window.
            if let Some(ask) = &self.pending_ask {
                let mut card = AskUserCard::new(ask.question.clone(), ask.quick_replies.clone());
                if let Some(cb) = self.on_answer_ask.clone() {
                    card = card.with_on_answer(move |resp, cred| (cb.borrow_mut())(resp, cred));
                }
                if let Some(cb) = self.on_skip_ask.clone() {
                    card = card.with_on_skip(move || (cb.borrow_mut())());
                }
                message_column = message_column.with_child(card.finish());
            }
            // A command the harness is waiting on closes the agent's output:
            // the turn stays suspended until Run or Reject, and the decision is
            // made where the command would run. The card scrolls with the
            // transcript, so a long command or a list of variants is read like
            // any other output instead of growing the input.
            if let Some(proposal) = self.composer_proposal.clone() {
                let state = CommandApprovalState {
                    selection: self.composer_proposal_selection.clone(),
                    scroll: self.composer_proposal_scroll.clone(),
                    editor_scroll: self.composer_proposal_editor_scroll.clone(),
                    draft: self.composer_proposal_draft.clone(),
                    caret: self.composer_proposal_caret.clone(),
                    // The rich input is not built while the card waits, so the
                    // card is the pane's only typing surface and always holds
                    // the keys.
                    field_focused: true,
                };
                message_column = message_column.with_child(command_approval_card(
                    &proposal,
                    &state,
                    self.on_command_decision.clone(),
                    app,
                ));
            }
            // A prompt queued while the agent was running renders as a pending
            // block in the transcript (warp-new model) with "Send now" + dismiss.
            if let Some(prompt) = &self.queued_prompt {
                let sm = app.theme.spacing_px(SpacingToken::Sm);
                let md = app.theme.spacing_px(SpacingToken::Md);
                let mut header = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new("Pending message")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    );
                if let Some(cb) = self.on_dismiss_queued.clone() {
                    let dismiss = Button::new(
                        Icon::new("x-close")
                            .with_size(11.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_variant(ButtonVariant::Ghost)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                    header = header.with_child(dismiss);
                }
                let mut footer = Flex::row().with_main_axis_alignment(MainAxisAlignment::End);
                if let Some(cb) = self.on_send_queued.clone() {
                    let send_now = Button::new(
                        Text::new("Send now")
                            .with_theme_color(ColorToken::Bg, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_variant(ButtonVariant::Primary)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                    footer = footer.with_child(send_now);
                }
                let queued_band = Container::new(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_spacing(sm)
                        .with_child(header.finish())
                        .with_child(
                            Text::new(prompt.clone())
                                .with_theme_color(ColorToken::Text, app)
                                .with_font_size(12.0)
                                .finish(),
                        )
                        .with_child(footer.finish())
                        .finish(),
                )
                // A full-width band of rows: no border and no rounded card.
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_padding(EdgeInsets::uniform(md))
                .finish();
                message_column = message_column.with_child(queued_band);
            }
            // The transcript scrolls (clipped to the viewport, wheel handled)
            // and tails the stream: the state is app-owned, so new content
            // follows the bottom until the user scrolls up, and their
            // scrollback position is held while it streams. A conversation
            // shorter than the pane is flushed to the end of it, so the last
            // message sits on the input (warp-new) instead of leaving the gap
            // between it and the input on screen.
            let transcript = Scrollable::new(message_column.finish(), Axis::Vertical)
                .with_state(self.scroll.clone())
                .with_bottom_anchor(true)
                .finish();
            // The surface records what its text elements painted and draws the
            // selection over them; it is also the box the pointer is read
            // against, so a press outside the transcript clears the selection.
            let transcript = match &self.transcript_selection {
                Some(selection) => super::surface::TranscriptSurface::new(
                    transcript,
                    self.text_recording.clone(),
                    Rc::clone(selection),
                    self.transcript_viewport.clone(),
                )
                .finish(),
                None => transcript,
            };
            // The latest user message pins to the transcript's top edge while
            // its own bubble is above it (grok-build's sticky turn). The header
            // is an overlay of the transcript's stack, so it covers the top
            // strip instead of taking a row from the scrolling content.
            let transcript: Box<dyn Element> = match sticky.overlay() {
                Some(header) => Stack::new()
                    .with_children(vec![transcript])
                    .with_overlay(header, Vector2F::zero())
                    .finish(),
                None => transcript,
            };
            if self.has_terminal_blocks() {
                if let Some(bar) = self.build_global_filter_bar(app) {
                    // The filter bar is content-sized, the transcript takes the
                    // rest: the column must claim the height it was given, or
                    // the inner `Expanded` is laid out unbounded and reports
                    // the whole transcript's height, pushing the composer below
                    // the pane whenever the conversation is long.
                    Flex::column()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_child(bar)
                        .with_child(Expanded::new(transcript).finish())
                        .finish()
                } else {
                    transcript
                }
            } else {
                transcript
            }
        };
        // The transcript consumes the remaining space so the composer pins to
        // the bottom of the chat view.
        column = column.with_child(Expanded::new(message_area).finish());

        if !self.messages.is_empty() && !self.quick_actions.is_empty() {
            let mut row = Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::Start)
                .with_spacing(app.theme.spacing_px(SpacingToken::Sm));
            for (label, cb) in &self.quick_actions {
                let cb = cb.clone();
                row = row.with_child(
                    QuickActionButton::new(label.clone(), move || (cb.borrow_mut())()).finish(),
                );
            }
            column = column.with_child(row.finish());
        }

        let current_value = self.composer_value.borrow().clone();
        let composer_value_for_change = self.composer_value.clone();
        let composer_value = self.composer_value.clone();
        let composer_value_cmd = self.composer_value.clone();
        let composer_value_cloud = self.composer_value.clone();
        let on_send = self.on_send.clone();
        let on_cmd_enter = self.on_cmd_enter.clone();
        let on_send_to_cloud = self.on_send_to_cloud.clone();
        let on_composer_change = self.on_composer_change.clone();
        let mut composer = ChatComposer::new()
            .with_value(current_value)
            // The files the attach control picked: one chip per file, over the
            // editor they belong to.
            .with_attachments(self.composer_attachments.clone())
            // A waiting command hides the input below, so this focus state
            // only describes the composer while the pane is free to type.
            .with_focused(self.composer_focused)
            .with_queued_prompt_present(self.queued_prompt.is_some())
            .with_caret(self.composer_caret.clone())
            // While a turn is in flight the status row above the input carries
            // the one Stop control; an idle input keeps it in its action row.
            .with_stop_visible(self.composer_stop_visible && !self.turn_status.is_in_flight())
            // The rich input is built the same way in both surfaces: the
            // context row (harness, directory, branch, attach) rides above the
            // editor, where it describes the draft, and the pane's instructions
            // are drawn by the pane over the separator above it. A terminal
            // pane's bar is built from this same widget in this same order, so
            // switching a pane between the shell and its agent view moves
            // nothing but the text in the editor.
            .with_context_above_editor(true)
            // The agent bar is the pane's bottom surface, like the shell bar:
            // it spans the pane instead of carrying a gutter of its own, so
            // both surfaces space the input the same way.
            .with_edge_to_edge(true)
            .with_on_change(move |text| {
                *composer_value_for_change.borrow_mut() = text.clone();
                if let Some(cb) = on_composer_change.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_send(move |text| {
                *composer_value.borrow_mut() = String::new();
                if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_cmd_enter(move |text| {
                *composer_value_cmd.borrow_mut() = String::new();
                if let Some(cb) = on_cmd_enter.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_send_to_cloud(move |text| {
                *composer_value_cloud.borrow_mut() = String::new();
                if let Some(cb) = on_send_to_cloud.as_ref() {
                    (cb.borrow_mut())(text);
                }
            });
        if let Some(label) = self.composer_model_label.clone() {
            composer = composer.with_model_label(label);
        }
        if let Some(path) = self.composer_path.clone() {
            composer = composer.with_path_label(path);
        }
        // The host a bound shell session is on, named over the editor with the
        // directory and branch it belongs beside (S2). A local session draws
        // nothing, the same as everywhere else.
        if let Some(session) = self.composer_ssh_session.clone() {
            composer = composer.with_ssh_session(Some(session));
        }
        if let Some(label) = self.composer_harness_label.clone() {
            composer = composer.with_harness_label(label);
        }
        if let Some(icon) = self.composer_harness_icon {
            composer = composer.with_harness_icon(icon);
        }
        if let Some(label) = self.composer_branch_label.clone() {
            composer = composer.with_branch_label(label);
        }
        if let Some(diff) = self.composer_diff.clone() {
            composer = composer
                .with_diff_menu(diff, self.composer_diff_menu_open.clone())
                .with_diff_menu_scroll(self.composer_diff_menu_scroll.clone());
            if let Some(open) = self.on_composer_diff_open.clone() {
                composer = composer.with_diff_open(move |file| (open.borrow_mut())(file));
            }
        }
        if let Some(cb) = self.on_composer_focus_change.clone() {
            composer = composer.with_on_focus_change(move |focused| (cb.borrow_mut())(focused));
        }
        if let Some(cb) = self.on_attach.clone() {
            composer = composer.with_on_attach(move || (cb.borrow_mut())());
        }
        if let Some(cb) = self.on_voice.clone() {
            composer = composer.with_on_voice(move || (cb.borrow_mut())());
        }
        if let Some(cb) = self.on_select_model.clone() {
            composer = composer.with_on_select_model(move || (cb.borrow_mut())());
        }
        if let Some(cb) = self.on_stop.clone() {
            composer = composer.with_on_stop(move || (cb.borrow_mut())());
        }
        if let Some(mode) = self.composer_approval_mode {
            composer = composer
                .with_approval_mode(mode)
                .with_approval_menu_open(self.composer_approval_menu_open.clone())
                .with_approval_menu_highlight(self.composer_approval_menu_highlight.clone());
            if let Some(cb) = self.on_select_approval_mode.clone() {
                composer =
                    composer.with_on_select_approval_mode(move |mode| (cb.borrow_mut())(mode));
            }
        }
        if let Some(proposal) = self.composer_proposal.clone() {
            composer = composer
                .with_proposal(proposal)
                .with_proposal_selection(self.composer_proposal_selection.clone());
        }
        if let Some(cb) = self.on_command_decision.clone() {
            composer =
                composer.with_on_decision(move |id, decision| (cb.borrow_mut())(id, decision));
        }
        if let Some(cb) = self.on_select_model_item.clone() {
            composer = composer
                .with_model_menu(
                    self.composer_model_items.clone(),
                    self.composer_model_menu_open.clone(),
                    move |idx| (cb.borrow_mut())(idx),
                )
                .with_model_menu_index(self.composer_model_index.clone());
        }
        if let Some(cb) = self.on_select_harness_item.clone() {
            composer = composer.with_harness_menu(
                self.composer_harness_items.clone(),
                self.composer_harness_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
        }
        if let Some(cb) = self.on_select_dir_item.clone() {
            composer = composer.with_dir_menu(
                self.composer_dir_items.clone(),
                self.composer_dir_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
            composer = composer.with_dir_menu_scroll(self.composer_dir_menu_scroll.clone());
            composer =
                composer.with_dir_menu_highlight(self.composer_dir_menu_highlight.clone());
        }
        if let Some(cb) = self.on_select_branch_item.clone() {
            composer = composer
                .with_branch_menu(
                    self.composer_branch_items.clone(),
                    self.composer_branch_menu_open.clone(),
                    move |idx| (cb.borrow_mut())(idx),
                )
                .with_branch_menu_highlight(self.composer_branch_menu_highlight.clone());
        }
        // The slash-command menu: the draft is its query, so the host hands the
        // view the commands it matches and the view keeps the editor focused.
        composer = composer.with_slash_menu(
            self.composer_slash_items.clone(),
            self.composer_slash_index.clone(),
            self.composer_slash_dismissed.clone(),
        );
        if let Some(cb) = self.on_composer_slash_move.clone() {
            composer = composer.with_on_slash_move(move |index| (cb.borrow_mut())(index));
        }
        if let Some(cb) = self.on_composer_slash_accept.clone() {
            composer = composer.with_on_slash_accept(move |index| (cb.borrow_mut())(index));
        }
        if let Some(cb) = self.on_composer_slash_dismiss.clone() {
            composer = composer.with_on_slash_dismiss(move || (cb.borrow_mut())());
        }
        // Modal editing, when the user turned it on: the pane's own mode state
        // and the app's system clipboard for the `"+`/`"*` registers.
        if let Some(vim) = self.composer_vim.clone() {
            composer = composer.with_vim(vim);
            if let Some(clipboard) = self.composer_clipboard.clone() {
                composer = composer.with_clipboard(clipboard);
            }
        }
        let composer = composer.finish();
        // A separator line above the rich input separates it from the
        // transcript. The composer is a content-sized child (not flex-grown),
        // so it pins to the bottom: its textarea grows with the draft (capped)
        // and the row under it (the model) hugs the bottom, leaving the message
        // transcript most of the height. A read-only transcript (a sub-agent's
        // child view) drops both — the input belongs to another conversation.
        //
        // The pane's instructions are the strip over the separator, the last row
        // of the content: they name the gestures the input under them answers,
        // and warp-new keeps them off the input's own rows. The build draws both
        // bands — the draft's commands and the models the model control opened —
        // over the whole input block, above the strip: one slot, which the
        // draft's own command list holds while the draft is a command, the way
        // grok-build puts them over the prompt and not over the pane.
        if self.show_composer {
            // A waiting command puts the card's keys first: the command list,
            // the instruction strip and the rich input all stay away, so the
            // card's command field is the pane's only typing surface. The
            // turn-status row (with its Stop) stays, because the turn is still
            // in flight.
            let waiting = self.composer_proposal.is_some();
            if !waiting {
                if let Some(menu) = self.slash_menu().or_else(|| self.model_menu()) {
                    column = column.with_child(menu);
                }
            }
            // The live turn-status row takes the instruction strip's slot while
            // a turn is in flight — the waiting text is what the bar is doing,
            // and the row also carries the turn's Stop — instead of stacking
            // above it. An idle pane keeps the strip and pays no status height.
            if self.turn_status.is_in_flight() {
                let mut footer = TurnStatusFooter::new(self.turn_status.clone());
                if self.composer_stop_visible {
                    if let Some(cb) = self.on_stop.clone() {
                        footer = footer.with_stop(move || (cb.borrow_mut())());
                    }
                }
                column = column.with_child(footer.finish());
            } else if !waiting && !self.composer_hints.is_empty() {
                let md = app.theme.spacing_px(SpacingToken::Md);
                let sm = app.theme.spacing_px(SpacingToken::Sm);
                let strip = ShortcutHints::new(self.composer_hints.clone()).finish(app);
                // The strip's caps sit a few points under the transcript's last
                // row: the gestures belong to the input below them, and the gap
                // is what reads them as the input's own row rather than as the
                // conversation's tail.
                //
                // The block is pinned to the input under it (the transcript
                // above it is the flex child that takes the slack), so the caps'
                // height over the separator is decided by the inset *beneath*
                // them and not by the gap above: `sm - STRIP_RISE` puts them
                // six points lower than `sm` alone did, with the block's own
                // height unchanged.
                column = column.with_child(
                    Padding::new(
                        strip,
                        EdgeInsets::new(md, 4.0 + STRIP_RISE, md, sm - STRIP_RISE),
                    )
                    .finish(),
                );
            }
            // The separator is the input's top edge and the composer sits on
            // it: the pair is one block, so the column's `md` spacing does not
            // open a gap under the rule. The shell pane's bar keeps its rows
            // `xs` under its own divider; this is the same edge.
            //
            // A waiting command hides the whole input: the card above is the
            // pane's only typing surface until Run or Reject settles the
            // decision. The turn-status row stays as the mark that the turn is
            // suspended.
            if !waiting {
                let input = Flex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_child(Divider::horizontal().finish())
                    .with_child(composer)
                    .finish();
                column = column.with_child(input);
            }
        }

        // No outer padding: the header and composer span the full width and
        // the composer pins flush to the bottom of the window.
        let content = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .finish();
        // The header floats above the transcript on its own layer: the tray the
        // dots open hangs over the messages below the bar, and an in-flow child
        // would be painted over by them. The column above reserves the bar's
        // height (`header_height`), so the transcript still starts under it.
        self.root = Some(match self.header.take() {
            Some(header) => Stack::new()
                .with_children(vec![content])
                .with_overlay(header, Vector2F::zero())
                .finish(),
            None => content,
        });
    }

    /// Advance every tool call in the transcript to the next fold. The
    /// transcript has no entry selection yet, so `e` advances them all; the
    /// header click is the per-call arm of the same toggle.
    pub(super) fn advance_tool_folds(&mut self) {
        let mut fold = self.tool_fold.borrow_mut();
        for message in &self.messages {
            for (index, call) in message.tool_calls.iter().enumerate() {
                let key = tool_fold_key(call, index);
                let current = fold
                    .get(&key)
                    .copied()
                    .unwrap_or(call.default_display_mode());
                fold.insert(key, current.next());
            }
        }
    }

    /// Open the last folded row of the transcript: grok-build's Right arrow
    /// (`Action::Expand`), pointed at the newest row that can still open.
    ///
    /// The transcript tails the stream, so the row the user is looking at is
    /// the last one: a tool run folds into its aggregated header and opens on
    /// the first press, a member call that is still collapsed opens next, and
    /// a thinking row that already finished opens after that. Returns whether
    /// anything changed, so the host can tell an expansion from an idle key.
    pub(super) fn expand_latest_fold(&mut self) -> bool {
        let mut fold = self.tool_fold.borrow_mut();
        for message in self.messages.iter().rev() {
            let targets = tool_fold_targets(&message.tool_calls, &*fold);
            for target in targets.into_iter().rev() {
                match target {
                    ToolFoldTarget::Group { key, .. } => {
                        if fold.get(&key).copied() != Some(ToolDisplayMode::Expanded) {
                            fold.insert(key, ToolDisplayMode::Expanded);
                            return true;
                        }
                    }
                    ToolFoldTarget::Call { key, index } => {
                        let current = fold
                            .get(&key)
                            .copied()
                            .unwrap_or_else(|| message.tool_calls[index].default_display_mode());
                        if current != ToolDisplayMode::Expanded {
                            fold.insert(key, ToolDisplayMode::Expanded);
                            return true;
                        }
                    }
                }
            }
            let mut expanded = self.reasoning_expanded.borrow_mut();
            for fragment in message.fragments.iter().rev() {
                let ChatFragmentKind::Reasoning { key, done, .. } = &fragment.kind else {
                    continue;
                };
                if !expanded.get(key).copied().unwrap_or(!*done) {
                    expanded.insert(key.clone(), true);
                    return true;
                }
            }
        }
        false
    }

    /// Close the last open row of the transcript: grok-build's Left arrow
    /// (`Action::Collapse`). It mirrors [`Self::expand_latest_fold`], so the two
    /// keys walk the same rows.
    pub(super) fn collapse_latest_fold(&mut self) -> bool {
        let mut fold = self.tool_fold.borrow_mut();
        for message in self.messages.iter().rev() {
            let targets = tool_fold_targets(&message.tool_calls, &*fold);
            for target in targets.into_iter().rev() {
                match target {
                    ToolFoldTarget::Group { key, .. } => {
                        if fold.get(&key).copied() == Some(ToolDisplayMode::Expanded) {
                            fold.insert(key, ToolDisplayMode::Collapsed);
                            return true;
                        }
                    }
                    ToolFoldTarget::Call { key, index } => {
                        let current = fold
                            .get(&key)
                            .copied()
                            .unwrap_or_else(|| message.tool_calls[index].default_display_mode());
                        if current != ToolDisplayMode::Collapsed {
                            fold.insert(key, ToolDisplayMode::Collapsed);
                            return true;
                        }
                    }
                }
            }
            let mut expanded = self.reasoning_expanded.borrow_mut();
            for fragment in message.fragments.iter().rev() {
                let ChatFragmentKind::Reasoning { key, done, .. } = &fragment.kind else {
                    continue;
                };
                if expanded.get(key).copied().unwrap_or(!*done) {
                    expanded.insert(key.clone(), false);
                    return true;
                }
            }
        }
        false
    }
}
