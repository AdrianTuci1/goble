//! The right side panel's tab strip and the swipe wrapper around its content.
//!
//! The strip is built from [`UiSnapshot::side_tabs`], so every chip is a real
//! control over app state: a click selects, the hover "x" closes, and the "+"
//! menu adds one of the pane surfaces. The content wrapper turns a horizontal
//! scroll gesture into a tab step while leaving vertical travel to whatever it
//! wraps.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, CrossAxisAlignment, Element, EventContext, Flex, Icon, LayoutContext,
    MainAxisSize, PaintContext, Point, SizeConstraint, Text,
};
use goble_ui::event::{DispatchedEvent, BUTTON_PRIMARY};
use goble_ui::geometry::{rectf, vec2f, PointF, RectF, Size2F, Vector2F};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::ColorToken;

use crate::state::{SideTabAction, SideTabKind};
use crate::ui::{UiActions, UiSnapshot};

/// A chip's glyph and label sizes. The strip is chrome-scale: it sits in the
/// 43 pt side section, not in the panel body.
const TAB_ICON_SIZE: f32 = 13.0;
const TAB_FONT_SIZE: f32 = 12.5;
const TAB_PADDING_X: f32 = 8.0;
const TAB_PADDING_Y: f32 = 5.0;
const TAB_GAP: f32 = 6.0;
const TAB_CORNER_RADIUS: f32 = 8.0;
/// The close control's square and the quiet gap before it. The square is laid
/// out on every chip that can close, so the chip does not change width when the
/// pointer arrives and the "x" appears.
const TAB_CLOSE_SIZE: f32 = 14.0;
const TAB_CLOSE_GAP: f32 = 4.0;
/// How far a horizontal gesture must travel before it steps a tab. One wheel
/// notch is 40 logical points (`PIXELS_PER_WHEEL_LINE`), so a single
/// horizontal notch counts as a completed swipe; a trackpad's precise deltas
/// accumulate to the same distance.
const SWIPE_THRESHOLD: f32 = 40.0;

/// The side panel's tab strip: one chip per tab, the selected one filled, in
/// strip order. The strip is a flex row so the chips keep their natural width
/// while the panel's section gives the row the remaining space.
pub fn build_tab_strip(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0);
    for (index, kind) in state.side_tabs.iter().enumerate() {
        // Overview is the panel's home surface, not a strip chip: the strip is
        // empty while it is the only surface, and every real tab (Terminal,
        // Side Question, Files, ...) is the one that gets a chip.
        if *kind == SideTabKind::Overview {
            continue;
        }
        let on_select = actions.on_side_tab.clone();
        let on_close = actions.on_side_tab.clone();
        let selected = index == state.active_side_tab;
        row = row.with_child(
            SideTabChip::new(
                app,
                *kind,
                selected,
                true,
                move || (on_select.borrow_mut())(SideTabAction::Select(index)),
                move || (on_close.borrow_mut())(SideTabAction::Close(index)),
            )
            .finish(),
        );
    }
    row.finish()
}

/// Wrap side-panel content so a horizontal scroll gesture steps the tab strip.
/// Vertical travel is delegated to the wrapped surface unchanged, which is what
/// keeps the Overview's own list scrolling inside the panel.
pub fn swipe_content(
    child: Box<dyn Element>,
    on_side_tab: Rc<RefCell<dyn FnMut(SideTabAction)>>,
    accum: Rc<RefCell<f32>>,
) -> Box<dyn Element> {
    SwipeContent {
        child,
        on_step: Rc::new(RefCell::new(move |step: i32| {
            (on_side_tab.borrow_mut())(SideTabAction::Cycle(step))
        })),
        accum,
        pointer_inside: false,
        size: None,
        origin: None,
    }
    .finish()
}

/// One tab chip. The close "x" is painted only while the pointer is over the
/// chip; it is laid out on every closeable chip so revealing it does not move
/// the label under the pointer. The whole chip selects, the trailing square
/// closes, and the "x" stays out of the way until hover.
struct SideTabChip {
    icon: Box<dyn Element>,
    label: Box<dyn Element>,
    close: Box<dyn Element>,
    closeable: bool,
    selected: bool,
    on_select: Rc<RefCell<dyn FnMut()>>,
    on_close: Rc<RefCell<dyn FnMut()>>,
    pressed: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
    icon_size: Vector2F,
    label_size: Vector2F,
    close_size: Vector2F,
}

