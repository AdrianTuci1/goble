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
use crate::state::PaneWorkItem;
use crate::ui::pickers::PARENT_LABEL;
use crate::ui::{
    build_ui, AiSnapshot, ComposerContext, MediaSnapshot, ProjectsSnapshot, ScreenFrameSnapshot,
    ScreenSnapshot, SidebarView, UiSnapshot,
};

use super::RootView;

impl RootView {
    /// Rebuild the element tree from the current app state.
    pub(super) fn rebuild(&mut self, app: &AppContext) {
        self.drain_events();
        // Collect the global search the worker has finished, if it answered the
        // query the field holds now. The walk runs off this thread, so this is
        // where its answer reaches the rows the sidebar draws.
        self.state.borrow_mut().take_global_search_result();
        // Advance any running screen-event replay by real time (one step per
        // frame). The schedule itself is deterministic; only the pacing uses
        // the wall clock, and tests drive it through `step_replay`. The held
        // frame is refreshed here too, for the source the sheet is broadcasting
        // and for one any conversation is showing inline: the card is a viewer
        // of its own, so a handed-off desktop streams with the sheet closed.
        let watched = {
            let state = self.state.borrow();
            state.inline_screen_viewers(&self.screen_state.borrow().selected_source) > 0
        };
        self.screen_state
            .borrow_mut()
            .tick(self.desktop.as_deref(), watched);
        // Follow what every pane's own shell reports as its working directory
        // before anything reads a path: the rich input's directory pill, the tab
        // label that derives from it and the explorer all draw from the pane's
        // session, and a `cd` at the pane's prompt moves all three.
        self.state.borrow_mut().adopt_reported_cwds();
        // A tab's name comes from what it holds: keep every tab nobody has
        // renamed labelled by its focused pane (the working directory of a
        // terminal, the conversation's subject of an agent) before the snapshot
        // the topbar draws from is taken. A tab the user renamed is untouched.
        self.state.borrow_mut().refresh_space_labels();
        // Give every rendered pane its own rich-input controls entry before the
        // snapshot is built, so two pty/agent panes never share the composer's
        // buttons.
        self.state.borrow_mut().ensure_pane_controls();
        // The Connections page's `~/.ssh` read is cached in app state, like the
        // picker and explorer caches beside it: it is refilled when the page is
        // shown (or by its reload row) and never while the page draws.
        self.state.borrow_mut().ensure_ssh_hosts();
        // The composer's context pills are the pane's own: the environment the
        // pane's conversation runs on (a conversation on a worker picks one) and
        // the working directory and branch, which describe one shell. All three
        // are filled in per pane below; what is built here is the frame they
        // share.
        let composer_context = {
            let state_s = self.state.borrow();
            ComposerContext {
                harness_label: String::new(),
                harness_ids: Vec::new(),
                harness_items: Vec::new(),
                harness_menu_open: state_s.harness_menu_open.clone(),
                dir_menu_open: state_s.dir_menu_open.clone(),
                dir_menu_scroll: state_s.dir_menu_scroll.clone(),
                dir_ids: Vec::new(),
                dir_items: Vec::new(),
                branch_label: String::new(),
                branch_ids: Vec::new(),
                branch_items: Vec::new(),
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
                        source: frame.source.clone(),
                        driver: crate::screen::screen_driver(self.desktop.as_deref(), &selected),
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
            // is window-level, but each pane keeps its own working directory,
            // branch and dropdown open flags. The directory and branch menus are
            // the pane's own two, and both are windows onto the real machine:
            // the directory lists what sits under the pane's cwd, the branch
            // lists the repository's local branches. Neither is read while its
            // menu is closed, which is why the rows arrive with the open flag.
            let composer_context: HashMap<u64, ComposerContext> = {
                let mut pickers = self.pickers.borrow_mut();
                let media = self.media_state.borrow();
                pane_chat
                    .keys()
                    .map(|&pid| {
                        let mut ctx = composer_context.clone();
                        let controls = s.pane_controls(pid);
                        let cwd = s
                            .pane_sessions
                            .get(&pid)
                            .map(|session| session.path.clone())
                            .unwrap_or_default();
                        ctx.harness_menu_open = controls.harness_menu_open.clone();

                        // The environment control belongs to the conversation
                        // that runs on a worker, and to it alone: the pane names
                        // the medium its own conversation runs on (A3), and the
                        // rows are the environments the app has. Every other
                        // pane is given no label, so no environment control is
                        // drawn for it — a local conversation has no environment
                        // of its own to choose.
                        ctx.harness_label = String::new();
                        ctx.harness_ids = Vec::new();
                        ctx.harness_items = Vec::new();
                        if let Some(environment) = s.pane_environment(pid) {
                            ctx.harness_label = media
                                .mediums
                                .iter()
                                .find(|m| m.id == environment)
                                .map(|m| m.label.clone())
                                .unwrap_or_else(|| environment.clone());
                            ctx.harness_ids = media.mediums.iter().map(|m| m.id.clone()).collect();
                            ctx.harness_items = media
                                .mediums
                                .iter()
                                .map(|m| {
                                    let mut item = PopupMenuItem::new(m.label.clone())
                                        .with_icon("computer");
                                    if m.id == environment {
                                        item = item.selected();
                                    }
                                    item
                                })
                                .collect();
                        }

                        ctx.dir_menu_open = controls.dir_menu_open.clone();
                        ctx.dir_menu_scroll = controls.dir_menu_scroll.clone();
                        let dir_rows =
                            pickers.directories(&cwd, *controls.dir_menu_open.borrow());
                        ctx.dir_ids = dir_rows.iter().map(|row| row.path.clone()).collect();
                        ctx.dir_items = dir_rows
                            .iter()
                            .map(|row| {
                                let icon = if row.label == PARENT_LABEL {
                                    "arrow-up"
                                } else {
                                    "folder"
                                };
                                let mut item =
                                    PopupMenuItem::new(row.label.clone()).with_icon(icon);
                                if row.path == cwd {
                                    item = item.selected();
                                }
                                item
                            })
                            .collect();

                        // The pill names where the pane is; a directory that is
                        // no repository has no branch and draws no branch pill.
                        ctx.branch_label = controls.branch.clone();
                        ctx.branch_menu_open = controls.branch_menu_open.clone();
                        let branch_rows =
                            pickers.branches(&cwd, *controls.branch_menu_open.borrow());
                        ctx.branch_ids = branch_rows.iter().map(|row| row.name.clone()).collect();
                        ctx.branch_items = branch_rows
                            .iter()
                            .map(|row| {
                                let mut item =
                                    PopupMenuItem::new(row.name.clone()).with_icon("git-branch");
                                if row.current {
                                    item = item.selected();
                                }
                                item
                            })
                            .collect();
                        (pid, ctx)
                    })
                    .collect()
            };
            // The project explorer is the active pane's working directory. Its
            // rows are read only while the explorer is the view on screen; the
            // cache lives beside the state, so a frame that changes nothing
            // reads no directory.
            let explorer_root = s.active_working_directory();
            let explorer_rows = self.explorer.borrow_mut().rows(
                &explorer_root,
                &s.explorer_expanded,
                s.sidebar_view == SidebarView::Explorer,
            );
            // Every file view on screen reads its file here, once — when the
            // file changed — instead of per frame in the element tree, and the
            // same pass highlights the lines it just read. Only the active
            // space is mounted, so only its file views are read.
            let pane_files = {
                let mut wanted = Vec::new();
                if let Some(space) = s.spaces.get(s.active_space) {
                    space.root.file_leaves(&mut wanted);
                }
                self.file_cache.borrow_mut().read(&wanted)
            };
            // The pane header's work chip is per pane, where `live_work` above
            // is app-wide, so it is built from each pane's own runtime. A pane
            // with nothing in flight gets no entry and draws no chip.
            let pane_live_work: HashMap<u64, Vec<PaneWorkItem>> = s
                .pane_runtime
                .keys()
                .map(|&pane_id| (pane_id, s.pane_live_work(pane_id)))
                .filter(|(_, items)| !items.is_empty())
                .collect();
            // The editing state sits beside the read it was settled against, so
            // it is taken in the same breath.
            let file_buffers = self.file_cache.borrow().buffers();
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
                pane_live_work,
                auto_approve: s.auto_approve,
                right_sidebar_open: s.right_sidebar_open,
                fullscreen: s.fullscreen,
                maximized_pane: s.maximized_pane,
                terminal_filters: s.terminal_filters.clone(),
                reasoning_expanded: s.reasoning_expanded.clone(),
                tool_fold: s.tool_fold.clone(),
                terminal_global_filters: s.terminal_global_filters.clone(),
                pane_terminal_scroll: s.pane_terminal_scroll.clone(),
                crons_open: s.crons_open,
                task_workflow_open: s.task_workflow_open,
                task_workflow_selected: s.task_workflow_selected.clone(),
                task_workflow_phase: s.task_workflow_phase.clone(),
                shortcuts_help_open: s.shortcuts_help_open,
                shortcuts_help_filter: s.shortcuts_help_filter.clone(),
                shortcuts_help_index: s.shortcuts_help_index,
                shortcuts_help_scroll: s.shortcuts_help_scroll.clone(),
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
                settings_category: s.settings_page(),
                settings_focus: s.settings_focus,
                settings_pane_focus: s.settings_pane_focus,
                settings_pane_field_active: s.settings_pane_field_active,
                settings_environment_groups: s.settings_environment_groups.clone(),
                settings_environment_open_group: s.settings_environment_open_group.clone(),
                settings_environment_group_draft: s.settings_environment_group_draft.clone(),
                settings_environment_secret_name: s.settings_environment_secret_name.clone(),
                settings_environment_secret_value: s.settings_environment_secret_value.clone(),
                settings_environment_editing: s.settings_environment_editing.clone(),
                settings_ssh_hosts: s.settings_ssh_hosts.clone(),
                settings_ssh_selected: s.settings_ssh_selected.clone(),
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
                llm_notice_heading: s.llm_notice_heading(),
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
                pane_drag: s.pane_drag.clone(),
                active_pane_id: s.active_pane_id,
                dragging_pane_id: s.dragging_pane_id,
                composer_path: s.composer_path.clone(),
                composer_context,
                pane_hover: s.pane_hover.clone(),
                pane_chat,
                pane_controls: s.pane_controls_snapshot(),
                terminal: s.terminal.clone(),
                sidebar_width: s.sidebar_width,
                sidebar_dragging: s.sidebar_dragging,
                sidebar_visible: s.sidebar_visible,
                conversations_expanded: s.conversations_expanded,
                sidebar_view: s.sidebar_view,
                starred: {
                    let mut ids: Vec<String> = s.starred_conversations.iter().cloned().collect();
                    ids.sort();
                    ids
                },
                collapsed_sections: {
                    let mut keys: Vec<String> = s.collapsed_sections.iter().cloned().collect();
                    keys.sort();
                    keys
                },
                explorer_root: explorer_root.clone(),
                explorer_rows,
                explorer_hover: s.explorer_hover.clone(),
                global_search_query: s.global_search_query.clone(),
                global_search_rows: s.global_search_rows.clone(),
                global_search_searched: s.global_search_searched,
                global_search_error: s.global_search_error.clone(),
                global_search_focused: s.global_search_focused,
                sidebar_scroll: s.sidebar_scroll.clone(),
                explorer_scroll: s.explorer_scroll.clone(),
                global_search_scroll: s.global_search_scroll.clone(),
                settings_scroll: s.settings_scroll.clone(),
                space_rename_editing: s.space_rename_editing,
                space_rename_draft: s.space_rename_draft.clone(),
                space_rename_focused: s.space_rename_focused,
                space_menu: s.space_menu.clone(),
                agent_cards: s.agent_cards.clone(),
                new_agent_hover: s.new_agent_hover.clone(),
                command_palette_open: s.command_palette_open,
                command_palette_query: s.command_palette_query.clone(),
                command_palette_index: s.command_palette_index,
                show_onboarding_tip: s.show_onboarding_tip,
                add_space_menu_open: s.add_space_menu_open.clone(),
                env_selector_open: s.env_selector_open.clone(),
                env_selector_hover: s.env_selector_hover.clone(),
                add_medium_dialog_open: s.add_medium_dialog_open,
                add_medium_draft: s.add_medium_draft.clone(),
                add_medium_focused: s.add_medium_focused,
                terminal_run_agent_menu_open: s.terminal_run_agent_menu_open.clone(),
                file_scroll: s.file_scroll.clone(),
                pane_files,
                file_buffers,
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
                    source: f.source.clone(),
                    driver: crate::screen::screen_driver(
                        self.desktop.as_deref(),
                        &s.selected_source,
                    ),
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
            // The Window menu's tab-strip items. The action takes a direction,
            // so each menu command gets its own no-argument wrapper.
            let next_space = actions.on_switch_space.clone();
            commands.insert(
                "next_space".into(),
                Rc::new(RefCell::new(move || (next_space.borrow_mut())(1))),
            );
            let previous_space = actions.on_switch_space.clone();
            commands.insert(
                "previous_space".into(),
                Rc::new(RefCell::new(move || (previous_space.borrow_mut())(-1))),
            );
            *self.window_control.commands.borrow_mut() = commands;
        }
        self.actions = Some(actions);
    }
}
