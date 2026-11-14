// A minimal private complex scalar keeps numerical conventions in one place.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct C {
    pub(crate) re: f64,
    pub(crate) im: f64,
}
impl C {
    pub(crate) fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    pub(crate) fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }
    pub(crate) fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
    pub(crate) fn finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}
impl std::ops::Add for C {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self::new(self.re + b.re, self.im + b.im)
    }
}
impl std::ops::Sub for C {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self::new(self.re - b.re, self.im - b.im)
    }
}
impl std::ops::Mul for C {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        Self::new(
            self.re * b.re - self.im * b.im,
            self.re * b.im + self.im * b.re,
        )
    }
}
impl std::ops::Mul<f64> for C {
    type Output = Self;
    fn mul(self, b: f64) -> Self {
        Self::new(self.re * b, self.im * b)
    }
}
impl std::ops::Div for C {
    type Output = Self;
    fn div(self, b: Self) -> Self {
        let d = b.re * b.re + b.im * b.im;
        Self::new(
            (self.re * b.re + self.im * b.im) / d,
            (self.im * b.re - self.re * b.im) / d,
        )
    }
}
