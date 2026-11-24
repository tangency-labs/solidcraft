//! Exact retained intervals for the parallel planar-band minimizing family.
//!
//! Upstream: none. Quadratic strip boundaries are stored as real algebraic
//! numbers; sign comparisons eliminate radicals before reading expansion signs.
//! Subtracting their closed intervals constructs the complement of their union.
//! A binary search of the binary64 lattice rounds algebraic endpoints directly,
//! without approximate roots, tolerance padding, or witness retries.
//!
//! Deviations: expansion overflow or underflow, and a real surviving component
//! containing no binary64 parameter, return a typed arithmetic-range error.
//! No recursion; directed rounding takes at most 1075 comparisons per endpoint.

use std::cmp::Ordering;

use crate::error::{Result, SolidError};
use crate::geom::expansion as exact;

const OP: &str = "blend::fillet_executive::session::planar_bands::interval";

/// An exact dyadic value as a nonoverlapping expansion, least significant first.
pub(super) type Scalar = Vec<f64>;

fn arithmetic_range() -> SolidError {
    SolidError::invalid_input(OP, "retained band interval outside exact arithmetic range")
}

fn compact(value: Scalar) -> Result<Scalar> {
    if !value.iter().all(|x| x.is_finite()) {
        return Err(arithmetic_range());
    }
    Ok(value.into_iter().filter(|x| *x != 0.0).collect())
}

fn negate(value: &[f64]) -> Scalar {
    value.iter().map(|x| -*x).collect()
}

/// Exact addition with finite components and zero elimination.
pub(super) fn add(a: &[f64], b: &[f64]) -> Result<Scalar> {
    compact(exact::add(a, b))
}

/// Exact subtraction with finite components and zero elimination.
pub(super) fn sub(a: &[f64], b: &[f64]) -> Result<Scalar> {
    compact(exact::sub(a, b))
}

/// Exact multiplication with finite components and zero elimination.
pub(super) fn mul(a: &[f64], b: &[f64]) -> Result<Scalar> {
    if !a.iter().chain(b).all(|x| x.is_finite()) {
        return Err(arithmetic_range());
    }
    let mut product = Vec::new();
    for &factor in b {
        product = add(&product, &scale(a, factor)?)?;
    }
    Ok(product)
}

/// Exact scalar multiplication with finite components and zero elimination.
pub(super) fn scale(a: &[f64], factor: f64) -> Result<Scalar> {
    if !factor.is_finite() || !a.iter().all(|x| x.is_finite()) {
        return Err(arithmetic_range());
    }
    compact(exact::scale(a, factor).ok_or_else(arithmetic_range)?)
}

/// An exact quotient whose denominator is strictly positive.
pub(super) struct Ratio {
    /// The numerator's exact expansion.
    pub numerator: Scalar,
    /// The denominator's exact positive expansion.
    pub denominator: Scalar,
}

/// Compare exact quotients by their positive-denominator cross products.
pub(super) fn compare_ratios(a: &Ratio, b: &Ratio) -> Result<Ordering> {
    if exact::sign(&a.denominator) <= 0 || exact::sign(&b.denominator) <= 0 {
        return Err(arithmetic_range());
    }
    Ok(ordering(exact::sign(&sub(
        &mul(&a.numerator, &b.denominator)?,
        &mul(&b.numerator, &a.denominator)?,
    )?)))
}

fn ordering(sign: i32) -> Ordering {
    sign.cmp(&0)
}

/// The sign of a + b sqrt(d), eliminating the radical only across opposite signs.
fn one_radical(a: &[f64], b: &[f64], d: &[f64]) -> Result<i32> {
    let (sa, sb, sd) = (exact::sign(a), exact::sign(b), exact::sign(d));
    if sd < 0 {
        return Err(arithmetic_range());
    }
    if sb == 0 || sd == 0 {
        return Ok(sa);
    }
    if sa == 0 {
        return Ok(sb);
    }
    if sa == sb {
        return Ok(sa);
    }
    let difference = sub(&mul(a, a)?, &mul(&mul(b, b)?, d)?)?;
    Ok(sa * exact::sign(&difference))
}

