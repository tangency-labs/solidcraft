//! Remove a chain of four-sided rounds as one local edit. The selected neighbours
//! identify the ends of each section even when all four of its edges are tangent.

use std::collections::{BTreeMap, BTreeSet};

use super::chain_edit::{remove_chain, Junction, Section};
use super::{
    bspline_part_with_nurb, defect, delete_fillet_face, edge_curve, editing_disabled, face_surface,
    null, own, refused,
};
use crate::blend::fillet_exact::{
    circle_spine_station, classify_blend_spine, BlendQuadric, BlendSpine, ExactBlend, QuadricKind,
};
use crate::curve::curve::Curve;
use crate::curve::ellipse::Ellipse;
use crate::curve::line::Line;
use crate::error::blend::{BlendFailure, RemovalRefusal as R};
use crate::error::{Result, SolidError};
use crate::geom::context::Context;
use crate::geom::geom_utility::p2::{intersect_two_planes, PlanesIntersection};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::implicit::{curve_lies_on, AnalyticSupport};
use crate::topology::brep::validation;
use crate::topology::{edge, face, vertex, Brep, EdgeId, FaceId, VertexId};

/// Delete the selected round sections together and join their surviving supports along
/// the sharp edges. A single face uses the ordinary straight or ring removal. A group
/// must form one open or closed chain. A refusal restores the original B-rep.
///
/// # Errors
///
/// Returns an error for an empty or repeated selection, a non-manifold or incomplete
/// chain, or when the sharp edges or the side edges cannot be continued on their supports.
pub fn delete_fillet_faces(
    k: &mut Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    tol_3d: Option<f64>,
) -> Result<()> {
    let Some(&first) = faces.first() else {
        return Err(null("no fillet faces selected"));
    };
    if !b.editing_enabled() {
        return Err(editing_disabled());
    }
    let selected: BTreeSet<_> = faces.iter().copied().collect();
    if selected.len() != faces.len() {
        return Err(null("a fillet face was selected more than once"));
    }
    let tol = match tol_3d {
        Some(t) => t,
        None => selected.iter().try_fold(0_f64, |t, &id| -> Result<f64> {
            let face_tol = face::p2::tolerance(b, id)?;
            if !face_tol.is_finite() || face_tol < 0.0 {
                return Err(null("invalid fillet removal tolerance"));
            }
            Ok(t.max(face_tol))
        })?,
    };
    if !tol.is_finite() || tol <= 0.0 {
        return Err(null("invalid fillet removal tolerance"));
    }
    let edit = |k: &mut Kernel| {
        b.with_rollback_untouched(
            |b| {
                if faces.len() == 1 {
                    return delete_fillet_face(k, b, first, Some(tol));
                }
                let mut bands = BTreeMap::new();
                for &id in &selected {
                    bands.insert(id, Band::read(k, b, id, &selected)?);
                }
                check_connected(first, &bands)?;
                let mut sections = Vec::new();
                let mut junctions = BTreeMap::new();
                for (&id, band) in &bands {
                    sections.push(band.plan(k, b, id, tol, &mut junctions)?);
                }
                let input = validation::standing(k, &mut b.clone_standing_for_self()?)?;
                let junctions: Vec<_> = junctions.into_values().collect();
                remove_chain(k, b, &sections, &junctions, tol)?;
                b.refresh_cached_topology();
                let found = validation::standing(k, &mut b.clone_standing_for_self()?)?;
                validate_chain(&input, &found)
            },
            Result::is_ok,
        )
    };
    Context::with_committed_notifications(k, edit)
}

fn validate_chain(input: &validation::Standing, found: &validation::Standing) -> Result<()> {
    let why = if let Some(check) = found.check.first_new_failure_name(&input.check) {
        check
    } else if found.check.assert_valid.is_none() {
        "the edited body's geometry could not be checked"
    } else if input.manifold && !found.manifold {
        "the chain edit made a manifold solid non-manifold"
    } else {
        return Ok(());
    };
    Err(SolidError::blend(
        "blend::fillet_removal::validate_chain",
        BlendFailure::Defect { why },
    ))
}

