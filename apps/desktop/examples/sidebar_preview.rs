//! Headless raster previews of the redesigned desktop sidebar.
//!
//! The live native window cannot be captured in this environment, so this
//! example builds the real Workspaces element tree (the same widgets the app's
//! sidebar draws) and rasterizes it to PNGs through the shared preview
//! rasterizer. It is a visual-verification tool, not a product surface.
//!
//! The scenes mirror the app's sidebar tree shape: the toolbelt, the search
//! field, the restored "New conversation" row, the Workspaces header and a list
//! inside an `Expanded` under a full-height Surface fill, so the PNGs show the
//! same bottom edge the window does (no page background after the last row).
//!
//! Usage: cargo run -p goble-app --example sidebar_preview

#[path = "../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::conversation_list_item::{
    SidebarActionRow, SidebarDrag, SidebarGroupRow, SidebarGroupUi, SidebarIconButton,
    SidebarMenuItem, SidebarSessionRow, SIDEBAR_GROUP_GAP, SIDEBAR_HEADER_BUTTON,
    SIDEBAR_HEADER_HEIGHT, SIDEBAR_HEADER_ICON, SIDEBAR_HEADER_PADDING_START,
    SIDEBAR_ICON_NEW_CHAT, SIDEBAR_ICON_PIN_FILL, SIDEBAR_ICON_PROJECT_ADD, SIDEBAR_ICON_SLIDERS,
    SIDEBAR_LIST_BOTTOM_PAD, SIDEBAR_ROW_GAP, SIDEBAR_SECTION_TITLE, SIDEBAR_SECTION_TOP_INSET,
    SIDEBAR_TITLE_LINE, SIDEBAR_TITLE_SIZE,
};
use goble_ui::elements::{
    AgentCardUi, AppContext, ConstrainedBox, Container, ConversationStatus, CrossAxisAlignment,
    EdgeInsets, Element, Expanded, Fill, Flex, HoverChipLayer, LayoutContext, MainAxisSize,
    Padding, PaintContext, Point, PopupMenu, PopupMenuItem, PopupMenuPosition, SearchInput,
    SizeConstraint, Spacer, Text,
};
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken, Theme};

const SIDEBAR_W: f32 = 300.0;
const SIDEBAR_H: f32 = 760.0;
/// A window taller than the list: the Surface fill must reach this bottom edge.
const TALL_H: f32 = 1150.0;
const OUT_DIR: &str = "/tmp/goble-work/out";

#[derive(Clone, Copy, PartialEq)]
enum Scene {
    Rest,
    Tall,
    ActionHover,
    SelectedRunning,
    Hover,
    HoverCard,
    WorkspaceHover,
    WorkspaceMenu,
    Menu,
    ViewOptions,
    Drag,
    Archived,
}

/// The DSH session menu the app builds for a listed conversation: Pin session,
/// Rename, Fork session — separator — Copy Session ID — separator — Archive
/// session, then Goble's own Delete last behind another separator.
fn full_menu_items() -> Vec<SidebarMenuItem> {
    vec![
        SidebarMenuItem::new("Pin session", "pin-outline", || {}),
        SidebarMenuItem::new("Rename", "edit", || {}),
        SidebarMenuItem::new("Fork session", "branch", || {}),
        SidebarMenuItem::new("Copy Session ID", "copy", || {}).with_separator_before(true),
        SidebarMenuItem::new("Archive session", "archive", || {}).with_separator_before(true),
        SidebarMenuItem::new("Delete conversation", "trash-02", || {})
            .with_separator_before(true)
            .with_danger(true),
    ]
}

fn row_ui() -> Rc<RefCell<AgentCardUi>> {
    Rc::new(RefCell::new(AgentCardUi::default()))
}

/// The workspace rows' tray rows: DSH's complete workspace context menu
/// (Rename, Show in Finder, then Delete workspace behind a separator). The app
/// supplies these per row.
fn workspace_menu_items() -> Vec<SidebarMenuItem> {
    vec![
        SidebarMenuItem::new("Rename", "edit", || {}),
        SidebarMenuItem::new("Show in Finder", "folder-open", || {}),
        SidebarMenuItem::new("Delete workspace", "trash-02", || {})
            .with_separator_before(true)
            .with_danger(true),
    ]
}

