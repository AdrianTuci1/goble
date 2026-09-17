use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{AppContext, Chip, Clipped, ComposerButton, COMPOSER_CONTROL_RADIUS, ConstrainedBox, Container, ContextPill, CrossAxisAlignment, EdgeInsets, Element, Flex, Icon, LayoutContext, MainAxisSize, Padding, PanelScroll, PaintContext, PillTraySide, Point, PopupMenu, PopupMenuItem, PopupMenuPosition, ShortcutHint, ShortcutHints, SizeConstraint, SlashMenuItem, Text, TextArea, Tooltip, TooltipPosition, Wrap, MENU_MAX_VISIBLE_ROWS};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::{PointF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};
use crate::vim::{Clipboard, VimMode, VimState};
use goble_core::harness::CommandDecision;
use goble_core::ssh_command::SshSession;
use super::proposal::CommandProposalUi;

/// Cap for the working-directory pill's label. A long path ellipsizes inside the
/// pill instead of pushing the other context pills across the row.
const DIR_PILL_MAX_WIDTH: f32 = 280.0;

/// The same cap for the session chip: a long `user@host (address)` narrows
/// inside its own box instead of pushing the context row out of the pane.
const SESSION_CHIP_MAX_WIDTH: f32 = 280.0;

/// What the model control reads when the pane has no model configured: the
/// turn would run on nothing, so naming a model there would be a claim nobody
/// made. The host's own resolved label says the same thing (its model value is
/// empty until a model is configured), and this covers the label it passes.
pub const MODEL_NOT_CONFIGURED: &str = "not configured";

