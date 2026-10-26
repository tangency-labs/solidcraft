//! Build solidcore B-rep bodies from the neutral [`step_import::Model`].
//!
//! This is the production counterpart of the difftest `build_rust` recipe: each STEP
//! body's faces, surfaces, curves and loops are turned into a `Brep`, then stitched
//! into a solid. Unsupported geometry returns an [`BuildError`]; the caller decides
//! whether to reject the whole file or skip a body.
//!
//! [`build`] leaves the healer off: it is fast and sufficient for simple planar bodies.
//! [`build_healed`] runs `SmApiHealBrep`, creating the UV trims and seam topology that
//! curved display tessellation needs. Callers that can retry should use
//! [`build_with_options`] and heal only when the fast build cannot tessellate.

use std::fmt;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::heal::heal_data::p1::api_heal_brep;
use solidcore::heal::stitch::p1::api_stitch_into_solid;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3;
use solidcore::topology::brep::p8::{create_faces_with_curves, FaceBuildEnv};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::trimming_tools;
use solidcore::topology::{Brep, FaceId};
use solidcore::{EntityRef, SolidError};
use step_import::model::{self as sm, Body, Bound};

/// Why a body could not be built.
#[derive(Debug, Clone)]
pub struct BuildError {
    /// Human-readable reason.
    pub message: String,
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BuildError {}

impl From<SolidError> for BuildError {
    fn from(error: SolidError) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

fn v3(point: sm::P3) -> Vec3 {
    Vec3::new(point[0], point[1], point[2])
}

fn placement(frame: &sm::Frame) -> Axis2Placement {
    Axis2Placement::new(v3(frame.origin), v3(frame.x), v3(frame.y))
}

fn usizes(values: &[u32]) -> Vec<usize> {
    values.iter().map(|&value| value as usize).collect()
}

fn make_curve(
    kernel: &Kernel,
    curve: &sm::Curve,
    bounds: Option<(f64, f64)>,
) -> Result<Curve, SolidError> {
    match curve {
        sm::Curve::Line {
            origin,
            dir,
            magnitude,
        } => Ok(match bounds {
            Some((low, high)) => Curve::from(Line::bounded(
                v3(*origin),
                v3(*dir),
                &Extent1d::from_min_max(low, high),
                &BoundedLineOptions {
                    scale: *magnitude,
                    dim: 3,
                    ..BoundedLineOptions::default()
                },
            )),
            None => Curve::from(Line::create_canonical(v3(*origin), v3(*dir))),
        }),
        sm::Curve::Circle { frame, radius } => {
            let interval = bounds.map(|(low, high)| Extent1d::from_min_max(low, high));
            Ok(Curve::from(Ellipse::create_canonical_circle(
                kernel,
                &placement(frame),
                *radius,
                interval.as_ref(),
                None,
            )?))
        }
        sm::Curve::Ellipse {
            frame,
            semi1,
            semi2,
        } => {
            let interval = bounds.map(|(low, high)| Extent1d::from_min_max(low, high));
            Ok(Curve::from(Ellipse::create_canonical(
                kernel,
                &placement(frame),
                *semi1,
                *semi2,
                interval.as_ref(),
                None,
            )?))
        }
        sm::Curve::BSpline(spline) => {
            let (first, last) = (
                spline.knots.first().copied().unwrap_or(0.0),
                spline.knots.last().copied().unwrap_or(0.0),
            );
            let trim = bounds
                .filter(|&(low, high)| !(low == first && high == last))
                .map(|(low, high)| Extent1d::from_min_max(low, high));
            let points: Vec<Point3> = spline.points.iter().map(|point| v3(*point)).collect();
            let mults = usizes(&spline.mults);
            Ok(Curve::from(BSplineCurve::create_canonical(
                &CanonicalData {
                    dim: 3,
                    degree: spline.degree as usize,
                    control_points: &points,
                    form: BSplineCurveForm::Unspecified,
                    knot_multiplicities: &mults,
                    knots: &spline.knots,
                    knot_type: KnotType::Unspecified,
                    weights: spline.weights.as_deref(),
                    trim,
                },
            )?))
        }
    }
}

fn make_surface(kernel: &Kernel, surface: &sm::Surface) -> Result<Surface, SolidError> {
    Ok(match surface {
        sm::Surface::Plane { frame } => Surface::from(Plane::create_canonical(&placement(frame))?),
        sm::Surface::Cylinder { frame, radius } => Surface::from(Cone::create_canonical_cylinder(
            kernel,
            &placement(frame),
            *radius,
        )?),
        sm::Surface::Cone {
            frame,
            radius,
            semi_angle_deg,
        } => Surface::from(Cone::create_canonical_with_semi_angle(
            kernel,
            &placement(frame),
            *radius,
            *semi_angle_deg,
            None,
            false,
            false,
        )?),
        sm::Surface::Sphere { frame, radius } => Surface::from(Sphere::create_canonical(
            kernel,
            &placement(frame),
            *radius,
        )?),
        sm::Surface::Torus {
            frame,
            major,
            minor,
        } => Surface::from(Torus::create_canonical(
            kernel,
            &placement(frame),
            *major,
            *minor,
            TorusOptions::default(),
            None,
        )?),
        sm::Surface::Revolution {
            curve,
            axis_origin,
            axis_dir,
        } => {
            let swept = make_curve(kernel, curve, None)?;
            Surface::from(SurfOfRevolution::create_canonical(
                kernel,
                swept,
                v3(*axis_origin),
                v3(*axis_dir),
                false,
            )?)
        }
        sm::Surface::Extrusion {
            curve,
            dir,
            magnitude,
        } => {
            let swept = make_curve(kernel, curve, None)?;
            let m = *magnitude;
            Surface::from(SurfOfExtrusion::create_canonical(
                kernel,
                swept,
                Vec3::new(dir[0] * m, dir[1] * m, dir[2] * m),
            )?)
        }
        sm::Surface::BSpline(spline) => {
            let points: Vec<Point3> = spline
                .points
                .iter()
                .flatten()
                .map(|point| v3(*point))
                .collect();
            let weights: Option<Vec<f64>> = spline
                .weights
                .as_ref()
                .map(|weights| weights.iter().flatten().copied().collect());
            let u_mults = usizes(&spline.u_mults);
            let v_mults = usizes(&spline.v_mults);
            Surface::from(BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: spline.u_degree as usize,
                v_degree: spline.v_degree as usize,
                control_points: &points,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &u_mults,
                v_multiplicities: &v_mults,
                u_knots: &spline.u_knots,
                v_knots: &spline.v_knots,
                knot_type: KnotType::Unspecified,
                weights: weights.as_deref(),
                uv_domain: None,
            })?)
        }
    })
}

