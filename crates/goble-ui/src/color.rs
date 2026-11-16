use palette::Srgba;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorU {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl ColorU {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_linear_f32(&self) -> [f32; 4] {
        let srgb = Srgba::new(
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        );
        let linear = srgb.into_linear();
        [linear.red, linear.green, linear.blue, linear.alpha]
    }

    pub fn to_u8_array(&self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

impl ColorU {
    /// Parse a `#rrggbb` (or `rrggbb`) hex string into an opaque color.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Self::new(r, g, b, 255))
    }

    /// The color as a lowercase `#rrggbb` string.
    pub fn to_hex_string(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Linear interpolation between two colors (opaque).
    pub fn mix(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self::new(
            lerp(self.r, other.r),
            lerp(self.g, other.g),
            lerp(self.b, other.b),
            255,
        )
    }

    /// Composite this colour over `base` at `alpha` (0..=1) and return an
    /// opaque result. `alpha` is the overlay's opacity; `self.a` is not used,
    /// because callers either have a token alpha (e.g. [`crate::theme`]'s
    /// `Scrim`, 153/255) or an animation value to hand, and the renderer has no
    /// blend pass to apply it later.
    pub fn mix_over(&self, base: &Self, alpha: f32) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        let lerp = |fg: u8, bg: u8| (fg as f32 * alpha + bg as f32 * (1.0 - alpha)).round() as u8;
        Self::new(
            lerp(self.r, base.r),
            lerp(self.g, base.g),
            lerp(self.b, base.b),
            255,
        )
    }

    /// WCAG 2.x relative luminance: linearise each sRGB channel, then weight
    /// red 0.2126, green 0.7152 and blue 0.0722. Alpha is ignored; composite a
    /// translucent colour with [`ColorU::mix_over`] first if it covers another
    /// surface.
    pub fn relative_luminance(&self) -> f32 {
        fn linear(channel: u8) -> f32 {
            let c = channel as f32 / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG 2.x contrast ratio, `(lighter + 0.05) / (darker + 0.05)`, in the
    /// range 1.0..=21.0. The arguments commute.
    pub fn contrast_ratio(&self, other: &Self) -> f32 {
        let a = self.relative_luminance();
        let b = other.relative_luminance();
        let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
        (lighter + 0.05) / (darker + 0.05)
    }
}

impl Default for ColorU {
    fn default() -> Self {
        Self::new(0, 0, 0, 255)
    }
}

/// Convert HSV (h in degrees 0..=360, s and v in 0..=1) to an opaque RGB color.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> ColorU {
    let h = (h.rem_euclid(360.0)) / 60.0;
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    ColorU::new(
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
        255,
    )
}

/// Convert an opaque RGB color to HSV (h in degrees 0..=360, s and v in 0..=1).
pub fn rgb_to_hsv(color: ColorU) -> (f32, f32, f32) {
    let r = color.r as f32 / 255.0;
    let g = color.g as f32 / 255.0;
    let b = color.b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let c = ColorU::from_hex("#10b981").expect("parse");
        assert_eq!(c, ColorU::new(0x10, 0xb9, 0x81, 255));
        assert_eq!(c.to_hex_string(), "#10b981");
    }

    #[test]
    fn from_hex_rejects_bad_len() {
        assert!(ColorU::from_hex("#abc").is_none());
        assert!(ColorU::from_hex("zzzzzz").is_none());
    }

    #[test]
    fn hsv_rgb_round_trips() {
        let s = 0.8;
        let v = 0.9;
        for h in [0.0, 120.0, 210.0, 300.0] {
            let c = hsv_to_rgb(h, s, v);
            let (h2, s2, v2) = rgb_to_hsv(c);
            assert!((h - h2).abs() < 2.0, "hue {h} vs {h2}");
            assert!((s - s2).abs() < 0.02, "sat {s} vs {s2}");
            assert!((v - v2).abs() < 0.02, "val {v} vs {v2}");
        }
    }

    #[test]
    fn mix_midpoint() {
        let a = ColorU::new(0, 0, 0, 255);
        let b = ColorU::new(255, 255, 255, 255);
        assert_eq!(a.mix(&b, 0.5), ColorU::new(128, 128, 128, 255));
    }

    /// White and black are the endpoints of the WCAG scale; the primaries pin
    /// the three channel weights. `#808080` is the hand-worked sRGB case:
    /// 128/255 = 0.5019608, ((0.5019608 + 0.055) / 1.055)^2.4 = 0.2158605.
    #[test]
    fn relative_luminance_matches_wcag_known_values() {
        let close = |actual: f32, expected: f32| {
            assert!(
                (actual - expected).abs() < 1e-6,
                "expected {expected}, got {actual}"
            );
        };
        close(ColorU::hex(0x000000).relative_luminance(), 0.0);
        close(ColorU::hex(0xffffff).relative_luminance(), 1.0);
        close(ColorU::hex(0xff0000).relative_luminance(), 0.2126);
        close(ColorU::hex(0x00ff00).relative_luminance(), 0.7152);
        close(ColorU::hex(0x0000ff).relative_luminance(), 0.0722);
        close(ColorU::hex(0x808080).relative_luminance(), 0.215_860_5);
    }

    /// Black on white is the maximum 21:1. `#767676` on white is
    /// (1.0 + 0.05) / (0.1811642 + 0.05) = 4.5422, and `#595959` on white is
    /// (1.0 + 0.05) / (0.0998987 + 0.05) = 7.0047. The ratio commutes.
    #[test]
    fn contrast_ratio_matches_wcag_known_pairs() {
        let white = ColorU::hex(0xffffff);
        let black = ColorU::hex(0x000000);
        assert!((white.contrast_ratio(&black) - 21.0).abs() < 1e-4);

        let grey = ColorU::hex(0x767676);
        assert!((grey.contrast_ratio(&white) - 4.542_225).abs() < 1e-5);
        assert!((white.contrast_ratio(&grey) - 4.542_225).abs() < 1e-5);

        let darker = ColorU::hex(0x595959);
        assert!((darker.contrast_ratio(&white) - 7.004_729).abs() < 1e-5);

        // The identity is 1.0, not zero: a colour always has some contrast
        // with itself.
        assert!((grey.contrast_ratio(&grey) - 1.0).abs() < 1e-6);
    }

    /// `mix_over` is an opaque composite: red over blue at 50% is
    /// (0.5*255, 0, 0.5*255) = (127.5, 0, 127.5), rounded to `#800080`.
    /// Black over white at 25% is 0.25*0 + 0.75*255 = 191.25 -> `#bfbfbf`.
    #[test]
    fn mix_over_composites_known_values() {
        let red = ColorU::hex(0xff0000);
        let blue = ColorU::hex(0x0000ff);
        assert_eq!(red.mix_over(&blue, 0.5), ColorU::new(128, 0, 128, 255));

        let black = ColorU::hex(0x000000);
        let white = ColorU::hex(0xffffff);
        assert_eq!(
            black.mix_over(&white, 0.25),
            ColorU::new(191, 191, 191, 255)
        );

        // The endpoints are the base and the overlay, and the result is always
        // opaque even when the overlay carries its own alpha.
        assert_eq!(black.mix_over(&white, 0.0), white);
        assert_eq!(black.mix_over(&white, 1.0), black);
        assert_eq!(
            ColorU::new(255, 0, 0, 128).mix_over(&white, 0.5),
            ColorU::new(255, 128, 128, 255)
        );
    }
}
