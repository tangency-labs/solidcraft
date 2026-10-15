//! Face queries and checked curve evaluation entry points.
//!
//! Upstream: `source/SM_API/src/SmApiCurves.cpp`, `source/SMLib/inc/SmCurve.h`,
//! `source/SMLib/src/SmFace.cpp`, `source/SMLib/src/SmShell.cpp`,
//! `source/SMLib/src/SmRegion.cpp`, `source/SMLib/inc/SmCurveClass.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`evaluate_curve`] | `SmApiEvaluateCurve` |
//! | [`calculate_bounding_box`] | `SmFace::CalculateBoundingBox` |
//! | [`calculate_shell_bounding_box`] | `SmShell::CalculateBoundingBox` |
//! | [`calculate_region_bounding_box`] | `SmRegion::CalculateBoundingBox` |
//! | [`edge_curve_on_classify`] | `SmFace::EdgeCurveOnClassify` |
//! | [`ClassifyProblem`] | `SmClassifyProblem` |
//! | [`point_3d_classify`], [`FaceClassifyWorld`] | `SmFace::Point3DClassify` |
//!
//! Deviations: a nullable curve is `Option<&Curve>`. The three optional output
//! pointers become a returned array `(position, first derivative, second
//! derivative)`; evaluation still requests all three, even when callers discard
//! some outputs. Box APIs retain in/out boxes: regions and shells initialize
//! their output before work and may leave a partial union on error; loose face
//! boxes replace their output only on success. No recursion is introduced.

use crate::curve::curve::Curve;
use crate::geom::extent3d::Extent3d;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::topology::face::p2 as face;
use crate::topology::shell::{self, ShellType};
use crate::topology::{Brep, EdgeId, FaceId, RegionId, ShellId, VertexId};
use crate::{Result, SolidError};

/// Evaluate position and the first two derivatives within the natural domain.
///
/// Uses the API's inclusive range check and the upstream defaults: evaluation
/// from the left, with nonzero tangents enabled. Both settings affect repeated
/// knots and collapsed tangents.
///
/// # Errors
/// Returns invalid input for a null curve, NaN parameter or parameter outside the
/// natural domain. Propagates the concrete curve evaluator's error unchanged.
pub fn evaluate_curve(curve: Option<&Curve>, parameter: f64) -> Result<[Vec3; 3]> {
    const OP: &str = "query::face::p1::evaluate_curve";
    let curve = curve.ok_or_else(|| SolidError::invalid_input(OP, "null curve"))?;
    let domain = curve.natural_interval();
    // Keep the negated conjunction: NaN must fail the API's range check.
    if !(parameter >= domain.min() && parameter <= domain.max()) {
        return Err(SolidError::invalid_input(
            OP,
            "parameter outside natural domain",
        ));
    }
    let values = curve.evaluate(parameter, 2, true, true)?;
    let mut iter = values.into_iter();
    let missing = || SolidError::invalid_input(OP, "curve returned too few derivatives");
    Ok([
        iter.next().ok_or_else(missing)?,
        iter.next().ok_or_else(missing)?,
        iter.next().ok_or_else(missing)?,
    ])
}

fn face_vertices(brep: &Brep, face: FaceId) -> Result<Vec<VertexId>> {
    super::p2::face_vertices(brep, face, false, None)
}

fn face_edges(_brep: &Brep, _face: FaceId) -> Result<Vec<EdgeId>> {
    // UNPORTED(topology.face_p4): SmFace::GetEdges
    Err(SolidError::unported("SmFace::GetEdges", "topology.face_p4"))
}

fn tight_face_box(
    _kernel: &Kernel,
    _brep: &Brep,
    _face: FaceId,
    _output: &mut Extent3d,
) -> Result<()> {
    // UNPORTED(query.face_p3): SmFace::CalculateTightBoundingBox
    Err(SolidError::unported(
        "SmFace::CalculateTightBoundingBox",
        "query.face_p3",
    ))
}

