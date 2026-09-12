use std::sync::Arc;

use crate::elements::chat_content::{tool_fold_key, ChatFragmentKind};
use crate::elements::{
    filter_option_labels, AppContext, AskUserCard, Axis, Button, ButtonVariant, ChatComposer,
    ChatMessageBubble, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Empty,
    Expanded, Fill, Flex, FrameView, Icon, MainAxisAlignment, MainAxisSize, PopupMenu,
    PopupMenuItem, PopupMenuPosition, QuickActionButton, Scrollable, Stack, Switch, Text,
    Tooltip, TooltipPosition, TopbarButton, TurnStatusFooter,
};
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

use super::ChatView;

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

impl ChatView {
    fn build_empty_state(&self, app: &AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let xl = app.theme.spacing_px(SpacingToken::Xl);
        let title = self
            .empty_title
            .clone()
            .unwrap_or_else(|| "New conversation".to_string());
        let subtitle = self
            .empty_subtitle
            .clone()
            .unwrap_or_else(|| "Ask anything to get started.".to_string());

        let column = Flex::column()
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

        Container::new(column)
            .with_padding(EdgeInsets::new(0.0, xl, 0.0, 0.0))
            .finish()
    }

    /// The conversation's own footer, at the end of its transcript: fork it
    /// into a new one, and the tokens it has spent behind a disclosure.
    ///
    /// The counts are the provider's own, summed over the conversation's model
    /// calls (input, the cached part when the provider reports one, and
    /// output). Nothing here is a price. A conversation with no reported usage
    /// draws no usage affordance at all, rather than a zero.
    fn build_conversation_footer(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        if self.messages.is_empty() {
            return None;
        }
        let has_usage = !self.usage.is_empty();
        let has_fork = self.on_fork.is_some();
        if !has_fork && !has_usage {
            return None;
        }
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm);

