//! A value with its first derivative, each in a [`Real`]: over an interval, `v` holds the
//! function's range and `d` its derivative's, by the chain rule on enclosures.
//!
//! Where the derivative is unbounded (`√x` at 0, `asin` at ±1) the operation is refused, and
//! the caller falls back on the value alone. A kink (`abs`, `min`, `max`, `hypot` at 0) is no
//! obstacle: the derivative's enclosure holds the slopes on both sides.

use std::cmp::Ordering;

use super::{Proven, Real};

/// `v` and its derivative `d` with respect to one variable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jet<N> {
    pub v: N,
    pub d: N,
}

impl<N: Real> Jet<N> {
    /// The variable itself.
    pub fn variable(v: N) -> Proven<Jet<N>> {
        Ok(Jet { v, d: N::exact(1.0)? })
    }
    /// A value that doesn't vary.
    pub fn constant(v: N) -> Proven<Jet<N>> {
        Ok(Jet { v, d: N::exact(0.0)? })
    }
    /// `f(self)` with value `v` and derivative `f1` at `self`.
    fn chain(self, v: N, f1: N) -> Proven<Jet<N>> {
        Ok(Jet { v, d: f1.mul(self.d)? })
    }
    fn one() -> Proven<N> {
        N::exact(1.0)
    }
    /// `(1 − v)(1 + v)`, which keeps its digits near ±1.
    fn one_minus_sq(v: N) -> Proven<N> {
        Self::one()?.sub(v)?.mul(Self::one()?.add(v)?)
    }
    /// Either of two jets, as `min` or `max` picks: the one surely chosen, or both enclosed.
    fn pick(a: Jet<N>, b: Jet<N>, first: Ordering, v: N) -> Proven<Jet<N>> {
        match a.v.sub(b.v)?.cmp_f64(0.0) {
            Some(o) if o == first => Ok(a),
            Some(o) if o == first.reverse() => Ok(b),
            // Equal or undecided: a kink. A plain number has one slope; it follows `min`/`max`,
            // which pick the second.
            _ => match a.d.hull(b.d) {
                Ok(d) => Ok(Jet { v, d }),
                Err(_) => Ok(b),
            },
        }
    }
}

