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
//! | [`Contact`], [`contact`] | none   |
//! | [`Defect`], [`stretches`] | none  |

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
/// beside the contact), the pairs that are square to `n`, opposite each other, whose levels
/// along `m` are the strip's extent along `m` within `tol` (`strip_extent(m)` gives that extent as
/// `(lo, hi)`), and whose thickness is not more than the strip's width across them (its extent
/// along `n × m`) plus `tol`; the narrowest pair wins, and a tie within `tol` is
/// [`NoPlateFaces::NotAPlate`].
///
/// The width rule is what makes the pair the plate's thickness and not its length. Bounding the
/// strip is checked along `m` only, so with `T` notched under one face the strip no longer
/// reaches that face, the thin pair drops out, and the length pair (whose faces still bound the
/// strip at its ends) would be left to read as a plate as thick as `T` is long. A plate's
/// thickness is never more than how wide its strip is across it, so the length pair fails the
/// width rule and there is no pair: the contact is skipped rather than given a wrong tab. One
/// rule on the other in-plane direction is enough (a second check along `m` against both
/// directions would ask the same question twice); a square post still passes it both ways and
/// stays a tie.
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
        for b in candidates.iter().skip(i + 1) {
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
            let (w_lo, w_hi) = strip_extent(n.cross(m))?;
            if !(w_lo.is_finite() && w_hi.is_finite() && w_lo <= w_hi) {
                return Err(invalid("the strip's extent is not finite and ordered"));
            }
            if thickness > w_hi - w_lo + tol {
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

/// How far the strip `Σ` reaches along `m`, `Σ` being a region placed on `frame` (P's plane):
/// `(lo, hi)`, the levels `m · X` of its outer loop's lowest and highest points. This is the
/// `strip_extent` that [`plate_faces`] asks for: `m` lies in P's plane (square to `n`), so
/// `m · (O + u·x_axis + v·y_axis) = m · O + u·(m · x_axis) + v·(m · y_axis)`: the region's 2D
/// extent along `(m · x_axis, m · y_axis)`, shifted by `m · O`.
///
/// # Errors
///
/// `m` not finite and unit, not in the frame's plane (its in-plane part's length further than
/// [`SQUARE_SLACK`] from 1), or a region whose extent cannot be found within `eps`.
pub fn strip_extent(
    sigma: &crate::prism::profile::Region2,
    frame: &crate::eval::Frame,
    m: Vec3,
    eps: f64,
) -> Result<(f64, f64)> {
    check_unit(m)?;
    let [ox, oy, oz] = frame.origin;
    let [xx, xy, xz] = frame.x_axis;
    let [yx, yy, yz] = frame.y_axis;
    let du = m.dot(Vec3::new(xx, xy, xz));
    let dv = m.dot(Vec3::new(yx, yy, yz));
    let d = crate::math::Vec2::new(du, dv);
    let off = (d.length() - 1.0).abs();
    if off.is_nan() || off > SQUARE_SLACK {
        return Err(invalid("the direction does not lie in the strip's plane"));
    }
    let d = d
        .unitize()
        .map_err(|_| invalid("the direction does not lie in the strip's plane"))?;
    let (lo, hi) = sigma
        .extent_along(d, eps)
        .ok_or_else(|| invalid("the strip's extent could not be found"))?;
    let base = m.dot(Vec3::new(ox, oy, oz));
    Ok((base + lo, base + hi))
}

/// How `T` meets a candidate plane `P` of `S`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Contact {
    /// A planar face of `T` facing `−n` lies within `tol` of `P` over an area of `S`'s face:
    /// `δ` its signed level along `n` from `P` (`|δ| ≤ tol`). The strip is that face's part.
    Touch {
        /// The face's level along `n`, measured from `P`.
        delta: f64,
    },
    /// No touch, but `T`'s material crosses `P` by more than `tol` over an area of `S`'s face.
    /// The strip is `T`'s section there.
    Overlap,
    /// Neither, with the bodies within `tol` of each other: `T` meets `S` but not on a flat end.
    EndNotFlat,
    /// Neither, with the bodies further apart than `tol`: the distance between them.
    NoContact {
        /// Their exact distance.
        distance: f64,
    },
}

/// The contact of `T` with `P` (design 5.3), from what the caller read: whether `T`'s faces on
/// `P` cover an area of `S`'s face (`on_has_area`), the touch level of such a face
/// ([`touch_offset`]), whether `T`'s section by `P` covers an area of it (`across_has_area`),
/// and, only when neither holds, the bodies' distance (`distance`, asked for then and only then).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite or negative `tol` or distance, or a touch level
/// outside `±tol`; and whatever `distance` returns.
pub fn contact(
    on_has_area: bool,
    touch: Option<f64>,
    across_has_area: bool,
    tol: f64,
    distance: impl FnOnce() -> Result<f64>,
) -> Result<Contact> {
    check_tol(tol)?;
    if let Some(delta) = touch {
        if delta.is_nan() || delta.abs() > tol {
            return Err(invalid("a touch level lies outside the tolerance"));
        }
        if on_has_area {
            return Ok(Contact::Touch { delta });
        }
    }
    if across_has_area {
        return Ok(Contact::Overlap);
    }
    let distance = distance()?;
    if !(distance.is_finite() && distance >= 0.0) {
        return Err(invalid(
            "the bodies' distance is not finite and non-negative",
        ));
    }
    Ok(if distance <= tol {
        Contact::EndNotFlat
    } else {
        Contact::NoContact { distance }
    })
}

/// One defect of `Band − Σ` (design 5.5): its extent along the strip (`u`) and its width across
/// the strip (along `w`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Defect {
    /// Its least and greatest `u`.
    pub along: (f64, f64),
    /// Its extent along `w`.
    pub across: f64,
}

/// The stretches of a strip (design 5.5, v2's sliver rule): `band`, `Σ`'s extent along `u`, less
/// the `u`-extent of every defect wider across the strip than `tol`; the closed intervals left
/// that are longer than `tol`, in increasing `u`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-finite or negative `tol`, or a band or defect whose
/// numbers are not finite and ordered.
pub fn stretches(band: (f64, f64), defects: &[Defect], tol: f64) -> Result<Vec<(f64, f64)>> {
    check_tol(tol)?;
    let ordered = |(a, b): (f64, f64)| a.is_finite() && b.is_finite() && a <= b;
    if !ordered(band) {
        return Err(invalid("the strip's extent is not finite and ordered"));
    }
    let mut cuts = Vec::with_capacity(defects.len());
    for d in defects {
        if !(ordered(d.along) && d.across.is_finite() && d.across >= 0.0) {
            return Err(invalid("a defect's extent is not finite and ordered"));
        }
        if d.across > tol {
            cuts.push(d.along);
        }
    }
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut from = band.0;
    for (a, b) in cuts {
        let to = a.min(band.1);
        if to - from > tol {
            out.push((from, to));
        }
        from = from.max(b);
    }
    if band.1 - from > tol {
        out.push((from, band.1));
    }
    Ok(out)
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
