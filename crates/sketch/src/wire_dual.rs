//! Numbers that carry their derivatives (forward-mode automatic differentiation): a 3D
//! constraint's residuals are written once, over [`Num`], and give both their values (`f64`)
//! and their exact Jacobian ([`Dual`]). Numerical differences lose their digits at large
//! coordinates, and the freedom count read from them goes wrong there (#10608).

use std::ops::{Add, Div, Mul, Neg, Sub};

use solidcraft_geom::Vec3;

/// Most unknowns one constraint reads: three coordinates of each of its points.
pub const N: usize = 9;

/// What a residual is computed in: plain numbers, or numbers with their derivatives.
pub trait Num: Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self> + Neg<Output = Self> {
    fn num(v: f64) -> Self;
    fn val(self) -> f64;
    fn sqrt(self) -> Self;
}

impl Num for f64 {
    fn num(v: f64) -> f64 {
        v
    }
    fn val(self) -> f64 {
        self
    }
    fn sqrt(self) -> f64 {
        f64::sqrt(self)
    }
}

/// A value and its derivatives by the (up to [`N`]) unknowns of one constraint.
#[derive(Clone, Copy, Debug)]
pub struct Dual {
    pub v: f64,
    pub d: [f64; N],
}

impl Dual {
    /// Unknown `i` itself: derivative 1 by itself.
    pub fn var(v: f64, i: usize) -> Dual {
        let mut d = [0.0; N];
        if let Some(x) = d.get_mut(i) {
            *x = 1.0;
        }
        Dual { v, d }
    }
    fn map(self, v: f64, f: f64) -> Dual {
        Dual { v, d: self.d.map(|x| x * f) }
    }
}

impl Add for Dual {
    type Output = Dual;
    fn add(self, o: Dual) -> Dual {
        let mut d = self.d;
        d.iter_mut().zip(o.d).for_each(|(a, b)| *a += b);
        Dual { v: self.v + o.v, d }
    }
}

impl Sub for Dual {
    type Output = Dual;
    fn sub(self, o: Dual) -> Dual {
        let mut d = self.d;
        d.iter_mut().zip(o.d).for_each(|(a, b)| *a -= b);
        Dual { v: self.v - o.v, d }
    }
}

impl Mul for Dual {
    type Output = Dual;
    // The product rule: (ab)' = a'b + ab'.
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn mul(self, o: Dual) -> Dual {
        let mut d = self.d;
        d.iter_mut().zip(o.d).for_each(|(a, b)| *a = *a * o.v + self.v * b);
        Dual { v: self.v * o.v, d }
    }
}

impl Div for Dual {
    type Output = Dual;
    #[allow(clippy::suspicious_arithmetic_impl)]
    fn div(self, o: Dual) -> Dual {
        // (a / b)' = (a' − (a / b) b') / b; a zero b gives numbers that are not finite, which the
        // solve refuses.
        let q = self.v / o.v;
        let mut d = self.d;
        d.iter_mut().zip(o.d).for_each(|(a, b)| *a = (*a - q * b) / o.v);
        Dual { v: q, d }
    }
}

impl Neg for Dual {
    type Output = Dual;
    fn neg(self) -> Dual {
        self.map(-self.v, -1.0)
    }
}

impl Num for Dual {
    fn num(v: f64) -> Dual {
        Dual { v, d: [0.0; N] }
    }
    fn val(self) -> f64 {
        self.v
    }
    fn sqrt(self) -> Dual {
        // The root of zero has no derivative; it is taken as zero there (a length of nothing).
        let s = self.v.sqrt();
        self.map(s, if s > 1e-150 { 0.5 / s } else { 0.0 })
    }
}

/// A point or direction over [`Num`].
#[derive(Clone, Copy, Debug)]
pub struct V<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

impl<T: Num> V<T> {
    pub fn of(p: Vec3) -> V<T> {
        V { x: T::num(p.x), y: T::num(p.y), z: T::num(p.z) }
    }
    pub fn dot(self, o: V<T>) -> T {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V<T>) -> V<T> {
        V { x: self.y * o.z - self.z * o.y, y: self.z * o.x - self.x * o.z, z: self.x * o.y - self.y * o.x }
    }
    pub fn len(self) -> T {
        self.dot(self).sqrt()
    }
    pub fn scale(self, s: T) -> V<T> {
        V { x: self.x * s, y: self.y * s, z: self.z * s }
    }
    pub fn xyz(self) -> [T; 3] {
        [self.x, self.y, self.z]
    }
}

impl<T: Num> Add for V<T> {
    type Output = V<T>;
    fn add(self, o: V<T>) -> V<T> {
        V { x: self.x + o.x, y: self.y + o.y, z: self.z + o.z }
    }
}

impl<T: Num> Sub for V<T> {
    type Output = V<T>;
    fn sub(self, o: V<T>) -> V<T> {
        V { x: self.x - o.x, y: self.y - o.y, z: self.z - o.z }
    }
}
