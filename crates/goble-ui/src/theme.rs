use crate::color::ColorU;

/// A complete theme for Goble UI.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    pub colors: ColorScheme,
    pub accent: AccentColor,
    /// Optional override for the main foreground (text) color. When set, the
    /// `Text` token resolves to this instead of `colors.text`.
    pub primary: Option<ColorU>,
    /// Optional override for the secondary foreground (muted) color. When set,
    /// the `Muted` token resolves to this instead of `colors.muted`.
    pub secondary: Option<ColorU>,
    /// Optional override for the accent/brand color. When set, the `Accent`
    /// token resolves to this instead of `accent.color()`.
    pub custom_accent: Option<ColorU>,
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
                bg: ColorU::hex(0x0e0e0e),
                surface: ColorU::hex(0x161616),
                surface_raised: ColorU::hex(0x1e1e1e),
                border: ColorU::hex(0x262626),
                text: ColorU::hex(0xe8e8e8),
                muted: ColorU::hex(0x8e8e8e),
                hover: ColorU::hex(0x1e1e1e),
                selected: ColorU::hex(0x2a2a2a),
                focus: ColorU::hex(0x19aad8),
                success: ColorU::hex(0x10b981),
                warning: ColorU::hex(0xf59e0b),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
                diff_insert_bg: ColorU::hex(0x0f3d20),
                diff_delete_bg: ColorU::hex(0x4d1116),
            },
            accent: AccentColor::Blue,
            primary: None,
            secondary: None,
            custom_accent: None,
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
                bg: ColorU::hex(0xf5f5f5),
                surface: ColorU::hex(0xffffff),
                surface_raised: ColorU::hex(0xf0f0f0),
                border: ColorU::hex(0xe0e0e0),
                text: ColorU::hex(0x1f1f1f),
                muted: ColorU::hex(0x6f6f6f),
                hover: ColorU::hex(0xf0f0f0),
                selected: ColorU::hex(0xe2e2e2),
                focus: ColorU::hex(0x00c2ff),
                success: ColorU::hex(0x10b981),
                warning: ColorU::hex(0xf59e0b),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
                diff_insert_bg: ColorU::hex(0xd6f5e0),
                diff_delete_bg: ColorU::hex(0xfbdada),
            },
            accent: AccentColor::Blue,
            primary: None,
            secondary: None,
            custom_accent: None,
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
                bg: ColorU::hex(0x0a0a0a),
                surface: ColorU::hex(0x121212),
                surface_raised: ColorU::hex(0x191919),
                border: ColorU::hex(0x212121),
                text: ColorU::hex(0xeaeaea),
                muted: ColorU::hex(0x6f6f6f),
                hover: ColorU::hex(0x191919),
                selected: ColorU::hex(0x222222),
                focus: ColorU::hex(0x19aad8),
                success: ColorU::hex(0x10b981),
                warning: ColorU::hex(0xf59e0b),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
                diff_insert_bg: ColorU::hex(0x0f3d20),
                diff_delete_bg: ColorU::hex(0x4d1116),
            },
            accent: AccentColor::Blue,
            primary: None,
            secondary: None,
            custom_accent: None,
            font: FontFamily::System,
            density: Density::Default,
            radius: Radius::Default,
            spacing: Spacing::default(),
        }
    }

    /// Return a copy with the custom primary/secondary/accent overrides applied
    /// (`None` keeps the theme's built-in color).
    pub fn with_overrides(
        mut self,
        primary: Option<ColorU>,
        secondary: Option<ColorU>,
        accent: Option<ColorU>,
    ) -> Self {
        self.primary = primary;
        self.secondary = secondary;
        self.custom_accent = accent;
        self
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
            ColorToken::Text => self.primary.unwrap_or(self.colors.text),
            ColorToken::Muted => self.secondary.unwrap_or(self.colors.muted),
            ColorToken::Hover => self.colors.hover,
            ColorToken::Selected => self.colors.selected,
            ColorToken::Focus => self.colors.focus,
            ColorToken::Accent => self.custom_accent.unwrap_or_else(|| self.accent.color()),
            ColorToken::Success => self.colors.success,
            ColorToken::Warning => self.colors.warning,
            ColorToken::Error => self.colors.error,
            ColorToken::Badge => self.colors.badge,
            ColorToken::DiffInsertBg => self.colors.diff_insert_bg,
            ColorToken::DiffDeleteBg => self.colors.diff_delete_bg,
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
    /// The focused text field's insertion caret and its ring, kept apart from
    /// `accent` so the neutral UI accent stays neutral. warp-new's default dark
    /// (and light) theme accent — `app/src/themes/default_themes.rs::dark_theme`
    /// `#19aad8`, `light_theme` `#00c2ff` — which is also its editor cursor
    /// colour, since `WarpTheme::cursor()` falls back to `accent()`.
    pub focus: ColorU,
    pub success: ColorU,
    pub warning: ColorU,
    pub error: ColorU,
    pub badge: ColorU,
    /// Background band behind an added line of a diff.
    pub diff_insert_bg: ColorU,
    /// Background band behind a removed line of a diff.
    pub diff_delete_bg: ColorU,
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
    /// The focused text field's caret and ring — see `ColorScheme::focus`.
    Focus,
    Accent,
    Success,
    Warning,
    Error,
    Badge,
    DiffInsertBg,
    DiffDeleteBg,
}

