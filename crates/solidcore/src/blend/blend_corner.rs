//! The blend corner (the fork's `SmBlendCorner`): the corner events at a filleted vertex
//! with two blends of one convexity among three or four edges, a Turn (Mitred or Swept)
//! or a Join, and the exact mitre of two equal cylinders.
//!
//! Upstream: none (pinned SMLib builds these corners with `SmFilletNx2Corner`). The
//! fork's `source/SMLib/src/SmBlendCorner.cpp`, `source/SMLib/inc/SmBlendCorner.h`
//! (tangency-labs/smlib `f9b49d9`, `74e9beb`, `48a8a9f`, `32fd80a`; smlib#79 step 4).
//!
//! | Rust                         | Fork                                        |
//! |------------------------------|---------------------------------------------|
//! | [`BlendCornerEvent`]         | `SmBlendCornerEvent`                        |
//! | [`BlendTurn`]                | `SmBlendTurn`                               |
//! | [`BlendCorner`]              | `SmBlendCorner`'s `m_eEvent`, `m_eTurn`     |
//! | [`BlendCorner::is_mitre_turn`] | `SmBlendCorner::IsMitreTurn`              |
//! | [`classify`]                 | `SmBlendCorner::Classify` (the Mitre part)  |
//! | [`exact_mitre_curve`]        | `SmBlendCorner::ExactMitreCurve`            |
//! | `foot_on_surface` (private)  | `sm_FootOnSurface`                          |
//! | `bernstein_product_21` (private) | `sm_BernsteinProduct21`                 |
//! | [`swept`]                    | the Swept turn (`Classify`'s Swept part, `BuildSweptSurface`, `CreateSweptPatch`) |
//!
//! Deviations: the blend corner is not its own corner kind here. The fork's `SmBlendCorner`
//! builds the Join, the corners of four edges and the Mitre turn with the two-blend
//! construction moved unchanged from `SmFilletNx2Corner` (`74e9beb`; `48a8a9f` restructures
//! the Mitre turn of three edges bit for bit as before, per its notes), and everything that
//! keyed on `SM_FCR_N_x_2` keys on `SM_FCR_BLEND` one for one; so a blend corner is an
//! [`FilletCornerKind::NByTwo`](crate::blend::fillet_corner::p3::FilletCornerKind::NByTwo)
//! corner with [`FilletCorner::blend`] set, built by the existing Nx2 port. Opt-in:
//! `Kernel::fixes.fillet_blend_corner` (WR-697). Out-params are return values.

use crate::blend::fillet_corner::p1::{
    check_face_complimentary_corner_concavity, test_g1_continuity,
};
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_exact::{classify_exact_blend, BlendQuadric, BlendSpine, QuadricKind};
use crate::blend::fillet_geom::p1::{FilletBrep, FilletEdgeType, FilletSolverId, FilletVertexType};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p1::{FilletSolverOps, FilletSurfaceGeneratorType};
use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::global_solver::SolverOperation;
use crate::intersect::surface_intersector::p1::SrfIndex;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{edge, Brep, EdgeId, EdgeuseId, VertexId};

pub mod swept;

const OP_CLASSIFY: &str = "blend::blend_corner::classify";
const OP_EXACT: &str = "blend::blend_corner::exact_mitre_curve";

/// `SmBlendCornerEvent`: the corner events the blend corner builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendCornerEvent {
    /// `SM_BCE_NONE`: not an event of the blend corner (yet).
    #[default]
    None,
    /// `SM_BCE_TURN`: two blends turn at the unblended side edge(s) between their faces.
    Turn,
    /// `SM_BCE_JOIN`: two blends whose edges continue each other (G1) at the vertex.
    Join,
}

/// `SmBlendTurn`: a Turn's two modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendTurn {
    /// `SM_BT_MITRE`: each blend run on to their intersection (the default; G0).
    #[default]
    Mitre,
    /// `SM_BT_SWEPT`: the blends' section revolved about the side edge (G1).
    Swept,
}

/// The blend corner's own members (`m_eEvent`, `m_eTurn`) on a corner built as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlendCorner {
    /// `m_eEvent`.
    pub event: BlendCornerEvent,
    /// `m_eTurn`.
    pub turn: BlendTurn,
}