        if let Some(cb) = self.on_fork.clone() {
            let fork = Button::new(
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
            .with_variant(ButtonVariant::Ghost)
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
            let label = format!("{} tokens", group_digits(self.usage.total()));
            let usage = Button::new(
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
                        Icon::new(if is_open { "chevron-down" } else { "chevron-right" })
                            .with_size(12.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .finish(),
            )
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(move || {
                let mut open = open.borrow_mut();
                *open = !*open;
            })
            .finish();
            row = row.with_child(
                Tooltip::new(usage, "Show this conversation's token usage")
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
        Some(column.finish())
    }

    /// The expanded usage line, shaped like the reference tool's readout: the
    /// prompt side carries its cached share in parentheses, because the cache is
    /// a subset of the input rather than a bucket beside it, so the three
    /// numbers never read as a sum. Then the generated side and the total.
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
            "{input}  ·  Output {}  ·  Total {}",
            group_digits(self.usage.output),
            group_digits(self.usage.total())
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

    /// The whole-transcript filter bar (filter button + tray). Returns `None`
    /// when no global filter is wired up.
    fn build_global_filter_bar(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let filter = self.global_terminal_filter.clone()?;
        let selected = *filter.selected.borrow();
        let muted = ColorToken::Muted;
        let items = filter_option_labels()
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let mut item = PopupMenuItem::new(*label);
                if i == selected {
                    item = item.selected();
                }
                item
            })
            .collect::<Vec<_>>();
        let trigger = TopbarButton::new(
            Icon::new("sliders")
                .with_size(14.0)
                .with_theme_color(muted, app)
                .finish(),
        )
        .with_size(24.0)
        .with_active(selected != 0)
        .finish();
        let filter_for_select = filter.clone();
        let menu = PopupMenu::new(trigger, items)
            .with_open(filter.open.clone())
            .with_position(PopupMenuPosition::Below)
            .with_on_select(move |idx| *filter_for_select.selected.borrow_mut() = idx)
            .finish();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .with_child(
                Text::new("Filter terminal output")
                    .with_theme_color(muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_child(menu)
            .finish();
        Some(
            Container::new(row)
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
            && self.inline_screen.is_none()
            && self.screen_link.is_none()
            && self.notice.is_none()
        {
            // Wrap in a scrollable so it fills the remaining height and pins
            // the composer to the bottom of the window (a bare container would
            // size to its content and leave a gap under the composer).
            Scrollable::new(self.build_empty_state(app), Axis::Vertical).finish()
        } else {
            let mut message_column = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            // An inline notice (e.g. "No API key configured") leads the
            // transcript instead of floating over the app as a dialog.
            if let Some(notice) = self.notice.take() {
                message_column = message_column.with_child(notice);
            }
            for message in &self.messages {
                let on_action = self.on_action.clone();
                let fragments = message.fragments.clone();
                let bubble = ChatMessageBubble::new(message.role, fragments)
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
                    })
                    .finish();
                message_column = message_column.with_child(bubble);
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
                        Text::new("✕")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_variant(ButtonVariant::Ghost)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                    header = header.with_child(dismiss);
                }
                let mut footer = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::End);
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
            // A live remote-desktop handoff renders inline at the end of the
            // transcript (the harness takes over control). It scrolls with the
            // conversation and carries a close button to clear it.
            if let Some(screen) = &self.inline_screen {
                let md = app.theme.spacing_px(SpacingToken::Md);
                let sm = app.theme.spacing_px(SpacingToken::Sm);
                let mut header = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new("Component desktop · harness in control")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    );
                if let Some(cb) = self.on_close_inline_screen.clone() {
                    let close = Button::new(
                        Text::new("✕")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_variant(ButtonVariant::Ghost)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                    header = header.with_child(close);
                }
                let frame = FrameView::new(
                    screen.source.clone(),
                    screen.frame_seq,
                    screen.width,
                    screen.height,
                    Arc::clone(&screen.data),
                );
                let block = Container::new(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_spacing(sm)
                        .with_child(header.finish())
                        .with_child(frame.finish())
                        .finish(),
                )
                // A full-width band of rows: no border and no rounded card.
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_padding(EdgeInsets::uniform(md))
                .finish();
                message_column = message_column.with_child(block);
            }
            // A detected BYOH handoff hyperlink (no live frame yet): show a
            // clickable affordance so the user can take over the harness's
            // desktop.
            if self.inline_screen.is_none() {
                if let Some(link) = &self.screen_link {
                    let md = app.theme.spacing_px(SpacingToken::Md);
                    let sm = app.theme.spacing_px(SpacingToken::Sm);
                    let mut row = Flex::row()
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Text::new("Harness handed off a desktop")
                                .with_theme_color(ColorToken::Muted, app)
                                .with_font_size(12.0)
                                .finish(),
                        );
                    if let Some(cb) = self.on_open_screen_link.clone() {
                        let link_for_cb = link.clone();
                        let open = Button::new(
                            Text::new("Open remote desktop")
                                .with_theme_color(ColorToken::Bg, app)
                                .with_font_size(12.0)
                                .finish(),
                        )
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || (cb.borrow_mut())(link_for_cb.clone()))
                        .finish();
                        row = row.with_child(open);
                    }
                    let band = Container::new(
                        Flex::column()
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .with_spacing(sm)
                            .with_child(row.finish())
                            .finish(),
                    )
                    // A full-width band of rows: no border and no rounded card.
                    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                    .with_padding(EdgeInsets::uniform(md))
                    .finish();
                    message_column = message_column.with_child(band);
                }
            }
            // The transcript scrolls (clipped to the viewport, wheel handled)
            // and tails the stream: the state is app-owned, so new content
            // follows the bottom until the user scrolls up, and their
            // scrollback position is held while it streams.
            let transcript = Scrollable::new(message_column.finish(), Axis::Vertical)
                .with_state(self.scroll.clone())
                .finish();
            if self.has_terminal_blocks() {
                if let Some(bar) = self.build_global_filter_bar(app) {
                    Flex::column()
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

        // Control strip above the rich input: the auto-approve toggle. Stop
        // lives in the composer footer (visible while the agent is streaming).
        if let Some(cb) = self.on_toggle_auto_approve.clone() {
            let sm = app.theme.spacing_px(SpacingToken::Sm);
            let md = app.theme.spacing_px(SpacingToken::Md);
            let switch = Switch::new()
                .with_checked(self.auto_approve)
                .with_size(Vector2F::new(36.0, 20.0))
                .with_on_change(move |checked| (cb.borrow_mut())(checked))
                .finish();
            let controls = Container::new(
                Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::Start)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(
                        Text::new("Auto-approve")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_child(switch)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(0.0, md, 0.0, 0.0))
            .finish();
            column = column.with_child(controls);
        }

        let current_value = self.composer_value.borrow().clone();
        let composer_value_for_change = self.composer_value.clone();
        let composer_value = self.composer_value.clone();
        let composer_value_cmd = self.composer_value.clone();
        let on_send = self.on_send.clone();
        let on_cmd_enter = self.on_cmd_enter.clone();
        let on_composer_change = self.on_composer_change.clone();
        let mut composer = ChatComposer::new()
            .with_value(current_value)
            .with_focused(self.composer_focused)
            .with_caret(self.composer_caret.clone())
            .with_stop_visible(self.composer_stop_visible)
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
            });
        if let Some(label) = self.composer_model_label.clone() {
            composer = composer.with_model_label(label);
        }
        if let Some(path) = self.composer_path.clone() {
            composer = composer.with_path_label(path);
        }
        if let Some(label) = self.composer_harness_label.clone() {
            composer = composer.with_harness_label(label);
        }
        if let Some(label) = self.composer_branch_label.clone() {
            composer = composer.with_branch_label(label);
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
            composer = composer.with_model_menu(
                self.composer_model_items.clone(),
                self.composer_model_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
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
        }
        if let Some(cb) = self.on_select_branch_item.clone() {
            composer = composer.with_branch_menu(
                self.composer_branch_items.clone(),
                self.composer_branch_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
        }
        if let Some(cb) = self.on_composer_slash.clone() {
            composer = composer.with_on_slash(move || (cb.borrow_mut())());
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
        // The live turn-status footer sits directly above the composer's
        // divider, in the pager idiom: full width, no border. It is added only
        // when it is in flight, so an idle pane pays neither its height nor the
        // column's inter-child spacing for it.
        if self.turn_status.is_in_flight() {
            let footer = TurnStatusFooter::new(self.turn_status.clone());
            column = column.with_child(footer.finish());
        }
        // A separator line above the rich input separates it from the
        // transcript. The composer is a content-sized child (not flex-grown),
        // so it pins to the bottom: its textarea grows with the draft (capped)
        // and the footer pills sit just below it, leaving the message
        // transcript most of the height. A read-only transcript (a sub-agent's
        // child view) drops both — the input belongs to another conversation.
        if self.show_composer {
            column = column.with_child(Divider::horizontal().finish());
            column = column.with_child(composer);
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
}
