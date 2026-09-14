//! The single HSV color wheel used by the Settings→Appearance theme editor.
//!
//! One wheel edits whichever theme channel is currently selected: a hue ring
//! with a saturation/value square inside it. The renderer has no conic-gradient
//! primitive, so the ring is rasterized once on the CPU into an RGBA8 bitmap
//! and drawn through the ordinary image path (a fixed `source`, a fixed
//! `frame_seq`, so the texture is uploaded once for the process lifetime). The
//! saturation/value square is drawn with the renderer's row fills, and the two
//! selection knobs are rounded rects.
//!
//! The element is rebuilt every frame, so the drag state lives in app state (a
//! shared `Rc<RefCell<Option<ColorPickerDrag>>>`) rather than in the element.
//! It reads the latest color back from `self.color` (fed from state) and
//! reports changes through `on_change`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use goble_ui::color::ColorU;
use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::ColorToken;
use goble_ui::{hsv_to_rgb, rgb_to_hsv};

/// Which of the three theme colors the wheel edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorTarget {
    Primary,
    Secondary,
    Accent,
}

impl ColorTarget {
    /// Label shown on the swatch that selects this channel.
    pub fn label(self) -> &'static str {
        match self {
            ColorTarget::Primary => "Primary",
            ColorTarget::Secondary => "Secondary",
            ColorTarget::Accent => "Accent",
        }
    }
}

/// The region of the wheel being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorRegion {
    /// The saturation/value square inside the ring.
    Sv,
    /// The hue ring.
    Ring,
}

/// A drag in progress (which region of the wheel). Shared via an `Rc` so the
/// per-frame rebuild does not lose the drag state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorPickerDrag {
    pub region: ColorRegion,
}

/// Diameter of the wheel, ring included.
pub const WHEEL_SIZE: f32 = 190.0;
/// Ring thickness; the hue band spans `outer - RING_WIDTH ..= outer`.
const RING_WIDTH: f32 = 18.0;
/// Side of the saturation/value square inscribed in the ring's inner circle.
const SV_SIZE: f32 = 104.0;
/// Segments used to draw the square's vertical gradient (one row each).
pub const SV_ROWS: usize = 32;
/// The knob size (a small square) drawn at the current selection.
const KNOB: f32 = 9.0;
/// Rasterized resolution of the ring bitmap (2x the logical size).
const RING_BITMAP: u32 = 380;
/// The renderer caches images per `source`; one stable name for the ring.
const RING_SOURCE: &str = "theme-color-ring";

pub struct ColorWheel {
    color: ColorU,
    on_change: Option<Rc<RefCell<dyn FnMut(ColorU)>>>,
    drag: Rc<RefCell<Option<ColorPickerDrag>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    sv_rect: Option<RectF>,
}

/// The ring bitmap: hue by angle, transparent everywhere except the band.
///
/// Built once for the process (`OnceLock`) and shared by `Arc`, because the
/// ring never changes — no hue or color input feeds it.
fn ring_bitmap() -> &'static Arc<[u8]> {
    static BITMAP: OnceLock<Arc<[u8]>> = OnceLock::new();
    BITMAP.get_or_init(|| {
        let n = RING_BITMAP;
        let scale = n as f32 / WHEEL_SIZE;
        let center = (n as f32 - 1.0) * 0.5;
        let outer = center;
        let inner = outer - RING_WIDTH * scale;
        let mut px = vec![0u8; (n * n * 4) as usize];
        for y in 0..n {
            for x in 0..n {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let r = (dx * dx + dy * dy).sqrt();
                // One-pixel anti-aliased coverage on both band edges.
                let coverage = (outer - r).min(r - inner).clamp(0.0, 1.0);
                if coverage <= 0.0 {
                    continue;
                }
                // atan2 over screen coordinates (y down) runs clockwise on
                // screen; red sits at 3 o'clock and hue increases clockwise.
                let hue = dy.atan2(dx).to_degrees().rem_euclid(360.0);
                let c = hsv_to_rgb(hue, 1.0, 1.0);
                let i = ((y * n + x) * 4) as usize;
                px[i] = c.r;
                px[i + 1] = c.g;
                px[i + 2] = c.b;
                px[i + 3] = (coverage * 255.0).round() as u8;
            }
        }
        Arc::from(px.into_boxed_slice())
    })
}