impl BlendCorner {
    /// `IsSwept`: a Turn, Swept.
    #[must_use]
    pub fn is_swept(self) -> bool {
        self.event == BlendCornerEvent::Turn && self.turn == BlendTurn::Swept
    }

    /// `IsMitreTurn`: a Turn, Mitred, at a corner of three edges (`all_eus`, the corner's
    /// `m_pAllEUs`).
    #[must_use]
    pub fn is_mitre_turn(self, all_eus: Option<&[EdgeuseId]>) -> bool {
        self.event == BlendCornerEvent::Turn
            && self.turn == BlendTurn::Mitre
            && all_eus.is_some_and(|all| all.len() == 3)
    }
}

/// `SmBlendCorner::Classify` without the Swept turn's checks
/// (`SmFilletExecutive::GetSweptTurns()` FALSE): the event at a filleted vertex with two
/// blended edge uses of one convexity among three or four (`all_eus`, the corner's ring).
/// A Join where their edges continue each other (G1 within `tangency_tol_deg`), or meet at
/// 5 degrees or less at a face corner of the other convexity; else a Turn, Mitred. Not two
/// solver edge uses among three or four: no event ([`BlendCornerEvent::None`]).
///
/// # Errors
///
/// [`SolidError::null_pointer`] without a vertex (`NER`); those of the continuity and
/// concavity tests.
pub fn classify(
    k: &Kernel,
    b: &mut Brep,
    vertex: Option<VertexId>,
    solver_eus: &[EdgeuseId],
    all_eus: &[EdgeuseId],
    tangency_tol_deg: f64,
) -> Result<BlendCorner> {
    let mut out = BlendCorner::default();
    let vertex = vertex.ok_or_else(|| SolidError::null_pointer(OP_CLASSIFY))?;
    if solver_eus.len() != 2 || (all_eus.len() != 3 && all_eus.len() != 4) {
        return Ok(out);
    }
    let (Some(&eu1_), Some(&eu2_)) = (solver_eus.first(), solver_eus.get(1)) else {
        return Err(SolidError::null_pointer(OP_CLASSIFY));
    };
    // a Join: the blends continue each other, or, at a face corner of the other
    // convexity, meet at 5 degrees or less (they blend there: no mitre so thin)
    let e1 = eu1::edge(b, eu1_)?;
    let e2 = eu1::edge(b, eu2_)?;
    let (g1, angle) = test_g1_continuity(k, b, Some(vertex), e1, e2, tangency_tol_deg)?;
    let angle_deg = angle.unwrap_or(90.0);
    if g1
        || (angle_deg <= 5.0
            && check_face_complimentary_corner_concavity(k, b, vertex, eu1_, eu2_, None)?)
    {
        out.event = BlendCornerEvent::Join;
        return Ok(out);
    }
    out.event = BlendCornerEvent::Turn;
    Ok(out)
}

/// The fillet solvers [`exact_mitre_curve`] classifies the rounds with
/// (`SmFilletGeom::GetFilletSolver()` of each round's geometry).
pub trait MitreSolvers {
    /// The derived solver of `id`.
    ///
    /// # Errors
    ///
    /// When `id` is not a live solver.
    fn ops(&mut self, id: FilletSolverId) -> Result<&mut dyn FilletSolverOps>;
}

/// `sm_FootOnSurface`: the point of `surface` nearest `point` (on it), from the guess
/// `uv`: Newton on the foot's normal equations, to rounding. `None` where it does not
/// converge in 40 steps (or the normal equations are singular).
fn foot_on_surface(
    k: &Kernel,
    surface: &Surface,
    point: Point3,
    mut uv: Point2,
) -> Result<Option<Point2>> {
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: true,
        non_zero_tangents: false,
    };
    for _ in 0..40 {
        let d = surface.evaluate(k, uv, 1, 1, flags)?;
        let at = |i: usize, j: usize| {
            d.get(i)
                .and_then(|row| row.get(j))
                .copied()
                .ok_or_else(|| SolidError::null_pointer(OP_EXACT))
        };
        let r = point - at(0, 0)?;
        let su = at(1, 0)?;
        let sv = at(0, 1)?;
        let a = su.dot(su);
        let b = su.dot(sv);
        let c = sv.dot(sv);
        let det = a * c - b * b;
        if det <= EFF_ZERO * a * c {
            return Ok(None);
        }
        let ru = su.dot(r);
        let rv = sv.dot(r);
        let du = (c * ru - b * rv) / det;
        let dv = (a * rv - b * ru) / det;
        uv.x += du;
        uv.y += dv;
        if du.abs() + dv.abs() <= 4.0 * EFF_ZERO * (1.0 + uv.x.abs() + uv.y.abs()) {
            return Ok(Some(uv));
        }
    }
    Ok(None)
}

