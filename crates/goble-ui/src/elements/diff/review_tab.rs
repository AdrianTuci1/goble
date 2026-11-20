//! The review tab's header: the file selector, the selected file's counts and
//! the controls that choose how its comparison reads.
//!
//! The element is only the header DSH draws above a comparison. A host owns the
//! parsed hunks and the body; this element owns the header's one row — the
//! 28 pt file-selector trigger, the `+A -D` counts, the split/unified and wrap
//! toggles, and the 28 × 28 open/other tools. Every control reports through a
//! callback, and an optional shared [`ReviewTabState`] lets a host rebuild the
//! element each frame without losing the selection or the two view choices.
//!
//! The measurements come from [`DiffSpec`], the same sheet the changed-files
//! card and [`FileDiffView`](super::FileDiffView) use.

use std::cell::RefCell;
use std::rc::Rc;

use super::spec::DiffSpec;
use crate::elements::chat_content::ChangedFile;
use crate::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext,
    Expanded, Fill, Flex, HoverRow, Icon, LayoutContext, MainAxisAlignment, MainAxisSize,
    PaintContext, Point, PopupMenu, PopupMenuItem, PopupMenuPosition, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily};

/// One changed file in the review tab. The card and the selector share DSH's
/// changed-file record, so a host does not convert between two shapes.
pub type ReviewTabFile = ChangedFile;

/// The header's owned choices. A host keeps one of these in an
/// `Rc<RefCell<_>>` and hands it to [`ReviewTab::with_state`] so the selection
/// and the split/wrap choices survive the per-frame rebuild.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewTabState {
    /// Index into the file list.
    pub selected: usize,
    /// Side-by-side when true, unified when false.
    pub split: bool,
    /// Wrap long lines inside their column.
    pub wrap: bool,
    /// Whether the file selector's menu is open.
    pub selector_open: bool,
    /// The row the selector's keyboard highlight is on.
    pub selector_highlight: Option<usize>,
}

impl Default for ReviewTabState {
    fn default() -> Self {
        Self {
            selected: 0,
            split: true,
            wrap: false,
            selector_open: false,
            selector_highlight: None,
        }
    }
}

/// The header under its descriptive name: it is the ReviewTab surface's header,
/// not the whole tab (the host owns the comparison body).
pub type ReviewTabHeader = ReviewTab;
/// A short alias for [`ReviewTab`].
pub type ReviewHeader = ReviewTab;

type SelectCallback = Rc<RefCell<dyn FnMut(usize) + 'static>>;
type ToggleCallback = Rc<RefCell<dyn FnMut() + 'static>>;
type ChangeCallback = Rc<RefCell<dyn FnMut(bool) + 'static>>;
type SimpleCallback = Rc<RefCell<dyn FnMut() + 'static>>;

