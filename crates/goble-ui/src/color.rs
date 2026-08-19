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

impl Default for ColorU {
    fn default() -> Self {
        Self::new(0, 0, 0, 255)
    }
}
