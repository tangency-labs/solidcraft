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
//! | [`passes_through`]     | none     |
//! | [`PairSkip`], [`PairJoint`], [`pair`] | none |
//! | [`with_swap`]          | none     |
//! | [`pair_at`]            | none     |

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

/// The band and defects of a strip Σ (design 5.5) for [`stretches`], in Σ's 2D frame with the
/// strip running along the unit `d`: the band `Σ`'s extent along `d` (`u`), and one [`Defect`]
/// per hole of Σ and per run of Σ's outer loop off the band's boundary (a notch, a chamfered
/// end), each with its extent along `u` and across (along `d` turned a quarter left). A segment
/// lies on the boundary when its ends and middle are within `tol` of one side of the band; a
/// run's extent is read from its segments' ends and middles (exact for lines).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `tol` not positive and finite, a `d` not unit, or a Σ
/// whose extent cannot be found.
pub fn strip_defects(
    sigma: &crate::prism::profile::Region2,
    d: crate::math::Vec2,
    tol: f64,
) -> Result<((f64, f64), Vec<Defect>)> {
    use crate::math::Vec2;
    use crate::prism::profile::{Region2, Seg2};
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid("tol must be positive and finite"));
    }
    if !(d.x.is_finite() && d.y.is_finite() && ((d.x * d.x + d.y * d.y).sqrt() - 1.0).abs() <= 1e-9)
    {
        return Err(invalid("the strip's direction is not unit"));
    }
    let w = Vec2::new(-d.y, d.x);
    let extent = |r: &Region2, v: Vec2| {
        r.extent_along(v, tol)
            .ok_or_else(|| invalid("the strip's extent could not be found"))
    };
    let band = extent(sigma, d)?;
    let across = extent(sigma, w)?;
    let mut out = Vec::new();
    for h in &sigma.holes {
        let r = Region2 {
            outer: h.clone(),
            holes: Vec::new(),
        };
        let (a, b) = extent(&r, w)?;
        out.push(Defect {
            along: extent(&r, d)?,
            across: b - a,
        });
    }
    let uw = |p: Vec2| (p.x * d.x + p.y * d.y, p.x * w.x + p.y * w.y);
    let points = |s: &Seg2| [s.point_at(0.0), s.point_at(0.5), s.point_at(1.0)].map(uw);
    let near = |x: f64, to: f64| (x - to).abs() <= tol;
    let on_side = |ps: &[(f64, f64); 3]| {
        ps.iter().all(|p| near(p.0, band.0))
            || ps.iter().all(|p| near(p.0, band.1))
            || ps.iter().all(|p| near(p.1, across.0))
            || ps.iter().all(|p| near(p.1, across.1))
    };
    let segs = &sigma.outer.segs;
    let off: Vec<bool> = segs.iter().map(|s| !on_side(&points(s))).collect();
    let n = segs.len();
    // Start the walk just after a segment on the boundary, so no run wraps around.
    if let Some(first) = off.iter().position(|o| !o) {
        let mut run: Option<((f64, f64), (f64, f64))> = None;
        for (seg, &is_off) in segs.iter().zip(&off).cycle().skip(first + 1).take(n) {
            if is_off {
                for (u, v) in points(seg) {
                    let ((ul, uh), (vl, vh)) = run.unwrap_or(((u, u), (v, v)));
                    run = Some(((ul.min(u), uh.max(u)), (vl.min(v), vh.max(v))));
                }
            } else if let Some((along, (vl, vh))) = run.take() {
                out.push(Defect {
                    along,
                    across: vh - vl,
                });
            }
        }
    }
    Ok((band, out))
}

