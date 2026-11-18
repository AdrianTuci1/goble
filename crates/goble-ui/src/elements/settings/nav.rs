//! The settings rail: search, grouped page rows, and the keyboard's first
//! region.
//!
//! Geometry from the parity doc (§3.1): 200px wide, 8px padding, 32px rows with
//! a 10px inset and an 8px radius, a 14px label (selected 14/500), 2px between
//! rows and no dividers. The selected row is a `Selected` fill, hover a `Hover`
//! fill, and the focused row carries the 2px ring. T3 puts search at the top of
//! the rail; this element does the same and keeps the `/` cap and the clear
//! affordance.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;

use super::{
    FocusOutline, SETTINGS_FOCUS_RING_INSET, SETTINGS_NAV_GROUP_LABEL_SIZE,
    SETTINGS_NAV_LABEL_SIZE, SETTINGS_NAV_ROW_GAP, SETTINGS_NAV_ROW_HEIGHT, SETTINGS_NAV_ROW_INSET,
    SETTINGS_NAV_ROW_RADIUS, SETTINGS_RAIL_PADDING, SETTINGS_RAIL_WIDTH,
    SETTINGS_SEARCH_ROW_HEIGHT,
};

/// One page in the rail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsNavItem {
    pub label: String,
}

impl SettingsNavItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

/// A titled group of pages. A group with an empty title draws no heading (T3
/// puts the first rows under the search with no label).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsNavGroup {
    pub title: String,
    pub items: Vec<SettingsNavItem>,
}

impl SettingsNavGroup {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            items: Vec::new(),
        }
    }

    pub fn with_item(mut self, item: SettingsNavItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn with_items(mut self, items: impl IntoIterator<Item = SettingsNavItem>) -> Self {
        self.items.extend(items);
        self
    }
}

type SelectCallback = Rc<RefCell<dyn FnMut(usize)>>;
type SearchCallback = Rc<RefCell<dyn FnMut(String)>>;
type SearchFocusCallback = Rc<RefCell<dyn FnMut(bool)>>;
type ClearCallback = Rc<RefCell<dyn FnMut()>>;

