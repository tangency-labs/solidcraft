//! The blend corner's Swept turn: two constant rounds of one radius that turn at a sharp
//! straight convex side edge, square to both rounded edges on a plane, end on their own
//! sections, and the corner's face is the end section revolved about the side edge, an
//! exact torus G1 to both rounds and to the common face.
//!
//! Upstream: none (pinned SMLib has no Swept turn). The fork's
//! `source/SMLib/src/SmBlendCorner.cpp`, `source/SMLib/inc/SmBlendCorner.h`
//! (tangency-labs/smlib e647dac and 1fcbdb7, unchanged to 5e8c9c1 but for the `FIT`
//! note below; smlib#79 step 4b). The geometry, from the fork's file head: each round i
//! ends on its section through the side vertex S, the ball there centred at `C_i`; with
//! `O` the common foot of the `C_i` on the side edge and `R = |C_i - O|`, the ball turning
//! about the edge sweeps the torus `T(u, v) = O + (R + r cos v)(cos u X + sin u Y) +
//! r sin v Z`, `u` over the turn's angle, `v` over the end section's arc from S to the
//! plate vertex A; a horn torus (`R = r`) where S is on the axis.
//!
//! | Rust                       | Fork                                                  |
//! |----------------------------|-------------------------------------------------------|
//! | [`SweptTurn`]              | `SmBlendCorner`'s `m_pSideEU`, `m_pTurnSurface`, `m_vTurn3d`, `m_vTurnUV`, `m_vTurnOrients` |
//! | [`TurnRefusal`]            | `sm_TurnNotBuilt`'s noted error                        |
//! | [`classify_swept`]         | `SmBlendCorner::Classify` (the Swept part, after `GetSweptTurns()`) |
//! | [`swept_corner_edge_geom`] | `SmBlendCorner::BuildSweptSurface` and `CalcCornerEdgeGeom`'s Swept arc |
//! | [`create_swept_patch`]     | `SmBlendCorner::CreateSweptPatch`                     |
//! | `tangent_at_vertex` (private) | `sm_TangentAtVertex`                               |
//! | `meridian_angle_deg` (private) | `sm_MeridianAngleDeg`                             |
//! | `MakeSweptTopology`        | `FilletCorner::make_swept_topology` (`fillet_corner::topology`) |
//!
//! Deviations: a refusal is returned ([`TurnRefusal`]) for the executive to note
//! (`NoteError` at the vertex) instead of noted here, and so is the fork's
//! `NoteFitRadius(0, edge)` at the concave side edge (5e8c9c1, [`TurnRefusal::fit_edge`]).
//! `%g` in the refusal's text is `fmt_g`. The torus and its loop
//! are kept in [`SweptTurn`] from the edge geometry until the patch takes them. Opt-in:
//! `Kernel::fixes.fillet_blend_corner` (WR-697) with the session's swept turns
//! (`FilletSession::set_swept_turns`, the fork's `SetSweptTurns`). No recursion.

use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::{CornerPatchEnv, FilletCorner};
use crate::blend::fillet_corner::topology::{
    check_tangential_edgeuse_at_vertex, find_side_edgeuse,
};
use crate::blend::fillet_geom::p1::{FilletBrep, FilletErrorType, FilletSolverId};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSurfaceGeneratorType};
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::intersect::surface_intersector::p1::SrfIndex;
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::procedural_blend::corner::CornerDefinition;
use crate::surface::surface::Surface;
use crate::surface::torus::{Torus, TorusOptions};
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, VertexId};

const OP_CLASSIFY: &str = "blend::blend_corner::swept::classify_swept";
const OP_BUILD: &str = "blend::blend_corner::swept::swept_corner_edge_geom";
const OP_PATCH: &str = "blend::blend_corner::swept::create_swept_patch";

