//! Build solidcore B-rep bodies from the neutral [`step_import::Model`], and map a
//! B-rep back to [`step_import::Model`] for export.
//!
//! [`brep_to_model()`] is the export counterpart: exact geometry and topology become a
//! neutral model that `step-export` writes as analytic/NURBS STEP.
//!
//! A body is built topology first ([`build_body`]): from the file's own topology, its
//! shared edges and vertices joined as the file joins them through the kernel's
//! `BrepConstructor`, leaving out a face whose construction fails. Where that does not
//! give a closed solid, the stitched build runs as before, the production counterpart
//! of the difftest `build_rust` recipe: each STEP body's faces, surfaces, curves and
//! loops are turned into a `Brep`, then stitched into a solid. Unsupported geometry
//! returns an [`BuildError`]; the caller decides whether to reject the whole file or
//! skip a body.
//!
//! Every build is display-ready: after stitching, faces on periodic surfaces get the
//! seam edges the file omits (a cylinder band written as two circles, a cone apex, ...),
//! splitting the face where the seam cannot be merged, and every face gets the UV trim
//! curve the tessellator and the mass integration read. As SMLib's importer does, a
//! face's parameter window and the angular seam of its cylinder or cone are chosen from
//! the edge vertices (the seam sits in the widest gap of the face's boundary points), so
//! a periodic face is never cut by the surface seam.
//!
//! [`build`] leaves the healer off. [`build_healed`] also runs `SmApiHealBrep`; healing
//! is not needed for the bodies this builder produces, and it is kept for callers that
//! imported an already damaged B-rep.
//!
//! Verified against SMLib's importer on the 331-face `nasty_cheese.stp` corpus fixture
//! (164 full-period cylinder bands): 332 faces, 1474 non-seam edges (1638 edge records
//! including 164 seam edges) and 1117 vertices, all equal to SMLib; area
//! `21330.0459756` against SMLib's `21330.0432020` (1.3e-7 relative) and volume
//! `28142.6196656` against `28142.6196320` (1.2e-9 relative) at accuracy `1e-9`. By
//! surface kind the area is plane `8365.1966` (SMLib `8365.264`), cylinder `12961.3151`
//! (`12961.244`) and sphere `3.5343` (`3.534`). The residual is the two integrators'
//! tolerance, not a geometry difference: the split face's two halves carry slightly
//! different trims but their areas sum to SMLib's.

pub mod brep_to_model;
mod constructor;

pub use brep_to_model::{brep_to_model, ExportModelError};
pub use constructor::FaceFailure;

use std::fmt;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
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
use solidcore::surface::surface::p2::SurfaceKind;
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p3;
use solidcore::topology::brep::p7;
use solidcore::topology::brep::p8::FaceBuildEnv;
use solidcore::topology::face::p2 as fp2;
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