/// The rail. `selected` is a flattened index over every item in every group, in
/// draw order; the app keeps the same numbering so the keyboard and the rail
/// cannot disagree.
pub struct SettingsNav {
    groups: Vec<SettingsNavGroup>,
    selected: usize,
    /// The rail's own focus ring, when the rail holds the keyboard.
    focused: Option<usize>,
    search: String,
    search_focused: bool,
    search_placeholder: String,
    show_search: bool,
    on_select: Option<SelectCallback>,
    on_search: Option<SearchCallback>,
    on_search_focus: Option<SearchFocusCallback>,
    on_clear: Option<ClearCallback>,
    built: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsNav {
    pub fn new(groups: Vec<SettingsNavGroup>) -> Self {
        Self {
            groups,
            selected: 0,
            focused: None,
            search: String::new(),
            search_focused: false,
            search_placeholder: "Search settings".to_string(),
            show_search: true,
            on_select: None,
            on_search: None,
            on_search_focus: None,
            on_clear: None,
            built: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    /// Which row carries the focus ring, when the rail owns the keyboard.
    pub fn with_focused(mut self, index: Option<usize>) -> Self {
        self.focused = index;
        self
    }

    pub fn with_search(mut self, value: impl Into<String>) -> Self {
        self.search = value.into();
        self
    }

    pub fn with_search_focused(mut self, focused: bool) -> Self {
        self.search_focused = focused;
        self
    }

    pub fn with_search_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.search_placeholder = placeholder.into();
        self
    }

    pub fn without_search(mut self) -> Self {
        self.show_search = false;
        self
    }

    pub fn with_on_select<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_search<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_search = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_clear<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_clear = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The search field's focus changes, so the app can put the caret in it (and
    /// take it out) from its own keyboard model.
    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_search_focus = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The flat index of the item labelled `label`, if the rail has one. Pages
    /// use it to point the rail at their own page without hardcoding numbers.
    pub fn index_of(&self, label: &str) -> Option<usize> {
        let mut index = 0;
        for group in &self.groups {
            for item in &group.items {
                if item.label == label {
                    return Some(index);
                }
                index += 1;
            }
        }
        None
    }

    /// Every item's label, flattened. The app uses the same order for its page
    /// list, so a rail row and a page can never drift.
    pub fn flat_labels(&self) -> Vec<&str> {
        self.groups
            .iter()
            .flat_map(|group| group.items.iter().map(|item| item.label.as_str()))
            .collect()
    }

    fn build(&mut self, app: &AppContext) {
        let mut children: Vec<Box<dyn Element>> = Vec::new();
        if self.show_search {
            children.push(self.search_row(app));
            // A little air between the search and the first group, like T3's
            // rail (`mt-2`).
            children.push(
                crate::elements::Empty::new()
                    .with_size(vec2f(0.0, 6.0))
                    .finish(),
            );
        }
        let mut flat_index = 0usize;
        for group in &self.groups {
            if !group.title.is_empty() {
                let heading = Text::new(group.title.clone())
                    .with_font_size(SETTINGS_NAV_GROUP_LABEL_SIZE)
                    .with_line_height(1.4)
                    .with_weight(FontWeight::Medium)
                    .with_color(app.theme.color(ColorToken::Muted))
                    .finish();
                children.push(
                    crate::elements::Container::new(heading)
                        .with_padding(crate::elements::EdgeInsets::new(
                            8.0,
                            SETTINGS_NAV_ROW_INSET,
                            4.0,
                            SETTINGS_NAV_ROW_INSET,
                        ))
                        .finish(),
                );
            }
            for item in &group.items {
                let index = flat_index;
                flat_index += 1;
                children.push(self.nav_row(app, item, index));
            }
        }
        self.built = Some(super::stretch_column(children, SETTINGS_NAV_ROW_GAP));
    }

    fn nav_row(&self, app: &AppContext, item: &SettingsNavItem, index: usize) -> Box<dyn Element> {
        let selected = index == self.selected;
        let label = Text::new(item.label.clone())
            .with_font_size(SETTINGS_NAV_LABEL_SIZE)
            .with_line_height(1.4)
            .with_weight(if selected {
                FontWeight::Medium
            } else {
                FontWeight::Regular
            })
            .with_color(if selected {
                app.theme.color(ColorToken::Text)
            } else {
                app.theme.color(ColorToken::Muted)
            })
            .finish();
        let content = crate::elements::Flex::row()
            .with_main_axis_size(crate::elements::MainAxisSize::Max)
            .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Center)
            .with_child(crate::elements::Expanded::new(label).finish())
            .finish();
        let on_select = self.on_select.clone();
        let row = crate::elements::HoverRow::new(content)
            .with_selected(selected)
            .with_padding(crate::elements::EdgeInsets::new(
                6.0,
                SETTINGS_NAV_ROW_INSET,
                6.0,
                SETTINGS_NAV_ROW_INSET,
            ))
            .with_corner_radius(SETTINGS_NAV_ROW_RADIUS)
            .with_on_click(move || {
                if let Some(callback) = on_select.as_ref() {
                    (callback.borrow_mut())(index);
                }
            })
            .finish();
        // The 32px row: 20px text line + 6/6 padding. The width is pinned to
        // the rail's inner width because a Flex column hands its children an
        // unbounded cross axis and the row's `Expanded` label needs a bound.
        let row = crate::elements::ConstrainedBox::new(row)
            .with_width(SETTINGS_RAIL_WIDTH - SETTINGS_RAIL_PADDING * 2.0)
            .with_height(SETTINGS_NAV_ROW_HEIGHT)
            .finish();
        if self.focused == Some(index) {
            Box::new(FocusOutline::new(
                row,
                app.theme.color(ColorToken::Focus),
                2.0,
                SETTINGS_FOCUS_RING_INSET,
                SETTINGS_NAV_ROW_RADIUS + 2.0,
            ))
        } else {
            row
        }
    }

    fn search_row(&self, app: &AppContext) -> Box<dyn Element> {
        let on_search = self.on_search.clone();
        let on_focus = self.on_search_focus.clone();
        let field = crate::elements::SearchInput::new()
            .with_value(self.search.clone())
            .with_placeholder(self.search_placeholder.clone())
            .with_focused(self.search_focused)
            .with_compact(true)
            .with_icon(true)
            .with_extra_height(4.0)
            .with_text_size(12.0)
            .with_corner_radius(8.0)
            .with_on_change(move |value| {
                if let Some(callback) = on_search.as_ref() {
                    (callback.borrow_mut())(value);
                }
            })
            .with_on_focus_change(move |focused| {
                if let Some(callback) = on_focus.as_ref() {
                    (callback.borrow_mut())(focused);
                }
            })
            .finish();
        let field = crate::elements::ConstrainedBox::new(field)
            .with_height(SETTINGS_SEARCH_ROW_HEIGHT)
            .finish();
        let mut parts: Vec<Box<dyn Element>> = vec![crate::elements::Expanded::new(field).finish()];
        if self.search.is_empty() {
            // The `/` cap T3 shows while the field is empty.
            let cap = Text::new("/")
                .with_font_size(11.0)
                .with_line_height(1.0)
                .with_color(app.theme.color(ColorToken::Muted))
                .finish();
            let cap = crate::elements::Container::new(cap)
                .with_padding(crate::elements::EdgeInsets::uniform(4.0))
                .with_background(crate::elements::Fill::Solid(
                    app.theme.color(ColorToken::SurfaceRaised),
                ))
                .with_corner_radius(4.0)
                .finish();
            parts.push(cap);
        } else {
            let clear = crate::elements::IconButton::new(
                crate::elements::Icon::new("x-close")
                    .with_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(vec2f(20.0, 20.0))
            .with_on_click({
                let on_clear = self.on_clear.clone();
                move || {
                    if let Some(callback) = on_clear.as_ref() {
                        (callback.borrow_mut())();
                    }
                }
            })
            .finish();
            parts.push(clear);
        }
        let row = crate::elements::Flex::row()
            .with_main_axis_size(crate::elements::MainAxisSize::Max)
            .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Center)
            .with_spacing(4.0)
            .with_children(parts)
            .finish();
        crate::elements::ConstrainedBox::new(row)
            .with_width(SETTINGS_RAIL_WIDTH - SETTINGS_RAIL_PADDING * 2.0)
            .with_height(SETTINGS_SEARCH_ROW_HEIGHT)
            .finish()
    }

    fn root_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.built.is_none() {
            self.build(app);
        }
        self.built.as_mut().unwrap()
    }
}

