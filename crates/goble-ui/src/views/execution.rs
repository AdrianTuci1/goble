use crate::elements::rect::RectElement;
use crate::elements::stack::Stack;
use crate::elements::text::TextElement;
use crate::elements::{BoxedElement, Element};
use crate::scene::Color;
use crate::theme::Theme;
use goble_core::execution::{ExecutionStatus, ExecutionTrace, LogEntry, LogLevel, Step};

pub struct ExecutionView {
    trace: ExecutionTrace,
}

impl ExecutionView {
    pub fn new(trace: ExecutionTrace) -> Self {
        Self { trace }
    }

    pub fn trace(&self) -> &ExecutionTrace {
        &self.trace
    }

    pub fn set_trace(&mut self, trace: ExecutionTrace) {
        self.trace = trace;
    }

    pub fn build(&self, width: f32, height: f32, theme: &Theme) -> BoxedElement {
        let mut root = Stack::vertical()
            .with_padding(theme.spacing_md)
            .with_spacing(theme.spacing_md)
            .with_background(theme.background.to_color())
            .with_children(vec![
                self.build_header(theme),
                self.build_steps(width, height, theme),
            ]);
        root.set_size((width, height));
        Box::new(root)
    }

    fn build_header(&self, theme: &Theme) -> BoxedElement {
        let status_text = format!(
            "Trace {} — {}",
            self.trace.id,
            status_label(&self.trace.status)
        );
        let status_color = status_color(&self.trace.status, theme);

        let mut header = Stack::horizontal()
            .with_spacing(theme.spacing_sm)
            .with_children(vec![
                Box::new(
                    RectElement::new(status_color)
                        .with_size(theme.font_size_sm, theme.font_size_sm),
                ),
                Box::new(
                    TextElement::new(status_text, theme.font_size_md)
                        .with_color(theme.text.to_color()),
                ),
            ]);
        header.set_size((
            f32::MAX,
            theme.font_size_md.max(theme.font_size_sm) + theme.spacing_sm,
        ));
        Box::new(header)
    }

    fn build_steps(&self, width: f32, height: f32, theme: &Theme) -> BoxedElement {
        let view = self.trace.sequential_view();
        let mut steps = Stack::vertical()
            .with_spacing(theme.spacing_sm)
            .with_background(theme.surface.to_color());

        for (depth, step) in view {
            steps.push(self.build_step(step, depth, width, theme));
        }

        steps.set_size((width, height));
        Box::new(steps)
    }

    fn build_step(&self, step: &Step, depth: usize, width: f32, theme: &Theme) -> BoxedElement {
        let indent = depth as f32 * theme.spacing_lg;
        let available_width = (width - indent - theme.spacing_md * 2.0).max(0.0);

        let mut row = Stack::horizontal()
            .with_spacing(theme.spacing_sm)
            .with_children(vec![
                Box::new(
                    RectElement::new(status_color(&step.status, theme))
                        .with_size(theme.font_size_sm, theme.font_size_sm),
                ),
                Box::new(
                    TextElement::new(&step.name, theme.font_size_md)
                        .with_color(theme.text.to_color()),
                ),
            ]);
        row.set_size((available_width, theme.font_size_md));

        let mut body = Stack::vertical()
            .with_spacing(theme.spacing_xs)
            .with_children(vec![Box::new(row)]);

        if !step.logs.is_empty() {
            let mut logs = Stack::vertical().with_spacing(theme.spacing_xs);
            for log in &step.logs {
                logs.push(self.build_log(log, theme));
            }
            body.push(Box::new(logs));
        }

        let mut container = Stack::vertical()
            .with_padding(theme.spacing_sm)
            .with_background(theme.surface.to_color())
            .with_children(vec![Box::new(body)]);
        container.set_size((width - indent, theme.font_size_md + theme.spacing_sm * 2.0));

        let mut indented = Stack::horizontal().with_children(vec![
            Box::new(RectElement::new(Color::new(0.0, 0.0, 0.0, 0.0)).with_size(indent, 1.0)),
            Box::new(container),
        ]);
        indented.set_size((width, theme.font_size_md + theme.spacing_sm * 2.0));

        Box::new(indented)
    }

