use crate::color::ColorU;

/// How much of the background a selection band carries. The accent at full
/// strength is an opaque band that swamps the glyphs it covers, so the band is
/// the accent pulled this far toward the background: desaturated, but still a
/// step away from the surface it sits on.
const SELECTION_BG_MIX: f32 = 0.55;

// The §2.2 light/dark values for the tokens that are not part of a palette's
// own ramp. The presets reuse these (or, where a palette's surface is not the
// default one, a value adjusted just enough to clear the 3:1 control gate).
const PLACEHOLDER_LIGHT: ColorU = ColorU::hex(0x696969);
const PLACEHOLDER_DARK: ColorU = ColorU::hex(0x8e8e8e);
const INPUT_BORDER_LIGHT: ColorU = ColorU::hex(0x8a8a8a);
const INPUT_BORDER_DARK: ColorU = ColorU::hex(0x6b6b6b);
/// Goble Day's surface is `#e4e4e4`, where `#8a8a8a` is 2.72:1. One step
/// darker (`#7f7f7f`, 3.15:1) clears the 3:1 control gate without changing the
/// palette.
const INPUT_BORDER_GOBLE_DAY: ColorU = ColorU::hex(0x7f7f7f);
/// Tokyo Night's surface is `#24283b` and Rose Pine Moon's is `#2a273f`; the
/// neutral dark input border `#6b6b6b` is 2.73:1 and 2.70:1 on them. The next
/// neutral step up, `#787878`, is 3.30:1 and 3.26:1.
const INPUT_BORDER_TINTED_DARK: ColorU = ColorU::hex(0x787878);
const BORDER_STRONG_LIGHT: ColorU = ColorU::hex(0x6b6b6b);
const BORDER_STRONG_DARK: ColorU = ColorU::hex(0x8e8e8e);
const INFO_LIGHT: ColorU = ColorU::hex(0x3b82f6);
const INFO_DARK: ColorU = ColorU::hex(0x60a5fa);
const INFO_FOREGROUND_LIGHT: ColorU = ColorU::hex(0x1d4ed8);
const INFO_FOREGROUND_DARK: ColorU = ColorU::hex(0x93c5fd);
const SUCCESS_FOREGROUND_LIGHT: ColorU = ColorU::hex(0x047857);
const SUCCESS_FOREGROUND_DARK: ColorU = ColorU::hex(0x34d399);
const WARNING_FOREGROUND_LIGHT: ColorU = ColorU::hex(0xb45309);
const WARNING_FOREGROUND_DARK: ColorU = ColorU::hex(0xfbbf24);
const ERROR_FOREGROUND_LIGHT: ColorU = ColorU::hex(0xb91c1c);
const ERROR_FOREGROUND_DARK: ColorU = ColorU::hex(0xf87171);

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
    /// How animations should be run. `Motion::Reduced` collapses every
    /// duration to zero in [`Theme::motion_ms`]; no component consumes it yet.
    pub motion: Motion,
    /// The interface contrast mode. `Contrast::High` substitutes
    /// [`ColorToken::BorderStrong`] for [`ColorToken::Border`], raises `Muted`
    /// to `Text`, and widens focus rings — see [`Theme::border_color`] and
    /// [`Theme::focus_width`].
    pub contrast: Contrast,
    /// Multiplier for the type scale, 1.0 by default. The appearance setting
    /// moves it over 0.75–1.25 (12–20px at a 16px root); [`Theme::type_px`]
    /// applies it.
    pub text_scale: f32,
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
                placeholder: PLACEHOLDER_DARK,
                input_border: INPUT_BORDER_DARK,
                border_strong: BORDER_STRONG_DARK,
                info: INFO_DARK,
                info_foreground: INFO_FOREGROUND_DARK,
                success_foreground: SUCCESS_FOREGROUND_DARK,
                warning_foreground: WARNING_FOREGROUND_DARK,
                error_foreground: ERROR_FOREGROUND_DARK,
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
            motion: Motion::Full,
            contrast: Contrast::Normal,
            text_scale: 1.0,
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
                muted: ColorU::hex(0x696969),
                hover: ColorU::hex(0xf0f0f0),
                selected: ColorU::hex(0xe2e2e2),
                focus: ColorU::hex(0x007a9e),
                success: ColorU::hex(0x10b981),
                warning: ColorU::hex(0xf59e0b),
                path: ColorU::hex(0xff9e64),
                command: ColorU::hex(0xe0af68),
                error: ColorU::hex(0xef4444),
                badge: ColorU::hex(0xe01e5a),
                diff_insert_bg: ColorU::hex(0xd6f5e0),
                diff_delete_bg: ColorU::hex(0xfbdada),
                placeholder: PLACEHOLDER_LIGHT,
                input_border: INPUT_BORDER_LIGHT,
                border_strong: BORDER_STRONG_LIGHT,
                info: INFO_LIGHT,
                info_foreground: INFO_FOREGROUND_LIGHT,
                success_foreground: SUCCESS_FOREGROUND_LIGHT,
                warning_foreground: WARNING_FOREGROUND_LIGHT,
                error_foreground: ERROR_FOREGROUND_LIGHT,
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
            motion: Motion::Full,
            contrast: Contrast::Normal,
            text_scale: 1.0,
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
                placeholder: PLACEHOLDER_DARK,
                input_border: INPUT_BORDER_DARK,
                border_strong: BORDER_STRONG_DARK,
                info: INFO_DARK,
                info_foreground: INFO_FOREGROUND_DARK,
                success_foreground: SUCCESS_FOREGROUND_DARK,
                warning_foreground: WARNING_FOREGROUND_DARK,
                error_foreground: ERROR_FOREGROUND_DARK,
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
            motion: Motion::Full,
            contrast: Contrast::Normal,
            text_scale: 1.0,
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
            motion: Motion::Full,
            contrast: Contrast::Normal,
            text_scale: 1.0,
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
            ColorToken::BorderStrong => self.colors.border_strong,
            ColorToken::Text => self.primary.unwrap_or(self.colors.text),
            // High contrast raises the muted foreground to the body text; the
            // same substitution the spec's `Contrast::High` describes.
            ColorToken::Muted => match self.contrast {
                Contrast::Normal => self.secondary.unwrap_or(self.colors.muted),
                Contrast::High => self.primary.unwrap_or(self.colors.text),
            },
            ColorToken::Placeholder => self.colors.placeholder,
            ColorToken::InputBorder => self.colors.input_border,
            ColorToken::Hover => self.colors.hover,
            ColorToken::Selected => self.colors.selected,
            ColorToken::Focus => self.colors.focus,
            ColorToken::Accent => self.custom_accent.unwrap_or_else(|| self.accent.color()),
            ColorToken::AccentForeground => {
                let accent = self.color(ColorToken::Accent);
                let white = ColorU::hex(0xffffff);
                let black = ColorU::hex(0x000000);
                if white.contrast_ratio(&accent) >= black.contrast_ratio(&accent) {
                    white
                } else {
                    black
                }
            }
            ColorToken::Selection => self
                .custom_accent
                .unwrap_or_else(|| self.accent.color())
                .mix(&self.color(ColorToken::Bg), SELECTION_BG_MIX),
            // A static quiet fill: half way from the surface toward the hover
            // band, so it is distinct from `Hover`, which stays a state.
            ColorToken::SurfaceMuted => self
                .color(ColorToken::Surface)
                .mix(&self.color(ColorToken::Hover), 0.5),
            // No blur in a wgpu renderer, so the 60% background is the whole
            // backdrop effect. The alpha is carried on the colour.
            ColorToken::Scrim => {
                let bg = self.color(ColorToken::Bg);
                ColorU::new(bg.r, bg.g, bg.b, 153)
            }
            ColorToken::Info => self.colors.info,
            ColorToken::InfoForeground => self.colors.info_foreground,
            ColorToken::Success => self.colors.success,
            ColorToken::SuccessForeground => self.colors.success_foreground,
            ColorToken::Warning => self.colors.warning,
            ColorToken::WarningForeground => self.colors.warning_foreground,
            ColorToken::Path => self.colors.path,
            ColorToken::Command => self.colors.command,
            ColorToken::Error => self.colors.error,
            ColorToken::ErrorForeground => self.colors.error_foreground,
            ColorToken::Badge => self.colors.badge,
            ColorToken::DiffInsertBg => self.colors.diff_insert_bg,
            ColorToken::DiffDeleteBg => self.colors.diff_delete_bg,
        }
    }

    /// The 1px rule width used by separators and control outlines.
    pub const fn border_width(&self) -> f32 {
        1.0
    }

    /// The rule colour: `BorderStrong` in high contrast, `Border` otherwise.
    pub fn border_color(&self) -> ColorU {
        match self.contrast {
            Contrast::Normal => self.color(ColorToken::Border),
            Contrast::High => self.color(ColorToken::BorderStrong),
        }
    }

    /// The resolved duration for a token from the [`Motion`] vocabulary. A
    /// reduced-motion theme returns zero for every duration, including the
    /// 200ms surface transition in the D1 acceptance test.
    pub fn motion_ms(&self, base_ms: u32) -> u32 {
        self.motion.duration_ms(base_ms)
    }

    /// The resolved corner radius for a T3 radius token.
    pub const fn radius_token(&self, token: RadiusToken) -> f32 {
        token.px()
    }

    /// The resolved pixel size for a type token, multiplied by the theme's
    /// text scale (1.0 by default; the appearance setting moves it 0.75–1.25).
    pub fn type_px(&self, token: TypeToken) -> f32 {
        token.px() * self.text_scale
    }

    /// The focus-ring width for a style, widened to 3px in high contrast.
    pub fn focus_width(&self, style: FocusStyle) -> f32 {
        match self.contrast {
            Contrast::Normal => style.width(),
            Contrast::High => 3.0,
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
                placeholder: PLACEHOLDER_DARK,
                input_border: INPUT_BORDER_DARK,
                border_strong: BORDER_STRONG_DARK,
                info: INFO_DARK,
                info_foreground: INFO_FOREGROUND_DARK,
                success_foreground: SUCCESS_FOREGROUND_DARK,
                warning_foreground: WARNING_FOREGROUND_DARK,
                error_foreground: ERROR_FOREGROUND_DARK,
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
                placeholder: PLACEHOLDER_LIGHT,
                input_border: INPUT_BORDER_GOBLE_DAY,
                border_strong: BORDER_STRONG_LIGHT,
                info: INFO_LIGHT,
                info_foreground: INFO_FOREGROUND_LIGHT,
                success_foreground: SUCCESS_FOREGROUND_LIGHT,
                warning_foreground: WARNING_FOREGROUND_LIGHT,
                error_foreground: ERROR_FOREGROUND_LIGHT,
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
                placeholder: PLACEHOLDER_DARK,
                input_border: INPUT_BORDER_TINTED_DARK,
                border_strong: BORDER_STRONG_DARK,
                info: INFO_DARK,
                info_foreground: INFO_FOREGROUND_DARK,
                success_foreground: SUCCESS_FOREGROUND_DARK,
                warning_foreground: WARNING_FOREGROUND_DARK,
                error_foreground: ERROR_FOREGROUND_DARK,
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
                placeholder: PLACEHOLDER_DARK,
                input_border: INPUT_BORDER_TINTED_DARK,
                border_strong: BORDER_STRONG_DARK,
                info: INFO_DARK,
                info_foreground: INFO_FOREGROUND_DARK,
                success_foreground: SUCCESS_FOREGROUND_DARK,
                warning_foreground: WARNING_FOREGROUND_DARK,
                error_foreground: ERROR_FOREGROUND_DARK,
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
                placeholder: PLACEHOLDER_DARK,
                input_border: INPUT_BORDER_DARK,
                border_strong: BORDER_STRONG_DARK,
                info: INFO_DARK,
                info_foreground: INFO_FOREGROUND_DARK,
                success_foreground: SUCCESS_FOREGROUND_DARK,
                warning_foreground: WARNING_FOREGROUND_DARK,
                error_foreground: ERROR_FOREGROUND_DARK,
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
    /// `accent` so the neutral UI accent stays neutral. Dark is warp-new's
    /// `#19aad8`; light is the §2.2 contrast-corrected `#007a9e` (the old
    /// `#00c2ff` was only 2.07:1 on white).
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
    /// Empty-field text: distinct from `Muted` so a placeholder can drift on
    /// its own. The §2.2 values track the corrected muted foreground at first.
    pub placeholder: ColorU,
    /// The only boundary of a field/select, at 3:1 against the surface. The
    /// ordinary `border` is a 1.3:1 separator and is not strong enough.
    pub input_border: ColorU,
    /// The boosted border used by high contrast.
    pub border_strong: ColorU,
    /// The fourth semantic fill, for harness status rows.
    pub info: ColorU,
    /// Text on info tints.
    pub info_foreground: ColorU,
    /// Text on success tints.
    pub success_foreground: ColorU,
    /// Text on warning tints.
    pub warning_foreground: ColorU,
    /// Text on error tints; the fill stays `error`.
    pub error_foreground: ColorU,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorToken {
    Bg,
    Surface,
    SurfaceRaised,
    Border,
    /// The high-contrast rule colour (`#6b6b6b` light, `#8e8e8e` dark).
    BorderStrong,
    Text,
    Muted,
    /// Empty-field text, distinct from `Muted`.
    Placeholder,
    /// The only boundary of a field/select; 3:1 against the surface.
    InputBorder,
    Hover,
    Selected,
    /// The focused text field's caret and ring — see `ColorScheme::focus`.
    Focus,
    Accent,
    /// Text/glyphs drawn on `Accent`; white or black, whichever reads better.
    AccentForeground,
    /// A text selection's band: the accent mixed toward the background, so a
    /// selected line reads as a tint under its glyphs rather than as a solid
    /// accent strip. The glyphs over it are still repainted in the background
    /// colour by the surfaces that paint a selection.
    Selection,
    /// A static quiet fill: `Surface.mix(Hover, 0.5)`.
    SurfaceMuted,
    /// `Bg` at 60% alpha; there is no blur in the renderer.
    Scrim,
    Info,
    /// Text on info tints.
    InfoForeground,
    Success,
    /// Text on success tints.
    SuccessForeground,
    Warning,
    /// Text on warning tints.
    WarningForeground,
    /// A tool row's file path — grok-build's warm orange (`theme.path`).
    Path,
    /// A tool row's shell command — grok-build's yellow (`theme.command`).
    Command,
    Error,
    /// Text on error tints; the fill is `Error`.
    ErrorForeground,
    Badge,
    DiffInsertBg,
    DiffDeleteBg,
}

impl ColorToken {
    /// Every token, in declaration order. The resolver's exhaustive `match`
    /// is the compile-time guarantee; this list is what the resolution and
    /// contrast tests walk.
    pub const ALL: [ColorToken; 30] = [
        ColorToken::Bg,
        ColorToken::Surface,
        ColorToken::SurfaceRaised,
        ColorToken::Border,
        ColorToken::BorderStrong,
        ColorToken::Text,
        ColorToken::Muted,
        ColorToken::Placeholder,
        ColorToken::InputBorder,
        ColorToken::Hover,
        ColorToken::Selected,
        ColorToken::Focus,
        ColorToken::Accent,
        ColorToken::AccentForeground,
        ColorToken::Selection,
        ColorToken::SurfaceMuted,
        ColorToken::Scrim,
        ColorToken::Info,
        ColorToken::InfoForeground,
        ColorToken::Success,
        ColorToken::SuccessForeground,
        ColorToken::Warning,
        ColorToken::WarningForeground,
        ColorToken::Path,
        ColorToken::Command,
        ColorToken::Error,
        ColorToken::ErrorForeground,
        ColorToken::Badge,
        ColorToken::DiffInsertBg,
        ColorToken::DiffDeleteBg,
    ];
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

/// T3's derived radius scale: 4, 6, 8, 12, 16. `Theme::radius_token` resolves
/// one to pixels. The legacy [`Radius`] presets stay for compatibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RadiusToken {
    Sm,
    Md,
    Lg,
    Xl,
    Xxl,
}

impl RadiusToken {
    pub const fn px(self) -> f32 {
        match self {
            RadiusToken::Sm => 4.0,
            RadiusToken::Md => 6.0,
            RadiusToken::Lg => 8.0,
            RadiusToken::Xl => 12.0,
            RadiusToken::Xxl => 16.0,
        }
    }
}

/// T3's type scale, in CSS pixels at a 1.0 text scale. `Theme::type_px`
/// multiplies the value by the theme's [`Theme::text_scale`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeToken {
    Micro,
    Caption,
    Body,
    Label,
    Title,
    Heading,
    Display,
}

impl TypeToken {
    pub const fn px(self) -> f32 {
        match self {
            TypeToken::Micro => 10.0,
            TypeToken::Caption => 11.0,
            TypeToken::Body => 12.0,
            TypeToken::Label => 14.0,
            TypeToken::Title => 16.0,
            TypeToken::Heading => 20.0,
            TypeToken::Display => 24.0,
        }
    }
}

/// The three focus/border treatments from §2.2. Widths are in pixels, offsets
/// are negative because the ring sits inside the control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FocusStyle {
    /// 2px inset, `Focus`.
    Control,
    /// 3px inset, `Focus`, replacing the field's border colour.
    Field,
    /// 2px full-width tab edge: `Border` at rest, `Accent` selected.
    TabEdge,
}