/// The `on` half of T's footprint on P (design v2 5.3): the regions, in `frame` (P's plane, its
/// normal `n` = x × y), of T's planar faces that lie in P and face `−n`. A face lies in P when
/// every vertex of it is within `tol` of P (its edges then lie in its plane, which is parallel
/// to P: [`prism::sheet`](crate::prism::sheet) reads them exactly and refuses one off the
/// plane); it faces `−n` when its outward normal is within [`SQUARE_SLACK`] of `−n`. Faces on
/// other surfaces, facing elsewhere or at another level are left out; T itself is not changed.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `tol` that is not positive and finite;
/// `Unsupported` for a kept face whose outward side is not decided or whose edges are not read
/// exactly in P; those of the topology walk.
pub fn footprint_on(
    k: &crate::kernel::Kernel,
    t: &crate::topology::Brep,
    frame: &crate::eval::Frame,
    tol: f64,
) -> Result<Vec<crate::prism::profile::Region2>> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid("tol must be positive and finite"));
    }
    let (_, _, _, n) = crate::prism::profile::axes(frame);
    let facing_down = -SQUARE_SLACK.cos();
    let read = crate::prism::sheet::planar_face_regions(k, t, frame, tol, |face, normal| {
        let Some(normal) = normal else {
            return Ok(false);
        };
        let natural =
            crate::prism::measure::outward_is_natural(t, face).ok_or(SolidError::Unsupported {
                operation: OP,
                reason: "a face whose outward side is not decided",
            })?;
        let outward_up = if natural {
            normal.dot(n)
        } else {
            -normal.dot(n)
        };
        if outward_up > facing_down {
            return Ok(false);
        }
        let loops = crate::query::api_queries::face_get_boundary_loops(t, face)?;
        for &e in &loops.edges {
            let ends = [
                crate::topology::edge::start_vertex(t, e)?,
                crate::topology::edge::end_vertex(t, e)?,
            ];
            for v in ends.into_iter().flatten() {
                let level = crate::prism::profile::height(frame, t.vertex(v)?.point);
                if level.is_nan() || level.abs() > tol {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    })?;
    Ok(read.regions)
}

/// The `across` half of design v2 5.3: T's section by P (the frame's plane), as regions of the
/// frame. The section is upstream's own ([`intersect_brep_with_plane`], each face's section
/// trimmed to the face) on a scratch copy of T standing for it (the section takes the B-rep
/// mutably; T is not changed and no boolean runs). Each section curve is read exactly
/// ([`planar_pieces`]), the curves are joined end to end (within `tol`) into closed loops, and a
/// loop inside an odd number of others is a hole of the smallest loop holding it.
///
/// [`intersect_brep_with_plane`]: crate::intersect::api_intersectors::intersect_brep_with_plane
/// [`planar_pieces`]: crate::prism::read::planar_pieces
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `tol` not positive and finite; `Unsupported` for a section
/// curve not read exactly in the frame, a section that does not close, or a loop whose turning or
/// place is not decided; those of the section.
pub fn footprint_across(
    k: &crate::kernel::Kernel,
    t: &crate::topology::Brep,
    frame: &crate::eval::Frame,
    tol: f64,
) -> Result<Vec<crate::prism::profile::Region2>> {
    use crate::prism::profile::{Loop2, Region2, Seg2};
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid("tol must be positive and finite"));
    }
    let refused = |reason: &'static str| SolidError::Unsupported {
        operation: OP,
        reason,
    };
    let (origin, _, _, n) = crate::prism::profile::axes(frame);
    let mut scratch = t.clone_standing_for_self()?;
    let curves =
        crate::intersect::api_intersectors::intersect_brep_with_plane(k, &mut scratch, origin, n)?;
    let mut work = crate::prism::sheet::READ_WORK;
    let mut runs: Vec<Vec<Seg2>> = Vec::new();
    for curve in &curves {
        let iv = curve.natural_interval();
        let ends = [
            curve.evaluate_point(k, iv.min())?,
            curve.evaluate_point(k, iv.max())?,
        ];
        let run = crate::prism::read::Run {
            curve,
            interval: iv,
            forward: true,
            ends,
        };
        let pieces = crate::prism::read::planar_pieces(k, &run, frame, tol, &mut work, true)
            .ok_or_else(|| refused("a section curve not read exactly in the plane"))?;
        if !pieces.is_empty() {
            runs.push(pieces);
        }
    }
    let near = |a: crate::math::Point2, b: crate::math::Point2| (a.x - b.x).hypot(a.y - b.y) <= tol;
    let start = |r: &[Seg2]| r.first().map(|s| s.point_at(0.0));
    let end = |r: &[Seg2]| r.last().map(|s| s.point_at(1.0));
    // join the curves end to end into closed loops
    let mut loops: Vec<Loop2> = Vec::new();
    while let Some(first) = runs.pop() {
        let begin = start(first.as_slice()).ok_or_else(|| refused("an empty section curve"))?;
        let mut segs = first;
        loop {
            let at = end(segs.as_slice()).ok_or_else(|| refused("an empty section curve"))?;
            if segs.len() > 1 && near(at, begin) {
                break;
            }
            let next = runs.iter().position(|r| {
                start(r.as_slice()).is_some_and(|p| near(p, at))
                    || end(r.as_slice()).is_some_and(|p| near(p, at))
            });
            let Some(i) = next else {
                if near(at, begin) {
                    break;
                }
                return Err(refused("a section that does not close"));
            };
            let run = runs.swap_remove(i);
            if start(run.as_slice()).is_some_and(|p| near(p, at)) {
                segs.extend(run);
            } else {
                segs.extend(run.iter().rev().map(Seg2::reversed));
            }
        }
        let lp = Loop2 { segs };
        // every loop counter-clockwise; holes are turned once their place is known
        loops.push(match lp.signed_area()?.sign() {
            Some(true) => lp,
            Some(false) => lp.reversed(),
            None => return Err(refused("a section loop whose turning is not decided")),
        });
    }
    let mid = |lp: &Loop2| {
        lp.segs
            .first()
            .map(|s| s.point_at(0.5))
            .ok_or_else(|| refused("an empty loop"))
    };
    let mut holders: Vec<Vec<usize>> = Vec::new();
    for (i, lp) in loops.iter().enumerate() {
        let p = mid(lp)?;
        let mut held_by = Vec::new();
        for (j, other) in loops.iter().enumerate() {
            if i != j
                && other
                    .holds(p)
                    .ok_or_else(|| refused("a section loop whose place is not decided"))?
            {
                held_by.push(j);
            }
        }
        holders.push(held_by);
    }
    let mut regions: Vec<Option<Region2>> = Vec::new();
    for (lp, held_by) in loops.iter().zip(&holders) {
        regions.push((held_by.len() % 2 == 0).then(|| Region2 {
            outer: lp.clone(),
            holes: Vec::new(),
        }));
    }
    for (lp, held_by) in loops.iter().zip(&holders) {
        if held_by.len() % 2 == 0 {
            continue;
        }
        let mut best: Option<(f64, usize)> = None;
        for &j in held_by {
            if let Some(Some(r)) = regions.get(j) {
                let area = r.outer.signed_area()?.value;
                if best.is_none_or(|(a, _)| area < a) {
                    best = Some((area, j));
                }
            }
        }
        let hole = best
            .and_then(|(_, j)| regions.get_mut(j))
            .and_then(Option::as_mut)
            .ok_or_else(|| refused("a hole outside the section's material"))?;
        hole.holes.push(lp.reversed());
    }
    let mut out: Vec<Region2> = regions.into_iter().flatten().collect();
    crate::prism::sheet::canonical(&mut out, tol)?;
    Ok(out)
}