/// Compute a face box; `tight` defaults to false in upstream.
///
/// A linear surface direction uses vertex and edge boxes, expanded separately
/// by their own tolerances. Other surfaces use the actual trimmed UV domain.
///
/// # Errors
/// Invalid arena links or a missing surface (a native null dereference), child
/// bounding-box errors, or an explicit missing dependency. The loose branch
/// leaves `output` unchanged on failure; the tight branch forwards its in/out box.
pub fn calculate_bounding_box(
    kernel: &Kernel,
    brep: &Brep,
    id: FaceId,
    tight: bool,
    output: &mut Extent3d,
) -> Result<()> {
    if tight {
        return tight_face_box(kernel, brep, id, output);
    }
    let surface = face::surface(brep, id)?.ok_or_else(|| {
        SolidError::invalid_input(
            "query::face::p1::calculate_bounding_box",
            "face without surface",
        )
    })?;
    let mut result = Extent3d::new();
    if surface.degree(SurfParamType::U) == 1 || surface.degree(SurfParamType::V) == 1 {
        for vertex in face_vertices(brep, id)? {
            let vertex = brep.vertex(vertex)?;
            let mut bbox = Extent3d::from_point(vertex.point);
            bbox.expand_absolute(vertex.tolerance);
            result = result.union(&bbox);
        }
        for edge in face_edges(brep, id)? {
            let edge = brep.edge(edge)?;
            if let Some(curve) = &edge.curve {
                let boxes = curve.calculate_bounding_box(
                    kernel,
                    &edge.interval,
                    BoundingBoxRequest {
                        normal: true,
                        pseudo: false,
                        polar: false,
                    },
                    true,
                )?;
                let mut bbox = boxes.normal.ok_or_else(|| {
                    SolidError::invalid_input(
                        "query::face::p1::calculate_bounding_box",
                        "missing requested edge box",
                    )
                })?;
                bbox.expand_absolute(edge.tolerance);
                result = result.union(&bbox);
            }
        }
    } else {
        result = surface
            .calculate_bounding_box(
                kernel,
                &face::uv_domain(brep, id)?,
                [true, false, false],
                true,
            )?
            .normal_box
            .ok_or_else(|| {
                SolidError::invalid_input(
                    "query::face::p1::calculate_bounding_box",
                    "missing requested surface box",
                )
            })?;
    }
    *output = result;
    Ok(())
}

/// Compute a shell box, with `tight` defaulting to false upstream.
///
/// Vertex shells use the point without tolerance expansion; wire shells use
/// each edge's box; face shells visit the native face-use list in order.
///
/// # Errors
/// Invalid links or a missing vertex/face (a native null dereference), and child
/// box errors. `output` is initialized first and retains earlier unions on error.
pub fn calculate_shell_bounding_box(
    kernel: &Kernel,
    brep: &Brep,
    id: ShellId,
    tight: bool,
    output: &mut Extent3d,
) -> Result<()> {
    output.init();
    let shell = brep.shell(id)?;
    let mut current = Extent3d::new();
    match shell.shell_type {
        ShellType::Vertex => {
            let vertex = shell.vertex(brep)?.ok_or_else(|| {
                SolidError::invalid_input(
                    "query::face::p1::calculate_shell_bounding_box",
                    "vertex shell without vertex",
                )
            })?;
            output.add_point(brep.vertex(vertex)?.point);
        }
        ShellType::Wire => {
            for edge in shell::wire_edges(brep, id)? {
                current = crate::query::edge::calculate_bounding_box(
                    kernel,
                    brep,
                    edge,
                    BoundingBoxRequest {
                        normal: true,
                        pseudo: false,
                        polar: false,
                    },
                    tight,
                )?
                .normal
                .ok_or_else(|| {
                    SolidError::invalid_input(
                        "query::face::p1::calculate_shell_bounding_box",
                        "missing requested edge box",
                    )
                })?;
                *output = output.union(&current);
            }
        }
        ShellType::Faceuse => {
            for faceuse in shell.get_faceuses()? {
                let face = brep.faceuse(faceuse)?.face.ok_or_else(|| {
                    SolidError::invalid_input(
                        "query::face::p1::calculate_shell_bounding_box",
                        "face use without face",
                    )
                })?;
                calculate_bounding_box(kernel, brep, face, tight, &mut current)?;
                *output = output.union(&current);
            }
        }
        ShellType::Unknown | ShellType::Other(_) => {}
    }
    Ok(())
}

/// Compute the union of the region's shell boxes; `tight` defaults to false upstream.
///
/// # Errors
/// An empty shell list returns `SM_ERR`, leaving an initialized empty output.
/// Child failures propagate while retaining the union of earlier shells.
pub fn calculate_region_bounding_box(
    kernel: &Kernel,
    brep: &Brep,
    id: RegionId,
    tight: bool,
    output: &mut Extent3d,
) -> Result<()> {
    output.init();
    let shells = brep.region(id)?.get_shells(None, |_, _| Ok(false))?;
    if shells.is_empty() {
        return Err(SolidError::sm_err(
            "query::face::p1::calculate_region_bounding_box",
        ));
    }
    let mut current = Extent3d::new();
    for shell in shells {
        calculate_shell_bounding_box(kernel, brep, shell, tight, &mut current)?;
        *output = output.union(&current);
    }
    Ok(())
}