/// One workspace row, exactly the widget the app builds: the folder/triangle
/// leading slot, its label, and the hover strip (More + New session) with the
/// app-owned tray.
fn group(label: &str, expanded: bool, active: bool) -> Box<dyn Element> {
    SidebarGroupRow::new(label, expanded)
        .with_active(active)
        .with_ui(Rc::new(RefCell::new(SidebarGroupUi::default())))
        .with_viewport_height(SIDEBAR_H)
        .with_menu_items(workspace_menu_items())
        .with_on_create_session(|| {})
        .finish()
}

/// The same workspace row with the pointer on it (DSH's folder->chevron swap
/// plus the action strip).
fn hovered_group(label: &str, expanded: bool, active: bool) -> Box<dyn Element> {
    SidebarGroupRow::new(label, expanded)
        .with_active(active)
        .with_hovered(true)
        .with_ui(Rc::new(RefCell::new(SidebarGroupUi::default())))
        .with_viewport_height(SIDEBAR_H)
        .with_menu_items(workspace_menu_items())
        .with_on_create_session(|| {})
        .finish()
}

/// A workspace row whose tray is open: DSH keeps the row's hover fill and the
/// strip shown while the menu is up (`menuOpen`).
fn open_menu_group(label: &str, expanded: bool) -> Box<dyn Element> {
    let ui = Rc::new(RefCell::new(SidebarGroupUi {
        menu_open: true,
        // DSH opens a context menu unfilled: no row carries the selection fill
        // until the pointer or the keyboard reaches one (the app opens at
        // `usize::MAX`, the "no row" marker).
        highlight: usize::MAX,
    }));
    SidebarGroupRow::new(label, expanded)
        .with_hovered(true)
        .with_ui(ui)
        .with_viewport_height(SIDEBAR_H)
        .with_menu_items(workspace_menu_items())
        .with_on_create_session(|| {})
        .finish()
}

fn session(
    id: &str,
    title: &str,
    time: &str,
    status: ConversationStatus,
    selected: bool,
    hovered: bool,
) -> Box<dyn Element> {
    SidebarSessionRow::new(id, title, time)
        .with_status(status)
        .with_selected(selected)
        .with_hovered(hovered)
        .with_harness("claude")
        .with_directory("/Users/adrian.tucicovencogmail.com/Projects/goble")
        .with_viewport_height(SIDEBAR_H)
        // The app's rows carry both quick actions, so a hovered row draws the
        // full DSH strip: More, Archive, Pin.
        .with_on_quick_pin(|| {})
        .with_on_quick_archive(|| {})
        .with_ui(row_ui())
        .finish()
}

/// The restored "New conversation" row: a 32pt full-width rounded band with a
/// 16pt glyph and a 14pt label, exactly the widget the app builds.
fn new_conversation(hovered: bool) -> Box<dyn Element> {
    SidebarActionRow::new(SIDEBAR_ICON_NEW_CHAT, "New conversation")
        .with_paint_inset(0.0)
        .with_hovered(hovered)
        .finish()
}

/// The sidebar's own toolbelt: the three view tabs, then Settings and Hide.
fn toolbelt() -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0);
    for (index, icon) in ["message-chat-square", "folder-closed", "search"]
        .into_iter()
        .enumerate()
    {
        row = row.with_child(
            SidebarIconButton::new(icon)
                .with_button_size(SIDEBAR_HEADER_BUTTON)
                .with_icon_size(14.0)
                .with_radius(12.0)
                .with_active(index == 0)
                .finish(),
        );
    }
    row = row.with_child(Spacer::new().finish());
    for icon in ["settings", "x-close"] {
        row = row.with_child(
            SidebarIconButton::new(icon)
                .with_button_size(SIDEBAR_HEADER_BUTTON)
                .with_icon_size(14.0)
                .with_radius(12.0)
                .finish(),
        );
    }
    row.finish()
}