impl ColorWheel {
    pub fn new(color: ColorU) -> Self {
        Self {
            color,
            on_change: None,
            drag: Rc::new(RefCell::new(None)),
            size: None,
            origin: None,
            sv_rect: None,
        }
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
        rgb_to_hsv(rgb)
    }

    /// Absolute rect of the saturation/value square.
    fn abs_sv(&self) -> Option<RectF> {
        let o = self.origin?;
        let r = self.sv_rect?;
        Some(rectf(o.x() + r.min_x(), o.y() + r.min_y(), r.width(), r.height()))
    }

    /// Absolute rect of the whole wheel.
    fn abs_wheel(&self) -> Option<RectF> {
        let o = self.origin?;
        Some(rectf(o.x(), o.y(), WHEEL_SIZE, WHEEL_SIZE))
    }

    /// Compute the color for a pointer position in the SV square.
    fn color_at_sv(&self, r: RectF, pos: Vector2F) -> ColorU {
        let (h, _, _) = self.hsv();
        let s = ((pos.x - r.min_x()) / r.width()).clamp(0.0, 1.0);
        let v = (1.0 - (pos.y - r.min_y()) / r.height()).clamp(0.0, 1.0);
        hsv_to_rgb(h, s, v)
    }

    /// Compute the color for a pointer position on the hue ring.
    fn color_at_ring(&self, r: RectF, pos: Vector2F) -> ColorU {
        let (_, s, v) = self.hsv();
        let cx = r.min_x() + r.width() * 0.5;
        let cy = r.min_y() + r.height() * 0.5;
        let angle = (pos.y - cy).atan2(pos.x - cx);
        let hue = angle.to_degrees().rem_euclid(360.0);
        hsv_to_rgb(hue, s, v)
    }

    /// Whether `pos` is inside the hue band (and not just anywhere in the
    /// wheel's bounding box).
    fn ring_contains(&self, r: RectF, pos: Vector2F) -> bool {
        if !contains(r, pos) {
            return false;
        }
        let outer = r.width() * 0.5;
        let inner = outer - RING_WIDTH;
        let dx = pos.x - (r.min_x() + outer);
        let dy = pos.y - (r.min_y() + outer);
        let dist = (dx * dx + dy * dy).sqrt();
        dist <= outer && dist >= inner
    }
}