/// `sm_BernsteinProduct21`: the product of two polynomials in Bernstein form on [0, 1], of
/// degrees 2 (`q`) and 1 (`l`): the cubic's Bernstein coefficients.
fn bernstein_product_21(q: [f64; 3], l: [f64; 2]) -> [f64; 4] {
    [
        q[0] * l[0],
        (q[0] * l[1] + 2.0 * q[1] * l[0]) / 3.0,
        (2.0 * q[1] * l[1] + q[2] * l[0]) / 3.0,
        q[2] * l[1],
    ]
}

/// The foot of `p` on the plane `plane` (its `point` and unit normal `dir`).
fn foot(p: Point3, plane: &BlendQuadric) -> Point3 {
    p - plane.dir * (p - plane.point).dot(plane.dir)
}

/// A rational B-spline of one span (`SmBSplineCurve::CreateCanonical`, end
/// multiplicities `degree + 1` on `knots`).
fn rational_span(
    dim: u64,
    degree: usize,
    points: &[Point3],
    weights: &[f64],
    form: BSplineCurveForm,
    knots: [f64; 2],
) -> Result<BSplineCurve> {
    BSplineCurve::create_canonical(&CanonicalData {
        dim,
        degree,
        control_points: points,
        form,
        knot_multiplicities: &[degree + 1, degree + 1],
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: Some(weights),
        trim: None,
    })
}