/// The search field: the app's compact, magnifier-less 32pt box.
fn search(app: &AppContext) -> Box<dyn Element> {
    SearchInput::new()
        .with_placeholder("Search")
        .with_compact(true)
        .with_icon(false)
        .with_text_size(13.0)
        .with_corner_radius(app.theme.radius_px())
        .with_extra_height(4.2)
        .finish()
}

fn header(menu_open: bool) -> Box<dyn Element> {
    let app = AppContext::default();
    let sliders = SidebarIconButton::new(SIDEBAR_ICON_SLIDERS)
        .with_button_size(SIDEBAR_HEADER_BUTTON)
        .with_icon_size(SIDEBAR_HEADER_ICON)
        .with_radius(12.0)
        .finish();
    let menu_open = Rc::new(RefCell::new(menu_open));
    let items = vec![
        PopupMenuItem {
            selected: true,
            ..PopupMenuItem::new("Folder")
                .with_icon("folder-close")
                .with_description("Group by folder")
        },
        PopupMenuItem {
            selected: false,
            ..PopupMenuItem::new("Flat list")
                .with_icon("flat-list")
                .with_description("One list, no folder headers")
        },
        PopupMenuItem {
            selected: true,
            ..PopupMenuItem::new("Recent first")
                .with_icon("clock")
                .with_description("Newest first")
        },
        PopupMenuItem {
            selected: false,
            ..PopupMenuItem::new("Manual order")
                .with_icon("chevrons-up-down")
                .with_description("Drag rows to reorder")
        },
        PopupMenuItem {
            selected: true,
            ..PopupMenuItem::new("Hide archived")
                .with_icon("archive-off")
                .with_description("Hidden from the list")
        },
        PopupMenuItem {
            selected: false,
            ..PopupMenuItem::new("Show archived")
                .with_icon("layers-three-01")
                .with_description("Archived included")
        },
        PopupMenuItem {
            selected: false,
            ..PopupMenuItem::new("Archived only")
                .with_icon("archive-check")
                .with_description("Only archived conversations")
        },
    ];
    let menu = PopupMenu::new(sliders, items)
        .with_open(menu_open.clone())
        .with_corner_radius(12.0)
        .with_max_width(220.0)
        .with_row_height(44.0)
        .with_icon_size(16.0)
        // The current choice reads in the app's accent cyan (the view-options
        // menu's own selected-label ink).
        .with_selected_color(ColorToken::Focus)
        .with_position(PopupMenuPosition::BelowEnd)
        .finish();
    let menu = PreviewDeferredOverlay::new(menu, menu_open).finish();
    let add = SidebarIconButton::new(SIDEBAR_ICON_PROJECT_ADD)
        .with_button_size(SIDEBAR_HEADER_BUTTON)
        .with_icon_size(SIDEBAR_HEADER_ICON)
        .with_radius(12.0)
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(
            Text::new(SIDEBAR_SECTION_TITLE)
                .with_font_size(SIDEBAR_TITLE_SIZE)
                .with_line_height(SIDEBAR_TITLE_LINE / SIDEBAR_TITLE_SIZE)
                .with_theme_color(ColorToken::Muted, &app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(menu)
        .with_child(add)
        .finish();
    Container::new(row)
        .with_padding(EdgeInsets::new(
            SIDEBAR_HEADER_PADDING_START,
            (SIDEBAR_HEADER_HEIGHT - SIDEBAR_HEADER_BUTTON) / 2.0,
            0.0,
            (SIDEBAR_HEADER_HEIGHT - SIDEBAR_HEADER_BUTTON) / 2.0,
        ))
        .finish()
}

fn block(children: Vec<Box<dyn Element>>) -> Box<dyn Element> {
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(SIDEBAR_ROW_GAP);
    for child in children {
        column = column.with_child(child);
    }
    column.finish()
}

fn scene(app: &AppContext, scene: Scene) -> Box<dyn Element> {
    let mut rows: Vec<Box<dyn Element>> = Vec::new();

    // The list rows for the scene, under the Workspaces header.
    match scene {
        Scene::Rest | Scene::Tall | Scene::ActionHover | Scene::ViewOptions => {
            rows.push(block(vec![
                group("goble", true, false),
                session(
                    "s1",
                    "Redesign panouri și composer-ul",
                    "4min",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
                session(
                    "s2",
                    "Automate Instagram posting",
                    "17h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
            rows.push(block(vec![
                group("Projects", true, false),
                session(
                    "s3",
                    "Open-source project for a self-hosted VPS",
                    "22h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
            rows.push(block(vec![
                group("Conductor", true, false),
                session(
                    "s4",
                    "Public key for the VPS server",
                    "23h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
        }
        Scene::SelectedRunning => {
            rows.push(block(vec![
                group("goble", true, true),
                session(
                    "s1",
                    "Redesign panouri și composer-ul",
                    "4min",
                    ConversationStatus::Running,
                    true,
                    false,
                ),
                session(
                    "s2",
                    "Automate Instagram posting",
                    "17h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
            rows.push(block(vec![
                group("Projects", true, false),
                session(
                    "s3",
                    "Open-source project for a self-hosted VPS",
                    "22h",
                    ConversationStatus::Waiting,
                    false,
                    false,
                ),
            ]));
        }
        Scene::Hover => {
            rows.push(block(vec![
                group("goble", true, false),
                session(
                    "s1",
                    "Redesign panouri și composer-ul",
                    "4min",
                    ConversationStatus::Success,
                    false,
                    true,
                ),
                session(
                    "s2",
                    "Automate Instagram posting",
                    "17h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
            rows.push(block(vec![
                group("Projects", true, false),
                session(
                    "s3",
                    "Open-source project for a self-hosted VPS",
                    "22h",
                    ConversationStatus::Error,
                    false,
                    false,
                ),
            ]));
        }
        Scene::WorkspaceHover => {
            // A workspace row under the pointer: the folder swaps to the
            // disclosure triangle and the strip shows More + New session.
            rows.push(block(vec![
                hovered_group("goble", true, false),
                session(
                    "s1",
                    "Redesign panouri și composer-ul",
                    "4min",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
                session(
                    "s2",
                    "Automate Instagram posting",
                    "17h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
            rows.push(block(vec![
                group("Projects", true, false),
                session(
                    "s3",
                    "Open-source project for a self-hosted VPS",
                    "22h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
        }
        Scene::WorkspaceMenu => {
            // The workspace row's tray open: DSH's workspace context menu.
            rows.push(block(vec![
                open_menu_group("goble", true),
                session(
                    "s1",
                    "Redesign panouri și composer-ul",
                    "4min",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
                session(
                    "s2",
                    "Automate Instagram posting",
                    "17h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
            rows.push(block(vec![
                group("Projects", true, false),
                session(
                    "s3",
                    "Open-source project for a self-hosted VPS",
                    "22h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
        }
        Scene::HoverCard => {
            let mut card_ui = AgentCardUi::default();
            card_ui.hover = true;
            card_ui.hover_card_open = true;
            // 22 hours ago, so the card shows the long-form relative time the
            // app shows (`22h ago`), not the row's compact label.
            let updated_at = (chrono::Utc::now() - chrono::Duration::hours(22)).to_rfc3339();
            rows.push(block(vec![
                group("goble", true, false),
                session(
                    "s1",
                    "Redesign panouri și composer-ul",
                    "4min",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
                SidebarSessionRow::new("s3", "Open-source project for a self-hosted VPS", "22h")
                    .with_status(ConversationStatus::Running)
                    .with_hovered(true)
                    .with_harness("codex")
                    .with_directory("/Users/adrian.tucicovencogmail.com/Projects/goble")
                    .with_updated_at(updated_at)
                    .with_ui(Rc::new(RefCell::new(card_ui)))
                    .finish(),
            ]));
        }
        Scene::Menu => {
            let mut ui = AgentCardUi::default();
            ui.hover = true;
            ui.menu_open = true;
            // DSH opens the context menu unfilled (see `open_menu_group`).
            ui.highlight = usize::MAX;
            rows.push(block(vec![
                group("goble", true, false),
                SidebarSessionRow::new("s1", "Redesign panouri și composer-ul", "4min")
                    .with_status(ConversationStatus::Success)
                    .with_starred(true)
                    .with_hovered(true)
                    .with_harness("goble")
                    .with_menu_items(full_menu_items())
                    .with_viewport_height(SIDEBAR_H)
                    .with_ui(Rc::new(RefCell::new(ui)))
                    .finish(),
                session(
                    "s2",
                    "Automate Instagram posting",
                    "17h",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
            ]));
        }
        Scene::Archived => {
            // The archived look: muted title, blank leading slot, no pin; the
            // hovered archived row also shows the hover card's Archived line.
            let mut card_ui = AgentCardUi::default();
            card_ui.hover = true;
            card_ui.hover_card_open = true;
            let updated_at = (chrono::Utc::now() - chrono::Duration::days(3)).to_rfc3339();
            rows.push(block(vec![
                group("goble", true, false),
                session(
                    "s1",
                    "Redesign panouri și composer-ul",
                    "4min",
                    ConversationStatus::Success,
                    false,
                    false,
                ),
                SidebarSessionRow::new("s2", "Automate Instagram posting", "3d")
                    .with_status(ConversationStatus::Error)
                    .with_archived(true)
                    .with_hovered(true)
                    .with_harness("codex")
                    .with_directory("/Users/adrian.tucicovencogmail.com/Projects/goble")
                    .with_updated_at(updated_at)
                    .with_viewport_height(SIDEBAR_H)
                    .with_on_quick_archive(|| {})
                    .with_ui(Rc::new(RefCell::new(card_ui)))
                    .finish(),
                SidebarSessionRow::new("s3", "Public key for the VPS server", "1y")
                    .with_status(ConversationStatus::Success)
                    .with_archived(true)
                    .with_harness("claude")
                    .with_viewport_height(SIDEBAR_H)
                    .with_ui(row_ui())
                    .finish(),
            ]));
        }
        Scene::Drag => {
            let drag = Rc::new(RefCell::new(SidebarDrag {
                press: None,
                active: true,
                marker: Some(("s2".to_string(), true)),
            }));
            let mut ui = AgentCardUi::default();
            ui.hover = true;
            rows.push(block(vec![
                group("goble", true, false),
                SidebarSessionRow::new("s1", "Redesign panouri și composer-ul", "4min")
                    .with_status(ConversationStatus::Success)
                    .with_hovered(true)
                    .with_harness("goble")
                    .with_ui(Rc::new(RefCell::new(ui)))
                    .with_drag(drag.clone())
                    .finish(),
                SidebarSessionRow::new("s2", "Automate Instagram posting", "17h")
                    .with_status(ConversationStatus::Success)
                    .with_harness("goble")
                    .with_ui(row_ui())
                    .with_drag(drag)
                    .finish(),
            ]));
        }
    }
    // The Starred demo block leads the list rows (the app pre-pends it inside
    // the workspaces list, i.e. after the header).
    rows.insert(
        0,
        block(vec![SidebarGroupRow::new("Starred", true)
            .with_leading_icon(SIDEBAR_ICON_PIN_FILL)
            .finish()]),
    );

    let mut list = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(SIDEBAR_GROUP_GAP);
    for row in rows {
        list = list.with_child(row);
    }
    let list = Padding::new(
        list.finish(),
        EdgeInsets::new(0.0, 0.0, 0.0, SIDEBAR_LIST_BOTTOM_PAD),
    )
    .finish();

    // The conversation view: search, the new-conversation row, the Workspaces
    // header, then the list — which takes the remaining height inside the
    // sidebar column.
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let mut view = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    view = view.with_child(search(app));
    view = view.with_child(
        Padding::new(
            new_conversation(scene == Scene::ActionHover),
            EdgeInsets::new(0.0, sm, 0.0, 0.0),
        )
        .finish(),
    );
    view = view.with_child(
        Padding::new(
            header(scene == Scene::ViewOptions),
            EdgeInsets::new(0.0, SIDEBAR_SECTION_TOP_INSET, 0.0, 0.0),
        )
        .finish(),
    );
    view = view.with_child(Expanded::new(list).finish());

    // The sidebar root: the toolbelt, then the view, over the Surface fill the
    // shell gives the column. `MainAxisSize::Max` makes that fill take the whole
    // canvas height, so a short list leaves sidebar surface below it — never the
    // page background.
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    column = column.with_child(toolbelt());
    column = column.with_child(Expanded::new(view.finish()).finish());
    let panel = Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::new(sm, xs, sm, xs))
        .finish();
    // The column is the sidebar's own 300pt width inside the (sometimes wider)
    // preview canvas, so right-hand overlays like the hover card stay on-canvas.
    let panel = ConstrainedBox::new(panel).with_width(SIDEBAR_W).finish();
    HoverLayer::new(panel).finish()
}

/// A child whose open popup must paint above the rest of the scene.
struct PreviewDeferredOverlay {
    inner: Rc<RefCell<Box<dyn Element>>>,
    open: Rc<RefCell<bool>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PreviewDeferredOverlay {
    fn new(child: Box<dyn Element>, open: Rc<RefCell<bool>>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(child)),
            open,
            size: None,
            origin: None,
        }
    }
    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for PreviewDeferredOverlay {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.borrow_mut().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.borrow_mut().paint(origin, ctx, app);
        if *self.open.borrow() {
            app.hover_chips
                .borrow_mut()
                .push_shared(origin, Rc::clone(&self.inner));
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
        event: &goble_ui::event::DispatchedEvent,
        ctx: &mut goble_ui::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.inner.borrow_mut().dispatch_event(event, ctx, app)
    }
}

struct HoverLayer {
    child: Box<dyn Element>,
    layer: HoverChipLayer,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl HoverLayer {
    fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            layer: HoverChipLayer::new(),
            size: None,
            origin: None,
        }
    }
    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for HoverLayer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        let _ = self.layer.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
        self.layer.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

fn main() -> anyhow::Result<()> {
    let assets =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/goble-ui/assets");
    std::fs::create_dir_all(OUT_DIR)?;
    // (name, scene, logical width, logical height). The hover card paints to the
    // right of the 300pt column, and the view-options panel needs the probe's
    // wider frame, so those two scenes get their own canvases. `rest-tall` is a
    // window much taller than the list: the Surface fill must reach its bottom.
    let scenes = [
        ("rest", Scene::Rest, SIDEBAR_W, SIDEBAR_H),
        ("rest-tall", Scene::Tall, SIDEBAR_W, TALL_H),
        (
            "new-conversation-hover",
            Scene::ActionHover,
            SIDEBAR_W,
            SIDEBAR_H,
        ),
        (
            "selected-running",
            Scene::SelectedRunning,
            SIDEBAR_W,
            SIDEBAR_H,
        ),
        ("hover", Scene::Hover, SIDEBAR_W, SIDEBAR_H),
        ("hover-card", Scene::HoverCard, 560.0, SIDEBAR_H),
        (
            "workspace-hover",
            Scene::WorkspaceHover,
            SIDEBAR_W,
            SIDEBAR_H,
        ),
        ("workspace-menu", Scene::WorkspaceMenu, SIDEBAR_W, SIDEBAR_H),
        ("menu", Scene::Menu, SIDEBAR_W, SIDEBAR_H),
        ("archived", Scene::Archived, 560.0, SIDEBAR_H),
        ("view-options", Scene::ViewOptions, 680.0, 840.0),
        ("drag", Scene::Drag, SIDEBAR_W, SIDEBAR_H),
    ];
    for (name, scene_kind, width, height) in scenes {
        for (theme_name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            let app = AppContext {
                theme,
                ..AppContext::default()
            };
            let element = scene(&app, scene_kind);
            let out =
                std::path::Path::new(OUT_DIR).join(format!("sidebar-{name}-{theme_name}.png"));
            raster::render_scene_to_png(&assets, &app, element, width, height, 2.0, &out)?;
        }
    }
    Ok(())
}
