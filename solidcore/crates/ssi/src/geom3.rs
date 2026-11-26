//! Points, directions and frames in space, and the knot insertion that
//! B-spline surfaces are clamped and cut into Bézier pieces with: the exact
//! geometry `surface3` is written in.

use std::ops::{Add, Mul, Neg, Sub};

/// A point or direction in space.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct V3 {
    /// The x coordinate.
    pub x: f64,
    /// The y coordinate.
    pub y: f64,
    /// The z coordinate.
    pub z: f64,
}

impl V3 {
    /// The vector (x, y, z).
    pub const fn new(x: f64, y: f64, z: f64) -> V3 {
        V3 { x, y, z }
    }
    /// The dot product.
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    /// The cross product.
    #[must_use]
    pub fn cross(self, o: V3) -> V3 {
        V3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    /// The Euclidean length.
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    /// The vector times s.
    #[must_use]
    pub fn scale(self, s: f64) -> V3 {
        V3::new(self.x * s, self.y * s, self.z * s)
    }
    /// The vector scaled to length 1; the zero vector stays zero.
    #[must_use]
    pub fn unit(self) -> V3 {
        let l = self.len();
        if l > 0.0 {
            self.scale(1.0 / l)
        } else {
            self
        }
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        V3::new(self.x * s, self.y * s, self.z * s)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        V3::new(-self.x, -self.y, -self.z)
    }
}

/// Right-handed frame: origin, z axis, x axis (y = z cross x).
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// The origin.
    pub o: V3,
    /// The unit z axis.
    pub z: V3,
    /// The unit x axis, perpendicular to z.
    pub x: V3,
}

impl Frame {
    /// The y axis, z cross x.
    pub fn y(&self) -> V3 {
        self.z.cross(self.x)
    }
}

impl Frame {
    /// Coordinates of p in this frame.
    pub fn local(&self, p: V3) -> V3 {
        let d = p - self.o;
        V3::new(d.dot(self.x), d.dot(self.y()), d.dot(self.z))
    }
    /// World point from frame coordinates.
    pub fn world(&self, l: V3) -> V3 {
        self.o + self.x * l.x + self.y() * l.y + self.z * l.z
    }
    /// World direction from frame coordinates.
    pub fn dir(&self, l: V3) -> V3 {
        self.x * l.x + self.y() * l.y + self.z * l.z
    }
}

/// Inserts knot u once into a (local piece of a) B-spline: The NURBS Book
/// algorithm 5.1 for one insertion, in homogeneous coordinates. Point i has
/// support `knots[i] .. knots[i + p + 1]`. None when u is outside the knot
/// vector or the counts do not describe a B-spline of degree p.
pub fn insert_knot<const N: usize>(
    p: usize,
    knots: &mut Vec<f64>,
    pts: &mut Vec<[f64; N]>,
    u: f64,
) -> Option<()> {
    let k = (0..knots.len().checked_sub(1)?).rev().find(|&i| {
        knots.get(i).is_some_and(|&a| a <= u) && knots.get(i + 1).is_some_and(|&b| b > u)
    })?;
    let s = knots
        .get(..=k)?
        .iter()
        .rev()
        .take_while(|&&x| x == u)
        .count();
    let mut q: Vec<[f64; N]> = Vec::with_capacity(pts.len() + 1);
    q.extend_from_slice(pts.get(..=k.checked_sub(p)?)?);
    for i in (k - p + 1)..=k.checked_sub(s)? {
        let (ki, kip) = (*knots.get(i)?, *knots.get(i + p)?);
        let a = (u - ki) / (kip - ki);
        let (pi, pm) = (pts.get(i)?, pts.get(i.checked_sub(1)?)?);
        let mut r = [0.0; N];
        for ((rc, x), y) in r.iter_mut().zip(pi).zip(pm) {
            *rc = a * x + (1.0 - a) * y;
        }
        q.push(r);
    }
    q.extend_from_slice(pts.get(k - s..)?);
    knots.insert(k + 1, u);
    *pts = q;
    Some(())
}

/// The same B-spline with clamped ends (first and last knots repeated degree + 1
/// times), exactly: insert each end of the domain `knots[p] .. knots[m - p]` until
/// it has multiplicity p, then drop the knots and points outside the domain.
/// A clamped input comes back unchanged; None when the counts do not describe a
/// B-spline of degree p.
pub fn clamp<const N: usize>(
    p: usize,
    knots: &[f64],
    pts: &[[f64; N]],
) -> Option<(Vec<f64>, Vec<[f64; N]>)> {
    let (mut kn, mut pt) = (knots.to_vec(), pts.to_vec());
    let clamped = |k: &[f64]| -> Option<bool> {
        let (first, last) = (k.first()?, k.last()?);
        let head = k.get(..=p)?;
        let tail = k.get(k.len().checked_sub(p + 1)?..)?;
        Some(head.iter().all(|x| x == first) && tail.iter().all(|x| x == last))
    };
    if clamped(&kn)? {
        return Some((kn, pt));
    }
    // Start: raise knots[p] to multiplicity p, keep the points from last_a - p on.
    let a = *kn.get(p)?;
    while kn.iter().filter(|&&x| x == a).count() < p {
        insert_knot(p, &mut kn, &mut pt, a)?;
    }
    let last_a = kn.iter().rposition(|&x| x == a)?;
    let mut k2 = vec![a; p + 1];
    k2.extend_from_slice(kn.get(last_a + 1..)?);
    let (mut kn, mut pt) = (k2, pt.get(last_a.checked_sub(p)?..)?.to_vec());
    // End: the mirror image.
    let m = kn.len().checked_sub(1)?;
    let b = *kn.get(m.checked_sub(p)?)?;
    while kn.iter().filter(|&&x| x == b).count() < p {
        insert_knot(p, &mut kn, &mut pt, b)?;
    }
    let first_b = kn.iter().position(|&x| x == b)?;
    let mut k3 = kn.get(..first_b)?.to_vec();
    k3.extend(std::iter::repeat_n(b, p + 1));
    let n = k3.len().checked_sub(p + 1)?;
    pt.truncate(n);
    Some((k3, pt))
}