/// `SmBlendCorner::ExactMitreCurve`: the mitre `mitre` between the rails' crossing X and
/// the side vertex S of `corner` (a Mitre turn) as the rounds' exact plane section, where
/// they are two cylinders of one radius r whose axes meet (at C) and the plane they are
/// symmetric about, through C square to the difference (or sum) of their axes, holds X
/// and S.
///
/// Cylinder 1, axis a through C, is `C + r (cos t u + sin t w) + h a`, u, w square to a.
/// In the plane through C with normal m, `h = -r (cos t u.m + sin t w.m) / a.m`, so its
/// section is `C + r (cos t U + sin t W)`, `U = u - (u.m / a.m) a`, `W = w - (w.m / a.m)
/// a`: the circle's affine image, a rational quadratic arc with the circle arc's weights
/// `(1, cos(t/2), 1)` and its control points mapped. By the symmetry the plane's section of
/// cylinder 2 is the same curve. Its UV curves on the rounds are exact too (rational
/// cubics). Where any of this does not hold the edge is left as it was (the rounds'
/// certified intersection is the mitre then) and `false` is returned.
///
/// # Errors
///
/// [`SolidError::null_pointer`] for missing topology or geometry the fork dereferences
/// (`NER`); those of the evaluations, the classification, the drop and the curve
/// construction.
#[allow(clippy::too_many_lines)] // one fork routine
pub fn exact_mitre_curve(
    k: &Kernel,
    target: &Brep,
    pseudo: &mut FilletBrep,
    corner: &FilletCorner,
    mitre: EdgeId,
    solvers: &mut dyn MitreSolvers,
) -> Result<bool> {
    let nul = || SolidError::null_pointer(OP_EXACT);
    let start = edge::vertex(pseudo.brep(), mitre)?.ok_or_else(nul)?;
    let end = edge::other_vertex(pseudo.brep(), mitre, Some(start))?.ok_or_else(nul)?;
    let from_s = pseudo.fillet_vertex(start)?.vertex_type == FilletVertexType::RailXEdgeuse;
    let (s, x) = if from_s { (start, end) } else { (end, start) };
    {
        let sv = pseudo.fillet_vertex(s)?;
        let xv = pseudo.fillet_vertex(x)?;
        if sv.vertex_type != FilletVertexType::RailXEdgeuse
            || xv.vertex_type != FilletVertexType::RailXRail
            || !sv.is_processed()
            || !xv.is_processed()
        {
            return Ok(false);
        }
    }
    let prim = eu1::primary_edgeuse(pseudo.brep(), mitre)?.ok_or_else(nul)?;
    let mate = eu1::mate(pseudo.brep(), prim)?.ok_or_else(nul)?;
    let eus = [prim, mate];
    let tol = corner.approx_tol_3d;

    // both rounds constant circular ones on straight spines (cylinders), of one radius; the
    // balls' centres at X and the spines' directions
    let mut centre = [Point3::default(); 2];
    let mut axis = [Vec3::default(); 2];
    let mut radius = [0.0_f64; 2];
    let mut geoms = [None; 2];
    for ii in 0..2 {
        let eu = eus.get(ii).copied().ok_or_else(nul)?;
        let g = pseudo.fillet_edgeuse(eu)?.fillet_geom().ok_or_else(nul)?;
        if let Some(slot) = geoms.get_mut(ii) {
            *slot = Some(g);
        }
        let solver_id = pseudo.geom(g)?.fillet_solver().ok_or_else(nul)?;
        let vu = pseudo
            .vu_at_rail_end(x, Some(g), None, None)?
            .ok_or_else(nul)?;
        let tsect = pseudo.fillet_vertexuse(vu)?.tsect_pnt().clone();
        let ops = solvers.ops(solver_id)?;
        let blend = classify_exact_blend(&mut *ops, k, target, &tsect)?;
        // Open gap: a round on a circular spine (a torus) has no exact mitre here yet; the
        // mitre is then the rounds' certified SSI curve
        let circular = ops.solver().fsg.as_ref().is_some_and(|f| {
            f.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Circular
        });
        if blend.spine != BlendSpine::Line || !circular {
            return Ok(false);
        }
        let solver = ops.solver();
        let mut on = [Point3::default(); 2];
        for (kk, side) in [SrfIndex::First, SrfIndex::Second].into_iter().enumerate() {
            let offset = pseudo
                .geom(g)?
                .offset_surface(kk, Some(solver))?
                .ok_or_else(nul)?;
            let p = offset.surface.evaluate_point(k, tsect.uv_pos(side))?;
            if let Some(slot) = on.get_mut(kk) {
                *slot = p;
            }
        }
        let [on0, on1] = on;
        if on0.distance(on1) > tol {
            return Ok(false);
        }
        let spine = pseudo.geom(g)?.center_line_curve().ok_or_else(nul)?;
        let Some((_, dir)) = spine.is_line(10, tol)? else {
            return Ok(false);
        };
        let Ok(dir) = dir.unitize() else {
            return Ok(false);
        };
        if let (Some(c), Some(a), Some(r)) =
            (centre.get_mut(ii), axis.get_mut(ii), radius.get_mut(ii))
        {
            *c = on0;
            *a = dir;
            *r = blend.radius;
        }
    }
    let [geom0, geom1] = geoms;
    let geoms = [geom0.ok_or_else(nul)?, geom1.ok_or_else(nul)?];
    // Open gap: two cylinders of unequal radii meet in a quartic, not a plane conic
    let [r0, r1] = radius;
    let dr = r0;
    if (r1 - dr).abs() > tol || dr <= tol {
        return Ok(false);
    }

    // C: where the axes meet
    let [a0, a1] = axis;
    let [c0, c1] = centre;
    let d = c1 - c0;
    let ab = a0.dot(a1);
    let den = 1.0 - ab * ab;
    if den <= EFF_ZERO {
        return Ok(false); // parallel axes: no turn
    }
    let s1 = (d.dot(a0) - ab * d.dot(a1)) / den;
    let s2 = (ab * d.dot(a0) - d.dot(a1)) / den;
    let c = c0 + a0 * s1;
    let c2 = c1 + a1 * s2;
    if c.distance(c2) > tol {
        return Ok(false);
    }

    // X and S exactly: on the planes the rounds roll on, X is the foot of C on the common
    // face and S where each round's rail on its other face meets the side edge's line
    let Some(EntityRef::Edgeuse(side_eu)) =
        pseudo.fillet_vertex(s)?.point_classification().object()
    else {
        return Err(nul());
    };
    let side_radial = eu1::radial(target, side_eu)?.ok_or_else(nul)?;
    let walls = [eu1::face(target, side_eu)?, eu1::face(target, side_radial)?];
    let Some(EntityRef::Face(common)) = pseudo.fillet_vertex(x)?.point_classification().object()
    else {
        return Err(nul());
    };
    let plane_of = |f| -> Result<BlendQuadric> {
        let surface = face2::surface(target, f)?.ok_or_else(nul)?;
        BlendQuadric::from_surface(k, surface, face2::uv_domain(target, f)?.mid(), tol)
    };
    let common_plane = plane_of(common)?;
    let mut wall_planes = [BlendQuadric::default(), BlendQuadric::default()];
    for (slot, wall) in wall_planes.iter_mut().zip(walls) {
        let wall = wall.ok_or_else(nul)?;
        *slot = plane_of(wall)?;
        if slot.kind != QuadricKind::Plane {
            return Ok(false);
        }
    }
    let side_edge = eu1::edge(target, side_eu)?.ok_or_else(nul)?;
    let side_curve = edge::curve(target, side_edge)?.ok_or_else(nul)?;
    if common_plane.kind != QuadricKind::Plane {
        return Ok(false);
    }
    let Some((edge_pt, edge_dir)) = side_curve.is_line(k, 10, tol)? else {
        return Ok(false);
    };
    let Ok(edge_dir) = edge_dir.unitize() else {
        return Ok(false);
    };
    let x_pnt = foot(c, &common_plane);
    let mut on_edge = [Point3::default(); 2];
    for ii in 0..2 {
        let (Some(wall), Some(a), Some(slot)) =
            (wall_planes.get(ii), axis.get(ii), on_edge.get_mut(ii))
        else {
            return Err(nul());
        };
        // the rail on wall ii, Q + s a, meets the edge's line, P + t e
        let q = foot(c, wall);
        let qp = q - edge_pt;
        let ae = a.dot(edge_dir);
        let d2 = 1.0 - ae * ae;
        if d2 <= EFF_ZERO {
            return Ok(false);
        }
        let t = (qp.dot(edge_dir) - ae * qp.dot(*a)) / d2;
        let sp = (ae * qp.dot(edge_dir) - qp.dot(*a)) / d2;
        *slot = edge_pt + edge_dir * t;
        if slot.distance(q + *a * sp) > tol {
            return Ok(false); // the rail misses the edge
        }
    }
    let [s_pnt, s_pnt1] = on_edge;
    if s_pnt1.distance(s_pnt) > tol
        || x_pnt.distance(pseudo.vertex_point(x)?) > tol
        || s_pnt.distance(pseudo.vertex_point(s)?) > tol
    {
        return Ok(false);
    }

    // the plane of symmetry holding X and S
    let mut m = None;
    for sign in [-1.0_f64, 1.0] {
        let Ok(try_m) = (a0 + a1 * sign).unitize() else {
            continue;
        };
        if (x_pnt - c).dot(try_m).abs() <= tol && (s_pnt - c).dot(try_m).abs() <= tol {
            m = Some(try_m);
            break;
        }
    }
    let Some(m) = m else {
        return Ok(false);
    };
    let a = a0;
    let am = a.dot(m);
    if am.abs() <= EFF_ZERO {
        return Ok(false);
    }

    // the arc on cylinder 1 from X (t = 0) to S
    let x_off = x_pnt - c;
    let u = x_off - a * x_off.dot(a);
    if (u.length() - dr).abs() > tol {
        return Ok(false);
    }
    let Ok(u) = u.unitize() else {
        return Ok(false);
    };
    let w = a.cross(u);
    let s_off = s_pnt - c;
    let dt = s_off.dot(w).atan2(s_off.dot(u));
    if dt.abs() <= EFF_ZERO || dt.abs() >= 0.99 * core::f64::consts::PI {
        return Ok(false);
    }
    let um = u - a * (u.dot(m) / am);
    let wm = w - a * (w.dot(m) / am);
    let xs = [1.0, 1.0, dt.cos()];
    let ys = [0.0, (0.5 * dt).tan(), dt.sin()];
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for jj in 0..3_usize {
        let kk = if from_s { 2 - jj } else { jj }; // the curve runs as the edge does
        let (Some(&px), Some(&py)) = (xs.get(kk), ys.get(kk)) else {
            return Err(nul());
        };
        points.push(c + (um * px + wm * py) * dr);
        weights.push(if kk == 1 { (0.5 * dt).cos() } else { 1.0 });
    }
    let curve = rational_span(
        3,
        2,
        &points,
        &weights,
        BSplineCurveForm::EllipticArc,
        [0.0, 1.0],
    )?;
    let curve = Curve::from(curve);
    let curve_ivl = curve.natural_interval();

    // Its UV curves on the two rounds, exact: the rational cubic with denominator W(s) (c s
    // + d), read off the round's own parameters at s = 0, 1/2, 1, held where the round's
    // surface at it is the mitre at s = 1/4, 3/4, 1/8, 7/8 (to 1e-12 of the size)
    let ws = [
        weights.first().copied().ok_or_else(nul)?,
        weights.get(1).copied().ok_or_else(nul)?,
        weights.get(2).copied().ok_or_else(nul)?,
    ];
    let size = 1.0 + x_pnt.max_dimension() + dr;
    let mut uvs: [Option<BSplineCurve>; 2] = [None, None];
    for (ii, &g) in geoms.iter().enumerate() {
        let surface = pseudo
            .geom(g)?
            .fillet_surface(None)?
            .ok_or_else(nul)?
            .clone();
        // a start for the feet: the round's own (fitted) drop of the mitre
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let dropped = surface
            .drop_curve::<NoObject>(k, &domain, &curve, &curve_ivl, tol, false, None, true)?;
        if dropped.uv_curves.len() != 1 || dropped.max_drop_to_surf > tol {
            return Ok(false); // not on that round: its certified intersection
        }
        let drop = dropped.uv_curves.first().ok_or_else(nul)?;
        let at_s = [0.0, 0.5, 1.0, 0.25, 0.75, 0.125, 0.875];
        let mut uv_at = [Point2::default(); 7];
        let mut on = [Point3::default(); 7];
        for (kk, &s_at) in at_s.iter().enumerate() {
            let guess = drop.evaluate_point(k, drop.natural_interval().evaluate(s_at))?;
            let p = curve.evaluate_point(k, curve_ivl.evaluate(s_at))?;
            let Some(found) = foot_on_surface(k, &surface, p, Point2::new(guess.x, guess.y))?
            else {
                return Ok(false);
            };
            if let (Some(uv_slot), Some(on_slot)) = (uv_at.get_mut(kk), on.get_mut(kk)) {
                *uv_slot = found;
                *on_slot = p;
            }
        }
        // which parameter runs along the spine (N / W) and which across it (linear
        // fractional)
        let coord = |kk: usize, dir: usize| -> f64 {
            uv_at
                .get(kk)
                .map_or(0.0, |p| if dir == 0 { p.x } else { p.y })
        };
        let mut exact = None;
        for across in 0..2_usize {
            if exact.is_some() {
                break;
            }
            let p0 = coord(0, across);
            let ph = coord(1, across);
            let p1 = coord(2, across);
            if (ph - p1).abs() <= EFF_ZERO {
                continue;
            }
            let gg = (p1 + p0 - 2.0 * ph) / (ph - p1); // p = (a s + b) / (g s + 1)
            let l = [1.0, 1.0 + gg];
            let num = [p0, p1 * (1.0 + gg)];
            if l[1] <= 0.0 {
                continue;
            }
            let along = 1 - across;
            let u0 = coord(0, along);
            let uh = coord(1, along);
            let u1 = coord(2, along);
            let wh = 0.25 * (ws[0] + 2.0 * ws[1] + ws[2]);
            let n = [
                u0 * ws[0],
                0.5 * (4.0 * uh * wh - u0 * ws[0] - u1 * ws[2]),
                u1 * ws[2],
            ];
            let den = bernstein_product_21(ws, l);
            let along_c = bernstein_product_21(n, l);
            let across_c = bernstein_product_21(ws, num);
            let mut uv_points = Vec::new();
            for j in 0..4 {
                let (Some(&dj), Some(&aj), Some(&cj)) =
                    (den.get(j), along_c.get(j), across_c.get(j))
                else {
                    return Err(nul());
                };
                let da = aj / dj;
                let dc = cj / dj;
                uv_points.push(if across == 1 {
                    Point3::new(da, dc, 0.0)
                } else {
                    Point3::new(dc, da, 0.0)
                });
            }
            let try_uv = rational_span(
                2,
                3,
                &uv_points,
                &den,
                BSplineCurveForm::Unspecified,
                [curve_ivl.min(), curve_ivl.max()],
            )?;
            let mut holds = true;
            for (kk, &s_at) in at_s.iter().enumerate() {
                if !holds {
                    break;
                }
                let w_pt = try_uv.evaluate_point(try_uv.natural_interval().evaluate(s_at))?;
                let on_round = surface.evaluate_point(k, Point2::new(w_pt.x, w_pt.y))?;
                let want = on.get(kk).copied().ok_or_else(nul)?;
                holds = on_round.distance(want) <= 1e-12 * size;
            }
            if holds {
                exact = Some(try_uv);
            }
        }
        let Some(exact) = exact else {
            return Ok(false); // the round is not parameterized so: its certified intersection
        };
        if let Some(slot) = uvs.get_mut(ii) {
            *slot = Some(exact);
        }
    }

    set_edge_curve(pseudo.brep_mut(), mitre, Some(curve))?;
    pseudo.set_vertex_point(x, x_pnt)?;
    pseudo.set_vertex_point(s, s_pnt)?;
    {
        let ivl = edge::interval(target, side_edge)?;
        let dropped =
            side_curve.drop_point(k, &ivl, s_pnt, None, tol, None, SolverOperation::Minimize)?;
        if dropped.success {
            pseudo
                .fillet_vertex_mut(s)?
                .set_original_t_param(dropped.parameter);
        }
    }
    let [uv0, uv1] = uvs;
    for ((eu, g), uv) in eus.into_iter().zip(geoms).zip([uv0, uv1]) {
        pseudo.set_edgeuse_uv_curve(eu, uv.map(Curve::from), true, true)?;
        pseudo.geom_mut(g)?.add_side_fillet_edgeuse(eu);
    }
    pseudo
        .fillet_edge_mut(mitre)?
        .set_fillet_edge_type(FilletEdgeType::Precomputed);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bernstein_product_is_the_cubic() {
        // (1 + t)^2 * (2 - t): Bernstein (1, 2, 4) x (2, 1)
        let p = bernstein_product_21([1.0, 2.0, 4.0], [2.0, 1.0]);
        for t in [0.0, 0.3, 0.5, 1.0_f64] {
            let s = 1.0 - t;
            let cubic = p[0] * s * s * s
                + 3.0 * p[1] * s * s * t
                + 3.0 * p[2] * s * t * t
                + p[3] * t * t * t;
            let want = (s * s + 4.0 * s * t + 4.0 * t * t) * (2.0 * s + t);
            assert!((cubic - want).abs() < 1e-14, "{t}: {cubic} vs {want}");
        }
    }

    #[test]
    fn mitre_turn_needs_three_edges() {
        let turn = BlendCorner {
            event: BlendCornerEvent::Turn,
            turn: BlendTurn::Mitre,
        };
        let e = EdgeuseId::from_index(0);
        assert!(turn.is_mitre_turn(Some(&[e, e, e])));
        assert!(!turn.is_mitre_turn(Some(&[e, e, e, e])));
        assert!(!turn.is_mitre_turn(None));
        let join = BlendCorner {
            event: BlendCornerEvent::Join,
            ..turn
        };
        assert!(!join.is_mitre_turn(Some(&[e, e, e])));
        assert!(!join.is_swept());
    }
}