impl FocusStyle {
    pub const fn width(self) -> f32 {
        match self {
            FocusStyle::Control => 2.0,
            FocusStyle::Field => 3.0,
            FocusStyle::TabEdge => 2.0,
        }
    }

    /// The inset offset; `TabEdge` is a bottom edge rather than a ring, so it
    /// has no inward offset.
    pub const fn offset(self) -> f32 {
        match self {
            FocusStyle::Control => -2.0,
            FocusStyle::Field => -3.0,
            FocusStyle::TabEdge => 0.0,
        }
    }

    /// The ring colour. `tab_edge` uses [`FocusStyle::tab_edge_color`] because
    /// its colour depends on selection.
    pub fn color(self, theme: &Theme) -> ColorU {
        match self {
            FocusStyle::Control | FocusStyle::Field => theme.color(ColorToken::Focus),
            FocusStyle::TabEdge => theme.color(ColorToken::Border),
        }
    }

    /// The 2px tab edge: `Border` at rest, `Accent` when the tab is selected.
    pub fn tab_edge_color(theme: &Theme, selected: bool) -> ColorU {
        if selected {
            theme.color(ColorToken::Accent)
        } else {
            theme.color(ColorToken::Border)
        }
    }
}

/// The interface contrast mode. `High` substitutes `BorderStrong`, raises
/// `Muted` to `Text`, and widens focus rings to 3px.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Contrast {
    Normal,
    High,
}