/// The DSH review-tab header.
pub struct ReviewTab {
    files: Vec<ReviewTabFile>,
    selected: Rc<RefCell<usize>>,
    split: Rc<RefCell<bool>>,
    wrap: Rc<RefCell<bool>>,
    selector_open: Rc<RefCell<bool>>,
    selector_highlight: Rc<RefCell<Option<usize>>>,
    state: Option<Rc<RefCell<ReviewTabState>>>,
    on_select: Option<SelectCallback>,
    on_toggle_split: Option<ToggleCallback>,
    on_split_change: Option<ChangeCallback>,
    on_toggle_wrap: Option<ToggleCallback>,
    on_wrap_change: Option<ChangeCallback>,
    on_open_file: Option<SimpleCallback>,
    on_other: Option<SimpleCallback>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Default for ReviewTab {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl ReviewTab {
    /// A header over `files`, opening on the first one with the split layout
    /// selected, the way DSH seeds a review tab.
    pub fn new<F: IntoIterator<Item = ReviewTabFile>>(files: F) -> Self {
        let files: Vec<_> = files.into_iter().collect();
        Self {
            files,
            selected: Rc::new(RefCell::new(0)),
            split: Rc::new(RefCell::new(true)),
            wrap: Rc::new(RefCell::new(false)),
            selector_open: Rc::new(RefCell::new(false)),
            selector_highlight: Rc::new(RefCell::new(None)),
            state: None,
            on_select: None,
            on_toggle_split: None,
            on_split_change: None,
            on_toggle_wrap: None,
            on_wrap_change: None,
            on_open_file: None,
            on_other: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    /// An empty header: the title form DSH draws while a review has no file.
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    pub fn from_files<F: IntoIterator<Item = ReviewTabFile>>(files: F) -> Self {
        Self::new(files)
    }

    /// Replace the files. The selected index is clamped to the new list.
    pub fn with_files<F: IntoIterator<Item = ReviewTabFile>>(mut self, files: F) -> Self {
        self.files = files.into_iter().collect();
        let last = self.files.len().saturating_sub(1);
        let selected = (*self.selected.borrow()).min(last);
        *self.selected.borrow_mut() = selected;
        self
    }

    /// Open on the file at `index`, clamped to the list.
    pub fn with_selected(self, index: usize) -> Self {
        *self.selected.borrow_mut() = index.min(self.files.len().saturating_sub(1));
        self
    }

    /// Open on the file whose path is `path`, when it is in the list.
    pub fn with_selected_path(self, path: impl AsRef<str>) -> Self {
        if let Some(index) = self
            .files
            .iter()
            .position(|file| file.path == path.as_ref())
        {
            *self.selected.borrow_mut() = index;
        }
        self
    }

    /// Draw the side-by-side layout (`true`) or the unified one (`false`).
    pub fn with_split(self, split: bool) -> Self {
        *self.split.borrow_mut() = split;
        self
    }

    /// Wrap long comparison lines.
    pub fn with_wrap(self, wrap: bool) -> Self {
        *self.wrap.borrow_mut() = wrap;
        self
    }

    /// Share the selector's open flag with the host. The menu toggles it, so a
    /// per-frame rebuild reopens the same state.
    pub fn with_selector_open(mut self, open: Rc<RefCell<bool>>) -> Self {
        self.selector_open = open;
        self
    }

    /// Share the selector's keyboard highlight with the host.
    pub fn with_selector_highlight(mut self, highlight: Rc<RefCell<Option<usize>>>) -> Self {
        self.selector_highlight = highlight;
        self
    }

    /// Give the header a shared state cell. Its selection, split/wrap choices
    /// and selector state are read when the header lays out, and written back
    /// when the header changes them.
    pub fn with_state(mut self, state: Rc<RefCell<ReviewTabState>>) -> Self {
        {
            let state = state.borrow();
            *self.selected.borrow_mut() = state.selected;
            *self.split.borrow_mut() = state.split;
            *self.wrap.borrow_mut() = state.wrap;
            *self.selector_open.borrow_mut() = state.selector_open;
            *self.selector_highlight.borrow_mut() = state.selector_highlight;
        }
        self.state = Some(state);
        self
    }

    /// Report the selected file's index. The menu closes after a selection.
    pub fn with_on_select<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Report the selected file itself rather than its index.
    pub fn with_on_file_select<F: FnMut(&ChangedFile) + 'static>(
        mut self,
        mut callback: F,
    ) -> Self {
        let files = self.files.clone();
        self.on_select = Some(Rc::new(RefCell::new(move |index: usize| {
            if let Some(file) = files.get(index) {
                callback(file);
            }
        })));
        self
    }

    /// Report a split/unified toggle, after the header has flipped its own
    /// choice. This is the no-argument form a host uses when its own action
    /// owns the state; [`Self::with_on_split_change`] carries the new value.
    pub fn with_on_toggle_split<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_split = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The same as [`Self::with_on_toggle_split`], under the shorter name.
    pub fn with_on_split<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_split = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Report a split/unified toggle with the new value.
    pub fn with_on_split_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_split_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Report a wrap toggle, after the header has flipped its own choice.
    pub fn with_on_toggle_wrap<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_wrap = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The same as [`Self::with_on_toggle_wrap`], under the shorter name.
    pub fn with_on_wrap<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_wrap = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Report a wrap toggle with the new value.
    pub fn with_on_wrap_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_wrap_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Report the open-file tool.
    pub fn with_on_open_file<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_open_file = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The same open-file callback under the name a compact caller uses.
    pub fn with_on_open<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_open_file = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Report the trailing "other" tool (DSH's file-actions slot).
    pub fn with_on_other<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_other = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The same trailing tool under the menu-ish name a host may use.
    pub fn with_on_more<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_other = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn files(&self) -> &[ReviewTabFile] {
        &self.files
    }

    pub fn selected(&self) -> usize {
        *self.selected.borrow()
    }

    pub fn selected_file(&self) -> Option<&ReviewTabFile> {
        self.files.get(self.selected())
    }

    pub fn split(&self) -> bool {
        *self.split.borrow()
    }

    pub fn wrap(&self) -> bool {
        *self.wrap.borrow()
    }

    fn sync_from_state(&mut self) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let state = state.borrow();
        let last = self.files.len().saturating_sub(1);
        *self.selected.borrow_mut() = state.selected.min(last);
        *self.split.borrow_mut() = state.split;
        *self.wrap.borrow_mut() = state.wrap;
        *self.selector_highlight.borrow_mut() = state.selector_highlight;
        *self.selector_open.borrow_mut() = state.selector_open;
    }

    fn write_state(&self) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let mut state = state.borrow_mut();
        state.selected = self.selected();
        state.split = self.split();
        state.wrap = self.wrap();
        state.selector_open = *self.selector_open.borrow();
        state.selector_highlight = *self.selector_highlight.borrow();
    }

    fn build(&mut self, app: &AppContext) {
        let file = self.files.get(self.selected()).cloned();
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(DiffSpec::REVIEW_HEADER_GAP);

        if let Some(file) = file.as_ref() {
            let items: Vec<PopupMenuItem> = self
                .files
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let item = PopupMenuItem::new(entry.path.clone())
                        .with_icon(crate::elements::file_icon_name(&entry.path))
                        .with_diff(entry.added as u32, entry.removed as u32);
                    if index == self.selected() {
                        item.selected()
                    } else {
                        item
                    }
                })
                .collect();
            let trigger = self.selector_trigger(app, file);
            let open = Rc::clone(&self.selector_open);
            let highlight = Rc::clone(&self.selector_highlight);
            let selected = Rc::clone(&self.selected);
            let state = self.state.clone();
            let callback = self.on_select.clone();
            let menu = PopupMenu::new(trigger, items)
                .with_open(open)
                .with_highlight_index(highlight)
                .with_selected_color(ColorToken::Text)
                .with_max_width(360.0)
                .with_position(PopupMenuPosition::Below)
                .with_panel_offset(vec2f(0.0, 4.0))
                .with_on_select(move |index| {
                    *selected.borrow_mut() = index;
                    if let Some(state) = state.as_ref() {
                        state.borrow_mut().selected = index;
                    }
                    if let Some(callback) = callback.as_ref() {
                        (callback.borrow_mut())(index);
                    }
                })
                .finish();
            row = row.with_child(menu);
            row = row.with_child(counts(file.added, file.removed, app));
        } else {
            row = row.with_child(
                Container::new(
                    Text::new("Review")
                        .with_font_family(FontFamily::System)
                        .with_font_size(13.0)
                        .with_line_height(20.0 / 13.0)
                        .with_weight(FontWeight::Medium)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_padding_left(DiffSpec::REVIEW_HEADER_PAD_LEFT)
                .finish(),
            );
        }

        row = row
            .with_child(Expanded::new(crate::elements::Empty::new().finish()).finish())
            .with_child(self.split_tool(app))
            .with_child(self.wrap_tool(app));

        if file.is_some() {
            row = row.with_child(self.icon_tool(app, "maximize-01", self.on_open_file.clone()));
            row = row.with_child(self.icon_tool(app, "dots-horizontal", self.on_other.clone()));
        }

        let content = Container::new(row.finish())
            .with_padding(EdgeInsets::new(
                DiffSpec::REVIEW_HEADER_PAD_LEFT,
                5.0,
                DiffSpec::REVIEW_HEADER_PAD_RIGHT,
                5.0,
            ))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .finish();
        self.root = Some(
            ConstrainedBox::new(content)
                .with_height(DiffSpec::REVIEW_HEADER_HEIGHT)
                .finish(),
        );
    }

    fn selector_trigger(&self, app: &AppContext, file: &ChangedFile) -> Box<dyn Element> {
        let label = Text::new(file.path.clone())
            .with_font_family(FontFamily::System)
            .with_font_size(DiffSpec::REVIEW_SELECTOR_SIZE)
            .with_line_height(
                DiffSpec::REVIEW_SELECTOR_LINE_HEIGHT / DiffSpec::REVIEW_SELECTOR_SIZE,
            )
            .with_weight(FontWeight::Medium)
            .with_theme_color(ColorToken::Text, app)
            .with_max_lines(1)
            .finish();
        let row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0)
            .with_child(
                // The label takes what the chevron leaves; a long path is
                // clipped inside the selector rather than pushing the tools.
                ConstrainedBox::new(label).with_max_width(240.0).finish(),
            )
            .with_child(
                Icon::new("chevron-down")
                    .with_size(DiffSpec::REVIEW_SELECTOR_CHEVRON)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish();
        let open = *self.selector_open.borrow();
        let content = Container::new(row)
            .with_padding(EdgeInsets::new(6.0, 4.0, 8.0, 4.0))
            .with_corner_radius(6.0)
            .with_background(Fill::Solid(app.theme.color(if open {
                ColorToken::Hover
            } else {
                ColorToken::Bg
            })))
            .finish();
        HoverRow::new(content).with_corner_radius(6.0).finish()
    }

    /// The view toggle. It keeps both words on the control — the active one in
    /// the primary ink — so the choice is legible at a glance, and the whole
    /// word pair is one 28 pt press target.
    fn split_tool(&self, app: &AppContext) -> Box<dyn Element> {
        let active = self.split();
        let label = |text: &'static str, on: bool| -> Box<dyn Element> {
            Text::new(text)
                .with_font_family(FontFamily::System)
                .with_font_size(11.0)
                .with_line_height(16.0 / 11.0)
                .with_weight(if on {
                    FontWeight::Medium
                } else {
                    FontWeight::Regular
                })
                .with_theme_color(
                    if on {
                        ColorToken::Text
                    } else {
                        ColorToken::Muted
                    },
                    app,
                )
                .finish()
        };
        let content = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(3.0)
            .with_child(label("Split", active))
            .with_child(
                Text::new("/")
                    .with_font_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(label("Unified", !active))
            .finish();
        let split = Rc::clone(&self.split);
        let state = self.state.clone();
        let toggle = self.on_toggle_split.clone();
        let change = self.on_split_change.clone();
        self.text_tool(app, content, active, move || {
            let next = {
                let mut active = split.borrow_mut();
                *active = !*active;
                *active
            };
            if let Some(state) = state.as_ref() {
                state.borrow_mut().split = next;
            }
            if let Some(toggle) = toggle.as_ref() {
                (toggle.borrow_mut())();
            }
            if let Some(change) = change.as_ref() {
                (change.borrow_mut())(next);
            }
        })
    }

    fn wrap_tool(&self, app: &AppContext) -> Box<dyn Element> {
        let content = Text::new("Wrap")
            .with_font_family(FontFamily::System)
            .with_font_size(11.0)
            .with_line_height(16.0 / 11.0)
            .with_weight(if self.wrap() {
                FontWeight::Medium
            } else {
                FontWeight::Regular
            })
            .with_theme_color(
                if self.wrap() {
                    ColorToken::Text
                } else {
                    ColorToken::Muted
                },
                app,
            )
            .finish();
        let wrap = Rc::clone(&self.wrap);
        let state = self.state.clone();
        let toggle = self.on_toggle_wrap.clone();
        let change = self.on_wrap_change.clone();
        self.text_tool(app, content, self.wrap(), move || {
            let next = {
                let mut active = wrap.borrow_mut();
                *active = !*active;
                *active
            };
            if let Some(state) = state.as_ref() {
                state.borrow_mut().wrap = next;
            }
            if let Some(toggle) = toggle.as_ref() {
                (toggle.borrow_mut())();
            }
            if let Some(change) = change.as_ref() {
                (change.borrow_mut())(next);
            }
        })
    }

    fn text_tool(
        &self,
        app: &AppContext,
        content: Box<dyn Element>,
        active: bool,
        on_click: impl FnMut() + 'static,
    ) -> Box<dyn Element> {
        let content = Container::new(content)
            .with_padding(EdgeInsets::new(7.0, 6.0, 7.0, 6.0))
            .with_corner_radius(6.0)
            .with_background(Fill::Solid(app.theme.color(if active {
                ColorToken::Hover
            } else {
                ColorToken::Bg
            })))
            .finish();
        HoverRow::new(
            ConstrainedBox::new(content)
                .with_height(DiffSpec::REVIEW_TOOL_SIZE)
                .finish(),
        )
        .with_corner_radius(6.0)
        .with_on_click(on_click)
        .finish()
    }

    fn icon_tool(
        &self,
        app: &AppContext,
        icon: &'static str,
        on_click: Option<SimpleCallback>,
    ) -> Box<dyn Element> {
        let content = Container::new(
            Icon::new(icon)
                .with_size(DiffSpec::REVIEW_TOOL_GLYPH)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(
            (DiffSpec::REVIEW_TOOL_SIZE - DiffSpec::REVIEW_TOOL_GLYPH) * 0.5,
        ))
        .with_corner_radius(6.0)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish();
        let content = ConstrainedBox::new(content)
            .with_width(DiffSpec::REVIEW_TOOL_SIZE)
            .with_height(DiffSpec::REVIEW_TOOL_SIZE)
            .finish();
        let mut row = HoverRow::new(content).with_corner_radius(6.0);
        if let Some(callback) = on_click {
            row = row.with_on_click(move || (callback.borrow_mut())());
        }
        row.finish()
    }
}

/// The `+A -D` pair beside the selector, at DSH's 12/20 mono size.
fn counts(added: u64, removed: u64, app: &AppContext) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0);
    let push = |row: Flex, text: String, token: ColorToken| {
        row.with_child(
            Text::new(text)
                .with_font_family(FontFamily::Mono)
                .with_font_size(DiffSpec::REVIEW_SELECTOR_SIZE)
                .with_line_height(
                    DiffSpec::REVIEW_SELECTOR_LINE_HEIGHT / DiffSpec::REVIEW_SELECTOR_SIZE,
                )
                .with_theme_color(token, app)
                .finish(),
        )
    };
    if added > 0 {
        row = push(row, format!("+{added}"), ColorToken::Success);
    }
    if removed > 0 {
        row = push(row, format!("-{removed}"), ColorToken::Error);
    }
    if added == 0 && removed == 0 {
        row = push(row, "0".to_string(), ColorToken::Muted);
    }
    row.finish()
}

