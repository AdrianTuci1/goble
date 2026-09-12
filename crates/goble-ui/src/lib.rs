pub mod color;
pub mod elements;
pub mod event;
pub mod geometry;
pub mod platform;
pub mod render;
pub mod scene;
pub mod style;
pub mod syntax;
pub mod test_util;
pub mod theme;
pub mod vim;

/// The provider-reported token accounting, re-exported so a host can hand the
/// transcript its conversation totals without naming `goble-core`.
pub use goble_core::llm::TokenUsage;
pub mod views;

pub use platform::current as platform_current;

pub use color::{hsv_to_rgb, rgb_to_hsv, ColorU};
pub use elements::{
    AgentCard, Align, Alignment, AppContext, AskUserCard, AskUserUi, Avatar, AvatarShape, Axis,
    Border, Button, ButtonVariant, Caption, ChatAction, ChatComposer, ChatFragment,
    ChatFragmentKind, ChatHeader, ChatLayout, ChatMessage, ChatMessageBubble, ChatRole, ChatSidebar,
    CommandProposalUi,
    Checkbox, Chip, Clipped,
    RoutineItem,
    AgentCardUi, Code, ConnectorCard, Container, ConversationEntry, ConversationListItem,
    ConversationSidebar,
    ConversationStatus, CrossAxisAlignment, Dialog, Diff, DiffLine, DiffLineKind, DiffRow,
    DiffStats, Hunk, Divider, Drawer, DrawerAnchor, DropdownItem,
    DropdownMenu, EdgeInsets, Element, EventContext, Expanded, Fill, Flex, FrameSize, FrameView,
    Header as UiHeader,
    Icon, IconButton, IconName, KeyHandler, Label, LabelSize, LayoutContext, MainAxisAlignment,
    MainAxisSize,
    Margin, Modal, Padding, Page, PaintContext, Point, QuickActionButton, Rect, RightPanel,
    RunningIndicator, ScrollState, Scrollable, SearchInput, Select, SelectOption,
    SelectableElement, Sidebar,
    SidebarItem, SizeConstraint, Sheet, Spacer, SplitNode, Stack, SubAgentRow, SubAgentRowStatus,
    Switch, Tab, TabBar, TerminalBlock,
    TerminalData, TerminalFilter, TerminalLine, TerminalLineKind, TerminalStatus, Text, TextArea,
    TextInput,
    ToolCall, ToolDisplayMode, tool_fold_key,
    ThreadListItem, ToggleButton, Toolbar, Topbar, TopbarButton, TurnActivity, TurnStatus,
    TurnStatusFooter, Vector2FExt, WorkKind, WorkKindCount,
    CHAT_RIGHT_SIDEBAR_WIDTH, CONVERSATION_SIDEBAR_WIDTH, DIALOG_DEFAULT_WIDTH, SHEET_DEFAULT_WIDTH,
};
pub use geometry::{rectf, size2f, vec2f, PointF, RectF, Size2F, Vector2F};
pub use syntax::highlight;
pub use views::chat_view::ChatView;
pub use views::settings_view::{SettingsPage, SettingsView};
pub use views::thread_list_view::{ThreadKind, ThreadListEntry, ThreadListView};
pub use views::thread_view::ThreadView;
pub use views::threads_container::ThreadsContainer;
