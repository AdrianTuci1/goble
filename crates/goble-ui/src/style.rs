use crate::color::ColorU;
use crate::elements::Fill;

#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub fn uniform(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn with_left(mut self, value: f32) -> Self {
        self.left = value;
        self
    }

    pub fn with_top(mut self, value: f32) -> Self {
        self.top = value;
        self
    }

    pub fn with_right(mut self, value: f32) -> Self {
        self.right = value;
        self
    }

    pub fn with_bottom(mut self, value: f32) -> Self {
        self.bottom = value;
        self
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BorderRadius {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_left: f32,
    pub bottom_right: f32,
}

impl BorderRadius {
    pub fn uniform(value: f32) -> Self {
        Self {
            top_left: value,
            top_right: value,
            bottom_left: value,
            bottom_right: value,
        }
    }
}

/// A minimal style bag for UI components.
#[derive(Clone, Copy, Debug, Default)]
pub struct UiComponentStyles {
    pub background: Option<Fill>,
    pub border_color: Option<ColorU>,
    pub border_width: Option<f32>,
    pub border_radius: Option<BorderRadius>,
    pub padding: Option<EdgeInsets>,
    pub margin: Option<EdgeInsets>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub font_size: Option<f32>,
    pub font_color: Option<ColorU>,
}

impl UiComponentStyles {
    pub fn merge(self, other: UiComponentStyles) -> Self {
        Self {
            background: other.background.or(self.background),
            border_color: other.border_color.or(self.border_color),
            border_width: other.border_width.or(self.border_width),
            border_radius: other.border_radius.or(self.border_radius),
            padding: other.padding.or(self.padding),
            margin: other.margin.or(self.margin),
            width: other.width.or(self.width),
            height: other.height.or(self.height),
            font_size: other.font_size.or(self.font_size),
            font_color: other.font_color.or(self.font_color),
        }
    }
}