impl<N: Real> Real for Jet<N> {
    fn exact(x: f64) -> Proven<Jet<N>> {
        Jet::constant(N::exact(x)?)
    }
    fn cmp_f64(self, c: f64) -> Option<Ordering> {
        self.v.cmp_f64(c)
    }
    fn as_point(self) -> Option<f64> {
        self.v.as_point()
    }
    fn hull(self, o: Jet<N>) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.hull(o.v)?, d: self.d.hull(o.d)? })
    }
    fn neg(self) -> Jet<N> {
        Jet { v: self.v.neg(), d: self.d.neg() }
    }
    fn add(self, o: Jet<N>) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.add(o.v)?, d: self.d.add(o.d)? })
    }
    fn sub(self, o: Jet<N>) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.sub(o.v)?, d: self.d.sub(o.d)? })
    }
    fn mul(self, o: Jet<N>) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.mul(o.v)?, d: self.d.mul(o.v)?.add(self.v.mul(o.d)?)? })
    }
    fn div(self, o: Jet<N>) -> Proven<Jet<N>> {
        let q = self.v.div(o.v)?;
        Ok(Jet { v: q, d: self.d.sub(q.mul(o.d)?)?.div(o.v)? })
    }
    fn pow(self, e: Jet<N>) -> Proven<Jet<N>> {
        let v = self.v.pow(e.v)?;
        match e.v.as_point() {
            Some(0.0) => Jet::constant(v),
            // A whole power: n·xⁿ⁻¹, with n − 1 exact.
            Some(n) if n == n.trunc() && n.abs() < 4.5e15 => self.chain(v, N::exact(n)?.mul(self.v.pow(N::exact(n - 1.0)?)?)?),
            // Any other fixed power: n·xⁿ/x.
            Some(n) => self.chain(v, N::exact(n)?.mul(v)?.div(self.v)?),
            // xʸ = e^(y ln x): (xʸ)' = xʸ (y' ln x + y x'/x).
            None => {
                let d = e.d.mul(self.v.ln()?)?.add(e.v.mul(self.d)?.div(self.v)?)?;
                Ok(Jet { v, d: v.mul(d)? })
            }
        }
    }
    fn sqrt(self) -> Proven<Jet<N>> {
        let v = self.v.sqrt()?;
        Ok(Jet { v, d: self.d.div(v.add(v)?)? })
    }
    fn exp(self) -> Proven<Jet<N>> {
        let v = self.v.exp()?;
        self.chain(v, v)
    }
    fn ln(self) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.ln()?, d: self.d.div(self.v)? })
    }
    fn log10(self) -> Proven<Jet<N>> {
        let ln10 = N::exact(10.0)?.ln()?;
        Ok(Jet { v: self.v.log10()?, d: self.d.div(self.v.mul(ln10)?)? })
    }
    fn sin(self) -> Proven<Jet<N>> {
        self.chain(self.v.sin()?, self.v.cos()?)
    }
    fn cos(self) -> Proven<Jet<N>> {
        self.chain(self.v.cos()?, self.v.sin()?.neg())
    }
    fn tan(self) -> Proven<Jet<N>> {
        let t = self.v.tan()?;
        self.chain(t, Self::one()?.add(t.mul(t)?)?)
    }
    fn asin(self) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.asin()?, d: self.d.div(Self::one_minus_sq(self.v)?.sqrt()?)? })
    }
    fn acos(self) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.acos()?, d: self.d.div(Self::one_minus_sq(self.v)?.sqrt()?)?.neg() })
    }
    fn atan(self) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.atan()?, d: self.d.div(Self::one()?.add(self.v.mul(self.v)?)?)? })
    }
    fn atan2(self, x: Jet<N>) -> Proven<Jet<N>> {
        let y = self;
        let num = x.v.mul(y.d)?.sub(y.v.mul(x.d)?)?;
        let den = x.v.mul(x.v)?.add(y.v.mul(y.v)?)?;
        Ok(Jet { v: y.v.atan2(x.v)?, d: num.div(den)? })
    }
    fn sinh(self) -> Proven<Jet<N>> {
        self.chain(self.v.sinh()?, self.v.cosh()?)
    }
    fn cosh(self) -> Proven<Jet<N>> {
        self.chain(self.v.cosh()?, self.v.sinh()?)
    }
    fn tanh(self) -> Proven<Jet<N>> {
        let t = self.v.tanh()?;
        self.chain(t, Self::one_minus_sq(t)?)
    }
    fn asinh(self) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.asinh()?, d: self.d.div(self.v.mul(self.v)?.add(Self::one()?)?.sqrt()?)? })
    }
    fn acosh(self) -> Proven<Jet<N>> {
        let one = Self::one()?;
        Ok(Jet { v: self.v.acosh()?, d: self.d.div(self.v.sub(one)?.mul(self.v.add(one)?)?.sqrt()?)? })
    }
    fn atanh(self) -> Proven<Jet<N>> {
        Ok(Jet { v: self.v.atanh()?, d: self.d.div(Self::one_minus_sq(self.v)?)? })
    }
    fn hypot(self, o: Jet<N>) -> Proven<Jet<N>> {
        let h = self.v.hypot(o.v)?;
        if h.cmp_f64(0.0) == Some(Ordering::Greater) {
            let d = self.v.mul(self.d)?.add(o.v.mul(o.d)?)?.div(h)?;
            return Ok(Jet { v: h, d });
        }
        // At the origin each partial derivative is somewhere in [−1, 1].
        let b = self.d.abs()?.add(o.d.abs()?)?;
        Ok(Jet { v: h, d: b.neg().hull(b)? })
    }
    fn abs(self) -> Proven<Jet<N>> {
        let v = self.v.abs()?;
        match self.v.cmp_f64(0.0) {
            Some(Ordering::Greater) => Ok(Jet { v, d: self.d }),
            Some(Ordering::Less) => Ok(Jet { v, d: self.d.neg() }),
            _ => Ok(Jet { v, d: self.d.neg().hull(self.d)? }),
        }
    }
    fn signum(self) -> Proven<Jet<N>> {
        Jet::constant(self.v.signum()?)
    }
    fn floor(self) -> Proven<Jet<N>> {
        Jet::constant(self.v.floor()?)
    }
    fn ceil(self) -> Proven<Jet<N>> {
        Jet::constant(self.v.ceil()?)
    }
    fn round(self) -> Proven<Jet<N>> {
        Jet::constant(self.v.round()?)
    }
    fn trunc(self) -> Proven<Jet<N>> {
        Jet::constant(self.v.trunc()?)
    }
    /// `a − |b|·q` with `q = ⌊a/|b|⌋` fixed: `a' − q·sign(b)·b'`.
    fn rem_euclid(self, o: Jet<N>) -> Proven<Jet<N>> {
        let v = self.v.rem_euclid(o.v)?;
        let q = self.v.div(o.v.abs()?)?.floor()?;
        let d = self.d.sub(q.mul(o.v.signum()?)?.mul(o.d)?)?;
        Ok(Jet { v, d })
    }
    fn min(self, o: Jet<N>) -> Proven<Jet<N>> {
        Jet::pick(self, o, Ordering::Less, self.v.min(o.v)?)
    }
    fn max(self, o: Jet<N>) -> Proven<Jet<N>> {
        Jet::pick(self, o, Ordering::Greater, self.v.max(o.v)?)
    }
}