/// The sign of a + b sqrt(d) + c sqrt(e), by at most two squarings.
fn two_radicals(a: &[f64], b: &[f64], d: &[f64], c: &[f64], e: &[f64]) -> Result<i32> {
    let sx = one_radical(a, b, d)?;
    let (sc, se) = (exact::sign(c), exact::sign(e));
    if se < 0 {
        return Err(arithmetic_range());
    }
    if sc == 0 || se == 0 {
        return Ok(sx);
    }
    if sx == 0 {
        return Ok(sc);
    }
    if sx == sc {
        return Ok(sx);
    }
    let constant = sub(
        &add(&mul(a, a)?, &mul(&mul(b, b)?, d)?)?,
        &mul(&mul(c, c)?, e)?,
    )?;
    let radical = scale(&mul(a, b)?, 2.0)?;
    Ok(sx * one_radical(&constant, &radical, d)?)
}

/// (a + b sqrt(d)) / q, with q > 0 and d >= 0.
#[derive(Clone)]
struct Root {
    a: Scalar,
    b: Scalar,
    d: Scalar,
    q: Scalar,
}

impl Root {
    fn rational(value: f64) -> Self {
        Self {
            a: vec![value],
            b: Vec::new(),
            d: Vec::new(),
            q: vec![1.0],
        }
    }

    fn compare(&self, other: &Self) -> Result<Ordering> {
        let a = sub(&mul(&self.a, &other.q)?, &mul(&other.a, &self.q)?)?;
        let b = mul(&self.b, &other.q)?;
        let c = negate(&mul(&other.b, &self.q)?);
        Ok(ordering(two_radicals(&a, &b, &self.d, &c, &other.d)?))
    }

    /// Greatest binary64 not above this root, which is known to lie in [0, 1].
    fn floor_bits(&self) -> Result<u64> {
        let mut lo = 0.0_f64;
        let mut hi = 1.0_f64;
        if self.compare(&Self::rational(1.0))? == Ordering::Equal {
            return Ok(hi.to_bits());
        }
        if self.compare(&Self::rational(0.0))? == Ordering::Equal {
            return Ok(lo.to_bits());
        }
        // Arithmetic bisection stays near the algebraic endpoint. Bisecting bit
        // keys would first probe about 1e-154, whose irrelevant squared residuals
        // can leave expansion arithmetic's range even for an ordinary root.
        // There are 1074 binary exponents from 1 to the least positive double;
        // one further halving detects adjacency by rounding to an endpoint.
        for _ in 0..1075 {
            let mid = lo + (hi - lo) * 0.5;
            if mid == lo || mid == hi {
                return Ok(lo.to_bits());
            }
            match self.compare(&Self::rational(mid))? {
                Ordering::Less => hi = mid,
                Ordering::Equal => return Ok(mid.to_bits()),
                Ordering::Greater => lo = mid,
            }
        }
        Err(arithmetic_range())
    }
}

struct Interval {
    lo: Root,
    hi: Root,
}

impl Interval {
    /// Select an interior binary64 using exactly directed endpoint rounding.
    fn parameter(&self) -> Result<Option<f64>> {
        let lo = self
            .lo
            .floor_bits()?
            .checked_add(1)
            .ok_or_else(arithmetic_range)?;
        let floor_hi = self.hi.floor_bits()?;
        let hi = if self.hi.compare(&Root::rational(f64::from_bits(floor_hi)))? == Ordering::Equal {
            floor_hi.checked_sub(1).ok_or_else(arithmetic_range)?
        } else {
            floor_hi
        };
        if lo > hi {
            return Ok(None);
        }
        let lower = f64::from_bits(lo);
        let upper = f64::from_bits(hi);
        // Rounding the arithmetic midpoint can reach a component's algebraic
        // boundary. Clamp its bit key to the already certified interior lattice.
        let middle = (lower + (upper - lower) * 0.5).to_bits();
        let span = hi.checked_sub(lo).ok_or_else(arithmetic_range)?;
        let offset = middle.saturating_sub(lo).min(span);
        let bounded = lo.checked_add(offset).ok_or_else(arithmetic_range)?;
        Ok(Some(f64::from_bits(bounded)))
    }
}