fn make_surface(
    kernel: &Kernel,
    surface: &sm::Surface,
    frame_override: Option<&sm::Frame>,
) -> Result<Surface, SolidError> {
    Ok(match surface {
        sm::Surface::Plane { frame } => Surface::from(Plane::create_canonical(&placement(frame))?),
        sm::Surface::Cylinder { frame, radius } => {
            let frame = frame_override.unwrap_or(frame);
            Surface::from(Cone::create_canonical_cylinder(
                kernel,
                &placement(frame),
                *radius,
            )?)
        }
        sm::Surface::Cone {
            frame,
            radius,
            semi_angle_deg,
        } => {
            let frame = frame_override.unwrap_or(frame);
            Surface::from(Cone::create_canonical_with_semi_angle(
                kernel,
                &placement(frame),
                *radius,
                *semi_angle_deg,
                None,
                false,
                false,
            )?)
        }
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

/// The frame of a surface that has one, for the face UV domain and seam frame.
fn surface_frame(surface: &sm::Surface) -> Option<&sm::Frame> {
    match surface {
        sm::Surface::Plane { frame }
        | sm::Surface::Cylinder { frame, .. }
        | sm::Surface::Cone { frame, .. }
        | sm::Surface::Sphere { frame, .. }
        | sm::Surface::Torus { frame, .. } => Some(frame),
        sm::Surface::Revolution { .. }
        | sm::Surface::Extrusion { .. }
        | sm::Surface::BSpline(_) => None,
    }
}

/// The start point of an edge a face loop uses twice (a seam edge), if there is one.
fn face_seam_point(body: &Body, face: &sm::Face) -> Option<Point3> {
    for bound in &face.loops {
        let Bound::Edges(edges) = &bound.bound else {
            continue;
        };
        let mut seen: Vec<usize> = Vec::new();
        for edge in edges {
            if seen.contains(&edge.edge) {
                let record = body.edges.get(edge.edge)?;
                let vertex = body.vertices.get(record.start)?;
                return Some(v3(vertex.point));
            }
            seen.push(edge.edge);
        }
    }
    None
}

/// The frame a cylinder or cone should use so its angular seam lies where the face does
/// not reach (the middle of the largest gap between the face's boundary points), or on
/// the face's own seam edge. A periodic surface whose seam runs through the face would
/// split the face's parameter window; the SMLib importer moves it out of the way.
fn seam_frame(frame: &sm::Frame, points: &[Point3], seam: Option<Point3>) -> sm::Frame {
    let origin = v3(frame.origin);
    let Ok(x) = v3(frame.x).unitize() else {
        return *frame;
    };
    let Ok(y) = v3(frame.y).unitize() else {
        return *frame;
    };
    let Ok(z) = x.cross(y).unitize() else {
        return *frame;
    };
    let radial = |point: Point3| {
        let d = point - origin;
        let r = d - z * d.dot(z);
        r.length()
    };
    let direction = if let Some(point) = seam {
        let d = point - origin;
        (d - z * d.dot(z)).unitize().ok()
    } else {
        let mut angles: Vec<f64> = points
            .iter()
            .filter(|&&point| radial(point) > 1e-12)
            .map(|&point| {
                let d = point - origin;
                let r = d - z * d.dot(z);
                f64::atan2(r.dot(y), r.dot(x))
            })
            .collect();
        if angles.is_empty() {
            None
        } else {
            angles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            let mut largest = 0.0_f64;
            let mut middle = 0.0_f64;
            for (index, &angle) in angles.iter().enumerate() {
                let next = angles
                    .get(index + 1)
                    .copied()
                    .unwrap_or(angles.first().copied().unwrap_or(angle) + std::f64::consts::TAU);
                let gap = (next - angle).rem_euclid(std::f64::consts::TAU);
                if gap > largest {
                    largest = gap;
                    middle = angle + gap * 0.5;
                }
            }
            let (sin, cos) = middle.sin_cos();
            (x * cos + y * sin).unitize().ok()
        }
    };
    let Some(new_x) = direction else {
        return *frame;
    };
    let Some(new_y) = z.cross(new_x).unitize().ok() else {
        return *frame;
    };
    sm::Frame {
        origin: frame.origin,
        x: [new_x.x, new_x.y, new_x.z],
        y: [new_y.x, new_y.y, new_y.z],
        z: frame.z,
    }
}

/// Seventeen points along each face edge, as the SMLib import samples them when it picks
/// a face's parameter window and seam direction. Conics are sampled on their own frame
/// (the edge curve's parameter interval is in STEP degrees, not the conic's rational
/// parameter), B-splines by their control polygon.
fn sample_face_points(body: &Body, face: &sm::Face) -> Vec<Point3> {
    let mut points = Vec::new();
    for bound in &face.loops {
        let Bound::Edges(edges) = &bound.bound else {
            continue;
        };
        for oriented in edges {
            let Some(edge) = body.edges.get(oriented.edge) else {
                continue;
            };
            let start = body.vertices.get(edge.start).map(|v| v3(v.point));
            let end = body.vertices.get(edge.end).map(|v| v3(v.point));
            match (&edge.curve, start, end) {
                (sm::Curve::Line { .. }, Some(start), Some(end)) => {
                    points.push(start);
                    points.push(end);
                }
                (sm::Curve::Circle { frame, radius }, Some(start), Some(end)) => {
                    sample_conic(
                        &mut points,
                        frame,
                        *radius,
                        *radius,
                        start,
                        end,
                        edge.same_sense,
                        edge.start == edge.end,
                        ConicKind::Circle,
                    );
                }
                (
                    sm::Curve::Ellipse {
                        frame,
                        semi1,
                        semi2,
                    },
                    Some(start),
                    Some(end),
                ) => {
                    sample_conic(
                        &mut points,
                        frame,
                        *semi1,
                        *semi2,
                        start,
                        end,
                        edge.same_sense,
                        edge.start == edge.end,
                        ConicKind::Ellipse,
                    );
                }
                (sm::Curve::BSpline(spline), _, _) => {
                    points.extend(spline.points.iter().map(|point| v3(*point)));
                }
                _ => {}
            }
        }
    }
    points
}

/// Which conic is sampled: SMLib reverses a circle by flipping its axis, an ellipse by
/// reversing its sweep.
#[derive(Clone, Copy)]
enum ConicKind {
    /// A circle.
    Circle,
    /// An ellipse.
    Ellipse,
}

/// Seventeen points on the trimmed circle or ellipse from `start` to `end`, in the edge's
/// direction (the SMLib import's `_sample`).
#[allow(clippy::too_many_arguments)]
fn sample_conic(
    points: &mut Vec<Point3>,
    frame: &sm::Frame,
    radius1: f64,
    radius2: f64,
    start: Point3,
    end: Point3,
    same_sense: bool,
    closed: bool,
    kind: ConicKind,
) {
    let origin = v3(frame.origin);
    let Some(mut z) = v3(frame.x).cross(v3(frame.y)).unitize().ok() else {
        return;
    };
    let Some(x) = v3(frame.x).unitize().ok() else {
        return;
    };
    if matches!(kind, ConicKind::Circle) && !same_sense {
        z = Vec3::new(-z.x, -z.y, -z.z);
    }
    let y = z.cross(x);
    let angle = |point: Point3| {
        let d = point - origin;
        f64::atan2(d.dot(y) / radius2, d.dot(x) / radius1)
    };
    let t0 = angle(start);
    let span = if closed {
        std::f64::consts::TAU
    } else {
        let sweep = (angle(end) - t0).rem_euclid(std::f64::consts::TAU);
        if matches!(kind, ConicKind::Ellipse) && !same_sense {
            if sweep == 0.0 {
                -std::f64::consts::TAU
            } else {
                sweep - std::f64::consts::TAU
            }
        } else if sweep == 0.0 {
            std::f64::consts::TAU
        } else {
            sweep
        }
    };
    for step in 0..=16u32 {
        let t = t0 + span * f64::from(step) / 16.0;
        let (sin, cos) = t.sin_cos();
        points.push(origin + x * (radius1 * cos) + y * (radius2 * sin));
    }
}

/// The face's parameter window: the loops' extent in the surface parameters, padded by 5%
/// as SMLib's importer does. A face whose surface is unbounded in a parameter would
/// otherwise integrate and tessellate over the whole infinite domain.
fn face_uv_domain(surface: &Surface, frame: &sm::Frame, points: &[Point3]) -> Option<Extent2d> {
    if points.is_empty() {
        return None;
    }
    let origin = v3(frame.origin);
    let x = v3(frame.x).unitize().ok()?;
    let y = v3(frame.y).unitize().ok()?;
    let z = x.cross(y).unitize().ok()?;
    match surface.kind() {
        SurfaceKind::Plane(_) => {
            let mut lo = (f64::INFINITY, f64::INFINITY);
            let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
            for &point in points {
                let d = point - origin;
                lo.0 = lo.0.min(d.dot(x));
                hi.0 = hi.0.max(d.dot(x));
                lo.1 = lo.1.min(d.dot(y));
                hi.1 = hi.1.max(d.dot(y));
            }
            if !(lo.0.is_finite() && lo.1.is_finite() && hi.0.is_finite() && hi.1.is_finite()) {
                return None;
            }
            let pad = 0.05 * (hi.0 - lo.0).max(hi.1 - lo.1).max(1e-6) + 1e-6;
            Some(Extent2d::from_bounds(
                lo.0 - pad,
                lo.1 - pad,
                hi.0 + pad,
                hi.1 + pad,
            ))
        }
        SurfaceKind::Cone(_) => {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for &point in points {
                let h = (point - origin).dot(z);
                lo = lo.min(h);
                hi = hi.max(h);
            }
            if !(lo.is_finite() && hi.is_finite()) {
                return None;
            }
            let pad = 0.05 * (hi - lo).max(1e-6) + 1e-6;
            let u = surface.natural_uv_domain_if_built().u_interval();
            Some(Extent2d::from_bounds(u.min(), lo - pad, u.max(), hi + pad))
        }
        _ => None,
    }
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

/// How [`build_body`] chooses between the topology-first and the stitched build.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BuildMode {
    /// Topology first; then stitched; then, where stitching fails, the open topology
    /// build with its left-out faces.
    #[default]
    Default,
    /// As [`BuildMode::Default`], but where stitching fails its error is returned
    /// instead of the open topology build.
    Strict,
    /// The stitched build only, as before the topology-first build.
    StitchOnly,
}

/// Options for [`build_with_options`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    /// Run the upstream healer after the seam fix and UV trim pass. The seam topology and
    /// UV trims this builder creates make healing unnecessary for well-formed bodies,
    /// and the healer is expensive on large real-world bodies.
    pub heal: bool,
    /// Skip inserted seams on non-cylindrical cones. Fallback for unbounded ST-ACIS
    /// cones whose generated seam runs out to `SolidCore`'s infinity marker.
    pub skip_cone_seams: bool,
    /// How the build is chosen ([`build_body`]).
    pub mode: BuildMode,
}

/// Which way [`build_body`] built a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPath {
    /// From the file's topology: its shared edges and vertices joined as the file joins
    /// them (`SmBrepConstructor`).
    Topology,
    /// Each face built on its own, then stitched by geometry.
    Stitch,
}

/// A body [`build_body`] built.
pub struct BuiltBody {
    /// The B-rep.
    pub brep: Brep,
    /// How it was built.
    pub path: BuildPath,
    /// With [`BuildPath::Topology`]: the faces the build left out, and those whose seam
    /// fix or UV trim failed (kept as built).
    pub face_failures: Vec<FaceFailure>,
    /// With [`BuildPath::Stitch`]: why the topology-first build was not kept (its
    /// error, or the check it failed); `None` when it did not run.
    pub topology_note: Option<String>,
}

/// Builds one neutral body into a solidcore B-rep ([`build_body`]: from its topology,
/// else stitched).
pub fn build(body: &Body) -> Result<Brep, BuildError> {
    build_with_options(body, BuildOptions::default())
}

/// Like [`build`], then runs the healer on the seam-fixed, trimmed body.
pub fn build_healed(body: &Body) -> Result<Brep, BuildError> {
    build_with_options(
        body,
        BuildOptions {
            heal: true,
            ..BuildOptions::default()
        },
    )
}

/// [`build`] without generated seams on non-cylindrical cones.
///
/// Fallback for unbounded ST-ACIS cones whose generated seam would be parameterized
/// at `SolidCore`'s infinity marker.
pub fn build_without_cone_seams(body: &Body) -> Result<Brep, BuildError> {
    build_with_options(
        body,
        BuildOptions {
            skip_cone_seams: true,
            ..BuildOptions::default()
        },
    )
}

/// Builds one neutral body into a solidcore B-rep with explicit options
/// ([`build_body`]).
pub fn build_with_options(body: &Body, options: BuildOptions) -> Result<Brep, BuildError> {
    build_body(body, options).map(|built| built.brep)
}

/// Builds one body, topology first.
///
/// The body is built from the file's topology (the faces' shared edges and vertices
/// joined as the file joins them, no geometric stitching), leaving out a face whose
/// construction fails, then gets the seam fix, the UV trims and, with
/// [`BuildOptions::heal`], the healer (a face whose seam fix or UV trim fails is kept
/// as built and reported). A closed solid ([`is_closed_solid`]) that needed no repair
/// (no face left out or failing its pass, and the seam fix and UV-trim pass kept its
/// edge and vertex counts) is kept as it is. Otherwise the stitched build runs too. The
/// topology build is kept when it is a closed solid, except where the stitched build is
/// also closed and their volumes differ by more than 1e-4 relative: then the topology
/// build is made again with the healer before the seam fix, and that is kept if it
/// agrees with the stitched build (the file's topology closed with the wrong trims),
/// else the first topology build. A topology build that is not closed gives way to
/// the stitched build; where stitching fails too, [`BuildMode::Default`] keeps the
/// open topology build (its left-out faces in [`BuiltBody::face_failures`]) and
/// [`BuildMode::Strict`] returns the stitched build's error. [`BuildMode::StitchOnly`]
/// builds by stitching only.
///
/// # Errors
///
/// The stitched build's error when it fails and the topology build gives no B-rep
/// (or in [`BuildMode::Strict`]).
pub fn build_body(body: &Body, options: BuildOptions) -> Result<BuiltBody, BuildError> {
    let stitched_body = |brep, topology_note| BuiltBody {
        brep,
        path: BuildPath::Stitch,
        face_failures: Vec::new(),
        topology_note,
    };
    if options.mode == BuildMode::StitchOnly {
        return build_stitched(body, options).map(|brep| stitched_body(brep, None));
    }
    let topology_built = |heal_first| {
        build_topology(body, options, heal_first).map(|(brep, face_failures, repaired)| {
            let built = BuiltBody {
                brep,
                path: BuildPath::Topology,
                face_failures,
                topology_note: None,
            };
            (built, repaired)
        })
    };
    let (topology, repaired) = match topology_built(false) {
        Ok((built, repaired)) => (Ok(built), repaired),
        Err(error) => (Err(error), true),
    };
    let topology_volume = topology
        .as_ref()
        .ok()
        .and_then(|built| closed_volume(&built.brep));
    // A closed topology build that needed no repair is kept without the stitched build.
    if topology_volume.is_some() && !repaired {
        return topology.map_err(|message| BuildError { message });
    }
    let stitched = build_stitched(body, options);
    let Some(topology_volume) = topology_volume else {
        // not a closed solid: the stitched build, else (by default) the open topology
        // build
        let note = match &topology {
            Ok(_) => "not a closed solid".to_string(),
            Err(error) => error.clone(),
        };
        return match stitched {
            Ok(brep) => Ok(stitched_body(brep, Some(note))),
            Err(error) if options.mode == BuildMode::Strict => Err(error),
            Err(error) => topology.map_err(|_| error),
        };
    };
    // Closed. Checked against a closed stitched build: where their volumes disagree,
    // the file's topology may close with the wrong trims, which the healer mends; the
    // healed topology build is kept when it then agrees with the stitched one, else
    // the topology build (the closer of the two to the reference in most such cases).
    let stitched_volume = stitched.as_ref().ok().and_then(closed_volume);
    let agrees = |a: f64, b: f64| (a - b).abs() <= AGREEMENT * a.abs().max(b.abs());
    if let Some(stitched_volume) = stitched_volume {
        if !agrees(topology_volume, stitched_volume) && !options.heal {
            if let Ok((healed, _)) = topology_built(true) {
                if closed_volume(&healed.brep).is_some_and(|v| agrees(v, stitched_volume)) {
                    return Ok(healed);
                }
            }
        }
    }
    topology.map_err(|message| BuildError { message })
}

/// The relative volume difference within which the topology-first and stitched builds
/// of a body agree ([`build_body`]).
const AGREEMENT: f64 = 1e-4;

/// The accuracy of the volume [`closed_volume`] integrates.
const VOLUME_ACCURACY: f64 = 1e-5;

/// The volume of `brep` when it is a closed solid ([`is_closed_solid`]).
fn closed_volume(brep: &Brep) -> Option<f64> {
    if !solidcore::query::api_queries::brep_is_manifold_solid(brep).unwrap_or(false)
        || validate_for_display(brep).is_err()
    {
        return None;
    }
    let kernel = Kernel::default();
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attrs = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    let mut copy = brep.clone();
    solidcore::mass::integrator::properties::brep_volume(
        &kernel,
        &mut copy,
        VOLUME_ACCURACY,
        &attrs,
    )
    .ok()
    .filter(|volume| volume.is_finite() && *volume > 0.0)
}

/// Whether `brep` is a closed solid: a manifold solid, display-ready (every face
/// trimmed), with a finite positive volume (integrated to accuracy 1e-5).
pub fn is_closed_solid(brep: &Brep) -> bool {
    closed_volume(brep).is_some()
}

/// The topology-first build alone, with its passes (as [`build_body`] makes it, before
/// it is checked or replaced by the stitched build): for diagnostics.
///
/// # Errors
///
/// Why the topology-first build gave no B-rep.
pub fn build_from_topology(body: &Body, options: BuildOptions) -> Result<BuiltBody, BuildError> {
    build_topology(body, options, false)
        .map(|(brep, face_failures, _)| BuiltBody {
            brep,
            path: BuildPath::Topology,
            face_failures,
            topology_note: None,
        })
        .map_err(|message| BuildError { message })
}

/// The topology-first build with the seam fix, the UV trims and, with
/// [`BuildOptions::heal`], the healer, as the stitched build gets them; `heal_first`
/// heals the built B-rep before the seam fix instead. A face whose seam fix or UV trim
/// fails is kept as built and reported.
/// Also returns whether the build needed repair: a face left out or failing its pass,
/// or the seam fix or UV-trim pass changing the edge or vertex count (the file's
/// topology lacked seams, or closed with edges missing).
fn build_topology(
    body: &Body,
    options: BuildOptions,
    heal_first: bool,
) -> Result<(Brep, Vec<FaceFailure>, bool), String> {
    let constructor::Constructed {
        mut kernel,
        mut brep,
        faces: made,
        mut failures,
    } = constructor::construct(body)?;
    let file_face = |face: FaceId| {
        made.iter()
            .find(|(id, _)| *id == face)
            .map_or(0, |&(_, file)| file)
    };
    if heal_first {
        api_heal_brep(&mut kernel, Some(&mut brep)).map_err(|e| format!("heal: {e}"))?;
    }
    let counts = |brep: &Brep| (brep.edge_list().len(), brep.vertex_list().len());
    let as_built = counts(&brep);
    let faces: Vec<FaceId> = brep.face_list().to_vec();
    for face in faces {
        let cone = options.skip_cone_seams
            && fp2::surface(&brep, face).ok().flatten().is_some_and(
                |surface| matches!(surface.kind(), SurfaceKind::Cone(cone) if !cone.is_cylinder()),
            );
        if !cone {
            if let Err(error) = trimming_tools::split_at_seams(&mut kernel, &mut brep, face) {
                failures.push(FaceFailure {
                    body: body.step_id,
                    face: file_face(face),
                    reason: format!("seam fix: {error}"),
                });
            }
        }
    }
    let faces: Vec<FaceId> = brep.face_list().to_vec();
    for face in faces {
        let trimmed = trimming_tools::face_uv_curves_present(&brep, face).and_then(|present| {
            if present {
                Ok(())
            } else {
                trimming_tools::create_uv_trim_curves(&kernel, &mut brep, face)
            }
        });
        if let Err(error) = trimmed {
            failures.push(FaceFailure {
                body: body.step_id,
                face: file_face(face),
                reason: format!("UV trims: {error}"),
            });
        }
    }
    let repaired = !failures.is_empty() || counts(&brep) != as_built;
    if options.heal && !heal_first {
        api_heal_brep(&mut kernel, Some(&mut brep)).map_err(|e| e.to_string())?;
    }
    Ok((brep, failures, repaired))
}

/// The stitched build: each face on its own, stitched, seam-fixed and trimmed.
fn build_stitched(body: &Body, options: BuildOptions) -> Result<Brep, BuildError> {
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
    let face_env = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    // A face on a cylinder or cone gets its own surface frame: the angular seam is
    // moved into the largest gap between the face's boundary points (or onto the face's
    // seam edge), so the face's parameter window is not cut by it.
    let face_frame = |face: &sm::Face, points: &[Point3]| -> Option<sm::Frame> {
        let frame = surface_frame(&face.surface)?;
        match face.surface {
            sm::Surface::Cylinder { .. } | sm::Surface::Cone { .. } => {
                Some(seam_frame(frame, points, face_seam_point(body, face)))
            }
            _ => Some(*frame),
        }
    };
    for face in &body.faces {
        let input = face_input(&kernel, body, face)?;
        let points = sample_face_points(body, face);
        let frame = face_frame(face, &points);
        let surface = make_surface(&kernel, &face.surface, frame.as_ref())?;
        let uv_domain = frame
            .as_ref()
            .and_then(|frame| face_uv_domain(&surface, frame, &points))
            .map_or_else(|| surface.natural_uv_domain_with_nurb(&kernel), Ok)?;
        let curves = p7::FaceCurvesInput {
            loops: input.loops,
            curves_3d: Some(input.curves),
            uv_curves: None,
            orientations: input.orients,
            loop_points: Vec::new(),
        };
        // Build the trimmed face from all of its loops at once. The incremental
        // big-face-then-trim path splits multi-loop faces and lazily fits UV trim
        // curves, which fails for lines projected onto periodic surfaces.
        p7::make_face_with_curves(
            &mut kernel,
            &mut brep,
            None,
            curves,
            surface,
            &uv_domain,
            if face.same_sense {
                OrientType::Same
            } else {
                OrientType::Opposite
            },
            None,
            face_env.this_face,
            face_env.env,
        )?;
    }
    drop(state);
    api_stitch_into_solid(&mut kernel, Some(&mut brep))?;
    // Periodic faces bounded without a seam edge (a cylinder band written as two
    // circles, a cone apex, ...) need the seam before the face can be trimmed and
    // tessellated. The kernel inserts it and splits the face where the seam cannot be
    // merged; then every face gets the UV trim the tessellator and mass integration
    // read. `SplitAtSeams` is called for every face: for a face without a seam problem
    // it returns no new faces (`SplitAtSeam` checks the crossed/missing flags, not
    // `HasSeamProblem`'s first-call return).
    let faces: Vec<FaceId> = brep.face_list().to_vec();
    for face in faces {
        let cone = options.skip_cone_seams
            && fp2::surface(&brep, face).ok().flatten().is_some_and(
                |surface| matches!(surface.kind(), SurfaceKind::Cone(cone) if !cone.is_cylinder()),
            );
        if !cone {
            trimming_tools::split_at_seams(&mut kernel, &mut brep, face)?;
        }
    }
    let faces: Vec<FaceId> = brep.face_list().to_vec();
    for face in faces {
        if !trimming_tools::face_uv_curves_present(&brep, face)? {
            trimming_tools::create_uv_trim_curves(&kernel, &mut brep, face)?;
        }
    }
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

/// Built bodies and the mapper's ordered, owned import findings.
pub struct ImportReport {
    /// Successfully built bodies, in mapper order.
    pub bodies: Vec<Brep>,
    /// Bodies skipped by the mapper, with their original reasons.
    pub skipped: Vec<step_import::model::Skipped>,
    /// Mapper diagnostics in their original order.
    pub diagnostics: Vec<step_import::model::Diagnostic>,
    /// Bodies built from the file's topology ([`BuildPath::Topology`]).
    pub constructed: usize,
    /// Faces the topology-first build left out of the bodies it built, in body order.
    pub face_failures: Vec<FaceFailure>,
}

/// Parses STEP bytes and builds every mapped body, preserving mapper findings.
///
/// Any parse or build failure returns an error, never a partial report.
pub fn import_with_report(
    bytes: &[u8],
    limits: &step_import::Limits,
) -> Result<ImportReport, BuildError> {
    let model = step_import::import(bytes, limits).map_err(|error| BuildError {
        message: error.to_string(),
    })?;
    let mut bodies = Vec::new();
    let mut constructed = 0;
    let mut face_failures = Vec::new();
    for body in &model.bodies {
        let built = build_body(body, BuildOptions::default())?;
        bodies.push(built.brep);
        if built.path == BuildPath::Topology {
            constructed += 1;
        }
        face_failures.extend(built.face_failures);
    }
    Ok(ImportReport {
        bodies,
        skipped: model.skipped,
        diagnostics: model.diagnostics,
        constructed,
        face_failures,
    })
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
                curve: line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 1.0),
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0,
            },
            Edge {
                step_id: 2,
                start: 1,
                end: 2,
                // The hypotenuse is 10*sqrt(2) long in a unit direction.
                curve: line(
                    [10.0, 0.0, 0.0],
                    [
                        -std::f64::consts::FRAC_1_SQRT_2,
                        std::f64::consts::FRAC_1_SQRT_2,
                        0.0,
                    ],
                    1.0,
                ),
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0 * std::f64::consts::SQRT_2,
            },
            Edge {
                step_id: 3,
                start: 2,
                end: 0,
                curve: line([0.0, 10.0, 0.0], [0.0, -1.0, 0.0], 1.0),
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

    /// A full-period cylindrical band: two closed circular loops on one periodic
    /// surface. Building all loops at once is what keeps the construction off the
    /// incremental split path that failed on this shape.
    fn cylinder_band() -> Body {
        let frame = Frame {
            origin: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let vertices = vec![
            Vertex {
                step_id: 1,
                point: [10.0, 0.0, 0.0],
            },
            Vertex {
                step_id: 2,
                point: [10.0, 0.0, 20.0],
            },
        ];
        let edges = vec![
            Edge {
                step_id: 1,
                start: 0,
                end: 0,
                curve: Curve::Circle {
                    frame,
                    radius: 10.0,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
            Edge {
                step_id: 2,
                start: 1,
                end: 1,
                curve: Curve::Circle {
                    frame: Frame {
                        origin: [0.0, 0.0, 20.0],
                        ..frame
                    },
                    radius: 10.0,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 360.0,
            },
        ];
        let loop_ = |edge| Loop {
            outer: edge == 0,
            orientation: true,
            bound: Bound::Edges(vec![OrientedEdge {
                edge,
                forward: true,
            }]),
        };
        Body {
            step_id: 2,
            name: Some("cylinder band".into()),
            kind: step_import::model::BodyKind::ManifoldSolid,
            unit_mm: 1.0,
            uncertainty_mm: None,
            shells: vec![Shell {
                step_id: 2,
                closed: true,
                orientation: true,
                faces: vec![0],
            }],
            faces: vec![Face {
                step_id: 2,
                surface: Surface::Cylinder {
                    frame,
                    radius: 10.0,
                },
                same_sense: true,
                loops: vec![loop_(0), loop_(1)],
            }],
            edges,
            vertices,
        }
    }

    /// The same two-loop band on a surface of extrusion. The incremental split path
    /// recognizes the surface on a split face without building its NURB, which is what
    /// broke `moon_buggy.stp` during stitching.
    fn extrusion_band() -> Body {
        let mut body = cylinder_band();
        let frame = Frame {
            origin: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let face = body.faces.first_mut().expect("band face");
        face.surface = Surface::Extrusion {
            curve: Box::new(Curve::Circle {
                frame,
                radius: 10.0,
            }),
            dir: [0.0, 0.0, 1.0],
            magnitude: 20.0,
        };
        body
    }

    /// A closed cylinder: two circular caps and the two-loop side band, sharing the two
    /// circles. The band is the periodic face whose seam the importer has to add.
    fn cylinder_solid() -> Body {
        let identity = Frame {
            origin: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
        };
        let mut body = cylinder_band();
        body.step_id = 3;
        body.name = Some("cylinder solid".into());
        body.shells = vec![Shell {
            step_id: 3,
            closed: true,
            orientation: true,
            faces: vec![0, 1, 2],
        }];
        body.faces = vec![
            Face {
                step_id: 3,
                surface: Surface::Cylinder {
                    frame: identity,
                    radius: 10.0,
                },
                same_sense: true,
                loops: vec![
                    Loop {
                        outer: true,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 0,
                            forward: true,
                        }]),
                    },
                    Loop {
                        outer: false,
                        orientation: true,
                        bound: Bound::Edges(vec![OrientedEdge {
                            edge: 1,
                            forward: false,
                        }]),
                    },
                ],
            },
            Face {
                step_id: 4,
                surface: Surface::Plane {
                    frame: Frame {
                        y: [0.0, -1.0, 0.0],
                        ..identity
                    },
                },
                same_sense: true,
                loops: vec![Loop {
                    outer: true,
                    orientation: true,
                    bound: Bound::Edges(vec![OrientedEdge {
                        edge: 0,
                        forward: false,
                    }]),
                }],
            },
            Face {
                step_id: 5,
                surface: Surface::Plane {
                    frame: Frame {
                        origin: [0.0, 0.0, 20.0],
                        ..identity
                    },
                },
                same_sense: true,
                loops: vec![Loop {
                    outer: true,
                    orientation: true,
                    bound: Bound::Edges(vec![OrientedEdge {
                        edge: 1,
                        forward: true,
                    }]),
                }],
            },
        ];
        body
    }

    #[test]
    fn builds_a_multi_loop_extrusion_band() {
        let brep = build(&extrusion_band()).expect("extrusion band build");
        assert_eq!(brep.face_list().len(), 1);
        // The seam edge joins the two loops; the generator's start point is already a
        // vertex, so no loop edge needs splitting.
        assert_eq!(brep.edge_list().len(), 3);
        assert!(has_uv_trims(&brep).expect("uv trims"));
        validate_for_display(&brep).expect("display");
    }

    #[test]
    fn builds_a_multi_loop_cylinder_band() {
        let brep = build(&cylinder_band()).expect("cylinder band build");
        assert_eq!(brep.face_list().len(), 1);
        // The surface seam is placed in the largest gap between the boundary points, so
        // each closed circle is split where it crosses it, and one seam edge joins the
        // two loops: 2 split circles + 1 seam.
        assert_eq!(brep.edge_list().len(), 5);
        assert_eq!(brep.vertex_list().len(), 5);
        assert!(has_uv_trims(&brep).expect("uv trims"));
        validate_for_display(&brep).expect("display");
    }

    #[test]
    fn builds_a_closed_seamed_cylinder() {
        let brep = build(&cylinder_solid()).expect("cylinder solid build");
        assert_eq!(brep.face_list().len(), 3);
        assert!(has_uv_trims(&brep).expect("uv trims"));
        validate_for_display(&brep).expect("display");
        let kernel = Kernel::default();
        let mut solid = brep.clone();
        let area = solidcore::mass::integrator::properties::brep_area(&kernel, &mut solid, 1e-9)
            .expect("area");
        let expected_area =
            2.0 * std::f64::consts::PI * 10.0 * 10.0 + 2.0 * std::f64::consts::PI * 10.0 * 20.0;
        // The stitcher re-fits the faces' UV trims, so the area carries the kernel's
        // fitting error (1e-4 relative here); the volume's radial errors cancel.
        assert!(
            (area - expected_area).abs() <= 1e-3 * expected_area,
            "area {area} vs {expected_area}"
        );
        let store = solidcore::geom::attribute::AttributeStore::default();
        let attrs = solidcore::mass::integrator::properties::PropertyAttributes {
            store: &store,
            key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
        };
        let volume =
            solidcore::mass::integrator::properties::brep_volume(&kernel, &mut solid, 1e-9, &attrs)
                .expect("volume");
        let expected_volume = std::f64::consts::PI * 10.0 * 10.0 * 20.0;
        assert!(
            (volume - expected_volume).abs() <= 1e-6 * expected_volume,
            "volume {volume} vs {expected_volume}"
        );
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
