//! A compact HSV color picker used by the Settings→Appearance theme editor.
//!
//! Renders a saturation/value square (the "color field") above a hue bar, so a
//! user can drag to pick any color. The renderer cannot draw a conic-gradient
//! wheel, so hue is a horizontal rainbow bar and the color field is a 2D
//! saturation×value square — the standard HSV picker layout.
//!
//! The element is rebuilt every frame, so the dragged-region state lives in app
//! state (a shared `Rc<RefCell<Option<ColorPickerDrag>>>`) rather than in the
//! element. It reads the latest color back from `self.color` (fed from state)
//! and reports changes through `on_change`.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::color::ColorU;
use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::{hsv_to_rgb, rgb_to_hsv};

/// Which of the three theme colors a picker edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorTarget {
    Primary,
    Secondary,
    Accent,
}

/// The region of the picker being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorRegion {
    /// The saturation/value square.
    Sv,
    /// The hue bar.
    Hue,
}

/// A drag in progress (which picker + which region). Shared via an `Rc` so the
/// per-frame rebuild does not lose the drag state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorPickerDrag {
    pub target: ColorTarget,
    pub region: ColorRegion,
}

const SV_HEIGHT: f32 = 110.0;
const BAR_HEIGHT: f32 = 16.0;
const BAR_GAP: f32 = 10.0;
const ROW_COUNT: usize = 32;
const HUE_SEGMENTS: usize = 24;
/// The knob size (a small square) drawn at the current selection.
const KNOB: f32 = 8.0;

pub struct ColorPicker {
    color: ColorU,
    target: ColorTarget,
    on_change: Option<Rc<RefCell<dyn FnMut(ColorU)>>>,
    drag: Rc<RefCell<Option<ColorPickerDrag>>>,
    width: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
    sv_rect: Option<RectF>,
    hue_rect: Option<RectF>,
}

impl ColorPicker {
    pub fn new(color: ColorU, target: ColorTarget) -> Self {
        Self {
            color,
            target,
            on_change: None,
            drag: Rc::new(RefCell::new(None)),
            width: 220.0,
            size: None,
            origin: None,
            sv_rect: None,
            hue_rect: None,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width.max(0.0);
        self
    }

    pub fn with_drag(mut self, drag: Rc<RefCell<Option<ColorPickerDrag>>>) -> Self {
        self.drag = drag;
        self
    }

    pub fn with_on_change<F: FnMut(ColorU) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn hsv(&self) -> (f32, f32, f32) {
        let color = self.color;
        let rgb = ColorU::new(color.r, color.g, color.b, 255);
        let (h, s, v) = rgb_to_hsv(rgb);
        (h, s, v)
    }

    /// Absolute rect of the saturation/value square.
    fn abs_sv(&self) -> Option<RectF> {
        let o = self.origin?;
        let r = self.sv_rect?;
        Some(rectf(o.x() + r.min_x(), o.y() + r.min_y(), r.width(), r.height()))
    }

    /// Absolute rect of the hue bar.
    fn abs_hue(&self) -> Option<RectF> {
        let o = self.origin?;
        let r = self.hue_rect?;
        Some(rectf(o.x() + r.min_x(), o.y() + r.min_y(), r.width(), r.height()))
    }

    /// Compute the color for a pointer position in the SV square.
    fn color_at_sv(&self, r: RectF, pos: Vector2F) -> ColorU {
        let (h, _, _) = self.hsv();
        let s = ((pos.x - r.min_x()) / r.width()).clamp(0.0, 1.0);
        let v = (1.0 - (pos.y - r.min_y()) / r.height()).clamp(0.0, 1.0);
        hsv_to_rgb(h, s, v)
    }

    /// Compute the color for a pointer position on the hue bar.
    fn color_at_hue(&self, r: RectF, pos: Vector2F) -> ColorU {
        let (_, s, v) = self.hsv();
        let h = ((pos.x - r.min_x()) / r.width()).clamp(0.0, 1.0) * 360.0;
        hsv_to_rgb(h, s, v)
    }
}

impl Element for ColorPicker {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let _ = app;
        let w = self.width.min(constraint.max.x).max(0.0);
        self.sv_rect = Some(rectf(0.0, 0.0, w, SV_HEIGHT));
        self.hue_rect = Some(rectf(0.0, SV_HEIGHT + BAR_GAP, w, BAR_HEIGHT));
        let size = vec2f(w, SV_HEIGHT + BAR_GAP + BAR_HEIGHT);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let _ = app;
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let (hue, sat, val) = self.hsv();

