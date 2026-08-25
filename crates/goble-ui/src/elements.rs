pub use agent_card::AgentCard;
pub use align::{Align, Alignment};
pub use avatar::{Avatar, AvatarShape};
pub use button::{Button, ButtonVariant};
pub use caption::Caption;
pub use chat_composer::ChatComposer;
pub use chat_content::{ChatAction, ChatFragment, ChatFragmentKind, ChatMessage, ChatRole};
pub use chat_header::ChatHeader;
pub use chat_layout::{ChatLayout, CHAT_RIGHT_SIDEBAR_WIDTH};
pub use chat_sidebar::{ChatSidebar, RoutineItem, CHAT_SIDEBAR_WIDTH};
pub use conversation_list_item::{AgentCardUi, ConversationListItem, ConversationStatus};
pub use conversation_sidebar::{
    ConversationEntry, ConversationSidebar, CONVERSATION_SIDEBAR_WIDTH,
};
pub mod markdown;
pub use chat_message_bubble::ChatMessageBubble;
pub use checkbox::Checkbox;
pub use chip::Chip;
pub use clipped::Clipped;
pub use code::Code;
pub use connector_card::ConnectorCard;
pub use constrained_box::ConstrainedBox;
pub use container::Container;
pub use divider::Divider;
pub use drawer::{Drawer, DrawerAnchor};
pub use dropdown_menu::{DropdownItem, DropdownMenu};
pub use empty::Empty;
pub use expanded::Expanded;
pub use flex::Flex;
pub use header::Header;
pub use hover_button::HoverButton;
pub use icon::{Icon, IconName};
pub use icon_button::IconButton;
pub use inline_text::{resolve_span as resolve_inline_span, InlineText, TextSpan};
pub use label::{Label, LabelSize};
pub use markdown::parse_markdown;
pub use modal::Modal;
pub use padding::Padding;
pub use page::Page;
pub use popup_menu::{PopupMenu, PopupMenuItem, PopupMenuPosition};
pub use quick_action_button::QuickActionButton;
pub use rect::Rect;
pub use right_panel::RightPanel;
pub use running_indicator::RunningIndicator;
pub use scrollable::Scrollable;
pub use search_input::SearchInput;
pub use select::{Select, SelectOption};
pub use sheet::{Sheet, SHEET_DEFAULT_WIDTH};
pub use shell::{ActiveView, SettingsTab, ShellState, ShellView, SidebarMode};
pub use sidebar::Sidebar;
pub use sidebar_item::SidebarItem;
pub use spacer::Spacer;
pub use stack::Stack;
pub use switch::Switch;
pub use tab_bar::{Tab, TabBar};
pub use terminal_block::{
    TerminalBlock, TerminalData, TerminalLine, TerminalLineKind, TerminalStatus,
};
pub use text::Text;
pub use text_area::TextArea;
pub use text_input::TextInput;
pub use thread_list_item::ThreadListItem;
pub use titlebar::TitleBar;
pub use toggle_button::ToggleButton;
pub use toolbar::Toolbar;
pub use tooltip::{Tooltip, TooltipPosition};
pub use topbar::{Topbar, TopbarButton};

use std::any::Any;

use crate::color::ColorU;
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, PointF, RectF, Size2F, Vector2F};
use crate::scene::ZIndex;

/// Constraints passed to an element during layout.
#[derive(Clone, Copy, Debug, Default)]
pub struct SizeConstraint {
    pub min: Vector2F,
    pub max: Vector2F,
}

impl SizeConstraint {
    pub fn new(min: Vector2F, max: Vector2F) -> Self {
        Self { min, max }
    }

    pub fn width(&self) -> f32 {
        self.max.x
    }

    pub fn height(&self) -> f32 {
        self.max.y
    }

    pub fn tight(size: Vector2F) -> Self {
        Self {
            min: size,
            max: size,
        }
    }

    pub fn loose(max: Vector2F) -> Self {
        Self {
            min: Vector2F::zero(),
            max,
        }
    }
}

/// Context available during layout.
#[derive(Default)]
pub struct LayoutContext;

/// Context available after layout.
#[derive(Default)]
pub struct AfterLayoutContext;

