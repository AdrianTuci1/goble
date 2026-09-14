//! A row that wraps: items are placed left to right and moved to a new run
//! when the next one no longer fits the width it was given.
//!
//! `Flex::row` gives every child the same constraint and reports their sum, so
//! a row of controls measured in a pane that is too narrow for them paints past
//! the pane's edge. This is the row for controls whose number or width the
//! layout cannot know in advance — the rich input's buttons, the instruction
//! strip — matching warp-new's `Wrap::row` for the same job: the items keep
//! their own size and the row grows downwards instead of outwards.

use crate::elements::{
    AppContext, CrossAxisAlignment, Element, EventContext, LayoutContext, PaintContext, Point,
    SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};

pub struct Wrap {
    children: Vec<Box<dyn Element>>,
    /// Gap between two items on the same run.
    spacing: f32,
    /// Gap between two runs.
    run_spacing: f32,
    cross_axis_alignment: CrossAxisAlignment,
    size: Option<Vector2F>,
    origin: Option<Point>,
    /// Where each child sat, relative to the wrap's own origin.
    offsets: Vec<Vector2F>,
    /// Each child's own size, and the runs it was placed in: the first child of
    /// the run and that run's height.
    sizes: Vec<Vector2F>,
    runs: Vec<(usize, f32)>,
}

impl Default for Wrap {
    fn default() -> Self {
        Self {
            children: Vec::new(),
            spacing: 0.0,
            run_spacing: 0.0,
            cross_axis_alignment: CrossAxisAlignment::Start,
            size: None,
            origin: None,
            offsets: Vec::new(),
            sizes: Vec::new(),
            runs: Vec::new(),
        }
    }
}

impl Wrap {
    pub fn new() -> Self {
        Self::default()
    }

    /// The only orientation there is: items flow along the x axis.
    pub fn row() -> Self {
        Self::new()
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn with_run_spacing(mut self, run_spacing: f32) -> Self {
        self.run_spacing = run_spacing;
        self
    }

    pub fn with_cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    pub fn with_child(mut self, child: Box<dyn Element>) -> Self {
        self.children.push(child);
        self
    }

    pub fn with_children(mut self, children: impl IntoIterator<Item = Box<dyn Element>>) -> Self {
        self.children.extend(children);
        self
    }

    /// `Stretch` is not a wrapping alignment: a run has no single cross size to
    /// stretch to, so it is treated as `Start`.
    fn cross_offset(&self, run_height: f32, child_height: f32) -> f32 {
        match self.cross_axis_alignment {
            CrossAxisAlignment::Center => (run_height - child_height).max(0.0) / 2.0,
            CrossAxisAlignment::End => (run_height - child_height).max(0.0),
            CrossAxisAlignment::Start | CrossAxisAlignment::Stretch => 0.0,
        }
    }
}

impl Extend<Box<dyn Element>> for Wrap {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        self.children.extend(iter);
    }
}

impl Element for Wrap {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let max_width = constraint.max.x.max(0.0);
        let max_height = constraint.max.y;

        self.offsets.clear();
        self.sizes.clear();
        self.runs.clear();

        let mut x = 0.0_f32;
        let mut y = 0.0_f32;
        let mut run_height = 0.0_f32;
        let mut run_first = 0_usize;
        let mut width = 0.0_f32;

        for (index, child) in self.children.iter_mut().enumerate() {
            // Each item is measured at its own size — unbounded along the run,
            // so a label that could wrap its text reports the width it wants
            // and the run, not the item, decides where the break goes.
            let measured = child.layout(
                SizeConstraint::new(Vector2F::zero(), vec2f(f32::INFINITY, max_height)),
                ctx,
                app,
            );
            // An item wider or taller than the row is clamped to it rather than
            // painted past the edge.
            let size = vec2f(
                measured.x.min(max_width),
                measured.y.min(max_height.max(0.0)),
            );
            // A run always keeps its first item, however narrow the constraint
            // is: an item wider than the whole row still starts at the left
            // edge rather than being pushed to a run of its own forever.
            if index > run_first && x + size.x > max_width {
                self.runs.push((run_first, run_height));
                y += run_height + self.run_spacing;
                x = 0.0;
                run_height = 0.0;
                run_first = index;
            }
            self.offsets.push(vec2f(x, y));
            self.sizes.push(size);
            x += size.x + self.spacing;
            width = width.max(x - self.spacing);
            run_height = run_height.max(size.y);
        }
        if !self.children.is_empty() {
            self.runs.push((run_first, run_height));
        }

        for (run, (first, height)) in self.runs.iter().enumerate() {
            let last = self
                .runs
                .get(run + 1)
                .map(|(first, _)| *first)
                .unwrap_or(self.children.len());
            for index in *first..last {
                let child_height = self.sizes[index].y;
                self.offsets[index].y += self.cross_offset(*height, child_height);
            }
        }

