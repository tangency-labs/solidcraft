//! Truck surfaces in the evaluator's form ([`EvalSurface`]), which the CPU and GPU evaluators
//! share. Surfaces the evaluators cannot express (a revolved intersection curve) or that are
//! too large for them give `None`; their faces are evaluated here instead.

use solidcraft_geom::Vec3;
use solidcraft_geom::bspline::MAX_SPLINE_DEGREE;
use solidcraft_geom::tess::{EvalCurve, EvalSurface, MAX_EVAL_KNOTS, MAX_EVAL_POINTS};
use truck_meshalgo::prelude::*;
use truck_modeling as mt;

use crate::body::from_p3;

/// `surface` in the evaluator's form, parameterised exactly as truck parameterises it.
pub(crate) fn eval_surface(surface: &mt::Surface) -> Option<EvalSurface> {
    let s = match surface {
        mt::Surface::Plane(p) => {
            EvalSurface::Plane { origin: from_p3(p.subs(0.0, 0.0)), du: from_v3(p.uder(0.0, 0.0)), dv: from_v3(p.vder(0.0, 0.0)) }
        }
        mt::Surface::BSplineSurface(b) => {
            let ctrl = b.control_points().iter().map(|row| row.iter().map(|p| [p.x, p.y, p.z, 1.0]).collect()).collect();
            nurbs(b.degrees(), b.knot_vecs(), ctrl)?
        }
        mt::Surface::NurbsSurface(n) => {
            let ctrl = n.control_points().iter().map(|row| row.iter().map(|h| [h.x, h.y, h.z, h.w]).collect()).collect();
            nurbs((n.udegree(), n.vdegree()), n.knot_vecs(), ctrl)?
        }
        mt::Surface::RevolutedCurve(proc) => {
            let rev = proc.entity();
            let profile = eval_curve(rev.entity_curve())?;
            let axis = from_v3(rev.axis()).normalized()?;
            let m = proc.transform();
            // cgmath is column-major: m[column][row].
            let transform = [0, 1, 2].map(|r| [m[0][r], m[1][r], m[2][r], m[3][r]]);
            if transform.iter().flatten().any(|x| !x.is_finite()) {
                return None;
            }
            EvalSurface::Revolved { profile, origin: from_p3(rev.origin()), axis, transform, swap: !proc.orientation() }
        }
    };
    Some(s)
}

fn from_v3(v: mt::Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn knots(k: &mt::KnotVec) -> Option<Vec<f64>> {
    let v: Vec<f64> = k.iter().copied().collect();
    (v.len() <= MAX_EVAL_KNOTS && v.iter().all(|x| x.is_finite())).then_some(v)
}

/// Truck's `control_points[i][j]` (i along u) as the evaluator's u-fastest array.
fn nurbs((pu, pv): (usize, usize), kv: &(mt::KnotVec, mt::KnotVec), rows: Vec<Vec<[f64; 4]>>) -> Option<EvalSurface> {
    let nu = rows.len();
    let nv = rows.first()?.len();
    if pu == 0 || pv == 0 || pu > MAX_SPLINE_DEGREE || pv > MAX_SPLINE_DEGREE || nu.checked_mul(nv)? > MAX_EVAL_POINTS {
        return None;
    }
    let mut ctrl = Vec::with_capacity(nu * nv);
    for j in 0..nv {
        for row in &rows {
            ctrl.push(<[[f64; 4]]>::get(row, j).copied()?);
        }
    }
    if ctrl.iter().flatten().any(|x| !x.is_finite()) {
        return None;
    }
    let knots = [knots(&kv.0)?, knots(&kv.1)?];
    if !(knots[0].len() == nu + pu + 1 && knots[1].len() == nv + pv + 1) {
        return None;
    }
    let closed = [closed_along(0, &ctrl, [nu, nv], [pu, pv], &knots), closed_along(1, &ctrl, [nu, nv], [pu, pv], &knots)];
    Some(EvalSurface::Nurbs { degree: [pu, pv], knots, count: [nu, nv], ctrl, closed })
}

/// Does the surface join itself along direction `d` (a tube's way round)? Its knots there must
/// be clamped, so the ends of the range are the first and last rows of control points, and
/// those two rows must be the same points: the join is then the parameter line `d = end`, and
/// the direction is periodic.
fn closed_along(d: usize, ctrl: &[[f64; 4]], count: [usize; 2], degree: [usize; 2], knots: &[Vec<f64>; 2]) -> bool {
    let (n, p) = (count[d], degree[d]);
    if n < 2 {
        return false;
    }
    let Some(k) = knots.get(d) else { return false };
    let Some((&a, &b)) = k.first().zip(k.last()) else { return false };
    // Clamped: degree + 1 knots equal at each end, and a range to speak of.
    if !(b > a && k.iter().take(p + 1).all(|x| *x == a) && k.iter().rev().take(p + 1).all(|x| *x == b)) {
        return false;
    }
    // Control point `(i, j)` with `i` along u, as a point.
    let at =
        |i: usize, j: usize| ctrl.get(j * count[0] + i).and_then(|h| (h[3].abs() > 1e-300).then(|| Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])));
    let along_d = |x: usize, t: usize| if d == 0 { at(x, t) } else { at(t, x) };
    let (mut extent, mut gap) = (0.0f64, 0.0f64);
    for t in 0..count[1 - d] {
        let (Some(first), Some(last)) = (along_d(0, t), along_d(n - 1, t)) else { return false };
        extent = extent.max(first.len()).max(last.len());
        gap = gap.max(first.dist(last));
    }
    gap <= 1e-9 * extent.max(1.0)
}

