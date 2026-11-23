//! Where a spot weld sits: from the stacked sheets and a point picked on one of their outer
//! faces, the spot's frame and every sheet it joins ([`spot_site`]), or why it can't be.
//!
//! The spot's line runs from the picked point along the picked face's inward normal, through
//! the stack. Each listed sheet must be crossed exactly once, the sheets must follow one
//! another along the line without a gap or an overlap, every face crossed must be flat and
//! square to the line, and the spot's footprint (its diameter about the line) must lie inside
//! every such face. The sheets are read, never changed: every solve runs on a copy.

use super::OP_SPOT;
use crate::error::weld::WeldFailure;
use crate::error::{Failure, Fit, FitMeasure, Reason, Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{BrepQuery, Ray};
use crate::topology::edgeuse::p1::faceuse_orientation;
use crate::topology::{face, Brep, FaceId};

/// One sheet a spot joins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotSheet {
    /// The sheet, by its place in the list given to [`spot_site`].
    pub sheet: usize,
    /// The face the spot's line enters it by.
    pub near: FaceId,
    /// The face it leaves it by.
    pub far: FaceId,
    /// The distance between the two along the line.
    pub thickness: f64,
}

/// Where a spot weld sits: what [`super::SpotWeld`] needs, and the faces it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotSite {
    /// The spot's centre on the faying face between the first two sheets.
    pub at: Point3,
    /// The spot's unit line, from the picked face into the stack.
    pub normal: Vec3,
    /// The sheets in stack order along `normal`, the picked one first.
    pub sheets: Vec<SpotSheet>,
}

fn refuse(why: WeldFailure) -> SolidError {
    Failure::new(OP_SPOT, Reason::Weld(why)).into_error()
}

/// The site of a spot of `diameter` picked at `point` on face `face` of sheet `picked`, the
/// sheets of the stack listed in `sheets` (any order, at least two).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for fewer than two sheets, a picked sheet not in the list, a
/// face id the picked sheet does not have, a point that is not finite or a diameter that is not finite
/// and positive. A [`WeldFailure`] where the stack does not take the spot: the point off the
/// picked face ([`WeldFailure::SpotPointOffFace`]), a listed sheet beyond the picked face
/// ([`WeldFailure::SpotFaceNotOuter`]), missed ([`WeldFailure::SpotSheetMissed`]) or crossed
/// more than once ([`WeldFailure::SpotSheetCrossedAgain`]), sheets that overlap
/// ([`WeldFailure::SpotSheetsOverlap`]) or don't touch ([`WeldFailure::SpotSheetsApart`]),
/// a face crossed that is not flat ([`WeldFailure::SpotFaceNotFlat`]) or not square to the
/// line ([`WeldFailure::SpotFaceNotSquare`]), and a footprint past a face's edge
/// ([`WeldFailure::SpotPastFaceEdge`], with the largest diameter that fits as its [`Fit`]),
/// or an edge whose distance from the line was not found
/// ([`WeldFailure::SpotEdgeNotMeasured`]).
/// Those of the copies, the ray solves and the edge distances otherwise.
pub fn spot_site(
    k: &Kernel,
    sheets: &[&Brep],
    picked: usize,
    face: FaceId,
    point: Point3,
    diameter: f64,
) -> Result<SpotSite> {
    let bad = |reason| Err(SolidError::invalid_input(OP_SPOT, reason));
    if sheets.len() < 2 {
        return bad("fewer than two sheets");
    }
    let Some(&body) = sheets.get(picked) else {
        return bad("a picked sheet not in the list");
    };
    if body.face(face).is_err() {
        return bad("a picked face the picked sheet does not have");
    }
    if !(point.x.is_finite() && point.y.is_finite() && point.z.is_finite()) {
        return bad("a spot point that is not finite");
    }
    if !(diameter.is_finite() && diameter > 0.0) {
        return bad("a spot diameter that is not finite and positive");
    }
    let tol = k.tolerances.zone_tol_3d;
    let normal = -outward_normal(k, body, picked, face)?;

    // the point on the picked face first, then every sheet crossed once
    let on_face = hits_along(k, body, point, normal, tol)?
        .first()
        .is_some_and(|h| h.face == face && h.distance <= tol);
    if !on_face {
        return Err(refuse(WeldFailure::SpotPointOffFace));
    }
    let mut crossed: Vec<(usize, Hit, Hit)> = Vec::new();
    for (i, &b) in sheets.iter().enumerate() {
        let hits = hits_along(k, b, point, normal, tol)?;
        // shortcut: a line through an edge meets both its faces at one distance and is refused
        // as crossed again, not as past the face's edge; merge hits at one distance if the
        // app needs the right name
        match hits.as_slice() {
            [near, far] => crossed.push((i, *near, *far)),
            [] => {
                let behind = !hits_along(k, b, point, -normal, tol)?.is_empty();
                return Err(refuse(if behind {
                    WeldFailure::SpotFaceNotOuter { sheet: i }
                } else {
                    WeldFailure::SpotSheetMissed { sheet: i }
                }));
            }
            // one hit: the point lies inside that sheet
            [_] => return Err(refuse(WeldFailure::SpotSheetsOverlap { sheet: i })),
            _ => return Err(refuse(WeldFailure::SpotSheetCrossedAgain { sheet: i })),
        }
    }
    crossed.sort_by(|a, b| a.1.distance.total_cmp(&b.1.distance));
    for pair in crossed.windows(2) {
        let [(_, _, before), (sheet, next, _)] = pair else {
            continue;
        };
        let gap = next.distance - before.distance;
        if gap < -tol {
            return Err(refuse(WeldFailure::SpotSheetsOverlap { sheet: *sheet }));
        }
        if gap > tol {
            return Err(refuse(WeldFailure::SpotSheetsApart { sheet: *sheet, gap }));
        }
    }

    // every face crossed flat and square, and the footprint inside it
    let sin_tol = k.tolerances.ang_tol_deg.to_radians().sin();
    let mut room = f64::INFINITY;
    let mut tightest = None;
    for &(sheet, near, far) in &crossed {
        let b = sheets.get(sheet).copied().unwrap_or(body);
        for hit in [near, far] {
            let n = outward_normal(k, b, sheet, hit.face)?;
            if n.cross(normal).length() > sin_tol {
                return Err(refuse(WeldFailure::SpotFaceNotSquare {
                    sheet,
                    face: hit.face,
                }));
            }
            for e in face::p4::edges(b, hit.face, None, false, None)? {
                let Some(c) = crate::query::edge::closest_point(k, b, e, hit.point)? else {
                    return Err(refuse(WeldFailure::SpotEdgeNotMeasured {
                        sheet,
                        face: hit.face,
                    }));
                };
                let d = c.distance;
                if d < room {
                    room = d;
                    tightest = Some((sheet, hit.face));
                }
            }
        }
    }
    if let Some((sheet, face)) = tightest.filter(|_| 2.0 * room < diameter) {
        return Err(Failure::new(
            OP_SPOT,
            Reason::Weld(WeldFailure::SpotPastFaceEdge {
                sheet,
                face,
                diameter,
            }),
        )
        .with_fit(Fit::new(2.0 * room, FitMeasure::Diameter, None))
        .into_error());
    }

    let first = crossed
        .first()
        .map_or(0.0, |(_, near, far)| far.distance - near.distance);
    Ok(SpotSite {
        at: point + normal * first,
        normal,
        sheets: crossed
            .into_iter()
            .map(|(sheet, near, far)| SpotSheet {
                sheet,
                near: near.face,
                far: far.face,
                thickness: far.distance - near.distance,
            })
            .collect(),
    })
}