/// Whether T runs through S (design v2 5.3 [m22], skip `passes_through`): some far face of S, a
/// planar face facing `−n` below P (the frame's plane, normal `n`), shares area with T's section
/// just past that face's plane (by `tol`, so a T flush with the face is accepted;
/// [`footprint_across`]). Pure 2D: the face is read as regions of the
/// frame moved down to its level ([`prism::sheet`](crate::prism::sheet)) and compared with
/// [`regions_overlap`](crate::prism::profile::regions_overlap) (`eps = tol`); no boolean runs and
/// neither plate is changed. A face only touching the section along an edge or at a corner is
/// not passed through.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `tol` not positive and finite; `Unsupported` for a far face
/// whose outward side is not decided, or a section and face whose overlap is not decided; those
/// of the reads and the section.
pub fn passes_through(
    k: &crate::kernel::Kernel,
    s: &crate::topology::Brep,
    t: &crate::topology::Brep,
    frame: &crate::eval::Frame,
    tol: f64,
) -> Result<bool> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid("tol must be positive and finite"));
    }
    let (_, _, _, n) = crate::prism::profile::axes(frame);
    let facing_down = -SQUARE_SLACK.cos();
    let far = std::cell::RefCell::new(Vec::new());
    crate::prism::sheet::planar_face_regions(k, s, frame, tol, |face, normal| {
        let Some(normal) = normal else {
            return Ok(false);
        };
        let natural =
            crate::prism::measure::outward_is_natural(s, face).ok_or(SolidError::Unsupported {
                operation: OP,
                reason: "a face whose outward side is not decided",
            })?;
        let outward_up = if natural {
            normal.dot(n)
        } else {
            -normal.dot(n)
        };
        if outward_up > facing_down {
            return Ok(false);
        }
        let loops = crate::query::api_queries::face_get_boundary_loops(s, face)?;
        let mut level = f64::INFINITY;
        for &e in &loops.edges {
            if let Some(v) = crate::topology::edge::start_vertex(s, e)? {
                level = level.min(crate::prism::profile::height(frame, s.vertex(v)?.point));
            }
        }
        if level.is_finite() && level < -tol {
            far.borrow_mut().push((face, level));
        }
        Ok(false)
    })?;
    for (face, level) in far.into_inner() {
        let [ox, oy, oz] = frame.origin;
        let at = crate::eval::Frame {
            origin: [ox + level * n.x, oy + level * n.y, oz + level * n.z],
            x_axis: frame.x_axis,
            y_axis: frame.y_axis,
        };
        // T's section just past the far face (by `tol`): a T flush with the face (its end in
        // the face's plane) has none there and is accepted (design v2 5.3).
        let past = crate::eval::Frame {
            origin: [
                ox + (level - tol) * n.x,
                oy + (level - tol) * n.y,
                oz + (level - tol) * n.z,
            ],
            x_axis: frame.x_axis,
            y_axis: frame.y_axis,
        };
        let section = footprint_across(k, t, &past, tol)?;
        if section.is_empty() {
            continue;
        }
        let read = crate::prism::sheet::planar_face_regions(k, s, &at, tol, |f, _| Ok(f == face))?;
        for a in &section {
            for b in &read.regions {
                match crate::prism::profile::regions_overlap(a, b, tol) {
                    Some(true) => return Ok(true),
                    Some(false) => {}
                    None => {
                        return Err(SolidError::Unsupported {
                            operation: OP,
                            reason: "whether the tab plate runs through a far face is not decided",
                        })
                    }
                }
            }
        }
    }
    Ok(false)
}