pub struct ChatComposer {
    pub(super) value: Rc<RefCell<String>>,
    /// Where the editor's insertion beam sits (a character index into `value`).
    /// App-owned, like the draft itself: the composer element is rebuilt every
    /// frame, so an index kept inside it would reset on every one of them.
    caret: Rc<RefCell<usize>>,
    placeholder: String,
    attachments: Vec<String>,
    model_label: Option<String>,
    path_label: Option<String>,
    /// The SSH session this pane's shell is bound to, when a submitted `ssh`
    /// line put it on a host (S1). While it is set the context row draws a chip
    /// naming the host; a local shell has none, so the chip's absence is itself
    /// the statement that the session is local.
    ssh_session: Option<SshSession>,
    pub(super) focused: bool,
    /// Whether a press outside the editor blurs it. Off for a host that makes
    /// this composer the only typing surface of its pane (the terminal pane):
    /// clicking the output above the bar must leave the caret where it was.
    blur_on_outside_click: bool,
    /// Draw the context row (the harness/directory/branch pills) above the
    /// editor instead of under it. A shell pane's bar keeps its own context
    /// over the draft, where the command it describes is typed.
    context_above_editor: bool,
    /// The instructions drawn in the rich input's own bottom row, under the
    /// editor and the action row — the gestures this surface answers that the
    /// host keeps inside the input. An empty list draws nothing.
    ///
    /// The strip a pane shows over its input is the host's, drawn above the
    /// separator: what belongs *in* the input is the one entry the host marks
    /// as its own, which is the shell bar's `⌘↵ new conversation`.
    hints: Vec<ShortcutHint>,
    /// The slash-command menu: the commands the draft can run, drawn above the
    /// editor while the draft starts with `/`. `slash_enabled` records that the
    /// host offers one at all, so a query that matches nothing still shows the
    /// menu (with its own empty state) instead of silently closing it.
    slash_enabled: bool,
    slash_items: Vec<SlashMenuItem>,
    slash_index: Rc<RefCell<usize>>,
    /// Whether Escape has put the menu away for the draft as it stands. Shared
    /// with the host so the dismissal survives the per-frame rebuild.
    slash_dismissed: Rc<RefCell<bool>>,
    on_slash_move: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_slash_accept: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_slash_dismiss: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    stop_visible: bool,
    /// warp-new style context pills: the agent harness, the working directory
    /// and the git branch, each with its own dropdown selector. The working
    /// directory reuses `path_label` (set via `with_path_label`) as its label.
    harness_label: Option<String>,
    harness_menu_items: Vec<PopupMenuItem>,
    harness_menu_open: Rc<RefCell<bool>>,
    on_select_harness_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    dir_menu_items: Vec<PopupMenuItem>,
    dir_menu_open: Rc<RefCell<bool>>,
    /// The directory tray's scroll offset, owned by the app for the same reason
    /// its open flag is: the tray is capped at [`MENU_MAX_VISIBLE_ROWS`] rows, so
    /// a deep directory scrolls, and the per-frame rebuild would otherwise put
    /// it back at the top.
    dir_menu_scroll: PanelScroll,
    on_select_dir_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    branch_label: Option<String>,
    branch_menu_items: Vec<PopupMenuItem>,
    branch_menu_open: Rc<RefCell<bool>>,
    on_select_branch_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    /// The command the harness is waiting on an approval for (A6). While set,
    /// the composer shows the proposal card and the editor holds the selected
    /// candidate so it can be run as-is or edited in place.
    pub(super) proposal: Option<CommandProposalUi>,
    /// Which candidate is selected. App-owned, like the footer menus' `open`
    /// flags, because the composer element is rebuilt every frame.
    pub(super) proposal_selected: Rc<RefCell<usize>>,
    pub(super) on_decision: Option<Rc<RefCell<dyn FnMut(String, CommandDecision) + 'static>>>,
    pub(super) on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    /// Cmd/Ctrl+Enter submit: start a NEW agent conversation (warp-new). The
    /// host decides what "new conversation" means; the composer only reports
    /// the keybinding.
    on_cmd_enter: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    /// Cmd/Ctrl+Alt+Enter submit: hand the draft to the cloud agent
    /// (warp-new's `⌘⌥⏎`). The host owns what "cloud" means — the routing
    /// choice and the transport — so the composer only reports the chord.
    on_send_to_cloud: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_attach: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_model: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_key: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_variant: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_voice: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_image: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_code: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_link: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_stop: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    model_menu_items: Vec<PopupMenuItem>,
    model_menu_open: Rc<RefCell<bool>>,
    on_select_model_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    /// The row the model band has chosen, host-owned like the open flag: the
    /// composer is rebuilt every frame, so an index kept inside it would reset
    /// on every one of them. Shared with the model menu when the host wires
    /// one, so the highlight survives between frames.
    model_index: Rc<RefCell<usize>>,
    /// Modal (vim) editing for the editor, when the host turned it on. The
    /// state lives with the host (the element is rebuilt every frame) and the
    /// footer draws the mode badge from it.
    vim: Option<Rc<RefCell<VimState>>>,
    /// The system clipboard the engine's `"+`/`"*` registers reach.
    clipboard: Option<Rc<RefCell<dyn Clipboard>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            value: Rc::new(RefCell::new(String::new())),
            caret: Rc::new(RefCell::new(0)),
            placeholder: String::from("Ask anything..."),
            attachments: Vec::new(),
            model_label: None,
            path_label: None,
            ssh_session: None,
            focused: false,
            blur_on_outside_click: true,
            context_above_editor: false,
            hints: Vec::new(),
            slash_enabled: false,
            slash_items: Vec::new(),
            slash_index: Rc::new(RefCell::new(0)),
            slash_dismissed: Rc::new(RefCell::new(false)),
            on_slash_move: None,
            on_slash_accept: None,
            on_slash_dismiss: None,
            stop_visible: false,
            on_change: None,
            on_send: None,
            on_cmd_enter: None,
            on_send_to_cloud: None,
            on_attach: None,
            on_select_model: None,
            on_select_key: None,
            on_select_variant: None,
            on_voice: None,
            on_image: None,
            on_code: None,
            on_link: None,
            on_stop: None,
            on_focus_change: None,
            model_menu_items: Vec::new(),
            model_menu_open: Rc::new(RefCell::new(false)),
            on_select_model_item: None,
            model_index: Rc::new(RefCell::new(0)),
            vim: None,
            clipboard: None,
            harness_label: None,
            harness_menu_items: Vec::new(),
            harness_menu_open: Rc::new(RefCell::new(false)),
            on_select_harness_item: None,
            dir_menu_items: Vec::new(),
            dir_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_scroll: PanelScroll::default(),
            on_select_dir_item: None,
            branch_label: None,
            branch_menu_items: Vec::new(),
            branch_menu_open: Rc::new(RefCell::new(false)),
            on_select_branch_item: None,
            proposal: None,
            proposal_selected: Rc::new(RefCell::new(0)),
            on_decision: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_value(self, value: impl Into<String>) -> Self {
        *self.value.borrow_mut() = value.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_attachments(mut self, attachments: Vec<String>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn with_model_label(mut self, label: impl Into<String>) -> Self {
        self.model_label = Some(label.into());
        self
    }

    pub fn with_path_label(mut self, path: impl Into<String>) -> Self {
        self.path_label = Some(path.into());
        self
    }

    /// The SSH session the pane's shell is bound to, or `None` for a local
    /// shell. It is the bound session itself, not a label: the chip's words are
    /// the session's own ([`SshSession::chip_label`]). A local session draws no
    /// chip at all.
    pub fn with_ssh_session(mut self, session: Option<SshSession>) -> Self {
        self.ssh_session = session;
        self
    }

    /// Label for the environment (medium) context pill: the environment the
    /// draft's turn will run on. The host draws it only where the environment is
    /// the user's to choose — a conversation running on a worker — so a host
    /// that passes no label draws no pill.
    pub fn with_harness_label(mut self, label: impl Into<String>) -> Self {
        self.harness_label = Some(label.into());
        self
    }

    /// Set the environment (medium) dropdown: items, the app-owned `open` flag,
    /// and a select callback (same contract as `with_model_menu`).
    pub fn with_harness_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.harness_menu_items = items;
        self.harness_menu_open = open;
        self.on_select_harness_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the working-directory dropdown (items, open flag, select callback).
    pub fn with_dir_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.dir_menu_items = items;
        self.dir_menu_open = open;
        self.on_select_dir_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Share the directory tray's scroll offset with the app, so a tray taller
    /// than its cap (`MENU_MAX_VISIBLE_ROWS` rows) keeps the position the wheel
    /// left it at across the per-frame rebuild.
    pub fn with_dir_menu_scroll(mut self, scroll: PanelScroll) -> Self {
        self.dir_menu_scroll = scroll;
        self
    }

    /// Label for the git-branch context pill.
    pub fn with_branch_label(mut self, label: impl Into<String>) -> Self {
        self.branch_label = Some(label.into());
        self
    }

    /// Set the git-branch dropdown (items, open flag, select callback).
    pub fn with_branch_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.branch_menu_items = items;
        self.branch_menu_open = open;
        self.on_select_branch_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Show the slash-command menu above the editor while the draft starts
    /// with `/`. `items` are the commands the host offers for the current
    /// draft, `index` is the chosen row (host-owned, like every other menu's
    /// open flag), `dismissed` is the host's Escape flag, and the callbacks are
    /// the host's: move the selection, run it, put the menu away.
    pub fn with_slash_menu(
        mut self,
        items: Vec<SlashMenuItem>,
        index: Rc<RefCell<usize>>,
        dismissed: Rc<RefCell<bool>>,
    ) -> Self {
        self.slash_enabled = true;
        self.slash_items = items;
        self.slash_index = index;
        self.slash_dismissed = dismissed;
        self
    }

    /// Fired when the selection moves (arrow keys, or the pointer onto a row).
    pub fn with_on_slash_move<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_slash_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when a row is run: Enter, Tab, or a click on it.
    pub fn with_on_slash_accept<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_slash_accept = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired on Escape: the menu closes and the draft stays as typed.
    pub fn with_on_slash_dismiss<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_slash_dismiss = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether the draft's own command list is the band the input shows: the
    /// draft is a command and Escape has not put its list away. Both bands are
    /// the host's to draw in that one slot, and both the host and this composer
    /// read this predicate, so the two can never be up together (see `rebuild`).
    fn slash_band_open(&self) -> bool {
        self.slash_enabled
            && crate::elements::slash_menu_open(
                &self.value.borrow(),
                *self.slash_dismissed.borrow(),
            )
    }

    /// The keys that pick from the models, taken before the editor sees them:
    /// Up/Down move the selection, Enter or Tab runs it, Escape puts the list
    /// away. The list itself is the host's, drawn in the slot the command list
    /// takes over the input; what the composer keeps of it is the key routing,
    /// because the editor owns the keyboard.
    ///
    /// The command list takes the keys while it is up: the two bands share one
    /// slot over the input, and the draft's own list is the one that holds it.
    fn handle_model_key(&mut self, event: &DispatchedEvent) -> bool {
        if !*self.model_menu_open.borrow()
            || self.on_select_model_item.is_none()
            || self.slash_band_open()
        {
            return false;
        }
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        match key.as_str() {
            "ArrowDown" | "ArrowUp" => {
                let len = self.model_menu_items.len();
                if len == 0 {
                    return true;
                }
                let selected = (*self.model_index.borrow()).min(len - 1);
                let target = if key == "ArrowDown" {
                    (selected + 1).min(len - 1)
                } else {
                    selected.saturating_sub(1)
                };
                *self.model_index.borrow_mut() = target;
                true
            }
            // Cmd/Ctrl+Enter is the host's own gesture (a new conversation), so
            // it falls through to the editor even with the band up.
            "Enter" | "Tab" if !modifiers.command && !modifiers.ctrl && !modifiers.shift => {
                let len = self.model_menu_items.len();
                let selected = if len == 0 {
                    0
                } else {
                    (*self.model_index.borrow()).min(len - 1)
                };
                let Some(cb) = self.on_select_model_item.clone() else {
                    return false;
                };
                *self.model_menu_open.borrow_mut() = false;
                (cb.borrow_mut())(selected);
                true
            }
            "Escape" => {
                *self.model_menu_open.borrow_mut() = false;
                true
            }
            _ => false,
        }
    }

    /// The keys that pick from the slash menu, taken before the editor sees
    /// them: Up/Down move the selection, Enter or Tab runs it, Escape puts the
    /// menu away. Every other key belongs to the editor, because what it types
    /// is the menu's query — which is why the menu is not a modal.
    fn handle_slash_key(&mut self, event: &DispatchedEvent) -> bool {
        if !self.slash_enabled
            || !crate::elements::slash_menu_open(
                &self.value.borrow(),
                *self.slash_dismissed.borrow(),
            )
        {
            return false;
        }
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        match key.as_str() {
            "ArrowDown" | "ArrowUp" => {
                let len = self.slash_items.len();
                if len == 0 {
                    return true;
                }
                let selected = (*self.slash_index.borrow()).min(len - 1);
                let target = if key == "ArrowDown" {
                    (selected + 1).min(len - 1)
                } else {
                    selected.saturating_sub(1)
                };
                *self.slash_index.borrow_mut() = target;
                if let Some(cb) = self.on_slash_move.clone() {
                    (cb.borrow_mut())(target);
                }
                true
            }
            // Cmd/Ctrl+Enter is the host's own gesture (a new conversation),
            // so it falls through to the editor even with the menu up.
            "Enter" | "Tab" if !modifiers.command && !modifiers.ctrl && !modifiers.shift => {
                let len = self.slash_items.len();
                let selected = if len == 0 {
                    0
                } else {
                    (*self.slash_index.borrow()).min(len - 1)
                };
                if let Some(cb) = self.on_slash_accept.clone() {
                    (cb.borrow_mut())(selected);
                }
                true
            }
            "Escape" => {
                if let Some(cb) = self.on_slash_dismiss.clone() {
                    (cb.borrow_mut())();
                }
                true
            }
            _ => false,
        }
    }

    /// Cmd/Ctrl+Alt+Enter submits the draft to the cloud agent (warp-new's
    /// `⌘⌥⏎`). Taken before the editor sees it, like the menu's keys: the chord
    /// belongs to the host, which owns the routing the cloud means. Shift is
    /// not part of the gesture, and a host that wired no callback keeps the key.
    fn handle_send_to_cloud_key(&mut self, event: &DispatchedEvent) -> bool {
        let DispatchedEvent::KeyDown { key, modifiers } = event else {
            return false;
        };
        if key != "Enter"
            || modifiers.shift
            || !modifiers.alt
            || !(modifiers.command || modifiers.ctrl)
        {
            return false;
        }
        let Some(cb) = self.on_send_to_cloud.clone() else {
            return false;
        };
        let text = self.value.borrow().clone();
        (cb.borrow_mut())(text);
        *self.value.borrow_mut() = String::new();
        true
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Whether a press outside the editor blurs it (the default). A host whose
    /// pane types only here turns it off, so a click on what the pane shows
    /// above the bar does not take the keyboard away from the editor.
    pub fn with_blur_on_outside_click(mut self, blur: bool) -> Self {
        self.blur_on_outside_click = blur;
        self
    }

    /// Draw the context row above the editor rather than under it.
    pub fn with_context_above_editor(mut self, above: bool) -> Self {
        self.context_above_editor = above;
        self
    }

    /// The instructions to draw in the input's own bottom row, under the editor
    /// and the action row. The strip that belongs over the input is the host's;
    /// this is the one entry the host keeps inside the input.
    pub fn with_hints(mut self, hints: Vec<ShortcutHint>) -> Self {
        self.hints = hints;
        self
    }

    /// Share the editor's insertion point with the host, so the beam survives
    /// the per-frame rebuild.
    pub fn with_caret(mut self, caret: Rc<RefCell<usize>>) -> Self {
        self.caret = caret;
        self
    }

    /// Turn on modal (vim) editing for the editor. The mode badge in the
    /// composer's footer reads the same state, so the surface says which mode
    /// the next key belongs to.
    pub fn with_vim(mut self, vim: Rc<RefCell<VimState>>) -> Self {
        self.vim = Some(vim);
        self
    }

    /// The system clipboard the `"+`/`"*` registers read and write.
    pub fn with_clipboard(mut self, clipboard: Rc<RefCell<dyn Clipboard>>) -> Self {
        self.clipboard = Some(clipboard);
        self
    }

    pub fn with_stop_visible(mut self, visible: bool) -> Self {
        self.stop_visible = visible;
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_send<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Cmd/Ctrl+Enter submits the draft as a NEW agent conversation (warp-new).
    pub fn with_on_cmd_enter<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_cmd_enter = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Cmd/Ctrl+Alt+Enter submits the draft to the cloud agent (warp-new's
    /// `⌘⌥⏎`). The host routes the conversation and runs the turn.
    pub fn with_on_send_to_cloud<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send_to_cloud = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_attach<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_attach = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_model<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_key<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_variant<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_variant = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_voice<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_voice = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_image<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_image = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_code<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_code = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_link<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_link = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_stop<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_stop = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the model menu: `items` to show, the app-owned `open` flag (so
    /// open state survives the per-frame rebuild), and a select callback.
    ///
    /// The host draws the models as it draws the slash-command list: one
    /// full-width band over the input, in the same slot, opened by the control
    /// here and taken with Up/Down, Enter or Tab, Escape or a click on a row.
    pub fn with_model_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.model_menu_items = items;
        self.model_menu_open = open;
        self.on_select_model_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Share the model list's chosen row with the host, so the highlight
    /// survives the per-frame rebuild the way the open flag does. A host that
    /// wires no cell keeps the selection inside the composer, which shows the
    /// row the pointer is on but cannot hold the keyboard's own move.
    pub fn with_model_menu_index(mut self, index: Rc<RefCell<usize>>) -> Self {
        self.model_index = index;
        self
    }

    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn attachments(&self) -> &[String] {
        &self.attachments
    }

    /// The context controls, left to right: what the draft will run on, as one
    /// wrapping row. Drawn above the editor when the host asked for it (a
    /// terminal pane keeps the shell's own context over the bar), below it
    /// otherwise.
    fn context_row(&self, app: &AppContext, sm: f32) -> Option<Box<dyn Element>> {
        let children = self.context_children(app);
        if children.is_empty() {
            return None;
        }
        Some(
            Wrap::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_run_spacing(sm)
                .with_children(children)
                .finish(),
        )
    }

    /// The action controls, left to right: the model the draft runs on, stop
    /// while a turn streams, and the modal-editing badge. They stay under the
    /// editor: they describe the turn, not the text being typed.
    fn action_row(&self, app: &AppContext, sm: f32) -> Option<Box<dyn Element>> {
        let children = self.action_children(app);
        if children.is_empty() {
            return None;
        }
        Some(
            Wrap::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_run_spacing(sm)
                .with_children(children)
                .finish(),
        )
    }

    /// The footer under the editor with everything on it: the context controls
    /// and the actions. They share the row (and wrap together) unless the host
    /// keeps the context above the editor, in which case the two are drawn apart
    /// — see `rebuild`.
    fn footer_row(&self, app: &AppContext, sm: f32) -> Option<Box<dyn Element>> {
        let mut children = self.context_children(app);
        children.extend(self.action_children(app));
        if children.is_empty() {
            return None;
        }
        Some(
            Wrap::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_run_spacing(sm)
                .with_children(children)
                .finish(),
        )
    }

    /// The context controls, left to right: what the draft will run on. Each is
    /// drawn only when the surface set it, so a terminal composer that sets
    /// only the directory and the branch shows exactly those two, and a
    /// composer with no context at all contributes nothing.
    ///
    /// The environment (medium) pill leads the row and is drawn the same way:
    /// only a host that sets its label gets it, which is the conversation whose
    /// turn runs somewhere the user has to pick — a conversation on a worker. A
    /// local conversation is never given one, so it draws no environment
    /// control at all.
    fn context_children(&self, app: &AppContext) -> Vec<Box<dyn Element>> {
        let mut children: Vec<Box<dyn Element>> = Vec::new();
        // The session chip leads the row: where the shell *is* frames the
        // directory and the branch that follow it, which describe what is
        // running there. A local session contributes nothing.
        if let Some(session) = self.ssh_session.as_ref() {
            children.push(self.session_chip(app, &session.chip_label()));
        }
        // What the draft runs on, when its host makes that a choice: the
        // environment the turn is submitted on. It frames the directory and the
        // branch that follow it, which describe the work inside it. A host that
        // names no environment draws no control.
        if let Some(label) = self
            .harness_label
            .clone()
            .filter(|label| !label.trim().is_empty())
        {
            children.push(self.context_pill(
                app,
                "computer",
                &label,
                &self.harness_menu_items,
                self.harness_menu_open.clone(),
                &self.on_select_harness_item,
                "Select environment",
            ));
        }
        if let Some(path) = self.path_label.clone() {
            // The directory pill is capped rather than flex-grown: a long path
            // ellipsizes inside the pill and the row's pills stay left-aligned
            // instead of being pushed apart by it.
            let dir_child = || {
                Flex::row()
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("folder")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        ConstrainedBox::new(
                            Clipped::new(
                                Text::new(path.clone())
                                    .with_theme_color(ColorToken::Muted, app)
                                    .with_font_size(11.0)
                                    .with_max_lines(1)
                                    .finish(),
                            )
                            .finish(),
                        )
                        .with_max_width(DIR_PILL_MAX_WIDTH)
                        .finish(),
                    )
                    .finish()
            };
            let trigger = Tooltip::new(
                ComposerButton::new(dir_child()).with_height(28.0).finish(),
                "Select working directory",
            )
            .with_position(TooltipPosition::Above)
            .finish();
            // The menu is the host's when it wired a select callback, rows or
            // not: the directory list is read when the menu opens, so it is
            // empty while closed and gating on the rows would leave the pill
            // with no way to open it.
            let dir = if let Some(cb) = self.on_select_dir_item.clone() {
                PopupMenu::new(trigger, self.dir_menu_items.clone())
                    .with_open(self.dir_menu_open.clone())
                    .with_position(PopupMenuPosition::Above)
                    .with_max_visible_rows(MENU_MAX_VISIBLE_ROWS)
                    .with_panel_scroll(self.dir_menu_scroll.clone())
                    .with_on_select(move |idx| (cb.borrow_mut())(idx))
                    .finish()
            } else {
                trigger
            };
            children.push(dir);
        }
        if let Some(label) = self.branch_label.clone() {
            children.push(self.context_pill(
                app,
                "git-branch",
                &label,
                &self.branch_menu_items,
                self.branch_menu_open.clone(),
                &self.on_select_branch_item,
                "Select branch",
            ));
        }
        if let Some(cb) = self.on_attach.clone() {
            let attach = ComposerButton::new(
                Icon::new("plus")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_height(28.0)
            .with_on_click(move || (cb.borrow_mut())())
            .finish();
            children.push(
                Tooltip::new(attach, "Attach")
                    .with_position(TooltipPosition::Above)
                    .finish(),
            );
        }
        children
    }

    /// The model control: the model the draft runs on, which opens the list of
    /// the models this pane can run.
    ///
    /// The control is the label alone. It named the model in words, and the
    /// icon it carried beside them read as decoration on a value: what the
    /// control does is already said by the list it opens. A pane with no model
    /// configured reads [`MODEL_NOT_CONFIGURED`] rather than a model name, so
    /// the control never claims a turn would run on something nobody chose.
    fn model_control(&self, app: &AppContext, label: String) -> Box<dyn Element> {
        let text = if label.trim().is_empty() {
            MODEL_NOT_CONFIGURED.to_string()
        } else {
            label
        };
        let button = ComposerButton::new(
            Text::new(text)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_height(28.0);
        let button = match self.on_select_model_item.clone() {
            // The list is the host's, drawn over the whole input block; the
            // control opens and closes it, so a second press on the control
            // puts it away the way Escape does.
            Some(_) => {
                let open = self.model_menu_open.clone();
                let index = self.model_index.clone();
                let dismissed = self.slash_dismissed.clone();
                let selected = self
                    .model_menu_items
                    .iter()
                    .position(|item| item.selected)
                    .unwrap_or(0);
                button
                    .with_on_click(move || {
                        let mut open = open.borrow_mut();
                        if !*open {
                            // The band opens on the model in force, so Enter
                            // takes the model the pane already runs.
                            *index.borrow_mut() = selected;
                            // One band at a time: opening the models puts the
                            // draft's command list away the way Escape does, so
                            // the keys that follow belong to this band alone.
                            *dismissed.borrow_mut() = true;
                        }
                        *open = !*open;
                    })
                    .finish()
            }
            // A host that wired only the plain callback keeps a control that
            // hands the press on, with no list to draw.
            None => match self.on_select_model.clone() {
                Some(cb) => button.with_on_click(move || (cb.borrow_mut())()).finish(),
                None => button.finish(),
            },
        };
        Tooltip::new(button, "Select model")
            .with_position(TooltipPosition::Above)
            .finish()
    }

    /// The action controls, left to right: the model the draft runs on, stop
    /// while a turn streams, and the modal-editing badge. A plain shell command
    /// runs no model, so a terminal composer contributes nothing here.
    fn action_children(&self, app: &AppContext) -> Vec<Box<dyn Element>> {
        let mut children: Vec<Box<dyn Element>> = Vec::new();
        let has_stop = self.stop_visible && self.on_stop.is_some();
        let badge = self.vim.as_ref().map(|vim| self.mode_badge(app, vim));
        if let Some(label) = self.model_label.clone() {
            children.push(self.model_control(app, label));
        }
        if has_stop {
            if let Some(cb) = self.on_stop.clone() {
                let stop = ComposerButton::new(
                    Icon::new("stop")
                        .with_size(16.0)
                        .with_theme_color(ColorToken::Error, app)
                        .finish(),
                )
                .with_height(28.0)
                .with_on_click(move || (cb.borrow_mut())())
                .finish();
                children.push(
                    Tooltip::new(stop, "Stop")
                        .with_position(TooltipPosition::Above)
                        .finish(),
                );
            }
        }
        if let Some(badge) = badge {
            children.push(badge);
        }
        children
    }

    /// The modal-editing badge, the last control of the footer: the half-typed
    /// command while one is pending, otherwise the mode the next key belongs
    /// to.
    fn mode_badge(&self, app: &AppContext, vim: &Rc<RefCell<VimState>>) -> Box<dyn Element> {
        let vim = vim.borrow();
        let showcmd = vim.showcmd();
        let mode = vim.mode();
        let text = if showcmd.is_empty() {
            mode.label().to_string()
        } else {
            showcmd
        };
        // Insert mode is the ordinary state, so it reads as a quiet hint; the
        // command modes carry the accent.
        let color = if mode == VimMode::Insert {
            ColorToken::Muted
        } else {
            ColorToken::Accent
        };
        let tooltip = if mode == VimMode::Insert {
            "Vim: insert mode (Esc for normal mode)"
        } else {
            "Vim mode"
        };
        Tooltip::new(
            Text::new(text)
                .with_theme_color(color, app)
                .with_font_size(11.0)
                .finish(),
            tooltip,
        )
        .with_position(TooltipPosition::Above)
        .finish()
    }

    /// One context pill: an icon and a label, content-sized. When `items` is
    /// non-empty it is wrapped in the shared [`PopupMenu`] so the app-owned
    /// `open` flag survives the per-frame rebuild, otherwise it renders as a
    /// plain pill. The agent's pills are the shared [`ContextPill`] (a terminal
    /// pane shows the same pill in its topbar), so the two surfaces stay
    /// identical.
    fn context_pill(
        &self,
        app: &AppContext,
        icon: &'static str,
        label: &str,
        items: &[PopupMenuItem],
        open: Rc<RefCell<bool>>,
        on_select: &Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
        tooltip: &str,
    ) -> Box<dyn Element> {
        let mut pill = ContextPill::new(icon, label.to_string())
            .with_tooltip(tooltip)
            .with_tray_side(PillTraySide::Above);
        // The callback, not the rows, is what says the host wired a menu: a
        // pill whose rows are read when it opens has none while it is closed.
        if let Some(cb) = on_select.clone() {
            pill = pill.with_menu(items.to_vec(), open, move |idx| (cb.borrow_mut())(idx));
        }
        pill.finish(app)
    }

    /// The session chip: the host a bound SSH session put this pane's shell on,
    /// as a flat row of an icon and the session's own words — no card, no
    /// outline, no tray. It is chrome beside the input rather than a control,
    /// so it draws the same muted 11 px text the working-directory pill's label
    /// uses, capped the same way.
    fn session_chip(&self, app: &AppContext, label: &str) -> Box<dyn Element> {
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(
                Icon::new("conversation-remote")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                ConstrainedBox::new(
                    Clipped::new(
                        Text::new(label.to_string())
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(11.0)
                            .with_max_lines(1)
                            .finish(),
                    )
                    .finish(),
                )
                .with_max_width(SESSION_CHIP_MAX_WIDTH)
                .finish(),
            )
            .finish()
    }

    /// Rebuild the rich-input tree. The composer hugs its content: the editor
    /// leads it, the context pills sit above the editor when the host asked for
    /// them there (a shell bar keeps its directory and branch over the draft)
    /// and under it otherwise, and the action row (model, stop) below it, so the
    /// rich input never fills the whole pane. This keeps the message transcript
    /// visible and lets the user see the terminal/agent surface instead of a
    /// tall input box. The instructions the host keeps inside the input close
    /// the block, under the action row.
    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let xs = app.theme.spacing_px(SpacingToken::Xs);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(sm);

        if !self.attachments.is_empty() {
            let mut attachment_row = Flex::row().with_spacing(sm);
            for attachment in &self.attachments {
                let chip = Chip::new(
                    Text::new(attachment.clone())
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .finish();
                attachment_row = attachment_row.with_child(chip);
            }
            column = column.with_child(attachment_row.finish());
        }

        // Command-proposal mode (A6). The editor below the card is the in-place
        // edit surface, so an empty draft is seeded with the selected candidate;
        // a draft the user typed or cycled to is left alone.
        if let Some(handles) = self.proposal_handles() {
            if self.value.borrow().trim().is_empty() {
                if let Some(seed) = handles.selected_candidate() {
                    // The beam goes after the seeded candidate: an edit to it is
                    // an edit to the end of the command, not an insertion before
                    // its first word.
                    *self.caret.borrow_mut() = seed.chars().count();
                    *self.value.borrow_mut() = seed;
                }
            }
        }
        if let Some(card) = self.proposal_card(app) {
            column = column.with_child(card);
        }

        // Send closure shared between Enter-to-submit and the (removed) send
        // button path; keeps Enter-to-send working. The submit carries the
        // Enter key's modifiers so the host can route a plain Enter (terminal
        // command) vs Cmd/Ctrl+Enter (a NEW agent conversation, warp-new).
        let value_for_send = self.value.clone();
        let on_send = self.on_send.clone();
        let on_cmd_enter = self.on_cmd_enter.clone();
        let send = Rc::new(RefCell::new(move |modifiers: ModifiersState| {
            let text = value_for_send.borrow().clone();
            let agent_submit = (modifiers.command || modifiers.ctrl) && !modifiers.shift;
            if agent_submit {
                // Cmd/Ctrl+Enter is the way into the agent, not only a submit:
                // the host opens the pane's agent view even with an empty draft
                // (warp-new's gesture), so the callback fires either way. An
                // empty draft is the host's own no-op to define.
                if let Some(cb) = on_cmd_enter.as_ref() {
                    (cb.borrow_mut())(text.clone());
                }
                *value_for_send.borrow_mut() = String::new();
            } else if !text.is_empty() {
                if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text.clone());
                }
                *value_for_send.borrow_mut() = String::new();
            }
        }));

        if self.context_above_editor {
            if let Some(row) = self.context_row(app, sm) {
                column = column.with_child(row);
            }
        }

        // Neither band is drawn here: the host puts them over the whole input
        // block (its own width), above the instruction strip. What the composer
        // keeps of them is the key routing — see `handle_slash_key` and
        // `handle_model_key`.

        // The textarea fills the whole composer width and is visually part of
        // the rich-input bar (no separate box).
        let value = self.value.clone();
        let on_change = self.on_change.clone();
        let on_focus_change = self.on_focus_change.clone();
        let send_for_submit = send.clone();
        let textarea = TextArea::new()
            .with_value(self.value.borrow().clone())
            .with_placeholder(self.placeholder.clone())
            .with_min_height(48.0)
            .with_focused(self.focused)
            .with_blur_on_outside_click(self.blur_on_outside_click)
            // The editor owns its whole line: a press to the right of the text
            // is a press in the editor and puts the beam at the end.
            .with_full_width(true)
            .with_caret(self.caret.clone())
            .with_vim_opt(self.vim.clone())
            .with_clipboard_opt(self.clipboard.clone())
            .with_on_change(move |text| {
                *value.borrow_mut() = text.clone();
                // The draft is the menu's query: the host re-filters the
                // commands from it, and the menu opens and closes with the
                // leading `/` itself (see `slash_menu_open`).
                if let Some(cb) = on_change.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_focus_change(move |focused| {
                if let Some(cb) = on_focus_change.as_ref() {
                    (cb.borrow_mut())(focused);
                }
            })
            .with_on_submit(move |mods| (send_for_submit.borrow_mut())(mods))
            .finish();
        // Cap the textarea so the rich input hugs its content (a few lines of
        // draft + the footer pills) instead of expanding to fill the pane.
        column = column.with_child(
            ConstrainedBox::new(textarea)
                .with_min_height(48.0)
                .with_max_height(160.0)
                .finish(),
        );

        // The rest of the input's controls sit under the editor: the context
        // pills (when the host keeps them here) and the actions the draft runs
        // with.
        if self.context_above_editor {
            // The context is over the editor, so what is left under it is the
            // actions.
            if let Some(row) = self.action_row(app, sm) {
                column = column.with_child(row);
            }
        } else if let Some(row) = self.footer_row(app, sm) {
            column = column.with_child(row);
        }

        // The instructions the host keeps inside the input close it, under the
        // action row: the shell bar's `⌘↵ new conversation` is the entry the
        // pane shows here rather than over the separator, where the rest of a
        // pane's strip goes.
        if !self.hints.is_empty() {
            column = column.with_child(ShortcutHints::new(self.hints.clone()).finish(app));
        }

        // The composer card keeps only its gutters/padding; the raised
        // background and 1px border are dropped so the rich input has no gray
        // inset box behind the textarea and pills. Its horizontal padding is
        // `md` — the same inset a message bubble gives its text — so the
        // editor's text lines up with the conversation's text column, while the
        // vertical is `xs`: the rows hug the card instead of floating in a
        // gutter of their own.
        let card = Container::new(column.finish())
            .with_padding(EdgeInsets::new(md, xs, md, xs))
            .with_corner_radius(COMPOSER_CONTROL_RADIUS)
            .finish();
        // The input's own horizontal margin is the transcript rows' margin
        // (`xs`), so the block sits as close to the pane's side edges as the
        // conversation does; it carries no vertical margin of its own, so the
        // rows are only `xs` from the top and bottom of the block.
        let card = Padding::new(card, EdgeInsets::new(xs, 0.0, xs, 0.0)).finish();
        // The two bands share one slot over the input, and both are the host's
        // to draw (the command list and the model list, in that order): while
        // the draft is a command that list holds the slot, so the model band is
        // put away rather than drawn beside it — and does not come back when the
        // draft stops being one, because the user never opened it again.
        if self.slash_band_open() {
            *self.model_menu_open.borrow_mut() = false;
        }
        self.root = Some(card);
    }
}

impl Default for ChatComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for ChatComposer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        if self.handle_send_to_cloud_key(event) {
            return true;
        }
        if self.handle_proposal_key(event) {
            return true;
        }
        // The model band is over the input and holds the keys that take a row
        // from it while it is up.
        if self.handle_model_key(event) {
            return true;
        }
        // While the menu is up it owns the keys that pick from it: the editor
        // keeps every other key, because what it types is the menu's query.
        if self.handle_slash_key(event) {
            return true;
        }
        let handled = self
            .root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false);
        if handled {
            return true;
        }
        // A press anywhere in the rich input that no child consumed (the card's
        // gutters, the pill rows, the empty tail of the column) is still a press
        // on the input: focus the editor so the surface becomes writable and the
        // beam shows. Children that do consume the press — the pills, the menus,
        // the editor itself — keep their own behaviour.
        if let DispatchedEvent::MouseDown { position, .. } = event {
            if !self.focused {
                if let Some(bounds) = self.bounds() {
                    if bounds.contains(PointF::new(position.x, position.y)) {
                        if let Some(cb) = self.on_focus_change.as_ref() {
                            (cb.borrow_mut())(true);
                        }
                        return true;
                    }
                }
            }
        }
        false
    }
}
