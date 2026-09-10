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
}