fn neighbour(b: &Brep, edge_id: EdgeId, face_id: FaceId) -> Result<FaceId> {
    let faces = edge::faces(b, edge_id, None)?;
    match *faces.as_slice() {
        [a, other] if a == face_id && a != other => Ok(other),
        [other, a] if a == face_id && a != other => Ok(other),
        _ => Err(refused(R::EdgeNotBetweenTwoFaces)),
    }
}

fn endpoints(b: &Brep, edge_id: EdgeId) -> Result<[VertexId; 2]> {
    match edge::start_end_vertices(b, edge_id)? {
        (Some(a), Some(z)) if a != z => Ok([a, z]),
        _ => Err(refused(R::EdgeWithoutTwoVertices)),
    }
}

struct Band {
    rails: [EdgeId; 2],
    supports: [FaceId; 2],
    ends: [EdgeId; 2],
    adjacent: [Option<FaceId>; 2],
    end_faces: [FaceId; 2],
}

impl Band {
    fn read(k: &Kernel, b: &mut Brep, id: FaceId, selected: &BTreeSet<FaceId>) -> Result<Self> {
        let edges = face::p4::edges(b, id, None, false, None)?;
        let mut around = Vec::with_capacity(4);
        for eid in edges {
            around.push((eid, neighbour(b, eid, id)?));
        }
        let [first, second, third, fourth]: [(_, _); 4] =
            around.try_into().map_err(|_| refused(R::NotFourEdges))?;
        // The two ends are opposite. Either one can be the selected join of an open strip.
        let [(end0, next0), (rail0, support0), (end1, next1), (rail1, support1)] =
            if selected.contains(&first.1) || selected.contains(&third.1) {
                [first, second, third, fourth]
            } else if selected.contains(&second.1) || selected.contains(&fourth.1) {
                [second, third, fourth, first]
            } else {
                return Err(refused(R::NotOneTangentChain));
            };
        let supports = [support0, support1];
        if supports[0] == supports[1] || supports.iter().any(|f| selected.contains(f)) {
            return Err(refused(R::RailsNotBetweenTwoFaces));
        }
        let rails = [rail0, rail1];
        for &rail in &rails {
            if !edge::is_tangent_edge(k, b, rail, 2.0, false)? {
                return Err(refused(R::OppositeEdgeNotTangent));
            }
        }
        let ends = [end0, end1];
        let end_faces = [next0, next1];
        let adjacent = end_faces.map(|f| selected.contains(&f).then_some(f));
        for (&end, next) in ends.iter().zip(adjacent) {
            if edge::is_tangent_edge(k, b, end, 2.0, false)? != next.is_some() {
                return Err(refused(R::AdjacentEdgesTangent));
            }
        }
        Ok(Self {
            rails,
            supports,
            ends,
            adjacent,
            end_faces,
        })
    }