/// A profile curve in the evaluator's form (a line extends for ever, as truck's does).
pub(crate) fn eval_curve(c: &mt::Curve) -> Option<EvalCurve> {
    let (degree, kv, ctrl): (usize, Vec<f64>, Vec<[f64; 4]>) = match c {
        mt::Curve::Line(l) => (1, vec![0.0, 0.0, 1.0, 1.0], vec![[l.0.x, l.0.y, l.0.z, 1.0], [l.1.x, l.1.y, l.1.z, 1.0]]),
        mt::Curve::BSplineCurve(b) => (b.degree(), knots(b.knot_vec())?, b.control_points().iter().map(|p| [p.x, p.y, p.z, 1.0]).collect()),
        mt::Curve::NurbsCurve(n) => (n.degree(), knots(n.knot_vec())?, n.control_points().iter().map(|h| [h.x, h.y, h.z, h.w]).collect()),
        mt::Curve::IntersectionCurve(_) => return None,
    };
    let ok = (1..=MAX_SPLINE_DEGREE).contains(&degree)
        && ctrl.len() <= MAX_EVAL_POINTS
        && kv.len() == ctrl.len() + degree + 1
        && ctrl.iter().flatten().all(|x| x.is_finite());
    ok.then_some(EvalCurve { degree, knots: kv, ctrl })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::body::Solid;

    /// Every face of `solid` evaluates as truck evaluates it, over its own parameter box.
    fn assert_matches_truck(solid: &Solid) -> [usize; 4] {
        let mut kinds = [0; 4];
        for face in solid.face_iter() {
            let surface = face.surface();
            let kind = match surface {
                mt::Surface::Plane(_) => 0,
                mt::Surface::BSplineSurface(_) => 1,
                mt::Surface::NurbsSurface(_) => 2,
                mt::Surface::RevolutedCurve(_) => 3,
            };
            let Some(eval) = eval_surface(&surface) else { panic!("not evaluable: {surface:?}") };
            // Sample the face's parameter box from its boundary vertices.
            let pts: Vec<(f64, f64)> = face.vertex_iter().filter_map(|v| surface.search_parameter(v.point(), None, 100)).collect();
            if pts.is_empty() {
                continue;
            }
            let (u0, u1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
            let (v0, v1) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
            for i in 0..=6 {
                for j in 0..=6 {
                    let (u, v) = (u0 + (u1 - u0) * i as f64 / 6.0, v0 + (v1 - v0) * j as f64 / 6.0);
                    let (p, n) = eval.eval(u, v);
                    let want = from_p3(surface.subs(u, v));
                    assert!(p.dist(want) < 1e-9 * (1.0 + want.len()), "point at ({u}, {v}): {p:?} vs {want:?}");
                    let wn = from_v3(surface.normal(u, v));
                    if wn.len() > 0.5 {
                        assert!(n.dist(wn) < 1e-6, "normal at ({u}, {v}): {n:?} vs {wn:?} ({surface:?})");
                    }
                    kinds[kind] += 1;
                }
            }
        }
        kinds
    }

    fn solid(b: crate::Body) -> Solid {
        (*b.solid).clone()
    }

    #[test]
    fn primitives_evaluate_like_truck() {
        let Ok(bx) = crate::box_solid(Vec3::new(-1.0, -2.0, 0.0), Vec3::new(3.0, 4.0, 5.0)) else { panic!("box") };
        let Ok(cyl) = crate::cylinder(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 1.0, 1.0), 2.5, 7.0) else { panic!("cylinder") };
        let Ok(sph) = crate::sphere(Vec3::new(0.0, 0.0, 1.0), 4.0) else { panic!("sphere") };
        let Ok(tor) = crate::torus(Vec3::ZERO, 6.0, 1.5) else { panic!("torus") };
        let mut kinds = [0; 4];
        for b in [bx, cyl, sph, tor] {
            for (k, n) in kinds.iter_mut().zip(assert_matches_truck(&solid(b))) {
                *k += n;
            }
        }
        // Planes, rational patches (sphere) and revolved profiles (cylinder, torus) all covered.
        assert!(kinds[0] > 0 && kinds[3] > 0 && kinds[1] + kinds[2] > 0, "{kinds:?}");
    }

    #[test]
    fn public_exact_face_and_edge_adapters_preserve_boundaries_and_parameters() {
        for body in [crate::box_solid(Vec3::ZERO, Vec3::new(4.0, 6.0, 8.0)).unwrap(), crate::cylinder(Vec3::ZERO, Vec3::Z, 3.0, 5.0).unwrap()] {
            for index in 0..body.face_count() {
                let face = crate::face_geometry(&body, index).unwrap();
                let adjacency = crate::face_boundaries(&body, index).unwrap();
                assert_eq!(adjacency.len(), face.loops.len());
                for (edges, wire) in adjacency.iter().zip(&face.loops) {
                    assert_eq!(edges.len(), wire.len());
                    for (edge, curve) in edges.iter().zip(wire) {
                        assert!(edge.neighbor.is_none_or(|neighbor| neighbor != index && neighbor < body.face_count()));
                        if body.face_count() == 6 {
                            assert!(edge.neighbor.is_some());
                        }
                        let exact = crate::edge_geometry(&body, &[edge.edge]).unwrap().remove(0);
                        let point = curve.at(0.0).unwrap().0;
                        assert!(point.dist(exact.at(0.0).unwrap().0).min(point.dist(exact.at(1.0).unwrap().0)) < 1e-8);
                    }
                }
                assert!(face.outer < face.loops.len());
                for wire in &face.loops {
                    for (edge, next) in wire.iter().zip(wire.iter().cycle().skip(1)) {
                        assert!(edge.at(1.0).unwrap().0.dist(next.at(0.0).unwrap().0) < 1e-9);
                        for fraction in [0.0, 0.21, 0.67, 1.0] {
                            let (point, tangent) = edge.at(fraction).unwrap();
                            assert!(tangent.is_finite());
                            let uv = face.parameters(point, None, 1e-7).unwrap();
                            assert!(face.surface.eval(uv[0], uv[1]).0.dist(point) < 1e-7);
                        }
                    }
                }
                assert!(face.parameters(Vec3::ZERO, None, f64::NAN).is_err());
            }
            let mesh = body.tessellate(0.01).unwrap();
            let ids: Vec<_> = (0..mesh.edges.len()).collect();
            let curves = crate::edge_geometry(&body, &ids).unwrap();
            for (curve, polyline) in curves.iter().zip(&mesh.edges) {
                assert!(curve.at(0.0).unwrap().0.dist(*polyline.first().unwrap()) < 1e-7);
                assert!(curve.at(1.0).unwrap().0.dist(*polyline.last().unwrap()) < 1e-7);
                assert!(curve.at(f64::NAN).is_none());
            }
            assert!(crate::face_geometry(&body, usize::MAX).is_err());
            assert!(crate::face_boundaries(&body, usize::MAX).is_err());
            assert!(crate::edge_geometry(&body, &[usize::MAX]).is_err());
        }
    }
    #[test]
    fn oblique_parameter_trims_stay_on_the_exact_surface_and_split_sewing_keeps_rim_history() {
        use solidcraft_geom::bspline::NurbsSurface;
        let points: Vec<_> = (0..3)
            .flat_map(|j| {
                (0..3).map(move |i| Vec3::new(i as f64 / 2.0, j as f64 / 2.0, (if i == 2 { 1.0 } else { 0.0 }) + (if j == 2 { 1.0 } else { 0.0 })))
            })
            .collect();
        let surface = NurbsSurface::new([2, 2], std::array::from_fn(|_| vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]), &points, &[1.0; 9]).unwrap();
        let diamond = vec![[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]];
        let trimmed = crate::nurbs_region_sheet(&surface, &[diamond]).unwrap();
        assert!(trimmed.is_surface());
        for edge in crate::face_geometry(&trimmed, 0).unwrap().loops.into_iter().flatten() {
            for t in [0.0, 0.13, 0.57, 0.91, 1.0] {
                let p = edge.at(t).unwrap().0;
                assert!((p.z - p.x * p.x - p.y * p.y).abs() < 1e-10);
            }
        }
        assert!(!trimmed.tessellate(0.005).unwrap().triangles.is_empty());
        let base = crate::box_solid(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)).unwrap();
        let top = (0..base.face_count()).find(|&i| base.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let open = crate::remove_faces(&base, &[top]).unwrap();
        let surface = NurbsSurface::new(
            [1, 1],
            std::array::from_fn(|_| vec![0.0, 0.0, 1.0, 1.0]),
            &[Vec3::new(0.0, 0.0, 2.0), Vec3::new(2.0, 0.0, 2.0), Vec3::new(0.0, 2.0, 2.0), Vec3::new(2.0, 2.0, 2.0)],
            &[1.0; 4],
        )
        .unwrap();
        let sheets: Vec<_> = (0..2)
            .flat_map(|j| (0..2).map(move |i| (i, j)))
            .map(|(i, j)| {
                let (u, v) = (i as f64 / 2.0, j as f64 / 2.0);
                crate::nurbs_region_sheet(&surface, &[vec![[u, v], [u + 0.5, v], [u + 0.5, v + 0.5], [u, v + 0.5]]]).unwrap()
            })
            .collect();
        let skin = crate::stitch(&sheets.iter().collect::<Vec<_>>(), 1e-6).unwrap();
        let result = crate::stitch_split(&[&open, &skin], 1e-6).unwrap();
        assert!(!result.is_surface() && result.validity().is_empty());
        assert_eq!(result.face_count(), 9);
        assert!((crate::measure(&result).unwrap().volume - 8.0).abs() < 1e-8);
        result.history().unwrap().check(&result, &[&open, &skin]).unwrap();
        assert!(crate::stitch_split(&[], 1e-6).is_err());
        assert!(crate::stitch_split(&[&skin], f64::NAN).is_err());
    }
    #[test]
    fn matched_face_replacement_qualifies_half_spaces_and_keeps_history() {
        use solidcraft_geom::bspline::{HPoint, NurbsSurface};
        let base = crate::box_solid(Vec3::new(-1.0, -1.0, -2.0), Vec3::new(1.0, 1.0, 0.0)).unwrap();
        let top = (0..base.face_count()).find(|&i| base.planar_face(i).unwrap().normal().z > 0.99).unwrap();
        for cut in [false, true] {
            let ctrl = (0..4)
                .flat_map(|j| {
                    (0..4).map(move |i| {
                        let z = if (1..3).contains(&i) && (1..3).contains(&j) { if cut { -1.0 } else { 1.0 } } else { 0.0 };
                        HPoint::weighted(Vec3::new(-1.0 + 2.0 * i as f64 / 3.0, -1.0 + 2.0 * j as f64 / 3.0, z), 1.0)
                    })
                })
                .collect();
            let surface =
                NurbsSurface::from_homogeneous([3, 3], std::array::from_fn(|_| vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]), ctrl).unwrap();
            let skin = crate::nurbs_sheet(&surface).unwrap();
            let result = crate::replace_matched_face(&base, top, &skin, cut, 1e-6).unwrap().unwrap_or_else(|| panic!("qualified cut={cut}"));
            assert!(!result.is_surface() && result.validity().is_empty());
            // Mass properties are tessellated (about 3e-4 relative), rather than
            // the exact integral of this bicubic height field.
            let volume = crate::measure(&result).unwrap().volume;
            assert!((volume - if cut { 7.0 } else { 9.0 }).abs() < 0.003, "cut={cut}, volume={volume}");
            result.history().unwrap().check(&result, &[&base, &skin]).unwrap();
            assert!(crate::replace_matched_face(&base, top, &skin, !cut, 1e-6).unwrap().is_none());
            assert!(crate::replace_matched_face(&base, top, &skin, cut, f64::NAN).unwrap().is_none());
        }
        let tool = crate::box_solid(Vec3::new(-1.0, -1.0, -3.0), Vec3::new(1.0, 1.0, 0.0)).unwrap();
        let cap = (0..tool.face_count()).find(|&i| tool.planar_face(i).unwrap().normal().z > 0.99).unwrap();
        let skin = crate::remove_faces(&tool, &[cap]).unwrap();
        assert!(crate::replace_matched_face(&base, top, &skin, true, 1e-6).unwrap().is_none());
        let tool = crate::box_solid(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 0.0)).unwrap();
        let cap = (0..tool.face_count()).find(|&i| tool.planar_face(i).unwrap().normal().z > 0.99).unwrap();
        let skin = crate::remove_faces(&tool, &[cap]).unwrap();
        assert!(crate::replace_matched_face(&base, top, &skin, true, 1e-6).unwrap().is_none(), "coincident walls require trimming");
    }
    #[test]
    fn transformed_and_mirrored_bodies_evaluate_like_truck() {
        let Ok(cyl) = crate::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 5.0) else { panic!("cylinder") };
        // Column-major: x → 3 − x, z → z + 1.
        let mirror = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [3.0, 0.0, 1.0, 1.0]];
        let Ok(m) = crate::transform_matrix(&cyl, mirror) else { panic!("transform") };
        assert!(assert_matches_truck(&solid(m)).iter().sum::<usize>() > 0);
    }
}