/// A Swept turn's own members on its corner: the side edge use (`m_pSideEU`) and, from
/// the edge geometry until the patch takes them, the torus (`m_pTurnSurface`) and its
/// loop (`m_vTurn3d`, `m_vTurnUV`, `m_vTurnOrients`).
#[derive(Debug, Clone)]
pub struct SweptTurn {
    /// `m_pSideEU`: the side edge's edge use the first round's rails meet.
    pub side_eu: EdgeuseId,
    /// `m_pTurnSurface`.
    pub surface: Option<Surface>,
    /// `m_vTurn3d`: the loop's natural iso-curves (the pole at S left out).
    pub curves_3d: Vec<Curve>,
    /// `m_vTurnUV`: their UV lines.
    pub uv_curves: Vec<Curve>,
    /// `m_vTurnOrients`: their orientations in the loop.
    pub orients: Vec<OrientType>,
}

impl SweptTurn {
    /// A Swept turn at `side_eu` with no surface yet.
    #[must_use]
    pub const fn new(side_eu: EdgeuseId) -> Self {
        Self {
            side_eu,
            surface: None,
            curves_3d: Vec::new(),
            uv_curves: Vec::new(),
            orients: Vec::new(),
        }
    }

    /// `DeleteSweptSurface`.
    fn delete_surface(&mut self) {
        self.curves_3d.clear();
        self.uv_curves.clear();
        self.orients.clear();
        self.surface = None;
    }
}

/// `sm_TurnNotBuilt`: a turn the blend corner does not build, with the error and the
/// reason the executive notes at the corner's vertex (`NoteError(vertex, SM_FIL_FAILURE,
/// error, why)`); no corner is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRefusal {
    /// `eError`: [`FilletErrorType::NoRoom`] or [`FilletErrorType::CornerUnsupported`].
    pub error: FilletErrorType,
    /// `cpWhy`.
    pub comment: String,
    /// The edge whose blend no size builds (the fork's `NoteFitRadius(0.0, edge)` before
    /// the refusal), for the executive to note: the first round's edge at a sharp concave
    /// side edge.
    pub fit_edge: Option<EdgeId>,
}

/// The executive's solvers as the Swept turn reads them.
pub trait SweptSolvers {
    /// `GetFilletSolverOfEdgeuse` (`NER` on none).
    ///
    /// # Errors
    ///
    /// When `eu` has no live solver.
    fn solver_of_edgeuse(&self, eu: EdgeuseId) -> Result<&FilletSolver>;

    /// `OffsetRadiiCanChange()` (a virtual) of `eu`'s solver.
    ///
    /// # Errors
    ///
    /// When `eu` has no live solver; the solver's failure.
    fn offset_radii_can_change(&self, eu: EdgeuseId) -> Result<bool>;

    /// The solver `id` (a fillet geometry's `GetFilletSolver()`).
    ///
    /// # Errors
    ///
    /// When `id` is not a live solver.
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver>;
}

/// `smos_Min`.
fn min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `smos_Max`.
fn max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `%g` of a finite `v`: six significant digits, trailing zeros dropped, exponent form
/// below `1e-4` and from `1e6`.
pub(crate) fn fmt_g(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format!("{v}");
    }
    let sci = format!("{v:.5e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s.to_owned()
        }
    };
    if !(-4..6).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", trim(mantissa), exp.abs());
    }
    let decimals = usize::try_from(5 - exp).unwrap_or(0);
    trim(&format!("{v:.decimals$}"))
}

/// `sm_TangentAtVertex`: the unit tangent of `e`'s curve at its end on `vertex`.
fn tangent_at_vertex(k: &Kernel, b: &Brep, e: EdgeId, vertex: VertexId) -> Result<Vec3> {
    let nul = || SolidError::null_pointer(OP_CLASSIFY);
    let curve = edge::curve(b, e)?.ok_or_else(nul)?;
    let ivl = edge::interval(b, e)?;
    let t = if edge::start_vertex(b, e)? == Some(vertex) {
        ivl.min()
    } else {
        ivl.max()
    };
    let pv = curve.evaluate(k, t, 1, true, true)?;
    pv.get(1).copied().ok_or_else(nul)?.unitize()
}

