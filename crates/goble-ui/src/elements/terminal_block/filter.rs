use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Expanded, Fill, Flex, Icon,
    MainAxisSize, PopupMenu, PopupMenuItem, PopupMenuPosition, SearchInput, Text, TopbarButton,
};
use crate::theme::{ColorToken, SpacingToken};

use super::line::TerminalLineKind;

/// The copy handler a terminal block's header button fires with the block's
/// full display text. App-owned, so it is shared like the block's filter state.
pub type TerminalCopyHandler = Rc<RefCell<dyn FnMut(String) + 'static>>;

/// App-owned filter state for a terminal block, or for a whole terminal surface.
///
/// Every cell lives in app state (`UiState.terminal_filters` per block,
/// `UiState.terminal_global_filters` per pane) so what the user typed, which
/// kinds they kept and whether the bar is up survive the per-frame element
/// rebuild. A block resolves its own entry by content key.
#[derive(Clone, Debug)]
pub struct TerminalFilter {
    /// Whether the line-kind tray is open.
    pub open: Rc<RefCell<bool>>,
    pub selected: Rc<RefCell<usize>>,
    /// Whether the filter bar itself is showing (the surface the filter button
    /// and the filter chords open).
    pub bar_open: Rc<RefCell<bool>>,
    /// The query typed into the bar's field. Empty filters nothing.
    pub query: Rc<RefCell<String>>,
    /// Whether the bar's field holds the caret, so its beam survives the rebuild.
    pub focused: Rc<RefCell<bool>>,
    /// Whether the pointer is over the block this state belongs to. Written by
    /// the block on a pointer move and read by the next frame's build, which is
    /// what shows the block's controls. A whole-surface filter never sets it.
    pub hover: Rc<RefCell<bool>>,
}

impl Default for TerminalFilter {
    fn default() -> Self {
        Self {
            open: Rc::new(RefCell::new(false)),
            selected: Rc::new(RefCell::new(0)),
            bar_open: Rc::new(RefCell::new(false)),
            query: Rc::new(RefCell::new(String::new())),
            focused: Rc::new(RefCell::new(false)),
            hover: Rc::new(RefCell::new(false)),
        }
    }
}

impl TerminalFilter {
    /// The query as the filter compares it: surrounding whitespace dropped and
    /// both sides folded to lower case, so a query matches its text whatever
    /// case either was typed in.
    fn needle(&self) -> String {
        self.query.borrow().trim().to_lowercase()
    }

    /// Whether a line passes the filter: its kind must be one the tray keeps,
    /// and its text must contain the query.
    pub fn matches(&self, kind: TerminalLineKind, text: &str) -> bool {
        let kind_ok = FILTERS
            .get(*self.selected.borrow())
            .map(|(_, pred)| pred(kind))
            .unwrap_or(true);
        if !kind_ok {
            return false;
        }
        let needle = self.needle();
        needle.is_empty() || text.to_lowercase().contains(&needle)
    }

    /// Whether the filter bar is showing.
    pub fn is_open(&self) -> bool {
        *self.bar_open.borrow()
    }

    /// Show the bar with its field ready for typing.
    pub fn open_bar(&self) {
        *self.bar_open.borrow_mut() = true;
        *self.focused.borrow_mut() = true;
    }

    /// Hide the bar and drop its query, so the surface draws every line again:
    /// a filter with no visible bar would hide output the user cannot see the
    /// reason for.
    pub fn close_bar(&self) {
        *self.bar_open.borrow_mut() = false;
        *self.focused.borrow_mut() = false;
        self.query.borrow_mut().clear();
    }

    pub fn toggle_bar(&self) {
        if self.is_open() {
            self.close_bar();
        } else {
            self.open_bar();
        }
    }

    pub fn set_query(&self, query: impl Into<String>) {
        *self.query.borrow_mut() = query.into();
    }

    /// Whether the pointer is over the block this state belongs to.
    pub fn is_hovered(&self) -> bool {
        *self.hover.borrow()
    }

