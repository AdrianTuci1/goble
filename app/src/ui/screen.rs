//! Screen panel: live broadcast + computer-use controls and the capturable
//! source list. Read-only state driven from [`crate::screen::ScreenState`].

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment,
    Divider, EdgeInsets, Element, EventContext, Fill, Flex, FrameView, Icon, Label, LabelSize,
    LayoutContext, MainAxisSize, PaintContext, Point, Scrollable, SizeConstraint, Spacer, Switch,
    Text, TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};

use super::{ScreenActions, ScreenSnapshot};

/// Right-anchored screen sheet: broadcast + computer-use toggles and the list
/// of capturable sources.
pub fn build_screen_sheet(
    app: &AppContext,
    screen: &ScreenSnapshot,
    screen_actions: &ScreenActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_close = screen_actions.on_close.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let on_refresh = screen_actions.on_refresh_sources.clone();
    let refresh_button = TopbarButton::new(
        Icon::new("refresh")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_on_click(move || (on_refresh.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Screen")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(refresh_button)
        .with_child(close_button)
        .finish();

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(build_toggle_row(app, screen, screen_actions));
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(build_recording_section(app, screen, screen_actions));
    column = column.with_child(build_sources(app, screen, screen_actions));
    if screen.frame.is_some() {
        column = column.with_child(build_frame(app, screen, screen_actions));
    }
    column = column.with_child(build_status(app, screen));

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// Broadcast + computer-use switches.
fn build_toggle_row(
    app: &AppContext,
    screen: &ScreenSnapshot,
    screen_actions: &ScreenActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);

    let on_broadcast = screen_actions.on_toggle_broadcast.clone();
    let broadcast_switch = Switch::new()
        .with_checked(screen.broadcast)
        .with_on_change(move |enabled| (on_broadcast.borrow_mut())(enabled))
        .finish();

    let on_compute = screen_actions.on_toggle_computer_use.clone();
    let compute_switch = Switch::new()
        .with_checked(screen.computer_use)
        .with_on_change(move |enabled| (on_compute.borrow_mut())(enabled))
        .finish();

    let broadcast_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(
            Text::new("Live broadcast")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(broadcast_switch)
        .finish();

    let compute_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(
            Text::new("Computer use")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(compute_switch)
        .finish();

    Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing)
        .with_child(broadcast_row)
        .with_child(compute_row)
        .finish()
}

/// Screen-event record/replay controls: record start/stop, replay once/loop,
/// stop replay, clear, plus the count + status.
fn build_recording_section(
    app: &AppContext,
    screen: &ScreenSnapshot,
    screen_actions: &ScreenActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);

    let on_record_start = screen_actions.on_record_start.clone();
    let on_record_stop = screen_actions.on_record_stop.clone();
    let on_replay_once = screen_actions.on_replay_once.clone();
    let on_replay_loop = screen_actions.on_replay_loop.clone();
    let on_replay_stop = screen_actions.on_replay_stop.clone();
    let on_clear = screen_actions.on_clear_recording.clone();

    let record_action = if screen.recording {
        on_record_stop
    } else {
        on_record_start
    };
    let record_button = Button::new(
        Text::new(if screen.recording { "Stop" } else { "Record" }).finish(),
    )
    .with_variant(if screen.recording {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Ghost
    })
    .with_on_click(move || (record_action.borrow_mut())())
    .finish();

    let once_button = Button::new(Text::new("Replay once").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_replay_once.borrow_mut())())
        .finish();
    let loop_button = Button::new(Text::new("Loop").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_replay_loop.borrow_mut())())
        .finish();
    let stop_button = Button::new(Text::new("Stop").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_replay_stop.borrow_mut())())
        .finish();
    // "Clear" is only meaningful once something is recorded.
    let clear_button = Button::new(Text::new("Clear").finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_clear.borrow_mut())())
        .finish();

    let actions_row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing)
        .with_child(record_button)
        .with_child(once_button)
        .with_child(loop_button)
        .with_child(stop_button)
        .with_child(clear_button)
        .finish();

    let count = screen.recorded_count;
    let status = match (&screen.replay_status, screen.recording, screen.replaying) {
        (Some(s), true, _) => s.clone(),
        (Some(s), false, true) => s.clone(),
        (Some(s), false, false) => {
            if count > 0 {
                s.clone()
            } else {
                format!("recorded {count} events")
            }
        }
        (None, _, _) => {
            if count > 0 {
                format!("recorded {count} events")
            } else {
                "record input + broadcast to build a replay".to_string()
            }
        }
    };
    let status_text = Text::new(status)
        .with_font_size(11.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish();

    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(
        Label::new("Record / replay")
            .with_size(LabelSize::Xs)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    column = column.with_child(actions_row);
    column = column.with_child(status_text);

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(md))
        .finish()
}