/// Diagnostic reason attached to a curve-sector classification (`SmClassifyProblem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum ClassifyProblem {
    /// No classification problem (`SM_CP_NONE`).
    #[default]
    None = 0,
    /// The vertex marks a surface singularity.
    VertexOnSingularity = 1,
    /// The classification changed once as the step-off grew.
    ToggledClassification = 2,
    /// The first bounding edge use is too short.
    ShortSectorEdgeuse1 = 3,
    /// The second bounding edge use is too short.
    ShortSectorEdgeuse2 = 4,
    /// The classified curve interval is too short.
    ShortInterval = 5,
    /// The classification changed more than once.
    SeesawClassification = 6,
    /// The iteration bound was reached without a definitive classification.
    UnresolvedClassification = 7,
}

/// The in/out answers of `SmFace::EdgeCurveOnClassify`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CurveOnClassification {
    /// Whether the segment is inside the face.
    pub inside: bool,
    /// Whether the classification encountered a problem.
    pub possible_problem: bool,
    /// The most recent reported problem.
    pub problem: ClassifyProblem,
}

/// Parameters passed unchanged to the face's sector classifier.
#[derive(Debug, Clone, Copy)]
pub struct EdgeCurveOnOptions {
    /// Parameter of the edge/curve intersection.
    pub edge_parameter: f64,
    /// Range of interest on the curve being classified.
    pub curve_interval: crate::geom::extent1d::Extent1d,
    /// Direction to classify, relative to the curve parameterization.
    pub curve_orientation: crate::nurbs::b_spline_surface_n_lib::p2::OrientType,
    /// The source curve's three-dimensional zone tolerance.
    pub curve_tolerance: f64,
    /// Whether classification may use UV-space recovery (upstream default true).
    pub uv_space_okay: bool,
    /// Optional native face debug index (upstream default `SM_UNDEF_ULONG`).
    pub face_index: u64,
}

impl EdgeCurveOnOptions {
    /// Required intersection data, with the two upstream optional defaults.
    #[must_use]
    pub const fn new(
        edge_parameter: f64,
        curve_interval: crate::geom::extent1d::Extent1d,
        curve_orientation: crate::nurbs::b_spline_surface_n_lib::p2::OrientType,
        curve_tolerance: f64,
    ) -> Self {
        Self {
            edge_parameter,
            curve_interval,
            curve_orientation,
            curve_tolerance,
            uv_space_okay: true,
            face_index: u64::MAX,
        }
    }
}

fn sector_classify_curve_on(
    _kernel: &Kernel,
    _brep: &Brep,
    _face: FaceId,
    _boundary: (crate::topology::EdgeuseId, f64),
    _curve: &Curve,
    _options: &EdgeCurveOnOptions,
    _output: &mut CurveOnClassification,
) -> Result<()> {
    // UNPORTED(query.face_p3): SmFace::SectorClassifyCurveOn
    // This call uses the same edge use in both directions, the edge's own zone
    // tolerance for both boundaries, and the native nonsingular (false, UV 0,0)
    // arguments. Caller curve parameters, flags and face index are retained above.
    Err(SolidError::unported(
        "SmFace::SectorClassifyCurveOn",
        "query.face_p3",
    ))
}

