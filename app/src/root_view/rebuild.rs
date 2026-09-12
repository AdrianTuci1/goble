//! The per-frame rebuild: snapshots, per-pane composer context and tree.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use goble_ui::elements::PopupMenuItem;
use goble_ui::AppContext;

use crate::actions::make_actions;
use crate::ai::make_ai_actions;
use crate::media::make_media_actions;
use crate::projects::make_projects_actions;
use crate::screen::make_screen_actions;
use crate::ui::{
    build_ui, AiSnapshot, ComposerContext, MediaSnapshot, ProjectsSnapshot, ScreenFrameSnapshot,
    ScreenSnapshot, UiSnapshot,
};

use super::RootView;

impl RootView {
    /// Rebuild the element tree from the current app state.
    pub(super) fn rebuild(&mut self, app: &AppContext) {
        self.drain_events();
        // Advance any running screen-event replay by real time (one step per
        // frame). The schedule itself is deterministic; only the pacing uses
        // the wall clock, and tests drive it through `step_replay`.
        self.screen_state
            .borrow_mut()
            .tick(self.desktop.as_deref());
        // Ensure every pane has a per-pane agent-header 3-dots menu flag before
        // the snapshot is built, so the tray open state is independent per pane.
        self.state.borrow_mut().ensure_agent_menu_flags();
        // Give every rendered pane its own rich-input controls entry before the
        // snapshot is built, so two pty/agent panes never share the composer's
        // buttons.
        self.state.borrow_mut().ensure_pane_controls();
        // Derive the composer context pills (harness = the selected harness,
        // working dir = the selected session's path, branch = git branch) from
        // the environment tree + the active pane's cwd. The harness pill lists
        // the work environment (medium) the active pane runs on; the sidebar
        // hosts the same selector, this pill is the quick overlay.
        let composer_context = {
            let media = self.media_state.borrow();
            let state_s = self.state.borrow();
            let harness_label = media
                .mediums
                .iter()
                .find(|m| m.id == media.selected_medium)
                .map(|m| m.label.clone())
                .unwrap_or_else(|| media.selected_medium.clone());
            let mut harness_ids = Vec::new();
            let mut harness_items = Vec::new();
            for m in &media.mediums {
                harness_ids.push(m.id.clone());
                let mut item = PopupMenuItem::new(m.label.clone()).with_icon("computer");
                if m.id == media.selected_medium {
                    item = item.selected();
                }
                harness_items.push(item);
            }
            let session_path = media.selected_session_path();
            let dir_label = if session_path.is_empty() {
                state_s.composer_path.clone()
            } else {
                session_path
            };
            let dir_label = crate::state::display_path(&dir_label);
            let mut dir_ids = Vec::new();
            let mut dir_items = Vec::new();
            if let Some(m) = media.mediums.iter().find(|m| m.id == media.selected_medium) {
                if let Some(proj) = m.projects.iter().find(|p| p.id == media.selected_project) {
                    for s in &proj.sessions {
                        dir_ids.push(s.id.clone());
                        let mut item = PopupMenuItem::new(crate::state::display_path(&s.path))
                            .with_icon("folder");
                        if s.id == media.selected_session {
                            item = item.selected();
                        }
                        dir_items.push(item);
                    }
                }
            }
            let branch_label = state_s.composer_branch.clone();
            let branch_ids = if branch_label.is_empty() {
                Vec::new()
            } else {
                vec![branch_label.clone()]
            };
            let branch_items = if branch_label.is_empty() {
                Vec::new()
            } else {
                vec![PopupMenuItem::new(branch_label.clone())
                    .with_icon("git-branch")
                    .selected()]
            };
            ComposerContext {
                harness_label,
                harness_ids,
                harness_items,
                harness_menu_open: state_s.harness_menu_open.clone(),
                dir_label,
                dir_ids,
                dir_items,
                dir_menu_open: state_s.dir_menu_open.clone(),
                branch_label,
                branch_ids,
                branch_items,
                branch_menu_open: state_s.branch_menu_open.clone(),
            }
        };
        let snapshot = {
            let s = self.state.borrow();
            let mut pane_chat = s.pane_chat_snapshot();
            // The topbar's live cue reads C1's one accessor once; the count and
            // phase are carried on the snapshot so `build_topbar` stays pure.
            let live_work = s.live_work();
            // Overlay the live inline handoff frame onto panes whose harness
            // handed control to the selected screen source, so the remote
            // desktop renders inside the chat's harness area.
            {
                let screen = self.screen_state.borrow();
                if let Some(frame) = &screen.frame {
                    let selected = screen.selected_source.clone();
                    let frame_snap = ScreenFrameSnapshot {
                        frame_seq: screen.frame_seq,
                        width: frame.width,
                        height: frame.height,
                        data: Arc::from(frame.data.clone()),
                    };
                    for (pid, snap) in pane_chat.iter_mut() {
                        let active = s
                            .pane_runtime
                            .get(pid)
                            .and_then(|rt| rt.inline_screen_source.as_ref())
                            .map(|src| src == &selected)
                            .unwrap_or(false);
                        if active {
                            snap.inline_screen = Some(frame_snap.clone());
                        }
                    }
                }
            }
            // Fan the composer pills out per pane: the harness (medium) choice
            // is a window-level environment, but each pane keeps its own
            // working directory, branch and dropdown open flags.
            let composer_context: HashMap<u64, ComposerContext> = {
                let media = self.media_state.borrow();
                let media_dir = media.selected_session_path();
                pane_chat
                    .keys()
                    .map(|&pid| {
                        let mut ctx = composer_context.clone();
                        let controls = s.pane_controls(pid);
                        ctx.harness_menu_open = controls.harness_menu_open.clone();
                        ctx.dir_menu_open = controls.dir_menu_open.clone();
                        ctx.branch_menu_open = controls.branch_menu_open.clone();
                        if media_dir.is_empty() {
                            if let Some(pane) = pane_chat.get(&pid) {
                                if !pane.composer_path.is_empty() {
                                    ctx.dir_label =
                                        crate::state::display_path(&pane.composer_path);
                                }
                            }
                        }
                        ctx.branch_label = controls.branch.clone();
                        if ctx.branch_label.is_empty() {
                            ctx.branch_ids = Vec::new();
                            ctx.branch_items = Vec::new();
                        } else {
                            ctx.branch_ids = vec![ctx.branch_label.clone()];
                            ctx.branch_items = vec![PopupMenuItem::new(ctx.branch_label.clone())
                                .with_icon("git-branch")
                                .selected()];
                        }
                        (pid, ctx)
                    })
                    .collect()
            };
            UiSnapshot {
                current_tab: s.current_tab,
                conversations: s.conversations.clone(),
                selected_id: s.selected_id.clone(),
                search_query: s.search_query.clone(),
                search_focused: s.search_focused,
                new_conversation_draft: s.new_conversation_draft.clone(),
                create_focused: s.create_focused,
                chat_messages: s.chat_messages.clone(),
                pending_ask: s.pending_ask.clone(),
                queued_prompt: s.queued_prompt.clone(),
                composer_draft: s.composer_draft.clone(),
                composer_focused: s.composer_focused,
                models: s.models.clone(),
                selected_model: s.selected_model.clone(),
                harnesses: s.harnesses.clone(),
                selected_harness: s.selected_harness.clone(),
                model_menu_open: s.model_menu_open.clone(),
                agent_name: s.agent_name.clone(),
                agent_busy: s.agent_busy,
                live_work_count: live_work.work_count(),
                live_work_phase: live_work.spinner_phase(),
                auto_approve: s.auto_approve,
                right_sidebar_open: s.right_sidebar_open,
                fullscreen: s.fullscreen,
                agent_header_menus: s.agent_header_menus.clone(),
                terminal_filters: s.terminal_filters.clone(),
                reasoning_expanded: s.reasoning_expanded.clone(),
                tool_fold: s.tool_fold.clone(),
                terminal_global_filters: s.terminal_global_filters.clone(),
                pane_terminal_scroll: s.pane_terminal_scroll.clone(),
                crons_open: s.crons_open,
                crons: s.crons.clone(),
                workflows: s.workflows.clone(),
                executions: s.executions.clone(),
                tasks: s.tasks.clone(),
                timeline: s.timeline.clone(),
                costs: s.costs.clone(),
                settings_page: s.settings_page,
                settings_profile_name: s.settings_profile_name.clone(),
                settings_profile_email: s.settings_profile_email.clone(),
                settings_dark_mode: s.settings_dark_mode,
                settings_llm_provider: s.settings_llm_provider.clone(),
                settings_llm_model: s.settings_llm_model.clone(),
                settings_llm_api_key: s.settings_llm_api_key.clone(),
                settings_llm_base_url: s.settings_llm_base_url.clone(),
                settings_llm_temperature: s.settings_llm_temperature.clone(),
                settings_workers: s.settings_workers.clone(),
                settings_cluster_name: s.settings_cluster_name.clone(),
                settings_cluster_configured: s.settings_cluster_configured,
                settings_authorized_keys: s.settings_authorized_keys.clone(),
                settings_vault_unlocked: s.settings_vault_unlocked,
                settings_overlay_open: s.settings_overlay_open,
                settings_category: s.settings_category,
                settings_invert_scroll: s.settings_invert_scroll,
                settings_scroll_speed: s.settings_scroll_speed,
                settings_font_size: s.settings_font_size,
                vim_mode: s.vim_mode,
                theme_primary: s.theme_primary.clone(),
                theme_secondary: s.theme_secondary.clone(),
                theme_accent: s.theme_accent.clone(),
                theme_color_target: s.theme_color_target,
                theme_color_drag: s.theme_color_drag.clone(),
                show_llm_key_banner: s.show_llm_key_banner,
                show_workspace_choice: s.show_workspace_choice,
                workspace_routing: s.workspace_routing,
                llm_dialog_open: s.llm_dialog_open,
                llm_dialog_provider: s.llm_dialog_provider.clone(),
                llm_dialog_model: s.llm_dialog_model.clone(),
                llm_dialog_api_key: s.llm_dialog_api_key.clone(),
                llm_dialog_base_url: s.llm_dialog_base_url.clone(),
                llm_dialog_temperature: s.llm_dialog_temperature.clone(),
                llm_dialog_focus: s.llm_dialog_focus.clone(),
                spaces: s.spaces.clone(),
                active_space: s.active_space,
                space_hover: s.space_hover,
                space_press: s.space_press,
                space_drag: s.space_drag,
                active_pane_id: s.active_pane_id,
                dragging_pane_id: s.dragging_pane_id,
                composer_path: s.composer_path.clone(),
                composer_context,
                pane_hover: s.pane_hover.clone(),
                pane_chat,
                pane_controls: s.pane_controls.clone(),
                terminal: s.terminal.clone(),
                sidebar_width: s.sidebar_width,
                sidebar_dragging: s.sidebar_dragging,
                sidebar_visible: s.sidebar_visible,
                conversations_expanded: s.conversations_expanded,
                sidebar_scroll: s.sidebar_scroll.clone(),
                settings_scroll: s.settings_scroll.clone(),
                space_rename_editing: s.space_rename_editing,
                space_rename_draft: s.space_rename_draft.clone(),
                space_rename_focused: s.space_rename_focused,
                agent_cards: s.agent_cards.clone(),
                new_agent_hover: s.new_agent_hover.clone(),
                command_palette_open: s.command_palette_open,
                command_palette_query: s.command_palette_query.clone(),
                command_palette_index: s.command_palette_index,
                show_onboarding_tip: s.show_onboarding_tip,
                add_space_menu_open: s.add_space_menu_open.clone(),
                env_selector_open: s.env_selector_open.clone(),
                add_medium_dialog_open: s.add_medium_dialog_open,
                add_medium_draft: s.add_medium_draft.clone(),
                add_medium_focused: s.add_medium_focused,
                terminal_run_agent_menu_open: s.terminal_run_agent_menu_open.clone(),
            }
        };
        let actions = make_actions(
            Rc::clone(&self.state),
            self.desktop.clone(),
            Rc::clone(&self.media_state),
            self.window_control.clone(),
            self.ui_zoom.clone(),
        );
        let ai_snapshot = {
            let s = self.ai_state.borrow();
            AiSnapshot {
                connectors_open: s.connectors_open,
                vault_open: s.vault_open,
                vault_unlocked: s.vault_unlocked,
                vault_secrets: s.vault_secrets.clone(),
                vault_unlock_draft: s.vault_unlock_draft.clone(),
                vault_new_key: s.vault_new_key.clone(),
                vault_new_value: s.vault_new_value.clone(),
                vault_error: s.vault_error.clone(),
                connector_search: s.connector_search.clone(),
                connectors: s.connectors.clone(),
                install_open: s.install_open,
                install_editing_id: s.install_editing_id.clone(),
                install_name: s.install_name.clone(),
                install_source: s.install_source.clone(),
                install_source_value: s.install_source_value.clone(),
                install_search_query: s.install_search_query.clone(),
                install_search_results: s.install_search_results.clone(),
                install_selected_secrets: s.install_selected_secrets.clone(),
                install_error: s.install_error.clone(),
                installing: s.installing,
            }
        };
        let ai_actions = make_ai_actions(Rc::clone(&self.ai_state), self.desktop.clone());
        let projects_snapshot = {
            let s = self.projects_state.borrow();
            ProjectsSnapshot {
                projects: s.projects.clone(),
            }
        };
        let projects_actions =
            make_projects_actions(Rc::clone(&self.projects_state), self.desktop.clone());
        let media_snapshot = {
            let s = self.media_state.borrow();
            MediaSnapshot {
                mediums: s.mediums.clone(),
                selected_medium: s.selected_medium.clone(),
                selected_project: s.selected_project.clone(),
                selected_session: s.selected_session.clone(),
                expanded: s.expanded.clone(),
            }
        };
        let media_actions = make_media_actions(
            Rc::clone(&self.media_state),
            Rc::clone(&self.state),
            self.desktop.clone(),
        );
        let screen_snapshot = {
            let s = self.screen_state.borrow();
            ScreenSnapshot {
                open: s.open,
                broadcast: s.broadcast,
                computer_use: s.computer_use,
                selected_source: s.selected_source.clone(),
                sources: s.sources.clone(),
                last_capture: s.last_capture.clone(),
                recording: s.recording,
                replaying: s.replaying,
                replay_loop: s.replay_loop,
                recorded_count: s.recorded.len(),
                replay_status: s.replay_status.clone(),
                frame: s.frame.as_ref().map(|f| ScreenFrameSnapshot {
                    frame_seq: s.frame_seq,
                    width: f.width,
                    height: f.height,
                    data: Arc::from(f.data.clone()),
                }),
            }
        };
        let screen_actions =
            make_screen_actions(Rc::clone(&self.screen_state), self.desktop.clone());
        self.element = build_ui(
            app,
            &snapshot,
            &actions,
            &ai_snapshot,
            &ai_actions,
            &projects_snapshot,
            &projects_actions,
            &media_snapshot,
            &media_actions,
            &screen_snapshot,
            &screen_actions,
        );
        // Register the app commands the native menubar (macOS) dispatches into.
        // The closures are rebuilt each frame, so the menu always triggers the
        // current actions. `window_control` is the same shared handle the mac
        // native menu holds (installation happens in goble-ui window.rs).
        {
            let mut commands: HashMap<String, Rc<RefCell<dyn FnMut()>>> = HashMap::new();
            commands.insert("new_space".into(), actions.on_add_space.clone());
            commands.insert("split_right".into(), actions.on_split_right.clone());
            commands.insert("split_down".into(), actions.on_split_down.clone());
            commands.insert("new_terminal".into(), actions.on_new_terminal.clone());
            commands.insert("close_pane".into(), actions.on_close_pane.clone());
            commands.insert("toggle_fullscreen".into(), actions.on_toggle_fullscreen.clone());
            commands.insert("clear_transcript".into(), actions.on_clear_transcript.clone());
            commands.insert("toggle_right_sidebar".into(), actions.on_toggle_right_sidebar.clone());
            commands.insert("open_settings".into(), actions.on_settings.clone());
            commands.insert("copy".into(), actions.on_copy.clone());
            *self.window_control.commands.borrow_mut() = commands;
        }
        self.actions = Some(actions);
    }
}
