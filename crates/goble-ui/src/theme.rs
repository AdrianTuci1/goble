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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub background: Rgb,
    pub surface: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
    pub border: Rgb,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            background: Rgb::new(10, 10, 12),
            surface: Rgb::new(22, 22, 26),
            text: Rgb::new(230, 230, 235),
            accent: Rgb::new(20, 184, 166),
            border: Rgb::new(40, 40, 46),
        }
    }

    pub fn light() -> Self {
        Self {
            background: Rgb::new(250, 250, 252),
            surface: Rgb::new(255, 255, 255),
            text: Rgb::new(24, 24, 27),
            accent: Rgb::new(13, 148, 136),
            border: Rgb::new(228, 228, 231),
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