/// Classify a curve segment crossing one face boundary, visiting all upward uses.
///
/// Initializes all answers after the null-edge check. A seam can have two upward
/// edge uses: the first outside result must not terminate the search. Problems
/// accumulate until a definitive inside result or the end of the list.
///
/// # Errors
/// Native null-edge status (without changing outputs), invalid topology links,
/// or a genuine missing curve-on-surface/sector-classification dependency.
pub fn edge_curve_on_classify(
    kernel: &Kernel,
    brep: &Brep,
    face_id: FaceId,
    edge: Option<EdgeId>,
    curve: &Curve,
    options: &EdgeCurveOnOptions,
    output: &mut CurveOnClassification,
) -> Result<()> {
    let edge =
        edge.ok_or_else(|| SolidError::null_pointer("query::face::p1::edge_curve_on_classify"))?;
    *output = CurveOnClassification::default();
    let edgeuses = &brep.edge(edge)?.edgeuses;
    let upward = face::upward_faceuse(brep, face_id)?;
    let lifted;
    let curve = if curve.dim() == 2 {
        let surface = face::surface(brep, face_id)?.ok_or_else(|| {
            SolidError::invalid_input(
                "query::face::p1::edge_curve_on_classify",
                "face without surface",
            )
        })?;
        let crate::surface::surface::SurfaceKind::BSpline(surface) = surface.kind() else {
            // DISPATCH GAP: CrvOnSurf currently stores only a BSplineSurface.
            // SetAll itself is ported; other concrete surfaces need its actual
            // polymorphic representation, never a B-spline stand-in.
            return Err(SolidError::unported(
                "SmCrvOnSurf surface-kind dispatch",
                "curve.crv_on_surf",
            ));
        };
        // Native SetAll borrows the UV curve (owner flag zero). Clone preserves
        // geometry but resets the Curve owner; retain that actual metadata here.
        let mut uv_curve = curve.clone();
        let _ = uv_curve.set_owner(curve.owner(), None);
        lifted = Curve::new(crate::curve::curve::CurveKind::CrvOnSurf(Box::new(
            crate::curve::crv_on_surf::CrvOnSurf::new(kernel, uv_curve, surface.clone(), None, 0)?,
        )));
        &lifted
    } else {
        curve
    };
    for &edgeuse in edgeuses {
        if crate::topology::edgeuse::p1::face(brep, edgeuse)? != Some(face_id)
            || crate::topology::edgeuse::p1::faceuse(brep, edgeuse)? != upward
        {
            continue;
        }
        let mut current = CurveOnClassification::default();
        sector_classify_curve_on(
            kernel,
            brep,
            face_id,
            (edgeuse, brep.edge(edge)?.tolerance),
            curve,
            options,
            &mut current,
        )?;
        if current.possible_problem {
            output.possible_problem = true;
            output.problem = current.problem;
        }
        if current.inside {
            output.inside = true;
            return Ok(());
        }
    }
    Ok(())
}

/// Options of `SmFace::Point3DClassify`, including loop restrictions and the UV hint.
#[derive(Debug, Clone, Copy)]
pub struct FacePointClassifyOptions<'a> {
    /// Already converted source zone tolerance, stored without re-clamping.
    pub source_zone_tolerance: f64,
    /// Intersect the point with face boundaries before loop containment.
    pub boundary_intersections: bool,
    /// Optional hint for the initial local surface solve.
    pub guess_uv: Option<crate::geom::vector2d::Point2>,
    /// Restrict classification to the requested loops (outer loop if absent/empty).
    pub target_loops_only: bool,
    /// Requested loop list, retaining caller order.
    pub target_loops: Option<&'a [crate::topology::LoopId]>,
    /// Allow UV-space recovery (upstream default true).
    pub uv_space_okay: bool,
}

impl FacePointClassifyOptions<'_> {
    /// Required source data with the upstream optional defaults.
    #[must_use]
    pub const fn new(source_zone_tolerance: f64, boundary_intersections: bool) -> Self {
        Self {
            source_zone_tolerance,
            boundary_intersections,
            guess_uv: None,
            target_loops_only: false,
            target_loops: None,
            uv_space_okay: true,
        }
    }
}

/// Maps a face and its surface to the caller's actual classification handles.
/// The geometry interface also resolves the eventual classified object's zone
/// tolerance. No identity is synthesized from a pointer or an arena address.
pub trait FaceClassifyWorld<O: crate::intersect::point_class::ClassObject>:
    crate::intersect::point_class::ClassGeometry<O>
{
    /// The actual object handle of this face.
    fn face_handle(&self, face: FaceId) -> O;
    /// The actual object handle of this face's surface.
    fn surface_handle(&self, face: FaceId) -> O;
    /// Forward UV classification with surface intersection testing disabled,
    /// retaining all caller boundary, tolerance, loop and UV-recovery options.
    ///
    /// # Errors
    /// Propagates the actual UV classifier's status, retaining its in/out writes.
    /// Implementations can use [`super::p2::point_classify`] with
    /// [`crate::intersect::point_class::PointClassification::try_map_objects`]
    /// to translate between their handles and B-rep entity ids.
    fn classify_uv(
        &self,
        face: FaceId,
        uv: crate::geom::vector2d::Point2,
        options: &FacePointClassifyOptions<'_>,
        output: &mut crate::intersect::point_class::PointClassification<O>,
    ) -> Result<()>;
}