impl SideTabChip {
    fn new(
        app: &AppContext,
        kind: SideTabKind,
        selected: bool,
        closeable: bool,
        on_select: impl FnMut() + 'static,
        on_close: impl FnMut() + 'static,
    ) -> Self {
        let token = if selected {
            ColorToken::Text
        } else {
            ColorToken::Muted
        };
        let icon = Icon::new(kind.icon())
            .with_size(TAB_ICON_SIZE)
            .with_theme_color(token, app)
            .finish();
        let label = Text::new(kind.title())
            .with_font_size(TAB_FONT_SIZE)
            .with_weight(FontWeight::Medium)
            .with_theme_color(token, app)
            .with_max_lines(1)
            .finish();
        let close = Icon::new("x")
            .with_size(TAB_CLOSE_SIZE - 2.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish();
        Self {
            icon,
            label,
            close,
            closeable,
            selected,
            on_select: Rc::new(RefCell::new(on_select)),
            on_close: Rc::new(RefCell::new(on_close)),
            pressed: false,
            size: None,
            origin: None,
            icon_size: Vector2F::zero(),
            label_size: Vector2F::zero(),
            close_size: Vector2F::zero(),
        }
    }

    /// The trailing close square for this chip, in window coordinates. It is
    /// the chip's inner trailing edge, inset by the chip padding.
    fn close_bounds(&self) -> Option<RectF> {
        if !self.closeable {
            return None;
        }
        let origin = self.origin.map(|p| vec2f(p.x(), p.y()))?;
        let size = self.size?;
        Some(rectf(
            origin.x + size.x - TAB_PADDING_X - TAB_CLOSE_SIZE,
            origin.y + (size.y - TAB_CLOSE_SIZE) / 2.0,
            TAB_CLOSE_SIZE,
            TAB_CLOSE_SIZE,
        ))
    }
}

impl Element for SideTabChip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let trailing = TAB_PADDING_X
            + if self.closeable {
                TAB_CLOSE_GAP + TAB_CLOSE_SIZE
            } else {
                0.0
            };
        let leading = TAB_PADDING_X + TAB_ICON_SIZE + TAB_GAP;
        let max_label_width = (constraint.max.x - leading - trailing).max(0.0);
        self.icon_size = self
            .icon
            .layout(SizeConstraint::tight(vec2f(TAB_ICON_SIZE, TAB_ICON_SIZE)), ctx, app);
        self.label_size = self.label.layout(
            SizeConstraint::new(
                Vector2F::zero(),
                vec2f(max_label_width, constraint.max.y.max(0.0)),
            ),
            ctx,
            app,
        );
        self.close_size = if self.closeable {
            self.close.layout(
                SizeConstraint::tight(vec2f(TAB_CLOSE_SIZE, TAB_CLOSE_SIZE)),
                ctx,
                app,
            )
        } else {
            Vector2F::zero()
        };
        let width = (leading + self.label_size.x + trailing).min(constraint.max.x.max(0.0));
        let height = self
            .label_size
            .y
            .max(TAB_ICON_SIZE)
            .max(if self.closeable {
                TAB_CLOSE_SIZE
            } else {
                0.0
            })
            + TAB_PADDING_Y * 2.0;
        let size = vec2f(width.max(constraint.min.x.min(constraint.max.x)), height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let bounds = RectF::new(
            PointF::new(origin.x, origin.y),
            Size2F::new(size.x, size.y),
        );
        let hovered = ctx.hovered(bounds);
        let background = if self.selected {
            Some(ColorToken::Selected)
        } else if hovered {
            Some(ColorToken::Hover)
        } else {
            None
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            if let Some(token) = background {
                renderer.fill_rounded_rect(bounds, app.theme.color(token), TAB_CORNER_RADIUS);
            }
        }
        let middle = origin.y + size.y / 2.0;
        self.icon.paint(
            vec2f(origin.x + TAB_PADDING_X, middle - self.icon_size.y / 2.0),
            ctx,
            app,
        );
        self.label.paint(
            vec2f(
                origin.x + TAB_PADDING_X + TAB_ICON_SIZE + TAB_GAP,
                middle - self.label_size.y / 2.0,
            ),
            ctx,
            app,
        );
        // The "x" is a hover reveal, not permanent chrome: the selected fill
        // alone says which tab is in front, and the close control appears when
        // the pointer is on the chip it would close.
        if hovered {
            if let Some(close) = self.close_bounds() {
                self.close.paint(
                    vec2f(close.min_x(), middle - self.close_size.y / 2.0),
                    ctx,
                    app,
                );
            }
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
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        let Some(bounds) = self.bounds() else {
            return false;
        };
        match event {
            DispatchedEvent::MouseDown { position, button } if *button == BUTTON_PRIMARY => {
                if contains(bounds, *position) {
                    self.pressed = true;
                    return true;
                }
                false
            }
            DispatchedEvent::MouseUp { position, button } if *button == BUTTON_PRIMARY => {
                if !self.pressed {
                    return false;
                }
                self.pressed = false;
                if !contains(bounds, *position) {
                    return false;
                }
                let close = self
                    .close_bounds()
                    .is_some_and(|close| contains(close, *position));
                if close {
                    (self.on_close.borrow_mut())();
                } else {
                    (self.on_select.borrow_mut())();
                }
                true
            }
            _ => false,
        }
    }
}

/// A transparent pass-through around side-panel content. It reads the pointer
/// at paint time (the tree is rebuilt every frame) and turns a horizontal
/// scroll into one tab step per completed [`SWIPE_THRESHOLD`] of travel.
struct SwipeContent {
    child: Box<dyn Element>,
    on_step: Rc<RefCell<dyn FnMut(i32)>>,
    accum: Rc<RefCell<f32>>,
    pointer_inside: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for SwipeContent {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let bounds = rectf(origin.x, origin.y, size.x, size.y);
        self.pointer_inside = ctx.hovered(bounds);
        self.child.paint(origin, ctx, app);
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
        if let DispatchedEvent::Scroll { delta, .. } = event {
            if self.pointer_inside {
                // A vertical gesture is the wrapped surface's own: the event
                // goes through unchanged so the Overview list, a transcript or
                // a terminal keeps its scroll. Only a horizontal-dominant
                // gesture is the tab strip's.
                if delta.x.abs() > delta.y.abs() && delta.x != 0.0 {
                    let mut accum = self.accum.borrow_mut();
                    // Reversing direction starts a new gesture instead of
                    // cancelling travel the user already made.
                    if *accum != 0.0 && accum.signum() != delta.x.signum() {
                        *accum = 0.0;
                    }
                    *accum += delta.x;
                    if accum.abs() >= SWIPE_THRESHOLD {
                        // Positive scroll pulls content right, which walks the
                        // strip back to the previous tab; negative travel is a
                        // left swipe to the next one.
                        let step = if *accum > 0.0 { -1 } else { 1 };
                        *accum = 0.0;
                        drop(accum);
                        (self.on_step.borrow_mut())(step);
                        return true;
                    }
                    return true;
                }
                // Vertical travel leaves no half-swipe behind.
                *self.accum.borrow_mut() = 0.0;
            }
        }
        self.child.dispatch_event(event, ctx, app)
    }
}

/// The icon a work item draws in the Task output tab.
fn work_icon(kind: crate::state::PaneWorkKind) -> &'static str {
    match kind {
        crate::state::PaneWorkKind::Task => "terminal-input",
        crate::state::PaneWorkKind::SubAgent => "message-plus-square",
    }
}