/// Where a line meets a sheet's face: the face, the distance along the line, the point.
#[derive(Debug, Clone, Copy)]
struct Hit {
    face: FaceId,
    distance: f64,
    point: Point3,
}

/// The first three faces of `b` the line from `from` along unit `dir` meets, nearest first
/// (enough to tell a sheet crossed once from one crossed again). The ray fire's solve gives
/// the first face only, so each next one is fired from just past the last, on a copy of `b`.
fn hits_along(k: &Kernel, b: &Brep, from: Point3, dir: Vec3, tol: f64) -> Result<Vec<Hit>> {
    let mut copy = b.clone_standing_for_self()?;
    let query = BrepQuery::of_brep(&copy);
    let mut hits = Vec::new();
    let mut origin = from;
    while hits.len() < 3 {
        let Some(h) = query.first_face_hit(k, &mut copy, Ray::new(origin, dir)?)? else {
            break;
        };
        hits.push(Hit {
            face: h.face,
            distance: (h.point - from).dot(dir).max(0.0),
            point: h.point,
        });
        // shortcut: a sheet thinner than this step reads as crossed once, not twice
        origin = h.point + dir * (10.0 * tol);
    }
    Ok(hits)
}

/// The unit normal of flat face `f` of sheet `sheet`, pointing out of the sheet.
fn outward_normal(k: &Kernel, b: &Brep, sheet: usize, f: FaceId) -> Result<Vec3> {
    let not_flat = || refuse(WeldFailure::SpotFaceNotFlat { sheet, face: f });
    let surface = face::p2::surface(b, f)?.ok_or_else(not_flat)?;
    let planar = surface.is_planar(k, k.tolerances.zone_tol_3d)?;
    if !planar.is_planar {
        return Err(not_flat());
    }
    let n = planar.normal.unitize().map_err(|_| not_flat())?;
    let outer = face::p2::outer_faceuse(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP_SPOT, "a face with no outer use"))?;
    Ok(if faceuse_orientation(b, outer)? == OrientType::Same {
        n
    } else {
        -n
    })
}
