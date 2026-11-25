//! Numbers that carry their derivatives (forward-mode automatic differentiation): a 3D
//! geometric constraint's residuals are written once, over [`Num`], and give both their values (`f64`)
//! and their exact Jacobian ([`Dual`]). Numerical differences lose their digits at large
//! coordinates, and the freedom count read from them goes wrong there (#10608).

use std::ops::{Add, Div, Mul, Neg, Sub};

/// Most unknowns one constraint reads: three coordinates of each of its points.
pub const N: usize = 18;

/// What a residual is computed in: plain numbers, or numbers with their derivatives.
pub trait Num:
    Copy
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
{
    /// A constant, with no derivatives.
    fn num(v: f64) -> Self;
    /// Its plain value.
    fn val(self) -> f64;
    /// Its square root.
    #[must_use]
    fn sqrt(self) -> Self;
    /// The angle of the point (x, self) from the x axis, as `f64::atan2`.
    #[must_use]
    fn atan2(self, x: Self) -> Self;
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
    fn atan2(self, x: f64) -> f64 {
        f64::atan2(self, x)
    }
}

/// A value and its derivatives by the (up to [`N`]) unknowns of one constraint.
#[derive(Clone, Copy, Debug)]
pub struct Dual {
    /// Its value.
    pub v: f64,
    /// Its partial derivatives.
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
        Dual {
            v,
            d: self.d.map(|x| x * f),
        }
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
        d.iter_mut()
            .zip(o.d)
            .for_each(|(a, b)| *a = *a * o.v + self.v * b);
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
        d.iter_mut()
            .zip(o.d)
            .for_each(|(a, b)| *a = (*a - q * b) / o.v);
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
    fn atan2(self, x: Dual) -> Dual {
        // d atan2(y, x) = (x dy − y dx) / (x² + y²); none at the origin.
        let r2 = self.v * self.v + x.v * x.v;
        let mut d = [0.0; N];
        if r2 > 1e-300 {
            d.iter_mut()
                .zip(self.d.iter().zip(x.d))
                .for_each(|(o, (dy, dx))| *o = (x.v * dy - self.v * dx) / r2);
        }
        Dual {
            v: self.v.atan2(x.v),
            d,
        }
    }
}

/// A point or direction over [`Num`].
#[derive(Clone, Copy, Debug)]
pub struct V<T> {
    /// X coordinate.
    pub x: T,
    /// Y coordinate.
    pub y: T,
    /// Z coordinate.
    pub z: T,
}