/// Why a pair of plates (T, S) at P gives no joint, as far as [`pair`] and [`with_swap`] decide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PairSkip {
    /// T meets P neither touching nor overlapping (`end_not_flat`, `no_contact`).
    Contact(Contact),
    /// T has no plate faces at the contact ([`plate_faces`]).
    NoPlateFaces(NoPlateFaces),
    /// T is not at 90° to P: the angle between `n` and a side face of T, in degrees; `None` when
    /// T lies face to face on S.
    NotSquare(Option<f64>),
    /// The reverse pair (S as tab plate, T as slot plate) is a plate joint [m10].
    Swapped,
}

/// A plate joint of T on P: how T meets P and T's plate faces there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairJoint {
    /// The contact, [`Contact::Touch`] or [`Contact::Overlap`].
    pub contact: Contact,
    /// T's plate faces and thickness.
    pub faces: PlateFaces,
}

/// The pair-level decision of design v2 5.3 and 5.4 for T on P (normal `n`): the contact first
/// (only a touch or an overlap goes on), then T's plate faces among `candidates` (its planar
/// faces beside the contact, [`plate_faces`] with `strip_extent`). When no pair qualifies, a
/// candidate at an angle to `n` other than square names T's tilt (`not_square`, the angle
/// [`bearing`] gives). A pair whose thickness is not less than `rise` (T's extent along `n` from
/// P) less `tol` is T lying face to face on S: `not_square` with no angle, never a plate as thick
/// as T is wide.
///
/// # Errors
///
/// Those of [`plate_faces`] and [`bearing`]; [`SolidError::InvalidInput`] for a `rise` that is
/// not finite.
pub fn pair(
    contact: Contact,
    candidates: &[Plane],
    n: Vec3,
    rise: f64,
    tol: f64,
    strip_extent: impl Fn(Vec3) -> Result<(f64, f64)>,
) -> Result<core::result::Result<PairJoint, PairSkip>> {
    check_tol(tol)?;
    if !rise.is_finite() {
        return Err(invalid("the tab plate's rise is not finite"));
    }
    if !matches!(contact, Contact::Touch { .. } | Contact::Overlap) {
        return Ok(Err(PairSkip::Contact(contact)));
    }
    match plate_faces(candidates, n, tol, strip_extent)? {
        Ok(faces) if faces.thickness >= rise - tol => Ok(Err(PairSkip::NotSquare(None))),
        Ok(faces) => Ok(Ok(PairJoint { contact, faces })),
        Err(NoPlateFaces::NoPair) => {
            for f in candidates {
                if let Bearing::Oblique(deg) = bearing(n, f.normal)? {
                    return Ok(Err(PairSkip::NotSquare(Some(deg))));
                }
            }
            Ok(Err(PairSkip::NoPlateFaces(NoPlateFaces::NoPair)))
        }
        Err(why) => Ok(Err(PairSkip::NoPlateFaces(why))),
    }
}