    fn plan(
        &self,
        k: &Kernel,
        b: &Brep,
        id: FaceId,
        tol: f64,
        junctions: &mut BTreeMap<EdgeId, Junction>,
    ) -> Result<Section> {
        let rail = edge_curve(b, self.rails[0])?;
        let mid = rail.evaluate_point(k, edge::interval(b, self.rails[0])?.mid())?;
        let [a, z] = self.supports;
        let surfaces = [face_surface(b, a)?, face_surface(b, z)?];
        // At zero offset the existing blend spine classifier intersects the support
        // surfaces themselves. The nearest meridian crossing picks the local branch.
        let quadric = |s: &crate::surface::surface::Surface| {
            BlendQuadric::from_surface(k, s, s.natural_uv_domain_with_nurb(k)?.mid(), tol)
        };
        let faces = [quadric(surfaces[0])?, quadric(surfaces[1])?];
        let mut spine = ExactBlend {
            faces,
            offsets: faces,
            ..ExactBlend::default()
        };
        if faces.iter().all(|q| q.kind == QuadricKind::Plane) {
            let [a, b] = faces;
            let PlanesIntersection::Line { point, direction } =
                intersect_two_planes(a.point, a.dir, b.point, b.dir)?
            else {
                return Err(refused(R::TangentSurfacesDoNotMeet));
            };
            spine.spine = BlendSpine::Line;
            spine.point = point;
            spine.dir = direction;
        } else {
            classify_blend_spine(mid, tol, &mut spine);
        }
        let rail0 = endpoints(b, self.rails[0])?;
        let rail1 = endpoints(b, self.rails[1])?;
        let mut corners = [Point3::ZERO; 2];
        let mut orientation = OrientType::Unknown;
        for ((eid, next), end_face) in self.ends.into_iter().zip(self.adjacent).zip(self.end_faces)
        {
            let vertices = endpoints(b, eid)?;
            let start0 = vertices.contains(&rail0[0]);
            let start1 = vertices.contains(&rail1[0]);
            if start0 == vertices.contains(&rail0[1]) || start1 == vertices.contains(&rail1[1]) {
                return Err(refused(R::SeamDoesNotJoinRails));
            }
            let sense = if start0 == start1 {
                OrientType::Same
            } else {
                OrientType::Opposite
            };
            if orientation != OrientType::Unknown && orientation != sense {
                return Err(refused(R::SeamDoesNotJoinRails));
            }
            orientation = sense;
            let near = (vertex::point(b, vertices[0])? + vertex::point(b, vertices[1])?) * 0.5;
            let (origin, normal) = if next.is_some() {
                // A shared circular section lies in the rolling ball's normal plane.
                let curve = edge_curve(b, eid)?;
                let arc = Ellipse::is_nurb_curve_circle(k, curve, None)?
                    .ok_or_else(|| refused(R::CornerNeedsFittedIntersection))?;
                let placement = arc.canonical().position;
                (placement.origin(), placement.z_axis())
            } else {
                let end = face_surface(b, end_face)?.is_planar(k, tol)?;
                if !end.is_planar {
                    return Err(refused(R::CornerNeedsFittedIntersection));
                }
                (end.point, end.normal.unitize()?)
            };
            let point = sharp_corner(&spine, near, origin, normal)?;
            let joint = junctions.entry(eid).or_insert(Junction {
                cross_edge: eid,
                vertices,
                point,
            });
            if joint.point.distance(point) > tol {
                return Err(refused(R::SeamsCrossApart));
            }
            let [from, to] = &mut corners;
            *(if start0 { from } else { to }) = joint.point;
        }
        let [from, to] = corners;
        if from.distance(to) <= tol {
            return Err(refused(R::TangentSurfacesDoNotMeet));
        }
        let sharp = match spine.spine {
            BlendSpine::Line => Curve::from(Line::create_line_segment(3, from, to, None)?),
            BlendSpine::Circle => {
                let interior = circle_spine_station(&spine, mid)
                    .ok_or_else(|| refused(R::TangentSurfacesDoNotMeet))?;
                Curve::from(Ellipse::circle_from_3_points(
                    k, from, interior, to, 3, false,
                )?)
            }
            other => return Err(refused(R::UnsupportedSharpSpine(other))),
        };
        // Prove the entire new trim lies on both supports, including a long arc
        // across the parameter seam. A pair of accepted endpoints is insufficient.
        let net = own(
            bspline_part_with_nurb(&sharp)?,
            "a line or circle just made has a B-spline part",
        )?;
        for s in surfaces {
            let support =
                AnalyticSupport::of(s).ok_or_else(|| refused(R::TangentSurfacesDoNotMeet))?;
            if !curve_lies_on(&net, &support.implicit())? {
                return Err(refused(R::TangentSurfacesDoNotMeet));
            }
        }
        let [first, second] = self.rails;
        let budgets = [edge::tolerance(b, first)?, edge::tolerance(b, second)?];
        if !budgets.iter().all(|t| t.is_finite() && *t >= 0.0) {
            return Err(refused(R::RailTrimOutsideTolerance));
        }
        Ok(Section {
            face: id,
            rails: self.rails,
            rail_tolerance: budgets[0].min(budgets[1]),
            sharp,
            rail_orientation: orientation,
        })
    }
}