        // Saturation×value square: draw value down (top→bottom) by blending each
        // row from the gray at that value toward the full-brightness hue.
        if let Some(r) = self.abs_sv() {
            let row_h = SV_HEIGHT / ROW_COUNT as f32;
            for row in 0..ROW_COUNT {
                let v = 1.0 - row as f32 / ROW_COUNT as f32;
                let y = r.min_y() + row as f32 * row_h;
                let row_rect = rectf(r.min_x(), y, r.width(), row_h);
                // Left is gray(v), right is the bright hue at value v.
                let bright = hsv_to_rgb(hue, 1.0, v);
                let gray = ColorU::new(
                    (v * 255.0).round() as u8,
                    (v * 255.0).round() as u8,
                    (v * 255.0).round() as u8,
                    255,
                );
                renderer.fill_rect(row_rect, gray);
                renderer.fill_rect_fade_right(row_rect, bright, 0.0);
            }
            // Selection knob on the SV square.
            let kx = r.min_x() + sat * r.width() - KNOB * 0.5;
            let ky = r.min_y() + (1.0 - val) * r.height() - KNOB * 0.5;
            let knob = rectf(kx, ky, KNOB, KNOB);
            renderer.fill_rounded_rect(knob, ColorU::new(255, 255, 255, 255), 2.0);
            renderer.fill_rounded_rect(
                rectf(kx + 1.5, ky + 1.5, KNOB - 3.0, KNOB - 3.0),
                hsv_to_rgb(hue, sat, val),
                1.0,
            );
        }

        // Hue bar: a horizontal rainbow of solid segments.
        if let Some(r) = self.abs_hue() {
            let seg_w = r.width() / HUE_SEGMENTS as f32;
            for i in 0..HUE_SEGMENTS {
                let seg = rectf(
                    r.min_x() + i as f32 * seg_w,
                    r.min_y(),
                    seg_w + 0.5,
                    r.height(),
                );
                renderer.fill_rect(seg, hsv_to_rgb(i as f32 / HUE_SEGMENTS as f32 * 360.0, 1.0, 1.0));
            }
            // Hue knob.
            let kx = r.min_x() + (hue / 360.0) * r.width() - KNOB * 0.5;
            let ky = r.min_y() + r.height() * 0.5 - KNOB * 0.5;
            renderer.fill_rounded_rect(rectf(kx, ky, KNOB, KNOB), ColorU::new(255, 255, 255, 255), 2.0);
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
        let dragging = self.drag.clone();
        let target = self.target;
        match event {
            DispatchedEvent::MouseDown { position, .. } => {
                if let Some(r) = self.abs_sv() {
                    if contains(r, *position) {
                        let color = self.color_at_sv(r, *position);
                        *dragging.borrow_mut() =
                            Some(ColorPickerDrag { target, region: ColorRegion::Sv });
                        if let Some(cb) = &self.on_change {
                            (cb.borrow_mut())(color);
                        }
                        return true;
                    }
                }
                if let Some(r) = self.abs_hue() {
                    if contains(r, *position) {
                        let color = self.color_at_hue(r, *position);
                        *dragging.borrow_mut() =
                            Some(ColorPickerDrag { target, region: ColorRegion::Hue });
                        if let Some(cb) = &self.on_change {
                            (cb.borrow_mut())(color);
                        }
                        return true;
                    }
                }
                false
            }
            DispatchedEvent::MouseMove { position } => {
                let active = dragging.borrow().clone();
                let is_sv =
                    active == Some(ColorPickerDrag { target, region: ColorRegion::Sv });
                let is_hue =
                    active == Some(ColorPickerDrag { target, region: ColorRegion::Hue });
                if is_sv {
                    if let Some(r) = self.abs_sv() {
                        let color = self.color_at_sv(r, *position);
                        if let Some(cb) = &self.on_change {
                            (cb.borrow_mut())(color);
                        }
                        return true;
                    }
                } else if is_hue {
                    if let Some(r) = self.abs_hue() {
                        let color = self.color_at_hue(r, *position);
                        if let Some(cb) = &self.on_change {
                            (cb.borrow_mut())(color);
                        }
                        return true;
                    }
                }
                false
            }
            DispatchedEvent::MouseUp { .. } => {
                let active = dragging.borrow().clone();
                if active == Some(ColorPickerDrag { target, region: ColorRegion::Sv })
                    || active == Some(ColorPickerDrag { target, region: ColorRegion::Hue })
                {
                    *dragging.borrow_mut() = None;
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}