struct FaceInput {
    loops: Vec<usize>,
    curves: Vec<Option<Curve>>,
    orients: Vec<OrientType>,
}

fn face_input(kernel: &Kernel, body: &Body, face: &sm::Face) -> Result<FaceInput, BuildError> {
    let mut out = FaceInput {
        loops: Vec::new(),
        curves: Vec::new(),
        orients: Vec::new(),
    };
    for bound in &face.loops {
        let Bound::Edges(edges) = &bound.bound else {
            continue;
        };
        let oriented: Vec<(usize, bool)> = if bound.orientation {
            edges.iter().map(|edge| (edge.edge, edge.forward)).collect()
        } else {
            edges
                .iter()
                .rev()
                .map(|edge| (edge.edge, !edge.forward))
                .collect()
        };
        out.loops.push(oriented.len());
        for (index, forward) in oriented {
            let edge = body.edges.get(index).ok_or_else(|| BuildError {
                message: format!("body #{} references missing edge {index}", body.step_id),
            })?;
            let (low, high) = if edge.same_sense {
                (edge.t_start, edge.t_end)
            } else {
                (edge.t_end, edge.t_start)
            };
            out.curves
                .push(Some(make_curve(kernel, &edge.curve, Some((low, high)))?));
            out.orients.push(if forward == edge.same_sense {
                OrientType::Same
            } else {
                OrientType::Opposite
            });
        }
    }
    Ok(out)
}

/// Options for [`build_with_options`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    /// Run the upstream healer after stitching. It creates UV trim curves and seam
    /// topology, but is expensive on large real-world bodies.
    pub heal: bool,
}

/// Builds one neutral body into a solidcore B-rep and stitches it.
pub fn build(body: &Body) -> Result<Brep, BuildError> {
    build_with_options(body, BuildOptions::default())
}

/// Like [`build`], but runs the healer after stitching.
pub fn build_healed(body: &Body) -> Result<Brep, BuildError> {
    build_with_options(body, BuildOptions { heal: true })
}

