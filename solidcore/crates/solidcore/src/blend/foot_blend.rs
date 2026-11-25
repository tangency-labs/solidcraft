//! The foot blend (#15129, jyviko's definition for weld beads): a blend set by its legs, not
//! by a rolling ball. Its leader is the joint edge `e(t)` itself, the root line. At each `t`
//! the foot on support `S_i` is the point of `S_i` in the plane square to `e'(t)` through
//! `e(t)`, at the leg's distance `z_i` from `e(t)` on that face's side: two equations per foot,
//! `(F - e(t)) . e'(t) = 0` and `|F - e(t)| = z_i`, in the foot's two surface parameters. The
//! section (the chord, or the bead's arc with its sagitta) lies between the two feet in that
//! same plane. A leg so measured is what a leg gauge measures: straight from the root to the
//! toe, at every station, on any supports.
//!
//! On a curved support the circle of radius `z_i` in the station's plane can meet the surface's
//! section there twice; the foot is the one on the branch that leaves `e(t)` along the face.
//! That direction is `b_i = sigma_i unit(n_i x e'(t))`, `n_i` the support's own normal at the
//! root and `sigma_i = +-1` the face's **side**, read once from the part where the blend is
//! made ([`FootBlend::of_edge`]) and kept in the definition, so a foot blend rebuilds from its
//! definition alone, without the part ([`FootBlendDefinition`]). Newton starts from
//! `e(t) + z_i b_i` dropped onto the support; where it ends on the other branch, finds no
//! foot or does not converge, it starts again from the previous station's foot before the
//! station is refused. Between the stations the foot is proved on that branch: a chain of
//! Krawczyk boxes over the leg, from the root to the toe (`FootBlend::prove_chain`).
//!
//! A station refused is a typed failure ([`crate::error::blend::BlendFailure`]): no toe at
//! the leg on the face's side, or an arc that would cut into a face, are refusals that name
//! the size that builds (the largest leg, the sagitta's least bound, over the stations
//! checked); a Newton that does not converge is [`BlendFailure::StationDidNotConverge`]; a
//! section face that does not hold its tolerance is [`BlendFailure::SkinPastStationCap`].
//!
//! The face's definition ([`FootBlendDefinition`], kept on its net) holds the supports, their
//! sides, the leader as its B-spline form and the face's piece of it, the legs, the sagitta,
//! the weld's spans and end shapes, the fit tolerance and the motions the net took. Its
//! rebuild is the same section face made again from it; its check compares the net with
//! that rebuild. The feet between the stations are proved ([`FootBlend::prove_feet`]: one root
//! per box for every point of each piece, Krawczyk, the stations' feet in it, joined to the
//! root on the face's branch); where the feet's series are enclosed the net is certified
//! within the fit tolerance ([`FootBlend::chord_net`], [`FootBlend::arc_net`]), elsewhere its
//! fit is **sampled** (at the midpoints between stations), not proved.
//!
//! Not upstream (solidcore's). No recursion.

use crate::blend::fillet_executive::p1::is_radius_supported;
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::weld_bead::face_holds;
use crate::blend::weld_bead::BeadEnd;
use crate::curve::curve::Curve;
use crate::error::blend::BlendFailure;
use crate::error::{Failure, Fit, FitMeasure, Result, Site, SolidError, TargetEdge};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO_RAD;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::surface::procedural_blend::bead::sagitta_bound;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::sweep::section_blend::Motion;
use crate::topology::edge;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{Brep, EdgeId, FaceId};

const OP: &str = "blend::foot_blend";
const OP_RECERTIFY: &str = "blend::foot_blend::FootBlendDefinition::recertify";

/// A failure of the foot blend that is the kernel's fault: `why` names the step and why
/// (the docs' failure-codes §6).
fn defect(why: &'static str) -> SolidError {
    SolidError::blend(OP, BlendFailure::Defect { why })
}

/// The most Newton steps a foot takes.
const NEWTON_STEPS: usize = 40;

/// The points along each leg at which a concave arc is checked against its support's section
/// ([`FootBlend::clearance_bound`]): on a section whose curvature grows fourfold from the root
/// to the foot, 16 put the bound within a thousandth of the one from 4096.
const ALONG_LEG: u32 = 16;

/// The most halvings the search for a leg that fits takes: they take a leg of 1e6 down to well
/// under the bracket's stop, `1e-7 (1 + hi)` with `hi` its upper end; past a leg of about
/// `1e-7 * 2^64` they end first.
const FIT_STEPS: usize = 64;

/// One support of a foot blend: its surface, the face it is on the part (where the blend was
/// made from one), and the face's side.
#[derive(Debug, Clone)]
pub struct FootSide {
    /// The face of the part, where the blend was made from one ([`FootBlend::of_edge`]).
    pub face: Option<FaceId>,
    /// The support's surface.
    pub surface: Surface,
    /// The face's side, `+1` or `-1`: the face leaves the root along `side (n x e')`, `n`
    /// the surface's own normal there and `e'` the leader's tangent.
    pub side: f64,
}

/// A foot blend's leader and supports: the joint edge's curve, as its B-spline form, over
/// its interval, and the edge's two faces.
#[derive(Debug, Clone)]
pub struct FootBlend {
    /// The joint edge of the part, where the blend was made from one.
    pub edge: Option<EdgeId>,
    /// The leader: the edge's curve as its B-spline form, in the edge curve's parameter.
    pub leader: BSplineCurve,
    /// The leader as a curve, for its evaluations.
    curve: Curve,
    /// The edge's interval on its curve.
    pub interval: Extent1d,
    /// The two supports, support 1 first.
    pub sides: [FootSide; 2],
    /// Whether the joint closes on itself (its start vertex is its end vertex).
    pub closed: bool,
}

/// One station of a foot blend: the root and the two feet, with each foot's surface
/// parameters and the root's on each support.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FootStation {
    /// The leader's parameter.
    pub t: f64,
    /// The root `e(t)`.
    pub root: Point3,
    /// The leader's unit tangent there, the station plane's normal.
    pub tangent: Vec3,
    /// The feet on support 1 and support 2.
    pub feet: [Point3; 2],
    /// Each foot's parameters on its support.
    pub uv: [Point2; 2],
    /// The root's parameters on each support.
    pub root_uv: [Point2; 2],
}

/// Why a foot was not found.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MissKind {
    /// No point of the face at the leg on its side (the other branch, or the circle only
    /// grazing the face's section: the equations singular).
    NoToe,
    /// Newton did not converge, from either start.
    NotConverged {
        residual: f64,
        step: f64,
        tolerance: f64,
    },
    /// A point did not drop onto the support, or its normal is not defined there: a defect.
    Stage,
}

/// A station not made: where, on which side, and why; or an arc past its bound there.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Miss {
    /// Support `side`'s foot at `t`.
    Foot { t: f64, side: usize, kind: MissKind },
    /// The arc at `t` cuts into a face: `|sagitta|` is past `bound` there.
    Cuts { t: f64, root: Point3 },
}

/// What one Newton run ended on.
enum Newton {
    Found(Point3, Point2),
    WrongSide,
    Singular,
    NotConverged {
        residual: f64,
        step: f64,
        uv: Point2,
    },
}

