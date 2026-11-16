//! Plate contact: where one plate's edge meets another plate's face square, the plate faces of the
//! plate whose edge it is, and that plate's thickness.
//!
//! Upstream: no counterpart. This is the shared job under Tab and Slot (and any joint built from
//! two plates meeting square, such as finger joints): every function here is plain float maths on
//! planes the caller has read from exact faces; the records and the kernel questions are asked by
//! `solidcore-ops`.
//!
//! Words: the *tab plate* `T` meets the *slot plate* `S` on `P`, a plane of one of `S`'s outward
//! planar faces with outward unit normal `n`. A plane is held as [`Plane`]: `normal · x = offset`
//! with `normal` the face's outward unit normal.
//!
//! | Rust                   | Upstream |
//! |------------------------|----------|
//! | [`SQUARE_SLACK`]       | none     |
//! | [`Plane`]              | none     |
//! | [`Bearing`], [`bearing`] | none   |
//! | [`touch_offset`]       | none     |
//! | [`PlateFaces`], [`plate_faces`] | none |

use crate::error::{Result, SolidError};
use crate::math::{Point3, Vec3};

/// Two plane normals within this angle (radians) of each other are parallel; a normal within it
/// of square to another is square. Picked well under any angle a person draws on purpose and well
/// over the angle an exact planar face's normal carries after a STEP round trip.
pub const SQUARE_SLACK: f64 = 1.0e-6;

const OP: &str = "plate_contact";

/// An oriented plane: the points `x` with `normal · x = offset`, `normal` a unit vector pointing
/// out of the material of the face it was read from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// Outward unit normal.
    pub normal: Vec3,
    /// Signed distance of the plane from the origin along `normal`.
    pub offset: f64,
}

impl Plane {
    /// The plane through `point` with outward normal `normal` (unitized here).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a non-finite point or a normal that cannot be unitized.
    pub fn through(point: Point3, normal: Vec3) -> Result<Self> {
        if !(point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) {
            return Err(invalid("a plane's point is not finite"));
        }
        let normal = finite_unit(normal)?;
        Ok(Self {
            normal,
            offset: normal.dot(point),
        })
    }
}

/// How one plane's normal stands to another's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bearing {
    /// The normals point the same way, within [`SQUARE_SLACK`].
    Same,
    /// The normals point opposite ways, within [`SQUARE_SLACK`].
    Opposite,
    /// The normals are square, within [`SQUARE_SLACK`].
    Square,
    /// Anything else; the angle between the normals in degrees, in `(0, 180)`.
    Oblique(f64),
}

/// How `b` stands to `a`. Both must be unit vectors (as in a [`Plane`]).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite or non-unit vector.
pub fn bearing(a: Vec3, b: Vec3) -> Result<Bearing> {
    check_unit(a)?;
    check_unit(b)?;
    // The angle from the cross product's length and the dot's sign: exact near 0 and 180°,
    // where `acos` of the dot loses half the digits.
    let sin = a.cross(b).length();
    let cos = a.dot(b);
    let angle = sin.atan2(cos);
    let slack = SQUARE_SLACK;
    if angle <= slack {
        Ok(Bearing::Same)
    } else if std::f64::consts::PI - angle <= slack {
        Ok(Bearing::Opposite)
    } else if (angle - std::f64::consts::FRAC_PI_2).abs() <= slack {
        Ok(Bearing::Square)
    } else {
        Ok(Bearing::Oblique(angle.to_degrees()))
    }
}

/// The touch test of one of `T`'s planar faces against `P`: when `face` faces `−n` (its normal
/// opposite `P`'s) and lies within `tol` of `P`, above or below it, its signed level `δ` along
/// `n` measured from `P` (`|δ| ≤ tol`); else `None`.
///
/// A planar face is the plane it lies in, so "every boundary point within `tol`" is the plane's
/// level once the normals are opposite within [`SQUARE_SLACK`]; the caller keeps the slack's
/// lever arm (the face's size times [`SQUARE_SLACK`]) under `tol` by refusing faces whose normal
/// strays further.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite or negative `tol`, or a plane whose normal is not
/// a unit vector.
pub fn touch_offset(face: Plane, p: Plane, tol: f64) -> Result<Option<f64>> {
    check_tol(tol)?;
    check_plane(face)?;
    check_plane(p)?;
    if bearing(p.normal, face.normal)? != Bearing::Opposite {
        return Ok(None);
    }
    // `face`: `(−n) · x = d_f`, so its level along `n` is `−d_f`.
    let delta = -face.offset - p.offset;
    Ok((delta.abs() <= tol).then_some(delta))
}