/// Context available during painting.
pub struct PaintContext {
    pub renderer: Option<crate::render::Renderer>,
    /// Logical pointer position (window coordinates). Hover is computed at
    /// paint time from this, because the tree is rebuilt every frame, so
    /// element-local hover state would otherwise be reset before it is drawn.
    pub cursor_position: Vector2F,
    /// Whether the pointer is currently over the window. When the pointer
    /// leaves the window there is no hover.
    pub cursor_inside: bool,
}

impl PaintContext {
    pub fn new(renderer: crate::render::Renderer) -> Self {
        Self {
            renderer: Some(renderer),
            cursor_position: vec2f(0.0, 0.0),
            cursor_inside: false,
        }
    }

    /// True when the given bounds contain the current pointer. Used at paint
    /// time for hover overlays; returns false when the pointer is outside.
    pub fn hovered(&self, bounds: RectF) -> bool {
        self.cursor_inside
            && crate::elements::interactive::contains(bounds, self.cursor_position)
    }
}

impl Default for PaintContext {
    fn default() -> Self {
        Self {
            renderer: Some(crate::render::Renderer::new()),
            cursor_position: vec2f(0.0, 0.0),
            cursor_inside: false,
        }
    }
}

/// Context available during event dispatch.
#[derive(Default)]
pub struct EventContext;

/// Generic application context.
#[derive(Default, Clone)]
pub struct AppContext {
    pub theme: crate::theme::Theme,
}

/// A point in element-space, including a z-index for stacking.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    xy: Vector2F,
    z_index: ZIndex,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            xy: vec2f(x, y),
            z_index: ZIndex(0),
        }
    }

    pub fn from_vec2f(xy: Vector2F, z_index: ZIndex) -> Self {
        Self { xy, z_index }
    }

    pub fn x(&self) -> f32 {
        self.xy.x
    }

    pub fn y(&self) -> f32 {
        self.xy.y
    }

    pub fn xy(&self) -> Vector2F {
        self.xy
    }

    pub fn z_index(&self) -> ZIndex {
        self.z_index
    }

    pub fn with_z_index(mut self, z_index: ZIndex) -> Self {
        self.z_index = z_index;
        self
    }
}

impl Default for Point {
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Axis {
    #[default]
    Horizontal,
    Vertical,
}

impl Axis {
    pub fn invert(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }

