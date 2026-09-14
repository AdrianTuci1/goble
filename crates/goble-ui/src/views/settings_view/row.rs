use crate::elements::{
    AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, Expanded, Fill, Flex, Icon,
    MainAxisSize, Text, Tooltip,
};
use crate::theme::{ColorToken, SpacingToken};

const ROW_FONT_SIZE: f32 = 12.0;
const ROW_HEADER_PADDING: f32 = 15.0;
const ROW_CONTROL_RIGHT_PADDING: f32 = 5.0;
const ROW_DESCRIPTION_RIGHT_PAD: f32 = 100.0;
const ROW_ICON_SIZE: f32 = 13.0;
const ROW_LOCAL_ONLY_TOOLTIP: &str = "This setting is not synced to your other devices";

/// A settings row at the warp-new level of complexity: a leading title (plus
/// optional muted secondary text, an info/tooltip icon and a not-cloud-synced
/// icon), the control on the right, and an optional muted description paragraph
/// below that wraps before reaching the control.
pub(super) struct SettingsRow<'a> {
    app: &'a AppContext,
    label: String,
    control: Box<dyn Element>,
    description: Option<String>,
    tooltip: Option<String>,
    secondary: Option<String>,
    local_only: bool,
}

impl<'a> SettingsRow<'a> {
    pub(super) fn new(
        app: &'a AppContext,
        label: impl Into<String>,
        control: Box<dyn Element>,
    ) -> Self {
        Self {
            app,
            label: label.into(),
            control,
            description: None,
            tooltip: None,
            secondary: None,
            local_only: false,
        }
    }

    pub(super) fn with_description(mut self, text: impl Into<String>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub(super) fn with_tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub(super) fn with_secondary(mut self, text: impl Into<String>) -> Self {
        self.secondary = Some(text.into());
        self
    }

    /// Marks the setting as local-only (not synced to other devices).
    pub(super) fn with_local_only(mut self) -> Self {
        self.local_only = true;
        self
    }

    pub(super) fn build(self) -> Box<dyn Element> {
        let app = self.app;
        let spacing = app.theme.spacing_px(SpacingToken::Md);

        // Leading label: the title with any icons / secondary text hanging
        // inline to the right of it. The row expands to fill the header, so the
        // control is pushed to the right edge of the card.
        let mut label_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.0)
            .with_child(
                Text::new(self.label.clone())
                    .with_font_size(ROW_FONT_SIZE)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            );

        if let Some(tooltip_text) = self.tooltip {
            label_row = label_row.with_child(
                Tooltip::new(
                    Icon::new("info")
                        .with_size(ROW_ICON_SIZE)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                    tooltip_text,
                )
                .finish(),
            );
        }
        if self.local_only {
            label_row = label_row.with_child(
                Tooltip::new(
                    Icon::new("cloud-off")
                        .with_size(ROW_ICON_SIZE)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                    ROW_LOCAL_ONLY_TOOLTIP,
                )
                .finish(),
            );
        }
        if let Some(secondary) = self.secondary {
            label_row = label_row.with_child(
                Text::new(secondary)
                    .with_font_size(ROW_FONT_SIZE)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        let label_row = label_row.finish();

        // Header row: the (expanded) label followed by the control inset from
        // the right edge, mirroring warp-new's Shrinkable + control padding.
        let header = Expanded::new(label_row).finish();
        let control = Container::new(self.control)
            .with_padding_right(ROW_CONTROL_RIGHT_PADDING)
            .finish();

        let mut header_row = Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(header)
                .with_child(control)
                .finish(),
        );
        if self.description.is_none() {
            header_row = header_row.with_padding_bottom(ROW_HEADER_PADDING);
        }
        let header_row = header_row.finish();

        // Description paragraph wraps before the control and sits under the row.
        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(header_row);
        if let Some(description) = self.description {
            let description = Text::new(description)
                .with_font_size(ROW_FONT_SIZE)
                .with_theme_color(ColorToken::Muted, app)
                .finish();
            column = column.with_child(
                Container::new(description)
                    .with_padding_right(ROW_DESCRIPTION_RIGHT_PAD)
                    .with_padding_bottom(ROW_HEADER_PADDING)
                    .finish(),
            );
        }

        Container::new(column.finish())
            .with_padding(EdgeInsets::uniform(spacing))
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_corner_radius(app.theme.radius_px())
            .finish()
    }
}
