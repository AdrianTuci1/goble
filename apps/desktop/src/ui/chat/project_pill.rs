//! The composer's project pill: the conversation's current project above the
//! input, opening the shared project picker.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets, Element, Flex, HoverRow,
    Icon, MainAxisSize, PopupMenu, PopupMenuPosition, Text,
};
use goble_ui::elements::{EventContext, LayoutContext, PaintContext, Point, SizeConstraint};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::theme::ColorToken;

use crate::ui::project_menu::{self, ProjectMenuContext};
use crate::ui::{UiActions, UiSnapshot};

pub(crate) fn build_project_pill(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane_id: u64,
) -> Box<dyn Element> {
    let path = state
        .pane_chat
        .get(&pane_id)
        .map(|chat| chat.composer_path.clone())
        .unwrap_or_default();
    let no_project = path.trim().is_empty();
    let label = if no_project {
        "No Project".to_string()
    } else {
        std::path::Path::new(&path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| path.clone())
    };
    let pill = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(5.0)
            .with_child(
                Text::new(label)
                    .with_font_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_child(
                Icon::new("chevron-down")
                    .with_size(11.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish(),
    )
    .with_padding(EdgeInsets::new(7.0, 3.0, 7.0, 3.0))
    .finish();
    let pill = ConstrainedBox::new(pill).with_max_width(220.0).finish();
    let trigger = HoverRow::new(pill).with_quiet_hover().finish();
    let menu_actions = actions.clone();
    let menu = PopupMenu::new(
        trigger,
        project_menu::items(actions, ProjectMenuContext::Composer, no_project),
    )
    .with_open(state.project_menu_open.clone())
    .with_corner_radius(12.0)
    .with_max_width(220.0)
    .with_row_height(34.0)
    .with_icon_size(14.0)
    .with_position(PopupMenuPosition::Above)
    .with_menu_id("composer.project-pill")
    .with_on_select(move |index| {
        project_menu::on_select(&menu_actions, ProjectMenuContext::Composer, index)
    })
    .finish();
    DeferredProjectMenu::new(menu, state.project_menu_open.clone()).finish()
}

/// The pill paints inline; its open menu is also queued on the frame's last
/// layer so the composer cannot cover it.
struct DeferredProjectMenu {
    inner: Rc<RefCell<Box<dyn Element>>>,
    open: Rc<RefCell<bool>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl DeferredProjectMenu {
    fn new(inner: Box<dyn Element>, open: Rc<RefCell<bool>>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(inner)),
            open,
            size: None,
            origin: None,
        }
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for DeferredProjectMenu {
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
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.inner.borrow_mut().dispatch_event(event, ctx, app)
    }
}