impl<T: Num> V<T> {
    /// A constant vector from plain coordinates.
    pub fn of([x, y, z]: [f64; 3]) -> V<T> {
        V {
            x: T::num(x),
            y: T::num(y),
            z: T::num(z),
        }
    }
    /// The scalar product.
    pub fn dot(self, o: V<T>) -> T {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    /// The vector product.
    #[must_use]
    pub fn cross(self, o: V<T>) -> V<T> {
        V {
            x: self.y * o.z - self.z * o.y,
            y: self.z * o.x - self.x * o.z,
            z: self.x * o.y - self.y * o.x,
        }
    }
    /// The Euclidean length.
    pub fn len(self) -> T {
        self.dot(self).sqrt()
    }
    /// Scale every coordinate.
    #[must_use]
    pub fn scale(self, s: T) -> V<T> {
        V {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }
    /// The coordinates in order.
    pub fn xyz(self) -> [T; 3] {
        [self.x, self.y, self.z]
    }
}

impl<T: Num> Add for V<T> {
    type Output = V<T>;
    fn add(self, o: V<T>) -> V<T> {
        V {
            x: self.x + o.x,
            y: self.y + o.y,
            z: self.z + o.z,
        }
    }
}

impl<T: Num> Sub for V<T> {
    type Output = V<T>;
    fn sub(self, o: V<T>) -> V<T> {
        V {
            x: self.x - o.x,
            y: self.y - o.y,
            z: self.z - o.z,
        }
    }
}

/// The weight two directions are compared with in mm: the geometric mean of their lengths, as a
/// constant ([`weight`]).
pub fn size<T: Num>(u: V<T>, v: V<T>) -> T {
    weight((u.len() * v.len()).sqrt())
}

/// A length used as a weight: its value, without derivatives.
pub fn weight<T: Num>(l: T) -> T {
    T::num(l.val().max(1e-9))
}

/// `v` scaled to length 1 (a vector of nothing stays one, pointing nowhere).
pub fn unit<T: Num>(v: V<T>) -> V<T> {
    let n = v.len();
    v.scale(T::num(1.0) / if n.val() > 1e-12 { n } else { T::num(1e-12) })
}

/// The centre of the circle through three points in their plane.
pub fn circumcenter<T: Num>(a: V<T>, b: V<T>, c: V<T>) -> V<T> {
    let (u, v) = (b - a, c - a);
    let n = u.cross(v);
    a + (n.cross(u).scale(v.dot(v)) + v.cross(n).scale(u.dot(u))).scale(T::num(0.5) / n.dot(n))
}

/// The radius of the circle through three points: |ab| |bc| |ca| / (2 |ab × ac|).
pub fn circumradius<T: Num>(a: V<T>, b: V<T>, c: V<T>) -> T {
    (b - a).len() * (c - b).len() * (a - c).len() / (T::num(2.0) * (b - a).cross(c - a).len())
}

/// The point's vector offset from the infinite line through `a` and `b`.
/// The denominator is floored at 1e-12, as for a vanishing solver direction.
pub fn point_on_line<T: Num>(p: V<T>, a: V<T>, b: V<T>) -> [T; 3] {
    let d = b - a;
    let n = d.len();
    let n = if n.val() > 1e-12 { n } else { T::num(1e-12) };
    (p - a).cross(d).scale(T::num(1.0) / n).xyz()
}

/// The point's height above the circle's plane and radial error in that plane.
/// All four points carry their derivatives; collinear circle points yield nonfinite
/// residuals, which the caller's solver rejects.
pub fn point_on_circle<T: Num>(p: V<T>, a: V<T>, b: V<T>, c: V<T>) -> [T; 2] {
    let center = circumcenter(a, b, c);
    let n = unit((b - a).cross(c - a));
    let d = p - center;
    let h = d.dot(n);
    [h, (d - n.scale(h)).len() - (a - center).len()]
}

/// Signed point-to-plane distance for a unit `normal`.
pub fn plane_distance<T: Num>(p: V<T>, origin: V<T>, normal: V<T>) -> T {
    (p - origin).dot(normal)
}

/// Signed line-to-plane angle and line length, for a unit plane `normal`.
pub fn plane_angle<T: Num>(start: V<T>, end: V<T>, normal: V<T>) -> (T, T) {
    let run = end - start;
    (run.dot(normal).atan2(run.cross(normal).len()), run.len())
}

/// The side a line-to-plane dimension drives, allowing 1e-9 mm of rounding.
pub fn plane_reversed(start: V<f64>, end: V<f64>, normal: V<f64>) -> bool {
    let run = end - start;
    run.dot(normal) < -1e-9 * run.len().max(1.0)
}

/// Residuals for a line rising at `value` radians to a plane (0 through pi/2).
/// At pi/2 use the vector direction, which stays smooth at that angle.
pub fn plane_angle_residual<T: Num>(
    start: V<T>,
    end: V<T>,
    normal: V<T>,
    reversed: bool,
    value: f64,
) -> Vec<T> {
    let run = end - start;
    let (u, length) = (unit(run), weight(run.len()));
    let normal = if reversed {
        normal.scale(-T::num(1.0))
    } else {
        normal
    };
    if value >= std::f64::consts::FRAC_PI_2 - 1e-9 {
        (u - normal).scale(length).xyz().to_vec()
    } else {
        vec![(u.dot(normal).atan2(u.cross(normal).len()) - T::num(value)) * length]
    }
}

/// Three points spaced a third of a turn apart on an orthonormal circle frame.
pub fn circle_points(c: V<f64>, x: V<f64>, y: V<f64>, radius: f64, from: f64) -> [[f64; 3]; 3] {
    [0.0, 1.0, 2.0].map(|k| {
        let t = from + k * std::f64::consts::TAU / 3.0;
        (c + x.scale(radius * t.cos()) + y.scale(radius * t.sin())).xyz()
    })
}

/// Three points on a planar arc's circle, starting at the arc's first point.
pub fn arc_circle_points(c: [f64; 2], start: [f64; 2], frame: &super::Frame) -> [[f64; 3]; 3] {
    let [cx, cy] = c;
    let [ax, ay] = start;
    let (dx, dy) = (ax - cx, ay - cy);
    let radius = (dx * dx + dy * dy).sqrt();
    circle_points(
        V::of(plane_point(frame, c)),
        V::of(frame.x_axis),
        V::of(frame.y_axis),
        radius,
        dy.atan2(dx),
    )
}

/// Whether two orthonormal planes share a support, allowing 1e-6 mm of height.
pub fn same_plane(a: &super::Frame, b: &super::Frame) -> bool {
    use crate::geom::vector3d::Vec3;
    let vector = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
    crate::geom::plane_height::is_square_plane_at(
        vector(a.origin),
        vector(plane_normal(a)),
        vector(b.origin),
        vector(plane_normal(b)),
        0.0,
        1e-6,
    )
}

/// The normal of an orthonormal plane frame.
pub fn plane_normal(frame: &super::Frame) -> [f64; 3] {
    V::<f64>::of(frame.x_axis).cross(V::of(frame.y_axis)).xyz()
}

/// A point expressed in a plane frame, in the coordinates that frame is defined in.
pub fn plane_point(frame: &super::Frame, [u, v]: [f64; 2]) -> [f64; 3] {
    (V::of(frame.origin) + V::of(frame.x_axis).scale(u) + V::of(frame.y_axis).scale(v)).xyz()
}
