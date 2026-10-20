//! Saved SSH hosts: the Cmd+Shift+N picker and the reconnect offer.
//!
//! The picker lists the entries from `~/.goble/hosts.toml`. A pick opens a
//! terminal pane running that host's `ssh -t` line; the pane keeps the entry's
//! name, so the same list can offer a reconnect after the session ends.
//! Arrow keys move the selection, Enter opens it, Escape and a backdrop click
//! close the picker without opening anything.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, EventContext, Expanded, Fill, Flex, LayoutContext, MainAxisSize, PaintContext, Point,
    Scrollable, SizeConstraint, Spacer, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::Dialog;

use super::actions::UiActions;
use super::snapshot::UiSnapshot;

/// Width of the saved-host picker.
pub const HOST_PICKER_WIDTH: f32 = 520.0;

/// Build the Cmd+Shift+N saved-host picker. When it is closed the dialog
/// occupies no space and ignores events.
pub fn build_host_picker(
    _app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let open = state.ssh_host_picker_open;
    let content = HostPickerContent {
        open,
        hosts: state.ssh_host_entries.clone(),
        index: state.ssh_host_picker_index,
        on_move: actions.on_ssh_host_move.clone(),
        on_pick: actions.on_ssh_host_pick.clone(),
        on_close: actions.on_close_ssh_host_picker.clone(),
        root: None,
        size: None,
        origin: None,
    };
    let on_close = actions.on_close_ssh_host_picker.clone();
    Dialog::new(Box::new(content))
        .with_open(open)
        .with_width(HOST_PICKER_WIDTH)
        .with_on_close(move || (on_close.borrow_mut())())
        .finish()
}

struct HostPickerContent {
    open: bool,
    hosts: Vec<goble_core::hosts::HostEntry>,
    index: usize,
    on_move: Rc<RefCell<dyn FnMut(i32)>>,
    on_pick: Rc<RefCell<dyn FnMut(usize)>>,
    on_close: Rc<RefCell<dyn FnMut()>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl HostPickerContent {
    fn selected(&self) -> usize {
        self.hosts.len().saturating_sub(1).min(self.index)
    }

    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let selected = self.selected();

        let header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(
                Text::new("Saved SSH hosts")
                    .with_font_size(13.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(Spacer::new().finish())
            .with_child(
                Text::new("~/.goble/hosts.toml")
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish();

        let mut list = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(4.0);
        if self.hosts.is_empty() {
            list = list.with_child(
                Container::new(
                    Text::new("No saved hosts yet — add a [[hosts]] table to hosts.toml")
                        .with_font_size(12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(md))
                .finish(),
            );
        } else {
            for (pos, host) in self.hosts.iter().enumerate() {
                let text = format!("{}\t{}", host.name, host.target());
                let variant = if pos == selected {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                };
                let on_pick = self.on_pick.clone();
                let button = Button::new(
                    Text::new(text)
                        .with_font_size(12.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_variant(variant)
                .with_on_click(move || (on_pick.borrow_mut())(pos))
                .finish();
                list = list.with_child(button);
            }
        }

        let column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm)
            .with_child(header)
            .with_child(Divider::horizontal().finish())
            .with_child(
                Expanded::new(Scrollable::new(list.finish(), Axis::Vertical).finish()).finish(),
            )
            .with_child(
                Text::new("Enter opens the host · Esc closes")
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        let panel = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_padding(EdgeInsets::uniform(md))
            .with_corner_radius(app.theme.radius_px());
        self.root = Some(panel.finish());
    }
}

impl Element for HostPickerContent {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = if self.open {
            self.rebuild(app);
            self.root
                .as_mut()
                .expect("the picker rebuilt its panel")
                .layout(constraint, ctx, app)
        } else {
            Vector2F::zero()
        };
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if !self.open {
            return;
        }
        if let Some(root) = &mut self.root {
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
        if !self.open {
            return false;
        }
        match event {
            DispatchedEvent::KeyDown { key, .. } => {
                match key.as_str() {
                    "ArrowDown" => (self.on_move.borrow_mut())(1),
                    "ArrowUp" => (self.on_move.borrow_mut())(-1),
                    "Enter" => {
                        if !self.hosts.is_empty() {
                            (self.on_pick.borrow_mut())(self.selected());
                        }
                    }
                    "Escape" => (self.on_close.borrow_mut())(),
                    _ => {}
                }
                // The picker is modal while it is up: its keys never reach a
                // pane behind it.
                true
            }
            _ => self
                .root
                .as_mut()
                .map(|root| root.dispatch_event(event, ctx, app))
                .unwrap_or(false),
        }
    }
}

/// The reconnect offer a saved-host pane shows when its session ends. Opening
/// it again restarts the pane's shell and runs the saved host's line; the
/// dismiss button (and Escape) leaves the pane on its plain local shell.
pub fn build_reconnect_prompt(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let prompt = state.ssh_reconnect_prompt.clone();
    let open = prompt.is_some();
    let host = prompt
        .as_ref()
        .map(|(_, host)| host.clone())
        .unwrap_or_default();

    let md = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let reconnect = actions.on_ssh_reconnect.clone();
    let dismiss = actions.on_ssh_reconnect_dismiss.clone();
    let dismiss_close = actions.on_ssh_reconnect_dismiss.clone();
    let dismiss_key = actions.on_ssh_reconnect_dismiss.clone();

    let content = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm)
        .with_child(
            Text::new("SSH session ended")
                .with_font_size(13.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new(format!("The connection to {host} ended. Reconnect this pane?"))
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(Spacer::new().finish())
                .with_child(
                    Button::new(Text::new("Dismiss").finish())
                        .with_variant(ButtonVariant::Ghost)
                        .with_on_click(move || (dismiss.borrow_mut())())
                        .finish(),
                )
                .with_child(
                    Button::new(Text::new("Reconnect").finish())
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || (reconnect.borrow_mut())())
                        .finish(),
                )
                .finish(),
        );
    let panel = Container::new(content.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_padding(EdgeInsets::uniform(md))
        .with_corner_radius(app.theme.radius_px());

    Dialog::new(Box::new(EscapeToClose {
        child: panel.finish(),
        on_close: dismiss_key,
    }))
    .with_open(open)
    .with_width(HOST_PICKER_WIDTH)
    .with_on_close(move || (dismiss_close.borrow_mut())())
    .finish()
}

/// A prompt whose `Escape` key dismisses it. The dialog closes on a backdrop
/// click; its panel content owns the key, so the prompt wires both paths to the
/// same handler.
struct EscapeToClose {
    child: Box<dyn Element>,
    on_close: Rc<RefCell<dyn FnMut()>>,
}

impl Element for EscapeToClose {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.child.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.child.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.child.size()
    }

    fn origin(&self) -> Option<Point> {
        self.child.origin()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if let DispatchedEvent::KeyDown { key, .. } = event {
            if key == "Escape" {
                (self.on_close.borrow_mut())();
                return true;
            }
        }
        self.child.dispatch_event(event, ctx, app)
    }
}