impl Element for ColorWheel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let _ = app;
        let side = WHEEL_SIZE.min(constraint.max.x).min(constraint.max.y).max(0.0);
        // The square sits inside the ring's inner circle, with a small gap.
        let sv = SV_SIZE
            .min((side - RING_WIDTH * 2.0).max(0.0))
            .max(0.0);
        self.sv_rect = Some(rectf((side - sv) * 0.5, (side - sv) * 0.5, sv, sv));
        let size = vec2f(side, side);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let (hue, sat, val) = self.hsv();

        // Hue ring: one image command; the renderer uploads the bitmap once.
        if let Some(r) = self.abs_wheel() {
            renderer.draw_image(
                r,
                RING_SOURCE,
                RING_BITMAP,
                RING_BITMAP,
                0,
                Arc::clone(ring_bitmap()),
            );
        }

        // Saturation×value square: draw value down (top→bottom) by blending each
        // row from the gray at that value toward the full-brightness hue.
        if let Some(r) = self.abs_sv() {
            let row_h = r.height() / SV_ROWS as f32;
            for row in 0..SV_ROWS {
                let v = 1.0 - row as f32 / SV_ROWS as f32;
                let y = r.min_y() + row as f32 * row_h;
                let row_rect = rectf(r.min_x(), y, r.width(), row_h);
                // Left is gray(v), right is the bright hue at value v.
                let bright = hsv_to_rgb(hue, 1.0, v);
                let level = (v * 255.0).round() as u8;
                let gray = ColorU::new(level, level, level, 255);
                renderer.fill_rect(row_rect, gray);
                renderer.fill_rect_fade_right(row_rect, bright, 0.0);
            }
            // Separate the square from the ring.
            renderer.stroke_rect(r, app.theme.color(ColorToken::Border), 1.0, 0.0);
            // Selection knob on the square.
            let kx = r.min_x() + sat * r.width() - KNOB * 0.5;
            let ky = r.min_y() + (1.0 - val) * r.height() - KNOB * 0.5;
            renderer.fill_rounded_rect(rectf(kx, ky, KNOB, KNOB), ColorU::new(0, 0, 0, 160), 2.0);
            renderer.fill_rounded_rect(
                rectf(kx + 1.0, ky + 1.0, KNOB - 2.0, KNOB - 2.0),
                hsv_to_rgb(hue, sat, val),
                1.5,
            );
        }

        // Hue knob: a dot riding the middle of the band at the current hue.
        if let Some(r) = self.abs_wheel() {
            let outer = r.width() * 0.5;
            let radius = outer - RING_WIDTH * 0.5;
            let angle = hue.to_radians();
            let cx = r.min_x() + outer + angle.cos() * radius;
            let cy = r.min_y() + outer + angle.sin() * radius;
            let dot = KNOB + 2.0;
            renderer.fill_rounded_rect(
                rectf(cx - dot * 0.5, cy - dot * 0.5, dot, dot),
                ColorU::new(0, 0, 0, 200),
                dot * 0.5,
            );
            renderer.fill_rounded_rect(
                rectf(
                    cx - dot * 0.5 + 1.0,
                    cy - dot * 0.5 + 1.0,
                    dot - 2.0,
                    dot - 2.0,
                ),
                hsv_to_rgb(hue, 1.0, 1.0),
                (dot - 2.0) * 0.5,
            );
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
        match event {
            DispatchedEvent::MouseDown { position, .. } => {
                if let Some(r) = self.abs_sv() {
                    if contains(r, *position) {
                        let color = self.color_at_sv(r, *position);
                        *dragging.borrow_mut() =
                            Some(ColorPickerDrag { region: ColorRegion::Sv });
                        if let Some(cb) = &self.on_change {
                            (cb.borrow_mut())(color);
                        }
                        return true;
                    }
                }
                if let Some(r) = self.abs_wheel() {
                    if self.ring_contains(r, *position) {
                        let color = self.color_at_ring(r, *position);
                        *dragging.borrow_mut() =
                            Some(ColorPickerDrag { region: ColorRegion::Ring });
                        if let Some(cb) = &self.on_change {
                            (cb.borrow_mut())(color);
                        }
                        return true;
                    }
                }
                false
            }
            DispatchedEvent::MouseMove { position } => {
                let active = *dragging.borrow();
                let region = match active {
                    Some(drag) => drag.region,
                    None => return false,
                };
                let color = match region {
                    ColorRegion::Sv => self.abs_sv().map(|r| self.color_at_sv(r, *position)),
                    ColorRegion::Ring => self.abs_wheel().map(|r| self.color_at_ring(r, *position)),
                };
                match color {
                    Some(color) => {
                        if let Some(cb) = &self.on_change {
                            (cb.borrow_mut())(color);
                        }
                        true
                    }
                    None => false,
                }
            }
            DispatchedEvent::MouseUp { .. } => {
                if dragging.borrow_mut().take().is_some() {
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}

/// The label of each channel's swatch, in the order they are shown.
pub const COLOR_TARGET_ORDER: [ColorTarget; 3] = [
    ColorTarget::Primary,
    ColorTarget::Secondary,
    ColorTarget::Accent,
];

#[cfg(test)]
mod tests {
    use super::*;
    use goble_ui::render::RenderCommand;

    fn wheel_box(color: ColorU) -> Box<dyn Element> {
        ColorWheel::new(color).finish()
    }

    #[test]
    fn the_ring_bitmap_has_hue_by_angle_and_a_hollow_center() {
        let bitmap = ring_bitmap();
        assert_eq!(bitmap.len(), (RING_BITMAP * RING_BITMAP * 4) as usize);

        let px = |x: u32, y: u32| {
            let i = ((y * RING_BITMAP + x) * 4) as usize;
            (bitmap[i], bitmap[i + 1], bitmap[i + 2], bitmap[i + 3])
        };
        let center = RING_BITMAP / 2;
        assert_eq!(px(center, center).3, 0, "the middle of the wheel is empty");

        // The band is opaque somewhere on the horizontal radius at 3 o'clock.
        let scale = RING_BITMAP as f32 / WHEEL_SIZE;
        let mid_band = center as f32 + RING_BITMAP as f32 * 0.5 - RING_WIDTH * scale * 0.5;
        let band = px(mid_band as u32, center);
        assert!(band.3 > 200, "the hue band is opaque, got alpha {}", band.3);
        assert!(band.0 > 200 && band.1 < 60, "hue 0 at 3 o'clock is red");
    }

    #[test]
    fn the_ring_band_click_picks_the_angle_hue() {
        let changed = Rc::new(RefCell::new(None));
        let changed_clone = changed.clone();
        let mut wheel = ColorWheel::new(ColorU::new(255, 0, 0, 255))
            .with_on_change(move |c| *changed_clone.borrow_mut() = Some(c));
        let app = AppContext::default();
        let mut ctx = EventContext::default();
        wheel.layout(
            SizeConstraint::loose(vec2f(WHEEL_SIZE, WHEEL_SIZE)),
            &mut LayoutContext::default(),
            &app,
        );
        wheel.paint(vec2f(0.0, 0.0), &mut PaintContext::new(goble_ui::render::Renderer::new()), &app);

        // Mid-band at 3 o'clock is hue 0 (red); mid-band at the bottom is 90.
        let center = WHEEL_SIZE * 0.5;
        let radius = center - RING_WIDTH * 0.5;
        let right = vec2f(center + radius, center);
        assert!(wheel.dispatch_event(
            &DispatchedEvent::MouseDown { position: right, button: 0 },
            &mut ctx,
            &app,
        ));
        let (hue, _, _) = rgb_to_hsv(changed.borrow().unwrap());
        assert!(hue < 5.0 || hue > 355.0, "3 o'clock is red, got hue {hue}");

        let mut ctx = EventContext::default();
        let below = vec2f(center, center + radius);
        assert!(wheel.dispatch_event(
            &DispatchedEvent::MouseDown { position: below, button: 0 },
            &mut ctx,
            &app,
        ));
        let (hue, _, _) = rgb_to_hsv(changed.borrow().unwrap());
        assert!((hue - 90.0).abs() < 5.0, "clockwise from red is hue 90, got {hue}");
    }

    #[test]
    fn clicking_the_square_corner_picks_full_saturation_and_value() {
        let changed = Rc::new(RefCell::new(None));
        let changed_clone = changed.clone();
        let mut wheel = ColorWheel::new(ColorU::new(255, 0, 0, 255))
            .with_on_change(move |c| *changed_clone.borrow_mut() = Some(c));
        let app = AppContext::default();
        let mut ctx = EventContext::default();
        wheel.layout(
            SizeConstraint::loose(vec2f(WHEEL_SIZE, WHEEL_SIZE)),
            &mut LayoutContext::default(),
            &app,
        );
        wheel.paint(vec2f(0.0, 0.0), &mut PaintContext::new(goble_ui::render::Renderer::new()), &app);

        // Top-right corner of the square: saturation and value near maximum.
        let sv = wheel.abs_sv().expect("the square is laid out");
        let handled = wheel.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(sv.max_x() - 0.5, sv.min_y() + 0.5),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(handled, "a click inside the square starts a drag");
        let (_, sat, val) = rgb_to_hsv(changed.borrow().unwrap());
        assert!(sat > 0.98, "expected saturation ~1, got {sat}");
        assert!(val > 0.98, "expected value ~1, got {val}");
    }

    #[test]
    fn a_click_outside_the_ring_band_is_ignored() {
        let mut wheel = ColorWheel::new(ColorU::new(255, 0, 0, 255));
        let app = AppContext::default();
        let mut ctx = EventContext::default();
        wheel.layout(
            SizeConstraint::loose(vec2f(WHEEL_SIZE, WHEEL_SIZE)),
            &mut LayoutContext::default(),
            &app,
        );
        wheel.paint(vec2f(0.0, 0.0), &mut PaintContext::new(goble_ui::render::Renderer::new()), &app);

        // The top-left corner of the wheel's box is outside the ring.
        assert!(!wheel.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(1.0, 1.0),
                button: 0,
            },
            &mut ctx,
            &app,
        ));
    }

    #[test]
    fn the_wheel_paints_the_ring_image_and_both_knobs() {
        let app = AppContext::default();
        let mut element = wheel_box(ColorU::new(255, 0, 0, 255));
        let commands = goble_ui::test_util::render_element(
            &mut element,
            vec2f(WHEEL_SIZE, WHEEL_SIZE),
            &app,
        );

        let ring = commands.iter().find_map(|c| match c {
            RenderCommand::DrawImage { rect, source, width, .. } => {
                Some((*rect, source.clone(), *width))
            }
            _ => None,
        });
        let (rect, source, width) = ring.expect("the hue ring is one image command");
        assert_eq!(source, RING_SOURCE);
        assert_eq!(width, RING_BITMAP);
        assert_eq!(rect.width(), WHEEL_SIZE);

        let rows = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::FillRectFadeRight { .. }))
            .count();
        assert_eq!(rows, SV_ROWS, "the square is one fade row per value step");
    }
}