impl Element for ReviewTab {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.sync_from_state();
        let last = self.files.len().saturating_sub(1);
        if self.selected() > last {
            *self.selected.borrow_mut() = last;
        }
        self.write_state();
        self.build(app);
        let size = self
            .root
            .as_mut()
            .map(|root| root.layout(constraint, ctx, app))
            .unwrap_or(Vector2F::zero());
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(root) = self.root.as_mut() {
            root.paint(origin, ctx, app);
        }
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        let Some(root) = self.root.as_mut() else {
            return false;
        };
        let handled = root.dispatch_event(event, ctx, app);
        // A press outside the selector closes it without the menu reporting a
        // handled event; the state still has to hear that it closed.
        self.write_state();
        handled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{EventContext, LayoutContext, SizeConstraint};
    use crate::event::DispatchedEvent;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    fn files() -> Vec<ChangedFile> {
        vec![
            ChangedFile {
                path: "src/panes.rs".to_string(),
                added: 18,
                removed: 4,
                status: 'M',
            },
            ChangedFile {
                path: "src/legacy.rs".to_string(),
                added: 0,
                removed: 9,
                status: 'D',
            },
        ]
    }

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn text_origin(commands: &[RenderCommand], needle: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == needle => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} is drawn: {commands:?}"))
    }

    fn icon_origin(commands: &[RenderCommand], needle: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, .. } if name == needle => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} is drawn: {commands:?}"))
    }

    fn click(element: &mut Box<dyn Element>, at: Vector2F, app: &AppContext) {
        let at = at + vec2f(2.0, 2.0);
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: at,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: at,
                button: 0,
            },
        ] {
            let _ = element.dispatch_event(&event, &mut ctx, app);
        }
    }

    #[test]
    fn the_review_tab_header_is_38_points_and_names_the_file_and_tools() {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = ReviewTab::new(files()).finish();
        let commands = render_element(&mut element, vec2f(800.0, 400.0), &app);

        assert_eq!(
            element.size().expect("laid out"),
            vec2f(800.0, DiffSpec::REVIEW_HEADER_HEIGHT)
        );
        let drawn = texts(&commands);
        for needle in ["src/panes.rs", "+18", "-4", "Split", "Unified", "Wrap"] {
            assert!(
                drawn.iter().any(|text| text == needle),
                "the header draws {needle:?}: {drawn:?}"
            );
        }
        // The two icon tools are DSH's 28 × 28 controls.
        for icon in ["maximize-01", "dots-horizontal"] {
            assert!(
                commands.iter().any(|command| matches!(
                    command,
                    RenderCommand::DrawIcon { name, .. } if name == icon
                )),
                "the header draws {icon:?}"
            );
        }
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, .. }
                    if (rect.height() - DiffSpec::REVIEW_TOOL_SIZE).abs() < 0.01
            )),
            "a tool is a 28 pt control: {commands:?}"
        );
        // The selector's 28 pt trigger is the first fill under the file name.
        let name = text_origin(&commands, "src/panes.rs");
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, .. }
                    if rect.min_y() <= name.y
                        && rect.max_y() >= name.y + 12.0
                        && (rect.height() - DiffSpec::REVIEW_SELECTOR_HEIGHT).abs() < 0.01
            )),
            "the selector trigger is DSH's 28 pt box: {commands:?}"
        );
    }

    #[test]
    fn zero_counts_are_left_out_of_the_header() {
        for (added, removed, shown, hidden) in [(3u64, 0u64, "+3", "-0"), (0, 4, "-4", "+0")] {
            let app = AppContext::default();
            let mut element: Box<dyn Element> = ReviewTab::new(vec![ChangedFile {
                path: "src/a.rs".to_string(),
                added,
                removed,
                status: 'M',
            }])
            .finish();
            let commands = render_element(&mut element, vec2f(700.0, 300.0), &app);
            let drawn = texts(&commands);
            assert!(
                drawn.iter().any(|text| text == shown),
                "the header shows {shown:?}: {drawn:?}"
            );
            assert!(
                !drawn.iter().any(|text| text == hidden),
                "the header omits {hidden:?}: {drawn:?}"
            );
        }
    }

    #[test]
    fn the_selector_marks_the_current_file_and_selects_another() {
        let app = AppContext::default();
        let picked = Rc::new(RefCell::new(None::<usize>));
        let sink = Rc::clone(&picked);
        let open = Rc::new(RefCell::new(false));
        let tab = ReviewTab::new(files())
            .with_selector_open(Rc::clone(&open))
            .with_on_select(move |index| *sink.borrow_mut() = Some(index));
        let selected = Rc::clone(&tab.selected);
        let mut element: Box<dyn Element> = tab.finish();
        let commands = render_element(&mut element, vec2f(800.0, 400.0), &app);
        let selector = text_origin(&commands, "src/panes.rs");
        click(&mut element, selector, &app);
        assert!(*open.borrow(), "the selector opens on a press");

        // The open panel marks the current file with a check and omits its
        // zero side (`+0` is never drawn for the deleted file).
        let commands = render_element(&mut element, vec2f(800.0, 400.0), &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "check"
            )),
            "the selected file carries a check: {commands:?}"
        );
        let drawn = texts(&commands);
        assert!(!drawn.iter().any(|text| text == "+0"), "{drawn:?}");
        assert!(!drawn.iter().any(|text| text == "-0"), "{drawn:?}");
        assert!(drawn.iter().any(|text| text == "-9"), "{drawn:?}");

        let other = text_origin(&commands, "src/legacy.rs");
        click(&mut element, other, &app);
        assert_eq!(*picked.borrow(), Some(1), "the menu reports the file index");
        assert_eq!(*selected.borrow(), 1);
        assert!(!*open.borrow(), "choosing a file closes the menu");
    }

    #[test]
    fn split_wrap_open_and_other_tools_report_their_callbacks() {
        let app = AppContext::default();
        let split_calls = Rc::new(RefCell::new(0usize));
        let split_value = Rc::new(RefCell::new(None::<bool>));
        let wrap_calls = Rc::new(RefCell::new(0usize));
        let wrap_value = Rc::new(RefCell::new(None::<bool>));
        let open_calls = Rc::new(RefCell::new(0usize));
        let other_calls = Rc::new(RefCell::new(0usize));
        let mut element: Box<dyn Element> = {
            let split_calls = Rc::clone(&split_calls);
            let split_value = Rc::clone(&split_value);
            let wrap_calls = Rc::clone(&wrap_calls);
            let wrap_value = Rc::clone(&wrap_value);
            let open_calls = Rc::clone(&open_calls);
            let other_calls = Rc::clone(&other_calls);
            ReviewTab::new(files())
                .with_on_toggle_split(move || *split_calls.borrow_mut() += 1)
                .with_on_split_change(move |value| *split_value.borrow_mut() = Some(value))
                .with_on_toggle_wrap(move || *wrap_calls.borrow_mut() += 1)
                .with_on_wrap_change(move |value| *wrap_value.borrow_mut() = Some(value))
                .with_on_open_file(move || *open_calls.borrow_mut() += 1)
                .with_on_other(move || *other_calls.borrow_mut() += 1)
                .finish()
        };
        let commands = render_element(&mut element, vec2f(800.0, 400.0), &app);
        let split = text_origin(&commands, "Split");
        click(&mut element, split, &app);
        let wrap = text_origin(&commands, "Wrap");
        click(&mut element, wrap, &app);
        let open = icon_origin(&commands, "maximize-01");
        click(&mut element, open, &app);
        let other = icon_origin(&commands, "dots-horizontal");
        click(&mut element, other, &app);

        assert_eq!(*split_calls.borrow(), 1);
        assert_eq!(*split_value.borrow(), Some(false));
        assert_eq!(*wrap_calls.borrow(), 1);
        assert_eq!(*wrap_value.borrow(), Some(true));
        assert_eq!(*open_calls.borrow(), 1);
        assert_eq!(*other_calls.borrow(), 1);
    }

    #[test]
    fn a_shared_state_cell_keeps_the_choices_across_rebuilds() {
        let app = AppContext::default();
        let state = Rc::new(RefCell::new(ReviewTabState::default()));
        let mut element: Box<dyn Element> = ReviewTab::new(files())
            .with_state(Rc::clone(&state))
            .finish();
        let commands = render_element(&mut element, vec2f(800.0, 400.0), &app);
        let wrap = text_origin(&commands, "Wrap");
        click(&mut element, wrap, &app);
        assert!(state.borrow().wrap, "the state hears the wrap toggle");

        let commands = render_element(&mut element, vec2f(800.0, 400.0), &app);
        let split = text_origin(&commands, "Split");
        click(&mut element, split, &app);
        assert!(!state.borrow().split, "the state hears the split toggle");

        // A fresh element over the same cell opens with the saved choices.
        let rebuilt = ReviewTab::new(files()).with_state(Rc::clone(&state));
        assert!(
            rebuilt.wrap() && !rebuilt.split(),
            "the rebuilt header reads the shared state"
        );
    }
}