/// `sm_MeridianAngleDeg`: the angle (degrees) of `w` in the frame (`x`, `z`) of a meridian
/// plane, `atan2(w.z, w.x)`.
fn meridian_angle_deg(w: Vec3, x: Vec3, z: Vec3) -> f64 {
    rad_to_deg(w.dot(z).atan2(w.dot(x)))
}

/// The radius of `solver`'s offset surface `rail` (`GetSurface(rail)->GetOffsetDistance()`,
/// absolute; `NER` on none).
fn offset_radius(solver: &FilletSolver, rail: usize, op: &'static str) -> Result<f64> {
    let slot = solver
        .surface(rail)?
        .ok_or_else(|| SolidError::null_pointer(op))?;
    Ok(slot.surface.offset_distance().abs())
}

/// `SmBlendCorner::Classify`'s Swept part (the fork's e647dac and 1fcbdb7; reached with
/// `GetSweptTurns()` on a Turn): whether the blend corner builds a Swept turn at `vertex`,
/// two blended edge uses `solver_eus` among `all_eus`. `None` where it does; else the
/// refusal: a sharp concave side edge the ball cannot pass ([`FilletErrorType::NoRoom`],
/// naming the radius the edge must be rounded at first), and, not built yet
/// ([`FilletErrorType::CornerUnsupported`]), a corner of four edges, rounds that share no
/// face, unequal, variable or non-circular rounds, a tangent side edge, and a side edge
/// that is not a straight line square (within the fillet's tolerance over the radius) to
/// both rounded edges with a planar common face.
///
/// # Errors
///
/// [`SolidError::null_pointer`] for missing topology, solvers or surfaces the fork
/// dereferences (`NER`); those of the topology, convexity, tangency and curve queries.
#[allow(clippy::too_many_lines)] // one fork routine
pub fn classify_swept(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    tangency_tol_deg: f64,
    solvers: &dyn SweptSolvers,
) -> Result<Option<TurnRefusal>> {
    let nul = || SolidError::null_pointer(OP_CLASSIFY);
    let refuse = |error: FilletErrorType, why: &str| {
        Ok(Some(TurnRefusal {
            error,
            comment: why.to_owned(),
            fit_edge: None,
        }))
    };
    let (Some(&eu1_), Some(&eu2_)) = (solver_eus.first(), solver_eus.get(1)) else {
        return Err(nul());
    };
    // a Swept turn is built at one side edge between the blends' other faces
    if all_eus.len() != 3 {
        return refuse(
            FilletErrorType::CornerUnsupported,
            "Fillet Error: a swept turn is built where three edges meet (not yet at a corner of four)",
        );
    }
    let mut ring = None;
    for &eu in all_eus {
        if eu != eu1_ && eu != eu2_ {
            ring = Some(eu);
        }
    }
    let ring = ring.ok_or_else(nul)?;
    let side = find_side_edgeuse(b, eu1_, ring)?;
    let common = match side {
        Some(side) => face_by_two_edgeuses(b, Some(eu1_), Some(eu2_), Some(side))?,
        None => None,
    };
    let (Some(side), Some(common)) = (side, common) else {
        return refuse(
            FilletErrorType::CornerUnsupported,
            "Fillet Error: a swept turn is built where the two rounds share a face",
        );
    };

    let fs = [
        solvers.solver_of_edgeuse(eu1_)?,
        solvers.solver_of_edgeuse(eu2_)?,
    ];
    let side_edge = eu1::edge(b, side)?.ok_or_else(nul)?;
    if is_concave_edge(k, b, side_edge)? {
        let radius = offset_radius(fs[0], 0, OP_CLASSIFY)?;
        // no radius builds a swept turn past a sharp concave side edge: its size is not the
        // cause (the fork's pExec->NoteFitRadius(0.0, pEU1->GetEdge()))
        return Ok(Some(TurnRefusal {
            error: FilletErrorType::NoRoom,
            comment: format!(
                "Fillet Error: a swept turn cannot pass the sharp concave corner edge between the rounds; round that edge at a radius of at least {} first",
                fmt_g(radius)
            ),
            fit_edge: eu1::edge(b, eu1_)?,
        }));
    }

    // what the exact torus needs
    let tol = min(fs[0].approx_tol_3d, fs[1].approx_tol_3d);
    let mut radius = -1.0;
    for (f, eu) in fs.iter().zip([eu1_, eu2_]) {
        let fsg = f.fsg.as_ref().ok_or_else(nul)?;
        let r0 = offset_radius(f, 0, OP_CLASSIFY)?;
        let r1 = offset_radius(f, 1, OP_CLASSIFY)?;
        if fsg.fillet_surface_generator_type() != FilletSurfaceGeneratorType::Circular
            || solvers.offset_radii_can_change(eu)?
            || (r0 - r1).abs() > tol
            || (radius >= 0.0 && (r0 - radius).abs() > tol)
        {
            return refuse(
                FilletErrorType::CornerUnsupported,
                "Fillet Error: a swept turn is built between constant rounds of one radius (not yet between unequal or variable rounds)",
            );
        }
        radius = r0;
    }
    if check_tangential_edgeuse_at_vertex(k, b, side, vertex, tangency_tol_deg)? {
        return refuse(
            FilletErrorType::CornerUnsupported,
            "Fillet Error: a swept turn is built at a sharp side edge (not at a tangent seam)",
        );
    }
    // each round's end section (square to its edge) holds the side edge where the edge is
    // square to it: off square by an angle whose sine is s, the section's arc of radius r
    // leaves the edge's plane by up to s r, held to the fillet's tolerance
    let sin_tol = tol / radius;
    let t1 = tangent_at_vertex(k, b, eu1::edge(b, eu1_)?.ok_or_else(nul)?, vertex)?;
    let t2 = tangent_at_vertex(k, b, eu1::edge(b, eu2_)?.ok_or_else(nul)?, vertex)?;
    let common_surface = face2::surface(b, common)?.ok_or_else(nul)?;
    let square = match edge::curve(b, side_edge)? {
        None => false,
        Some(curve) => match curve.is_line(k, 10, tol)? {
            None => false,
            Some((_, axis)) => match axis.unitize() {
                Err(_) => false,
                Ok(axis) => {
                    axis.dot(t1).abs() <= sin_tol
                        && axis.dot(t2).abs() <= sin_tol
                        && common_surface.is_planar(k, tol)?.is_planar
                }
            },
        },
    };
    if !square {
        return refuse(
            FilletErrorType::CornerUnsupported,
            "Fillet Error: a swept turn is built where the side edge is straight and square to both rounded edges on a plane (not yet at a curved or drafted side edge)",
        );
    }
    Ok(None)
}