impl Default for Contrast {
    fn default() -> Self {
        Contrast::Normal
    }
}

/// T3's motion vocabulary. `Reduced` returns zero for every duration; easing
/// values stay available because a zero-duration transition can still be
/// described without animating.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    Full,
    Reduced,
}

impl Motion {
    pub const FAST_MS: u32 = 100;
    pub const BASE_MS: u32 = 150;
    pub const SLOW_MS: u32 = 200;
    pub const DRAWER_MS: u32 = 300;

    /// `ease-out`: cubic-bezier(0.0, 0.0, 0.2, 1.0).
    pub const EASE_OUT: [f32; 4] = [0.0, 0.0, 0.2, 1.0];
    /// `--ease-drawer`: cubic-bezier(0.32, 0.72, 0.0, 1.0).
    pub const EASE_DRAWER: [f32; 4] = [0.32, 0.72, 0.0, 1.0];

    pub const fn is_reduced(self) -> bool {
        matches!(self, Motion::Reduced)
    }

    /// The spec's `Motion::reduced` spelling: `true` when durations collapse.
    pub const fn reduced(self) -> bool {
        self.is_reduced()
    }

    /// The effective duration for `base_ms`: zero under reduced motion,
    /// `base_ms` otherwise.
    pub const fn duration_ms(self, base_ms: u32) -> u32 {
        if self.is_reduced() {
            0
        } else {
            base_ms
        }
    }