        let size = vec2f(width, (y + run_height).max(0.0));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        for (index, child) in self.children.iter_mut().enumerate() {
            let offset = self
                .offsets
                .get(index)
                .copied()
                .unwrap_or_else(Vector2F::zero);
            child.paint(origin + offset, ctx, app);
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
        for child in &mut self.children {
            if child.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::Text;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use crate::theme::ColorToken;

    fn app() -> AppContext {
        AppContext::default()
    }

    fn label(text: &str) -> Box<dyn Element> {
        Text::new(text.to_string())
            .with_theme_color(ColorToken::Text, &app())
            .with_font_size(12.0)
            .with_max_lines(1)
            .finish()
    }

    fn lines(commands: &[RenderCommand], text: &str) -> Vec<(f32, f32)> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text: run, origin, .. } if run == text => {
                    Some((origin.x, origin.y))
                }
                _ => None,
            })
            .collect()
    }

    /// Everything fits: one run, items in order, left to right.
    #[test]
    fn items_that_fit_stay_on_one_run_in_order() {
        let app = app();
        let mut wrap = Wrap::row()
            .with_spacing(8.0)
            .with_child(label("alpha"))
            .with_child(label("beta"))
            .with_child(label("gamma"));
        let size = wrap.layout(
            SizeConstraint::loose(vec2f(600.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let commands = render_element(&mut (Box::new(wrap) as Box<dyn Element>), vec2f(600.0, 100.0), &app);
        let alpha = lines(&commands, "alpha")[0];
        let beta = lines(&commands, "beta")[0];
        let gamma = lines(&commands, "gamma")[0];
        assert!(alpha.0 < beta.0 && beta.0 < gamma.0, "left to right, in order");
        assert_eq!(alpha.1, beta.1, "one run, one line");
        assert_eq!(beta.1, gamma.1, "one run, one line");
        assert!(size.y < 30.0, "one run is one line tall: {size:?}");
    }

    /// The next item does not fit: it starts a new run instead of painting past
    /// the constraint, and the wrap's height covers both runs.
    #[test]
    fn an_item_that_does_not_fit_starts_a_new_run() {
        let app = app();
        let mut one_run = Wrap::row()
            .with_spacing(8.0)
            .with_child(label("alpha"))
            .with_child(label("beta"));
        let wide = one_run.layout(
            SizeConstraint::loose(vec2f(600.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        // One pixel less than the two items need together.
        let narrow_width = wide.x - 1.0;

        let mut wrap = Wrap::row()
            .with_spacing(8.0)
            .with_child(label("alpha"))
            .with_child(label("beta"));
        let narrow = wrap.layout(
            SizeConstraint::loose(vec2f(narrow_width, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let commands = render_element(
            &mut (Box::new(wrap) as Box<dyn Element>),
            vec2f(narrow_width, 100.0),
            &app,
        );
        let alpha = lines(&commands, "alpha")[0];
        let beta = lines(&commands, "beta")[0];
        assert!(
            beta.1 > alpha.1,
            "the item that did not fit moved to the next run: {alpha:?} then {beta:?}"
        );
        assert!(beta.0 <= alpha.0, "and it starts at the run's left edge");
        assert!(
            narrow.y > wide.y,
            "two runs are taller than one: {narrow:?} vs {wide:?}"
        );
        assert!(
            alpha.0 < narrow_width && beta.0 < narrow_width,
            "both items stay inside the constraint"
        );
        assert!(
            wide.x > narrow_width,
            "the wrap reports the widest run: {wide:?} into {narrow_width}"
        );
    }

    /// A run keeps its first item whatever its width, so an item wider than the
    /// row is drawn once from the left edge rather than being pushed down
    /// forever.
    #[test]
    fn the_first_item_of_a_run_always_stays_on_it() {
        let app = app();
        let mut wrap = Wrap::row()
            .with_spacing(8.0)
            .with_child(label("a very wide first item that does not fit at all"))
            .with_child(label("b"));
        let size = wrap.layout(
            SizeConstraint::loose(vec2f(40.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let commands = render_element(&mut (Box::new(wrap) as Box<dyn Element>), vec2f(40.0, 200.0), &app);
        let wide = lines(&commands, "a very wide first item that does not fit at all")[0];
        let small = lines(&commands, "b")[0];
        assert_eq!(wide.0, 0.0, "the wide item keeps its place on the run");
        assert_eq!(wide.1, 0.0, "it is the first run, not pushed down");
        assert!(
            small.1 > wide.1,
            "and only the item after it moved on: {wide:?} then {small:?}"
        );
        assert!(size.y < 100.0, "two items, two runs at most: {size:?}");
    }

    /// Items on a run are centered against the tallest of them.
    #[test]
    fn a_run_centers_its_items_on_the_tallest() {
        let app = app();
        let tall = crate::elements::Container::new(label("tall"))
            .with_padding(crate::elements::EdgeInsets::new(0.0, 0.0, 0.0, 10.0))
            .finish();
        let mut wrap = Wrap::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(tall)
            .with_child(label("short"));
        let _ = wrap.layout(
            SizeConstraint::loose(vec2f(400.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let commands = render_element(&mut (Box::new(wrap) as Box<dyn Element>), vec2f(400.0, 200.0), &app);
        let tall_line = lines(&commands, "tall")[0];
        let short_line = lines(&commands, "short")[0];
        assert!(
            short_line.1 > tall_line.1,
            "the shorter item is pushed down to the run's middle: {tall_line:?} vs {short_line:?}"
        );
    }
}