/// Swapped [m10]: `forward` is the pair (T, S); when it is skipped for want of T's plate faces or
/// because T lies face to face on S, `reverse` (S as tab plate, T as slot plate) is tried, and a
/// joint there makes the skip [`PairSkip::Swapped`]; otherwise the first reason stands. Any
/// other outcome of `forward` is returned as it is and `reverse` is not asked.
///
/// # Errors
///
/// Those of `forward` and `reverse`.
pub fn with_swap<J>(
    forward: impl FnOnce() -> Result<core::result::Result<J, PairSkip>>,
    reverse: impl FnOnce() -> Result<core::result::Result<J, PairSkip>>,
) -> Result<core::result::Result<J, PairSkip>> {
    match forward()? {
        Err(skip @ (PairSkip::NoPlateFaces(_) | PairSkip::NotSquare(None))) => {
            Ok(Err(match reverse()? {
                Ok(_) => PairSkip::Swapped,
                Err(_) => skip,
            }))
        }
        other => Ok(other),
    }
}

/// One face of a body as [`pair_at`] reads it over a frame: its outward plane when it is planar,
/// and its vertices' least and greatest heights.
#[derive(Clone, Copy, Debug)]
struct FaceRead {
    face: crate::topology::FaceId,
    plane: Option<Plane>,
    low: f64,
    high: f64,
}

/// Every face of `b` read over `frame` ([`FaceRead`]); nothing is changed and no region is built.
fn read_faces(
    k: &crate::kernel::Kernel,
    b: &crate::topology::Brep,
    frame: &crate::eval::Frame,
    tol: f64,
) -> Result<Vec<FaceRead>> {
    let read = std::cell::RefCell::new(Vec::new());
    crate::prism::sheet::planar_face_regions(k, b, frame, tol, |face, normal| {
        let loops = crate::query::api_queries::face_get_boundary_loops(b, face)?;
        let (mut low, mut high, mut at) = (f64::INFINITY, f64::NEG_INFINITY, None);
        for &e in &loops.edges {
            let ends = [
                crate::topology::edge::start_vertex(b, e)?,
                crate::topology::edge::end_vertex(b, e)?,
            ];
            for v in ends.into_iter().flatten() {
                let q = b.vertex(v)?.point;
                let h = crate::prism::profile::height(frame, q);
                low = low.min(h);
                high = high.max(h);
                at.get_or_insert(q);
            }
        }
        let plane = match (normal, at) {
            (Some(normal), Some(at)) => {
                let natural = crate::prism::measure::outward_is_natural(b, face).ok_or(
                    SolidError::Unsupported {
                        operation: OP,
                        reason: "a face whose outward side is not decided",
                    },
                )?;
                Some(Plane::through(at, if natural { normal } else { -normal })?)
            }
            _ => None,
        };
        read.borrow_mut().push(FaceRead {
            face,
            plane,
            low,
            high,
        });
        Ok(false)
    })?;
    Ok(read.into_inner())
}

/// The frame of `p`: its origin the foot of the perpendicular from the world origin, its x axis
/// the world axis least along `n` made square to `n`, its y axis `n × x` (so `x × y = n`).
fn frame_of(p: Plane) -> Result<crate::eval::Frame> {
    let n = p.normal;
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    let e = if ax <= ay && ax <= az {
        Vec3::X
    } else if ay <= az {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let x = finite_unit(e - n * e.dot(n))?;
    let y = n.cross(x);
    let o = n * p.offset;
    Ok(crate::eval::Frame {
        origin: [o.x, o.y, o.z],
        x_axis: [x.x, x.y, x.z],
        y_axis: [y.x, y.y, y.z],
    })
}

/// S's faces in `p` (design 7.4's face region): the regions, in the frame of `p` ([`pair_at`]'s
/// `(u, w)` plane: x the world axis least along `n` made square to `n`, y `n × x`), of S's planar
/// faces facing `n` with every vertex within `tol` of `p`, holes included. Empty when S has no
/// face there. S is not changed.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `tol` not positive and finite or a bad plane; those of the
/// face reads.
pub fn slot_face(
    k: &mut crate::kernel::Kernel,
    s: &crate::topology::Brep,
    p: Plane,
    tol: f64,
) -> Result<Vec<crate::prism::profile::Region2>> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid("tol must be positive and finite"));
    }
    check_plane(p)?;
    let frame = frame_of(p)?;
    face_regions_in(k, s, p, &frame, tol)
}

/// [`slot_face`] in `frame` (`p`'s, already checked).
fn face_regions_in(
    k: &mut crate::kernel::Kernel,
    s: &crate::topology::Brep,
    p: Plane,
    frame: &crate::eval::Frame,
    tol: f64,
) -> Result<Vec<crate::prism::profile::Region2>> {
    let n = p.normal;
    let up = SQUARE_SLACK.cos();
    let s_on: Vec<_> = read_faces(k, s, frame, tol)?
        .into_iter()
        .filter(|f| {
            f.plane.is_some_and(|q| q.normal.dot(n) >= up) && f.low >= -tol && f.high <= tol
        })
        .map(|f| f.face)
        .collect();
    Ok(
        crate::prism::sheet::planar_face_regions(k, s, frame, tol, |f, _| Ok(s_on.contains(&f)))?
            .regions,
    )
}