/// The capturable/controllable source list, one selectable button per source.
fn build_sources(
    app: &AppContext,
    screen: &ScreenSnapshot,
    screen_actions: &ScreenActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let mut buttons = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm);

    for entry in &screen.sources {
        let selected = entry.source == screen.selected_source;
        let source = entry.source.clone();
        let on_select = screen_actions.on_select_source.clone();
        let variant = if selected {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Ghost
        };
        let label = Text::new(entry.source.clone())
            .with_font_size(11.0)
            .with_theme_color(ColorToken::Text, app)
            .finish();
        let button = Button::new(label)
            .with_variant(variant)
            .with_on_click(move || (on_select.borrow_mut())(source.clone()))
            .finish();
        buttons = buttons.with_child(button);
    }

    if screen.sources.is_empty() {
        buttons = buttons.with_child(
            Text::new("No capturable sources.")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    }

    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    column = column.with_child(
        Label::new("Sources")
            .with_size(LabelSize::Xs)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    column = column.with_child(Scrollable::new(buttons.finish(), Axis::Horizontal).finish());

    Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
}

/// Broadcast status line (dimensions of the last capture, or an error).
fn build_status(app: &AppContext, screen: &ScreenSnapshot) -> Box<dyn Element> {
    let status = match &screen.last_capture {
        Some(capture) => capture.clone(),
        None => format!("{} · broadcast {}", screen.selected_source, on_off(screen.broadcast)),
    };
    Text::new(status)
        .with_font_size(11.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// The held live frame, rendered through [`FrameView`] and wrapped so
/// pointer/keyboard/scroll input is routed to the source's controller when
/// computer-use is on.
fn build_frame(
    app: &AppContext,
    screen: &ScreenSnapshot,
    screen_actions: &ScreenActions,
) -> Box<dyn Element> {
    let frame = screen.frame.as_ref().expect("build_frame requires a frame");
    let view = FrameView::new(
        screen.selected_source.clone(),
        frame.frame_seq,
        frame.width,
        frame.height,
        std::sync::Arc::clone(&frame.data),
    );

    let surface = ScreenFrameSurface::new(
        view.finish(),
        screen.computer_use,
        frame.width,
        frame.height,
        screen_actions.on_screen_click.clone(),
        screen_actions.on_screen_type.clone(),
        screen_actions.on_screen_scroll.clone(),
    );

    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);
    // Cap the frame height so a 16:9 (or other) source stays a visible strip
    // in the narrow sheet instead of pushing the controls off-screen. The
    // FrameView scales to fill whatever rect it is given.
    let surface = ConstrainedBox::new(surface.finish()).with_max_height(220.0);
    Container::new(surface.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::new(md, sm, md, sm))
        .finish()
}

/// Wraps the live [`FrameView`] and, when computer-use is enabled, translates
/// pointer clicks, printable keystrokes and scrolls into controller input for
/// the selected source. Coordinates are mapped from the on-screen frame rect
/// into the source's pixel space via the known frame dimensions.
struct ScreenFrameSurface {
    inner: Box<dyn Element>,
    computer_use: bool,
    src_width: u32,
    src_height: u32,
    on_click: Rc<RefCell<dyn FnMut(u32, u32)>>,
    on_type: Rc<RefCell<dyn FnMut(String)>>,
    on_scroll: Rc<RefCell<dyn FnMut(i32, i32)>>,
    hovered: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ScreenFrameSurface {
    fn new(
        inner: Box<dyn Element>,
        computer_use: bool,
        src_width: u32,
        src_height: u32,
        on_click: Rc<RefCell<dyn FnMut(u32, u32)>>,
        on_type: Rc<RefCell<dyn FnMut(String)>>,
        on_scroll: Rc<RefCell<dyn FnMut(i32, i32)>>,
    ) -> Self {
        Self {
            inner,
            computer_use,
            src_width,
            src_height,
            on_click,
            on_type,
            on_scroll,
            hovered: false,
            size: None,
            origin: None,
        }
    }

    fn map_pixel(&self, position: Vector2F) -> (u32, u32) {
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return (0, 0),
        };
        let width = bounds.width().max(1.0);
        let height = bounds.height().max(1.0);
        let x = ((position.x - bounds.min_x()) / width * self.src_width as f32)
            .round()
            .clamp(0.0, self.src_width as f32) as u32;
        let y = ((position.y - bounds.min_y()) / height * self.src_height as f32)
            .round()
            .clamp(0.0, self.src_height as f32) as u32;
        (x, y)
    }
}

impl Element for ScreenFrameSurface {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.paint(origin, ctx, app);
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
        let inner_handled = self.inner.dispatch_event(event, ctx, app);
        match event {
            DispatchedEvent::MouseMove { position } => {
                self.hovered = self.bounds().map(|b| contains(b, *position)).unwrap_or(false);
                inner_handled
            }
            DispatchedEvent::MouseDown { position, .. } => {
                if self.computer_use
                    && self.bounds().map(|b| contains(b, *position)).unwrap_or(false)
                {
                    let (x, y) = self.map_pixel(*position);
                    (self.on_click.borrow_mut())(x, y);
                    return true;
                }
                inner_handled
            }
            DispatchedEvent::Scroll { delta } => {
                if self.computer_use && self.hovered {
                    (self.on_scroll.borrow_mut())(delta.x as i32, delta.y as i32);
                    return true;
                }
                inner_handled
            }
            DispatchedEvent::KeyDown { key, .. } => {
                // Only forward printable single-character keys, and only while
                // the pointer is over the frame, so the frame never steals
                // shortcuts from the rest of the sheet.
                if self.computer_use && self.hovered && key.chars().count() == 1 {
                    (self.on_type.borrow_mut())(key.clone());
                    return true;
                }
                inner_handled
            }
            _ => inner_handled,
        }
    }
}
