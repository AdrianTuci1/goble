//! App-level space tab bar: one tab per "space" (pane tree) in the general
//! topbar area.
//!
//! Tabs are repositionable by dragging and selectable by clicking. Like the
//! rest of the tree, this element is rebuilt every frame, so hover/press/drag
//! state lives in app state (the `space_hover` / `space_press` / `space_drag`
//! fields on [`crate::ui::UiSnapshot`]) rather than in element-owned fields,
//! which reset on each rebuild. The element only reads that state and reports
//! pointer events back through the callbacks, which mutate it.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Fill, Flex, Icon,
    LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

/// Gap between adjacent space tabs, in logical points.
const TAB_SPACING: f32 = 4.0;

pub struct SpaceBar {
    names: Vec<String>,
    active: usize,
    hover: Option<usize>,
    press: Option<usize>,
    drag: Option<usize>,
    on_select: Option<Rc<RefCell<dyn FnMut(usize)>>>,
    on_space_press: Option<Rc<RefCell<dyn FnMut(usize)>>>,
    on_space_hover: Option<Rc<RefCell<dyn FnMut(Option<usize>)>>>,
    on_space_reorder: Option<Rc<RefCell<dyn FnMut(usize, usize)>>>,
    on_space_release: Option<Rc<RefCell<dyn FnMut()>>>,
    on_add_space: Option<Rc<RefCell<dyn FnMut()>>>,
    /// Close a tab by index (reads the trailing "x" on each tab).
    on_close_space: Option<Rc<RefCell<dyn FnMut(usize)>>>,
    /// Per-tab root elements, in row order.
    tab_roots: Vec<Box<dyn Element>>,
    /// Per-tab bounds, relative to the SpaceBar origin. Filled during `layout`.
    tab_rects: Vec<RectF>,
    /// Per-tab close "x" bounds, relative to the SpaceBar origin.
    close_rects: Vec<RectF>,
    /// The trailing "+" add-space button's element and bounds (relative to the
    /// SpaceBar origin). `None` when no add callback is set.
    add_root: Option<Box<dyn Element>>,
    add_rect: Option<RectF>,
    size: Option<Vector2F>,
    origin: Option<Vector2F>,
}