/// The B-spline form of an edge's curve, in the curve's own parameter: a B-spline as it is,
/// a line or a conic by its stored NURBS form.
fn leader_of(curve: &Curve) -> Result<BSplineCurve> {
    use crate::curve::curve::p2::CurveKind;
    match curve.kind() {
        CurveKind::BSpline(b) => Ok(b.clone()),
        CurveKind::Line(l) => Ok(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Ok(e.base().clone()),
        CurveKind::Parabola(p) => Ok(p.base().clone()),
        CurveKind::Hyperbola(h) => Ok(h.base.clone()),
        _ => Err(SolidError::invalid_input(
            OP,
            "a joint edge whose curve has no B-spline form",
        )),
    }
}

/// The support's unnormalised normal `S_u x S_v` at `uv`, or `None` where its derivatives are
/// missing.
fn normal_at(k: &Kernel, surface: &Surface, uv: Point2) -> Result<Option<Vec3>> {
    let d = surface.evaluate(k, uv, 1, 1, EvaluateFlags::default())?;
    Ok(
        match (
            d.get(1).and_then(|r| r.first()),
            d.first().and_then(|r| r.get(1)),
        ) {
            (Some(&du), Some(&dv)) => Some(du.cross(dv)),
            _ => None,
        },
    )
}

/// `point` dropped onto `surface` near `near`: its parameters, or `None` where the drop fails.
fn dropped(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    point: Point3,
    near: Option<Point2>,
) -> Result<Option<Point2>> {
    let d = surface.drop_point::<crate::surface::surface::p4::NoObject>(
        k,
        point,
        domain,
        &DropPointOptions {
            guess_uv: near,
            ..DropPointOptions::default()
        },
    )?;
    Ok(d.success.then_some(d.result_uv))
}

/// The binormal of edge use `eu` of `target` at `t` ([`evaluate_binormal`], in 3D only),
/// reading the part only: the evaluation builds the face's trimmed surface cache, and that
/// build attaches uv trim curves to the face's edge uses that had none, which the part's
/// file would then hold. So it runs on an exact copy of the part, which is then dropped
/// ([`Brep::with_rollback_untouched`]); the part keeps the caches it had.
///
/// # Errors
///
/// Those of the evaluation and of the copy.
fn binormal_reading(
    k: &Kernel,
    target: &mut Brep,
    eu: crate::topology::EdgeuseId,
    t: f64,
) -> Result<crate::topology::edgeuse::p2::BinormalEval> {
    // shortcut: one copy of the part per reading (twenty per edge), not one per edge; share
    // one copy per edge when beads on large parts get slow
    target.with_rollback_untouched(
        |copy| evaluate_binormal(k, copy, eu, t, true, &BinormalOptions::default()),
        |_| false,
    )
}

impl FootBlend {
    /// The foot blend of `edge` of `target`: its curve as its B-spline form, and its two faces
    /// through the edge uses on the void's side (whose faces' normals point out of the
    /// material), each with its side read from the edge use's binormal at the middle of the
    /// edge. With `first` a face of the edge, that face is support 1. `target` is only read:
    /// it is mutable for the binormals' evaluation ([`binormal_reading`]), which leaves it as
    /// it was.
    ///
    /// # Errors
    ///
    /// Native invalid input for an edge with no curve or no B-spline form of it in its own
    /// parameter, not between two faces on the void's side, a face with no surface, or a
    /// face that does not leave the edge across it; those of the evaluations.
    pub fn of_edge(
        k: &Kernel,
        target: &mut Brep,
        edge_id: EdgeId,
        first: Option<FaceId>,
    ) -> Result<Self> {
        let curve = target
            .edge(edge_id)?
            .curve
            .clone()
            .ok_or_else(|| SolidError::invalid_input(OP, "an edge with no curve"))?;
        let leader = leader_of(&curve)?;
        let interval = edge::interval(target, edge_id)?;
        let mut blend = Self::from_parts(leader, interval, [None, None], [1.0, 1.0])?;
        blend.edge = Some(edge_id);
        // the leader's form runs as the edge's curve: the same points at the same parameters
        let tol = edge::tolerance(target, edge_id)?.max(1e-9);
        for t in [interval.min(), interval.evaluate(0.5), interval.max()] {
            let on = |c: &Curve| -> Result<Point3> {
                c.evaluate(k, t, 0, false, false)?
                    .first()
                    .copied()
                    .ok_or_else(|| defect("evaluating the joint edge's curve or its B-spline leader at a parameter of the edge's own interval returned no point"))
            };
            if on(&curve)?.distance(on(&blend.curve)?) > tol {
                return Err(SolidError::invalid_input(
                    OP,
                    "a joint edge whose curve's B-spline form runs in another parameter",
                ));
            }
        }
        let void = target.infinite_region();
        let middle = interval.evaluate(0.5);
        let (_, tangent) = blend.root_at(k, middle)?;
        let mut sides: Vec<FootSide> = Vec::with_capacity(2);
        let mut uses = Vec::with_capacity(2);
        for eu in edge::edgeuses(target, edge_id, None, None)? {
            let Some(face) = eu1::face(target, eu)? else {
                continue;
            };
            let region = match eu1::shell(target, eu)? {
                Some(shell) => target.shell(shell)?.region,
                None => None,
            };
            if void.is_some() && region != void {
                continue;
            }
            if sides.iter().any(|s| s.face == Some(face)) {
                continue;
            }
            let surface = target
                .face(face)?
                .surface
                .clone()
                .ok_or_else(|| SolidError::invalid_input(OP, "a face with no surface"))?;
            let into = binormal_reading(k, target, eu, middle)?;
            let across = normal_at(k, &surface, into.uv)?
                .map(|n| n.cross(tangent))
                .ok_or_else(|| defect("the support's surface had no normal at the joint's middle, where the edge use's binormal was just evaluated on it"))?;
            let dot = across.dot(into.binormal);
            if dot.abs() <= 0.5 * across.length() * into.binormal.length() {
                return Err(SolidError::invalid_input(
                    OP,
                    "a face that does not leave the edge across it",
                ));
            }
            sides.push(FootSide {
                face: Some(face),
                surface,
                side: dot.signum(),
            });
            uses.push(eu);
        }
        // an inside corner all along: the second face rises on the first one's outer side,
        // `n1 . b2` the sine of its rise (both unit), read against the angle zero; an outside
        // or tangent stretch would bury the bead in the part or close it through the edge.
        // Read at 9 stations here, the quick refusal; between them the rise is proved to keep
        // its sign (FootBlend::prove_feet)
        if let [eu1, eu2] = uses[..] {
            for i in 1..=9 {
                let t = interval.evaluate(f64::from(i) / 10.0);
                let first = binormal_reading(k, target, eu1, t)?;
                let second = binormal_reading(k, target, eu2, t)?;
                let rise = first.face_normal.unitize()?.dot(second.binormal.unitize()?);
                if rise <= EFF_ZERO_RAD {
                    return Err(SolidError::invalid_input(
                        OP,
                        "a weld bead lies in an inside corner (a concave edge) only, not on an outside or a tangent edge",
                    ));
                }
            }
        }
        let [a, b] = <[FootSide; 2]>::try_from(sides)
            .map_err(|_| SolidError::invalid_input(OP, "an edge between two faces"))?;
        blend.sides = if first.is_some() && first == b.face {
            [b, a]
        } else {
            [a, b]
        };
        let (start, end) = edge::start_end_vertices(target, edge_id)?;
        blend.closed = start.is_some() && start == end;
        Ok(blend)
    }

    /// A foot blend from its parts, without a part: the leader, its interval, the supports'
    /// surfaces (`None` until set) and their sides. Support surfaces left `None` are planes
    /// through the origin; set them with [`FootBlend::sides`] before any station.
    ///
    /// # Errors
    ///
    /// Native invalid input for a side that is not `+1` or `-1`, or an interval not inside
    /// the leader's knots.
    fn from_parts(
        leader: BSplineCurve,
        interval: Extent1d,
        supports: [Option<Surface>; 2],
        sides: [f64; 2],
    ) -> Result<Self> {
        if !sides.iter().all(|s| *s == 1.0 || *s == -1.0) {
            return Err(SolidError::invalid_input(OP, "a side that is not +1 or -1"));
        }
        let (knots, _) = leader.knots(None)?;
        let (Some(&lo), Some(&hi)) = (knots.first(), knots.last()) else {
            return Err(SolidError::invalid_input(OP, "a leader with no knots"));
        };
        let gap = 1e-12 * (1.0 + lo.abs().max(hi.abs()));
        if !(interval.min() >= lo - gap && interval.max() <= hi + gap)
            || interval.min() >= interval.max()
        {
            return Err(SolidError::invalid_input(
                OP,
                "an interval not inside its leader",
            ));
        }
        let plane = || {
            Surface::from(crate::surface::plane::Plane::from_normal(
                Point3::ZERO,
                Vec3::new(0.0, 0.0, 1.0),
            ))
        };
        let [s1, s2] = supports;
        let [o1, o2] = sides;
        Ok(Self {
            edge: None,
            curve: Curve::from(leader.clone()),
            leader,
            interval,
            sides: [
                FootSide {
                    face: None,
                    surface: s1.unwrap_or_else(plane),
                    side: o1,
                },
                FootSide {
                    face: None,
                    surface: s2.unwrap_or_else(plane),
                    side: o2,
                },
            ],
            closed: false,
        })
    }

    /// The root `e(t)` and the leader's unit tangent there.
    fn root_at(&self, k: &Kernel, t: f64) -> Result<(Point3, Vec3)> {
        let at = self.curve.evaluate(k, t, 1, false, false)?;
        let (Some(&root), Some(&d)) = (at.first(), at.get(1)) else {
            return Err(defect("evaluating the leader at a parameter of its own interval returned no point or no tangent"));
        };
        Ok((root, d.unitize()?))
    }

    /// Support `i`'s foot at the root `root` (tangent `tangent`) at the leg `z`: from the
    /// root's direction into the face, and, where that misses, from `near`'s foot (the
    /// previous station's). The foot, its parameters and the root's, or why it was missed.
    #[allow(clippy::too_many_arguments)]
    fn foot(
        &self,
        k: &Kernel,
        i: usize,
        root: Point3,
        tangent: Vec3,
        z: f64,
        near: Option<&FootStation>,
    ) -> Result<std::result::Result<(Point3, Point2, Point2), MissKind>> {
        let Some(side) = self.sides.get(i) else {
            return Ok(Err(MissKind::Stage));
        };
        let surface = &side.surface;
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let near_root = near.and_then(|s| s.root_uv.get(i).copied());
        let near_foot = near.and_then(|s| s.uv.get(i).copied());
        let Some(root_uv) = dropped(k, surface, &domain, root, near_root)? else {
            return Ok(Err(MissKind::Stage));
        };
        let Some(n) = normal_at(k, surface, root_uv)? else {
            return Ok(Err(MissKind::Stage));
        };
        let b = n.cross(tangent) * side.side;
        let Ok(b) = (b - tangent * b.dot(tangent)).unitize() else {
            return Ok(Err(MissKind::Stage));
        };
        // to rounding: the feet are exact at their stations
        let tol = 1e-11 * (1.0 + root.max_dimension() + z);
        let mut starts: Vec<Option<Point2>> =
            vec![dropped(k, surface, &domain, root + b * z, Some(root_uv))?];
        // the retry: from the previous station's foot
        if let Some(uv) = near_foot {
            starts.push(Some(uv));
        }
        let mut miss: Option<MissKind> = None;
        for start in starts {
            let Some(uv) = start else {
                miss = miss.or(Some(MissKind::Stage));
                continue;
            };
            match newton(k, surface, root, tangent, b, z, uv, tol)? {
                Newton::Found(p, w) => return Ok(Ok((p, w, root_uv))),
                Newton::WrongSide | Newton::Singular => miss = Some(MissKind::NoToe),
                // stopped on the edge of the support's domain: the support ends within the leg
                Newton::NotConverged { uv, .. }
                    if on_rim(&domain, periods_of(k, surface, &domain)?, uv) =>
                {
                    miss = Some(MissKind::NoToe);
                }
                Newton::NotConverged { residual, step, .. } => {
                    if !matches!(miss, Some(MissKind::NoToe)) {
                        miss = Some(MissKind::NotConverged {
                            residual,
                            step,
                            tolerance: tol,
                        });
                    }
                }
            }
        }
        Ok(Err(miss.unwrap_or(MissKind::Stage)))
    }

    /// The station at the leader's parameter `t` with the legs `legs` (support 1's, support
    /// 2's), each foot from the root's direction into its face, and from `near`'s feet where
    /// that misses; or the miss.
    fn try_station(
        &self,
        k: &Kernel,
        t: f64,
        legs: [f64; 2],
        near: Option<&FootStation>,
    ) -> Result<std::result::Result<FootStation, Miss>> {
        let (root, tangent) = self.root_at(k, t)?;
        let mut feet = [Point3::ZERO; 2];
        let mut uv = [Point2::new(0.0, 0.0); 2];
        let mut root_uv = [Point2::new(0.0, 0.0); 2];
        for (i, &z) in legs.iter().enumerate() {
            match self.foot(k, i, root, tangent, z, near)? {
                Ok((p, w, r)) => {
                    if let (Some(f), Some(u), Some(q)) =
                        (feet.get_mut(i), uv.get_mut(i), root_uv.get_mut(i))
                    {
                        (*f, *u, *q) = (p, w, r);
                    }
                }
                Err(kind) => return Ok(Err(Miss::Foot { t, side: i, kind })),
            }
        }
        Ok(Ok(FootStation {
            t,
            root,
            tangent,
            feet,
            uv,
            root_uv,
        }))
    }

    /// The station at the leader's parameter `t` with the legs `legs` (support 1's, support
    /// 2's): each foot on its support, in the plane square to the leader through the root, at
    /// its leg's distance from the root, on the branch that leaves the root along the face
    /// (from the previous station `near`'s feet where the root's direction misses).
    ///
    /// # Errors
    ///
    /// Native invalid input for a leg that is not finite and positive; the typed failures of
    /// a foot not found (no toe at the leg, with no fit: [`FootBlend::section_face`] names
    /// one; a Newton that does not converge; a defect); those of the evaluations.
    pub fn station(
        &self,
        k: &Kernel,
        t: f64,
        legs: [f64; 2],
        near: Option<&FootStation>,
    ) -> Result<FootStation> {
        check_legs(legs)?;
        match self.try_station(k, t, legs, near)? {
            Ok(s) => Ok(s),
            Err(miss) => Err(self.miss_error(miss, None).into_error()),
        }
    }

    /// The failure of `miss`, with `fit` (the size that builds) where one was found.
    fn miss_error(&self, miss: Miss, fit: Option<f64>) -> Failure {
        let edge = self.edge.map(TargetEdge::of_target);
        let site = |at: Option<Point3>| Site {
            edge,
            at,
            ..Site::default()
        };
        match miss {
            Miss::Foot { t, kind, side } => {
                let at = self.root_at_or_none(t);
                match kind {
                    MissKind::NoToe => {
                        let face = self
                            .sides
                            .get(side)
                            .and_then(|s| s.face)
                            .map(crate::error::TargetFace::of_target);
                        let f = Failure::new(
                            OP,
                            BlendFailure::NoRoom(
                                crate::blend::fillet_executive::NoRoomReason::NoToeAtLeg,
                            ),
                        )
                        .or_at(Site { face, ..site(at) });
                        match fit {
                            Some(size) => f.with_fit(Fit::new(size, FitMeasure::Leg, edge)),
                            None => f,
                        }
                    }
                    MissKind::NotConverged {
                        residual,
                        step,
                        tolerance,
                    } => Failure::new(
                        OP,
                        BlendFailure::StationDidNotConverge {
                            s: t,
                            residual: Some(residual),
                            tolerance,
                            step: Some(step),
                            iterations: u32::try_from(NEWTON_STEPS).ok(),
                        },
                    )
                    .or_at(site(at)),
                    MissKind::Stage => Failure::new(OP, BlendFailure::Defect { why: "placing a foot found no root of the leader on its support, no normal or binormal there, or no start for the foot's solve, though the leader runs on both supports" }).or_at(site(at)),
                }
            }
            Miss::Cuts { root, .. } => {
                let f = Failure::new(
                    OP,
                    BlendFailure::NoRoom(
                        crate::blend::fillet_executive::NoRoomReason::BeadCutsFace,
                    ),
                )
                .or_at(site(Some(root)));
                match fit {
                    Some(size) => f.with_fit(Fit::new(size, FitMeasure::Sagitta, edge)),
                    None => f,
                }
            }
        }
    }

    /// The root at `t`, where the leader evaluates there.
    fn root_at_or_none(&self, t: f64) -> Option<Point3> {
        self.leader.evaluate_point(t).ok()
    }

    /// The largest leg on support `i`, up to `z`, whose foot is found at every one of the
    /// parameters `ts` (in order, each from the one before): `z` where it is, else halved down
    /// from it until the bracket is `1e-7 (1 + hi)` wide, `hi` its upper end (`FIT_STEPS`). 0
    /// where no leg is found at all.
    ///
    /// # Errors
    ///
    /// Those of the evaluations.
    fn leg_fit(&self, k: &Kernel, i: usize, ts: &[f64], z: f64) -> Result<f64> {
        let holds = |leg: f64| -> Result<bool> {
            let mut near: Option<FootStation> = None;
            for &t in ts {
                let (root, tangent) = self.root_at(k, t)?;
                match self.foot(k, i, root, tangent, leg, near.as_ref())? {
                    Ok((p, w, r)) => {
                        let mut s = near.unwrap_or(FootStation {
                            t,
                            root,
                            tangent,
                            feet: [p; 2],
                            uv: [w; 2],
                            root_uv: [r; 2],
                        });
                        if let (Some(f), Some(u), Some(q)) =
                            (s.feet.get_mut(i), s.uv.get_mut(i), s.root_uv.get_mut(i))
                        {
                            (*f, *u, *q) = (p, w, r);
                        }
                        near = Some(s);
                    }
                    Err(_) => return Ok(false),
                }
            }
            Ok(true)
        };
        if holds(z)? {
            return Ok(z);
        }
        let (mut lo, mut hi) = (0.0_f64, z);
        for _ in 0..FIT_STEPS {
            if hi - lo <= 1e-7 * (1.0 + hi) {
                break;
            }
            let mid = 0.5 * (lo + hi);
            if holds(mid)? {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    /// The least sagitta bound (concave or convex as `sagitta`'s sign) over the stations at
    /// `ts` with the legs `legs`, each station from the one before; stations not made are
    /// skipped. Infinite where none is made.
    ///
    /// # Errors
    ///
    /// Those of the evaluations.
    fn sagitta_fit(&self, k: &Kernel, ts: &[f64], legs: [f64; 2], sagitta: f64) -> Result<f64> {
        let mut least = f64::INFINITY;
        let mut near: Option<FootStation> = None;
        for &t in ts {
            if let Ok(s) = self.try_station(k, t, legs, near.as_ref())? {
                least = least.min(self.station_bound(k, &s, sagitta)?);
                near = Some(s);
            }
        }
        Ok(least)
    }

    /// The open bound on `|sagitta|` at the station `s`: `(c / 2) tan(min(beta_i) / 2)`
    /// concave, `(c / 2) tan((pi - max(beta_i)) / 2)` convex
    /// ([`crate::surface::procedural_blend::bead::sagitta_bound`]), `c` the chord and `beta_i`
    /// the angle at foot `i` between the chord and the face's direction back towards the root
    /// in the station's plane. Concave, no more than [`FootBlend::clearance_bound`] either:
    /// the angle at the foot only says that the arc leaves the foot off the face, and a
    /// support that curves towards the bead more tightly near its foot than the arc does
    /// comes back into the arc between the foot and the root.
    ///
    /// # Errors
    ///
    /// Those of the evaluations; a defect where a face's normal is not defined at its foot.
    fn station_bound(&self, k: &Kernel, s: &FootStation, sagitta: f64) -> Result<f64> {
        let [a, b] = s.feet;
        let mut toes = [0.0_f64; 2];
        for (i, (toe, other)) in toes.iter_mut().zip([b, a]).enumerate() {
            let (Some(side), Some(&uv), Some(&foot)) =
                (self.sides.get(i), s.uv.get(i), s.feet.get(i))
            else {
                return Err(defect("a station lacked the side, the foot or the foot's parameters of one of its two supports, though every station is built with both"));
            };
            let n = normal_at(k, &side.surface, uv)?.ok_or_else(|| {
                defect("the support's surface had no normal at a foot just solved on it")
            })?;
            let mut d = n.cross(s.tangent);
            if d.dot(s.root - foot) < 0.0 {
                d *= -1.0;
            }
            let c = other - foot;
            *toe = c.cross(d).length().atan2(c.dot(d));
        }
        let bound = sagitta_bound(b.distance(a), toes, sagitta);
        if sagitta < 0.0 {
            return Ok(bound.min(self.clearance_bound(k, s)?));
        }
        Ok(bound)
    }

    /// The largest concave sagitta at the station `s` whose arc stays off the supports between
    /// the root and the feet, sampled: the least sagitta of the arcs through both feet and a
    /// point of a support's section, over the root and `ALONG_LEG - 1` points of each section
    /// at even distances from the root up to the foot (each a foot at a shorter leg; one not
    /// found is left out). The supports' sections lie on the root's side of the chord, and an
    /// arc deeper than the one through a point of them has that point inside its circle. As
    /// the point nears a foot the arc through it nears the one tangent to the face there,
    /// [`FootBlend::station_bound`]'s. Infinite where the root lies on the chord's line.
    ///
    /// # Errors
    ///
    /// Those of the evaluations.
    fn clearance_bound(&self, k: &Kernel, s: &FootStation) -> Result<f64> {
        let [a, b] = s.feet;
        let chord = b - a;
        let c = chord.length();
        let Ok(along) = chord.unitize() else {
            return Ok(f64::INFINITY);
        };
        let middle = a + chord * 0.5;
        // towards the root, square to the chord
        let to_root = s.root - middle;
        let Ok(up) = (to_root - along * to_root.dot(along)).unitize() else {
            return Ok(f64::INFINITY);
        };
        let through = |p: Point3| {
            let q = p - middle;
            let (x, y) = (q.dot(along), q.dot(up));
            if y <= 0.0 {
                // across the chord from the root: between the feet even the chord cuts it
                return if x.abs() < 0.5 * c {
                    0.0
                } else {
                    f64::INFINITY
                };
            }
            // the circle through the feet and p: its centre `depth` beyond the chord
            let depth = (0.25 * c * c - x * x - y * y) / (2.0 * y);
            (0.25 * c * c + depth * depth).sqrt() - depth
        };
        let mut least = through(s.root);
        for (i, foot) in s.feet.iter().enumerate() {
            let leg = foot.distance(s.root);
            for j in 1..ALONG_LEG {
                let z = leg * f64::from(j) / f64::from(ALONG_LEG);
                if let Ok((p, _, _)) = self.foot(k, i, s.root, s.tangent, z, Some(s))? {
                    least = least.min(through(p));
                }
            }
        }
        Ok(least)
    }
}

impl FootBlend {
    /// The stations a weld's sizes are checked and searched at: on the leader's knots in the
    /// interval, at least 4 between each two and 32 in all. `within` the spans' leader
    /// parameters, ascending (none: the whole interval): only the stations inside them, and
    /// their ends, as the bead covers no more.
    ///
    /// # Errors
    ///
    /// Those of the knots.
    fn check_stations(&self, within: &[[f64; 2]]) -> Result<Vec<f64>> {
        let breaks = self.breaks()?;
        // at least 32 stations, and 4 between each two knots
        let per = 32_usize
            .div_ceil(breaks.len().saturating_sub(1).max(1))
            .max(4);
        let mut ts = Vec::new();
        for pair in breaks.windows(2) {
            let &[lo, hi] = pair else { continue };
            ts.extend((0..per).map(|i| lo + (hi - lo) * i as f64 / per as f64));
        }
        ts.extend(breaks.last().copied());
        if !within.is_empty() {
            ts.extend(within.iter().flatten().copied());
            ts.retain(|t| within.iter().any(|&[a, b]| (a..=b).contains(t)));
            ts.sort_by(f64::total_cmp);
            ts.dedup();
        }
        Ok(ts)
    }

    /// `refused`, a refusal of this joint's bead with the legs `legs` and the sagitta
    /// `sagitta`, with its size searched over the whole weld: the piece that failed (one
    /// segment of a closed joint, one span of a stitch) names the size that builds over its
    /// own stations, and another piece may take a smaller one. For no toe at the leg, the
    /// largest leg on that side, up to the size named, whose foot is found at every one of
    /// [`FootBlend::check_stations`] of `within` (the weld's spans as leader parameters; none:
    /// the whole interval); for an arc that cuts into a face, the least bound over them, where
    /// it is below the size named. A toe off its face is searched over those stations as it is
    /// ([`FootBlend::toes_on_faces`]). The sizes are those of the stations (sampled), and a
    /// leg's is its own side's: the other side's refusal comes once this one's size is asked.
    /// Any other error is returned as it is.
    ///
    /// # Errors
    ///
    /// Those of the evaluations of the search.
    pub fn fit_over(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        sagitta: f64,
        within: &[[f64; 2]],
        refused: SolidError,
    ) -> Result<SolidError> {
        use crate::blend::fillet_executive::NoRoomReason;
        let no_toe = matches!(
            refused.blend_failure(),
            Some(BlendFailure::NoRoom(NoRoomReason::NoToeAtLeg))
        );
        let mut failure = match refused {
            SolidError::Failed(failure) => failure,
            other => return Ok(other),
        };
        let Some(fit) = failure.fit else {
            return Ok(SolidError::Failed(failure));
        };
        let size = match fit.measure {
            FitMeasure::Leg if no_toe => {
                let face = failure.site.face;
                let side = self.sides.iter().position(|s| {
                    face.is_some() && s.face.map(crate::error::TargetFace::of_target) == face
                });
                match side {
                    Some(i) if fit.size > 0.0 => {
                        let ts = self.check_stations(within)?;
                        self.leg_fit(k, i, &ts, fit.size)?
                    }
                    _ => fit.size,
                }
            }
            FitMeasure::Sagitta => fit
                .size
                .min(self.sagitta_that_fits(k, legs, sagitta, within)?),
            _ => fit.size,
        };
        failure.fit = Some(Fit::new(size, fit.measure, fit.edge));
        Ok(SolidError::Failed(failure))
    }

    /// The largest sagitta (a magnitude, on `sagitta`'s side) this joint's bead takes with the
    /// legs `legs` over [`FootBlend::check_stations`] of `within`: a hair below the least bound
    /// over them, as the bound is open. Infinite where no station is made.
    ///
    /// # Errors
    ///
    /// Those of the evaluations.
    pub fn sagitta_that_fits(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        sagitta: f64,
        within: &[[f64; 2]],
    ) -> Result<f64> {
        let ts = self.check_stations(within)?;
        let bound = self.sagitta_fit(k, &ts, legs, sagitta)?;
        Ok((bound * (1.0 - 1e-9)).max(0.0))
    }

    /// Checks that each toe lies on its face of `target` at the legs `legs`: at
    /// [`FootBlend::check_stations`] of `within` (sampled), each foot's parameters
    /// classified on its face (trims included). Stations not made are skipped (the build
    /// refuses them). `target` is only read.
    ///
    /// # Errors
    ///
    /// The refusal [`crate::blend::fillet_executive::NoRoomReason::LegPastFace`] at the first
    /// toe found off its face, naming the face and the edge, with the largest leg on that side
    /// whose toes stay on the face at those stations (halved down from the leg until the bracket
    /// is `1e-7 (1 + hi)` wide, `hi` its upper end; 0 where that leg is below the smallest the
    /// body builds, [`is_radius_supported`]: eight times its tolerance, as no leg on that side
    /// builds then; the other side is not looked at once one is off: its refusal comes once
    /// this size is asked); those of the classification.
    pub fn toes_on_faces(
        &self,
        k: &Kernel,
        target: &Brep,
        legs: [f64; 2],
        within: &[[f64; 2]],
    ) -> Result<()> {
        check_legs(legs)?;
        // shortcut: a tapered or rounded end is checked at the span's full legs, so one whose
        // smaller legs would fit near a face's end is refused; check the end's own legs when
        // such a refusal comes up
        let ts = self.check_stations(within)?;
        // each support's domain and periods: a foot past a periodic support's seam is moved
        // into the domain by whole periods before it is classified
        let mut frames = Vec::new();
        for side in &self.sides {
            let domain = side.surface.natural_uv_domain_with_nurb(k)?;
            let periods = periods_of(k, &side.surface, &domain)?;
            frames.push((domain, periods));
        }
        let on_face = |target: &Brep, i: usize, face: FaceId, uv: Point2| -> Result<bool> {
            let (Some((domain, periods)), Some(side)) = (frames.get(i), self.sides.get(i)) else {
                return Ok(true);
            };
            let uv = wrapped(domain, *periods, uv);
            face_holds(k, target, face, &side.surface, domain, *periods, uv)
        };
        // the stations, each from the one before; and the first toe off its face
        let mut off: Option<(usize, Point3)> = None;
        let mut near: Option<FootStation> = None;
        'stations: for &t in &ts {
            let Ok(s) = self.try_station(k, t, legs, near.as_ref())? else {
                continue;
            };
            for (i, side) in self.sides.iter().enumerate() {
                let (Some(face), Some(&uv), Some(&foot)) = (side.face, s.uv.get(i), s.feet.get(i))
                else {
                    continue;
                };
                if !on_face(target, i, face, uv)? {
                    off = Some((i, foot));
                    break 'stations;
                }
            }
            near = Some(s);
        }
        let Some((i, foot)) = off else {
            return Ok(());
        };
        let face = self.sides.get(i).and_then(|s| s.face);
        let z = legs.get(i).copied().unwrap_or(0.0);
        // the largest leg on that side whose toes stay on the face, halved down from the leg
        let holds = |leg: f64| -> Result<bool> {
            let Some(face) = face else {
                return Ok(true);
            };
            let mut near: Option<FootStation> = None;
            for &t in &ts {
                let (root, tangent) = self.root_at(k, t)?;
                let Ok((p, uv, r)) = self.foot(k, i, root, tangent, leg, near.as_ref())? else {
                    return Ok(false);
                };
                if !on_face(target, i, face, uv)? {
                    return Ok(false);
                }
                let mut s = near.unwrap_or(FootStation {
                    t,
                    root,
                    tangent,
                    feet: [p; 2],
                    uv: [uv; 2],
                    root_uv: [r; 2],
                });
                if let (Some(f), Some(u), Some(q)) =
                    (s.feet.get_mut(i), s.uv.get_mut(i), s.root_uv.get_mut(i))
                {
                    (*f, *u, *q) = (p, uv, r);
                }
                near = Some(s);
            }
            Ok(true)
        };
        // of the leg found, not of the leg asked: a face far shorter than the leg asked still
        // gets its fit
        let (mut lo, mut hi) = (0.0_f64, z);
        for _ in 0..FIT_STEPS {
            if hi - lo <= 1e-7 * (1.0 + hi) {
                break;
            }
            let mid = 0.5 * (lo + hi);
            if holds(mid)? {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        // a leg below the body's resolution is refused before the toes are read, so a retry at
        // it would not build: no leg fits
        if !is_radius_supported(lo, target.tolerance()) {
            lo = 0.0;
        }
        let edge = self.edge.map(TargetEdge::of_target);
        Err(Failure::new(
            OP,
            BlendFailure::NoRoom(crate::blend::fillet_executive::NoRoomReason::LegPastFace),
        )
        .or_at(Site {
            edge,
            face: face.map(crate::error::TargetFace::of_target),
            at: Some(foot),
            ..Site::default()
        })
        .with_fit(Fit::new(lo, FitMeasure::Leg, edge))
        .into_error())
    }
}

/// Whether `uv` lies on the rim of `domain` (to a millionth of its size): where a Newton the
/// surface's evaluation holds inside its domain stops when the point it seeks is past it.
fn on_rim(domain: &Extent2d, periods: [Option<f64>; 2], uv: Point2) -> bool {
    let (lo, hi) = (domain.min(), domain.max());
    let rim = |x: f64, lo: f64, hi: f64, period: Option<f64>| {
        // a periodic direction has no rim: its seam is not where the face ends
        if period.is_some() {
            return false;
        }
        let d = 1e-6 * (hi - lo);
        (x - lo).abs() <= d || (x - hi).abs() <= d || x < lo || x > hi
    };
    let [pu, pv] = periods;
    rim(uv.x, lo.x, hi.x, pu) || rim(uv.y, lo.y, hi.y, pv)
}

/// The period of `surface` in u and in v over its natural domain `domain`, where it is
/// periodic.
///
/// # Errors
///
/// Those of the periodicity test.
fn periods_of(k: &Kernel, surface: &Surface, domain: &Extent2d) -> Result<[Option<f64>; 2]> {
    use crate::nurbs::b_spline_surface::SurfParamType;
    let (lo, hi) = (domain.min(), domain.max());
    Ok([
        surface
            .is_periodic(k, domain, SurfParamType::U)?
            .then_some(hi.x - lo.x),
        surface
            .is_periodic(k, domain, SurfParamType::V)?
            .then_some(hi.y - lo.y),
    ])
}

/// `uv` with each periodic direction's parameter moved by whole periods into `domain`.
fn wrapped(domain: &Extent2d, periods: [Option<f64>; 2], uv: Point2) -> Point2 {
    let (lo, _) = (domain.min(), domain.max());
    let wrap = |x: f64, from: f64, period: Option<f64>| match period {
        Some(p) if p > 0.0 && p.is_finite() => from + (x - from).rem_euclid(p),
        _ => x,
    };
    let [pu, pv] = periods;
    Point2::new(wrap(uv.x, lo.x, pu), wrap(uv.y, lo.y, pv))
}

/// Legs finite and positive.
fn check_legs(legs: [f64; 2]) -> Result<()> {
    if legs.iter().all(|z| z.is_finite() && *z > 0.0) {
        Ok(())
    } else {
        Err(SolidError::invalid_input(OP, "a leg that is not positive"))
    }
}

/// The foot on `surface` in the plane through `root` square to the unit `tangent`, at
/// distance `z` from `root`, on the branch along the unit direction `b` (square to `tangent`,
/// into the face): Newton on `F1 = (P - root) . tangent`, `F2 = |P - root|^2 - z^2` in the
/// surface's parameters, from `uv`.
///
/// # Errors
///
/// Those of the evaluations.
#[allow(clippy::too_many_arguments)]
fn newton(
    k: &Kernel,
    surface: &Surface,
    root: Point3,
    tangent: Vec3,
    b: Vec3,
    z: f64,
    uv: Point2,
    tol: f64,
) -> Result<Newton> {
    let mut uv = uv;
    let mut residual = f64::INFINITY;
    let mut step = f64::INFINITY;
    for _ in 0..NEWTON_STEPS {
        let d = surface.evaluate(k, uv, 1, 1, EvaluateFlags::default())?;
        let (Some(&p), Some(&du), Some(&dv)) = (
            d.first().and_then(|r| r.first()),
            d.get(1).and_then(|r| r.first()),
            d.first().and_then(|r| r.get(1)),
        ) else {
            return Ok(Newton::Singular);
        };
        let w = p - root;
        let f1 = w.dot(tangent);
        let f2 = w.dot(w) - z * z;
        residual = f1.abs().max(f2.abs() / z.max(f64::MIN_POSITIVE));
        if f1.abs() <= tol && f2.abs() <= tol * z {
            if w.dot(b) <= 0.0 {
                return Ok(Newton::WrongSide);
            }
            return Ok(Newton::Found(p, uv));
        }
        let (a11, a12) = (du.dot(tangent), dv.dot(tangent));
        let (a21, a22) = (2.0 * w.dot(du), 2.0 * w.dot(dv));
        let det = a11 * a22 - a12 * a21;
        if det.abs() <= f64::EPSILON * (a11.abs() + a12.abs()) * (a21.abs() + a22.abs()) {
            return Ok(Newton::Singular);
        }
        let su = (f1 * a22 - f2 * a12) / det;
        let sv = (a11 * f2 - a21 * f1) / det;
        step = su.hypot(sv);
        uv = Point2::new(uv.x - su, uv.y - sv);
    }
    Ok(Newton::NotConverged { residual, step, uv })
}

/// The fit tolerance a weld bead's section face is built to: 0.01 mm, far below what a weld
/// can be made or measured to. It is checked at samples (the midpoints between stations,
/// [`FootBlend::section_face`]), not proved: a sampled check, not a bound, until the face's
/// certificate follows. The feet themselves are solved to rounding at their stations
/// (Newton converges quadratically, so that costs a step or two); this tolerance sets how
/// many stations the face between them needs, which is where the time goes.
pub const WELD_FIT_TOLERANCE: f64 = 0.01;

/// The most times [`FootBlend::section_face`] doubles its stations.
const REFINEMENTS: usize = 9;

impl FootBlend {
    /// The section face of the foot blend with the legs `legs` and the bead's `sagitta`
    /// (`0`: the chord, `> 0` convex, `< 0` concave): at each station the chord between the
    /// two feet, or the circular arc between them at the sagitta in the station's plane
    /// (bulging away from the root for a positive sagitta), and through those sections a
    /// rational net, `u` the leader's parameter over the edge's interval, `v` from foot 1 (0)
    /// to foot 2 (1). The arcs are interpolated in homogeneous form (cubic in `u`) at stations
    /// on each of the leader's knots (at least four), doubled between them until, at the
    /// midpoint between each two stations, the net holds the feet, the section's middle and
    /// the root within `tol`. Where its feet's series are enclosed the net is instead the
    /// cubic Hermite net through the section's rows and their rates, **certified** within
    /// `tol` of the true section ([`FootBlend::chord_net`], [`FootBlend::arc_net`]; the root
    /// still sampled); elsewhere it is exact at its stations and between them **sampled** at
    /// one point per interval, not bounded.
    ///
    /// # Errors
    ///
    /// Native invalid input for a leg that is not positive; the refusals of a station, typed,
    /// with the size that builds over the stations of the refinement refused: a foot with no
    /// toe at its leg on its face's side (the largest leg on that side that finds its toe), an
    /// arc that would cut into a face (the sagitta's least bound); a Newton that does not
    /// converge ([`BlendFailure::StationDidNotConverge`]); a net that does not hold within
    /// `tol` after its refinements ([`BlendFailure::SkinPastStationCap`]); feet not proved
    /// between its stations ([`FootBlend::prove_feet`]); those of the arcs and the
    /// interpolation.
    pub fn section_face(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
    ) -> Result<Surface> {
        let (lo, hi) = (self.interval.min(), self.interval.max());
        Ok(Surface::from(
            self.segment(k, legs, sagitta, tol, lo, hi)?.face,
        ))
    }

    /// The proof of the feet ([`FootBlend::prove_feet`]) between the stations of
    /// [`Self::section_face`] with the same arguments.
    ///
    /// # Errors
    ///
    /// Those of [`Self::section_face`].
    pub fn prove_section(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
    ) -> Result<FeetProof> {
        let (lo, hi) = (self.interval.min(), self.interval.max());
        Ok(self.segment(k, legs, sagitta, tol, lo, hi)?.proof)
    }

    /// The proved distance of [`Self::section_face`]'s net (with the same arguments) from the
    /// true section: `Some` where the net is certified ([`FootBlend::chord_net`],
    /// [`FootBlend::arc_net`]: its feet's series enclosed), `None` where it is sampled.
    ///
    /// # Errors
    ///
    /// Those of [`Self::section_face`].
    pub fn section_bound(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
    ) -> Result<Option<f64>> {
        let (lo, hi) = (self.interval.min(), self.interval.max());
        Ok(self.segment(k, legs, sagitta, tol, lo, hi)?.bound)
    }

    /// [`Self::section_face`] over the leader's parameters `a..b` inside its interval, with
    /// its sides (the feet's curves) and the root's curve through the same stations.
    ///
    /// # Errors
    ///
    /// Those of [`Self::section_face`].
    #[allow(clippy::too_many_arguments)]
    fn segment(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
        a: f64,
        b: f64,
    ) -> Result<Segment> {
        check_legs(legs)?;
        let mut breaks = vec![a];
        breaks.extend(
            self.breaks()?
                .into_iter()
                .filter(|&x| x > a + 1e-9 * (b - a) && x < b - 1e-9 * (b - a)),
        );
        breaks.push(b);
        // at least four stations, the cubic's least
        let spans = breaks.len().saturating_sub(1).max(1);
        let mut count = 3_usize.div_ceil(spans).max(1);
        let mut worst = f64::INFINITY;
        let mut made = 0;
        for _ in 0..REFINEMENTS {
            // each span between the leader's knots split evenly: a station on every knot, where
            // the leader may turn less smoothly
            let mut stations = Vec::new();
            for pair in breaks.windows(2) {
                let &[lo, hi] = pair else { continue };
                stations.extend((0..count).map(|i| lo + (hi - lo) * i as f64 / count as f64));
            }
            if let Some(&last) = breaks.last() {
                stations.push(last);
            }
            made = stations.len();
            let mut rows = Vec::new();
            let mut roots = Vec::new();
            let mut solved: Vec<FootStation> = Vec::new();
            for &t in &stations {
                match self.section_rows(k, t, legs, sagitta, solved.last())? {
                    Ok((row, s)) => {
                        rows.push(row);
                        roots.push(s.root);
                        solved.push(s);
                    }
                    Err(miss) => return Err(self.refusal(k, miss, &stations, legs, sagitta)?),
                }
            }
            // the net certified where its feet's series are enclosed; else the net through the
            // rows, sampled
            let certified = if sagitta == 0.0 {
                self.chord_net(k, legs, &solved, tol)?
            } else {
                self.arc_net(k, legs, &solved, sagitta, tol)?
            };
            let (face, feet, bound) = match certified {
                Chord::Net(face, feet, bound) => (face, feet, Some(bound)),
                Chord::Over(bound) => {
                    worst = bound;
                    count *= 2;
                    continue;
                }
                Chord::NotEnclosed => {
                    let (face, feet) = net_through_rows(&stations, &rows, sagitta)?;
                    (face, feet, None)
                }
            };
            let root = through_at(&roots, &stations)?;
            // sampled, not proved: at the midpoint between each two stations, the root, and
            // where the net is not certified the feet and the section's middle
            worst = bound.unwrap_or(0.0);
            for (j, pair) in stations.windows(2).enumerate() {
                let &[lo, hi] = pair else { continue };
                let t = 0.5 * (lo + hi);
                let (row, s) = match self.section_rows(k, t, legs, sagitta, solved.get(j))? {
                    Ok(made) => made,
                    Err(miss) => {
                        return Err(self.refusal(k, miss, &stations, legs, sagitta)?);
                    }
                };
                let (Some(&first), Some(&last)) = (row.0.first(), row.0.last()) else {
                    continue;
                };
                let middle = section_middle(&row.0, sagitta);
                if bound.is_none() {
                    for (v, want) in [(0.0, first), (0.5, middle), (1.0, last)] {
                        let got = face.evaluate_point(Point2::new(t, v))?;
                        worst = worst.max(got.distance(want));
                    }
                }
                worst = worst.max(root.evaluate_point(t)?.distance(s.root));
            }
            if worst <= tol {
                let proof = self.prove_feet(k, legs, &solved)?;
                let start_uv = solved
                    .first()
                    .map_or([Point2::new(0.0, 0.0); 2], |s| s.root_uv);
                return Ok(Segment {
                    face,
                    feet,
                    root,
                    start_uv,
                    proof,
                    bound,
                });
            }
            count *= 2;
        }
        Err(SolidError::blend(
            OP,
            BlendFailure::SkinPastStationCap {
                asked: tol,
                reached: worst,
                stations: made,
                rounds: REFINEMENTS,
            },
        ))
    }

    /// The refusal of `miss`, met among the stations at `ts` with the legs `legs` and the
    /// sagitta `sagitta`, with the size that builds over those stations where it is a size's.
    ///
    /// # Errors
    ///
    /// Those of the evaluations of the fit's search.
    fn refusal(
        &self,
        k: &Kernel,
        miss: Miss,
        ts: &[f64],
        legs: [f64; 2],
        sagitta: f64,
    ) -> Result<SolidError> {
        let fit = match miss {
            Miss::Foot {
                t,
                side,
                kind: MissKind::NoToe,
            } => {
                let mut along: Vec<f64> = ts.to_vec();
                along.push(t);
                along.sort_by(f64::total_cmp);
                along.dedup();
                let z = legs.get(side).copied().unwrap_or(0.0);
                Some(self.leg_fit(k, side, &along, z)?)
            }
            Miss::Cuts { t, .. } => {
                let mut along: Vec<f64> = ts.to_vec();
                along.push(t);
                along.sort_by(f64::total_cmp);
                along.dedup();
                let bound = self.sagitta_fit(k, &along, legs, sagitta)?;
                // the bound is open: a sagitta a hair below it builds
                bound.is_finite().then(|| (bound * (1.0 - 1e-9)).max(0.0))
            }
            Miss::Foot { .. } => None,
        };
        Ok(self.miss_error(miss, fit).into_error())
    }

    /// The leader's interval split at its curve's knots inside it, ends included, ascending.
    ///
    /// # Errors
    ///
    /// Those of the knots.
    fn breaks(&self) -> Result<Vec<f64>> {
        let (lo, hi) = (self.interval.min(), self.interval.max());
        let mut breaks = vec![lo];
        let (knots, _) = self.leader.knots(None)?;
        let gap = 1e-9 * (hi - lo);
        breaks.extend(knots.into_iter().filter(|&x| x > lo + gap && x < hi - gap));
        breaks.push(hi);
        Ok(breaks)
    }

    /// One station's section as control rows across, and the station: the chord's two feet
    /// (weights 1), or the arc's five rows of [`BeadArc::rows`] with their weights; or the
    /// miss: a foot not found, or an arc that would cut into a face there (its sagitta past
    /// the station's bound, [`FootBlend::station_bound`]).
    ///
    /// [`BeadArc::rows`]: crate::surface::procedural_blend::bead::BeadArc::rows
    fn section_rows(
        &self,
        k: &Kernel,
        t: f64,
        legs: [f64; 2],
        sagitta: f64,
        near: Option<&FootStation>,
    ) -> Result<std::result::Result<(SectionRow, FootStation), Miss>> {
        use crate::surface::procedural_blend::bead::BeadArc;
        let s = match self.try_station(k, t, legs, near)? {
            Ok(s) => s,
            Err(miss) => return Ok(Err(miss)),
        };
        let [a, b] = s.feet;
        if sagitta == 0.0 {
            return Ok(Ok(((vec![a, b], vec![1.0, 1.0]), s)));
        }
        if sagitta.abs() >= self.station_bound(k, &s, sagitta)? {
            return Ok(Err(Miss::Cuts { t, root: s.root }));
        }
        let convex = (a + (b - a) * 0.5) - s.root;
        let arc = BeadArc::new(a, b, s.tangent, convex, sagitta)?;
        let (row, w) = arc.rows();
        Ok(Ok(((row.to_vec(), w.to_vec()), s)))
    }
}

/// One station's section across: its control points and their weights.
type SectionRow = (Vec<Point3>, Vec<f64>);

/// One segment of a foot blend over `a..b`: its section face's net, the net's two sides (the
/// feet's curves, foot 1's and foot 2's) and the root's curve, all through the same stations.
struct Segment {
    face: BSplineSurface,
    feet: [BSplineCurve; 2],
    root: BSplineCurve,
    /// The root's parameters on each support at the segment's start.
    start_uv: [Point2; 2],
    /// The feet's proof between its stations.
    proof: FeetProof,
    /// The section face's proved distance from the true section ([`FootBlend::chord_net`]),
    /// where it is certified.
    bound: Option<f64>,
}

/// The cubic through `points` at the parameters `stations`.
///
/// # Errors
///
/// Those of the interpolation.
fn through_at(points: &[Point3], stations: &[f64]) -> Result<BSplineCurve> {
    use crate::nurbs::crv_fit::p1::ParamMethod;
    BSplineCurve::create_interpolating_curve_with_params(
        ParamMethod::ChordLength,
        3,
        3,
        points,
        &[],
        None,
        false,
        Some(stations),
    )
}

/// The middle across of a section row: the chord's midpoint, or the arc's apex (its middle
/// control point).
fn section_middle(row: &[Point3], sagitta: f64) -> Point3 {
    if sagitta == 0.0 {
        match (row.first(), row.last()) {
            (Some(&a), Some(&b)) => a + (b - a) * 0.5,
            _ => Point3::ZERO,
        }
    } else {
        row.get(2).copied().unwrap_or(Point3::ZERO)
    }
}

/// The rational net through the section rows at `stations`: each row's control points in
/// homogeneous form interpolated cubic along `u` at the stations, the weights too, then
/// divided back.
fn net_through_rows(
    stations: &[f64],
    rows: &[SectionRow],
    sagitta: f64,
) -> Result<(BSplineSurface, [BSplineCurve; 2])> {
    use crate::nurbs::b_spline_curve::KnotType;
    use crate::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData};
    let across = if sagitta == 0.0 { 2 } else { 5 };
    let mut columns: Vec<Vec<Point3>> = vec![Vec::new(); across];
    let mut weights: Vec<Vec<Point3>> = vec![Vec::new(); across];
    for (points, w) in rows {
        for (j, (column, wcol)) in columns.iter_mut().zip(weights.iter_mut()).enumerate() {
            let (Some(&p), Some(&w)) = (points.get(j), w.get(j)) else {
                return Err(defect("a section row had fewer control points or weights than the net has columns, though every row is built with the same count"));
            };
            column.push(Point3::new(p.x * w, p.y * w, p.z * w));
            wcol.push(Point3::new(w, 0.0, 0.0));
        }
    }
    let through = |points: &[Point3]| through_at(points, stations);
    let mut curves = Vec::new();
    let mut wcurves = Vec::new();
    for (column, wcol) in columns.iter().zip(&weights) {
        curves.push(through(column)?);
        wcurves.push(through(wcol)?);
    }
    let first = curves
        .first()
        .ok_or_else(|| defect("the net had no column curves, though every row has its two feet"))?;
    // the net's sides v = 0 and v = 1: the feet's columns, of weight 1
    let sides = [
        first.clone(),
        curves
            .last()
            .ok_or_else(|| {
                defect("the net had no column curves, though every row has its two feet")
            })?
            .clone(),
    ];
    let (u_knots, u_mults) = first.knots(None)?;
    let count = first.control_points()?.len();
    let mut points = Vec::new();
    let mut net_weights = Vec::new();
    for i in 0..count {
        for (c, wc) in curves.iter().zip(&wcurves) {
            let p = c.control_points()?.get(i).copied().ok_or_else(|| defect("a column curve had fewer control points than the rows it was interpolated through"))?;
            let w = wc
                .control_points()?
                .get(i)
                .map(|c| c.x)
                .filter(|w| *w > 0.0)
                .ok_or_else(|| defect("a column's weight curve had fewer control points than the rows, or a weight not above zero, though it interpolates positive weights"))?;
            points.push(Point3::new(p.x / w, p.y / w, p.z / w));
            net_weights.push(w);
        }
    }
    let (v_degree, v_mults, v_knots): (usize, &[usize], &[f64]) = if sagitta == 0.0 {
        (1, &[2, 2], &[0.0, 1.0])
    } else {
        (2, &[3, 2, 3], &[0.0, 0.5, 1.0])
    };
    let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 3,
        v_degree,
        control_points: &points,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &u_mults,
        v_multiplicities: v_mults,
        u_knots: &u_knots,
        v_knots,
        knot_type: KnotType::Unspecified,
        weights: Some(&net_weights),
        uv_domain: None,
    })?;
    Ok((net, sides))
}

/// How many segments a closed joint's bead is built in: each face then runs less than a full
/// turn round it, so no face needs a seam.
const CLOSED_SEGMENTS: usize = 4;

/// The first count of points along a rung, root excluded, at even shares of each leg.
const RUNG_POINTS: usize = 4;

/// The most points of a rung past the root: their count is doubled from [`RUNG_POINTS`]
/// until the rung holds its support to the fit tolerance.
const RUNG_POINTS_CAP: usize = 64;

/// A section face [`FootBlend::add_span`] added: the face and the leader's parameters it runs
/// over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionFace {
    /// The face, in the body it was added to.
    pub face: FaceId,
    /// The leader's parameters it runs over.
    pub interval: [f64; 2],
}

impl FootBlend {
    /// The weld bead's body (#15129) over the whole joint: the section face, a strip on each
    /// support between the root and its foot, and, where the joint edge is open, a planar end
    /// cap at each end in the station's plane, stitched into a solid of its own, each section
    /// face keeping its definition ([`FootBlendDefinition`]). A closed joint (a branch round a
    /// tube) is built in four (`CLOSED_SEGMENTS`) segments, each face less than a turn. No part is
    /// read or changed.
    ///
    /// # Errors
    ///
    /// Those of [`Self::add_span`] and of the closing ([`close_bead_body`]).
    pub fn body(&self, k: &mut Kernel, legs: [f64; 2], sagitta: f64, tol: f64) -> Result<Brep> {
        let mut body = Brep::new_with_infinite_region()?;
        body.set_editing_enabled(true);
        let (lo, hi) = (self.interval.min(), self.interval.max());
        let made = match self.add_span(k, &mut body, legs, sagitta, tol, [lo, hi]) {
            Ok(made) => made,
            // a refusal's size over all of the joint, not the failed segment's alone
            Err(refused) => return Err(self.fit_over(k, legs, sagitta, &[], refused)?),
        };
        close_bead_body(k, &mut body, tol)?;
        let weld = BeadWeld {
            legs,
            sagitta,
            spans: vec![[lo, hi]],
            ends: [BeadEnd::Square; 2],
            tol,
        };
        self.define_faces(&mut body, &made, &weld)?;
        Ok(body)
    }

    /// The faces of one span `span` of the leader (inside its interval) added to `body`
    /// unstitched (several beads and spans go into one body before it is closed with
    /// [`close_bead_body`]): the section face and the two strips per segment, and square end
    /// caps in the stations' planes at the span's ends, unless the span is the whole of a
    /// closed joint (built in four (`CLOSED_SEGMENTS`) segments, no caps). The section faces added,
    /// with their pieces of the leader.
    ///
    /// # Errors
    ///
    /// Native invalid input for a span not inside the interval; those of
    /// [`Self::section_face`] and of the curves and faces.
    pub fn add_span(
        &self,
        k: &mut Kernel,
        body: &mut Brep,
        legs: [f64; 2],
        sagitta: f64,
        tol: f64,
        span: [f64; 2],
    ) -> Result<Vec<SectionFace>> {
        use crate::blend::fillet_cliff_overflow::{face_of_loop, LoopCurve};
        use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
        use crate::surface::plane::Plane;

        let [lo, hi] = span;
        let (a, b) = (self.interval.min(), self.interval.max());
        let gap = 1e-12 * (1.0 + a.abs().max(b.abs()));
        if !(lo >= a - gap && hi <= b + gap && lo < hi) {
            return Err(SolidError::invalid_input(
                OP,
                "a span not inside its joint edge",
            ));
        }
        let whole = (lo - a).abs() <= gap && (hi - b).abs() <= gap;
        let ring = self.closed && whole;
        let pieces = if ring { CLOSED_SEGMENTS } else { 1 };
        let cuts: Vec<f64> = (0..=pieces)
            .map(|i| lo + (hi - lo) * i as f64 / pieces as f64)
            .collect();
        let same = |curve: &BSplineCurve| LoopCurve {
            curve: Curve::from(curve.clone()),
            orient: OrientType::Same,
        };
        let opposite = |curve: &BSplineCurve| LoopCurve {
            curve: Curve::from(curve.clone()),
            orient: OrientType::Opposite,
        };
        // the cuts' rungs (one per support) and sections, shared by the faces either side
        let mut rungs: Vec<[BSplineCurve; 2]> = Vec::new();
        let mut sections: Vec<BSplineCurve> = Vec::new();
        for &t in &cuts {
            let (r1, r2, section) = self.cut_curves(k, t, legs, sagitta)?;
            rungs.push([r1, r2]);
            sections.push(section);
        }
        let mut made = Vec::new();
        for (j, pair) in cuts.windows(2).enumerate() {
            let &[a, b] = pair else { continue };
            let seg = self.segment(k, legs, sagitta, tol, a, b)?;
            let (Some(ra), Some(rb), Some(sa), Some(sb)) = (
                rungs.get(j),
                rungs.get(j + 1),
                sections.get(j),
                sections.get(j + 1),
            ) else {
                return Err(defect(
                    "a cut of the span had no rung or no section, though one is built at every cut",
                ));
            };
            let [f1, f2] = &seg.feet;
            // the section face: foot 1 along, the far section, foot 2 back, the near section
            let face = face_of_loop(
                k,
                body,
                Surface::from(seg.face.clone()),
                vec![same(f1), same(sb), opposite(f2), opposite(sa)],
            )?;
            made.push(SectionFace {
                face,
                interval: [a, b],
            });
            // the strips: the root along, the far rung out, the foot back, the near rung in
            for (i, (side, foot)) in self.sides.iter().zip([f1, f2]).enumerate() {
                let (Some(near), Some(far), Some(&start)) =
                    (ra.get(i), rb.get(i), seg.start_uv.get(i))
                else {
                    continue;
                };
                strip_face(
                    k,
                    body,
                    side.surface.clone(),
                    vec![same(&seg.root), same(far), opposite(foot), opposite(near)],
                    start,
                )?;
            }
        }
        // the square end caps of an open span, in the stations' planes
        if !ring {
            for (end, t) in [(0_usize, lo), (cuts.len() - 1, hi)] {
                let (Some([r1, r2]), Some(section)) = (rungs.get(end), sections.get(end)) else {
                    continue;
                };
                let (root, tangent) = self.root_at(k, t)?;
                let plane = Surface::from(Plane::from_normal(root, tangent));
                face_of_loop(k, body, plane, vec![same(r1), same(section), opposite(r2)])?;
            }
        }
        Ok(made)
    }

    /// Gives each of the section faces `made` of `body` its definition: this blend with the
    /// weld `weld`, over the face's piece of the leader.
    ///
    /// # Errors
    ///
    /// A defect for a face with no surface; those of the face lookups.
    pub fn define_faces(
        &self,
        body: &mut Brep,
        made: &[SectionFace],
        weld: &BeadWeld,
    ) -> Result<()> {
        let [s1, s2] = &self.sides;
        for m in made {
            let definition = FootBlendDefinition {
                supports: [s1.surface.clone(), s2.surface.clone()],
                sides: [s1.side, s2.side],
                leader: self.leader.clone(),
                interval: m.interval,
                legs: weld.legs,
                sagitta: weld.sagitta,
                spans: weld.spans.clone(),
                ends: weld.ends,
                tol: weld.tol,
                motions: Vec::new(),
            };
            body.face_mut(m.face)?
                .surface
                .as_mut()
                .ok_or_else(|| {
                    defect("a bead face just made on its foot-blend surface had no surface")
                })?
                .set_foot_blend_definition(definition);
        }
        Ok(())
    }

    /// At the leader's parameter `t`: each support's rung (its section by the station's plane
    /// from the root to its foot, through points at shares of the leg, exact in that plane)
    /// and the section between the feet (the chord, or the arc as a rational quadratic with
    /// [`BeadArc::rows`]' points and weights).
    ///
    /// [`BeadArc::rows`]: crate::surface::procedural_blend::bead::BeadArc::rows
    ///
    /// # Errors
    ///
    /// The refusals of [`Self::station`] and of the section's bound; those of the curves.
    fn cut_curves(
        &self,
        k: &Kernel,
        t: f64,
        legs: [f64; 2],
        sagitta: f64,
    ) -> Result<(BSplineCurve, BSplineCurve, BSplineCurve)> {
        use crate::nurbs::b_spline_curve::{BSplineCurveForm, CanonicalData, KnotType};
        let ((row, weights), full) = match self.section_rows(k, t, legs, sagitta, None)? {
            Ok(made) => made,
            Err(miss) => return Err(self.refusal(k, miss, &[t], legs, sagitta)?),
        };
        let rung = |points: &[Point3]| {
            BSplineCurve::create_interpolating_curve_with_params(
                crate::nurbs::crv_fit::p1::ParamMethod::ChordLength,
                3,
                3,
                points,
                &[],
                None,
                false,
                None,
            )
        };
        // the rungs through feet at shares of the leg, exact at those points; between them
        // the cubic is checked against its support and the shares doubled until it holds
        let mut count = RUNG_POINTS;
        let (r1, r2) = loop {
            let mut rung_points: [Vec<Point3>; 2] = [vec![full.root], vec![full.root]];
            for i in 1..=count {
                let share = i as f64 / count as f64;
                let scaled = legs.map(|z| z * share);
                let s = self.station(k, t, scaled, Some(&full))?;
                for (points, foot) in rung_points.iter_mut().zip(s.feet) {
                    points.push(foot);
                }
            }
            let [p1, p2] = &rung_points;
            let (r1, r2) = (rung(p1)?, rung(p2)?);
            let mut worst = 0.0_f64;
            for (r, side) in [&r1, &r2].into_iter().zip(&self.sides) {
                worst = worst.max(rung_off_support(k, r, &side.surface, 2 * count)?);
            }
            if worst <= WELD_FIT_TOLERANCE {
                break (r1, r2);
            }
            if count >= RUNG_POINTS_CAP {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::SkinPastStationCap {
                        asked: WELD_FIT_TOLERANCE,
                        reached: worst,
                        stations: count,
                        rounds: 0,
                    },
                ));
            }
            count *= 2;
        };
        let (degree, mults, knots): (usize, &[usize], &[f64]) = if sagitta == 0.0 {
            (1, &[2, 2], &[0.0, 1.0])
        } else {
            (2, &[3, 2, 3], &[0.0, 0.5, 1.0])
        };
        let section = BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree,
            control_points: &row,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: mults,
            knots,
            knot_type: KnotType::Unspecified,
            weights: if sagitta == 0.0 { None } else { Some(&weights) },
            trim: None,
        })?;
        Ok((r1, r2, section))
    }
}

