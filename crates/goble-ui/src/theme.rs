use crate::color::ColorU;

/// A complete theme for Goble UI.
///
/// Mirrors the TypeScript design tokens in `goble-desktop/src/utils/designSystem.ts`.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    pub colors: ColorScheme,
    pub accent: AccentColor,
    pub font: FontFamily,
    pub density: Density,
    pub radius: Radius,
    pub spacing: Spacing,
}

impl Theme {
    /// Dark theme (default).
    pub fn dark() -> Self {
        Self {
            name: "dark",
            colors: ColorScheme {
                bg: ColorU::hex(0x0f1115),
                surface: ColorU::hex(0x181b21),
                surface_raised: ColorU::hex(0x21252e),
                border: ColorU::hex(0x2a2e36),
                text: ColorU::hex(0xe4e6eb),
                muted: ColorU::hex(0x8b949e),
                hover: ColorU::hex(0x21252e),
                selected: ColorU::hex(0x2a2e36),
                success: ColorU::hex(0x10b981),
                warning: ColorU::hex(0xf59e0b),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
            },
            accent: AccentColor::Blue,
            font: FontFamily::System,
            density: Density::Default,
            radius: Radius::Default,
            spacing: Spacing::default(),
        }
    }

    /// Light theme.
    pub fn light() -> Self {
        Self {
            name: "light",
            colors: ColorScheme {
                bg: ColorU::hex(0xf6f7f9),
                surface: ColorU::hex(0xffffff),
                surface_raised: ColorU::hex(0xf3f4f6),
                border: ColorU::hex(0xe2e4e9),
                text: ColorU::hex(0x1f2937),
                muted: ColorU::hex(0x6b7280),
                hover: ColorU::hex(0xf3f4f6),
                selected: ColorU::hex(0xe5e7eb),
                success: ColorU::hex(0x10b981),
                warning: ColorU::hex(0xf59e0b),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
            },
            accent: AccentColor::Blue,
            font: FontFamily::System,
            density: Density::Default,
            radius: Radius::Default,
            spacing: Spacing::default(),
        }
    }

    /// Midnight theme.
    pub fn midnight() -> Self {
        Self {
            name: "midnight",
            colors: ColorScheme {
                bg: ColorU::hex(0x0a0c10),
                surface: ColorU::hex(0x11131a),
                surface_raised: ColorU::hex(0x181b23),
                border: ColorU::hex(0x1f222b),
                text: ColorU::hex(0xe8eaed),
                muted: ColorU::hex(0x6b7280),
                hover: ColorU::hex(0x181b23),
                selected: ColorU::hex(0x1e212b),
                success: ColorU::hex(0x10b981),
                warning: ColorU::hex(0xf59e0b),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
            },
            accent: AccentColor::Blue,
            font: FontFamily::System,
            density: Density::Default,
            radius: Radius::Default,
            spacing: Spacing::default(),
        }
    }

    /// Resolve the accent color to a concrete [`ColorU`].
    pub fn accent_color(&self) -> ColorU {
        self.accent.color()
    }

    /// Resolve a named color token.
    pub fn color(&self, token: ColorToken) -> ColorU {
        match token {
            ColorToken::Bg => self.colors.bg,
            ColorToken::Surface => self.colors.surface,
            ColorToken::SurfaceRaised => self.colors.surface_raised,
            ColorToken::Border => self.colors.border,
            ColorToken::Text => self.colors.text,
            ColorToken::Muted => self.colors.muted,
            ColorToken::Hover => self.colors.hover,
            ColorToken::Selected => self.colors.selected,
            ColorToken::Accent => self.accent_color(),
            ColorToken::Success => self.colors.success,
            ColorToken::Warning => self.colors.warning,
            ColorToken::Error => self.colors.error,
            ColorToken::Badge => self.colors.badge,
        }
    }

    /// Spacing multiplier applied to `Spacing` values.
    pub fn density_factor(&self) -> f32 {
        self.density.factor()
    }

    /// Resolved corner radius in pixels.
    pub fn radius_px(&self) -> f32 {
        self.radius.px()
    }

    /// Resolved spacing value in pixels.
    pub fn spacing_px(&self, token: SpacingToken) -> f32 {
        self.spacing.px(token) * self.density_factor()
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorScheme {
    pub bg: ColorU,
    pub surface: ColorU,
    pub surface_raised: ColorU,
    pub border: ColorU,
    pub text: ColorU,
    pub muted: ColorU,
    pub hover: ColorU,
    pub selected: ColorU,
    pub success: ColorU,
    pub warning: ColorU,
    pub error: ColorU,
    pub badge: ColorU,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorToken {
    Bg,
    Surface,
    SurfaceRaised,
    Border,
    Text,
    Muted,
    Hover,
    Selected,
    Accent,
    Success,
    Warning,
    Error,
    Badge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccentColor {
    Blue,
    Green,
    Purple,
    Orange,
}

impl AccentColor {
    pub fn color(&self) -> ColorU {
        match self {
            AccentColor::Blue => ColorU::hex(0x2563eb),
            AccentColor::Green => ColorU::hex(0x10b981),
            AccentColor::Purple => ColorU::hex(0x8b5cf6),
            AccentColor::Orange => ColorU::hex(0xf97316),
        }
    }
}

impl Default for AccentColor {
    fn default() -> Self {
        AccentColor::Blue
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontFamily {
    System,
    Mono,
    Serif,
}

impl Default for FontFamily {
    fn default() -> Self {
        FontFamily::System
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Radius {
    Sharp,
    Default,
    Rounded,
}

impl Radius {
    pub fn px(&self) -> f32 {
        match self {
            Radius::Sharp => 0.0,
            Radius::Default => 8.0,
            Radius::Rounded => 14.0,
        }
    }
}

impl Default for Radius {
    fn default() -> Self {
        Radius::Default
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Density {
    Compact,
    Default,
    Spacious,
}

impl Density {
    pub fn factor(&self) -> f32 {
        match self {
            Density::Compact => 0.85,
            Density::Default => 1.0,
            Density::Spacious => 1.15,
        }
    }
}

impl Default for Density {
    fn default() -> Self {
        Density::Default
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

impl Spacing {
    pub fn px(&self, token: SpacingToken) -> f32 {
        match token {
            SpacingToken::Xs => self.xs,
            SpacingToken::Sm => self.sm,
            SpacingToken::Md => self.md,
            SpacingToken::Lg => self.lg,
            SpacingToken::Xl => self.xl,
        }
    }
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
            xl: 24.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpacingToken {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
}

impl ColorU {
    /// Create a color from a 24-bit RGB hex value, fully opaque.
    pub const fn hex(rgb: u32) -> Self {
        Self::new(
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
            255,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_default_is_dark() {
        let theme = Theme::default();
        assert_eq!(theme.name, "dark");
        assert_eq!(theme.color(ColorToken::Bg), ColorU::hex(0x0f1115));
    }

    #[test]
    fn accent_color_resolves() {
        assert_eq!(AccentColor::Blue.color(), ColorU::hex(0x2563eb));
    }

    #[test]
    fn density_factors() {
        assert_eq!(Density::Compact.factor(), 0.85);
        assert_eq!(Density::Spacious.factor(), 1.15);
    }

    #[test]
    fn radius_px() {
        assert_eq!(Radius::Sharp.px(), 0.0);
        assert_eq!(Radius::Rounded.px(), 14.0);
    }

    #[test]
    fn spacing_scaled_by_density() {
        let mut theme = Theme::light();
        theme.density = Density::Compact;
        assert_eq!(theme.spacing_px(SpacingToken::Lg), 16.0 * 0.85);
    }
}