impl SpaceBar {
    pub fn new(names: Vec<String>, active: usize) -> Self {
        Self {
            names,
            active,
            hover: None,
            press: None,
            drag: None,
            on_select: None,
            on_space_press: None,
            on_space_hover: None,
            on_space_reorder: None,
            on_space_release: None,
            on_add_space: None,
            on_close_space: None,
            tab_roots: Vec::new(),
            tab_rects: Vec::new(),
            close_rects: Vec::new(),
            add_root: None,
            add_rect: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_hover(mut self, hover: Option<usize>) -> Self {
        self.hover = hover;
        self
    }

    pub fn with_press(mut self, press: Option<usize>) -> Self {
        self.press = press;
        self
    }

    pub fn with_drag(mut self, drag: Option<usize>) -> Self {
        self.drag = drag;
        self
    }

    pub fn with_on_select<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_space_press<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_space_press = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_space_hover<F: FnMut(Option<usize>) + 'static>(mut self, callback: F) -> Self {
        self.on_space_hover = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_space_reorder<F: FnMut(usize, usize) + 'static>(mut self, callback: F) -> Self {
        self.on_space_reorder = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_space_release<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_space_release = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Show a trailing "+" button that adds a new space (calls `callback`).
    pub fn with_on_add_space<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_add_space = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Show a trailing "x" on each tab that closes that space (calls `callback`
    /// with the tab index). Clicking the x only closes; it never selects/opens
    /// the tab and never starts a drag.
    pub fn with_on_close_space<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_close_space = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// The tab whose center the pointer is nearest to, clamped to a real tab.
    /// Used for hover + click hit-testing. `None` when there are no tabs.
    fn tab_index_at(&self, x: f32) -> Option<usize> {
        if self.tab_rects.is_empty() {
            return None;
        }
        let mut best = 0usize;
        let mut best_d = f32::MAX;
        for (i, r) in self.tab_rects.iter().enumerate() {
            let d = (x - self.absolute_center_x(*r)).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        Some(best)
    }

    /// The insertion index for dragging tab `from` to horizontal position `x`.
    ///
    /// Counts how many non-dragged tab centers are left of the pointer; the
    /// dragged tab belongs at that position. This is monotonic in `x`, so a
    /// drag moves the tab toward the pointer without oscillating near
    /// midpoints (closest-center would swap back and forth there).
    fn drag_target_index(&self, from: usize, x: f32) -> usize {
        self.tab_rects
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != from)
            .filter(|(_, r)| x > self.absolute_center_x(**r))
            .count()
    }

    /// The element's origin in window space; zero when it has not been painted
    /// yet (so tests that dispatch without painting still work).
    fn origin_offset(&self) -> Vector2F {
        self.origin
            .map(|o| vec2f(o.x, o.y))
            .unwrap_or(Vector2F::zero())
    }

    /// Convert a layout-relative rect (from `tab_rects` / `add_rect`) into
    /// absolute window coordinates for hit-testing against event positions,
    /// which are in window space.
    fn absolute_rect(&self, r: RectF) -> RectF {
        let o = self.origin_offset();
        rectf(o.x + r.min_x(), o.y + r.min_y(), r.width(), r.height())
    }

    /// The absolute X of a tab's center, for hit-testing against event X.
    fn absolute_center_x(&self, r: RectF) -> f32 {
        self.origin_offset().x + r.center().x
    }

    fn tab_bg(&self, index: usize) -> ColorToken {
        let pressed = self.press == Some(index) || self.drag == Some(index);
        if pressed || index == self.active {
            ColorToken::Surface
        } else if self.hover == Some(index) {
            ColorToken::SurfaceRaised
        } else {
            ColorToken::Bg
        }
    }
}

impl Element for SpaceBar {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.tab_roots.clear();
        self.tab_rects.clear();
        self.close_rects.clear();
        self.add_root = None;
        self.add_rect = None;

        let pad = app.theme.spacing_px(SpacingToken::Md);
        let mut x = 0.0f32;
        let mut row_height = 0.0f32;
        for (i, name) in self.names.iter().enumerate() {
            let color = if i == self.active {
                ColorToken::Accent
            } else {
                ColorToken::Muted
            };
            let bg = self.tab_bg(i);
            let mut inner = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.0)
                .with_child(Text::new(name).with_theme_color(color, app).finish());
            if self.on_close_space.is_some() {
                inner = inner.with_child(
                    Icon::new("x")
                        .with_size(11.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                );
            }
            let mut tab: Box<dyn Element> = Container::new(Box::new(inner))
                .with_padding(EdgeInsets::new(pad, pad * 0.5, pad * 0.4, pad * 0.5))
                .with_background(Fill::Solid(app.theme.color(bg)))
                .finish();
            let size = tab.layout(constraint, ctx, app);
            self.tab_rects.push(rectf(x, 0.0, size.x, size.y));
            self.tab_roots.push(tab);
            // The trailing "x" occupies the last ~18px of the tab.
            let close_w = if self.on_close_space.is_some() { 18.0 } else { 0.0 };
            self.close_rects.push(rectf(
                x + size.x - close_w,
                0.0,
                close_w,
                size.y,
            ));
            x += size.x + TAB_SPACING;
            row_height = row_height.max(size.y);
        }
        // Trailing "+" add-space button, laid out like a tab but never drag
        // reorderable. It creates a new space so the topbar can grow.
        if self.on_add_space.is_some() {
            let plus = Text::new("+").with_theme_color(ColorToken::Accent, app).finish();
            let mut add: Box<dyn Element> = Container::new(plus)
                .with_padding(EdgeInsets::new(pad, pad * 0.5, pad, pad * 0.5))
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .finish();
            let size = add.layout(constraint, ctx, app);
            // A little extra gap before the add button so it reads as separate.
            x += TAB_SPACING;
            self.add_rect = Some(rectf(x, 0.0, size.x, size.y));
            self.add_root = Some(add);
            row_height = row_height.max(size.y);
        }
        let mut width = self
            .tab_rects
            .iter()
            .map(|r| r.width())
            .sum::<f32>()
            + self.tab_rects.len().saturating_sub(1) as f32 * TAB_SPACING;
        if let Some(add_rect) = self.add_rect {
            width += add_rect.width() + TAB_SPACING;
        }
        self.size = Some(vec2f(width.max(0.0), row_height));
        self.size.unwrap()
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(origin);
        // Strip background so the gap between tabs matches the tab fill.
        if let Some(renderer) = ctx.renderer.as_mut() {
            if let Some(size) = self.size {
                renderer.fill_rounded_rect(rectf(origin.x, origin.y, size.x, size.y), app.theme.color(ColorToken::Bg), 0.0);
            }
        }
        for (i, r) in self.tab_rects.iter().enumerate() {
            self.tab_roots[i].paint(origin + vec2f(r.min_x(), r.min_y()), ctx, app);
        }
        if let (Some(add_root), Some(r)) = (&mut self.add_root, self.add_rect) {
            add_root.paint(origin + vec2f(r.min_x(), r.min_y()), ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin.map(|o| Point::from_vec2f(o, Default::default()))
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        match event {
            DispatchedEvent::MouseDown { position, .. } => {
                if let Some(r) = self.add_rect {
                    if contains(self.absolute_rect(r), *position) {
                        // The add button activates on release; consume the down
                        // so it does not start a tab drag.
                        return true;
                    }
                }
                // A press on a tab's trailing "x" only closes (on release); it
                // must not start a tab press/drag or swallow into the tab body.
                for r in &self.close_rects {
                    if r.width() > 0.0 && contains(self.absolute_rect(*r), *position) {
                        return true;
                    }
                }
                // Only a press that actually lands on a tab consumes the event.
                // `tab_index_at` returns the *nearest* tab for any x, so without
                // a bounds check the SpaceBar would swallow every MouseDown in
                // the window (tab reorder/hover only needs "nearest", but to
                // open something below the topbar the down must fall through).
                for (i, r) in self.tab_rects.iter().enumerate() {
                    if contains(self.absolute_rect(*r), *position) {
                        if let Some(cb) = &self.on_space_press {
                            (cb.borrow_mut())(i);
                        }
                        return true;
                    }
                }
                false
            }
            DispatchedEvent::MouseMove { position } => {
                // Update hover from the pointer, then advance the drag.
                let hover = self.tab_index_at(position.x);
                if let Some(cb) = &self.on_space_hover {
                    (cb.borrow_mut())(hover);
                }
                if let Some(drag_i) = self.drag {
                    let target = self.drag_target_index(drag_i, position.x);
                    if target != drag_i {
                        if let Some(cb) = &self.on_space_reorder {
                            (cb.borrow_mut())(drag_i, target);
                        }
                    }
                    return true;
                }
                if let Some(press_i) = self.press {
                    if let Some(r) = self.tab_rects.get(press_i) {
                        let ar = self.absolute_rect(*r);
                        if position.x < ar.min_x() || position.x > ar.max_x() {
                            // The pointer left the pressed tab: begin a drag and
                            // move the tab toward the pointer.
                            let target = self.drag_target_index(press_i, position.x);
                            if target != press_i {
                                if let Some(cb) = &self.on_space_reorder {
                                    (cb.borrow_mut())(press_i, target);
                                }
                            }
                            return true;
                        }
                    }
                }
                false
            }
            DispatchedEvent::MouseUp { position, .. } => {
                if let Some(r) = self.add_rect {
                    if contains(self.absolute_rect(r), *position) {
                        if let Some(cb) = &self.on_add_space {
                            (cb.borrow_mut())();
                        }
                        return true;
                    }
                }
                if self.drag.is_some() {
                    // A drag finished; do not select on drop.
                    if let Some(cb) = &self.on_space_release {
                        (cb.borrow_mut())();
                    }
                    return true;
                }
                // Release on a tab's trailing "x" closes that space, and must
                // win over selecting the tab (a fresh down on the x set no press).
                for (i, r) in self.close_rects.iter().enumerate() {
                    if r.width() > 0.0 && contains(self.absolute_rect(*r), *position) {
                        if let Some(cb) = &self.on_close_space {
                            (cb.borrow_mut())(i);
                        }
                        return true;
                    }
                }
                if let Some(press_i) = self.press {
                    if let Some(r) = self.tab_rects.get(press_i) {
                        if contains(self.absolute_rect(*r), *position) {
                            if let Some(cb) = &self.on_select {
                                (cb.borrow_mut())(press_i);
                            }
                        }
                    }
                    if let Some(cb) = &self.on_space_release {
                        (cb.borrow_mut())();
                    }
                    return true;
                }
                // No active press/drag/add hit: this release belongs to whatever
                // is beneath the bar, so let it propagate (this is what lets a
                // click that started elsewhere complete its click in the body).
                false
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_ui::event::DispatchedEvent;
    use goble_ui::geometry::vec2f;

    fn app() -> AppContext {
        AppContext::default()
    }

    /// Lay out a three-tab bar and return it plus each tab's center x.
    fn laid_out() -> (SpaceBar, [f32; 3]) {
        let app = app();
        let mut bar = SpaceBar::new(
            vec!["A".to_string(), "B".to_string(), "C".to_string()],
            0,
        );
        bar.layout(
            SizeConstraint::loose(vec2f(400.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let centers = [
            bar.tab_rects[0].center().x,
            bar.tab_rects[1].center().x,
            bar.tab_rects[2].center().x,
        ];
        (bar, centers)
    }

    #[test]
    fn mouse_down_reports_the_pressed_tab() {
        let app = app();
        let (mut bar, centers) = laid_out();
        let pressed = Rc::new(RefCell::new(None));
        let on_press = pressed.clone();
        bar.on_space_press = Some(Rc::new(RefCell::new(move |i| {
            *on_press.borrow_mut() = Some(i);
        })));
        let mut ctx = EventContext::default();
        bar.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(centers[1], 10.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(*pressed.borrow(), Some(1));
    }

    #[test]
    fn dragging_past_the_last_tab_reorders_to_the_end() {
        let app = app();
        let (mut bar, centers) = laid_out();
        // The pointer is over tab 0 and begins moving right past tab 2.
        bar.press = Some(0);
        let reorder = Rc::new(RefCell::new(None));
        let on_reorder = reorder.clone();
        bar.on_space_reorder = Some(Rc::new(RefCell::new(move |from, to| {
            *on_reorder.borrow_mut() = Some((from, to));
        })));
        let mut ctx = EventContext::default();
        bar.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(centers[2] + 30.0, 10.0),
            },
            &mut ctx,
            &app,
        );
        assert_eq!(*reorder.borrow(), Some((0, 2)));
    }

    #[test]
    fn releasing_after_a_drag_does_not_select() {
        let app = app();
        let (mut bar, _centers) = laid_out();
        bar.drag = Some(2);
        let selected = Rc::new(RefCell::new(None));
        let on_select = selected.clone();
        bar.on_select = Some(Rc::new(RefCell::new(move |i| {
            *on_select.borrow_mut() = Some(i);
        })));
        let released = Rc::new(RefCell::new(false));
        let on_release = released.clone();
        bar.on_space_release = Some(Rc::new(RefCell::new(move || {
            *on_release.borrow_mut() = true;
        })));
        let mut ctx = EventContext::default();
        bar.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(80.0, 10.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(*selected.borrow(), None, "a drop should not select");
        assert!(*released.borrow(), "release clears the drag state");
    }

    #[test]
    fn clicking_the_plus_button_adds_a_space() {
        let app = app();
        let mut bar = SpaceBar::new(vec!["A".to_string()], 0);
        let added = Rc::new(RefCell::new(false));
        let on_add = added.clone();
        bar.on_add_space = Some(Rc::new(RefCell::new(move || {
            *on_add.borrow_mut() = true;
        })));
        bar.layout(
            SizeConstraint::loose(vec2f(400.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let add_rect = bar.add_rect.expect("add button should be laid out");
        let mut ctx = EventContext::default();
        bar.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(add_rect.center().x, 10.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(*added.borrow(), "clicking + should create a new space");
    }

    #[test]
    fn clicking_a_tab_selects_it_and_clears_press() {
        let app = app();
        let (mut bar, centers) = laid_out();
        bar.press = Some(0);
        let selected = Rc::new(RefCell::new(None));
        let on_select = selected.clone();
        bar.on_select = Some(Rc::new(RefCell::new(move |i| {
            *on_select.borrow_mut() = Some(i);
        })));
        let mut ctx = EventContext::default();
        bar.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(centers[0], 10.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(*selected.borrow(), Some(0), "clicking a tab selects it");
    }

    #[test]
    fn clicking_the_close_x_closes_that_space() {
        let app = app();
        let mut bar = SpaceBar::new(vec!["A".to_string(), "B".to_string()], 0);
        let closed = Rc::new(RefCell::new(None));
        let on_close = closed.clone();
        bar.on_close_space = Some(Rc::new(RefCell::new(move |i| {
            *on_close.borrow_mut() = Some(i);
        })));
        bar.layout(
            SizeConstraint::loose(vec2f(400.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let close_rect = bar.close_rects[1];
        let mut ctx = EventContext::default();
        bar.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(close_rect.center().x, 10.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(
            *closed.borrow(),
            Some(1),
            "releasing on the x closes that space"
        );
    }

    #[test]
    fn pressing_the_close_x_does_not_select_the_tab() {
        let app = app();
        let mut bar = SpaceBar::new(vec!["A".to_string(), "B".to_string()], 0);
        let closed = Rc::new(RefCell::new(None));
        let on_close = closed.clone();
        let selected = Rc::new(RefCell::new(false));
        let on_select = selected.clone();
        bar.on_close_space = Some(Rc::new(RefCell::new(move |i| {
            *on_close.borrow_mut() = Some(i);
        })));
        bar.on_select = Some(Rc::new(RefCell::new(move |_| {
            *on_select.borrow_mut() = true;
        })));
        bar.layout(
            SizeConstraint::loose(vec2f(400.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let close_rect = bar.close_rects[0];
        let mut ctx = EventContext::default();
        // A mouse-down on the x must not begin a tab press (so release only closes).
        assert!(
            bar.dispatch_event(
                &DispatchedEvent::MouseDown {
                    position: vec2f(close_rect.center().x, 10.0),
                    button: 0,
                },
                &mut ctx,
                &app,
            ),
            "the x swallows mouse-down so the tab is not pressed"
        );
        assert_eq!(bar.press, None, "a press on the x never selects the tab");
        bar.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(close_rect.center().x, 10.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(*closed.borrow(), Some(0), "release on the x closes it");
        assert!(!*selected.borrow(), "closing via the x does not select the tab");
    }
}