/// The two plate faces of `T` at a contact and its thickness between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateFaces {
    /// The face whose outward normal is `−m`.
    pub a: Plane,
    /// The face whose outward normal is `m`; `m` runs from `a` to `b` across the plate.
    pub b: Plane,
    /// `t_T`: `b`'s level along `m` less `a`'s.
    pub thickness: f64,
}

/// Why [`plate_faces`] found no plate faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoPlateFaces {
    /// No two candidate faces are opposite, square to `n`, and bound the strip across.
    NoPair,
    /// Two or more pairs share the least thickness (a square post): which way is across is
    /// not defined.
    NotAPlate,
}

/// `T`'s plate faces at `P` (normal `n`): among `candidates` (the planar faces the caller found
/// beside the contact), the pairs that are square to `n`, opposite each other, and whose levels
/// along `m` are the strip's extent along `m` within `tol` (`strip_extent(m)` gives that extent as
/// `(lo, hi)`); the narrowest pair wins, and a tie within `tol` is [`NoPlateFaces::NotAPlate`].
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite or negative `tol`, a non-unit normal, or an
/// extent that is not finite and ordered; and whatever `strip_extent` returns.
pub fn plate_faces(
    candidates: &[Plane],
    n: Vec3,
    tol: f64,
    strip_extent: impl Fn(Vec3) -> Result<(f64, f64)>,
) -> Result<core::result::Result<PlateFaces, NoPlateFaces>> {
    check_tol(tol)?;
    check_unit(n)?;
    for f in candidates {
        check_plane(*f)?;
    }
    let mut best: Option<PlateFaces> = None;
    let mut tied = false;
    for (i, a) in candidates.iter().enumerate() {
        if bearing(n, a.normal)? != Bearing::Square {
            continue;
        }
        for b in &candidates[i + 1..] {
            if bearing(a.normal, b.normal)? != Bearing::Opposite {
                continue;
            }
            // Outward normals opposite: the material lies between the two planes when the sum
            // of their offsets (the second's level along its normal less the first's) is
            // positive. `m` is chosen by the normals alone, so the answer does not depend on the
            // order of `candidates`.
            let (lo_face, hi_face) = if leads(b.normal) { (*a, *b) } else { (*b, *a) };
            let m = hi_face.normal;
            let lo = -lo_face.offset;
            let hi = hi_face.offset;
            let thickness = hi - lo;
            if thickness <= tol {
                continue;
            }
            let (s_lo, s_hi) = strip_extent(m)?;
            if !(s_lo.is_finite() && s_hi.is_finite() && s_lo <= s_hi) {
                return Err(invalid("the strip's extent is not finite and ordered"));
            }
            if (s_lo - lo).abs() > tol || (s_hi - hi).abs() > tol {
                continue;
            }
            let pair = PlateFaces {
                a: lo_face,
                b: hi_face,
                thickness,
            };
            match best {
                Some(cur) if (cur.thickness - thickness).abs() <= tol => tied = true,
                Some(cur) if cur.thickness < thickness => {}
                _ => {
                    best = Some(pair);
                    tied = false;
                }
            }
        }
    }
    Ok(match best {
        None => Err(NoPlateFaces::NoPair),
        Some(_) if tied => Err(NoPlateFaces::NotAPlate),
        Some(p) => Ok(p),
    })
}

/// Of two opposite unit normals, whether `v` is the one taken as `m`: the first of its components
/// larger in size than [`SQUARE_SLACK`] is positive. A unit vector has a component of at least
/// `1/√3`, so one always qualifies.
fn leads(v: Vec3) -> bool {
    [v.x, v.y, v.z]
        .into_iter()
        .find(|c| c.abs() > SQUARE_SLACK)
        .is_some_and(|c| c > 0.0)
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

/// How far a unit vector's squared length may stray from 1 (a few ulps of the sum of three
/// squares).
const UNIT_SLACK: f64 = 8.0 * f64::EPSILON;

fn check_unit(v: Vec3) -> Result<()> {
    if !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
        return Err(invalid("a normal is not finite"));
    }
    if (v.length_squared() - 1.0).abs() > UNIT_SLACK {
        return Err(invalid("a normal is not a unit vector"));
    }
    Ok(())
}

fn finite_unit(v: Vec3) -> Result<Vec3> {
    if !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
        return Err(invalid("a normal is not finite"));
    }
    v.unitize()
        .map_err(|_| invalid("a normal has no direction"))
}

fn check_plane(p: Plane) -> Result<()> {
    check_unit(p.normal)?;
    if !p.offset.is_finite() {
        return Err(invalid("a plane's offset is not finite"));
    }
    Ok(())
}

fn check_tol(tol: f64) -> Result<()> {
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(invalid("the tolerance is not finite and non-negative"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