/// The strip Σ of the pair (T, S) at `p` (design 5.5's input), as [`pair_at`] chooses it: the
/// first region of T's footprint on `p` ([`footprint_on`]) sharing an area with S's faces in `p`
/// ([`slot_face`]), or, when that is no touch over an area of S's face, the first such region of
/// T's section by `p` ([`footprint_across`]). In the frame of `p` ([`slot_face`]'s `(u, w)`
/// plane). `None` when T meets no area of S's face there. Neither body is changed.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `tol` not positive and finite or a bad plane;
/// `Unsupported` for an overlap not decided; those of the reads and the section.
pub fn strip(
    k: &mut crate::kernel::Kernel,
    t: &crate::topology::Brep,
    s: &crate::topology::Brep,
    p: Plane,
    tol: f64,
) -> Result<Option<crate::prism::profile::Region2>> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid("tol must be positive and finite"));
    }
    check_plane(p)?;
    let frame = frame_of(p)?;
    let t_faces = read_faces(k, t, &frame, tol)?;
    let touch = first_touch(&t_faces, p, tol)?;
    Ok(sigma_of(k, t, s, p, &frame, touch, tol)?.0)
}

/// The direction the strip of a joint runs in, in the frame of `p` ([`slot_face`]'s `(u, w)`
/// plane), for [`strip_defects`]: the plate face `a`'s outward normal projected into that frame
/// and turned a quarter left, so the strip runs along the plate, square to its thickness. A
/// unit vector.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a bad `p`, or an `a` whose normal has no part in `p`'s
/// plane (a face parallel to `p` is no plate face of a joint at `p`).
pub fn strip_direction(p: Plane, a: Plane) -> Result<crate::math::Vec2> {
    check_plane(p)?;
    let frame = frame_of(p)?;
    let [xx, xy, xz] = frame.x_axis;
    let [yx, yy, yz] = frame.y_axis;
    let m = a.normal;
    let (mu, mw) = (m.dot(Vec3::new(xx, xy, xz)), m.dot(Vec3::new(yx, yy, yz)));
    let len = mu.hypot(mw);
    if !(len.is_finite() && len > SQUARE_SLACK.sin()) {
        return Err(invalid("the plate face's normal has no part in p's plane"));
    }
    Ok(crate::math::Vec2::new(-mw / len, mu / len))
}

/// The strip's width: `Σ`'s extent across its run `d`, along `w`, a quarter turn left of `d` (as
/// [`strip_defects`] reads it); for a tab plate standing on `p` these are the levels of its faces
/// A and B the slots span.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `tol` not positive and finite, a `d` that is not a finite
/// unit vector, or a strip whose extent cannot be found.
pub fn strip_across(
    sigma: &crate::prism::profile::Region2,
    d: crate::math::Vec2,
    tol: f64,
) -> Result<(f64, f64)> {
    check_tol(tol)?;
    if !(d.x.is_finite() && d.y.is_finite() && (d.x.hypot(d.y) - 1.0).abs() <= 1e-9) {
        return Err(invalid("the strip's direction is not unit"));
    }
    sigma
        .extent_along(crate::math::Vec2::new(-d.y, d.x), tol)
        .ok_or_else(|| invalid("the strip's extent could not be found"))
}

/// S's thickness under `p` (design 6.1's `t_S`): how far below `p` the nearest of S's planar
/// faces facing away from `n` lies (its highest vertex's depth). `None` when S has no such face
/// below `p`. S is not changed.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `tol` not positive and finite or a bad plane; those of the
/// face reads.
pub fn slot_thickness(
    k: &mut crate::kernel::Kernel,
    s: &crate::topology::Brep,
    p: Plane,
    tol: f64,
) -> Result<Option<f64>> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid("tol must be positive and finite"));
    }
    check_plane(p)?;
    let frame = frame_of(p)?;
    let down = -SQUARE_SLACK.cos();
    Ok(read_faces(k, s, &frame, tol)?
        .into_iter()
        .filter(|f| f.plane.is_some_and(|q| q.normal.dot(p.normal) <= down) && f.high < -tol)
        .map(|f| -f.high)
        .min_by(f64::total_cmp))
}