/// `SmBlendCorner::BuildSweptSurface` then `CalcCornerEdgeGeom`'s Swept arc (the fork's
/// e647dac): the torus of the Swept turn and its loop, from the processed vertices S, A,
/// B (`corner.vertices`) and the balls of the two rounds at S, kept in
/// [`FilletCorner::swept`]; then the arc A-B (`corner.edges[1]`) is a copy of the
/// torus's natural iso-curve from A to B (reversed where it runs from B to A). The
/// corner's other edges are left to their own kinds (`SmFilletCorner::CalcCornerEdgeGeom`,
/// the caller's).
///
/// # Errors
///
/// `SM_ERR` where the corner is not as the Swept topology made it or its geometry
/// does not hold (the radii, the feet, the turn, the torus's corners at S, A and B, a
/// loop of other than three curves, no iso-curve from A to B);
/// [`SolidError::null_pointer`] for missing topology or geometry the fork dereferences;
/// those of the evaluations, the convexity test and the torus.
#[allow(clippy::too_many_lines)] // one fork routine
pub fn swept_corner_edge_geom(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
    solvers: &dyn SweptSolvers,
) -> Result<()> {
    let nul = || SolidError::null_pointer(OP_BUILD);
    let err = || SolidError::sm_err(OP_BUILD);
    let swept = corner.swept.as_mut().ok_or_else(nul)?;
    swept.delete_surface();
    let side_eu = swept.side_eu;
    let &[s, a, b] = corner.vertices.as_slice() else {
        return Err(err());
    };
    for v in [s, a, b] {
        if !pseudo.fillet_vertex(v)?.is_processed() {
            return Err(err());
        }
    }
    let tol = corner.approx_tol_3d;
    let s_pnt = pseudo.vertex_point(s)?;

    // the balls at S: each round's offset surfaces at its section through S
    let mut centre = [Point3::default(); 2];
    let mut minor = -1.0;
    for (slot, plate) in centre.iter_mut().zip([a, b]) {
        let geoms = pseudo.vertex_fillet_geoms(plate)?;
        let &[g] = geoms.as_slice() else {
            return Err(err());
        };
        let vu = pseudo
            .vu_at_rail_end(s, Some(g), None, None)?
            .ok_or_else(nul)?;
        let tsect = pseudo.fillet_vertexuse(vu)?.tsect_pnt().clone();
        let solver_id = pseudo.geom(g)?.fillet_solver().ok_or_else(nul)?;
        let solver = solvers.solver(solver_id)?;
        let mut on = [Point3::default(); 2];
        for (p, side) in on.iter_mut().zip([SrfIndex::First, SrfIndex::Second]) {
            let rail = usize::from(side == SrfIndex::Second);
            let offset = pseudo
                .geom(g)?
                .offset_surface(rail, Some(solver))?
                .ok_or_else(nul)?;
            *p = offset.surface.evaluate_point(k, tsect.uv_pos(side))?;
        }
        let [on0, on1] = on;
        if on0.distance(on1) > tol {
            return Err(err()); // the section's offset points are its ball's centre
        }
        *slot = on0;
        let r = on0.distance(s_pnt);
        if minor >= 0.0 && (r - minor).abs() > tol {
            return Err(err()); // Classify took the radii to be equal
        }
        minor = r;
    }

    // the axis: the side edge's line; the torus's centre the balls' common foot on it
    let side_edge = eu1::edge(target, side_eu)?.ok_or_else(nul)?;
    let side_curve = edge::curve(target, side_edge)?.ok_or_else(nul)?;
    let (line_pt, axis) = side_curve.is_line(k, 10, tol)?.ok_or_else(err)?;
    let axis = axis.unitize()?;
    let [c0, c1] = centre;
    let foot0 = line_pt + axis * (c0 - line_pt).dot(axis);
    let foot1 = line_pt + axis * (c1 - line_pt).dot(axis);
    if foot0.distance(foot1) > tol {
        return Err(err());
    }
    let origin = foot0;
    let x = c0 - origin;
    let x2 = c1 - origin;
    let mut major = x.length();
    if major <= tol || (x2.length() - major).abs() > tol {
        return Err(err());
    }
    // S on the axis: the horn torus, its tube through the axis at S (as a revolved circle
    // that reaches the axis). The two radii, computed apart, differ in their last bits,
    // and a spindle torus's tube would cross the axis there.
    if (major - minor).abs() <= tol && (s_pnt - origin).length() <= tol {
        major = minor;
    }
    let x = x.unitize()?;
    let x2 = x2.unitize()?;
    let z = x.cross(x2);
    let sin = z.length();
    if sin <= EFF_ZERO {
        return Err(err()); // no turn
    }
    let z = z.unitize()?;
    let y = z.cross(x);
    let turn_deg = rad_to_deg(sin.atan2(x.dot(x2)));

    // the end section's arc: from S to A in the meridian (X, Z) through the first ball
    let a_pnt = pseudo.vertex_point(a)?;
    let b_pnt = pseudo.vertex_point(b)?;
    let vs = meridian_angle_deg(s_pnt - c0, x, z);
    let mut va = meridian_angle_deg(a_pnt - c0, x, z);
    while va - vs > 180.0 {
        va -= 360.0;
    }
    while va - vs < -180.0 {
        va += 360.0;
    }
    let domain = Extent2d::from_bounds(0.0, min(vs, va), turn_deg, max(vs, va));
    let mut frame = Axis2Placement::default();
    frame.set_canonical(origin, x, y)?;
    // The face's normal is its surface's: out of the material, towards the ball where the
    // rounds add material (concave), away from it where they remove it. The torus's normal
    // points out of its tube unless its NURB parameters are swapped.
    let first = corner
        .solver_eus
        .as_deref()
        .and_then(<[EdgeuseId]>::first)
        .copied()
        .ok_or_else(nul)?;
    let first_edge = eu1::edge(target, first)?.ok_or_else(nul)?;
    let adds_material = is_concave_edge(k, target, first_edge)?;
    let torus = Torus::create_canonical(
        k,
        &frame,
        major,
        minor,
        TorusOptions {
            swap_uv: adds_material,
            inside_out: false,
            circle_nurb: None,
        },
        Some(&domain),
    )?;

    // its corners are the corner's vertices: A and S at u = 0, B at u = theta
    for (uv, want) in [
        (Point2::new(0.0, va), a_pnt),
        (Point2::new(turn_deg, va), b_pnt),
        (Point2::new(0.0, vs), s_pnt),
    ] {
        if torus.evaluate_step_point(uv)?.distance(want) > tol {
            return Err(err());
        }
    }
    let mut surface = Surface::from(torus);
    if k.fixes.fillet_round_as_definition {
        keep_definition(
            k,
            target,
            corner,
            &mut surface,
            (line_pt, axis),
            centre,
            minor,
            tol,
        )?;
    }
    let natural = surface.natural_uv_domain_with_nurb(k)?;
    let trims = surface.create_natural_uv_trim_curves(k, &natural, SurfParamType::V, true)?;
    if trims.curves_3d.len() != 3 || trims.uv_curves.len() != 3 || trims.orients.len() != 3 {
        return Err(err()); // the ends and the arc; the side at S a point
    }

    // the arc A-B is the torus's (its iso-curve at A's tube angle, from A to B)
    let &[_, arc_edge, _] = corner.edges.as_slice() else {
        return Err(err());
    };
    let arc_a = edge::vertex(pseudo.brep(), arc_edge)?.ok_or_else(nul)?;
    let arc_b = edge::other_vertex(pseudo.brep(), arc_edge, Some(arc_a))?.ok_or_else(nul)?;
    let sa = pseudo.vertex_point(arc_a)?;
    let sb = pseudo.vertex_point(arc_b)?;
    let mut arc = None;
    for curve in &trims.curves_3d {
        let ivl = curve.natural_interval();
        let lo = curve.evaluate_point(k, ivl.min())?;
        let hi = curve.evaluate_point(k, ivl.max())?;
        let forward = lo.distance(sa) <= tol && hi.distance(sb) <= tol;
        let backward = lo.distance(sb) <= tol && hi.distance(sa) <= tol;
        if !forward && !backward {
            continue;
        }
        let mut copy = curve.clone();
        if backward {
            copy.reverse_parameterization(k, &ivl)?;
        }
        arc = Some(copy);
        break;
    }
    let arc = arc.ok_or_else(nul)?;
    set_edge_curve(pseudo.brep_mut(), arc_edge, Some(arc))?;

    let swept = corner.swept.as_mut().ok_or_else(nul)?;
    swept.surface = Some(surface);
    swept.curves_3d = trims.curves_3d;
    swept.uv_curves = trims.uv_curves.into_iter().map(Curve::from).collect();
    swept.orients = trims.orients;
    Ok(())
}