fn solve_face_point<O: crate::geom::solution_array::SolutionObject>(
    _surface: &crate::surface::surface::Surface,
    _cache: Option<&crate::surface::surface_cache::p1::SurfaceCache<O>>,
    _domain: &crate::geom::extent2d::Extent2d,
    _point: crate::geom::vector3d::Point3,
    _zone_tolerance: f64,
    guess: Option<crate::geom::vector2d::Point2>,
    _handle: O,
) -> Result<Option<crate::geom::solution_array::Solution<O>>> {
    // Both calls use SM_SO_INTERSECT. The global call uses no target distance
    // and SM_SR_SINGLE; this caller consumes only the first returned solution.
    if guess.is_some() {
        // UNPORTED(surface.surface_p5): SmSurface::LocalPointSolve
        Err(SolidError::unported(
            "SmSurface::LocalPointSolve",
            "surface.surface_p5",
        ))
    } else {
        // UNPORTED(surface.surface_p4): SmSurface::GlobalPointSolve
        Err(SolidError::unported(
            "SmSurface::GlobalPointSolve",
            "surface.surface_p4",
        ))
    }
}

/// Classify a 3D point against a trimmed face, using the supplied actual handles.
///
/// Initializes class/face/source tolerance while retaining other caller state.
/// A hint tries the local solver before falling back to the global solver; the
/// source tolerance is never increased to find an answer. UV classification is
/// followed by the actual 3D gap and the source/object intersection-tolerance test.
///
/// # Errors
/// Invalid topology or geometry handles, cache/solve/classification errors, and
/// genuine missing dependencies. In/out classification writes survive failure.
pub fn point_3d_classify<
    O: crate::intersect::point_class::ClassObject + crate::geom::solution_array::SolutionObject,
>(
    kernel: &Kernel,
    brep: &Brep,
    id: FaceId,
    world: &impl FaceClassifyWorld<O>,
    point: crate::geom::vector3d::Point3,
    options: &FacePointClassifyOptions<'_>,
    output: &mut crate::intersect::point_class::PointClassification<O>,
) -> Result<()> {
    use crate::intersect::point_class::PointClassType;
    const OP: &str = "query::face::p1::point_3d_classify";
    let surface = face::surface(brep, id)?
        .ok_or_else(|| SolidError::invalid_input(OP, "face without surface"))?;
    output.set_class_object(PointClassType::Unknown, None);
    output.set_face(Some(world.face_handle(id)));
    output.set_src_zone_tol_3d(options.source_zone_tolerance);
    // The cache factory returns an owned working cache. Disabling its point
    // test cannot change another caller's cache; dropping it ends this scope,
    // replacing native checkout/checkin and the temporary Boolean restoration.
    let handle = world.surface_handle(id);
    let mut cache = crate::error::ignored(crate::surface::surface::p1::get_surface_cache(
        kernel,
        surface,
        Some(handle),
    ))?;
    if let Some(cache) = &mut cache {
        cache.point_test_enabled = false;
    }
    let domain = face::uv_domain(brep, id)?;
    let mut solution = None;
    if let Some(guess) = options.guess_uv {
        solution = solve_face_point(
            surface,
            cache.as_ref(),
            &domain,
            point,
            options.source_zone_tolerance,
            Some(guess),
            handle,
        )?;
    }
    if solution.is_none() {
        solution = solve_face_point(
            surface,
            cache.as_ref(),
            &domain,
            point,
            options.source_zone_tolerance,
            None,
            handle,
        )?;
    }
    let Some(solution) = solution else {
        return Ok(());
    };
    let parameters = &solution.start.parameters;
    let uv = crate::geom::vector2d::Point2::new(
        *parameters
            .first()
            .ok_or_else(|| SolidError::invalid_input(OP, "missing u parameter"))?,
        *parameters
            .get(1)
            .ok_or_else(|| SolidError::invalid_input(OP, "missing v parameter"))?,
    );
    let size = domain.size().length();
    if !domain.contains_point(uv, size / 1000.0) {
        return Ok(());
    }
    world.classify_uv(id, uv, options, output)?;
    if let Some(surface_point) = crate::error::ignored(surface.evaluate_point(uv))? {
        output.set_gap_3d(point.distance(surface_point));
    }
    // End the native temporary-cache scope before querying the classified object.
    drop(cache);
    if output.point_class() != PointClassType::Unknown
        && output.point_class() != PointClassType::Region
    {
        let tolerance = crate::geom::tol::xsect_tol_3d_from_zones(
            kernel,
            options.source_zone_tolerance,
            output.obj_zone_tol_3d(kernel, world)?,
        );
        if output.gap_3d(kernel, world, None)? > tolerance {
            output.set_class_object(PointClassType::Unknown, None);
        }
    }
    Ok(())
}