/// The largest distance from `rung` to `surface` at `samples` points spread evenly over the
/// rung's parameters, ends excluded (they are the root and the foot, on the support).
///
/// # Errors
///
/// A defect where a sample does not drop onto the support; those of the evaluations.
fn rung_off_support(
    k: &Kernel,
    rung: &BSplineCurve,
    surface: &Surface,
    samples: usize,
) -> Result<f64> {
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let ivl = rung.natural_interval();
    let mut worst = 0.0_f64;
    let mut near = None;
    for i in 1..samples {
        let p = rung.evaluate_point(ivl.evaluate(i as f64 / samples as f64))?;
        let uv = dropped(k, surface, &domain, p, near)?
            .ok_or_else(|| defect("dropping a point of a rung onto its support found no parameter, though the rung's ends are feet on that support"))?;
        let on = surface
            .evaluate(k, uv, 0, 0, EvaluateFlags::default())?
            .first()
            .and_then(|r| r.first())
            .copied()
            .ok_or_else(|| defect("evaluating the support's surface at a parameter of its domain returned no point"))?;
        worst = worst.max(p.distance(on));
        near = Some(uv);
    }
    Ok(worst)
}

/// A strip's face on its support `surface` (a face of the part's, continued): the loop `lp`
/// as [`crate::blend::fillet_cliff_overflow::face_of_loop`] makes it, but over the strip's own
/// parameter domain. The loop's samples are dropped each near the one before, from `start`
/// (the root's parameters at the loop's start), with the jumps of a periodic support's
/// period taken out, so a strip whose edge lies on the support's seam (a joint's vertex on a
/// tube's seam) has its parameters on one side of it: the face's domain is the samples' box
/// in a periodic direction (the natural domain otherwise, or where the box spans a period).
///
/// # Errors
///
/// A defect where a sample does not drop onto the support; those of the samples and the face
/// construction.
fn strip_face(
    k: &mut Kernel,
    body: &mut Brep,
    surface: Surface,
    mut lp: Vec<crate::blend::fillet_cliff_overflow::LoopCurve>,
    start: Point2,
) -> Result<FaceId> {
    use crate::error::EntityRef;
    use crate::nurbs::b_spline_surface::SurfParamType;
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use crate::topology::brep::p3::with_kernel_callbacks;
    use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
    let natural = surface.natural_uv_domain_with_nurb(k)?;
    let (lo, hi) = (natural.min(), natural.max());
    let periods = [
        surface
            .is_periodic(k, &natural, SurfParamType::U)?
            .then_some(hi.x - lo.x),
        surface
            .is_periodic(k, &natural, SurfParamType::V)?
            .then_some(hi.y - lo.y),
    ];
    let [pu, pv] = periods;
    let wrap = |x: f64, from: f64, period: Option<f64>| match period {
        Some(p) if p > 0.0 => from + (x - from).rem_euclid(p),
        _ => x,
    };
    let unwrap = |x: f64, near: f64, period: Option<f64>| match period {
        Some(p) if p > 0.0 => x + p * ((near - x) / p).round(),
        _ => x,
    };
    let mut uvs: Vec<Point2> = Vec::new();
    let mut prev = start;
    for c in &lp {
        let ivl = c.curve.natural_interval();
        let mut along = Vec::with_capacity(9);
        for i in 0..=8 {
            along.push(
                c.curve
                    .evaluate_point(k, ivl.evaluate(f64::from(i) / 8.0))?,
            );
        }
        if c.orient == OrientType::Opposite {
            along.reverse();
        }
        let count = along.len().saturating_sub(1);
        for &p in along.iter().take(count) {
            let guess = Point2::new(wrap(prev.x, lo.x, pu), wrap(prev.y, lo.y, pv));
            let Some(uv) = dropped(k, &surface, &natural, p, Some(guess))? else {
                return Err(defect("dropping a point of a bead face's boundary curve onto that face's surface found no parameter, though the curve was built on it"));
            };
            let uv = Point2::new(unwrap(uv.x, prev.x, pu), unwrap(uv.y, prev.y, pv));
            uvs.push(uv);
            prev = uv;
        }
    }
    let mut area = 0.0;
    for (i, a) in uvs.iter().enumerate() {
        let Some(b) = (i + 1).checked_rem(uvs.len()).and_then(|n| uvs.get(n)) else {
            return Err(defect(
                "the face's parameter polygon had no next point inside its own walk",
            ));
        };
        area += a.x * b.y - b.x * a.y;
    }
    if area < 0.0 {
        lp.reverse();
        for c in &mut lp {
            c.orient = if c.orient == OrientType::Same {
                OrientType::Opposite
            } else {
                OrientType::Same
            };
        }
    }
    // the strip's own domain in a periodic direction: its samples' box moved by whole periods
    // into the natural domain, a little wider, inside it; the natural domain where the box
    // does not fit in it (a strip across the seam)
    let span = |get: fn(&Point2) -> f64, period: Option<f64>, (nlo, nhi): (f64, f64)| {
        let (mut a, mut b) = (f64::INFINITY, f64::NEG_INFINITY);
        for uv in &uvs {
            a = a.min(get(uv));
            b = b.max(get(uv));
        }
        match period {
            Some(p) if p > 0.0 && b - a < p => {
                let shift = -p * ((a - nlo) / p + 1e-9).floor();
                let (a, b) = (a + shift, b + shift);
                let slack = 1e-9 * p;
                if a < nlo - slack || b > nhi + slack {
                    return (nlo, nhi);
                }
                let m = 0.05 * (p - (b - a));
                ((a - m).max(nlo), (b + m).min(nhi))
            }
            _ => (nlo, nhi),
        }
    };
    let (u0, u1) = span(|uv| uv.x, pu, (lo.x, hi.x));
    let (v0, v1) = span(|uv| uv.y, pv, (lo.y, hi.y));
    let domain = Extent2d::from_bounds(u0, v0, u1, v1);
    let input = FaceCurvesInput {
        loops: vec![lp.len()],
        curves_3d: Some(lp.iter().map(|c| Some(c.curve.clone())).collect()),
        uv_curves: None,
        orientations: lp.iter().map(|c| c.orient).collect(),
        loop_points: Vec::new(),
    };
    let made = with_kernel_callbacks(k, body, |k, b, env| {
        let region_id = b.infinite_region();
        let key_of = env.key_of;
        let face_ref = |f: FaceId| key_of(EntityRef::Face(f));
        make_face_with_curves(
            k,
            b,
            region_id,
            input,
            surface,
            &domain,
            OrientType::Same,
            None,
            &face_ref,
            env,
        )
    })?;
    made.face
        .ok_or_else(|| defect("making a bead face from its boundary curves returned no face, though the call succeeded"))
}