/// Not the fork (#2488, with `fillet_round_as_definition`): the Swept turn's torus keeps
/// its definition ([`CornerDefinition::swept`]): the rounds' common face, the side edge's
/// line, the balls' centres at the side vertex and their radius, re-certified at once
/// ([`CornerDefinition::recertify`]: the net a torus, the torus the end section's about the
/// side edge, on the common face).
///
/// # Errors
///
/// [`SolidError::null_pointer`] without the rounds' edge uses, their common face or its
/// surface; those of the definition and its re-certification.
#[allow(clippy::too_many_arguments)]
fn keep_definition(
    k: &Kernel,
    target: &Brep,
    corner: &FilletCorner,
    surface: &mut Surface,
    (axis_point, axis): (Point3, Vec3),
    centres: [Point3; 2],
    radius: f64,
    tol: f64,
) -> Result<()> {
    let nul = || SolidError::null_pointer(OP_BUILD);
    let swept = corner.swept.as_ref().ok_or_else(nul)?;
    let eus = corner.solver_eus.as_deref().ok_or_else(nul)?;
    let (Some(&eu1_), Some(&eu2_)) = (eus.first(), eus.get(1)) else {
        return Err(nul());
    };
    let common = face_by_two_edgeuses(target, Some(eu1_), Some(eu2_), Some(swept.side_eu))?
        .ok_or_else(nul)?;
    let common = face2::surface(target, common)?.ok_or_else(nul)?;
    let definition = CornerDefinition::swept(k, common, axis_point, axis, centres, radius, tol)?;
    definition.recertify(k, surface)?;
    surface.set_corner_definition(definition);
    Ok(())
}

