pub mod color;
pub mod elements;
pub mod event;
pub mod geometry;
pub mod platform;
pub mod render;
pub mod scene;
pub mod style;
pub mod test_util;
pub mod theme;
pub mod views;

pub use platform::current as platform_current;

pub use color::ColorU;
pub use elements::{
    AgentCard, Align, Alignment, AppContext, Avatar, AvatarShape, Axis, Border, Button,
    ButtonVariant, Caption, ChatAction, ChatComposer, ChatFragment, ChatFragmentKind, ChatHeader,
    ChatLayout, ChatMessage, ChatMessageBubble, ChatRole, ChatSidebar, Checkbox, Chip, Clipped,
    AgentCardUi, Code, ConnectorCard, Container, ConversationEntry, ConversationListItem,
    ConversationSidebar,
    ConversationStatus, CrossAxisAlignment, Divider, Drawer, DrawerAnchor, DropdownItem,
    DropdownMenu, EdgeInsets, Element, EventContext, Expanded, Fill, Flex, Header as UiHeader,
    Icon, IconButton, IconName, Label, LabelSize, LayoutContext, MainAxisAlignment, MainAxisSize,
    Margin, Modal, Padding, Page, PaintContext, Point, QuickActionButton, Rect, RightPanel,
    RunningIndicator, Scrollable, SearchInput, Select, SelectOption, SelectableElement, Sidebar,
    SidebarItem, SizeConstraint, Sheet, Spacer, Stack, Switch, Tab, TabBar, TerminalBlock,
    TerminalData, TerminalLine, TerminalLineKind, TerminalStatus, Text, TextArea, TextInput,
    ThreadListItem, ToggleButton, Toolbar, Topbar, TopbarButton, Vector2FExt,
    CHAT_RIGHT_SIDEBAR_WIDTH, CONVERSATION_SIDEBAR_WIDTH, SHEET_DEFAULT_WIDTH,
};
pub use geometry::{rectf, size2f, vec2f, PointF, RectF, Size2F, Vector2F};
pub use views::chat_view::ChatView;
pub use views::settings_view::{SettingsPage, SettingsView};
pub use views::thread_list_view::{ThreadKind, ThreadListEntry, ThreadListView};
pub use views::thread_view::ThreadView;
pub use views::threads_container::ThreadsContainer;