    pub fn set_hovered(&self, hovered: bool) {
        *self.hover.borrow_mut() = hovered;
    }
}

const FILTER_LABELS: &[&str] = &["All", "Commands", "Output", "Notes", "Success", "Errors"];

pub(super) const FILTERS: &[(&str, fn(TerminalLineKind) -> bool)] = &[
    (FILTER_LABELS[0], |_| true),
    (FILTER_LABELS[1], |k| k == TerminalLineKind::Command),
    (FILTER_LABELS[2], |k| k == TerminalLineKind::Output),
    (FILTER_LABELS[3], |k| k == TerminalLineKind::Info),
    (FILTER_LABELS[4], |k| k == TerminalLineKind::Success),
    (FILTER_LABELS[5], |k| k == TerminalLineKind::Error),
];

/// The filter option labels, in order, shared by a block's bar and the
/// whole-transcript filter bar.
pub fn filter_option_labels() -> &'static [&'static str] {
    FILTER_LABELS
}

/// Show or hide `filter`'s bar and focus its field. The one operation the
/// filter chords perform, on a block's filter or on a whole surface's.
pub fn toggle_terminal_filter(filter: &TerminalFilter) {
    filter.toggle_bar();
}

/// Whether `filter`'s bar is showing.
pub fn terminal_filter_open(filter: &TerminalFilter) -> bool {
    filter.is_open()
}

/// The filter bar of a terminal surface: the kind tray, the query field and how
/// many lines the query leaves.
///
/// The field is a real text input — it takes the keyboard, edits in place and
/// draws its own beam — and the query it edits lives in `filter`'s cells, so
/// what was typed survives the per-frame rebuild. The caller places the bar:
/// a block hangs it under its header, a whole surface spans it across the top.
pub fn terminal_filter_bar(
    filter: &TerminalFilter,
    placeholder: &str,
    matched: usize,
    total: usize,
    app: &AppContext,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let muted = ColorToken::Muted;

    // The kind tray keeps the line-kind narrowing inside the bar.
    let selected = *filter.selected.borrow();
    let items = FILTERS
        .iter()
        .enumerate()
        .map(|(i, (label, _))| {
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
    .with_size(FILTER_BUTTON_SIZE)
    .with_active(selected != 0 || *filter.open.borrow())
    .finish();
    let filter_for_select = filter.clone();
    let tray = PopupMenu::new(trigger, items)
        .with_open(filter.open.clone())
        .with_position(PopupMenuPosition::Below)
        .with_on_select(move |idx| *filter_for_select.selected.borrow_mut() = idx)
        .finish();

    // The query field: a text input the app rebuilds from the filter's own
    // query, so the caret, the value and the drawn lines all agree.
    let filter_for_change = filter.clone();
    let filter_for_focus = filter.clone();
    let field = SearchInput::new()
        .with_value(filter.query.borrow().clone())
        .with_placeholder(placeholder)
        .with_focused(*filter.focused.borrow())
        .with_compact(true)
        .with_icon(false)
        .with_on_change(move |value| filter_for_change.set_query(value))
        .with_on_focus_change(move |focused| *filter_for_focus.focused.borrow_mut() = focused)
        .finish();

    let count = Text::new(format!("{matched} of {total}"))
        .with_theme_color(muted, app)
        .with_font_size(FILTER_COUNT_FONT_SIZE)
        .with_max_lines(1)
        .finish();

    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(tray)
        .with_child(Expanded::new(field).finish())
        .with_child(count)
        .finish();
    Container::new(row)
        .with_padding(EdgeInsets::uniform(sm))
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(FILTER_BAR_CORNER_RADIUS)
        .finish()
}

/// The filter button's target size, matching a block header's own controls.
pub(super) const FILTER_BUTTON_SIZE: f32 = 24.0;

const FILTER_COUNT_FONT_SIZE: f32 = 11.0;

const FILTER_BAR_CORNER_RADIUS: f32 = 6.0;