/// Retain a parameter where every exact quadratic [C, B, A] is strictly positive.
/// Each A is nonnegative; linear inequalities and constants are included.
/// Closed forbidden intervals are subtracted from (0, 1), preserving all gaps.
pub(super) fn retained_parameter(polynomials: &[[Scalar; 3]]) -> Result<Option<f64>> {
    // A covering early polynomial must not hide malformed later input.
    for [c, b, a] in polynomials {
        if !a.iter().chain(b).chain(c).all(|x| x.is_finite()) || exact::sign(a) < 0 {
            return Err(SolidError::invalid_input(
                OP,
                "invalid squared-affine strip polynomial",
            ));
        }
    }
    let mut kept = vec![Interval {
        lo: Root::rational(0.0),
        hi: Root::rational(1.0),
    }];
    for [c, b, a] in polynomials {
        let (a, b, c) = (
            compact(a.clone())?,
            compact(b.clone())?,
            compact(c.clone())?,
        );
        let sa = exact::sign(&a);
        if sa < 0 {
            return Err(SolidError::invalid_input(
                OP,
                "invalid squared-affine strip polynomial",
            ));
        }
        let blocked = if sa == 0 {
            let sb = exact::sign(&b);
            if sb == 0 {
                if exact::sign(&c) <= 0 {
                    return Ok(None);
                }
                continue;
            }
            let root = Root {
                a: if sb > 0 { negate(&c) } else { c },
                b: Vec::new(),
                d: Vec::new(),
                q: if sb > 0 { b } else { negate(&b) },
            };
            if sb > 0 {
                Interval {
                    lo: Root::rational(0.0),
                    hi: root,
                }
            } else {
                Interval {
                    lo: root,
                    hi: Root::rational(1.0),
                }
            }
        } else {
            let discriminant = sub(&mul(&b, &b)?, &scale(&mul(&a, &c)?, 4.0)?)?;
            if exact::sign(&discriminant) < 0 {
                continue;
            }
            let denominator = scale(&a, 2.0)?;
            Interval {
                lo: Root {
                    a: negate(&b),
                    b: vec![-1.0],
                    d: discriminant.clone(),
                    q: denominator.clone(),
                },
                hi: Root {
                    a: negate(&b),
                    b: vec![1.0],
                    d: discriminant,
                    q: denominator,
                },
            }
        };
        let mut remainder = Vec::new();
        for interval in kept {
            if blocked.lo.compare(&interval.hi)? != Ordering::Less
                || blocked.hi.compare(&interval.lo)? != Ordering::Greater
            {
                remainder.push(interval);
                continue;
            }
            if interval.lo.compare(&blocked.lo)? == Ordering::Less {
                remainder.push(Interval {
                    lo: interval.lo.clone(),
                    hi: blocked.lo.clone(),
                });
            }
            if blocked.hi.compare(&interval.hi)? == Ordering::Less {
                remainder.push(Interval {
                    lo: blocked.hi.clone(),
                    hi: interval.hi,
                });
            }
        }
        kept = remainder;
        if kept.is_empty() {
            return Ok(None);
        }
    }
    let midpoint = Root::rational(0.5);
    for interval in &kept {
        if interval.lo.compare(&midpoint)? == Ordering::Less
            && midpoint.compare(&interval.hi)? == Ordering::Less
        {
            return Ok(Some(0.5));
        }
    }
    for interval in &kept {
        if let Some(parameter) = interval.parameter()? {
            return Ok(Some(parameter));
        }
    }
    Err(SolidError::invalid_input(
        OP,
        "retained band interval has no representable interior parameter",
    ))
}
