use crate::color::ColorU;

/// How much of the background a selection band carries. The accent at full
/// strength is an opaque band that swamps the glyphs it covers, so the band is
/// the accent pulled this far toward the background: desaturated, but still a
/// step away from the surface it sits on.
const SELECTION_BG_MIX: f32 = 0.55;

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
    /// Optional override for the page background. When set, the `Bg` token
    /// resolves to this instead of `colors.bg`.
    pub bg: Option<ColorU>,
    /// Optional override for the pane surface. When set, the `Surface` token
    /// resolves to this instead of `colors.surface`.
    pub surface: Option<ColorU>,
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
                path: ColorU::hex(0xff9e64),
                command: ColorU::hex(0xe0af68),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
                diff_insert_bg: ColorU::hex(0x0f3d20),
                diff_delete_bg: ColorU::hex(0x4d1116),
            },
            accent: AccentColor::Blue,
            primary: None,
            secondary: None,
            custom_accent: None,
            bg: None,
            surface: None,
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
                path: ColorU::hex(0xff9e64),
                command: ColorU::hex(0xe0af68),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
                diff_insert_bg: ColorU::hex(0xd6f5e0),
                diff_delete_bg: ColorU::hex(0xfbdada),
            },
            accent: AccentColor::Blue,
            primary: None,
            secondary: None,
            custom_accent: None,
            bg: None,
            surface: None,
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
                path: ColorU::hex(0xff9e64),
                command: ColorU::hex(0xe0af68),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
                diff_insert_bg: ColorU::hex(0x0f3d20),
                diff_delete_bg: ColorU::hex(0x4d1116),
            },
            accent: AccentColor::Blue,
            primary: None,
            secondary: None,
            custom_accent: None,
            bg: None,
            surface: None,
            font: FontFamily::System,
            density: Density::Default,
            radius: Radius::Default,
            spacing: Spacing::default(),
        }
    }

    /// A preset palette as the whole theme: the base scheme and its own accent,
    /// with no channel overrides. Selecting a preset in the Appearance page
    /// clears the overrides, so what is shown is exactly this palette; the wheel
    /// can then refine it again.
    pub fn preset(preset: ThemePreset) -> Self {
        Self {
            name: preset.key(),
            colors: preset.colors(),
            accent: AccentColor::Custom(preset.accent()),
            primary: None,
            secondary: None,
            custom_accent: None,
            bg: None,
            surface: None,
            font: FontFamily::System,
            density: Density::Default,
            radius: Radius::Default,
            spacing: Spacing::default(),
        }
    }

    /// Return a copy with the custom channel overrides applied (`None` keeps the
    /// theme's built-in color). `accent` replaces the preset's own accent; the
    /// background and surface overrides win over the base palette's `bg` and
    /// `surface`.
    pub fn with_overrides(
        mut self,
        primary: Option<ColorU>,
        secondary: Option<ColorU>,
        accent: Option<ColorU>,
        bg: Option<ColorU>,
        surface: Option<ColorU>,
    ) -> Self {
        self.primary = primary;
        self.secondary = secondary;
        self.custom_accent = accent;
        self.bg = bg;
        self.surface = surface;
        self
    }

    /// Resolve the accent color to a concrete [`ColorU`].
    pub fn accent_color(&self) -> ColorU {
        self.accent.color()
    }

    /// The palette a terminal paints with: the theme's own text, background and
    /// caret colours over the standard sixteen ANSI colours, so a shell in a
    /// pane agrees with the window it is in. The palette's own sixteen are the
    /// `xterm` defaults — the theme does not define terminal colours — while the
    /// three defaults it does define are taken from the theme.
    pub fn terminal_palette(&self) -> goble_terminal::Palette {
        let text = self.color(ColorToken::Text);
        let background = self.color(ColorToken::Bg);
        let cursor = self.color(ColorToken::Accent);
        goble_terminal::Palette::xterm().with_defaults(
            (text.r, text.g, text.b),
            (background.r, background.g, background.b),
            (cursor.r, cursor.g, cursor.b),
        )
    }

    /// Resolve a named color token.
    pub fn color(&self, token: ColorToken) -> ColorU {
        match token {
            ColorToken::Bg => self.bg.unwrap_or(self.colors.bg),
            ColorToken::Surface => self.surface.unwrap_or(self.colors.surface),
            ColorToken::SurfaceRaised => self.colors.surface_raised,
            ColorToken::Border => self.colors.border,
            ColorToken::Text => self.primary.unwrap_or(self.colors.text),
            ColorToken::Muted => self.secondary.unwrap_or(self.colors.muted),
            ColorToken::Hover => self.colors.hover,
            ColorToken::Selected => self.colors.selected,
            ColorToken::Focus => self.colors.focus,
            ColorToken::Accent => self.custom_accent.unwrap_or_else(|| self.accent.color()),
            ColorToken::Selection => self
                .custom_accent
                .unwrap_or_else(|| self.accent.color())
                .mix(&self.color(ColorToken::Bg), SELECTION_BG_MIX),
            ColorToken::Success => self.colors.success,
            ColorToken::Warning => self.colors.warning,
            ColorToken::Path => self.colors.path,
            ColorToken::Command => self.colors.command,
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

/// A ready-made palette the Appearance page offers beside the Default base.
///
/// The colours are grok-build's pager themes
/// (`crates/codegen/xai-grok-pager-render/src/theme/`): the same palettes
/// (`groknight.rs`, `grokday.rs`, `tokyonight.rs`, `rosepine.rs`,
/// `oscura.rs`), mapped onto Goble's [`ColorScheme`]. The accent is the
/// palette's own assistant/system accent, so a preset looks like it does in
/// grok. Picking one clears the five channel overrides; the wheel can then
/// override any channel it edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemePreset {
    GobleNight,
    GobleDay,
    TokyoNight,
    RosePineMoon,
    OscuraMidnight,
}

impl ThemePreset {
    /// The presets, in the order the Themes stepper walks them.
    pub const ALL: [ThemePreset; 5] = [
        ThemePreset::GobleNight,
        ThemePreset::GobleDay,
        ThemePreset::TokyoNight,
        ThemePreset::RosePineMoon,
        ThemePreset::OscuraMidnight,
    ];

    /// The key `[theme] preset` stores.
    pub const fn key(self) -> &'static str {
        match self {
            ThemePreset::GobleNight => "goble-night",
            ThemePreset::GobleDay => "goble-day",
            ThemePreset::TokyoNight => "tokyonight",
            ThemePreset::RosePineMoon => "rosepine-moon",
            ThemePreset::OscuraMidnight => "oscura-midnight",
        }
    }

    /// Read a stored key back. An unknown key is not a preset, so the caller
    /// keeps its Default base rather than failing the whole config.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            // The first spelling the two Goble palettes shipped with, kept so
            // a config written before the rename still resolves.
            "groknight" => Some(Self::GobleNight),
            "grokday" => Some(Self::GobleDay),
            _ => Self::ALL.into_iter().find(|preset| preset.key() == key),
        }
    }

    /// The name the selector shows.
    pub const fn display_name(self) -> &'static str {
        match self {
            ThemePreset::GobleNight => "Goble Night",
            ThemePreset::GobleDay => "Goble Day",
            ThemePreset::TokyoNight => "Tokyo Night",
            ThemePreset::RosePineMoon => "Rose Pine Moon",
            ThemePreset::OscuraMidnight => "Oscura Midnight",
        }
    }

    /// One line saying what the palette looks like.
    pub const fn description(self) -> &'static str {
        match self {
            ThemePreset::GobleNight => "Neutral gray, Tokyo Night accents",
            ThemePreset::GobleDay => "The same accents on a light gray base",
            ThemePreset::TokyoNight => "Deep navy with blue, violet and teal",
            ThemePreset::RosePineMoon => "Muted rose, iris and pine",
            ThemePreset::OscuraMidnight => "Near-black with purple accents",
        }
    }

    /// The base colours, read from the palette constants named in each arm.
    pub fn colors(self) -> ColorScheme {
        match self {
            // groknight.rs: BG_STORM, bg_dark, BG_HIGHLIGHT; the TokyoNight
            // Night accents grok pairs with its neutral gray ramp.
            ThemePreset::GobleNight => ColorScheme {
                bg: ColorU::hex(0x141414),
                surface: ColorU::hex(0x1c1c1c),
                surface_raised: ColorU::hex(0x242424),
                border: ColorU::hex(0x323237),
                text: ColorU::hex(0xe1e1e1),
                muted: ColorU::hex(0x6c6c6c),
                hover: ColorU::hex(0x2c2c2c),
                selected: ColorU::hex(0x363636),
                focus: ColorU::hex(0x7aa2f7),
                success: ColorU::hex(0x9ece6a),
                warning: ColorU::hex(0xe0af68),
                path: ColorU::hex(0xff9e64),
                command: ColorU::hex(0xe0af68),
                error: ColorU::hex(0xf7768e),
                badge: ColorU::hex(0xbb9af7),
                diff_insert_bg: ColorU::hex(0x063806),
                diff_delete_bg: ColorU::hex(0x420e14),
            },
            // grokday.rs: the light counterpart, same hue family deepened for
            // contrast on #eeeeee.
            ThemePreset::GobleDay => ColorScheme {
                bg: ColorU::hex(0xeeeeee),
                surface: ColorU::hex(0xe4e4e4),
                surface_raised: ColorU::hex(0xdedede),
                border: ColorU::hex(0xc8c8cd),
                text: ColorU::hex(0x262626),
                muted: ColorU::hex(0x767676),
                hover: ColorU::hex(0xd0d0d0),
                selected: ColorU::hex(0xc6c6c6),
                focus: ColorU::hex(0x2f64d2),
                success: ColorU::hex(0x378e23),
                warning: ColorU::hex(0xa27612),
                path: ColorU::hex(0xc3691e),
                command: ColorU::hex(0xa27612),
                error: ColorU::hex(0xcd3048),
                badge: ColorU::hex(0x7d4bc6),
                diff_insert_bg: ColorU::hex(0xdaf2dc),
                diff_delete_bg: ColorU::hex(0xf5dade),
            },
            // tokyonight.rs: Night `BG` under the Storm `BG_STORM` base, with
            // the Storm highlight as the raised band.
            ThemePreset::TokyoNight => ColorScheme {
                bg: ColorU::hex(0x1a1b26),
                surface: ColorU::hex(0x24283b),
                surface_raised: ColorU::hex(0x292e42),
                border: ColorU::hex(0x3c4b78),
                text: ColorU::hex(0xc0caf5),
                muted: ColorU::hex(0x565f89),
                hover: ColorU::hex(0x28314c),
                selected: ColorU::hex(0x283457),
                focus: ColorU::hex(0x7aa2f7),
                success: ColorU::hex(0x9ece6a),
                warning: ColorU::hex(0xe0af68),
                path: ColorU::hex(0xff9e64),
                command: ColorU::hex(0xe0af68),
                error: ColorU::hex(0xf7768e),
                badge: ColorU::hex(0xbb9af7),
                diff_insert_bg: ColorU::hex(0x0f4114),
                diff_delete_bg: ColorU::hex(0x550f14),
            },
            // rosepine.rs: BASE, SURFACE, OVERLAY; the two highlight steps are
            // the hover and selection bands.
            ThemePreset::RosePineMoon => ColorScheme {
                bg: ColorU::hex(0x232136),
                surface: ColorU::hex(0x2a273f),
                surface_raised: ColorU::hex(0x393552),
                border: ColorU::hex(0x44415a),
                text: ColorU::hex(0xe0def4),
                muted: ColorU::hex(0x6e6a86),
                hover: ColorU::hex(0x44415a),
                selected: ColorU::hex(0x56526e),
                focus: ColorU::hex(0x3e8fb0),
                success: ColorU::hex(0x9ccfd8),
                warning: ColorU::hex(0xf6c177),
                path: ColorU::hex(0xea9a97),
                command: ColorU::hex(0xf6c177),
                error: ColorU::hex(0xeb6f92),
                badge: ColorU::hex(0xc4a7e7),
                diff_insert_bg: ColorU::hex(0x192d37),
                diff_delete_bg: ColorU::hex(0x371e28),
            },
            // oscura.rs: BASE is a hair under SURFACE, so the raised bands use
            // ELEVATED and the purple-tinted highlight ramp.
            ThemePreset::OscuraMidnight => ColorScheme {
                bg: ColorU::hex(0x030304),
                surface: ColorU::hex(0x0f1216),
                surface_raised: ColorU::hex(0x242034),
                border: ColorU::hex(0x343048),
                text: ColorU::hex(0xe4e4e4),
                muted: ColorU::hex(0x81868f),
                hover: ColorU::hex(0x242034),
                selected: ColorU::hex(0x343048),
                focus: ColorU::hex(0x7dcfdf),
                success: ColorU::hex(0x50b48c),
                warning: ColorU::hex(0xebd96e),
                path: ColorU::hex(0xf1bd00),
                command: ColorU::hex(0xebd96e),
                error: ColorU::hex(0xdc5a64),
                badge: ColorU::hex(0xc4a7e7),
                diff_insert_bg: ColorU::hex(0x0a231e),
                diff_delete_bg: ColorU::hex(0x2d0f19),
            },
        }
    }

    /// The palette's accent, which is also its badge hue in every arm.
    pub const fn accent(self) -> ColorU {
        match self {
            ThemePreset::GobleNight => ColorU::hex(0xbb9af7),
            ThemePreset::GobleDay => ColorU::hex(0x7d4bc6),
            ThemePreset::TokyoNight => ColorU::hex(0x7aa2f7),
            ThemePreset::RosePineMoon => ColorU::hex(0xc4a7e7),
            ThemePreset::OscuraMidnight => ColorU::hex(0x9b7ece),
        }
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
    /// File paths on a tool row, kept apart from `warning` so a path reads in
    /// the pager's orange rather than the UI's amber.
    pub path: ColorU,
    /// Shell commands on a tool row, in the pager's yellow.
    pub command: ColorU,
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
    /// A text selection's band: the accent mixed toward the background, so a
    /// selected line reads as a tint under its glyphs rather than as a solid
    /// accent strip. The glyphs over it are still repainted in the background
    /// colour by the surfaces that paint a selection.
    Selection,
    Success,
    Warning,
    /// A tool row's file path — grok-build's warm orange (`theme.path`).
    Path,
    /// A tool row's shell command — grok-build's yellow (`theme.command`).
    Command,
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
    /// A preset's own accent, carried from its palette rather than one of the
    /// four named colours the Settings page offers.
    Custom(ColorU),
}

