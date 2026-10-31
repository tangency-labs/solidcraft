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

use solidcore::curve::curve::p2::CurveKind;
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

fn curve(curve: &solidcore::curve::curve::Curve) -> Result<ModelCurve, ExportModelError> {
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
        CurveKind::BSpline(curve) => {
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
        edges.push(Edge {
            step_id: u64::from(id.index()) + 1,
            start,
            end,
            curve: curve(edge_curve)?,
            same_sense: true,
            t_start: interval.min(),
            t_end: interval.max(),
        });
    }

    let mut faces = Vec::new();
    let mut face_indices = Vec::new();
    let mut face_by_id = BTreeMap::new();
    let mut pcurves = BTreeMap::new();
    let pcurve_kernel = solidcore::kernel::Kernel::default();
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
                    let mut uv = Vec::with_capacity(65);
                    for step in 0..=64 {
                        let t = low + (high - low) * f64::from(step) / 64.0;
                        if let Ok(p) = uv_curve.evaluate_point(&pcurve_kernel, t) {
                            uv.push([p.x, p.y]);
                        }
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
                        // SolidCore stores analytic surface UV in degrees; STEP stores
                        // angular surface parameters in radians.
                        let angular = matches!(
                            face.surface
                                .as_ref()
                                .map(solidcore::surface::surface::Surface::kind),
                            Some(
                                solidcore::surface::surface::p2::SurfaceKind::Cone(_)
                                    | solidcore::surface::surface::p2::SurfaceKind::Sphere(_)
                                    | solidcore::surface::surface::p2::SurfaceKind::Torus(_)
                            )
                        );
                        if angular {
                            for curve in &mut curves {
                                for point in curve {
                                    point[0] = point[0].to_radians();
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
    let mut kernel = solidcore::kernel::Kernel::default();
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
    }
}