/// Slot `k`'s prism (design 7.2): its outline in `p`'s frame ([`slot_face`]'s `(u, w)` plane)
/// extruded along `n` from `−t_S − over` to `+over`, `over` the overshoot past S's two faces.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a bad plane, a `t_s` not finite and positive or an `over`
/// not finite and non-negative; those of the extrusion.
pub fn slot_prism(
    k: &mut crate::kernel::Kernel,
    p: Plane,
    outline: &crate::prism::profile::Region2,
    t_s: f64,
    over: f64,
) -> Result<crate::topology::Brep> {
    check_plane(p)?;
    if !(t_s.is_finite() && t_s > 0.0 && over.is_finite() && over >= 0.0) {
        return Err(invalid(
            "the slot plate's thickness must be positive and the overshoot non-negative",
        ));
    }
    let frame = frame_of(p)?;
    crate::prism::extrude::extrude_region(k, &frame, outline, -t_s - over, over)
}

/// Tab `k`'s prism (design 7.1): its outline in T's `(u, z)` plane (`u` along the strip's run
/// `d` in `p`'s frame, `z` along `n`, from `p`'s frame origin) extruded across the plate from
/// `w_A` to `w_B` (`faces`, the levels along `w`, a quarter turn left of `d`).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a bad plane, a `d` that is not a finite unit vector, or
/// faces that are not finite and ordered; those of the extrusion.
pub fn tab_prism(
    k: &mut crate::kernel::Kernel,
    p: Plane,
    d: crate::math::Vec2,
    outline: &crate::prism::profile::Region2,
    faces: (f64, f64),
) -> Result<crate::topology::Brep> {
    check_plane(p)?;
    if !(d.x.is_finite() && d.y.is_finite() && (d.x.hypot(d.y) - 1.0).abs() <= 1e-9) {
        return Err(invalid("the strip's direction is not unit"));
    }
    let (wa, wb) = faces;
    if !(wa.is_finite() && wb.is_finite() && wa < wb) {
        return Err(invalid("the tab plate's faces must be finite and ordered"));
    }
    let pf = frame_of(p)?;
    let [xx, xy, xz] = pf.x_axis;
    let [yx, yy, yz] = pf.y_axis;
    let u = Vec3::new(xx, xy, xz) * d.x + Vec3::new(yx, yy, yz) * d.y;
    let n = p.normal;
    // The frame (u, n) has normal u × n = −w, so w ∈ [w_A, w_B] is the level span [−w_B, −w_A].
    let frame = crate::eval::Frame {
        origin: pf.origin,
        x_axis: [u.x, u.y, u.z],
        y_axis: [n.x, n.y, n.z],
    };
    crate::prism::extrude::extrude_region(k, &frame, outline, -wb, -wa)
}

/// The touch level of T's first planar face within `tol` of `p` ([`touch_offset`]).
fn first_touch(t_faces: &[FaceRead], p: Plane, tol: f64) -> Result<Option<f64>> {
    for q in t_faces.iter().filter_map(|f| f.plane) {
        if let Some(delta) = touch_offset(q, p, tol)? {
            return Ok(Some(delta));
        }
    }
    Ok(None)
}

/// Σ as [`pair_at`] chooses it, with whether it came from T's faces on `p` (`on_has_area`) or
/// from T's section by `p` (`across_has_area`); `frame` is `p`'s, already checked.
fn sigma_of(
    k: &mut crate::kernel::Kernel,
    t: &crate::topology::Brep,
    s: &crate::topology::Brep,
    p: Plane,
    frame: &crate::eval::Frame,
    touch: Option<f64>,
    tol: f64,
) -> Result<(Option<crate::prism::profile::Region2>, bool, bool)> {
    use crate::prism::profile::{regions_overlap, Region2};
    let s_regions = face_regions_in(k, s, p, frame, tol)?;
    let on_s = |a: &Region2| -> Result<bool> {
        for b in &s_regions {
            match regions_overlap(a, b, tol) {
                Some(true) => return Ok(true),
                Some(false) => {}
                None => {
                    return Err(SolidError::Unsupported {
                        operation: OP,
                        reason: "whether the tab plate meets the slot plate's face is not decided",
                    })
                }
            }
        }
        Ok(false)
    };
    let mut sigma = None;
    for r in footprint_on(k, t, frame, tol)? {
        if on_s(&r)? {
            sigma = Some(r);
            break;
        }
    }
    let on_has_area = sigma.is_some();
    let mut across_has_area = false;
    if !(on_has_area && touch.is_some()) {
        for r in footprint_across(k, t, frame, tol)? {
            if on_s(&r)? {
                sigma = Some(r);
                across_has_area = true;
                break;
            }
        }
    }
    Ok((sigma, on_has_area, across_has_area))
}

