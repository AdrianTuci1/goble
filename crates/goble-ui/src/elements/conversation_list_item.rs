use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::InteractiveState;
use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Fill, Flex,
    LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{PointF, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct ConversationListItem {
    title: String,
    preview: String,
    timestamp: String,
    selected: bool,
    on_select: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_delete: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Box<dyn Element>,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ConversationListItem {
    pub fn new(
        title: impl Into<String>,
        preview: impl Into<String>,
        timestamp: impl Into<String>,
        selected: bool,
        app: &AppContext,
    ) -> Self {
        let title = title.into();
        let preview = preview.into();
        let timestamp = timestamp.into();
        let root = Self::build_root(&title, &preview, &timestamp, selected, app);
        Self {
            title,
            preview,
            timestamp,
            selected,
            on_select: None,
            on_delete: None,
            root,
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    pub fn with_on_select<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_delete<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_delete = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build_root(
        title: &str,
        preview: &str,
        timestamp: &str,
        selected: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let spacing_sm = app.theme.spacing_px(SpacingToken::Sm);
        let spacing_md = app.theme.spacing_px(SpacingToken::Md);
        let text_color = app.theme.color(ColorToken::Text);
        let muted_color = app.theme.color(ColorToken::Muted);

        let title = Text::new(title)
            .with_font_size(13.0)
            .with_color(text_color)
            .finish();
        let preview = Text::new(preview)
            .with_font_size(11.0)
            .with_color(muted_color)
            .with_max_lines(1)
            .finish();
        let timestamp = Text::new(timestamp)
            .with_font_size(10.0)
            .with_color(muted_color)
            .finish();

        let text_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(title)
            .with_child(preview)
            .with_child(timestamp)
            .finish();

        let delete_hint = Text::new("\u{00d7}")
            .with_font_size(14.0)
            .with_color(muted_color)
            .finish();

        let row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing_sm)
            .with_child(text_column)
            .with_child(delete_hint)
            .finish();

        Container::new(row)
            .with_padding(EdgeInsets::uniform(spacing_md))
            .with_background(if selected {
                Fill::Solid(app.theme.color(ColorToken::Selected))
            } else {
                Fill::None
            })
            .finish()
    }

    fn rebuild(&mut self, app: &AppContext) {
        self.root = Self::build_root(
            &self.title,
            &self.preview,
            &self.timestamp,
            self.selected,
            app,
        );
    }
}

impl Element for ConversationListItem {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let size = self.root.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.paint(origin, ctx, app);
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
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        // Let the delete button (last child) handle the event first.
        if self.root.dispatch_event(event, ctx, app) {
            return true;
        }

        let select_cb = self.on_select.clone();
        let on_select = move || {
            if let Some(cb) = select_cb.as_ref() {
                (cb.borrow_mut())();
            }
        };

        let delete_cb = self.on_delete.clone();
        let on_delete = move || {
            if let Some(cb) = delete_cb.as_ref() {
                (cb.borrow_mut())();
            }
        };

        match event {
            DispatchedEvent::MouseDown { position, .. } => {
                if bounds.contains(PointF::new(position.x, position.y)) {
                    self.state.pressed = true;
                    return true;
                }
                false
            }
            DispatchedEvent::MouseUp { position, .. } => {
                if self.state.pressed {
                    self.state.pressed = false;
                    if bounds.contains(PointF::new(position.x, position.y)) {
                        let delete_area_width = 32.0f32;
                        let near_delete = position.x >= bounds.max_x() - delete_area_width;
                        if near_delete {
                            on_delete();
                        } else {
                            on_select();
                        }
                        return true;
                    }
                }
                false
            }
            DispatchedEvent::MouseMove { position } => {
                let inside = bounds.contains(PointF::new(position.x, position.y));
                self.state.hover = inside;
                false
            }
            _ => false,
        }
    }
}