/// `SmBlendCorner::CreateSweptPatch` (the fork's e647dac): the corner's face, the torus
/// bounded by its natural loop (`MakeFaceWithCurves` in the fillet B-rep's infinite
/// region, its natural UV domain, `SM_OT_SAME`); the surface joins `corner.surfaces` and
/// the face `corner.surface_faces` (the corner's patch face for the stitch).
///
/// # Errors
///
/// [`SolidError::null_pointer`] without the torus, the fillet B-rep or a new face
/// (`NER`); those of the face construction.
pub fn create_swept_patch(
    corner: &mut FilletCorner,
    env: &mut CornerPatchEnv<'_, '_>,
) -> Result<()> {
    let nul = || SolidError::null_pointer(OP_PATCH);
    let swept = corner.swept.as_mut().ok_or_else(nul)?;
    let surface = swept.surface.take().ok_or_else(nul)?;
    // the face takes the surface and the curves
    let curves_3d = std::mem::take(&mut swept.curves_3d);
    let uv_curves = std::mem::take(&mut swept.uv_curves);
    let orients = swept.orients.clone();
    let fillet = env.fillet.as_mut().ok_or_else(nul)?;
    let input = FaceCurvesInput {
        loops: vec![curves_3d.len()],
        curves_3d: Some(curves_3d.into_iter().map(Some).collect()),
        uv_curves: Some(uv_curves),
        orientations: orients,
        loop_points: Vec::new(),
    };
    let uv_domain = surface.natural_uv_domain_with_nurb(env.k)?;
    let region = fillet.brep.infinite_region();
    let key_of = fillet.notify.key_of;
    let face_ref = |f| key_of(EntityRef::Face(f));
    let made = make_face_with_curves(
        env.k,
        fillet.brep,
        region,
        input,
        surface.clone(),
        &uv_domain,
        OrientType::Same,
        None,
        &face_ref,
        &mut fillet.notify,
    )?;
    let face = made.face.ok_or_else(nul)?;
    // the corner's patch face (SmFilletCorner::GetPatchFaces, the stitch)
    corner.surfaces.push(surface);
    corner.surface_faces.push(face);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_g_is_printf_g() {
        assert_eq!(fmt_g(1.0), "1");
        assert_eq!(fmt_g(1.5), "1.5");
        assert_eq!(fmt_g(2.25), "2.25");
        assert_eq!(fmt_g(0.1), "0.1");
        assert_eq!(fmt_g(123_456.0), "123456");
        assert_eq!(fmt_g(1_234_567.0), "1.23457e+06");
        assert_eq!(fmt_g(0.000_012_5), "1.25e-05");
        assert_eq!(fmt_g(0.333_333_333), "0.333333");
    }

    #[test]
    fn meridian_angle_is_atan2_in_the_frame() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let z = Vec3::new(0.0, 0.0, 1.0);
        assert!((meridian_angle_deg(Vec3::new(0.0, 5.0, 1.0), x, z) - 90.0).abs() < 1e-12);
        assert!((meridian_angle_deg(Vec3::new(-1.0, 0.0, 0.0), x, z) - 180.0).abs() < 1e-12);
    }
}