/// Closes the weld bead faces added to `body` ([`FootBlend::add_span`]): stitched within
/// `tol` (or the body's own tolerance, if larger) into closed shells, oriented as a solid, and
/// its geometry named by its own topology, a body of its own.
///
/// # Errors
///
/// A defect ([`BlendFailure::Defect`]) where the faces do not close into a manifold
/// solid; those of the stitching and the orientation.
pub fn close_bead_body(k: &mut Kernel, body: &mut Brep, tol: f64) -> Result<()> {
    use crate::topology::brep::p3::{orient_trimmed_surfaces, with_kernel_callbacks};
    use crate::topology::topology_traverser::Marks;
    let tolerance = body.tolerance().max(tol);
    let stitched = crate::topology::brep::p4::stitch_faces(k, body, tolerance, None, false)?;
    let mut open = stitched.lamina_edges != 0;
    if !open {
        let oriented = with_kernel_callbacks(k, body, |k, b, env| {
            let mut marks = Marks::default();
            orient_trimmed_surfaces(k, b, env, &mut marks, true, false, None)
        })?;
        open = oriented.maybe_not_closed_solid;
    }
    if open || !crate::topology::brep::p2::is_manifold_solid(body)? {
        // the faces are the bead's own, made to meet: their not closing is the kernel's fault
        return Err(defect(
            "the bead's own faces, made to meet, did not close into a manifold solid",
        ));
    }
    // The section face is fitted to `tol` at samples, so its edges carry about `tol` (a little
    // past it between the samples); a body's edge must stay below 1000 times the body's
    // tolerance (check_brep, test 59). The body's tolerance is raised to a thousandth of its
    // widest edge's, with 1 % room (and every smaller face, edge and vertex tolerance with
    // it), so each of its edges is an edge of a valid body.
    let mut widest = tolerance;
    for &e in body.edge_list() {
        widest = widest.max(body.edge(e)?.tolerance);
    }
    crate::topology::brep::p2::set_tolerance(k, body, widest * 1.01 / 1000.0, true, true)?;
    body.use_arena_naming()
}

/// What a weld asks of its foot blend beside the supports and leader: the legs, the sagitta,
/// the spans (pieces of the leader's parameter, ascending, apart) with the end shape at the
/// start and the end of each, and the fit tolerance.
#[derive(Debug, Clone, PartialEq)]
pub struct BeadWeld {
    /// The legs on support 1 and support 2.
    pub legs: [f64; 2],
    /// The arc's height over the chord (`> 0` convex, `< 0` concave, `0` the chord).
    pub sagitta: f64,
    /// The spans, as the leader's parameters.
    pub spans: Vec<[f64; 2]>,
    /// The end shape at each span's start and end.
    pub ends: [BeadEnd; 2],
    /// The fit tolerance ([`WELD_FIT_TOLERANCE`]).
    pub tol: f64,
}

/// A foot blend's section face's definition (#15129), kept on its net: the supports by value
/// and their sides, the leader (the joint edge's curve as its B-spline form) and the face's
/// piece of it, the legs, the sagitta, the weld's spans and end shapes, the fit tolerance,
/// and the motions the net took since it was made, in order. The net is
/// [`FootBlend::section_face`] of the piece made from these, then each motion. No part is
/// read: the sides stand in for the faces' directions off the edge.
#[derive(Debug, Clone)]
pub struct FootBlendDefinition {
    supports: [Surface; 2],
    sides: [f64; 2],
    leader: BSplineCurve,
    interval: [f64; 2],
    legs: [f64; 2],
    sagitta: f64,
    spans: Vec<[f64; 2]>,
    ends: [BeadEnd; 2],
    tol: f64,
    motions: Vec<Motion>,
}

/// The parts of a [`FootBlendDefinition`], as a record holds them.
#[derive(Debug, Clone)]
pub struct FootBlendParts {
    /// The supports S1, S2.
    pub supports: [Surface; 2],
    /// Each face's side, `+1` or `-1` ([`FootSide::side`]).
    pub sides: [f64; 2],
    /// The leader.
    pub leader: BSplineCurve,
    /// The face's piece of the leader's parameter.
    pub interval: [f64; 2],
    /// The legs.
    pub legs: [f64; 2],
    /// The sagitta.
    pub sagitta: f64,
    /// The weld's spans, as the leader's parameters.
    pub spans: Vec<[f64; 2]>,
    /// The end shape at each span's start and end.
    pub ends: [BeadEnd; 2],
    /// The fit tolerance.
    pub tol: f64,
    /// The motions, in order.
    pub motions: Vec<Motion>,
}

impl FootBlendDefinition {
    /// A definition from its parts, as a record holds them; not checked here: the rebuild
    /// ([`FootBlendDefinition::rebuild`]) refuses what the foot blend refuses, and
    /// [`FootBlendDefinition::recertify`] a net that is not the rebuilt one.
    #[must_use]
    pub fn from_parts(parts: FootBlendParts) -> Self {
        Self {
            supports: parts.supports,
            sides: parts.sides,
            leader: parts.leader,
            interval: parts.interval,
            legs: parts.legs,
            sagitta: parts.sagitta,
            spans: parts.spans,
            ends: parts.ends,
            tol: parts.tol,
            motions: parts.motions,
        }
    }

    /// The parts, as a record holds them.
    #[must_use]
    pub fn parts(&self) -> FootBlendParts {
        FootBlendParts {
            supports: self.supports.clone(),
            sides: self.sides,
            leader: self.leader.clone(),
            interval: self.interval,
            legs: self.legs,
            sagitta: self.sagitta,
            spans: self.spans.clone(),
            ends: self.ends,
            tol: self.tol,
            motions: self.motions.clone(),
        }
    }

    /// Support `i`'s surface (0 or 1), where the bead was made: before its motions
    /// ([`Self::transform`]).
    #[must_use]
    pub fn support(&self, i: usize) -> Option<&Surface> {
        self.supports.get(i)
    }

    /// The legs.
    #[must_use]
    pub const fn legs(&self) -> [f64; 2] {
        self.legs
    }

    /// The sagitta.
    #[must_use]
    pub const fn sagitta(&self) -> f64 {
        self.sagitta
    }

    /// The weld's spans, as the leader's parameters.
    #[must_use]
    pub fn spans(&self) -> &[[f64; 2]] {
        &self.spans
    }

    /// The end shape at each span's start and end.
    #[must_use]
    pub const fn ends(&self) -> [BeadEnd; 2] {
        self.ends
    }

    /// The face's piece of the leader's parameter.
    #[must_use]
    pub const fn interval(&self) -> [f64; 2] {
        self.interval
    }

    /// The net's distance from the definition it states: the fit tolerance, checked at the
    /// midpoints between the stations when the net was made (sampled, not proved: the
    /// certificate follows).
    #[must_use]
    pub const fn bound(&self) -> f64 {
        self.tol
    }

    /// The foot blend the definition runs on: its leader over the face's piece, its supports
    /// and their sides.
    ///
    /// # Errors
    ///
    /// Native invalid input for a side that is not `+1` or `-1`, or a piece not inside the
    /// leader.
    pub fn blend(&self) -> Result<FootBlend> {
        let [a, b] = self.interval;
        let [s1, s2] = &self.supports;
        FootBlend::from_parts(
            self.leader.clone(),
            Extent1d::from_min_max(a, b),
            [Some(s1.clone()), Some(s2.clone())],
            self.sides,
        )
    }

