use crate::scene::Color;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_color(&self) -> Color {
        Color::from_u8(self.r, self.g, self.b, 255)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub background: Rgb,
    pub surface: Rgb,
    pub surface_hover: Rgb,
    pub text: Rgb,
    pub text_secondary: Rgb,
    pub accent: Rgb,
    pub border: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
    pub font_size_sm: f32,
    pub font_size_md: f32,
    pub font_size_lg: f32,
    pub spacing_xs: f32,
    pub spacing_sm: f32,
    pub spacing_md: f32,
    pub spacing_lg: f32,
    pub radius_sm: f32,
    pub radius_md: f32,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            background: Rgb::new(10, 10, 12),
            surface: Rgb::new(22, 22, 26),
            surface_hover: Rgb::new(30, 30, 36),
            text: Rgb::new(230, 230, 235),
            text_secondary: Rgb::new(150, 150, 160),
            accent: Rgb::new(20, 184, 166),
            border: Rgb::new(40, 40, 46),
            success: Rgb::new(34, 197, 94),
            warning: Rgb::new(234, 179, 8),
            error: Rgb::new(239, 68, 68),
            font_size_sm: 12.0,
            font_size_md: 14.0,
            font_size_lg: 18.0,
            spacing_xs: 4.0,
            spacing_sm: 8.0,
            spacing_md: 16.0,
            spacing_lg: 24.0,
            radius_sm: 4.0,
            radius_md: 8.0,
        }
    }

    pub fn light() -> Self {
        Self {
            background: Rgb::new(250, 250, 252),
            surface: Rgb::new(255, 255, 255),
            surface_hover: Rgb::new(245, 245, 247),
            text: Rgb::new(24, 24, 27),
            text_secondary: Rgb::new(100, 100, 110),
            accent: Rgb::new(13, 148, 136),
            border: Rgb::new(228, 228, 231),
            success: Rgb::new(22, 163, 74),
            warning: Rgb::new(202, 138, 4),
            error: Rgb::new(220, 38, 38),
            font_size_sm: 12.0,
            font_size_md: 14.0,
            font_size_lg: 18.0,
            spacing_xs: 4.0,
            spacing_sm: 8.0,
            spacing_md: 16.0,
            spacing_lg: 24.0,
            radius_sm: 4.0,
            radius_md: 8.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dark_theme() {
        let theme = Theme::dark();
        assert_eq!(theme.accent, Rgb::new(20, 184, 166));
    }

    #[test]
    fn test_light_theme() {
        let theme = Theme::light();
        assert_eq!(theme.background, Rgb::new(250, 250, 252));
    }
}