impl Element for SettingsNav {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let inner = SizeConstraint::new(
            vec2f(
                (SETTINGS_RAIL_WIDTH - SETTINGS_RAIL_PADDING * 2.0).max(0.0),
                constraint.min.y,
            ),
            vec2f(
                SETTINGS_RAIL_WIDTH - SETTINGS_RAIL_PADDING * 2.0,
                constraint.max.y,
            ),
        );
        let content = self.root_mut(app).layout(inner, ctx, app);
        let size = vec2f(SETTINGS_RAIL_WIDTH, constraint.max.y);
        self.size = Some(size);
        // The content's height is what the pane may need to scroll; recorded on
        // the child, but the rail itself fills the pane's height.
        let _ = content;
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        if let Some(renderer) = ctx.renderer.as_mut() {
            // The rail's own surface, then the hairline T3 draws on its end
            // (`border-e`), read as the `Border` token.
            renderer.fill_rect(
                rectf(origin.x, origin.y, size.x, size.y),
                app.theme.color(ColorToken::Surface),
            );
            renderer.fill_rect(
                rectf(origin.x + size.x - 1.0, origin.y, 1.0, size.y),
                app.theme.color(ColorToken::Border),
            );
        }
        let padding = crate::elements::EdgeInsets::uniform(SETTINGS_RAIL_PADDING);
        self.root_mut(app).paint(
            vec2f(origin.x + padding.left, origin.y + padding.top),
            ctx,
            app,
        );
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
        // The children were laid out at the rail's padding offset; dispatch
        // through them directly (they hold their own absolute origins from
        // layout).
        self.root_mut(app).dispatch_event(event, ctx, app)
    }
}
/// Whether a query matches a nav item: a case-insensitive substring, the way
/// T3's search matches a setting's label and section.
pub fn search_matches(query: &str, label: &str, section: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    label.to_lowercase().contains(&query) || section.to_lowercase().contains(&query)
}

/// The empty search result sentence, in one place for the rail and the tests.
pub const NO_SETTINGS_FOUND: &str = "No settings found";
