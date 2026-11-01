//! Convert a `SolidCore` B-rep into the neutral [`step_import::Model`].
//!
//! This is the export counterpart of [`crate::build`]. It maps exact `SolidCore`
//! topology and analytic/B-spline geometry to the neutral model that
//! `step_export` writes, so an engine B-rep can be exported without being
//! tessellated.
//!
//! The initial bridge writes one `MANIFOLD_SOLID_BREP` per B-rep, with all live faces
//! in one closed shell. Curves and surfaces are written as their exact analytic or
//! NURBS entities. Curved faces carry pcurves; periodic seams carry both sides of
//! the seam so the writer can emit a single `SEAM_CURVE`.

use std::collections::BTreeMap;

use solidcore::curve::curve::p1::ApproximateCurveOptions;
use solidcore::curve::curve::p2::CurveKind;
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::ZONE_TOL_3D;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::topology::brep::p2::is_sheet;
use solidcore::topology::edgeuse::p1 as eu;
use solidcore::topology::{Brep, VertexId};
use step_import::model::{
    Body, BodyKind, Bound, Curve as ModelCurve, Edge, Face, Frame, Loop, Model, OrientedEdge,
    Shell, Surface as ModelSurface, Vertex,
};

/// Why a B-rep could not be mapped to the neutral model.
#[derive(Debug, Clone)]
pub struct ExportModelError {
    /// Human-readable reason.
    pub message: String,
}

impl std::fmt::Display for ExportModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExportModelError {}

fn err(message: impl Into<String>) -> ExportModelError {
    ExportModelError {
        message: message.into(),
    }
}

fn point(p: solidcore::math::Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn vec(v: solidcore::math::Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn unit(v: solidcore::math::Vec3) -> Result<[f64; 3], ExportModelError> {
    let raw = vec(v);
    let length = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2]).sqrt();
    if !(length.is_finite() && length > 0.0) {
        return Err(err("degenerate direction"));
    }
    Ok([raw[0] / length, raw[1] / length, raw[2] / length])
}

fn frame(
    position: &solidcore::geom::axis2_placement::Axis2Placement,
) -> Result<Frame, ExportModelError> {
    let z = unit(position.z_axis())?;
    let x = unit(position.x_axis())?;
    let y = [
        z[1] * x[2] - z[2] * x[1],
        z[2] * x[0] - z[0] * x[2],
        z[0] * x[1] - z[1] * x[0],
    ];
    Ok(Frame {
        origin: point(position.origin()),
        x,
        y,
        z,
    })
}

fn bspline_to_model(curve: &BSplineCurve) -> Result<ModelCurve, ExportModelError> {
    let (points, weights) = curve.control_polygon().map_err(|e| err(e.to_string()))?;
    let (knots, mults) = curve.knots(None).map_err(|e| err(e.to_string()))?;
    Ok(ModelCurve::BSpline(step_import::model::BSplineCurve {
        degree: curve.degree().map_err(|e| err(e.to_string()))? as u32,
        points: points.into_iter().map(point).collect(),
        weights: (!weights.is_empty()).then_some(weights),
        knots,
        mults: mults.into_iter().map(|m| m as u32).collect(),
    }))
}

fn approximate_crv_on_surf(
    kernel: &Kernel,
    curve: &Curve,
    interval: Extent1d,
    tolerance: f64,
) -> Result<ModelCurve, ExportModelError> {
    let breaks = [interval.min(), interval.max()];
    let tol = if tolerance > 0.0 {
        tolerance
    } else {
        ZONE_TOL_3D
    };
    let opts = ApproximateCurveOptions::default();
    let approx = curve
        .approximate_curve(kernel, &breaks, tol, &opts)
        .map_err(|e| err(format!("failed to approximate curve on surface: {e}")))?;
    match approx.curve.kind() {
        CurveKind::BSpline(curve) => bspline_to_model(curve),
        other => Err(err(format!(
            "expected B-spline approximation for curve on surface, got {other:?}"
        ))),
    }
}