    fn build_log(&self, log: &LogEntry, theme: &Theme) -> BoxedElement {
        let label = format!("[{:?}] {}", log.level, log.message);
        let mut text = TextElement::new(label, theme.font_size_sm)
            .with_color(log_level_color(log.level.clone(), theme));
        text.set_size((f32::MAX, theme.font_size_sm * 1.4));
        Box::new(text)
    }
}

pub fn status_color(status: &ExecutionStatus, theme: &Theme) -> Color {
    match status {
        ExecutionStatus::Success => theme.success.to_color(),
        ExecutionStatus::Failure(_) => theme.error.to_color(),
        ExecutionStatus::Running => theme.accent.to_color(),
        ExecutionStatus::Pending => theme.warning.to_color(),
        ExecutionStatus::Cancelled => theme.text_secondary.to_color(),
    }
}

pub fn status_label(status: &ExecutionStatus) -> String {
    match status {
        ExecutionStatus::Success => "Success".to_string(),
        ExecutionStatus::Failure(reason) => format!("Failure: {reason}"),
        ExecutionStatus::Running => "Running".to_string(),
        ExecutionStatus::Pending => "Pending".to_string(),
        ExecutionStatus::Cancelled => "Cancelled".to_string(),
    }
}

pub fn log_level_color(level: LogLevel, theme: &Theme) -> Color {
    match level {
        LogLevel::Debug => theme.text_secondary.to_color(),
        LogLevel::Info => theme.text.to_color(),
        LogLevel::Warn => theme.warning.to_color(),
        LogLevel::Error => theme.error.to_color(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{LayoutContext, SizeConstraint};
    use goble_core::agent::AgentId;
    use goble_core::execution::LogLevel;

    fn sample_trace() -> ExecutionTrace {
        let mut trace = ExecutionTrace::new(AgentId::generate());
        let root = trace.add_root_step("root-step");
        let root_id = root.id.clone();
        root.log(LogLevel::Info, "root started");
        root.finish(ExecutionStatus::Success);

        let child = trace.add_child_step(&root_id, "child-step").unwrap();
        let child_id = child.id.clone();
        child.log(LogLevel::Warn, "child warning");
        child.finish(ExecutionStatus::Success);

        let grandchild = trace.add_child_step(&child_id, "grandchild-step").unwrap();
        grandchild.log(LogLevel::Error, "grandchild error");
        grandchild.finish(ExecutionStatus::Failure("boom".to_string()));

        trace.finish(ExecutionStatus::Success);
        trace
    }

    #[test]
    fn test_build_execution_view() {
        let trace = sample_trace();
        let view = ExecutionView::new(trace);
        let theme = Theme::dark();
        let mut element = view.build(800.0, 600.0, &theme);

        let mut ctx = LayoutContext::new(theme.clone(), 1.0);
        let size = element.layout(SizeConstraint::new(800.0, 600.0), &mut ctx);
        assert!(size.0 > 0.0);
        assert!(size.1 > 0.0);
    }

    #[test]
    fn test_status_colors() {
        let theme = Theme::dark();
        assert_eq!(
            status_color(&ExecutionStatus::Success, &theme),
            theme.success.to_color()
        );
        assert_eq!(
            status_color(&ExecutionStatus::Failure("x".to_string()), &theme),
            theme.error.to_color()
        );
    }

    #[test]
    fn test_log_level_colors() {
        let theme = Theme::dark();
        assert_eq!(
            log_level_color(LogLevel::Debug, &theme),
            theme.text_secondary.to_color()
        );
        assert_eq!(
            log_level_color(LogLevel::Info, &theme),
            theme.text.to_color()
        );
        assert_eq!(
            log_level_color(LogLevel::Warn, &theme),
            theme.warning.to_color()
        );
        assert_eq!(
            log_level_color(LogLevel::Error, &theme),
            theme.error.to_color()
        );
    }
}