    pub fn to_point(self, main: f32, cross: f32) -> Vector2F {
        match self {
            Self::Horizontal => vec2f(main, cross),
            Self::Vertical => vec2f(cross, main),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisOrientation {
    Normal,
    Reverse,
}

impl Default for AxisOrientation {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MainAxisAlignment {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CrossAxisAlignment {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MainAxisSize {
    #[default]
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Fill {
    #[default]
    None,
    Solid(ColorU),
}

impl From<ColorU> for Fill {
    fn from(color: ColorU) -> Self {
        Fill::Solid(color)
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct Margin {
    top: f32,
    left: f32,
    bottom: f32,
    right: f32,
}

impl Margin {
    pub const fn uniform(margin: f32) -> Self {
        Self {
            top: margin,
            left: margin,
            bottom: margin,
            right: margin,
        }
    }

    pub const fn with_left(mut self, margin: f32) -> Self {
        self.left = margin;
        self
    }

    pub const fn with_right(mut self, margin: f32) -> Self {
        self.right = margin;
        self
    }

    pub const fn with_top(mut self, margin: f32) -> Self {
        self.top = margin;
        self
    }

    pub const fn with_bottom(mut self, margin: f32) -> Self {
        self.bottom = margin;
        self
    }

    pub fn top(&self) -> f32 {
        self.top
    }
    pub fn left(&self) -> f32 {
        self.left
    }
    pub fn bottom(&self) -> f32 {
        self.bottom
    }
    pub fn right(&self) -> f32 {
        self.right
    }
}

pub use crate::style::EdgeInsets;

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub struct Border {
    width: f32,
    color: Fill,
    top: bool,
    left: bool,
    bottom: bool,
    right: bool,
}

impl Border {
    pub const fn new(width: f32) -> Self {
        Self {
            width,
            color: Fill::None,
            top: false,
            left: false,
            bottom: false,
            right: false,
        }
    }

    pub fn all(width: f32) -> Self {
        Self {
            width,
            color: Fill::None,
            top: true,
            left: true,
            bottom: true,
            right: true,
        }
    }

    pub fn with_border_fill<F: Into<Fill>>(mut self, fill: F) -> Self {
        self.color = fill.into();
        self
    }

    pub fn with_border_color(mut self, color: ColorU) -> Self {
        self.color = Fill::Solid(color);
        self
    }
}

impl From<ColorU> for Border {
    fn from(value: ColorU) -> Self {
        Border::all(1.0).with_border_color(value)
    }
}

pub trait Vector2FExt {
    fn along(self, axis: Axis) -> f32;
    fn project_onto(self, axis: Axis) -> Vector2F;
}

impl Vector2FExt for Vector2F {
    fn along(self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.x,
            Axis::Vertical => self.y,
        }
    }

    fn project_onto(self, axis: Axis) -> Vector2F {
        match axis {
            Axis::Horizontal => vec2f(self.x, 0.0),
            Axis::Vertical => vec2f(0.0, self.y),
        }
    }
}

/// The core UI element trait.
pub trait Element {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F;

    fn after_layout(&mut self, _ctx: &mut AfterLayoutContext, _app: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext);

    fn size(&self) -> Option<Vector2F>;

    fn origin(&self) -> Option<Point>;

    fn z_index(&self) -> Option<ZIndex> {
        self.origin().map(|p| p.z_index())
    }

    fn bounds(&self) -> Option<RectF> {
        self.origin().and_then(|p| {
            self.size()
                .map(|s| RectF::new(PointF::new(p.x(), p.y()), Size2F::new(s.x, s.y)))
        })
    }

    fn parent_data(&self) -> Option<&dyn Any> {
        None
    }

    fn flex_grow(&self) -> Option<f32> {
        None
    }

    fn as_selectable_element(&self) -> Option<&dyn SelectableElement> {
        None
    }

    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }

    fn finish(self) -> Box<dyn Element>
    where
        Self: 'static + Sized,
    {
        Box::new(self)
    }
}

/// Marker trait for elements that support text selection.
pub trait SelectableElement {
    fn selection_bounds(&self) -> Option<RectF>;
}

/// Helper trait for elements that can contain children.
pub trait ParentElement: Element {
    fn add_child(&mut self, child: Box<dyn Element>);
    fn add_children(&mut self, children: impl IntoIterator<Item = Box<dyn Element>>);
}

impl<T> ParentElement for T
where
    T: Element + Extend<Box<dyn Element>>,
{
    fn add_child(&mut self, child: Box<dyn Element>) {
        self.extend(Some(child));
    }

    fn add_children(&mut self, children: impl IntoIterator<Item = Box<dyn Element>>) {
        self.extend(children);
    }
}

pub mod agent_card;
pub mod align;
pub mod avatar;
pub mod button;
pub mod caption;
pub mod chat_composer;
pub mod chat_content;
pub mod chat_header;
pub mod chat_layout;
pub mod chat_message_bubble;
pub mod chat_sidebar;
pub mod checkbox;
pub mod chip;
pub mod clipped;
pub mod code;
pub mod connector_card;
pub mod constrained_box;
pub mod container;
pub mod conversation_list_item;
pub mod conversation_sidebar;
pub mod divider;
pub mod drawer;
pub mod dropdown_menu;
pub mod empty;
pub mod expanded;
pub mod flex;
pub mod group_chat_message;
pub mod group_chat_message_group;
pub use group_chat_message::GroupChatMessage;
pub use group_chat_message_group::GroupChatMessageGroup;
pub mod header;
pub mod hover_button;
pub mod icon;
pub mod icon_button;
pub mod inline_text;
pub mod interactive;
pub mod label;
pub mod modal;
pub mod padding;
pub mod page;
pub mod popup_menu;
pub mod quick_action_button;
pub mod rect;
pub mod right_panel;
pub mod running_indicator;
pub mod scrollable;
pub mod search_input;
pub mod select;
pub mod sheet;
pub mod shell;
pub mod sidebar;
pub mod sidebar_item;
pub mod spacer;
pub mod stack;
pub mod switch;
pub mod tab_bar;
pub mod terminal_block;
pub mod text;
pub mod text_area;
pub mod text_input;
pub mod thread_list_item;
pub mod titlebar;
pub mod toggle_button;
pub mod toolbar;
pub mod tooltip;
pub mod topbar;