/// The pair (T, S) at `p` read from the two bodies and decided by [`pair`] (design v2 5.3, 5.4):
/// S's faces in `p` facing `n` (every vertex within `tol` of it), T's footprint on `p`
/// ([`footprint_on`]) and the touch level of T's first planar face facing `−n` within `tol`
/// ([`touch_offset`]); T's section by `p` ([`footprint_across`]) only when that is no touch over
/// an area of S's face; the bodies' distance ([`boundary_distance`], on copies, bracketed to
/// `tol`) only when neither holds. The strip Σ is the first footprint region sharing an area with
/// S's faces (its extent stands for the strip's: no boolean runs); the candidates are T's planar
/// faces; the rise is the greatest height of T's vertices over `p`. Neither body is changed.
///
/// [`boundary_distance`]: crate::query::contact::boundary_distance
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for `tol` not positive and finite or a bad plane;
/// `Unsupported` for an overlap or a distance not decided, a face whose outward side is not
/// decided; those of the reads, the section and [`pair`].
pub fn pair_at(
    k: &mut crate::kernel::Kernel,
    t: &crate::topology::Brep,
    s: &crate::topology::Brep,
    p: Plane,
    tol: f64,
) -> Result<core::result::Result<PairJoint, PairSkip>> {
    if !(tol.is_finite() && tol > 0.0) {
        return Err(invalid("tol must be positive and finite"));
    }
    check_plane(p)?;
    let n = p.normal;
    let frame = frame_of(p)?;
    let t_faces = read_faces(k, t, &frame, tol)?;
    let touch = first_touch(&t_faces, p, tol)?;
    let (sigma, on_has_area, across_has_area) = sigma_of(k, t, s, p, &frame, touch, tol)?;
    let how = contact(on_has_area, touch, across_has_area, tol, || {
        let mut a = crate::topology::brep::p3::brep_copy(k, t)?;
        let mut b = crate::topology::brep::p3::brep_copy(k, s)?;
        let c = crate::query::contact::boundary_distance(k, &mut a, &mut b, tol)?;
        match c.verdict {
            crate::geom::certified::Verdict::Yes => Ok(c.upper),
            _ => Err(SolidError::Unsupported {
                operation: OP,
                reason: "the plates' distance is not decided",
            }),
        }
    })?;
    let candidates: Vec<Plane> = t_faces.iter().filter_map(|f| f.plane).collect();
    let rise = t_faces
        .iter()
        .map(|f| f.high)
        .fold(f64::NEG_INFINITY, f64::max);
    pair(how, &candidates, n, rise, tol, |m| match &sigma {
        Some(r) => strip_extent(r, &frame, m, tol),
        None => Err(invalid("there is no strip")),
    })
}

/// A face bounding the strip Σ across it (along `w`), as [`eased_edge`] reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StripBound {
    /// A planar face: its unit normal and its extent along `n` (how tall it stands over P).
    Plane {
        /// The face's unit normal.
        normal: Vec3,
        /// Its extent along `n`, mm.
        height: f64,
    },
    /// A cylindrical face: its unit axis.
    Cylinder {
        /// The cylinder's unit axis.
        axis: Vec3,
    },
    /// A face on any other surface.
    Other,
}

/// Whether T's edge at S is eased (design v2 5.4, [m11]): a face bounding Σ across the strip is a
/// planar face at an angle to `n` (neither square to it nor parallel, by [`bearing`]) narrower
/// along `n` than `t` by more than `tol` (a chamfer), or a cylindrical face whose axis runs along
/// the strip `along` (a round). The pair is then skipped `eased_edge`, not `not_square`.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a non-unit `n`, `along`, normal or axis, a non-finite
/// height, or `t` or `tol` not positive and finite.
pub fn eased_edge(bounds: &[StripBound], n: Vec3, along: Vec3, t: f64, tol: f64) -> Result<bool> {
    if !(t.is_finite() && t > 0.0 && tol.is_finite() && tol > 0.0) {
        return Err(invalid("thickness and tol must be positive and finite"));
    }
    check_unit(n)?;
    check_unit(along)?;
    let mut eased = false;
    for b in bounds {
        eased |= match *b {
            StripBound::Plane { normal, height } => {
                if !height.is_finite() {
                    return Err(invalid("a bound's height must be finite"));
                }
                matches!(bearing(n, normal)?, Bearing::Oblique(_)) && height < t - tol
            }
            StripBound::Cylinder { axis } => {
                matches!(bearing(along, axis)?, Bearing::Same | Bearing::Opposite)
            }
            StripBound::Other => false,
        };
    }
    Ok(eased)
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