    /// The net the definition gives: the foot blend's section face over the piece, then each
    /// motion.
    ///
    /// # Errors
    ///
    /// Native invalid input for numbers the foot blend refuses (a side, a piece outside the
    /// leader, a leg not positive, a tolerance not positive); its refusals; those of the
    /// transforms.
    pub fn rebuild(&self, k: &Kernel) -> Result<BSplineSurface> {
        if !(self.tol.is_finite() && self.tol > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "a tolerance that is not positive",
            ));
        }
        let [a, b] = self.interval;
        let mut net = self
            .blend()?
            .segment(k, self.legs, self.sagitta, self.tol, a, b)?
            .face;
        for m in &self.motions {
            net.transform(&m.rotate_n_move, m.scale)?;
        }
        Ok(net)
    }

    /// Checks `net` against this definition: the net is a B-spline surface, and the rebuilt
    /// net ([`FootBlendDefinition::rebuild`]) is it, in some orientation of the parameter
    /// square (the swap and reversals a face's net takes), the same degrees and knots and its
    /// control points within rounding (`1e-8 (1 + the net's size)`: the rebuild solves the feet
    /// again, to rounding, and may round differently on another machine). Returns the
    /// definition's bound ([`FootBlendDefinition::bound`]): the net's fit to the true section
    /// is the one sampled when it was made, not proved.
    ///
    /// # Errors
    ///
    /// `InvalidInput` naming what fails; those of the rebuild.
    pub fn recertify(&self, k: &Kernel, net: &Surface) -> Result<f64> {
        use crate::surface::surface::p2::SurfaceKind;
        let refuse = |why| SolidError::invalid_input(OP_RECERTIFY, why);
        let SurfaceKind::BSpline(form) = net.kind() else {
            return Err(refuse("a foot blend's net is a B-spline surface"));
        };
        let rebuilt = self.rebuild(k)?;
        let [a, _] = self.interval;
        let [z1, z2] = self.legs;
        let size = z1.max(z2)
            + self
                .leader
                .evaluate_point(a)
                .map_or(0.0, Point3::max_dimension);
        let eps = 1e-8 * (1.0 + size);
        match crate::surface::procedural_blend::corner::net_distance(&rebuilt, form) {
            Some(d) if d <= eps => Ok(self.tol),
            _ => Err(refuse("a foot blend's net is not its definition's")),
        }
    }

    /// Moves the definition with its net (`Transform`): the motion is kept, to be replayed on
    /// the rebuilt net, as a section blend's is
    /// ([`crate::sweep::section_blend::SectionBlendDefinition::transform`]). The supports and
    /// the leader stay where the bead was made (the rebuild makes the net there, then moves
    /// it), so [`Self::support`] is not the moved face's support.
    ///
    /// # Errors
    ///
    /// Native invalid input for a motion that is not rigid (a scale other than 1, a mirror):
    /// the legs, the sagitta and the bound would no longer be the net's. A face does not ask
    /// for one ([`crate::surface::procedural_blend::KeptDefinition::follows`] leaves the
    /// definition behind).
    pub fn transform(
        &mut self,
        rotate_n_move: &crate::geom::axis2_placement::Axis2Placement,
        scale: Option<Vec3>,
    ) -> Result<()> {
        if scale.is_some_and(|s| s.x != 1.0 || s.y != 1.0 || s.z != 1.0) {
            return Err(SolidError::invalid_input(
                OP,
                "a weld bead's definition moves only rigidly",
            ));
        }
        self.motions.push(Motion {
            rotate_n_move: *rotate_n_move,
            scale,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{on_rim, wrapped};
    use crate::geom::extent2d::Extent2d;
    use crate::geom::vector2d::Point2;

    /// A Newton that stops past a periodic direction's seam has not met the face's end; past
    /// a bounded direction's end (or on it) it has. A foot past the seam is moved into the
    /// domain by whole periods; a bounded direction is left as it is.
    #[test]
    fn a_periodic_seam_is_no_rim_and_a_foot_past_it_is_wrapped() {
        let tau = core::f64::consts::TAU;
        let domain = Extent2d::from_bounds(0.0, -1.0, tau, 1.0);
        let periodic_u = [Some(tau), None];
        assert!(!on_rim(&domain, periodic_u, Point2::new(tau + 0.3, 0.0)));
        assert!(!on_rim(&domain, periodic_u, Point2::new(0.0, 0.0)));
        assert!(on_rim(&domain, periodic_u, Point2::new(1.0, 1.0)));
        assert!(on_rim(&domain, periodic_u, Point2::new(1.0, -1.5)));
        assert!(on_rim(&domain, [None, None], Point2::new(tau + 0.3, 0.0)));
        let w = wrapped(&domain, periodic_u, Point2::new(tau + 0.3, 1.5));
        assert!((w.x - 0.3).abs() <= 1e-12 && w.y == 1.5, "{w:?}");
        let w = wrapped(&domain, periodic_u, Point2::new(-0.5, 0.2));
        assert!((w.x - (tau - 0.5)).abs() <= 1e-12, "{w:?}");
    }
}

/// The most times [`FootBlend::prove_feet`] halves a piece to prove the feet over it.
const PROOF_HALVINGS: usize = 4;

/// The longest piece [`FootBlend::prove_feet`] proves the feet over at first, as a share of
/// the smaller leg (the root's travel over it).
const PIECE_OF_LEG: f64 = 0.125;

/// The most pieces [`FootBlend::prove_feet`] cuts one span between two stations into.
const MAX_PIECES: f64 = 1024.0;

/// The first of [`FootBlend::prove_chain`]'s steps up the leg ends at this share of it: one
/// eighth, as a piece is of the leg along the joint ([`PIECE_OF_LEG`]).
const CHAIN_FIRST: f64 = 0.125;

/// Each of [`FootBlend::prove_chain`]'s further steps ends this many times as far from the
/// root as it starts: a step of one quarter of its distance. Over a step the distance from the
/// root (`|w|`, which `G₂` sets) changes by that share, and the box's centred form spreads its
/// rate in `ρ` by it; steps of an equal share of the leg would spread it by a half next to
/// the first, where Krawczyk's test fails (by the quarter, 10 steps up the leg).
const CHAIN_GROWTH: f64 = 1.25;

/// How far from the root [`FootBlend::prove_chain`] starts, in bounds on the gap between the
/// support's point on the root's path and the root: there the gap moves the branch's direction
/// by at most one part in this many (the root itself is known no closer than that gap).
const ROOT_GAP: f64 = 64.0;

/// The most Krawczyk steps one of [`FootBlend::prove_chain`]'s boxes gets: twice
/// `geom::krawczyk`'s, as it widens one unknown at a time.
const CHAIN_KRAWCZYK_STEPS: usize = 16;

/// A support's NURBS form, for the feet's proof: its data, the surface over it (to place a
/// foot on the form, whose parameters differ from an analytic support's own), its natural
/// domain, and its period in u and v where it is periodic.
struct ProofSupport {
    cn: crate::blend::fillet_section::station::Nurbs,
    form: Surface,
    domain: Extent2d,
    period: [Option<f64>; 2],
}

impl ProofSupport {
    /// The form of `surface`, or `None` where it has none (the proof is then not made).
    fn of(k: &Kernel, surface: &Surface) -> Result<Option<Self>> {
        use crate::blend::fillet_section::station::{as_b_spline, Nurbs};
        use crate::nurbs::b_spline_surface::SurfParamType;
        let Some(b) = as_b_spline(k, surface)? else {
            return Ok(None);
        };
        let cn = Nurbs::of(&b)?;
        let form = Surface::from(b.into_owned());
        let domain = form.natural_uv_domain_with_nurb(k)?;
        let (lo, hi) = (domain.min(), domain.max());
        let period = [
            form.is_periodic(k, &domain, SurfParamType::U)?
                .then_some(hi.x - lo.x),
            form.is_periodic(k, &domain, SurfParamType::V)?
                .then_some(hi.y - lo.y),
        ];
        Ok(Some(Self {
            cn,
            form,
            domain,
            period,
        }))
    }

    /// The parameters of `point` (on the support) on the form, near `near`.
    fn place(&self, k: &Kernel, point: Point3, near: Option<Point2>) -> Result<Option<Point2>> {
        dropped(k, &self.form, &self.domain, point, near)
    }

    /// `uv` moved by whole periods to the side of a seam that `near` is on.
    fn unwrapped(&self, uv: Point2, near: Point2) -> Point2 {
        let unwrap = |x: f64, n: f64, p: Option<f64>| match p {
            Some(p) if p > 0.0 => x + p * ((n - x) / p).round(),
            _ => x,
        };
        let [pu, pv] = self.period;
        Point2::new(unwrap(uv.x, near.x, pu), unwrap(uv.y, near.y, pv))
    }

    /// The form's partials to order `lk` over the box `x`, enclosed: the box cut at the knot
    /// lines inside the knots' range, each part on its own polynomial piece
    /// (`nurbs_partials_over_piece`), and the hull. Past the range, in a periodic direction
    /// the box is moved by whole periods into it and, where it runs across the seam, cut
    /// there (the surface is the same on both sides); in another direction the part past the
    /// range is on the end piece continued (`station::Nurbs`: a plane goes on as the plane),
    /// where the station's Newton found its foot too. The form is continuous across its inner
    /// knot lines, which order 1 needs for the mean-value form (`geom::krawczyk`). `None`
    /// where the box spans a period or a piece's enclosure fails.
    fn partials(
        &self,
        x: [Ivl; 2],
        lk: usize,
    ) -> Result<Option<crate::blend::fillet_section::station::Partials<Ivl>>> {
        use crate::blend::fillet_section::certificate::{
            knot_cuts, nurbs_partials_over_piece, partials_hull,
        };
        let cn = &self.cn;
        // one direction's parts with their pieces: (from, to, piece)
        let parts = |iv: Ivl,
                     knots: &[f64],
                     p: usize,
                     n: usize,
                     period: Option<f64>|
         -> Result<Option<Vec<(f64, f64, usize)>>> {
            let (Some(&first), Some(&last)) = (knots.get(p), knots.get(n)) else {
                return Ok(None);
            };
            let mut ranges = vec![(iv.lo, iv.hi)];
            if let Some(per) = period.filter(|per| *per > 0.0) {
                if iv.lo < first || iv.hi > last {
                    if iv.hi - iv.lo >= per {
                        return Ok(None);
                    }
                    let shift = -per * ((iv.lo - first) / per).floor();
                    let (a, b) = (iv.lo + shift, iv.hi + shift);
                    ranges = if b <= last {
                        vec![(a, b)]
                    } else {
                        vec![(a, last), (first, b - per)]
                    };
                }
            }
            let mut out = Vec::new();
            for (a, b) in ranges {
                if a < first {
                    out.push((a, b.min(first), p));
                }
                let (ca, cb) = (a.max(first), b.min(last));
                if ca <= cb {
                    match knot_cuts(knots, p, n, ca, cb, 0) {
                        Ok(c) => out.extend(c),
                        Err(e) if e.is_cancelled() => return Err(e),
                        Err(_) => return Ok(None),
                    }
                }
                if b > last {
                    out.push((a.max(last), b, n.saturating_sub(1)));
                }
            }
            Ok(Some(out))
        };
        let [xu, xv] = x;
        let [pu, pv] = self.period;
        let (Some(us), Some(vs)) = (
            parts(xu, &cn.u, cn.p, cn.nu, pu)?,
            parts(xv, &cn.v, cn.q, cn.nv, pv)?,
        ) else {
            return Ok(None);
        };
        let mut out = None;
        for &(a0, a1, ku) in &us {
            for &(b0, b1, kv) in &vs {
                let part = match nurbs_partials_over_piece(cn, ku, kv, a0, a1, b0, b1, lk) {
                    Ok(p) => p,
                    Err(e) if e.is_cancelled() => return Err(e),
                    Err(_) => return Ok(None),
                };
                out = Some(match out {
                    None => part,
                    Some(h) => partials_hull(h, &part, lk),
                });
            }
        }
        Ok(out)
    }
}

/// `w`, its rate `w′` along the path, `S_u` and `S_v`, and `w` at the piece's middle, as
/// [`FootBlend::prove_feet`]'s pieces enclose them.
type Centred = (
    crate::blend::fillet_section::series::V3<Ivl>,
    crate::blend::fillet_section::series::V3<Ivl>,
    crate::blend::fillet_section::series::V3<Ivl>,
    crate::blend::fillet_section::series::V3<Ivl>,
    crate::blend::fillet_section::series::V3<Ivl>,
);

/// `a . b` in intervals.
fn idot(
    a: crate::blend::fillet_section::series::V3<Ivl>,
    b: crate::blend::fillet_section::series::V3<Ivl>,
) -> Ivl {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// `a - b` in intervals.
fn isub(
    a: crate::blend::fillet_section::series::V3<Ivl>,
    b: crate::blend::fillet_section::series::V3<Ivl>,
) -> crate::blend::fillet_section::series::V3<Ivl> {
    crate::blend::fillet_section::series::V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

/// `a × b` in intervals.
fn icross(
    a: crate::blend::fillet_section::series::V3<Ivl>,
    b: crate::blend::fillet_section::series::V3<Ivl>,
) -> crate::blend::fillet_section::series::V3<Ivl> {
    crate::blend::fillet_section::series::V3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

/// The path a foot's piece is proved about ([`FootBlend::prove_piece`]): `x(t) = mid + slope
/// (t − t̃) + δ` on the support's form, `t̃` the piece's middle, and the box of `δ` Krawczyk's
/// test ran on, in which the foot is the only point for every `t` of the piece.
#[derive(Debug, Clone, Copy)]
struct PiecePath {
    mid: [f64; 2],
    slope: [f64; 2],
    region: [Ivl; 2],
}

/// Krawczyk's test about `mid` (`geom::krawczyk`'s operator), its preconditioner the
/// Jacobian's middle there inverted: the box proved to hold a root, and the box it is the
/// only one in. The first box is `geom::krawczyk`'s (twice Newton's step about `mid`); a box
/// not proved is widened only in the unknowns whose operator leaves it, so an unknown that is
/// already held is not widened with the others: near the root the chain's two unknowns are
/// known to very different widths (along the joint to the gap's share, across it to its
/// square), and the uniform widening of `geom::krawczyk` grows the first without end.
///
/// # Errors
///
/// Those of `value` and `jacobian`.
fn krawczyk_about(
    mid: [f64; 2],
    value: &dyn Fn([Ivl; 2]) -> Result<Option<[Ivl; 2]>>,
    jacobian: &dyn Fn([Ivl; 2]) -> Result<Option<[[Ivl; 2]; 2]>>,
) -> Result<Option<([Ivl; 2], [Ivl; 2])>> {
    use crate::blend::fillet_section::interval::round_up;
    use crate::geom::krawczyk::{contraction, inverse_of_columns2, k_operator};
    let at = mid.map(Ivl::from);
    let (Some(j), Some(f)) = (jacobian(at)?, value(at)?) else {
        return Ok(None);
    };
    let middle = j.map(|col| col.map(Ivl::mid));
    let Some(c) = inverse_of_columns2(&middle) else {
        return Ok(None);
    };
    let mut radius = [0.0_f64; 2];
    for ((r, row), m) in radius.iter_mut().zip(&c).zip(&mid) {
        let cf = row
            .iter()
            .zip(&f)
            .fold(Ivl::from(0.0), |s, (&ci, &fi)| s + Ivl::from(ci) * fi);
        *r = round_up(2.0 * cf.mag() + 1e-12 * (1.0 + m.abs()));
    }
    for _ in 0..CHAIN_KRAWCZYK_STEPS {
        if radius.iter().any(|r| !r.is_finite()) {
            return Ok(None);
        }
        let [m0, m1] = mid;
        let [r0, r1] = radius;
        let y = [Ivl::new(m0 - r0, m0 + r0), Ivl::new(m1 - r1, m1 + r1)];
        let Some([[a, b], [cc, d]]) = jacobian(y)? else {
            return Ok(None);
        };
        // by rows: the first equation's partials, then the second's
        let (cm, _) = contraction(&c, &[[a, cc], [b, d]]);
        let k = k_operator(&mid, &c, &f, &cm, &y);
        let held = [0, 1].map(|i| match (k.get(i), y.get(i)) {
            (Some(ki), Some(yi)) => ki.lo > yi.lo && ki.hi < yi.hi,
            _ => false,
        });
        if held.iter().all(|h| *h) {
            let [k0, k1] = k;
            let [y0, y1] = y;
            return Ok(Some((
                [
                    Ivl::new(k0.lo.max(y0.lo), k0.hi.min(y0.hi)),
                    Ivl::new(k1.lo.max(y1.lo), k1.hi.min(y1.hi)),
                ],
                y,
            )));
        }
        for (((r, ki), m), h) in radius.iter_mut().zip(&k).zip(&mid).zip(held) {
            if !h {
                let reach = (ki.lo - m).abs().max((ki.hi - m).abs());
                *r = round_up(1.5 * reach.max(*r) + 1e-12 * (1.0 + m.abs()));
            }
        }
    }
    Ok(None)
}

/// A proved piece of a foot ([`FootBlend::prove_piece`]): the box of its parameters on the
/// support's form over the whole piece, `w = S − e` there in its centred form, and the path
/// it was proved about.
type ProvedPiece = (
    [Ivl; 2],
    crate::blend::fillet_section::series::V3<Ivl>,
    PiecePath,
);

/// What [`FootBlend::prove_feet`] proved: the spans' pieces and how many halvings they took.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FeetProof {
    /// The pieces the stations' spans were proved over (each span one piece unless halved).
    pub pieces: usize,
    /// The halvings it took.
    pub halvings: usize,
    /// The chains from the root to the toe proved (`FootBlend::prove_chain`): one per piece for
    /// each support proved.
    pub chains: usize,
}

impl FootBlend {
    /// Proves the feet between the stations `solved` (consecutive, solved with the legs
    /// `legs`, in order along the leader): over each span `T` between two stations and for each support, Krawczyk's test
    /// (`geom::krawczyk`) on the foot's equations `G₁ = (S − e(t))·e′(t)`, `G₂ = |S − e(t)|² − z²`
    /// with `t` ranging over all of `T`, in a box about the two stations' feet on the support's
    /// NURBS form. A pass proves, for every `t` in `T`, one foot in the box and no other: the
    /// continuation of the stations' feet. Then the chain from the root to that foot
    /// (`FootBlend::prove_chain`): the foot is on the branch of the support's section that
    /// leaves the root along the face. With both supports proved, the corner's rise keeps its
    /// sign over the piece, `(n₁ × n₂)·e′` (the supports' normals at the root's path) not 0,
    /// and the same sign on every piece: the corner stays an inside corner between the
    /// stations [`FootBlend::of_edge`] reads it at. A span not proved is halved (a station at
    /// its middle), at most [`PROOF_HALVINGS`] times. Supports with no NURBS form are not
    /// proved (their spans count as proved pieces of none; the rise is then not proved).
    ///
    /// # Errors
    ///
    /// Native invalid input where a piece's rise is proved of the other sign (an outside or a
    /// tangent stretch of the edge); where a piece stays unproved: the refusal of no toe at the
    /// leg (with the largest leg that finds its toes) where the equations are near singular at
    /// its end (the leg's circle grazes the face's section there: the two toes on it meet),
    /// else [`BlendFailure::NotCertified`]; those of the evaluations and of the middle stations.
    pub fn prove_feet(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        solved: &[FootStation],
    ) -> Result<FeetProof> {
        use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
        let leader = NurbsCurve::of(&self.leader)?;
        let supports = [
            ProofSupport::of(k, &self.sides[0].surface)?,
            ProofSupport::of(k, &self.sides[1].surface)?,
        ];
        let mut proof = FeetProof::default();
        // each support's last foot on its form: the next piece's start, to place it near
        let mut guesses: [Option<Point2>; 2] = [None, None];
        // the rise's sign (positive or not) on the pieces proved so far
        let mut rise: Option<bool> = None;
        // each span cut into pieces no longer than a share of the smaller leg (over a piece
        // the toe moves along the joint as the root does, and the box encloses the two apart:
        // the Jacobian's spread grows with the piece's length over the leg), then each piece
        // and its halves from a work list (no recursion): (start, end, depth)
        let reach = legs.iter().fold(f64::INFINITY, |m, z| m.min(*z)) * PIECE_OF_LEG;
        let mut work: Vec<(FootStation, FootStation, usize)> = Vec::new();
        for w in solved.windows(2) {
            let &[a, b] = w else { continue };
            let count = (a.root.distance(b.root) / reach)
                .ceil()
                .clamp(1.0, MAX_PIECES) as usize;
            let mut from = a;
            for j in 1..=count {
                let to = if j == count {
                    b
                } else {
                    let t = a.t + (b.t - a.t) * j as f64 / count as f64;
                    match self.try_station(k, t, legs, Some(&from))? {
                        Ok(s) => s,
                        Err(miss) => {
                            let ts: Vec<f64> = solved.iter().map(|s| s.t).collect();
                            return Err(self.refusal(k, miss, &ts, legs, 0.0)?);
                        }
                    }
                };
                work.push((from, to, 0));
                from = to;
            }
        }
        work.reverse();
        while let Some((a, b, depth)) = work.pop() {
            let mut proved = true;
            let mut normals = [None, None];
            let mut chains = 0;
            for (i, ((support, guess), normal)) in supports
                .iter()
                .zip(guesses.iter_mut())
                .zip(normals.iter_mut())
                .enumerate()
            {
                let Some(support) = support else { continue };
                let z = legs.get(i).copied().unwrap_or(0.0);
                let (Some(&fa), Some(&fb)) = (a.feet.get(i), b.feet.get(i)) else {
                    return Err(defect("a station lacked the foot of a support it is proved on, though every station is built with both"));
                };
                // (a piece after a halving starts where its guess does not: place it afresh)
                if depth > 0 {
                    *guess = None;
                }
                let Some((.., path)) =
                    Self::prove_piece(k, support, &leader, a.t, b.t, z, fa, fb, guess)?
                else {
                    proved = false;
                    break;
                };
                let Some(n) = self.prove_chain(k, support, &leader, i, &a, &b, z, path)? else {
                    proved = false;
                    break;
                };
                *normal = Some(n);
                chains += 1;
            }
            if proved {
                if let [Some(n1), Some(n2)] = normals {
                    let over =
                        curve_derivs_over(&leader, Ivl::new(a.t.min(b.t), a.t.max(b.t)), 1, None)?;
                    let Some(&d) = over.get(1) else {
                        return Err(defect("the leader's derivatives over a piece of its own interval came back without the first"));
                    };
                    let r = idot(icross(n1, n2), d);
                    let sign = if r.lo > 0.0 {
                        Some(true)
                    } else if r.hi < 0.0 {
                        Some(false)
                    } else {
                        None
                    };
                    match (sign, rise) {
                        (None, _) => proved = false,
                        (Some(s), Some(was)) if s != was => {
                            return Err(SolidError::invalid_input(
                                OP,
                                "a weld bead lies in an inside corner (a concave edge) only, not on an outside or a tangent edge",
                            ));
                        }
                        (Some(s), _) => rise = Some(s),
                    }
                }
            }
            if proved {
                proof.pieces += 1;
                proof.chains += chains;
                continue;
            }
            if depth >= PROOF_HALVINGS {
                return Err(self.unproved(k, legs, &a, &b, solved, proof)?);
            }
            let m = 0.5 * (a.t + b.t);
            let mid = match self.try_station(k, m, legs, Some(&a))? {
                Ok(s) => s,
                Err(miss) => {
                    let ts: Vec<f64> = solved.iter().map(|s| s.t).collect();
                    return Err(self.refusal(k, miss, &ts, legs, 0.0)?);
                }
            };
            proof.halvings += 1;
            work.push((mid, b, depth + 1));
            work.push((a, mid, depth + 1));
        }
        Ok(proof)
    }

    /// Krawczyk's test for one support's foot over a piece `[ta, tb]` of the leader, `fa` and
    /// `fb` the feet at its ends and `z` the leg. The foot is taken as the straight path
    /// between the ends' feet on the support's form plus a correction `δ`, `x(t) = x̃ + s (t −
    /// t̃) + δ` about the middle `t̃`, and `w(t) = S(x(t)) − e(t)` is enclosed in its centred
    /// form `w(t̃) + w′(X, T) (t − t̃)`, `w′ = S_u s_u + S_v s_v − e′`: the root and the toe move
    /// together, and the centred form keeps them together (enclosed apart, the toe's travel
    /// along the joint would enter the Jacobian at its full size). The unknowns are `δ`, about
    /// 0; the equations `G₁ = w·e′`, `G₂ = |w|² − z²` hold for every `t` in the piece.
    /// Proved, the box of the foot's parameters on the form over the whole piece (the path
    /// plus the correction's box) and `w` over it in its centred form (narrow: the root's
    /// and the toe's travel along the joint cancel in it); `None` where it is not proved.
    #[allow(clippy::too_many_arguments)]
    fn prove_piece(
        k: &Kernel,
        support: &ProofSupport,
        leader: &crate::blend::fillet_section::certificate::NurbsCurve,
        ta: f64,
        tb: f64,
        z: f64,
        fa: Point3,
        fb: Point3,
        guess: &mut Option<Point2>,
    ) -> Result<Option<ProvedPiece>> {
        use crate::blend::fillet_section::certificate::curve_derivs_over;
        use crate::blend::fillet_section::series::V3;
        use crate::geom::krawczyk::{inverse_of_columns2, krawczyk_region};
        // the feet on the form, the second placed near the first (one side of a seam)
        // (the piece before ended at this foot: its place is that piece's end)
        let ua = match *guess {
            Some(uv) => uv,
            None => match support.place(k, fa, None)? {
                Some(uv) => uv,
                None => return Ok(None),
            },
        };
        let Some(ub) = support.place(k, fb, Some(ua))? else {
            return Ok(None);
        };
        let unwrap = |x: f64, near: f64, p: Option<f64>| match p {
            Some(p) if p > 0.0 => x + p * ((near - x) / p).round(),
            _ => x,
        };
        let [pu, pv] = support.period;
        let ub = Point2::new(unwrap(ub.x, ua.x, pu), unwrap(ub.y, ua.y, pv));
        *guess = Some(ub);
        let (lo, hi) = (ta.min(tb), ta.max(tb));
        let tm = 0.5 * (lo + hi);
        let dt = Ivl::new(lo, hi) - Ivl::from(tm);
        let span = tb - ta;
        if span == 0.0 {
            return Ok(None);
        }
        let mid = [0.5 * (ua.x + ub.x), 0.5 * (ua.y + ub.y)];
        let slope = [(ub.x - ua.x) / span, (ub.y - ua.y) / span];
        let at_mid = curve_derivs_over(leader, Ivl::from(tm), 1, None)?;
        let over = curve_derivs_over(leader, Ivl::new(lo, hi), 2, None)?;
        let (Some(&em), Some(&dm), Some(&d), Some(&dd)) =
            (at_mid.first(), at_mid.get(1), over.get(1), over.get(2))
        else {
            return Ok(None);
        };
        let scale = |v: V3<Ivl>, a: Ivl| V3 {
            x: v.x * a,
            y: v.y * a,
            z: v.z * a,
        };
        let add = |a: V3<Ivl>, b: V3<Ivl>| V3 {
            x: a.x + b.x,
            y: a.y + b.y,
            z: a.z + b.z,
        };
        let s = slope.map(Ivl::from);
        let [m0, m1] = mid.map(Ivl::from);
        // w over the δ box and the piece, centred at t̃; its rate w′ along the path; S's
        // first partials over the box x(T) + δ they are read on; and w at t̃
        let centred = |delta: [Ivl; 2]| -> Result<Option<Centred>> {
            let [d0, d1] = delta;
            let Some(here) = support.partials([m0 + d0, m1 + d1], 0)? else {
                return Ok(None);
            };
            let [s0, s1] = s;
            let swept = [m0 + d0 + s0 * dt, m1 + d1 + s1 * dt];
            let Some(p) = support.partials(swept, 1)? else {
                return Ok(None);
            };
            let (su, sv) = (p.at(1, 0), p.at(0, 1));
            let slope_w = isub(add(scale(su, s0), scale(sv, s1)), d);
            let wm = isub(here.at(0, 0), em);
            let w = add(wm, scale(slope_w, dt));
            Ok(Some((w, slope_w, su, sv, wm)))
        };
        let zz = Ivl::from(z) * Ivl::from(z);
        let two = Ivl::from(2.0);
        // the equations centred along the path: G(t̃) + G′(T) (t − t̃), G′ their total rate
        // along it, (w′·e′ + w·e″, 2 w·w′), which nearly vanishes where the path follows the
        // foot (enclosed apart, w·e″ alone, the leg times the joint's turn, would not)
        let value = |delta: [Ivl; 2]| -> Result<Option<[Ivl; 2]>> {
            Ok(centred(delta)?.map(|(w, rate, _, _, wm)| {
                let g1 = idot(wm, dm) + (idot(rate, d) + idot(w, dd)) * dt;
                let g2 = idot(wm, wm) - zz + two * idot(w, rate) * dt;
                [g1, g2]
            }))
        };
        let jacobian = |delta: [Ivl; 2]| -> Result<Option<[[Ivl; 2]; 2]>> {
            // by columns: the partials in δ_u, then in δ_v
            Ok(centred(delta)?.map(|(w, _, su, sv, _)| {
                [
                    [idot(su, d), two * idot(w, su)],
                    [idot(sv, d), two * idot(w, sv)],
                ]
            }))
        };
        // the preconditioner: the Jacobian's middle inverted
        let Some(j) = jacobian([Ivl::from(0.0); 2])? else {
            return Ok(None);
        };
        let middle = j.map(|col| col.map(Ivl::mid));
        let Some(c) = inverse_of_columns2(&middle) else {
            return Ok(None);
        };
        // proved: one foot in the box for every t, and the box holds the path's ends (the
        // stations' feet, δ = 0 there), so that foot is theirs
        let Some((delta, region)) = krawczyk_region::<2>([0.0; 2], c, &value, &jacobian)?
            .filter(|(b, _)| b.iter().all(|x| x.lo <= 0.0 && x.hi >= 0.0))
        else {
            return Ok(None);
        };
        let Some((w, ..)) = centred(delta)? else {
            return Ok(None);
        };
        let ([d0, d1], [s0, s1]) = (delta, s);
        Ok(Some((
            [m0 + d0 + s0 * dt, m1 + d1 + s1 * dt],
            w,
            PiecePath { mid, slope, region },
        )))
    }

    /// The chain from the root to support `i`'s foot over the piece from `a` to `b`, the foot
    /// proved about `foot` ([`FootBlend::prove_piece`]) at the leg `z`: for every `t` of the
    /// piece, the points of the support's section in the station's plane at each distance
    /// `ρ` from the root, `ρ` from near the root to `z`, are one branch, one point at each
    /// `ρ`, that leaves the root along the face (on the side `sides[i]` names) and ends in the
    /// foot. So the foot is the first point of that branch at the leg: the toe a leg gauge
    /// finds, not a point of the section that curves back, or of another sheet of it.
    ///
    /// The leg is climbed in steps from [`CHAIN_FIRST`] of it, each [`CHAIN_GROWTH`] times as
    /// far from the root as the one before, each a box ([`FootBlend::prove_box`]:
    /// Krawczyk's test with `t` over the piece and `ρ` over the step), the last about the
    /// foot's own path. Below the first step, where the equations' two rows meet at the root
    /// (`|S − e|² = ρ²` loses its rank), the points are taken as `x = r(t) + ρ y` about the
    /// root's path `r(t)` on the form (the stations' roots on it, joined), and the test runs on
    /// `y` with the equations divided by `ρ` and `ρ²` (`W = (S(x) − e)/ρ`: `W·e′ = 0`,
    /// `|W|² = 1`), from `ρ` = [`ROOT_GAP`] times the bound on the gap `S(r(t)) − e(t)` (the
    /// root itself is known no closer). There `W` leaves along the face: `W·b > 0`, `b` the
    /// face's side times `n × e′`. Where two boxes meet, the branch's point at the piece's
    /// middle `t̃` is proved to rounding (a box of no extent in `t` and `ρ`) and lies strictly
    /// inside the regions of both, each of which holds one root only; the toe's lies so in the
    /// last box's and in the foot's own. Each box's root is continuous in `t` and `ρ` (its
    /// Jacobian is regular over its region), so the boxes' roots are one branch over all of
    /// the piece.
    ///
    /// `Some(n)`, the form's normal `S_u × S_v` over the root's path (its own sense, for
    /// [`FootBlend::prove_feet`]'s rise); `None` where any of it is not proved.
    ///
    /// # Errors
    ///
    /// Those of the evaluations and the enclosures.
    #[allow(clippy::too_many_arguments)]
    fn prove_chain(
        &self,
        k: &Kernel,
        support: &ProofSupport,
        leader: &crate::blend::fillet_section::certificate::NurbsCurve,
        i: usize,
        a: &FootStation,
        b: &FootStation,
        z: f64,
        foot: PiecePath,
    ) -> Result<Option<crate::blend::fillet_section::series::V3<Ivl>>> {
        use crate::blend::fillet_section::certificate::curve_derivs_over;
        use crate::blend::fillet_section::series::{v_add, v_scale, v_sub, V3};
        let Some(side) = self.sides.get(i) else {
            return Ok(None);
        };
        let span = b.t - a.t;
        if span == 0.0 || z <= 0.0 || !z.is_finite() {
            return Ok(None);
        }
        let (lo, hi) = (a.t.min(b.t), a.t.max(b.t));
        let tm = 0.5 * (lo + hi);
        let dt = Ivl::new(lo, hi) - Ivl::from(tm);
        // the roots on the form, on the foot's side of a seam
        let [fm0, fm1] = foot.mid;
        let near = Point2::new(fm0, fm1);
        let Some(ra) = support.place(k, a.root, Some(near))? else {
            return Ok(None);
        };
        let ra = support.unwrapped(ra, near);
        let Some(rb) = support.place(k, b.root, Some(ra))? else {
            return Ok(None);
        };
        let rb = support.unwrapped(rb, ra);
        let mr = [0.5 * (ra.x + rb.x), 0.5 * (ra.y + rb.y)];
        let [mr0, mr1] = mr;
        let sr = [(rb.x - ra.x) / span, (rb.y - ra.y) / span];
        let at_mid = curve_derivs_over(leader, Ivl::from(tm), 1, None)?;
        let over = curve_derivs_over(leader, Ivl::new(lo, hi), 2, None)?;
        let (Some(&em), Some(&dm), Some(&d), Some(&dd)) =
            (at_mid.first(), at_mid.get(1), over.get(1), over.get(2))
        else {
            return Ok(None);
        };
        let [m0, m1] = mr.map(Ivl::from);
        let [s0, s1] = sr.map(Ivl::from);
        let path = [m0 + s0 * dt, m1 + s1 * dt];
        let (Some(pm), Some(pr)) = (support.partials([m0, m1], 1)?, support.partials(path, 2)?)
        else {
            return Ok(None);
        };
        // the gap S(r(t)) − e(t), to second order about t̃ (to first, the root's travel along
        // the joint would enter it at its full size)
        let two = Ivl::from(2.0);
        let g0 = v_sub(pm.at(0, 0), em);
        let g1 = v_sub(
            v_add(v_scale(pm.at(1, 0), s0), v_scale(pm.at(0, 1), s1)),
            dm,
        );
        let g2 = v_sub(
            v_add(
                v_add(
                    v_scale(pr.at(2, 0), s0 * s0),
                    v_scale(pr.at(1, 1), two * s0 * s1),
                ),
                v_scale(pr.at(0, 2), s1 * s1),
            ),
            dd,
        );
        let h = Ivl::from(dt.mag());
        let dt2 = Ivl::new(0.0, (h * h).hi);
        let gap = v_add(
            v_add(g0, v_scale(g1, dt)),
            v_scale(g2, Ivl::from(0.5) * dt2),
        );
        let mut steps: Vec<f64> = Vec::new();
        let mut up = z * CHAIN_FIRST;
        steps.push(up);
        while up < z {
            up = (up * CHAIN_GROWTH).min(z);
            steps.push(up);
        }
        let Some(&first) = steps.first() else {
            return Ok(None);
        };
        let start = (ROOT_GAP * ivl_norm(gap)).max(f64::MIN_POSITIVE);
        if !start.is_finite() || start >= first {
            return Ok(None);
        }
        // the branch's points at t̃ at each step's leg (the station's foot there), on the form;
        // the boxes are anchored at them, the last at the foot's path's middle
        let (root, tangent) = self.root_at(k, tm)?;
        let mut x: Vec<[f64; 2]> = Vec::new();
        let mut root_uv = None;
        let mut at = Point2::new(mr0, mr1);
        for &rho in &steps {
            let Ok((p, _, r)) = self.foot(k, i, root, tangent, rho, None)? else {
                return Ok(None);
            };
            root_uv = Some(r);
            let Some(u) = support.place(k, p, Some(at))? else {
                return Ok(None);
            };
            at = support.unwrapped(u, at);
            x.push([at.x, at.y]);
        }
        // each of them proved at its own (t̃, ρ), to rounding: where two boxes meet, this point
        // in the regions of both (each the only root there) makes their roots one
        let mut held: Vec<[Ivl; 2]> = Vec::new();
        for (&rho, &[u, v]) in steps.iter().zip(&x) {
            let Some((kp, _)) = Self::prove_box(
                support,
                leader,
                [tm, tm],
                tm,
                [rho, rho],
                rho,
                [u, v],
                [0.0; 2],
                [0.0; 2],
            )?
            else {
                return Ok(None);
            };
            let [k0, k1] = kp;
            held.push([Ivl::from(u) + k0, Ivl::from(v) + k1]);
        }
        if let Some(last) = x.last_mut() {
            *last = foot.mid;
        }
        // the form's sense against the support's own normal, which the side was read against
        let Some(r) = root_uv else {
            return Ok(None);
        };
        let Some(ns) = normal_at(k, &side.surface, r)? else {
            return Ok(None);
        };
        let nf = ivl_mid(pm.at(1, 0)).0.cross(ivl_mid(pm.at(0, 1)).0);
        let agree = nf.dot(ns);
        if agree.is_nan() || agree.abs() <= 0.5 * nf.length() * ns.length() {
            return Ok(None);
        }
        let sense = side.side * agree.signum();
        let normal = icross(pr.at(1, 0), pr.at(0, 1));
        // below the first step: y = (x − r(t))/ρ about the first point, W = gap/ρ + S̄_x y
        // (S̄_x the form's partials over the segment from r(t) to x, in r(T) + [0, ρ₁] Y)
        let Some(&[x1u, x1v]) = x.first() else {
            return Ok(None);
        };
        let y_mid = [(x1u - mr0) / first, (x1v - mr1) / first];
        let lifted_gap = v_scale(gap, Ivl::from(1.0) / Ivl::new(start, first));
        let reach = Ivl::new(0.0, first);
        let [p0, p1] = path;
        let lift = |y: [Ivl; 2]| -> Result<Option<[V3<Ivl>; 3]>> {
            let [y0, y1] = y;
            Ok(support
                .partials([p0 + reach * y0, p1 + reach * y1], 1)?
                .map(|p| {
                    let (su, sv) = (p.at(1, 0), p.at(0, 1));
                    let w = v_add(lifted_gap, v_add(v_scale(su, y0), v_scale(sv, y1)));
                    [w, su, sv]
                }))
        };
        let value = |y: [Ivl; 2]| -> Result<Option<[Ivl; 2]>> {
            Ok(lift(y)?.map(|[w, ..]| [idot(w, d), idot(w, w) - Ivl::from(1.0)]))
        };
        let jacobian = |y: [Ivl; 2]| -> Result<Option<[[Ivl; 2]; 2]>> {
            Ok(lift(y)?.map(|[w, su, sv]| {
                [
                    [idot(su, d), two * idot(w, su)],
                    [idot(sv, d), two * idot(w, sv)],
                ]
            }))
        };
        let Some((ky, region)) = krawczyk_about(y_mid, &value, &jacobian)? else {
            return Ok(None);
        };
        // it leaves along the face
        let Some([w, ..]) = lift(ky)? else {
            return Ok(None);
        };
        let into = idot(w, icross(normal, d)) * Ivl::from(sense);
        if into.lo <= 0.0 {
            return Ok(None);
        }
        // the point at (t̃, ρ₁) in its region: y = (x − r(t̃))/ρ₁
        let inside = |e: Ivl, r: Ivl| e.lo > r.lo && e.hi < r.hi;
        let Some(&[p0u, p0v]) = held.first() else {
            return Ok(None);
        };
        let [g0r, g1r] = region;
        let lift1 = Ivl::from(1.0) / Ivl::from(first);
        if !inside((p0u - m0) * lift1, g0r) || !inside((p0v - m1) * lift1, g1r) {
            return Ok(None);
        }
        let tops = steps.len().saturating_sub(1);
        for (q, ((r, p), h)) in steps
            .windows(2)
            .zip(x.windows(2))
            .zip(held.windows(2))
            .enumerate()
        {
            let (&[r0, r1], &[[u0, v0], [u1, v1]], &[[a0, a1], [b0, b1]]) = (r, p, h) else {
                return Ok(None);
            };
            let top = q + 1 == tops;
            // along the joint, the root's slope moved toward the foot's with the leg; the last
            // step the foot's own
            let along = if top {
                foot.slope
            } else {
                let [fs0, fs1] = foot.slope;
                let [rs0, rs1] = sr;
                [rs0 + (fs0 - rs0) * r1 / z, rs1 + (fs1 - rs1) * r1 / z]
            };
            let out = [(u1 - u0) / (r1 - r0), (v1 - v0) / (r1 - r0)];
            let Some((_, yq)) = Self::prove_box(
                support,
                leader,
                [lo, hi],
                tm,
                [r0, r1],
                r1,
                [u1, v1],
                along,
                out,
            )?
            else {
                return Ok(None);
            };
            // the points at (t̃, r0) and (t̃, r1) in this box's region there
            let [y0, y1] = yq;
            let [o0, o1] = out;
            let down = Ivl::from(r0) - Ivl::from(r1);
            if !inside(a0, Ivl::from(u1) + Ivl::from(o0) * down + y0)
                || !inside(a1, Ivl::from(v1) + Ivl::from(o1) * down + y1)
                || !inside(b0, Ivl::from(u1) + y0)
                || !inside(b1, Ivl::from(v1) + y1)
            {
                return Ok(None);
            }
            // and the toe at (t̃, z) in the foot's own region, about the same path
            if top {
                let [f0, f1] = foot.region;
                if !inside(b0, Ivl::from(u1) + f0) || !inside(b1, Ivl::from(v1) + f1) {
                    return Ok(None);
                }
            }
        }
        Ok(Some(normal))
    }

    /// Krawczyk's test for a support's points at the distances `r` from the root over the
    /// piece `t` of the leader: the point of the support in the plane square to the leader
    /// through `e(t)` at the distance `ρ` from it, for every `t` in `t` and `ρ` in `r`, about
    /// the path `x(t, ρ) = mid + along (t − tm) + out (ρ − rm) + δ` on the form
    /// ([`FootBlend::prove_piece`]'s test with the distance a range too, its equations
    /// centred in both: `G₁ = w·e′`, `G₂ = |w|² − ρ²`, their rates in `ρ` `w_ρ·e′` and
    /// `2 w·w_ρ − 2ρ`, `w_ρ = S_u out_u + S_v out_v`). Proved, Krawczyk's box of `δ` (one point
    /// in it for every `t` and `ρ`) and the box the test ran on (that point the only one in
    /// it); `None` where it is not proved.
    ///
    /// # Errors
    ///
    /// Those of the evaluations and the enclosures.
    #[allow(clippy::too_many_arguments)]
    fn prove_box(
        support: &ProofSupport,
        leader: &crate::blend::fillet_section::certificate::NurbsCurve,
        t: [f64; 2],
        tm: f64,
        r: [f64; 2],
        rm: f64,
        mid: [f64; 2],
        along: [f64; 2],
        out: [f64; 2],
    ) -> Result<Option<([Ivl; 2], [Ivl; 2])>> {
        use crate::blend::fillet_section::certificate::curve_derivs_over;
        use crate::blend::fillet_section::series::{v_add, v_scale, V3};
        let [lo, hi] = t;
        let [r0, r1] = r;
        let dt = Ivl::new(lo, hi) - Ivl::from(tm);
        let rho = Ivl::new(r0, r1);
        let dr = rho - Ivl::from(rm);
        let at_mid = curve_derivs_over(leader, Ivl::from(tm), 1, None)?;
        let over = curve_derivs_over(leader, Ivl::new(lo, hi), 2, None)?;
        let (Some(&em), Some(&dm), Some(&d), Some(&dd)) =
            (at_mid.first(), at_mid.get(1), over.get(1), over.get(2))
        else {
            return Ok(None);
        };
        let [m0, m1] = mid.map(Ivl::from);
        let [s0, s1] = along.map(Ivl::from);
        let [q0, q1] = out.map(Ivl::from);
        let two = Ivl::from(2.0);
        // w over the δ box, the piece and the step, centred at (t̃, rm); its rates along the
        // joint and out along the leg; S's first partials over the swept box; w at (t̃, rm)
        let centred = |delta: [Ivl; 2]| -> Result<Option<[V3<Ivl>; 6]>> {
            let [d0, d1] = delta;
            let Some(here) = support.partials([m0 + d0, m1 + d1], 0)? else {
                return Ok(None);
            };
            let swept = [m0 + d0 + s0 * dt + q0 * dr, m1 + d1 + s1 * dt + q1 * dr];
            let Some(p) = support.partials(swept, 1)? else {
                return Ok(None);
            };
            let (su, sv) = (p.at(1, 0), p.at(0, 1));
            let rate = isub(v_add(v_scale(su, s0), v_scale(sv, s1)), d);
            let outward = v_add(v_scale(su, q0), v_scale(sv, q1));
            let wm = isub(here.at(0, 0), em);
            let w = v_add(v_add(wm, v_scale(rate, dt)), v_scale(outward, dr));
            Ok(Some([w, rate, outward, su, sv, wm]))
        };
        let rr = Ivl::from(rm) * Ivl::from(rm);
        let value = |delta: [Ivl; 2]| -> Result<Option<[Ivl; 2]>> {
            Ok(centred(delta)?.map(|[w, rate, outward, _, _, wm]| {
                let g1 = idot(wm, dm) + (idot(rate, d) + idot(w, dd)) * dt + idot(outward, d) * dr;
                let g2 = idot(wm, wm) - rr
                    + two * idot(w, rate) * dt
                    + (two * idot(w, outward) - two * rho) * dr;
                [g1, g2]
            }))
        };
        let jacobian = |delta: [Ivl; 2]| -> Result<Option<[[Ivl; 2]; 2]>> {
            Ok(centred(delta)?.map(|[w, _, _, su, sv, _]| {
                [
                    [idot(su, d), two * idot(w, su)],
                    [idot(sv, d), two * idot(w, sv)],
                ]
            }))
        };
        krawczyk_about([0.0; 2], &value, &jacobian)
    }

    /// The failure of a span from `a` to `b` that stays unproved: no toe at the leg where the
    /// foot's equations are near singular at either end (the toes on the leg's circle meet),
    /// with its fit over the stations `solved`; else [`BlendFailure::NotCertified`].
    fn unproved(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        a: &FootStation,
        b: &FootStation,
        solved: &[FootStation],
        proof: FeetProof,
    ) -> Result<SolidError> {
        for s in [a, b] {
            for (i, side) in self.sides.iter().enumerate() {
                let (Some(&uv), Some(&foot)) = (s.uv.get(i), s.feet.get(i)) else {
                    continue;
                };
                let d = side
                    .surface
                    .evaluate(k, uv, 1, 1, EvaluateFlags::default())?;
                let (Some(&du), Some(&dv)) = (
                    d.get(1).and_then(|r| r.first()),
                    d.first().and_then(|r| r.get(1)),
                ) else {
                    continue;
                };
                let w = foot - s.root;
                let (a11, a12) = (du.dot(s.tangent), dv.dot(s.tangent));
                let (a21, a22) = (2.0 * w.dot(du), 2.0 * w.dot(dv));
                let det = a11 * a22 - a12 * a21;
                let scale = (a11.hypot(a21)) * (a12.hypot(a22));
                if det.abs() <= 1e-6 * scale {
                    let ts: Vec<f64> = solved.iter().map(|s| s.t).collect();
                    let miss = Miss::Foot {
                        t: s.t,
                        side: i,
                        kind: MissKind::NoToe,
                    };
                    return self.refusal(k, miss, &ts, legs, 0.0);
                }
            }
        }
        Ok(SolidError::blend(
            OP,
            BlendFailure::NotCertified {
                halvings: proof.halvings,
                stations: solved.len(),
            },
        ))
    }
}

/// The highest power of the leader's parameter [`FootBlend::feet_series`] encloses: the cubic
/// Hermite remainder of a section's rows over a piece takes their fourth derivative.
pub const FEET_SERIES_ORDER: usize = 4;

/// A foot's Taylor coefficients in the leader's parameter, `a_k = a⁽ᵏ⁾ / k!` for
/// `k <= FEET_SERIES_ORDER`, each enclosed over a piece.
pub type FootSeries = [crate::blend::fillet_section::series::V3<Ivl>; FEET_SERIES_ORDER + 1];

impl FootBlend {
    /// Each foot's Taylor coefficients in the leader's parameter `t` ([`FootSeries`]) for
    /// every `t` in `[ta, tb]`, with the legs `legs`: the span cut into pieces as
    /// [`FootBlend::prove_feet`] cuts it (no longer than a share of the smaller leg, a piece
    /// not proved halved), each piece's feet proved, then the coefficients of the foot's
    /// parameters solved order by order from the foot's equations as series (`G₁ = w·e′`,
    /// `G₂ = |w|² − z²`, `w = S(x(t)) − e(t)`), each in intervals over the piece's box, the
    /// foot's point composed from them, and the pieces' enclosures hulled. `None` where a
    /// piece stays unproved, the span runs across a knot of the leader, a foot's box across
    /// a knot line of a support that is not analytic (its form is only as smooth as its knots
    /// allow there), or the equations' linear part is not proved invertible over a box. Feet
    /// on a support with no NURBS form are not enclosed (`None` too).
    ///
    /// # Errors
    ///
    /// Native invalid input for a leg that is not finite and positive; those of the
    /// evaluations and the enclosures.
    pub fn feet_series(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        ta: f64,
        tb: f64,
    ) -> Result<Option<[FootSeries; 2]>> {
        use crate::blend::fillet_section::certificate::NurbsCurve;
        use crate::blend::fillet_section::series::v_hull;
        use crate::surface::surface::p2::SurfaceKind;
        check_legs(legs)?;
        let (lo, hi) = (ta.min(tb), ta.max(tb));
        if hi - lo <= 0.0
            || !(hi - lo).is_finite()
            || self.breaks()?.iter().any(|&x| lo < x && x < hi)
        {
            return Ok(None);
        }
        let leader = NurbsCurve::of(&self.leader)?;
        let mut supports = Vec::new();
        for side in &self.sides {
            let Some(support) = ProofSupport::of(k, &side.surface)? else {
                return Ok(None);
            };
            let analytic = matches!(
                side.surface.kind(),
                SurfaceKind::Plane(_)
                    | SurfaceKind::Cone(_)
                    | SurfaceKind::Sphere(_)
                    | SurfaceKind::Torus(_)
            );
            supports.push((support, analytic));
        }
        let Ok(first) = self.try_station(k, lo, legs, None)? else {
            return Ok(None);
        };
        let Ok(last) = self.try_station(k, hi, legs, Some(&first))? else {
            return Ok(None);
        };
        // pieces as prove_feet cuts a span, each from a work list with its depth
        let reach = legs.iter().fold(f64::INFINITY, |m, z| m.min(*z)) * PIECE_OF_LEG;
        let count = (first.root.distance(last.root) / reach)
            .ceil()
            .clamp(1.0, MAX_PIECES) as usize;
        let mut work: Vec<(FootStation, FootStation, usize)> = Vec::new();
        let mut from = first;
        for j in 1..=count {
            let to = if j == count {
                last
            } else {
                let t = lo + (hi - lo) * j as f64 / count as f64;
                let Ok(s) = self.try_station(k, t, legs, Some(&from))? else {
                    return Ok(None);
                };
                s
            };
            work.push((from, to, 0));
            from = to;
        }
        work.reverse();
        let mut hull: [Option<FootSeries>; 2] = [None, None];
        while let Some((a, b, depth)) = work.pop() {
            let mut made: Vec<FootSeries> = Vec::new();
            for (i, (support, analytic)) in supports.iter().enumerate() {
                let (Some(&z), Some(&fa), Some(&fb)) = (legs.get(i), a.feet.get(i), b.feet.get(i))
                else {
                    return Err(defect("a station lacked the foot of a support it is solved on, though every station is built with both"));
                };
                let proved =
                    Self::prove_piece(k, support, &leader, a.t, b.t, z, fa, fb, &mut None)?;
                let crosses =
                    |iv: Ivl, knots: &[f64]| knots.iter().any(|&u| iv.lo < u && u < iv.hi);
                let series = match proved {
                    Some(([xu, xv], ..))
                        if !analytic
                            && (crosses(xu, &support.cn.u) || crosses(xv, &support.cn.v)) =>
                    {
                        return Ok(None);
                    }
                    Some((x, w0, _)) => {
                        foot_series_over(support, &leader, Ivl::new(a.t, b.t), x, w0)?
                    }
                    None => None,
                };
                match series {
                    Some(s) => made.push(s),
                    None => break,
                }
            }
            if let [s1, s2] = made.as_slice() {
                for (h, s) in hull.iter_mut().zip([*s1, *s2]) {
                    *h = Some(match h.take() {
                        None => s,
                        Some(old) => {
                            let mut out = old;
                            for (o, n) in out.iter_mut().zip(s) {
                                *o = v_hull(*o, n);
                            }
                            out
                        }
                    });
                }
                continue;
            }
            if depth >= PROOF_HALVINGS {
                return Ok(None);
            }
            let Ok(mid) = self.try_station(k, 0.5 * (a.t + b.t), legs, Some(&a))? else {
                return Ok(None);
            };
            work.push((mid, b, depth + 1));
            work.push((a, mid, depth + 1));
        }
        Ok(match hull {
            [Some(f1), Some(f2)] => Some([f1, f2]),
            _ => None,
        })
    }
}

/// The share of a span [`FootBlend::chord_net`] reads each end's rate over (one-sided: a
/// station on a knot of the leader ends two spans whose rates differ).
const RATE_PIECE: f64 = 1e-6;

/// The rounding [`FootBlend::chord_net`] adds to its bound, relative to `1 +` the net's
/// largest coordinate: the control points' few operations and the net's evaluation, far
/// below it (definitions-and-certificates.md §4.4's floors are about `1e-10` at part scale).
const NET_ROUNDING: f64 = 1e-10;

/// `|v|` over an enclosure: the root of the sum of its components' largest squares.
fn ivl_norm(v: crate::blend::fillet_section::series::V3<Ivl>) -> f64 {
    let m = [v.x.mag(), v.y.mag(), v.z.mag()];
    (m.iter().map(|c| c * c).sum::<f64>()).sqrt() * (1.0 + 4.0 * f64::EPSILON)
}

/// The middle of an enclosure, and its largest distance from it.
fn ivl_mid(v: crate::blend::fillet_section::series::V3<Ivl>) -> (Vec3, f64) {
    let mid = Vec3::new(v.x.mid(), v.y.mid(), v.z.mid());
    let r = [v.x, v.y, v.z]
        .iter()
        .zip([mid.x, mid.y, mid.z])
        .map(|(c, m)| (c.hi - m).max(m - c.lo))
        .map(|d| d * d)
        .sum::<f64>()
        .sqrt();
    (mid, r * (1.0 + 4.0 * f64::EPSILON))
}

/// The largest distance of a point from `p` in an enclosure.
fn ivl_off(v: crate::blend::fillet_section::series::V3<Ivl>, p: Point3) -> f64 {
    [(v.x, p.x), (v.y, p.y), (v.z, p.z)]
        .iter()
        .map(|(c, x)| (c.hi - x).abs().max((x - c.lo).abs()))
        .map(|d| d * d)
        .sum::<f64>()
        .sqrt()
        * (1.0 + 4.0 * f64::EPSILON)
}

/// What [`FootBlend::chord_net`] made: the net, its sides and its bound; or the bound of the
/// first span past the tolerance (the net is not made); or nothing, a span's series not
/// enclosed.
#[allow(clippy::large_enum_variant)] // one made per round of stations, never stored
enum Chord {
    Net(BSplineSurface, [BSplineCurve; 2], f64),
    Over(f64),
    NotEnclosed,
}

impl FootBlend {
    /// The chord's section face certified over the stations `solved` (in order along the
    /// leader, legs `legs`): on each span `[t_i, t_(i+1)]`, length `h`, each side of the net is
    /// the cubic Hermite through the stations' feet with each end's rate, the middle of its
    /// foot's first coefficient over a sliver of the span at that end
    /// ([`FootBlend::feet_series`] over [`RATE_PIECE`] of it); the net is linear across between
    /// its sides, as the chord is between its feet, so it is within the larger side's bound of
    /// the true section. A side's bound on the span is the Hermite remainder `h⁴/384 · max
    /// |a⁗| = h⁴/16 · max |a_4|` (the foot's series over the span) plus the net's distance from
    /// the Hermite of the true feet and rates (at each end the foot's distance from its series'
    /// enclosure plus `h/3` times the rate's distance from its own: the control points move by
    /// those, and the Bernstein basis sums to 1) plus [`NET_ROUNDING`]. The stations sit at
    /// the leader's knots, so no span runs across one. The net (u the leader's parameter,
    /// cubic, each station a knot of multiplicity 3: C1 within a span of the leader, C0 at
    /// its knots; v from foot 1 to foot 2, linear), its two sides, and the bound: the largest
    /// of the spans' ([`Chord::Net`]); the first span's bound past `tol` ([`Chord::Over`]: the
    /// rest are not enclosed); [`Chord::NotEnclosed`] where a span's series are not.
    ///
    /// # Errors
    ///
    /// Those of the series and of the net's construction.
    fn chord_net(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        solved: &[FootStation],
        tol: f64,
    ) -> Result<Chord> {
        use crate::nurbs::b_spline_curve::{BSplineCurveForm, CanonicalData, KnotType};
        use crate::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData};
        let mut sides: [Vec<Point3>; 2] = [Vec::new(), Vec::new()];
        let mut knots = Vec::new();
        let mut mults = Vec::new();
        let mut bound = 0.0_f64;
        let mut size = 0.0_f64;
        for pair in solved.windows(2) {
            let &[a, b] = pair else { continue };
            let h = b.t - a.t;
            let sliver = RATE_PIECE * h;
            let (Some(whole), Some(start), Some(end)) = (
                self.feet_series(k, legs, a.t, b.t)?,
                self.feet_series(k, legs, a.t, a.t + sliver)?,
                self.feet_series(k, legs, b.t - sliver, b.t)?,
            ) else {
                return Ok(Chord::NotEnclosed);
            };
            if knots.is_empty() {
                knots.push(a.t);
                mults.push(4);
            } else if let Some(m) = mults.last_mut() {
                *m = 3;
            }
            knots.push(b.t);
            mults.push(4);
            for (i, side) in sides.iter_mut().enumerate() {
                let (Some(w), Some(s0), Some(s1), Some(&fa), Some(&fb)) = (
                    whole.get(i),
                    start.get(i),
                    end.get(i),
                    a.feet.get(i),
                    b.feet.get(i),
                ) else {
                    return Err(defect("a station or a series lacked the foot of a support, though both are made for the two"));
                };
                let (Some(&a4), Some(&p0), Some(&r0), Some(&p1), Some(&r1)) =
                    (w.get(4), s0.first(), s0.get(1), s1.first(), s1.get(1))
                else {
                    return Err(defect("a foot's series lacked an order it is made to"));
                };
                let (rate_a, da) = ivl_mid(r0);
                let (rate_b, db) = ivl_mid(r1);
                let ends = (ivl_off(p0, fa) + h / 3.0 * da).max(ivl_off(p1, fb) + h / 3.0 * db);
                let hermite = h.powi(4) / 16.0 * ivl_norm(a4);
                bound = bound.max(hermite + ends);
                if bound > tol {
                    return Ok(Chord::Over(bound));
                }
                if side.is_empty() {
                    side.push(fa);
                }
                side.push(fa + rate_a * (h / 3.0));
                side.push(fb - rate_b * (h / 3.0));
                side.push(fb);
                size = size.max(fa.max_dimension()).max(fb.max_dimension());
            }
        }
        let [s1, s2] = &sides;
        if s1.len() < 4 || s1.len() != s2.len() {
            return Ok(Chord::NotEnclosed);
        }
        let bound = bound + NET_ROUNDING * (1.0 + size);
        let curve = |points: &[Point3]| {
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 3,
                control_points: points,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &mults,
                knots: &knots,
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
        };
        let mut points = Vec::new();
        for (p, q) in s1.iter().zip(s2) {
            points.push(*p);
            points.push(*q);
        }
        let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: 1,
            control_points: &points,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &mults,
            v_multiplicities: &[2, 2],
            u_knots: &knots,
            v_knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            uv_domain: None,
        })?;
        Ok(Chord::Net(net, [curve(s1)?, curve(s2)?], bound))
    }
}

/// One row of a section, as series in the leader's parameter about a point `c₀`: its point
/// less `c₀`, times its weight, and its weight.
type RowSeries = (
    crate::blend::fillet_section::series::VSer<Ivl>,
    crate::blend::fillet_section::series::Ser<Ivl>,
);

/// The arc's five rows (`BeadArc::rows`: foot, corner, apex, corner, foot, weights `[1, w, 1,
/// w, 1]`) as series over a piece, from the feet's series `feet` and the leader's
/// derivatives `d` over it (at least to order `FEET_SERIES_ORDER + 1`), about `about`, for
/// the sagitta `sagitta` on the side `side` (`±1`: the sign of `(e′ × c)·(m − e)` there). In
/// their algebraic form, which is `BeadArc`'s: with `c = b − a` and `L = |c|`, the apex is
/// `m + side · h (e′ × c) / |e′ × c|` (`m` the chord's middle), the corners are the apex
/// `∓ c (1/4 + h²/L²)` (where the arc's tangents at its ends meet the one at its apex, at
/// `R tan(φ)` from it, `tan φ = 2|h|/L`), and `w = cos φ = 1/√(1 + 4h²/L²)`. `None` where `L`
/// or `|e′ × c|` is not proved positive.
///
/// # Errors
///
/// Those of the series.
fn arc_rows_series(
    feet: &[FootSeries; 2],
    d: &[crate::blend::fillet_section::series::V3<Ivl>],
    side: f64,
    sagitta: f64,
    about: Point3,
) -> Result<Option<[RowSeries; 5]>> {
    use crate::blend::fillet_section::series::{Ser, Trunc, VSer, V3};
    const N: usize = FEET_SERIES_ORDER;
    let tr = Trunc::new(N, 0)?;
    let zero = V3 {
        x: Ivl::from(0.0),
        y: Ivl::from(0.0),
        z: Ivl::from(0.0),
    };
    let one = || Ser::constant(Ivl::from(1.0));
    let of = |f: &FootSeries| {
        let mut v = VSer::constant(zero);
        for (kk, c) in f.iter().enumerate() {
            let at = if kk == 0 {
                [
                    c.x - Ivl::from(about.x),
                    c.y - Ivl::from(about.y),
                    c.z - Ivl::from(about.z),
                ]
            } else {
                [c.x, c.y, c.z]
            };
            for (comp, x) in v.a.iter_mut().zip(at) {
                comp.set(kk, 0, x);
            }
        }
        v
    };
    let [fa, fb] = feet;
    let (a, b) = (of(fa), of(fb));
    // the leader's rate: e′_k = D_(k+1) / k!
    let mut de = VSer::constant(zero);
    let mut fact = 1.0_f64;
    for kk in 0..=N {
        if kk > 0 {
            fact *= kk as f64;
        }
        let Some(dk) = d.get(kk + 1) else {
            return Ok(None);
        };
        let f = Ivl::from(1.0) / Ivl::from(fact);
        for (comp, x) in de.a.iter_mut().zip([dk.x, dk.y, dk.z]) {
            comp.set(kk, 0, x * f);
        }
    }
    let c = b.sub(&a, tr);
    let l2 = c.dot(&c, tr);
    let u = de.cross(&c, tr);
    let uu = u.dot(&u, tr);
    if !(l2.at(0, 0).is_positive() && uu.at(0, 0).is_positive()) {
        return Ok(None);
    }
    let inv_l = l2.inv_sqrt(tr)?;
    let toward = u.times(&uu.inv_sqrt(tr)?, tr);
    let half = Ser::constant(Ivl::from(0.5));
    let apex = a.add(&b, tr).times(&half, tr).add(
        &toward.times(&Ser::constant(Ivl::from(side * sagitta)), tr),
        tr,
    );
    let h2 = Ivl::from(sagitta) * Ivl::from(sagitta);
    let r = inv_l.mul(&inv_l, tr).scale(h2, tr);
    let q = Ser::constant(Ivl::from(0.25)).add(&r, tr);
    let w = one().add(&r.scale(Ivl::from(4.0), tr), tr).inv_sqrt(tr)?;
    let cq = c.times(&q, tr);
    let p1 = apex.sub(&cq, tr).times(&w, tr);
    let p3 = apex.add(&cq, tr).times(&w, tr);
    Ok(Some([
        (a, one()),
        (p1, w.clone()),
        (apex, one()),
        (p3, w),
        (b, one()),
    ]))
}

/// The arc's five rows at one station, as [`arc_rows_series`] makes them (its constant
/// terms), from the feet `fa`, `fb`, the leader's direction `along` and the root `root`; and
/// the side `±1` (`None` where the chord or `along × c` vanishes).
fn arc_rows_at(
    fa: Point3,
    fb: Point3,
    along: Vec3,
    root: Point3,
    sagitta: f64,
) -> Option<([Point3; 5], [f64; 5], f64)> {
    let c = fb - fa;
    let l2 = c.dot(c);
    let u = along.cross(c);
    let m = fa + c * 0.5;
    let toward = u.unitize().ok()?;
    let side = if toward.dot(m - root) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    if l2.is_nan() || l2 <= 0.0 {
        return None;
    }
    let apex = m + toward * (side * sagitta);
    let r = sagitta * sagitta / l2;
    let q = 0.25 + r;
    let w = 1.0 / (1.0 + 4.0 * r).sqrt();
    Some((
        [fa, apex - c * q, apex, apex + c * q, fb],
        [1.0, w, 1.0, w, 1.0],
        side,
    ))
}

impl FootBlend {
    /// The arc's section face certified over the stations `solved` (legs `legs`, the sagitta
    /// `sagitta`), as [`FootBlend::chord_net`] certifies the chord's, on its five rows in
    /// homogeneous form about each span's first root `c₀` ([`arc_rows_series`]): each row's
    /// point and weight the cubic Hermite through the stations' rows and their one-sided
    /// rates; on a span, with `E_p` and `E_w` the largest of the rows' point and weight bounds
    /// (each the Hermite remainder `h⁴/16 · max |r_4|`, plus the stations' rows' and rates'
    /// distance from their enclosures), the net is within `(E_p + |S − c₀| E_w) / W` of the
    /// true section, `W` the least weight (the rows' least less `E_w`), `|S − c₀|` the largest
    /// row's reach (the arc lies in its rows' hull), plus [`NET_ROUNDING`]. The net: u cubic
    /// (as the chord's), v the arc's two rational quadratic pieces. [`Chord::Over`] at the
    /// first span past `tol`, or with an infinite bound where a span is too long for its
    /// rows' enclosure (a weight or a length not proved positive): more stations narrow it;
    /// [`Chord::NotEnclosed`] where a span's feet's series are not enclosed or its side is not
    /// decided.
    ///
    /// # Errors
    ///
    /// Those of the series, the leader's derivatives and the net's construction.
    fn arc_net(
        &self,
        k: &Kernel,
        legs: [f64; 2],
        solved: &[FootStation],
        sagitta: f64,
        tol: f64,
    ) -> Result<Chord> {
        use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
        use crate::nurbs::b_spline_curve::{BSplineCurveForm, CanonicalData, KnotType};
        use crate::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData};
        let leader = NurbsCurve::of(&self.leader)?;
        // per row: the net's control points (in model space) and weights along u
        let mut rows: [(Vec<Point3>, Vec<f64>); 5] = Default::default();
        let mut knots = Vec::new();
        let mut mults = Vec::new();
        let mut bound = 0.0_f64;
        let mut size = 0.0_f64;
        for pair in solved.windows(2) {
            let &[a, b] = pair else { continue };
            let h = b.t - a.t;
            let sliver = RATE_PIECE * h;
            let (Some(at_a), Some(at_b)) = (
                arc_rows_at(a.feet[0], a.feet[1], a.tangent, a.root, sagitta),
                arc_rows_at(b.feet[0], b.feet[1], b.tangent, b.root, sagitta),
            ) else {
                return Ok(Chord::NotEnclosed);
            };
            let side = at_a.2;
            if at_b.2 != side {
                return Ok(Chord::NotEnclosed);
            }
            let about = a.root;
            let mut series = Vec::new();
            for (lo, hi) in [(a.t, b.t), (a.t, a.t + sliver), (b.t - sliver, b.t)] {
                let Some(feet) = self.feet_series(k, legs, lo, hi)? else {
                    return Ok(Chord::NotEnclosed);
                };
                let d = curve_derivs_over(&leader, Ivl::new(lo, hi), FEET_SERIES_ORDER + 1, None)?;
                // a span too long for its rows' enclosure: more stations
                let Some(r) = arc_rows_series(&feet, &d, side, sagitta, about)? else {
                    return Ok(Chord::Over(f64::INFINITY));
                };
                series.push(r);
            }
            let Ok([whole, start, end]) = <[[RowSeries; 5]; 3]>::try_from(series) else {
                return Err(defect("the arc's rows were enclosed over the span and its two ends, three pieces, but not three came back"));
            };
            if knots.is_empty() {
                knots.push(a.t);
                mults.push(4);
            } else if let Some(m) = mults.last_mut() {
                *m = 3;
            }
            knots.push(b.t);
            mults.push(4);
            let pt = |v: &crate::blend::fillet_section::series::VSer<Ivl>, kk: usize| {
                let [x, y, z] = &v.a;
                crate::blend::fillet_section::series::V3 {
                    x: x.at(kk, 0),
                    y: y.at(kk, 0),
                    z: z.at(kk, 0),
                }
            };
            let (mut e_p, mut e_w, mut reach, mut w_min) =
                (0.0_f64, 0.0_f64, 0.0_f64, f64::INFINITY);
            let ends = [(&start, &at_a), (&end, &at_b)];
            // each row's Hermite control points in homogeneous form about c₀: (point, weight)
            let mut controls: [[(Vec3, f64); 4]; 5] = [[(Vec3::new(0.0, 0.0, 0.0), 0.0); 4]; 5];
            for (j, ((wp, ww), control)) in whole.iter().zip(controls.iter_mut()).enumerate() {
                // the Hermite remainder of the row's point and weight
                let hp = h.powi(4) / 16.0 * ivl_norm(pt(wp, 4));
                let hw = h.powi(4) / 16.0 * ww.at(4, 0).mag();
                let p0 = pt(wp, 0);
                let w0 = ww.at(0, 0);
                if !w0.is_positive() {
                    return Ok(Chord::Over(f64::INFINITY));
                }
                w_min = w_min.min(w0.lo);
                reach = reach.max(ivl_norm(p0) / w0.lo);
                let mut off_p = 0.0_f64;
                let mut off_w = 0.0_f64;
                let mut ends_hw =
                    [(Vec3::new(0.0, 0.0, 0.0), 0.0, Vec3::new(0.0, 0.0, 0.0), 0.0); 2];
                for ((rows_end, at), e) in ends.iter().zip(ends_hw.iter_mut()) {
                    let (Some((sp, sw)), Some(&p), Some(&wt)) =
                        (rows_end.get(j), at.0.get(j), at.1.get(j))
                    else {
                        return Err(defect("a row of the arc lacked its end's series or value, though every end has its five"));
                    };
                    // the station's row in homogeneous form about c₀
                    let ph = (p - about) * wt;
                    let (rate_p, dp) = ivl_mid(pt(sp, 1));
                    let rw = sw.at(1, 0);
                    let (rate_w, dw) = (rw.mid(), (rw.hi - rw.mid()).max(rw.mid() - rw.lo));
                    off_p = off_p.max(ivl_off(pt(sp, 0), Point3::ZERO + ph) + h / 3.0 * dp);
                    let w_enc = sw.at(0, 0);
                    off_w =
                        off_w.max((w_enc.hi - wt).abs().max((wt - w_enc.lo).abs()) + h / 3.0 * dw);
                    *e = (ph, wt, rate_p, rate_w);
                }
                e_p = e_p.max(hp + off_p);
                e_w = e_w.max(hw + off_w);
                let [(pa, wa, ra, rwa), (pb, wb, rb, rwb)] = ends_hw;
                *control = [
                    (pa, wa),
                    (pa + ra * (h / 3.0), wa + rwa * (h / 3.0)),
                    (pb - rb * (h / 3.0), wb - rwb * (h / 3.0)),
                    (pb, wb),
                ];
            }
            let least = w_min - e_w;
            if least.is_nan() || least <= 0.0 {
                return Ok(Chord::Over(f64::INFINITY));
            }
            bound = bound.max((e_p + reach * e_w) / least);
            if bound > tol {
                return Ok(Chord::Over(bound));
            }
            size = size.max(about.max_dimension() + reach);
            for ((points, weights), control) in rows.iter_mut().zip(&controls) {
                let skip = usize::from(!points.is_empty());
                for &(ph, wt) in control.iter().skip(skip) {
                    if wt.is_nan() || wt <= 0.0 {
                        return Ok(Chord::Over(f64::INFINITY));
                    }
                    points.push(about + ph * (1.0 / wt));
                    weights.push(wt);
                }
            }
        }
        let bound = bound + NET_ROUNDING * (1.0 + size);
        let [(first, _), .., (last, _)] = &rows;
        if first.len() < 4 {
            return Ok(Chord::NotEnclosed);
        }
        let mut points = Vec::new();
        let mut weights = Vec::new();
        for i in 0..first.len() {
            for (p, w) in &rows {
                let (Some(&pi), Some(&wi)) = (p.get(i), w.get(i)) else {
                    return Err(defect("a row of the arc's net had fewer control points than the others, though every span adds the same to each"));
                };
                points.push(pi);
                weights.push(wi);
            }
        }
        let curve = |points: &[Point3]| {
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 3,
                control_points: points,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &mults,
                knots: &knots,
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
        };
        let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
            u_degree: 3,
            v_degree: 2,
            control_points: &points,
            form: BSplineSurfaceForm::Unspecified,
            u_multiplicities: &mults,
            v_multiplicities: &[3, 2, 3],
            u_knots: &knots,
            v_knots: &[0.0, 0.5, 1.0],
            knot_type: KnotType::Unspecified,
            weights: Some(&weights),
            uv_domain: None,
        })?;
        Ok(Chord::Net(net, [curve(first)?, curve(last)?], bound))
    }
}