/// The Task output tab's list: one row per live work item of the active pane,
/// with the item's own description and its kind. The rows are the pane's real
/// work, not a static sample; an idle pane says so.
pub fn build_task_output(
    app: &AppContext,
    state: &UiSnapshot,
) -> Box<dyn Element> {
    let items = state
        .pane_live_work
        .get(&state.active_pane_id)
        .cloned()
        .unwrap_or_default();
    let mut rows = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0);
    if items.is_empty() {
        rows = rows.with_child(
            Text::new("No work in this pane")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        );
    } else {
        for item in items {
            let text = if item.description.trim().is_empty() {
                item.kind.label().to_string()
            } else {
                item.description
            };
            rows = rows.with_child(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.0)
                    .with_child(
                        Icon::new(work_icon(item.kind))
                            .with_size(13.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new(text)
                            .with_font_size(12.0)
                            .with_theme_color(ColorToken::Text, app)
                            .with_max_lines(1)
                            .finish(),
                    )
                    .finish(),
            );
        }
    }
    rows.finish()
}

/// The Files tab's honest empty state. It is shown when the active pane has no
/// file open, instead of a view of a file that is not there.
pub fn no_file_message(app: &AppContext) -> Box<dyn Element> {
    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(4.0)
        .with_child(
            Text::new("Open a file from Explorer")
                .with_font_size(12.5)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(
            Text::new("The active pane is not showing a file.")
                .with_font_size(11.5)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    use goble_ui::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
    use goble_ui::render::{RenderCommand, Renderer};

    use crate::state::{SideTabAction, UiState};

    fn state_with(tabs: &[SideTabKind], active: usize) -> UiState {
        let mut state = UiState::mock();
        state.side_tabs = tabs.to_vec();
        state.active_side_tab = active;
        state
    }

    /// The panel starts on the Overview home surface, but reopening keeps the
    /// tab that was up when it was collapsed: Overview is not re-selected and
    /// no chip is added for it.
    #[test]
    fn the_side_panel_starts_on_overview_and_reopens_on_its_last_tab() {
        let mut state = UiState::mock();
        assert_eq!(state.side_tabs, vec![SideTabKind::Overview]);
        assert_eq!(state.active_side_tab, 0);
        assert_eq!(state.active_side_tab_kind(), SideTabKind::Overview);

        state.add_side_tab(SideTabKind::Terminal);
        assert_eq!(state.active_side_tab_kind(), SideTabKind::Terminal);
        state.open_side_panel();
        assert_eq!(
            state.active_side_tab_kind(),
            SideTabKind::Terminal,
            "opening the panel keeps the tab that was showing"
        );
    }

    /// The plus menu adds one tab per kind and shows the existing one instead of
    /// stacking a duplicate.
    #[test]
    fn adding_a_tab_selects_it_once() {
        let mut state = UiState::mock();
        state.add_side_tab(SideTabKind::Agent);
        assert_eq!(state.side_tabs, vec![SideTabKind::Overview, SideTabKind::Agent]);
        assert_eq!(state.active_side_tab, 1);
        state.add_side_tab(SideTabKind::Files);
        state.add_side_tab(SideTabKind::Agent);
        assert_eq!(
            state.side_tabs,
            vec![
                SideTabKind::Overview,
                SideTabKind::Agent,
                SideTabKind::Files
            ],
            "Agent is not added twice"
        );
        assert_eq!(state.active_side_tab, 1, "the existing Agent tab is shown");
    }

    /// Closing the active tab selects its next neighbour, or the previous one
    /// when it was the last; closing the only tab brings Overview back.
    #[test]
    fn closing_tabs_selects_a_neighbour_and_never_leaves_the_panel_empty() {
        let mut state = state_with(
            &[
                SideTabKind::Overview,
                SideTabKind::Agent,
                SideTabKind::Files,
                SideTabKind::Terminal,
            ],
            2,
        );
        state.close_side_tab(2);
        assert_eq!(state.active_side_tab, 2, "the next tab takes the slot");
        assert_eq!(state.active_side_tab_kind(), SideTabKind::Terminal);

        state.close_side_tab(2);
        assert_eq!(
            state.active_side_tab_kind(),
            SideTabKind::Agent,
            "closing the last tab selects the previous neighbour"
        );

        let mut only = state_with(&[SideTabKind::Agent], 0);
        only.close_side_tab(0);
        assert_eq!(only.side_tabs, vec![SideTabKind::Overview]);
        assert_eq!(only.active_side_tab, 0);
    }

    /// A horizontal swipe steps the strip and wraps at both ends.
    #[test]
    fn cycling_tabs_wraps() {
        let mut state = state_with(
            &[SideTabKind::Overview, SideTabKind::Agent, SideTabKind::Terminal],
            0,
        );
        state.cycle_side_tab(1);
        assert_eq!(state.active_side_tab_kind(), SideTabKind::Agent);
        state.cycle_side_tab(-1);
        assert_eq!(state.active_side_tab_kind(), SideTabKind::Overview);
        state.cycle_side_tab(-1);
        assert_eq!(
            state.active_side_tab_kind(),
            SideTabKind::Terminal,
            "the step wraps backwards"
        );
        state.cycle_side_tab(1);
        assert_eq!(state.active_side_tab_kind(), SideTabKind::Overview);
    }

    fn laid_out_chip(
        app: &AppContext,
        selected: bool,
        select: Rc<Cell<usize>>,
        close: Rc<Cell<usize>>,
    ) -> SideTabChip {
        let mut chip = SideTabChip::new(
            app,
            SideTabKind::Agent,
            selected,
            true,
            move || select.set(select.get() + 1),
            move || close.set(close.get() + 1),
        );
        let size = chip.layout(
            SizeConstraint::loose(vec2f(300.0, 40.0)),
            &mut LayoutContext::default(),
            app,
        );
        assert!(size.x > TAB_CLOSE_SIZE && size.y > 0.0, "chip laid out: {size:?}");
        chip
    }

    /// Clicking a chip selects it; the trailing close square closes it instead.
    #[test]
    fn a_chip_clicks_and_its_hover_x_closes() {
        let app = AppContext::default();
        let select = Rc::new(Cell::new(0));
        let close = Rc::new(Cell::new(0));
        let mut chip = laid_out_chip(&app, true, select.clone(), close.clone());
        let size = chip.size.unwrap();
        let mut paint = PaintContext::new(Renderer::new());
        paint.cursor_inside = false;
        chip.paint(vec2f(0.0, 0.0), &mut paint, &app);
        let mut ctx = EventContext::default();

        let in_the_label = vec2f(12.0, size.y / 2.0);
        chip.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: in_the_label,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        chip.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: in_the_label,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(select.get(), 1, "the label selects the tab");
        assert_eq!(close.get(), 0);

        let on_the_x = vec2f(
            size.x - TAB_PADDING_X - TAB_CLOSE_SIZE / 2.0,
            size.y / 2.0,
        );
        chip.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: on_the_x,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        chip.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: on_the_x,
                button: BUTTON_PRIMARY,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(close.get(), 1, "the trailing x closes the tab");
        assert_eq!(select.get(), 1, "and does not also select it");
    }

    /// The close "x" is painted only while the pointer is over the chip.
    #[test]
    fn the_close_x_is_a_hover_reveal() {
        let app = AppContext::default();
        let select = Rc::new(Cell::new(0));
        let close = Rc::new(Cell::new(0));
        let mut chip = laid_out_chip(&app, false, select, close);
        let size = chip.size.unwrap();

        let mut away = PaintContext::new(Renderer::new());
        away.cursor_inside = true;
        away.cursor_position = vec2f(0.0, 400.0);
        chip.paint(vec2f(0.0, 0.0), &mut away, &app);
        assert!(
            !away
                .renderer
                .as_ref()
                .unwrap()
                .commands()
                .iter()
                .any(|command| matches!(command, RenderCommand::DrawIcon { name, .. } if *name == "x-close")),
            "no x away from the chip"
        );

        let mut over = PaintContext::new(Renderer::new());
        over.cursor_inside = true;
        over.cursor_position = vec2f(size.x / 2.0, size.y / 2.0);
        chip.paint(vec2f(0.0, 0.0), &mut over, &app);
        assert!(
            over
                .renderer
                .as_ref()
                .unwrap()
                .commands()
                .iter()
                .any(|command| matches!(command, RenderCommand::DrawIcon { name, .. } if *name == "x-close")),
            "the hovered chip reveals its x"
        );
    }

    /// A small element that records the scroll events the swipe wrapper lets
    /// through.
    struct ScrollProbe {
        deltas: Rc<RefCell<Vec<Vector2F>>>,
        size: Option<Vector2F>,
        origin: Option<Point>,
    }

    impl Element for ScrollProbe {
        fn layout(
            &mut self,
            _constraint: SizeConstraint,
            _ctx: &mut LayoutContext,
            _app: &AppContext,
        ) -> Vector2F {
            let size = vec2f(200.0, 200.0);
            self.size = Some(size);
            size
        }

        fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
            self.origin = Some(Point::from_vec2f(origin, Default::default()));
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
            _ctx: &mut EventContext,
            _app: &AppContext,
        ) -> bool {
            if let DispatchedEvent::Scroll { delta, .. } = event {
                self.deltas.borrow_mut().push(*delta);
                return true;
            }
            false
        }
    }

    fn swipe_harness() -> (
        Box<dyn Element>,
        Rc<RefCell<Vec<Vector2F>>>,
        Rc<Cell<usize>>,
        Rc<RefCell<f32>>,
    ) {
        let app = AppContext::default();
        let deltas = Rc::new(RefCell::new(Vec::new()));
        let steps = Rc::new(Cell::new(0));
        let accum = Rc::new(RefCell::new(0.0));
        let on_tab: Rc<RefCell<dyn FnMut(SideTabAction)>> = {
            let steps = steps.clone();
            Rc::new(RefCell::new(move |action| {
                if matches!(action, SideTabAction::Cycle(_)) {
                    steps.set(steps.get() + 1);
                }
            }))
        };
        let child: Box<dyn Element> = ScrollProbe {
            deltas: deltas.clone(),
            size: None,
            origin: None,
        }
        .finish();
        let wrapper = swipe_content(child, on_tab, accum.clone());
        (wrapper, deltas, steps, accum)
    }

    fn paint_inside(wrapper: &mut Box<dyn Element>, app: &AppContext) {
        let _ = wrapper.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = vec2f(50.0, 50.0);
        wrapper.paint(vec2f(0.0, 0.0), &mut ctx, app);
    }

    /// Vertical travel reaches the wrapped surface; a horizontal wheel notch
    /// steps one tab and is consumed; a precise trackpad swipe accumulates to
    /// the same threshold.
    #[test]
    fn a_swipe_steps_the_strip_and_vertical_scroll_passes_through() {
        let app = AppContext::default();
        let (mut wrapper, deltas, steps, accum) = swipe_harness();
        paint_inside(&mut wrapper, &app);
        let mut ctx = EventContext::default();

        let vertical = vec2f(0.0, -80.0);
        let consumed = wrapper.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vertical,
                precise: true,
            },
            &mut ctx,
            &app,
        );
        let _ = consumed;
        assert_eq!(*deltas.borrow(), vec![vertical]);
        assert_eq!(steps.get(), 0);

        // One wheel notch (40 pt) is a completed horizontal swipe.
        let consumed = wrapper.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(-40.0, 0.0),
                precise: false,
            },
            &mut ctx,
            &app,
        );
        assert!(consumed, "the horizontal notch is the strip's");
        assert_eq!(steps.get(), 1);
        assert_eq!(*deltas.borrow(), vec![vertical], "the child never saw it");
        assert_eq!(*accum.borrow(), 0.0, "the gesture resets after a step");

        // A trackpad's small precise deltas accumulate to the same distance.
        for _ in 0..4 {
            wrapper.dispatch_event(
                &DispatchedEvent::Scroll {
                    delta: vec2f(-10.0, 0.0),
                    precise: true,
                },
                &mut ctx,
                &app,
            );
        }
        assert_eq!(steps.get(), 2, "four 10 pt precise deltas are one swipe");
        assert_eq!(*deltas.borrow(), vec![vertical]);
    }

    /// The swipe needs the pointer over the panel: a horizontal wheel elsewhere
    /// in the window is not the side panel's.
    #[test]
    fn a_swipe_outside_the_content_is_left_alone() {
        let app = AppContext::default();
        let (mut wrapper, deltas, steps, _accum) = swipe_harness();
        let _ = wrapper.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = EventContext::default();
        wrapper.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(-100.0, 0.0),
                precise: false,
            },
            &mut ctx,
            &app,
        );
        assert_eq!(steps.get(), 0, "no pointer, no swipe");
        assert_eq!(*deltas.borrow(), vec![vec2f(-100.0, 0.0)]);
    }

    /// The clean empty state is not a fake file body.
    #[test]
    fn the_files_empty_state_is_honest() {
        let app = AppContext::default();
        let mut message = no_file_message(&app);
        let size = message.layout(
            SizeConstraint::loose(vec2f(320.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        message.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        let texts: Vec<String> = ctx
            .renderer
            .as_ref()
            .unwrap()
            .commands()
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(size.y > 0.0);
        assert!(texts.iter().any(|text| text == "Open a file from Explorer"));
    }
}