/// The preset colours a tab can be tinted with: the six warp-new's own tab menu
/// offers (`app/src/ui_components/color_dot.rs::TAB_COLOR_OPTIONS` — the normal
/// ANSI red through cyan), read out of the terminal palette
/// ([`goble_terminal::palette::ANSI_16`]) so a tab and the shell inside it agree
/// on what "red" is. The "no colour" entry is the `Option`'s `None`, as it is
/// there: the menu draws it as its own dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabColor {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
}

impl TabColor {
    /// The six presets, in the order warp-new's dot row draws them.
    pub const ALL: [TabColor; 6] = [
        TabColor::Red,
        TabColor::Green,
        TabColor::Yellow,
        TabColor::Blue,
        TabColor::Magenta,
        TabColor::Cyan,
    ];

    /// The palette index the colour is read at: red is ANSI 1 through cyan at 6.
    pub const fn ansi_index(self) -> usize {
        match self {
            TabColor::Red => 1,
            TabColor::Green => 2,
            TabColor::Yellow => 3,
            TabColor::Blue => 4,
            TabColor::Magenta => 5,
            TabColor::Cyan => 6,
        }
    }

    /// The colour as the palette defines it.
    pub fn color(self) -> ColorU {
        let (r, g, b) = goble_terminal::palette::ANSI_16[self.ansi_index()];
        ColorU::new(r, g, b, 255)
    }

    /// The name the menu's tooltip reads, as warp-new's own `Display` spells it.
    pub const fn label(self) -> &'static str {
        match self {
            TabColor::Red => "Red",
            TabColor::Green => "Green",
            TabColor::Yellow => "Yellow",
            TabColor::Blue => "Blue",
            TabColor::Magenta => "Magenta",
            TabColor::Cyan => "Cyan",
        }
    }

    /// How much of the colour a tab carries while it is in `state`, over the
    /// surface that tab is filled with: warp-new's own 60 / 40 / 20 split of the
    /// tab background between the active, the hovered and the resting tab
    /// (`app/src/tab.rs`, `base_opacity`).
    pub fn tint_over(self, base: ColorU, state: TabTint) -> ColorU {
        base.mix(&self.color(), state.opacity())
    }
}

/// Which of a tab's three states its colour is measured at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabTint {
    Active,
    Hovered,
    Resting,
}

impl TabTint {
    /// The share of the colour the tab carries in this state.
    fn opacity(self) -> f32 {
        match self {
            TabTint::Active => 0.6,
            TabTint::Hovered => 0.4,
            TabTint::Resting => 0.2,
        }
    }
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
            // Neutral gray: the UI is intentionally blue-free (gray/black tints).
            AccentColor::Blue => ColorU::hex(0x9a9a9a),
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
        assert_eq!(theme.color(ColorToken::Bg), ColorU::hex(0x0e0e0e));
    }

    #[test]
    fn accent_color_resolves() {
        assert_eq!(AccentColor::Blue.color(), ColorU::hex(0x9a9a9a));
    }

    /// The focus blue is warp-new's own default dark theme accent, and it is
    /// deliberately not the UI accent: the caret and the focused field's ring
    /// stay blue even though the rest of the UI is neutral.
    #[test]
    fn focus_is_the_reference_blue_and_not_the_accent() {
        let theme = Theme::dark();
        assert_eq!(theme.color(ColorToken::Focus), ColorU::hex(0x19aad8));
        assert_ne!(
            theme.color(ColorToken::Focus),
            theme.color(ColorToken::Accent),
            "the focus blue is not the neutral UI accent"
        );
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
