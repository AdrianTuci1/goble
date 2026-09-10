use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Fill, Flex, LayoutContext,
    MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct Tab {
    label: String,
    selected: bool,
    root: Option<Box<dyn Element>>,
    on_click: Option<Rc<RefCell<dyn FnMut()>>>,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Tab {
    pub fn new(label: impl Into<String>, selected: bool) -> Self {
        Self {
            label: label.into(),
            selected,
            root: None,
            on_click: None,
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let color = if self.selected {
            ColorToken::Accent
        } else {
            ColorToken::Muted
        };
        let bg = if self.selected || self.state.is_active() {
            ColorToken::Surface
        } else {
            ColorToken::Bg
        };
        let label = Text::new(&self.label).with_theme_color(color, app).finish();
        let root = Container::new(label)
            .with_padding(EdgeInsets::new(spacing, spacing, spacing, spacing))
            .with_background(Fill::Solid(app.theme.color(bg)))
            .finish();
        self.root = Some(root);
    }

    fn bounds(&self) -> Option<crate::geometry::RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }
}

impl Element for Tab {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        if self.root.is_none() {
            self.rebuild(app);
        }
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
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        let Some(bounds) = self.bounds() else {
            return false;
        };
        let before_hover = self.state.hover;
        let on_click = self.on_click.clone();
        let consumed = handle_mouse_event(
            &mut self.state,
            event,
            bounds,
            ctx,
            &mut || {
                if let Some(cb) = &on_click {
                    (cb.borrow_mut())();
                }
            },
        );
        if self.state.hover != before_hover {
            // Rebuild on hover change so the highlight follows the pointer.
            self.root = None;
            if self.root.is_none() {
                self.rebuild(app);
            }
        }
        consumed
    }
}

pub struct TabBar {
    root: Option<Box<dyn Element>>,
    tabs: Vec<Tab>,
    selected_index: usize,
    on_select: Option<Rc<RefCell<dyn FnMut(usize)>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TabBar {
    pub fn new(tabs: Vec<Tab>, selected_index: usize, app: &AppContext) -> Self {
        let mut this = Self {
            root: None,
            tabs,
            selected_index,
            on_select: None,
            size: None,
            origin: None,
        };
        this.rebuild(app);
        this
    }

    pub fn with_on_select<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let on_select = self.on_select.clone();
        let mut row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing);
        for (index, tab) in self.tabs.iter().enumerate() {
            let mut t = Tab::new(tab.label.clone(), tab.selected);
            if let Some(on_select) = &on_select {
                let item = Rc::clone(on_select);
                let index = index;
                t = t.with_on_click(move || (item.borrow_mut())(index));
            }
            row = row.with_child(Box::new(t));
        }

        let root = Container::new(row.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_border(app.theme.color(ColorToken::Border).into())
            .finish();
        self.root = Some(root);
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }
}

impl Element for TabBar {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
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
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root.as_mut().unwrap().dispatch_event(event, ctx, app)
    }
}