/// Builds one neutral body into a solidcore B-rep with explicit options.
pub fn build_with_options(body: &Body, options: BuildOptions) -> Result<Brep, BuildError> {
    let mut kernel = Kernel::default();
    let mut brep = p3::new_brep(
        &mut kernel,
        solidcore::geom::tol::USE_DEFAULT,
        solidcore::geom::tol::USE_DEFAULT,
    )?;
    let mut state = ArenaNotifyState::new(&brep);
    let mut env = state.env(None);
    let key_of: &dyn Fn(EntityRef) -> solidcore::geom::object::ObjectRef = env.key_of;
    let this_face = |face: FaceId| key_of(EntityRef::Face(face));
    let mut marks = Marks::default();
    let mut face_env = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    for face in &body.faces {
        let surface = Some(make_surface(&kernel, &face.surface)?);
        let input = face_input(&kernel, body, face)?;
        create_faces_with_curves(
            &mut kernel,
            &mut brep,
            None,
            &input.loops,
            Some(input.curves),
            &input.orients,
            surface,
            &mut face_env,
        )?;
    }
    drop(state);
    api_stitch_into_solid(&mut kernel, Some(&mut brep))?;
    if options.heal {
        api_heal_brep(&mut kernel, Some(&mut brep))?;
    }
    Ok(brep)
}

/// Whether every face has a UV trim curve (its own or its mate's).
pub fn has_uv_trims(brep: &Brep) -> Result<bool, BuildError> {
    for &face in brep.face_list() {
        if !trimming_tools::face_uv_curves_present(brep, face)? {
            return Ok(false);
        }
    }
    Ok(!brep.face_list().is_empty())
}

/// Rejects a B-rep that is not display-ready: missing UV trims would make the
/// tessellator fill the face's whole parameter domain and produce overlapping
/// "flat rectangle" geometry.
pub fn validate_for_display(brep: &Brep) -> Result<(), BuildError> {
    if brep.face_list().is_empty() {
        return Err(BuildError {
            message: "B-rep has no faces".into(),
        });
    }
    if !has_uv_trims(brep)? {
        return Err(BuildError {
            message: "B-rep has faces without UV trim curves".into(),
        });
    }
    Ok(())
}

/// Parses STEP bytes and builds every body.
pub fn import(bytes: &[u8], limits: &step_import::Limits) -> Result<Vec<Brep>, BuildError> {
    let model = step_import::import(bytes, limits).map_err(|error| BuildError {
        message: error.to_string(),
    })?;
    model.bodies.iter().map(build).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use step_import::model::{
        Bound, Curve, Edge, Face, Frame, Loop, OrientedEdge, Shell, Surface, Vertex,
    };

    fn triangle() -> Body {
        let frame = Frame {
            origin: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let vertices = vec![
            Vertex {
                step_id: 1,
                point: [0.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 2,
                point: [10.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 3,
                point: [0.0, 10.0, 0.0],
            },
        ];
        let line = |origin, dir, magnitude| Curve::Line {
            origin,
            dir,
            magnitude,
        };
        let edges = vec![
            Edge {
                step_id: 1,
                start: 0,
                end: 1,
                curve: line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 10.0),
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0,
            },
            Edge {
                step_id: 2,
                start: 1,
                end: 2,
                curve: line([10.0, 0.0, 0.0], [-1.0, 1.0, 0.0], 10.0),
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0,
            },
            Edge {
                step_id: 3,
                start: 2,
                end: 0,
                curve: line([0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 10.0),
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0,
            },
        ];
        Body {
            step_id: 1,
            name: Some("triangle".into()),
            kind: step_import::model::BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: None,
            shells: vec![Shell {
                step_id: 1,
                closed: true,
                orientation: true,
                faces: vec![0],
            }],
            faces: vec![Face {
                step_id: 1,
                surface: Surface::Plane { frame },
                same_sense: true,
                loops: vec![Loop {
                    outer: true,
                    orientation: true,
                    bound: Bound::Edges(vec![
                        OrientedEdge {
                            edge: 0,
                            forward: true,
                        },
                        OrientedEdge {
                            edge: 1,
                            forward: true,
                        },
                        OrientedEdge {
                            edge: 2,
                            forward: true,
                        },
                    ]),
                }],
            }],
            edges,
            vertices,
        }
    }

    #[test]
    fn builds_a_triangle_body() {
        let brep = build(&triangle()).expect("build");
        assert!(!brep.face_list().is_empty());
        assert!(!build_healed(&triangle())
            .expect("healed build")
            .face_list()
            .is_empty());
    }
}