impl AccentColor {
    pub fn color(&self) -> ColorU {
        match self {
            // Neutral gray: the UI is intentionally blue-free (gray/black tints).
            AccentColor::Blue => ColorU::hex(0x9a9a9a),
            AccentColor::Green => ColorU::hex(0x10b981),
            AccentColor::Purple => ColorU::hex(0x8b5cf6),
            AccentColor::Orange => ColorU::hex(0xf97316),
            AccentColor::Custom(color) => *color,
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

    /// A selection band is a tint, not a solid accent strip: the accent pulled
    /// 55% of the way toward the background, and still a step away from both so
    /// the band and the inverted glyphs over it stay apart.
    #[test]
    fn selection_is_a_desaturated_accent() {
        for theme in [Theme::dark(), Theme::light(), Theme::midnight()] {
            let accent = theme.color(ColorToken::Accent);
            let bg = theme.color(ColorToken::Bg);
            let band = theme.color(ColorToken::Selection);
            assert_eq!(
                band,
                accent.mix(&bg, 0.55),
                "{}: the band is the accent mixed toward the background",
                theme.name
            );
            assert_ne!(band, accent, "{}: not the accent itself", theme.name);
            assert_ne!(band, bg, "{}: not the background", theme.name);
        }
    }

    /// A custom accent tints the selection too, the same way it tints the accent.
    #[test]
    fn a_custom_accent_tints_the_selection() {
        let accent = ColorU::hex(0x8b5cf6);
        let theme = Theme::dark().with_overrides(None, None, Some(accent), None, None);
        let bg = theme.color(ColorToken::Bg);
        assert_eq!(theme.color(ColorToken::Selection), accent.mix(&bg, 0.55));
    }

    /// Every preset resolves a whole palette: its own background, surface, text
    /// and accent, and the stored key reads back.
    #[test]
    fn each_preset_carries_its_own_palette() {
        for preset in ThemePreset::ALL {
            let theme = Theme::preset(preset);
            assert_eq!(theme.name, preset.key());
            assert_eq!(theme.color(ColorToken::Bg), preset.colors().bg);
            assert_eq!(theme.color(ColorToken::Surface), preset.colors().surface);
            assert_eq!(theme.color(ColorToken::Text), preset.colors().text);
            assert_eq!(theme.color(ColorToken::Accent), preset.accent());
            assert_eq!(ThemePreset::from_key(preset.key()), Some(preset));
        }
        assert_eq!(ThemePreset::from_key("no-such-theme"), None);
    }

    /// No two presets share a background, surface, text or accent, so the
    /// selector's previews cannot be mistaken for one another.
    #[test]
    fn the_presets_are_distinct() {
        for (i, a) in ThemePreset::ALL.iter().enumerate() {
            for b in &ThemePreset::ALL[i + 1..] {
                for (token, name) in [
                    (ColorToken::Bg, "background"),
                    (ColorToken::Surface, "surface"),
                    (ColorToken::Text, "text"),
                    (ColorToken::Accent, "accent"),
                ] {
                    assert_ne!(
                        Theme::preset(*a).color(token),
                        Theme::preset(*b).color(token),
                        "{} and {} share a {name}",
                        a.key(),
                        b.key()
                    );
                }
            }
        }
    }

    /// The two background channels take the same override path as the text and
    /// accent ones, and a selection band mixes against the overridden background.
    #[test]
    fn a_background_and_surface_override_reach_their_tokens() {
        let bg = ColorU::hex(0x101020);
        let surface = ColorU::hex(0x202030);
        let theme = Theme::preset(ThemePreset::TokyoNight).with_overrides(
            None,
            None,
            None,
            Some(bg),
            Some(surface),
        );
        assert_eq!(theme.color(ColorToken::Bg), bg);
        assert_eq!(theme.color(ColorToken::Surface), surface);
        assert_eq!(
            theme.color(ColorToken::Selection),
            theme.color(ColorToken::Accent).mix(&bg, 0.55)
        );
        assert_eq!(
            theme.color(ColorToken::SurfaceRaised),
            ThemePreset::TokyoNight.colors().surface_raised,
            "the raised band follows the surface, not the override"
        );
    }

    /// A preset is a base: an override that names no accent leaves the preset's
    /// own accent in place.
    #[test]
    fn an_empty_override_keeps_the_preset_accent() {
        let accent = ThemePreset::RosePineMoon.accent();
        let theme =
            Theme::preset(ThemePreset::RosePineMoon).with_overrides(None, None, None, None, None);
        assert_eq!(theme.color(ColorToken::Accent), accent);
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