/// A foot's [`FootSeries`] over the leader's parameters `t`, `x` a proved box of the foot's
/// parameters on `support`'s form for every `t` there and `w0` an enclosure of `w = S − e`
/// there (the centred one of the piece's proof: `w` over the box would count the toe's and
/// the root's travel twice, and with it the linear part would not be proved invertible): the
/// parameters'
/// coefficients `x_k` (`k >= 1`) solved from `J x_k = −r_k`, `r_k` the equations' `k`th
/// coefficient with `x_k` left 0 and `J` their linear part, enclosed by `x_k ∈ −C r_k +
/// (I − C J) x_k` with `C` the inverse of `J`'s middle and `‖x_k‖ <= ‖C r_k‖ / (1 − q)`,
/// `q = ‖I − C J‖ < 1`; then the foot's point `S(x(t))` composed (`Σ P_ab Δu^a Δv^b / a! b!`
/// over the partials `P` enclosed on the box). Each coefficient holds the true one at every
/// `t` in `t`, the foot there being in the box. `None` where the partials are not enclosed
/// or `q` is not proved below 1.
///
/// # Errors
///
/// Those of the enclosures and the series.
fn foot_series_over(
    support: &ProofSupport,
    leader: &crate::blend::fillet_section::certificate::NurbsCurve,
    t: Ivl,
    x: [Ivl; 2],
    w0: crate::blend::fillet_section::series::V3<Ivl>,
) -> Result<Option<FootSeries>> {
    use crate::blend::fillet_section::certificate::curve_derivs_over;
    use crate::blend::fillet_section::series::{Ser, Trunc, VSer, V3};
    use crate::geom::krawczyk::inverse_of_columns2;
    const N: usize = FEET_SERIES_ORDER;
    let tr = Trunc::new(N, 0)?;
    let zero = V3 {
        x: Ivl::from(0.0),
        y: Ivl::from(0.0),
        z: Ivl::from(0.0),
    };
    let Some(p) = support.partials(x, N)? else {
        return Ok(None);
    };
    let d = curve_derivs_over(leader, t, N + 1, None)?;
    // the leader e and its rate e′ as series: e_k = D_k / k!, e′_k = D_(k+1) / k!
    let mut e = VSer::constant(zero);
    let mut de = VSer::constant(zero);
    let mut fact = 1.0_f64;
    for kk in 0..=N {
        if kk > 0 {
            fact *= kk as f64;
        }
        let (Some(dk), Some(dk1)) = (d.get(kk), d.get(kk + 1)) else {
            return Ok(None);
        };
        let f = Ivl::from(1.0) / Ivl::from(fact);
        for (ser, v) in [(&mut e, dk), (&mut de, dk1)] {
            for (c, vj) in ser.a.iter_mut().zip([v.x, v.y, v.z]) {
                c.set(kk, 0, vj * f);
            }
        }
    }
    // S(x0 + Δ) to order N, Δ = (du, dv) without constant terms, from the partials on the box
    let compose = |du: &Ser<Ivl>, dv: &Ser<Ivl>| -> VSer<Ivl> {
        let mut pu = vec![Ser::constant(Ivl::from(1.0))];
        let mut pv = vec![Ser::constant(Ivl::from(1.0))];
        for a in 1..=N {
            if let (Some(lu), Some(lv)) = (pu.last().cloned(), pv.last().cloned()) {
                pu.push(lu.mul_valued(du, a - 1, 1, tr));
                pv.push(lv.mul_valued(dv, a - 1, 1, tr));
            }
        }
        let mut s = VSer::constant(zero);
        let mut fa = 1.0_f64;
        for (a, pua) in pu.iter().enumerate() {
            if a > 0 {
                fa *= a as f64;
            }
            let mut fb = 1.0_f64;
            for (b, pvb) in pv.iter().enumerate().take(N - a + 1) {
                if b > 0 {
                    fb *= b as f64;
                }
                let mono = pua.mul_valued(pvb, a, b, tr);
                let f = Ivl::from(1.0) / Ivl::from(fa * fb);
                let c = p.at(a, b);
                for (o, cj) in s.a.iter_mut().zip([c.x, c.y, c.z]) {
                    o.add_scaled(&mono, cj * f, tr);
                }
            }
        }
        s
    };
    let Some(&e1) = d.get(1) else {
        return Ok(None);
    };
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    let two = Ivl::from(2.0);
    // the linear part by columns (in x_u, then x_v), and its middle's inverse by rows
    let j = [
        [idot(su, e1), two * idot(w0, su)],
        [idot(sv, e1), two * idot(w0, sv)],
    ];
    let Some(c) = inverse_of_columns2(&j.map(|col| col.map(Ivl::mid))) else {
        return Ok(None);
    };
    // I − C J, by rows, and q its row norm
    let mut m = [[Ivl::from(0.0); 2]; 2];
    let mut q = 0.0_f64;
    for (row, (mi, ci)) in m.iter_mut().zip(c).enumerate() {
        let mut sum = 0.0_f64;
        for (col, (mij, jc)) in mi.iter_mut().zip(j).enumerate() {
            let cj = Ivl::from(ci[0]) * jc[0] + Ivl::from(ci[1]) * jc[1];
            let id = if row == col { 1.0 } else { 0.0 };
            *mij = Ivl::from(id) - cj;
            sum += mij.mag();
        }
        q = q.max(sum);
    }
    // rounded up: the sum of magnitudes, and 1 − q below
    let q = crate::blend::fillet_section::interval::round_up(q * (1.0 + 4.0 * f64::EPSILON));
    if !q.is_finite() || q >= 1.0 {
        return Ok(None);
    }
    let mut du = Ser::constant(Ivl::from(0.0));
    let mut dv = Ser::constant(Ivl::from(0.0));
    for kk in 1..=N {
        let mut w = compose(&du, &dv).sub(&e, tr);
        // w's constant term the centred one
        for (c, v) in w.a.iter_mut().zip([w0.x, w0.y, w0.z]) {
            c.set(0, 0, v);
        }
        let r = [w.dot(&de, tr).at(kk, 0), w.dot(&w, tr).at(kk, 0)];
        let cr = c.map(|ci| Ivl::from(ci[0]) * r[0] + Ivl::from(ci[1]) * r[1]);
        let size = cr.iter().fold(0.0_f64, |a, x| a.max(x.mag()));
        let rho = (Ivl::from(size) / (Ivl::from(1.0) - Ivl::from(q))).hi;
        if !rho.is_finite() {
            return Ok(None);
        }
        let ball = Ivl::new(-rho, rho);
        let ([cr0, cr1], [m0, m1]) = (cr, m);
        du.set(kk, 0, Ivl::from(0.0) - cr0 + m0[0] * ball + m0[1] * ball);
        dv.set(kk, 0, Ivl::from(0.0) - cr1 + m1[0] * ball + m1[1] * ball);
    }
    let s = compose(&du, &dv);
    let mut out = [zero; N + 1];
    let [sx, sy, sz] = &s.a;
    for (kk, o) in out.iter_mut().enumerate() {
        *o = V3 {
            x: sx.at(kk, 0),
            y: sy.at(kk, 0),
            z: sz.at(kk, 0),
        };
    }
    Ok(Some(out))
}
