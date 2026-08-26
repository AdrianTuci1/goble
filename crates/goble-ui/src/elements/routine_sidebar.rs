use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, EventContext,
    Fill, Flex, Icon, Label, LabelSize, LayoutContext, MainAxisAlignment, PaintContext, Point,
    RoutineCardUi, RoutineListItem, Scrollable, SizeConstraint, Spacer, Text, TopbarButton,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub const ROUTINE_SIDEBAR_WIDTH: f32 = 260.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RoutineTrigger {
    #[default]
    Manual,
    Cron,
}

impl RoutineTrigger {
    pub fn label(&self) -> &'static str {
        match self {
            RoutineTrigger::Manual => "Manual",
            RoutineTrigger::Cron => "Cron",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RoutineStatus {
    #[default]
    Idle,
    Running,
    Success,
    Error,
    Stopped,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoutineEntry {
    pub id: String,
    pub name: String,
    pub trigger: RoutineTrigger,
    pub enabled: bool,
    pub status: RoutineStatus,
}

impl RoutineEntry {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            trigger: RoutineTrigger::Manual,
            enabled: true,
            status: RoutineStatus::Idle,
        }
    }

    pub fn with_trigger(mut self, trigger: RoutineTrigger) -> Self {
        self.trigger = trigger;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_status(mut self, status: RoutineStatus) -> Self {
        self.status = status;
        self
    }
}

pub struct RoutineSidebar {
    routines: Vec<RoutineEntry>,
    selected_id: Option<String>,
    on_create: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_toggle_enabled: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl RoutineSidebar {
    pub fn new(routines: Vec<RoutineEntry>) -> Self {
        Self {
            routines,
            selected_id: None,
            on_create: None,
            on_select: None,
            on_delete: None,
            on_toggle_enabled: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected(mut self, id: Option<String>) -> Self {
        self.selected_id = id;
        self
    }

    pub fn with_on_create<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_create = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_delete<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_delete = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_toggle_enabled<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_enabled = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let _sm = app.theme.spacing_px(SpacingToken::Sm);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        // Header: title + new routine button.
        let title = Label::new(format!("Routines ({})", self.routines.len()))
            .with_size(LabelSize::Sm)
            .with_theme_color(ColorToken::Text, app)
            .finish();

        let on_create = self.on_create.clone();
        let create_button = TopbarButton::new(
            Icon::new("plus")
                .with_size(18.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_size(32.0)
        .with_on_click(move || {
            if let Some(cb) = on_create.as_ref() {
                (cb.borrow_mut())();
            }
        })
        .finish();

        let header = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(title)
            .with_child(create_button)
            .finish();
        column = column.with_child(header);

        // Routine list.
        if self.routines.is_empty() {
            column = column.with_child(
                Container::new(
                    Text::new("No routines yet.\nAsk the agent to create one.")
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(12.0)
                        .with_line_height(1.4)
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
            );
        } else {
            let mut list = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(2.0);
            for entry in &self.routines {
                let selected = self
                    .selected_id
                    .as_ref()
                    .map(|id| id == &entry.id)
                    .unwrap_or(false);
                let select_id = entry.id.clone();
                let delete_id = entry.id.clone();
                let toggle_id = entry.id.clone();
                let on_select = self.on_select.clone();
                let on_delete = self.on_delete.clone();
                let on_toggle = self.on_toggle_enabled.clone();
                let ui = Rc::new(RefCell::new(RoutineCardUi::default()));
                let item = RoutineListItem::new(
                    entry.id.clone(),
                    entry.name.clone(),
                    entry.trigger,
                    entry.enabled,
                    entry.status,
                    ui,
                    selected,
                )
                .with_on_click(move || {
                    if let Some(cb) = on_select.as_ref() {
                        (cb.borrow_mut())(select_id.clone());
                    }
                })
                .with_on_delete(move || {
                    if let Some(cb) = on_delete.as_ref() {
                        (cb.borrow_mut())(delete_id.clone());
                    }
                })
                .with_on_toggle_enabled(move || {
                    if let Some(cb) = on_toggle.as_ref() {
                        (cb.borrow_mut())(toggle_id.clone());
                    }
                })
                .finish();
                list = list.with_child(item);
            }
            column = column.with_child(
                Scrollable::new(list.finish(), crate::elements::Axis::Vertical).finish(),
            );
        }

        column = column.with_child(Spacer::new().finish());
        column = column.with_child(Divider::horizontal().finish());

        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
    }
}

impl Element for RoutineSidebar {
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn routine_sidebar_layouts() {
        let app = AppContext::default();
        let routines = vec![
            RoutineEntry::new("r1", "Morning summary"),
            RoutineEntry::new("r2", "Code review").with_trigger(RoutineTrigger::Cron),
        ];
        let mut sidebar = RoutineSidebar::new(routines).with_selected(Some("r1".to_string()));
        let size = sidebar.layout(
            SizeConstraint::loose(vec2f(ROUTINE_SIDEBAR_WIDTH, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