    pub const fn fast_ms(self) -> u32 {
        self.duration_ms(Self::FAST_MS)
    }

    pub const fn base_ms(self) -> u32 {
        self.duration_ms(Self::BASE_MS)
    }

    pub const fn slow_ms(self) -> u32 {
        self.duration_ms(Self::SLOW_MS)
    }

    pub const fn drawer_ms(self) -> u32 {
        self.duration_ms(Self::DRAWER_MS)
    }

    pub const fn ease_out(self) -> [f32; 4] {
        Self::EASE_OUT
    }

    pub const fn ease_drawer(self) -> [f32; 4] {
        Self::EASE_DRAWER
    }
}

impl Default for Motion {
    fn default() -> Self {
        Motion::Full
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
    pub xxs: f32,
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
    pub xxl: f32,
    pub xxxl: f32,
}

impl Spacing {
    pub fn px(&self, token: SpacingToken) -> f32 {
        match token {
            SpacingToken::Xxs => self.xxs,
            SpacingToken::Xs => self.xs,
            SpacingToken::Sm => self.sm,
            SpacingToken::Md => self.md,
            SpacingToken::Lg => self.lg,
            SpacingToken::Xl => self.xl,
            SpacingToken::Xxl => self.xxl,
            SpacingToken::Xxxl => self.xxxl,
        }
    }
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            xxs: 2.0,
            xs: 4.0,
            sm: 8.0,
            md: 12.0,
            lg: 16.0,
            xl: 24.0,
            xxl: 32.0,
            xxxl: 48.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpacingToken {
    Xxs,
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
    Xxl,
    Xxxl,
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

    /// Every theme a host can build: the three named bases plus all five
    /// presets. The preset arms in `ThemePreset::colors` are exhaustive, so a
    /// new preset fails to compile until it supplies the new tokens too.
    fn all_themes() -> Vec<Theme> {
        let mut themes = vec![Theme::dark(), Theme::light(), Theme::midnight()];
        themes.extend(ThemePreset::ALL.into_iter().map(Theme::preset));
        themes
    }

    /// The resolver's `match` has no wildcard, so a new `ColorToken` is a
    /// compile error until it is resolved. This test then walks every token on
    /// every theme and checks the one structural invariant they share: only
    /// the scrim is translucent, everything else is opaque.
    #[test]
    fn every_theme_resolves_every_color_token() {
        assert_eq!(ColorToken::ALL.len(), 30);
        for theme in all_themes() {
            for token in ColorToken::ALL {
                let color = theme.color(token);
                let expected_alpha = if token == ColorToken::Scrim { 153 } else { 255 };
                assert_eq!(
                    color.a, expected_alpha,
                    "{}: {token:?} resolved with alpha {}",
                    theme.name, color.a
                );
            }
        }
    }

    /// 4.5:1 body text on the canvas in every theme, including the presets.
    #[test]
    fn text_reads_on_every_canvas() {
        for theme in all_themes() {
            let ratio = theme
                .color(ColorToken::Text)
                .contrast_ratio(&theme.color(ColorToken::Bg));
            assert!(
                ratio >= 4.5,
                "{}: Text on Bg is {ratio:.2}:1, needs 4.5:1",
                theme.name
            );
        }
    }

    /// The §2.3 correction is what lifts light `Muted` from 4.41:1 to 4.82:1 on
    /// `SurfaceRaised`; dark `#8e8e8e` is 5.09:1. Only the two spec-defined
    /// themes are gated here: the presets own their muted ramps, and four of
    /// the five are below 4.5:1 on their raised surface (Goble Night 2.96,
    /// Goble Day 3.38, Tokyo Night 2.17, Rose Pine Moon 2.25, Oscura 4.31,
    /// Midnight 3.50). Darkening them would change each palette's identity, so
    /// follow the D1 escape hatch and leave the gate on dark/light rather than
    /// silently weakening it.
    #[test]
    fn muted_reads_on_surface_raised_in_the_spec_themes() {
        for theme in [Theme::dark(), Theme::light()] {
            let ratio = theme
                .color(ColorToken::Muted)
                .contrast_ratio(&theme.color(ColorToken::SurfaceRaised));
            assert!(
                ratio >= 4.5,
                "{}: Muted on SurfaceRaised is {ratio:.2}:1, needs 4.5:1",
                theme.name
            );
        }
        assert_eq!(
            Theme::light().color(ColorToken::Muted),
            ColorU::hex(0x696969)
        );
        assert_eq!(
            Theme::dark().color(ColorToken::Muted),
            ColorU::hex(0x8e8e8e)
        );
    }

    /// The focus ring clears the 3:1 non-text minimum on every canvas, and the
    /// light default is exactly the §2.3 corrected `#007a9e` (4.91:1 on white,
    /// 4.31:1 on `#f0f0f0`), not the old `#00c2ff` (2.07:1 on white).
    #[test]
    fn focus_reads_on_every_canvas() {
        for theme in all_themes() {
            let ratio = theme
                .color(ColorToken::Focus)
                .contrast_ratio(&theme.color(ColorToken::Bg));
            assert!(
                ratio >= 3.0,
                "{}: Focus on Bg is {ratio:.2}:1, needs 3:1",
                theme.name
            );
        }
        assert_eq!(
            Theme::light().color(ColorToken::Focus),
            ColorU::hex(0x007a9e)
        );
    }

    /// `InputBorder` is the only boundary of a field, so it must clear 3:1 on
    /// the surface. Dark/light are the spec's exact `#6b6b6b` / `#8a8a8a`; the
    /// three presets whose surface defeats the neutral value get the smallest
    /// neutral step that passes (see the `INPUT_BORDER_*` constants).
    #[test]
    fn input_border_reads_on_every_surface() {
        for theme in all_themes() {
            let ratio = theme
                .color(ColorToken::InputBorder)
                .contrast_ratio(&theme.color(ColorToken::Surface));
            assert!(
                ratio >= 3.0,
                "{}: InputBorder on Surface is {ratio:.2}:1, needs 3:1",
                theme.name
            );
        }
        assert_eq!(
            Theme::dark().color(ColorToken::InputBorder),
            ColorU::hex(0x6b6b6b)
        );
        assert_eq!(
            Theme::light().color(ColorToken::InputBorder),
            ColorU::hex(0x8a8a8a)
        );
    }

    /// The light/dark foreground pairs are the §2.2 values exactly, and each
    /// reads at 4.5:1 or better on its own surface. The presets keep the same
    /// light or dark pair according to their palette.
    #[test]
    fn the_foreground_pairs_are_the_spec_values_and_read_on_surface() {
        let dark = Theme::dark();
        assert_eq!(dark.color(ColorToken::Info), ColorU::hex(0x60a5fa));
        assert_eq!(
            dark.color(ColorToken::InfoForeground),
            ColorU::hex(0x93c5fd)
        );
        assert_eq!(
            dark.color(ColorToken::SuccessForeground),
            ColorU::hex(0x34d399)
        );
        assert_eq!(
            dark.color(ColorToken::WarningForeground),
            ColorU::hex(0xfbbf24)
        );
        assert_eq!(
            dark.color(ColorToken::ErrorForeground),
            ColorU::hex(0xf87171)
        );

        let light = Theme::light();
        assert_eq!(light.color(ColorToken::Info), ColorU::hex(0x3b82f6));
        assert_eq!(
            light.color(ColorToken::InfoForeground),
            ColorU::hex(0x1d4ed8)
        );
        assert_eq!(
            light.color(ColorToken::SuccessForeground),
            ColorU::hex(0x047857)
        );
        assert_eq!(
            light.color(ColorToken::WarningForeground),
            ColorU::hex(0xb45309)
        );
        assert_eq!(
            light.color(ColorToken::ErrorForeground),
            ColorU::hex(0xb91c1c)
        );

        for theme in [dark, light] {
            for token in [
                ColorToken::InfoForeground,
                ColorToken::SuccessForeground,
                ColorToken::WarningForeground,
                ColorToken::ErrorForeground,
            ] {
                let ratio = theme
                    .color(token)
                    .contrast_ratio(&theme.color(ColorToken::Surface));
                assert!(
                    ratio >= 4.5,
                    "{}: {token:?} on Surface is {ratio:.2}:1, needs 4.5:1",
                    theme.name
                );
            }
        }
    }

    /// The neutral accent `#9a9a9a` reads better with black (7.46:1) than
    /// white (2.81:1), so `AccentForeground` resolves black. A very dark custom
    /// accent flips it to white. Choosing the better side always clears 4.5:1.
    #[test]
    fn accent_foreground_picks_the_readable_side() {
        assert_eq!(
            Theme::dark().color(ColorToken::AccentForeground),
            ColorU::hex(0x000000)
        );
        let dark_accent = ColorU::hex(0x101010);
        let theme = Theme::dark().with_overrides(None, None, Some(dark_accent), None, None);
        assert_eq!(
            theme.color(ColorToken::AccentForeground),
            ColorU::hex(0xffffff)
        );

        for theme in all_themes() {
            let ratio = theme
                .color(ColorToken::AccentForeground)
                .contrast_ratio(&theme.color(ColorToken::Accent));
            assert!(
                ratio >= 4.5,
                "{}: AccentForeground on Accent is {ratio:.2}:1, needs 4.5:1",
                theme.name
            );
        }
    }

    /// `SurfaceMuted` is a static fill, not a state: exactly `Surface` mixed
    /// half way toward `Hover` (light `#f8f8f8`, dark `#1a1a1a`).
    #[test]
    fn surface_muted_is_the_static_half_way_fill() {
        for theme in all_themes() {
            assert_eq!(
                theme.color(ColorToken::SurfaceMuted),
                theme
                    .color(ColorToken::Surface)
                    .mix(&theme.color(ColorToken::Hover), 0.5),
                "{}",
                theme.name
            );
        }
        assert_eq!(
            Theme::light().color(ColorToken::SurfaceMuted),
            ColorU::hex(0xf8f8f8)
        );
        assert_eq!(
            Theme::dark().color(ColorToken::SurfaceMuted),
            ColorU::hex(0x1a1a1a)
        );
    }

    /// The scrim is the background at 60% alpha (153/255), with no blur.
    #[test]
    fn scrim_is_the_background_at_sixty_percent() {
        for theme in all_themes() {
            let bg = theme.color(ColorToken::Bg);
            let scrim = theme.color(ColorToken::Scrim);
            assert_eq!(scrim, ColorU::new(bg.r, bg.g, bg.b, 153), "{}", theme.name);
        }
    }

    /// The border/strong pair is the spec's exact light/dark value. `High`
    /// swaps the rule to it, raises `Muted` to `Text` and widens focus.
    #[test]
    fn high_contrast_raises_muted_border_and_focus_width() {
        let normal = Theme::light();
        assert_eq!(normal.border_width(), 1.0);
        assert_eq!(
            normal.color(ColorToken::BorderStrong),
            ColorU::hex(0x6b6b6b)
        );
        assert_eq!(
            normal.border_color(),
            normal.color(ColorToken::Border),
            "normal contrast keeps the ordinary border"
        );
        assert_eq!(normal.focus_width(FocusStyle::Control), 2.0);
        assert_eq!(normal.focus_width(FocusStyle::Field), 3.0);

        let mut high = normal.clone();
        high.contrast = Contrast::High;
        assert_eq!(
            high.color(ColorToken::Muted),
            high.color(ColorToken::Text),
            "high contrast raises muted text to body text"
        );
        assert_eq!(high.border_color(), high.color(ColorToken::BorderStrong));
        assert_eq!(high.focus_width(FocusStyle::Control), 3.0);
        assert_eq!(high.focus_width(FocusStyle::TabEdge), 3.0);
    }

    /// The three focus treatments: 2px control, 3px field, 2px tab edge; the
    /// tab edge is `Border` at rest and `Accent` selected.
    #[test]
    fn focus_and_border_scales_are_the_t3_values() {
        assert_eq!(FocusStyle::Control.width(), 2.0);
        assert_eq!(FocusStyle::Control.offset(), -2.0);
        assert_eq!(FocusStyle::Field.width(), 3.0);
        assert_eq!(FocusStyle::Field.offset(), -3.0);
        assert_eq!(FocusStyle::TabEdge.width(), 2.0);

        let theme = Theme::dark();
        assert_eq!(
            FocusStyle::Control.color(&theme),
            theme.color(ColorToken::Focus)
        );
        assert_eq!(
            FocusStyle::Field.color(&theme),
            theme.color(ColorToken::Focus)
        );
        assert_eq!(
            FocusStyle::tab_edge_color(&theme, false),
            theme.color(ColorToken::Border)
        );
        assert_eq!(
            FocusStyle::tab_edge_color(&theme, true),
            theme.color(ColorToken::Accent)
        );
    }

    /// The four motion steps, their easing, and the reduced-motion rule that
    /// every duration collapses to zero (the D1 acceptance case: 200ms -> 0).
    #[test]
    fn motion_is_zero_when_reduced() {
        let mut theme = Theme::dark();
        assert_eq!(theme.motion_ms(200), 200);
        assert_eq!(Motion::Full.fast_ms(), 100);
        assert_eq!(Motion::Full.base_ms(), 150);
        assert_eq!(Motion::Full.slow_ms(), 200);
        assert_eq!(Motion::Full.drawer_ms(), 300);
        assert_eq!(Motion::Full.ease_out(), [0.0, 0.0, 0.2, 1.0]);
        assert_eq!(Motion::Full.ease_drawer(), [0.32, 0.72, 0.0, 1.0]);

        theme.motion = Motion::Reduced;
        assert_eq!(theme.motion_ms(200), 0);
        assert_eq!(theme.motion_ms(100), 0);
        assert_eq!(Motion::Reduced.fast_ms(), 0);
        assert_eq!(Motion::Reduced.base_ms(), 0);
        assert_eq!(Motion::Reduced.slow_ms(), 0);
        assert_eq!(Motion::Reduced.drawer_ms(), 0);
        assert!(Motion::Reduced.is_reduced());
        assert!(!Motion::Full.is_reduced());
    }

    /// The T3 radius and type scales, plus the text multiplier: 14px at 1.0
    /// and 17.5px at 1.25 (the 20px interface-size setting).
    #[test]
    fn radius_and_type_scales_are_the_t3_values() {
        let theme = Theme::dark();
        assert_eq!(theme.radius_token(RadiusToken::Sm), 4.0);
        assert_eq!(theme.radius_token(RadiusToken::Md), 6.0);
        assert_eq!(theme.radius_token(RadiusToken::Lg), 8.0);
        assert_eq!(theme.radius_token(RadiusToken::Xl), 12.0);
        assert_eq!(theme.radius_token(RadiusToken::Xxl), 16.0);

        assert_eq!(theme.type_px(TypeToken::Micro), 10.0);
        assert_eq!(theme.type_px(TypeToken::Caption), 11.0);
        assert_eq!(theme.type_px(TypeToken::Body), 12.0);
        assert_eq!(theme.type_px(TypeToken::Label), 14.0);
        assert_eq!(theme.type_px(TypeToken::Title), 16.0);
        assert_eq!(theme.type_px(TypeToken::Heading), 20.0);
        assert_eq!(theme.type_px(TypeToken::Display), 24.0);

        let mut scaled = theme;
        scaled.text_scale = 1.25;
        assert_eq!(scaled.type_px(TypeToken::Label), 17.5);
        assert_eq!(scaled.type_px(TypeToken::Body), 15.0);
    }

    /// The spacing scale keeps 4/8/12/16/24 and adds 2/32/48.
    #[test]
    fn spacing_has_the_added_steps() {
        let theme = Theme::dark();
        assert_eq!(theme.spacing_px(SpacingToken::Xxs), 2.0);
        assert_eq!(theme.spacing_px(SpacingToken::Xs), 4.0);
        assert_eq!(theme.spacing_px(SpacingToken::Sm), 8.0);
        assert_eq!(theme.spacing_px(SpacingToken::Md), 12.0);
        assert_eq!(theme.spacing_px(SpacingToken::Lg), 16.0);
        assert_eq!(theme.spacing_px(SpacingToken::Xl), 24.0);
        assert_eq!(theme.spacing_px(SpacingToken::Xxl), 32.0);
        assert_eq!(theme.spacing_px(SpacingToken::Xxxl), 48.0);
    }

    /// The light corrections the spec calls out, and the corresponding dark
    /// values, pinned by exact hex so a drift cannot pass silently.
    #[test]
    fn light_theme_corrections_are_exact() {
        let light = Theme::light();
        assert_eq!(light.color(ColorToken::Focus), ColorU::hex(0x007a9e));
        assert_eq!(light.color(ColorToken::Muted), ColorU::hex(0x696969));
        assert_eq!(light.color(ColorToken::Placeholder), ColorU::hex(0x696969));
        assert_eq!(light.color(ColorToken::InputBorder), ColorU::hex(0x8a8a8a));
        assert_eq!(light.color(ColorToken::BorderStrong), ColorU::hex(0x6b6b6b));

        let dark = Theme::dark();
        assert_eq!(dark.color(ColorToken::Focus), ColorU::hex(0x19aad8));
        assert_eq!(dark.color(ColorToken::Placeholder), ColorU::hex(0x8e8e8e));
        assert_eq!(dark.color(ColorToken::InputBorder), ColorU::hex(0x6b6b6b));
        assert_eq!(dark.color(ColorToken::BorderStrong), ColorU::hex(0x8e8e8e));
    }
}