fn curve(curve: &Curve) -> Result<ModelCurve, ExportModelError> {
    match curve.kind() {
        CurveKind::Line(line) => Ok(ModelCurve::Line {
            origin: point(line.line_point()),
            dir: unit(line.line_vector())?,
            magnitude: line.line_scale().abs(),
        }),
        CurveKind::Ellipse(ellipse) => {
            let frame = frame(ellipse.position())?;
            let x = ellipse.x_radius();
            let y = ellipse.y_radius();
            if (x - y).abs() <= 1.0e-12 * x.abs().max(y.abs()).max(1.0) {
                Ok(ModelCurve::Circle { frame, radius: x })
            } else {
                Ok(ModelCurve::Ellipse {
                    frame,
                    semi1: x,
                    semi2: y,
                })
            }
        }
        CurveKind::BSpline(curve) => bspline_to_model(curve),
        CurveKind::CrvOnSurf(_) => approximate_crv_on_surf(
            &Kernel::default(),
            curve,
            curve.natural_interval(),
            ZONE_TOL_3D,
        ),
        other => Err(err(format!("unsupported curve kind {other:?}"))),
    }
}

fn surface(
    surface: &solidcore::surface::surface::Surface,
) -> Result<ModelSurface, ExportModelError> {
    use solidcore::surface::surface::p2::SurfaceKind;
    match surface.kind() {
        SurfaceKind::Plane(plane) => Ok(ModelSurface::Plane {
            frame: frame(plane.position())?,
        }),
        SurfaceKind::Sphere(sphere) => Ok(ModelSurface::Sphere {
            frame: frame(sphere.base().position())?,
            radius: sphere.radius(),
        }),
        SurfaceKind::Torus(torus) => Ok(ModelSurface::Torus {
            frame: frame(torus.base().position())?,
            major: torus.major_radius(),
            minor: torus.minor_radius(),
        }),
        SurfaceKind::Cone(cone) => Ok(ModelSurface::Cone {
            frame: frame(cone.rev().position())?,
            radius: cone.base_radius(),
            semi_angle_deg: cone.semi_angle_deg(),
        }),
        SurfaceKind::SurfOfRevolution(revolution) => Ok(ModelSurface::Revolution {
            curve: Box::new(curve(
                revolution
                    .gen_curve()
                    .ok_or_else(|| err("revolution has no generator"))?,
            )?),
            axis_origin: point(revolution.position().origin()),
            axis_dir: unit(revolution.position().z_axis())?,
        }),
        SurfaceKind::SurfOfExtrusion(extrusion) => {
            let vector = extrusion.extrusion_vector();
            let magnitude =
                (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
            Ok(ModelSurface::Extrusion {
                curve: Box::new(curve(
                    extrusion
                        .gen_curve()
                        .ok_or_else(|| err("extrusion has no generator"))?,
                )?),
                dir: unit(vector)?,
                magnitude,
            })
        }
        SurfaceKind::BSpline(bspline) => {
            let net = bspline
                .control_point_net()
                .map_err(|e| err(e.to_string()))?;
            let v_count = net.v_count.max(1);
            let points: Vec<Vec<[f64; 3]>> = net
                .points
                .chunks(v_count)
                .map(|row| row.iter().map(|p| point(*p)).collect())
                .collect();
            let weights = if net.weights.is_empty() {
                None
            } else {
                Some(net.weights.chunks(v_count).map(<[f64]>::to_vec).collect())
            };
            let canonical = bspline.canonical().map_err(|e| err(e.to_string()))?;
            Ok(ModelSurface::BSpline(step_import::model::BSplineSurface {
                u_degree: canonical.u_degree as u32,
                v_degree: canonical.v_degree as u32,
                points,
                weights,
                u_knots: canonical.u_knots,
                u_mults: canonical
                    .u_multiplicities
                    .into_iter()
                    .map(|m| m as u32)
                    .collect(),
                v_knots: canonical.v_knots,
                v_mults: canonical
                    .v_multiplicities
                    .into_iter()
                    .map(|m| m as u32)
                    .collect(),
            }))
        }
        other => Err(err(format!("unsupported surface kind {other:?}"))),
    }
}

fn edge_start_is_end(brep: &Brep, edge: solidcore::topology::EdgeId) -> bool {
    use solidcore::topology::edge;
    let Ok((start, end)) = edge::start_end_vertices(brep, edge) else {
        return false;
    };
    match (start, end) {
        (Some(start), Some(end)) if start == end => true,
        (Some(start), Some(end)) => {
            let Ok(a) = brep.vertex(start) else {
                return false;
            };
            let Ok(b) = brep.vertex(end) else {
                return false;
            };
            let (a, b) = (a.point, b.point);
            (a.x - b.x).abs() < 1.0e-7 && (a.y - b.y).abs() < 1.0e-7 && (a.z - b.z).abs() < 1.0e-7
        }
        _ => false,
    }
}

/// Which of `surface`'s STEP parameters (u, v) are angles (ISO 10303-42): a cylinder's
/// or cone's u (its v is a length), a sphere's or torus's u and v, a surface of
/// revolution's u and, for a conic generator, its v (the generator's parameter), and an
/// extrusion's u for a conic generator.
fn step_angles(surface: &solidcore::surface::surface::Surface) -> (bool, bool) {
    use solidcore::surface::surface::p2::SurfaceKind;
    let conic = |curve: Option<&Curve>| {
        curve.is_some_and(|curve| matches!(curve.kind(), CurveKind::Ellipse(_)))
    };
    match surface.kind() {
        SurfaceKind::Cone(_) => (true, false),
        SurfaceKind::Sphere(_) | SurfaceKind::Torus(_) => (true, true),
        SurfaceKind::SurfOfRevolution(revolution) => (true, conic(revolution.gen_curve())),
        SurfaceKind::SurfOfExtrusion(extrusion) => (conic(extrusion.gen_curve()), false),
        _ => (false, false),
    }
}

/// Maps one B-rep to the neutral STEP model.
pub fn brep_to_model(brep: &Brep) -> Result<Model, ExportModelError> {
    let vertex_ids = brep.vertex_list().to_vec();
    let vertex_index: BTreeMap<VertexId, usize> = vertex_ids
        .iter()
        .enumerate()
        .map(|(i, v)| (*v, i))
        .collect();
    let vertices = vertex_ids
        .iter()
        .map(|id| {
            brep.vertex(*id)
                .map(|v| Vertex {
                    step_id: u64::from(id.index()) + 1,
                    point: point(v.point),
                })
                .map_err(|e| err(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let kernel = Kernel::default();
    let mut edges = Vec::new();
    let mut edge_index = BTreeMap::new();
    for id in brep.edge_list() {
        let edge = brep.edge(*id).map_err(|e| err(e.to_string()))?;
        let (start, end) = solidcore::topology::edge::start_end_vertices(brep, *id)
            .map_err(|e| err(e.to_string()))?;
        let start = start
            .and_then(|v| vertex_index.get(&v).copied())
            .ok_or_else(|| err("edge has no start vertex"))?;
        let end = end
            .and_then(|v| vertex_index.get(&v).copied())
            .ok_or_else(|| err("edge has no end vertex"))?;
        let edge_curve = edge
            .curve
            .as_ref()
            .ok_or_else(|| err("edge has no curve"))?;
        let interval = edge.interval;
        edge_index.insert(*id, edges.len());
        let curve_geom = match edge_curve.kind() {
            CurveKind::CrvOnSurf(_) => {
                approximate_crv_on_surf(&kernel, edge_curve, interval, edge.tolerance)?
            }
            _ => curve(edge_curve)?,
        };
        edges.push(Edge {
            step_id: u64::from(id.index()) + 1,
            start,
            end,
            curve: curve_geom,
            same_sense: true,
            t_start: interval.min(),
            t_end: interval.max(),
        });
    }

    let mut faces = Vec::new();
    let mut face_indices = Vec::new();
    let mut face_by_id = BTreeMap::new();
    let mut pcurves = BTreeMap::new();
    let pcurve_kernel = &kernel;
    for id in brep.face_list() {
        let face = brep.face(*id).map_err(|e| err(e.to_string()))?;
        let surf = face
            .surface
            .as_ref()
            .ok_or_else(|| err("face has no surface"))?;
        let faceuse = face.faceuse.ok_or_else(|| err("face has no faceuse"))?;
        let loops =
            eu::faceuse_loopuses(brep, faceuse, false, None).map_err(|e| err(e.to_string()))?;
        let mut bounds = Vec::new();
        for (loop_index, loopuse) in loops.iter().enumerate() {
            let edgeuses = eu::loopuse_edgeuses(brep, *loopuse).map_err(|e| err(e.to_string()))?;
            let mut oriented = Vec::new();
            let planar_face = matches!(
                face.surface
                    .as_ref()
                    .map(solidcore::surface::surface::Surface::kind),
                Some(solidcore::surface::surface::p2::SurfaceKind::Plane(_))
            );
            for (use_index, edgeuse) in edgeuses.into_iter().enumerate() {
                let edge = eu::edge(brep, edgeuse)
                    .map_err(|e| err(e.to_string()))?
                    .ok_or_else(|| err("edgeuse has no edge"))?;
                if !planar_face {
                    let Ok((Some(uv_curve), _)) = eu::uv_trim_curve(brep, edgeuse) else {
                        continue;
                    };
                    let interval = uv_curve.natural_interval();
                    let (low, high) = (interval.min(), interval.max());
                    // The UV trim curve is in the surface's NURB parameters; a STEP pcurve
                    // is in its STEP ones (ISO 10303-42), which SolidCore converts to and
                    // from (the angles not linearly). A pcurve with a sample that does not
                    // convert is left out.
                    let mut uv = Vec::with_capacity(65);
                    let mut converted = true;
                    for step in 0..=64 {
                        let t = low + (high - low) * f64::from(step) / 64.0;
                        if let Ok(p) = uv_curve.evaluate_point(pcurve_kernel, t) {
                            match surf.convert_uv_from_nurbs_to_step(
                                pcurve_kernel,
                                solidcore::geom::vector2d::Point2::new(p.x, p.y),
                            ) {
                                Ok(step_uv) => uv.push([step_uv.x, step_uv.y]),
                                Err(_) => converted = false,
                            }
                        }
                    }
                    if !converted {
                        uv.clear();
                    }
                    if uv.len() >= 2 {
                        let opposite = eu::orientation(brep, edgeuse).is_ok_and(|orientation| {
                            orientation
                                != solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same
                        });
                        if opposite {
                            uv.reverse();
                        }
                        let mut curves = vec![uv.clone()];
                        let periodic = matches!(
                            face.surface
                                .as_ref()
                                .map(solidcore::surface::surface::Surface::kind),
                            Some(
                                solidcore::surface::surface::p2::SurfaceKind::Cone(_)
                                    | solidcore::surface::surface::p2::SurfaceKind::Sphere(_)
                                    | solidcore::surface::surface::p2::SurfaceKind::Torus(_)
                                    | solidcore::surface::surface::p2::SurfaceKind::SurfOfRevolution(
                                        _
                                    )
                                    | solidcore::surface::surface::p2::SurfaceKind::BSpline(_)
                            )
                        );
                        if periodic {
                            let closed_edge = edge_start_is_end(brep, edge);
                            if closed_edge {
                                let mean_u = uv.iter().map(|p| p[0]).sum::<f64>() / uv.len() as f64;
                                let shift = if mean_u > 180.0 { -360.0 } else { 360.0 };
                                let mut other = uv.clone();
                                for point in &mut other {
                                    point[0] += shift;
                                }
                                curves.push(other);
                            }
                        }
                        // SolidCore's STEP parameters give angles in degrees; the file's
                        // plane angle unit is the radian ([`step_angles`]).
                        let (angular_u, angular_v) = step_angles(surf);
                        for curve in &mut curves {
                            for point in curve {
                                if angular_u {
                                    point[0] = point[0].to_radians();
                                }
                                if angular_v {
                                    point[1] = point[1].to_radians();
                                }
                            }
                        }
                        pcurves.insert((u64::from(id.index()) + 1, loop_index, use_index), curves);
                    }
                }
                let forward = eu::orientation(brep, edgeuse).map_err(|e| err(e.to_string()))?
                    == solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same;
                oriented.push(OrientedEdge {
                    edge: *edge_index
                        .get(&edge)
                        .ok_or_else(|| err("edgeuse edge not indexed"))?,
                    forward,
                });
            }
            let orientation = solidcore::topology::loopuse::orientation(brep, *loopuse)
                .map_err(|e| err(e.to_string()))?
                == solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same;
            bounds.push(Loop {
                outer: loop_index == 0,
                orientation,
                bound: Bound::Edges(oriented),
            });
        }
        let same_sense = brep
            .faceuse(faceuse)
            .map_err(|e| err(e.to_string()))?
            .orientation
            == solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType::Same;
        face_indices.push(faces.len());
        face_by_id.insert(*id, faces.len());
        faces.push(Face {
            step_id: u64::from(id.index()) + 1,
            surface: surface(surf)?,
            same_sense,
            loops: bounds,
        });
    }
    if faces.is_empty() {
        return Err(err("B-rep has no faces"));
    }
    let mut kernel = Kernel::default();
    let sheet = is_sheet(&mut kernel, brep).map_err(|e| err(e.to_string()))?;
    let mut shell_records = Vec::new();
    let mut seen_shell_faces: Vec<Vec<usize>> = Vec::new();
    for (shell_position, shell_id) in brep.shells().ids().enumerate() {
        let mut shell_faces = Vec::new();
        for faceuse in brep
            .shell(shell_id)
            .map_err(|e| err(e.to_string()))?
            .get_faceuses()
            .map_err(|e| err(e.to_string()))?
        {
            let face_id = brep.faceuse(faceuse).map_err(|e| err(e.to_string()))?.face;
            if let Some(face_id) = face_id {
                if let Some(index) = face_by_id.get(&face_id) {
                    shell_faces.push(*index);
                }
            }
        }
        if shell_faces.is_empty() {
            continue;
        }
        let mut canonical = shell_faces.clone();
        canonical.sort_unstable();
        if seen_shell_faces.contains(&canonical) {
            continue;
        }
        seen_shell_faces.push(canonical);
        shell_records.push(Shell {
            step_id: shell_position as u64 + 1,
            closed: !sheet,
            orientation: true,
            faces: shell_faces,
        });
    }
    if shell_records.is_empty() {
        // No region/shell: fall back to every live face in one body.
        shell_records.push(Shell {
            step_id: 1,
            closed: !sheet,
            orientation: true,
            faces: face_indices,
        });
    }
    let bodies = shell_records
        .into_iter()
        .enumerate()
        .map(|(index, shell)| Body {
            step_id: index as u64 + 1,
            name: None,
            kind: if sheet {
                BodyKind::ShellModel
            } else {
                BodyKind::ManifoldSolid
            },
            unit_mm: 1.0,
            uncertainty_mm: None,
            shells: vec![shell],
            faces: faces.clone(),
            edges: edges.clone(),
            vertices: vertices.clone(),
        })
        .collect();
    Ok(Model {
        bodies,
        pcurves,
        ..Model::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::kernel::Kernel;
    use solidcore::math::Point3;
    use solidcore::primitive::api_primitives as api;

    #[test]
    fn maps_a_box_and_a_sphere_to_the_neutral_model() {
        let mut kernel = Kernel::default();
        for brep in [
            api::create_box(&mut kernel, Point3::new(-1.0, -1.0, -1.0), 2.0, 2.0, 2.0).unwrap(),
            api::create_sphere(&mut kernel, Point3::new(0.0, 0.0, 0.0), 2.0).unwrap(),
        ] {
            let model = brep_to_model(&brep).expect("brep to model");
            assert_eq!(model.bodies.len(), 1);
            let body = &model.bodies[0];
            assert!(!body.faces.is_empty());
            assert!(!body.edges.is_empty());
            assert!(!body.vertices.is_empty());
            assert!(body.faces.iter().any(|face| matches!(
                face.surface,
                ModelSurface::Plane { .. } | ModelSurface::Sphere { .. }
            )));
        }
        let cylinder =
            api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, -2.0), 2.0, 4.0).unwrap();
        let model = brep_to_model(&cylinder).expect("cylinder to model");
        assert!(
            model.pcurves.values().any(|curves| curves.len() >= 2),
            "cylinder seam edge should carry two pcurves: {:?}",
            model.pcurves
        );
        for curves in model.pcurves.values() {
            for curve in curves {
                assert!(
                    curve
                        .iter()
                        .any(|point| point[0].abs() <= std::f64::consts::TAU + 0.5),
                    "cylinder pcurve u must be radians: {curve:?}"
                );
            }
        }
        // The cylinder's v is a length: the seam's pcurves span the 4 mm height.
        let vs = model
            .pcurves
            .values()
            .flatten()
            .flatten()
            .map(|point| point[1]);
        let (low, high) = vs.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), v| {
            (low.min(v), high.max(v))
        });
        assert!(
            (high - low - 4.0).abs() < 1e-9,
            "cylinder pcurve v must be millimetres: {low} to {high}"
        );
    }

    fn plus(p: [f64; 3], s: f64, d: [f64; 3]) -> [f64; 3] {
        [p[0] + s * d[0], p[1] + s * d[1], p[2] + s * d[2]]
    }

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// The point of a line or circle at its file parameter (a circle's in radians).
    fn iso_curve_point(curve: &ModelCurve, t: f64) -> [f64; 3] {
        match curve {
            ModelCurve::Line {
                origin,
                dir,
                magnitude,
            } => plus(*origin, t * magnitude, *dir),
            ModelCurve::Circle { frame, radius } => plus(
                plus(frame.origin, radius * t.cos(), frame.x),
                radius * t.sin(),
                frame.y,
            ),
            other => panic!("generator {other:?}"),
        }
    }

    /// The ISO 10303-42 point of an exported surface at file parameters (angles in
    /// radians).
    fn iso_point(surface: &ModelSurface, u: f64, v: f64) -> [f64; 3] {
        let (su, cu) = u.sin_cos();
        let (sv, cv) = v.sin_cos();
        let around = |frame: &Frame, r: f64, height: f64| {
            plus(
                plus(plus(frame.origin, r * cu, frame.x), r * su, frame.y),
                height,
                frame.z,
            )
        };
        match surface {
            ModelSurface::Cylinder { frame, radius } => around(frame, *radius, v),
            ModelSurface::Cone {
                frame,
                radius,
                semi_angle_deg,
            } => around(frame, radius + v * semi_angle_deg.to_radians().tan(), v),
            ModelSurface::Sphere { frame, radius } => around(frame, radius * cv, radius * sv),
            ModelSurface::Torus {
                frame,
                major,
                minor,
            } => around(frame, major + minor * cv, minor * sv),
            ModelSurface::Revolution {
                curve,
                axis_origin,
                axis_dir,
            } => {
                // The generator's point rotated by u about the axis.
                let d = plus(iso_curve_point(curve, v), -1.0, *axis_origin);
                let along = dot(d, *axis_dir);
                let radial = plus(d, -along, *axis_dir);
                let turned = plus(plus([0.0; 3], cu, radial), su, cross(*axis_dir, radial));
                plus(plus(*axis_origin, along, *axis_dir), 1.0, turned)
            }
            ModelSurface::Extrusion {
                curve,
                dir,
                magnitude,
            } => plus(iso_curve_point(curve, u), v * magnitude, *dir),
            other => panic!("surface {other:?}"),
        }
    }

    /// An open band on `surface` between two circles about the z axis of radius
    /// `radius`, at z = `bottom` and z = `top`, joined by the seam edge `seam` (its
    /// curve and parameters from bottom to top). With its seam given, the build adds
    /// none (an added seam is a curve on the surface, which the exporter does not
    /// write).
    fn band(
        surface: ModelSurface,
        radius: f64,
        bottom: f64,
        top: f64,
        seam: (ModelCurve, f64, f64),
    ) -> Body {
        let circle = |index: usize, z: f64| Edge {
            step_id: index as u64 + 1,
            start: index,
            end: index,
            curve: ModelCurve::Circle {
                frame: Frame {
                    origin: [0.0, 0.0, z],
                    x: [1.0, 0.0, 0.0],
                    y: [0.0, 1.0, 0.0],
                    z: [0.0, 0.0, 1.0],
                },
                radius,
            },
            same_sense: true,
            t_start: 0.0,
            t_end: 360.0,
        };
        let (seam_curve, seam_start, seam_end) = seam;
        let seam = Edge {
            step_id: 3,
            start: 0,
            end: 1,
            curve: seam_curve,
            same_sense: true,
            t_start: seam_start,
            t_end: seam_end,
        };
        let uses = [(0, true), (2, true), (1, false), (2, false)]
            .into_iter()
            .map(|(edge, forward)| OrientedEdge { edge, forward })
            .collect();
        Body {
            step_id: 1,
            name: None,
            kind: BodyKind::ShellModel,
            unit_mm: 1.0,
            uncertainty_mm: None,
            shells: vec![Shell {
                step_id: 1,
                closed: false,
                orientation: true,
                faces: vec![0],
            }],
            faces: vec![Face {
                step_id: 1,
                surface,
                same_sense: true,
                loops: vec![Loop {
                    outer: true,
                    orientation: true,
                    bound: Bound::Edges(uses),
                }],
            }],
            edges: vec![circle(0, bottom), circle(1, top), seam],
            vertices: vec![
                Vertex {
                    step_id: 1,
                    point: [radius, 0.0, bottom],
                },
                Vertex {
                    step_id: 2,
                    point: [radius, 0.0, top],
                },
            ],
        }
    }

    /// Every exported pcurve point, lifted by the ISO 10303-42 formula of the exported
    /// surface at the file's parameters, lies on an edge of the B-rep: the pcurves are
    /// in STEP parameters (converted from the kernel's NURB ones), angles in radians.
    #[test]
    fn exported_pcurves_lie_on_their_edges() {
        use solidcore::geom::global_solver::SolverOperation;
        let mut kernel = Kernel::default();
        let origin = Point3::new(0.0, 0.0, 0.0);
        let mut breps = vec![
            (
                "cylinder",
                api::create_cylinder(&mut kernel, Point3::new(0.0, 0.0, -2.0), 2.0, 4.0).unwrap(),
            ),
            (
                "cone",
                api::create_cone(&mut kernel, origin, 3.0, 1.0, 4.0).unwrap(),
            ),
            (
                "sphere",
                api::create_sphere(&mut kernel, origin, 2.0).unwrap(),
            ),
            (
                "torus",
                api::create_torus(&mut kernel, origin, 5.0, 1.5).unwrap(),
            ),
        ];
        // A surface of revolution of a line and an extrusion of a circle (its u an
        // angle). (A revolved circular arc band gets a seam inserted as a curve on the
        // surface even with its own seam edge, which this exporter does not write yet.)
        let z_frame = |origin: [f64; 3], y: [f64; 3], z: [f64; 3]| Frame {
            origin,
            x: [1.0, 0.0, 0.0],
            y,
            z,
        };
        let line = |x: f64| ModelCurve::Line {
            origin: [x, 0.0, 0.0],
            dir: [0.0, 0.0, 1.0],
            magnitude: 1.0,
        };
        let revolved_line = ModelSurface::Revolution {
            curve: Box::new(line(10.0)),
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
        };
        let extruded_circle = ModelSurface::Extrusion {
            curve: Box::new(ModelCurve::Circle {
                frame: z_frame([0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
                radius: 10.0,
            }),
            dir: [0.0, 0.0, 1.0],
            magnitude: 20.0,
        };
        for (name, body) in [
            (
                "revolved line",
                band(revolved_line, 10.0, 0.0, 20.0, (line(10.0), 0.0, 20.0)),
            ),
            (
                "extruded circle",
                band(extruded_circle, 10.0, 0.0, 20.0, (line(10.0), 0.0, 20.0)),
            ),
        ] {
            let brep = crate::build(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
            breps.push((name, brep));
        }
        for (name, brep) in &breps {
            let model = brep_to_model(brep).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!model.pcurves.is_empty(), "{name}: no pcurves");
            // (a degenerate edge, e.g. at a pole, may have no curve)
            let edges: Vec<_> = brep
                .edge_list()
                .iter()
                .filter_map(|&e| {
                    let edge = brep.edge(e).unwrap();
                    Some((edge.curve.as_ref()?, edge.interval))
                })
                .collect();
            for ((face_id, _, _), curves) in &model.pcurves {
                let face = model.bodies[0]
                    .faces
                    .iter()
                    .find(|face| face.step_id == *face_id)
                    .unwrap();
                for &[u, v] in curves.iter().flatten() {
                    let [x, y, z] = iso_point(&face.surface, u, v);
                    let p = Point3::new(x, y, z);
                    let gap = edges
                        .iter()
                        .filter_map(|(curve, interval)| {
                            curve
                                .drop_point(
                                    &kernel,
                                    interval,
                                    p,
                                    None,
                                    1e-9,
                                    None,
                                    SolverOperation::Minimize,
                                )
                                .ok()
                                .filter(|dropped| dropped.success)
                                .map(|dropped| dropped.distance)
                        })
                        .fold(f64::INFINITY, f64::min);
                    assert!(gap < 1e-4, "{name}: ({u}, {v}) lifts {gap} off every edge");
                }
            }
        }
    }
}
