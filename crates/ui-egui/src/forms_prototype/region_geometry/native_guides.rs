//! Exact world Bezier restrictions of retained native logical guide routes.
//! Native edge intervals are intrinsic; analytic conic angles are mapped through
//! the public exact form adapter, never used as rational spline parameters.
use super::curve_pieces;
use solidcore_record::{
    Vec3,
    brep::{Curve, Curve2, Loop, Surface},
};
use solidcraft_engine::{
    doc::{
        forms::{NativeGuide, NativeGuides},
        kernel::{Body, DisplayGuidePoint},
    },
    geom::{
        brep::{eval::SurfaceEval, pcurve},
        bspline::{CurveN, HPoint, NurbsSurface},
        tess::{EvalSurface, from_kernel},
    },
};
use std::collections::{BTreeMap, btree_map::Entry};

type Result<T> = std::result::Result<T, String>;
const MAX_CONTROLS: usize = 16_384;
const MAX_DEGREE: usize = 32;
const MAX_ROUTES: usize = 65_536;

struct EdgePieces {
    edge: [usize; 2],
    /// World homogeneous controls in the logical edge's direction. The local
    /// parameter of a conic piece is rational, not its intrinsic native angle.
    pieces: Vec<Vec<HPoint>>,
}
enum GuideSurface {
    Plane { origin: Vec3, du: Vec3, dv: Vec3 },
    Nurbs(NurbsSurface),
}
fn spend(remaining: &mut usize, work: usize) -> Result<()> {
    *remaining = remaining.checked_sub(work).ok_or("exact native Form guide extraction exceeded its work budget")?;
    Ok(())
}
/// Preparation runs on the existing CAD worker. No display mesh or sampled
/// guide positions can supply an omitted native route.
pub(super) fn edge_pieces(native: &NativeGuides, guide: &NativeGuide, remaining: &mut usize) -> Result<Vec<Vec<HPoint>>> {
    let mut result = world_beziers(native, std::slice::from_ref(guide), remaining)?;
    let extracted = result.pop().ok_or("native logical Form edge has no extracted source")?;
    if extracted.edge != guide.edge || !result.is_empty() {
        return Err("native logical Form edge lost its source identity".into());
    }
    Ok(extracted.pieces)
}
fn world_beziers(native: &NativeGuides, guides: &[NativeGuide], remaining: &mut usize) -> Result<Vec<EdgePieces>> {
    if guides.is_empty() || guides.len() > MAX_ROUTES {
        return Err("native Form guide route count exceeds its budget".into());
    }
    spend(remaining, guides.len())?;
    let mut surfaces = BTreeMap::new();
    let mut used = BTreeMap::new();
    let mut result = Vec::with_capacity(guides.len());
    let mut output_controls = 0_usize;
    for edge in guides {
        if edge.edge[0] >= edge.edge[1] || used.insert(edge.edge, ()).is_some() || edge.pieces.is_empty() || edge.pieces.len() > MAX_ROUTES {
            return Err("native Form guides require distinct directed logical edges and complete pieces".into());
        }
        spend(remaining, edge.pieces.len())?;
        let mut pieces = Vec::new();
        for route in &edge.pieces {
            if route.points.len() < 2 || route.points.len() > MAX_CONTROLS {
                return Err("native Form guide station count exceeds its budget".into());
            }
            spend(remaining, route.points.len())?;
            let face = native.body.record().faces.get(route.face as usize).ok_or("native Form guide lost its exact face")?;
            if face.loops.len() > 4096 {
                return Err("native Form guide face exceeds its loop budget".into());
            }
            let face_uses = face
                .loops
                .iter()
                .try_fold(0_usize, |count, wire| {
                    count.checked_add(match wire {
                        Loop::Edges { edges } => edges.len(),
                        _ => 0,
                    })
                })
                .ok_or("native Form guide face-use size overflowed")?;
            if face_uses > 4096 {
                return Err("native Form guide face exceeds its edge-use budget".into());
            }
            let scan_work = route
                .points
                .len()
                .checked_mul(face_uses + face.loops.len())
                .and_then(|work| work.checked_mul(4))
                .ok_or("native Form guide scan size overflowed")?;
            spend(remaining, scan_work)?;
            for pair in route.points.windows(2) {
                let (a, b) = (&pair[0], &pair[1]);
                let extracted = match (a, b) {
                    (DisplayGuidePoint::Edge { edge: a_edge, parameter: a }, DisplayGuidePoint::Edge { edge: b_edge, parameter: b })
                        if a_edge == b_edge =>
                    {
                        if !face.loops.iter().any(|wire| matches!(wire, Loop::Edges { edges } if edges.iter().any(|use_| use_.edge == *a_edge))) {
                            return Err("native Form rim station does not belong to its routed face".into());
                        }
                        intrinsic_edge_pieces(&native.body, *a_edge as usize, [*a, *b], remaining)?
                    }
                    _ => {
                        let surface = match surfaces.entry(route.face) {
                            Entry::Occupied(entry) => entry.into_mut(),
                            Entry::Vacant(entry) => entry.insert(guide_surface(face.parameters(), remaining)?),
                        };
                        let a = station_uv(&native.body, route.face as usize, a, remaining)?;
                        let b = station_uv(&native.body, route.face as usize, b, remaining)?;
                        surface_line(surface, a, b, remaining)?
                    }
                };
                if pieces.len().checked_add(extracted.len()).is_none_or(|count| count > MAX_ROUTES) {
                    return Err("native Form guide Bezier count exceeds its budget".into());
                }
                output_controls =
                    output_controls.checked_add(extracted.iter().map(Vec::len).sum::<usize>()).ok_or("native Form guide output size overflowed")?;
                if output_controls > 262_144 {
                    return Err("native Form guide output exceeds its coefficient budget".into());
                }
                pieces.extend(extracted);
            }
        }
        if pieces.is_empty() {
            return Err("native logical Form edge has no exact curve restrictions".into());
        }
        result.push(EdgePieces { edge: edge.edge, pieces });
    }
    Ok(result)
}
fn preflight_tensor<T>(u: &[f64], v: &[f64], rows: &[Vec<T>], remaining: &mut usize) -> Result<()> {
    if rows.is_empty() || rows.len() > MAX_CONTROLS || u.len() > MAX_CONTROLS || v.len() > MAX_CONTROLS {
        return Err("native guide surface exceeds its borrowed-size budget".into());
    }
    let columns = rows.first().map_or(0, Vec::len);
    let count = rows.len().checked_mul(columns).ok_or("native guide tensor size overflowed")?;
    if columns == 0 || count > MAX_CONTROLS || rows.iter().any(|row| row.len() != columns) {
        return Err("native guide surface has an invalid coefficient shape".into());
    }
    let work = count
        .checked_mul(8)
        .and_then(|work| work.checked_add(u.len()))
        .and_then(|work| work.checked_add(v.len()))
        .ok_or("native guide tensor work size overflowed")?;
    spend(remaining, work)
}
fn guide_surface(surface: &Surface, remaining: &mut usize) -> Result<GuideSurface> {
    // tess::surface::Params::Same is used for precisely these raw records.
    // Analytic labels with a saved NURBS form reach this function through that
    // form (Face::parameters); raw analytic angles must not be mistaken for UV.
    match surface {
        Surface::Plane { .. } | Surface::PlaneThrough { .. } => spend(remaining, 12)?,
        Surface::Spline { u_knots, v_knots, points } => preflight_tensor(u_knots, v_knots, points, remaining)?,
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => preflight_tensor(u_knots, v_knots, homogeneous, remaining)?,
        _ => {
            return Err(
                "native Form UV guide extraction requires a saved raw planar or NURBS form; analytic display UV mapping is not implemented".into()
            );
        }
    }
    match SurfaceEval::new(surface).map_err(|error| error.to_string())? {
        SurfaceEval::Eval(EvalSurface::Plane { origin, du, dv }, _) => {
            Ok(GuideSurface::Plane { origin: from_kernel(origin), du: from_kernel(du), dv: from_kernel(dv) })
        }
        SurfaceEval::Eval(EvalSurface::Nurbs { degree, knots, ctrl, .. }, _) => {
            let surface = NurbsSurface::from_homogeneous(
                degree,
                knots,
                ctrl.into_iter().map(|point| HPoint { x: point[0], y: point[1], z: point[2], w: point[3] }).collect(),
            )
            .map_err(|error| error.to_string())?;
            Ok(GuideSurface::Nurbs(surface))
        }
        _ => Err("native Form guide does not have the display evaluator's same raw parameters".into()),
    }
}
fn station_uv(body: &Body, face: usize, station: &DisplayGuidePoint, remaining: &mut usize) -> Result<[f64; 2]> {
    match *station {
        DisplayGuidePoint::Surface(point) => {
            if point.iter().any(|value| !value.is_finite()) {
                return Err("nonfinite native Form surface station".into());
            }
            Ok(point)
        }
        DisplayGuidePoint::Edge { edge, parameter } => {
            let source = body.record().faces.get(face).ok_or("missing native Form guide face")?;
            let native_edge = body.record().edges.get(edge as usize).ok_or("missing intrinsic Form guide edge")?;
            if !parameter.is_finite() || parameter < native_edge.interval[0] || parameter > native_edge.interval[1] {
                return Err("native Form guide station escaped its edge interval".into());
            }
            let mut uses = source
                .loops
                .iter()
                .filter_map(|wire| match wire {
                    Loop::Edges { edges } => Some(edges.as_slice()),
                    _ => None,
                })
                .flatten()
                .filter(|use_| use_.edge == edge);
            let use_ = uses.next().ok_or("native Form edge station does not belong to its face")?;
            if uses.next().is_some() {
                return Err("native Form edge station has ambiguous seam-side UV provenance".into());
            }
            let (knots, count) = match &use_.pcurve {
                Curve2::Spline { knots, points } => (knots.as_slice(), points.len()),
                Curve2::RationalSpline { knots, homogeneous } => (knots.as_slice(), homogeneous.len()),
            };
            if count > MAX_CONTROLS || knots.len() > MAX_CONTROLS {
                return Err("native Form station pcurve exceeds its borrowed-size budget".into());
            }
            let degree = count.checked_add(1).and_then(|count| knots.len().checked_sub(count)).ok_or("invalid native Form station pcurve degree")?;
            if degree == 0 || degree > MAX_DEGREE || count <= degree {
                return Err("invalid native Form station pcurve shape".into());
            }
            let work = count
                .checked_mul(3)
                .and_then(|work| work.checked_add(knots.len()))
                .and_then(|work| work.checked_add(3 * (degree + 1) * (degree + 1)))
                .ok_or("native station work overflowed")?;
            spend(remaining, work)?;
            let [a, b] = use_.pcurve.interval().ok_or("native Form station pcurve has no interval")?;
            if !a.is_finite() || !b.is_finite() || a >= b {
                return Err("native Form station pcurve has an invalid interval".into());
            }
            let uv = pcurve::eval(&use_.pcurve, parameter.clamp(a, b)).ok_or("native Form station pcurve does not evaluate")?;
            if uv.iter().any(|value| !value.is_finite()) {
                return Err("native Form station parameters are nonfinite".into());
            }
            Ok(uv)
        }
    }
}
fn surface_line(surface: &GuideSurface, a: [f64; 2], b: [f64; 2], remaining: &mut usize) -> Result<Vec<Vec<HPoint>>> {
    match surface {
        GuideSurface::Plane { origin, du, dv } => {
            spend(remaining, 8)?;
            let points = [a, b].map(|uv| HPoint::weighted(*origin + *du * uv[0] + *dv * uv[1], 1.0));
            if points.iter().any(|point| !point.is_finite()) {
                return Err("native planar Form guide overflowed".into());
            }
            Ok(vec![points.to_vec()])
        }
        GuideSurface::Nurbs(surface) => {
            let degree = surface.degree();
            let moving = [usize::from(a[0] != b[0]) * degree[0], usize::from(a[1] != b[1]) * degree[1]];
            let work = (surface.knots()[0].len() + surface.knots()[1].len() + 2)
                .checked_mul(degree[0] + 1)
                .and_then(|work| work.checked_mul(degree[1] + 1))
                .and_then(|work| work.checked_mul(moving[0] + 1))
                .and_then(|work| work.checked_mul(moving[1] + 1))
                .ok_or("native Form parameter-line work overflowed")?;
            spend(remaining, work)?;
            if a == b {
                let point = HPoint::weighted(surface.eval(a[0], a[1]).map_err(|error| error.to_string())?, 1.0);
                return Ok(vec![vec![point, point]]);
            }
            let (knots, controls) = surface.parameter_line(a, b).map_err(|error| error.to_string())?;
            let degree = knots.len().checked_sub(controls.len() + 1).ok_or("invalid native Form parameter-line degree")?;
            let curve = CurveN { degree, knots, ctrl: controls.into_iter().map(|point| [point.x, point.y, point.z, point.w]).collect() };
            Ok(curve_pieces::bezier_pieces(&curve, remaining).map_err(str::to_owned)?.into_iter().map(|piece| piece.controls).collect())
        }
    }
}
fn intrinsic_edge_pieces(body: &Body, edge: usize, range: [f64; 2], remaining: &mut usize) -> Result<Vec<Vec<HPoint>>> {
    let edge = body.record().edges.get(edge).ok_or("missing native Form intrinsic curve")?;
    if range.iter().any(|value| !value.is_finite() || *value < edge.interval[0] || *value > edge.interval[1]) || range[0] == range[1] {
        return Err("native Form curve restriction has no valid intrinsic interval".into());
    }
    let reversed = range[1] < range[0];
    let range = [range[0].min(range[1]), range[0].max(range[1])];
    let mut pieces = source_curve(edge.parameters(), range, remaining)?;
    if reversed {
        pieces.reverse();
        for piece in &mut pieces {
            piece.reverse();
        }
    }
    Ok(pieces)
}
fn source_curve(source: &Curve, range: [f64; 2], remaining: &mut usize) -> Result<Vec<Vec<HPoint>>> {
    solidcore_ops::form_guide_curves::source_curve(source, range, remaining)
        .map(|pieces| pieces.into_iter().map(|piece| piece.into_iter().map(|[x, y, z, w]| HPoint { x, y, z, w }).collect()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::brep::Frame;
    use solidcraft_engine::geom::Vec3Geom as _;
    use solidcraft_engine::{
        doc::{
            forms::NativeGuide,
            kernel::{self, DisplayGuide},
        },
        geom::brep::eval::CurveEval,
    };

    fn frame() -> Frame {
        Frame { origin: [3.0, -2.0, 1.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }
    }
    fn ends(pieces: &[Vec<HPoint>]) -> [Vec3; 2] {
        [pieces.first().unwrap().first().unwrap().project().unwrap(), pieces.last().unwrap().last().unwrap().project().unwrap()]
    }
    fn at(controls: &[HPoint], t: f64) -> Vec3 {
        let mut row = controls.to_vec();
        while row.len() > 1 {
            row = row.windows(2).map(|pair| pair[0] * (1.0 - t) + pair[1] * t).collect();
        }
        row[0].project().unwrap()
    }
    #[test]
    fn analytic_conic_restrictions_use_the_form_parameter_map_and_cross_the_quarter_seam() {
        for source in [Curve::Circle { frame: frame(), radius: 4.0 }, Curve::Ellipse { frame: frame(), radii: [4.0, 2.0] }] {
            for range in [[0.17, 1.2], [5.8, 6.5], [-0.3, 0.7]] {
                let pieces = source_curve(&source, range, &mut 1_000_000).unwrap();
                let expected = CurveEval::new(&source).unwrap();
                let [a, b] = ends(&pieces);
                assert!(a.dist(expected.point(range[0]).unwrap()) < 1e-12);
                assert!(b.dist(expected.point(range[1]).unwrap()) < 1e-12);
                assert!(pieces.iter().flatten().all(|point| point.is_finite() && point.w > 0.0));
                for piece in pieces {
                    let point = at(&piece, 0.37) - Vec3::new(3.0, -2.0, 1.0);
                    let ry = if matches!(source, Curve::Circle { .. }) { 4.0 } else { 2.0 };
                    assert!((point.x * point.x / 16.0 + point.y * point.y / (ry * ry) - 1.0).abs() < 1e-12);
                }
            }
        }
    }
    #[test]
    fn native_rim_routes_preserve_intrinsic_basis_logical_identity_and_reverse_direction() {
        let body = kernel::cylinder(Vec3::ZERO, Vec3::Z, 4.0, 5.0).unwrap();
        let (id, edge) = body.record().edges.iter().enumerate().find(|(_, edge)| matches!(edge.curve, Curve::Circle { .. })).unwrap();
        let [a, b] = edge.interval;
        let range = [a * 0.83 + b * 0.17, a * 0.23 + b * 0.77];
        let expected = CurveEval::new(edge.parameters()).unwrap();
        let face = edge.uses.first().unwrap().face;
        for reversed in [false, true] {
            let range = if reversed { [range[1], range[0]] } else { range };
            let source = NativeGuides {
                body: body.clone(),
                edges: vec![NativeGuide {
                    edge: [7, 19],
                    pieces: vec![DisplayGuide {
                        face,
                        points: range.map(|parameter| DisplayGuidePoint::Edge { edge: id as u32, parameter }).to_vec(),
                    }],
                }],
            };
            let output = world_beziers(&source, &source.edges, &mut 1_000_000).unwrap();
            assert_eq!(output.len(), 1);
            assert_eq!(output[0].edge, [7, 19]);
            let [a, b] = ends(&output[0].pieces);
            assert!(a.dist(expected.point(range[0]).unwrap()) < 1e-10);
            assert!(b.dist(expected.point(range[1]).unwrap()) < 1e-10);
        }
    }
    #[test]
    fn native_uv_and_edge_station_routes_use_one_raw_nurbs_chart_exactly() {
        let surface = NurbsSurface::from_homogeneous(
            [1, 1],
            [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
            vec![
                HPoint::weighted(Vec3::new(0.0, 0.0, 0.0), 1.0),
                HPoint::weighted(Vec3::new(10.0, 0.0, 0.0), 1.0),
                HPoint::weighted(Vec3::new(0.0, 8.0, 0.0), 1.0),
                HPoint::weighted(Vec3::new(10.0, 8.0, 3.0), 1.0),
            ],
        )
        .unwrap();
        let body = kernel::nurbs_sheet(&surface).unwrap();
        let face = body.record().faces.first().unwrap();
        let Loop::Edges { edges } = face.loops.first().unwrap() else { panic!("native sheet has a boundary") };
        let use_ = &edges[0];
        let native = &body.record().edges[use_.edge as usize];
        let parameter = native.interval[0] * 0.65 + native.interval[1] * 0.35;
        let uv = pcurve::eval(&use_.pcurve, parameter).unwrap();
        let edge_id = use_.edge;
        let target = [0.7, 0.6];
        let source = NativeGuides {
            body,
            edges: vec![NativeGuide {
                edge: [2, 8],
                pieces: vec![DisplayGuide {
                    face: 0,
                    points: vec![DisplayGuidePoint::Edge { edge: edge_id, parameter }, DisplayGuidePoint::Surface(target)],
                }],
            }],
        };
        let output = world_beziers(&source, &source.edges, &mut 1_000_000).unwrap();
        let [a, b] = ends(&output[0].pieces);
        assert!(a.dist(surface.eval(uv[0], uv[1]).unwrap()) < 1e-12);
        assert!(b.dist(surface.eval(target[0], target[1]).unwrap()) < 1e-12);
        assert_eq!(output[0].pieces.len(), 1);
        assert!(at(&output[0].pieces[0], 0.5).dist(surface.eval((uv[0] + target[0]) * 0.5, (uv[1] + target[1]) * 0.5).unwrap()) < 1e-12);
    }
    #[test]
    fn exact_native_extraction_refuses_unmapped_analytic_uv_procedural_and_hostile_sources() {
        let raw = Surface::Cylinder { frame: frame(), radius: 2.0 };
        assert!(guide_surface(&raw, &mut 1_000_000).is_err());
        let curve = Curve::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[0.0; 3], [1.0; 3]] };
        assert!(source_curve(&curve, [0.0, 1.0], &mut 0).is_err());
        let oversized = Curve::Spline { knots: vec![0.0; MAX_CONTROLS + 1], points: Vec::new() };
        assert!(source_curve(&oversized, [0.0, 1.0], &mut 0).is_err());
        let plane = Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] };
        let intersection =
            Curve::SurfaceIntersection { surfaces: [Box::new(plane.clone()), Box::new(plane)], leader: Box::new(curve), tolerance: 0.01 };
        assert!(source_curve(&intersection, [0.0, 1.0], &mut 1_000_000).is_err());
    }
}