/// Intersect the sharp support curve with the section or the real terminal face.
fn sharp_corner(spine: &ExactBlend, near: Point3, origin: Point3, normal: Vec3) -> Result<Point3> {
    let missing = || refused(R::TangentSurfacesDoNotMeet);
    let p = spine.point;
    let d = spine.dir;
    match spine.spine {
        BlendSpine::Line => {
            let den = d.dot(normal);
            if den.abs() <= EFF_ZERO {
                return Err(missing());
            }
            Ok(p + d * ((origin - p).dot(normal) / den))
        }
        BlendSpine::Circle => {
            let station = circle_spine_station(spine, near).ok_or_else(missing)?;
            let center = p + d * (station - p).dot(d);
            let projected = normal - d * normal.dot(d);
            let n = projected.unitize()?;
            let h = (origin - center).dot(normal) / projected.length();
            let r = station.distance(center);
            if h.abs() >= r {
                return Err(missing());
            }
            let foot = center + n * h;
            let half = d.cross(n) * ((r - h) * (r + h)).sqrt();
            let (a, z) = (foot + half, foot - half);
            Ok(if a.distance(near) < z.distance(near) {
                a
            } else {
                z
            })
        }
        other => Err(refused(R::UnsupportedSharpSpine(other))),
    }
}

fn check_connected(first: FaceId, bands: &BTreeMap<FaceId, Band>) -> Result<()> {
    let mut visit = vec![first];
    let mut seen = BTreeSet::new();
    while let Some(f) = visit.pop() {
        if seen.insert(f) {
            let band = bands
                .get(&f)
                .ok_or_else(|| defect("a band's neighbours are drawn from the bands"))?;
            visit.extend(band.adjacent.into_iter().flatten());
        }
    }
    if seen.len() != bands.len() {
        return Err(refused(R::NotOneTangentChain));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::assert_array::{labels::SURFACE_TYPE, AssertArray, AssertReport, ReportSpec};

    #[test]
    fn a_conic_support_intersection_names_the_unsupported_sharp_curve() {
        let spine = ExactBlend {
            spine: BlendSpine::Conic,
            ..Default::default()
        };
        let err = sharp_corner(&spine, Point3::ZERO, Point3::ZERO, Vec3::Z).unwrap_err();
        assert_eq!(
            err.blend_failure(),
            Some(&BlendFailure::RemovalRefused(R::UnsupportedSharpSpine(
                BlendSpine::Conic
            )))
        );
    }

    #[test]
    fn chain_validation_identifies_the_check_that_refused_the_edit() {
        let input = validation::Standing {
            check: validation::BrepCheck {
                pointers_status: crate::error::SM_SUCCESS,
                pointer_reports: AssertArray::new(),
                assert_valid: Some(true),
                reports: AssertArray::new(),
            },
            manifold: true,
        };
        let check_reason = |found: &validation::Standing, why| {
            let err = validate_chain(&input, found).unwrap_err();
            assert_eq!(err.blend_failure(), Some(&BlendFailure::Defect { why }));
        };
        let mut found = input.clone();
        found.manifold = false;
        check_reason(&found, "the chain edit made a manifold solid non-manifold");
        found.check.assert_valid = None;
        check_reason(&found, "the edited body's geometry could not be checked");
        let report = AssertReport::new(ReportSpec {
            test_class: SURFACE_TYPE,
            test_index: 4,
            ..Default::default()
        })
        .unwrap();
        let name = report.name;
        found.check.assert_valid = Some(false);
        assert!(found.check.reports.add_unique_delete_duplicate(report));
        check_reason(&found, name);
        assert!(validate_chain(&found, &found).is_ok());
    }
}
