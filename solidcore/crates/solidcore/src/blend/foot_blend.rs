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
//! station is refused.
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
//! that rebuild. The fit between the net and the true section is **sampled** (at the
//! midpoints between stations), not proved: the certificate (one root per box, Krawczyk)
//! follows.
//!
//! Not upstream (solidcore's). No recursion.

use crate::blend::weld_bead::BeadEnd;
use crate::curve::curve::Curve;
use crate::error::blend::BlendFailure;
use crate::error::{Failure, Fit, FitMeasure, Result, Site, SolidError, TargetEdge};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
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

/// The most Newton steps a foot takes.
const NEWTON_STEPS: usize = 40;

/// The most halvings the search for a leg that fits takes.
const FIT_STEPS: usize = 40;

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

impl FootBlend {
    /// The foot blend of `edge` of `target`: its curve as its B-spline form, and its two faces
    /// through the edge uses on the void's side (whose faces' normals point out of the
    /// material), each with its side read from the edge use's binormal at the middle of the
    /// edge. With `first` a face of the edge, that face is support 1. `target` is only read:
    /// it is mutable for the binormals' evaluation, asked in 3D only, which builds no uv trim
    /// curve.
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
                    .ok_or_else(|| SolidError::blend(OP, BlendFailure::StageFailed))
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
            let into = evaluate_binormal(k, target, eu, middle, true, &BinormalOptions::default())?;
            let across = normal_at(k, &surface, into.uv)?
                .map(|n| n.cross(tangent))
                .ok_or_else(|| SolidError::blend(OP, BlendFailure::StageFailed))?;
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
            return Err(SolidError::blend(OP, BlendFailure::StageFailed));
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
                    MissKind::Stage => Failure::new(OP, BlendFailure::StageFailed).or_at(site(at)),
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
    /// parameters `ts` (in order, each from the one before): halved down from `z`. 0 where
    /// no leg is found at all.
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
        let (mut lo, mut hi) = (0.0_f64, z);
        for _ in 0..FIT_STEPS {
            if hi - lo <= 1e-7 * (1.0 + z) {
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
    /// in the station's plane.
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
                return Err(SolidError::blend(OP, BlendFailure::StageFailed));
            };
            let n = normal_at(k, &side.surface, uv)?
                .ok_or_else(|| SolidError::blend(OP, BlendFailure::StageFailed))?;
            let mut d = n.cross(s.tangent);
            if d.dot(s.root - foot) < 0.0 {
                d *= -1.0;
            }
            let c = other - foot;
            *toe = c.cross(d).length().atan2(c.dot(d));
        }
        Ok(sagitta_bound(b.distance(a), toes, sagitta))
    }
}

impl FootBlend {
    /// Checks that each toe lies on its face of `target` at the legs `legs`: at stations on the
    /// leader's knots in the interval, at least 4 between each two and 32 in all (sampled),
    /// each foot's parameters
    /// classified on its face (trims included). Stations not made are skipped (the build
    /// refuses them). `target` is only read.
    ///
    /// # Errors
    ///
    /// The refusal [`crate::blend::fillet_executive::NoRoomReason::LegPastFace`] at the first
    /// toe found off its face, naming the face and the edge, with the largest leg on that side
    /// whose toes stay on the face at those stations; those of the classification.
    pub fn toes_on_faces(&self, k: &Kernel, target: &mut Brep, legs: [f64; 2]) -> Result<()> {
        use crate::intersect::point_class::PointClassType as C;
        check_legs(legs)?;
        let breaks = self.breaks()?;
        // at least 32 stations, and 4 between each two knots
        let per = 32_usize
            .div_ceil(breaks.len().saturating_sub(1).max(1))
            .max(4);
        let mut ts = Vec::with_capacity(per * breaks.len());
        for pair in breaks.windows(2) {
            let &[lo, hi] = pair else { continue };
            ts.extend((0..per).map(|i| lo + (hi - lo) * i as f64 / per as f64));
        }
        ts.extend(breaks.last().copied());
        // each support's domain and periods: a foot past a periodic support's seam is moved
        // into the domain by whole periods before it is classified
        let mut frames = Vec::with_capacity(self.sides.len());
        for side in &self.sides {
            let domain = side.surface.natural_uv_domain_with_nurb(k)?;
            let periods = periods_of(k, &side.surface, &domain)?;
            frames.push((domain, periods));
        }
        let on_face = |target: &mut Brep, i: usize, face: FaceId, uv: Point2| -> Result<bool> {
            let uv = frames
                .get(i)
                .map_or(uv, |(domain, periods)| wrapped(domain, *periods, uv));
            Ok(matches!(
                crate::query::api_queries::face_classify_uv(k, target, face, uv)?,
                C::Face | C::Edge | C::Vertex | C::Edgeuse
            ))
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
        let mut holds = |leg: f64| -> Result<bool> {
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
        let (mut lo, mut hi) = (0.0_f64, z);
        for _ in 0..FIT_STEPS {
            if hi - lo <= 1e-7 * (1.0 + z) {
                break;
            }
            let mid = 0.5 * (lo + hi);
            if holds(mid)? {
                lo = mid;
            } else {
                hi = mid;
            }
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
    /// the root within `tol`. Exact at its stations; between them that is a **sampled** check
    /// at one point per interval, not a bound: nothing is certified yet.
    ///
    /// # Errors
    ///
    /// Native invalid input for a leg that is not positive; the refusals of a station, typed,
    /// with the size that builds over the stations of the refinement refused: a foot with no
    /// toe at its leg on its face's side (the largest leg on that side that finds its toe), an
    /// arc that would cut into a face (the sagitta's least bound); a Newton that does not
    /// converge ([`BlendFailure::StationDidNotConverge`]); a net that does not hold within
    /// `tol` after its refinements ([`BlendFailure::SkinPastStationCap`]); those of the arcs
    /// and the interpolation.
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
            let mut rows = Vec::with_capacity(stations.len());
            let mut roots = Vec::with_capacity(stations.len());
            let mut solved: Vec<FootStation> = Vec::with_capacity(stations.len());
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
            let (face, feet) = net_through_rows(&stations, &rows, sagitta)?;
            let root = through_at(&roots, &stations)?;
            // sampled, not proved: at the midpoint between each two stations, the feet, the
            // section's middle and the root
            worst = 0.0_f64;
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
                for (v, want) in [(0.0, first), (0.5, middle), (1.0, last)] {
                    let got = face.evaluate_point(Point2::new(t, v))?;
                    worst = worst.max(got.distance(want));
                }
                worst = worst.max(root.evaluate_point(t)?.distance(s.root));
            }
            if worst <= tol {
                let start_uv = solved
                    .first()
                    .map_or([Point2::new(0.0, 0.0); 2], |s| s.root_uv);
                return Ok(Segment {
                    face,
                    feet,
                    root,
                    start_uv,
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
    let stage = || SolidError::blend(OP, BlendFailure::StageFailed);
    let across = if sagitta == 0.0 { 2 } else { 5 };
    let mut columns: Vec<Vec<Point3>> = vec![Vec::with_capacity(stations.len()); across];
    let mut weights: Vec<Vec<Point3>> = vec![Vec::with_capacity(stations.len()); across];
    for (points, w) in rows {
        for (j, (column, wcol)) in columns.iter_mut().zip(weights.iter_mut()).enumerate() {
            let (Some(&p), Some(&w)) = (points.get(j), w.get(j)) else {
                return Err(stage());
            };
            column.push(Point3::new(p.x * w, p.y * w, p.z * w));
            wcol.push(Point3::new(w, 0.0, 0.0));
        }
    }
    let through = |points: &[Point3]| through_at(points, stations);
    let mut curves = Vec::with_capacity(across);
    let mut wcurves = Vec::with_capacity(across);
    for (column, wcol) in columns.iter().zip(&weights) {
        curves.push(through(column)?);
        wcurves.push(through(wcol)?);
    }
    let first = curves.first().ok_or_else(stage)?;
    // the net's sides v = 0 and v = 1: the feet's columns, of weight 1
    let sides = [first.clone(), curves.last().ok_or_else(stage)?.clone()];
    let (u_knots, u_mults) = first.knots(None)?;
    let count = first.control_points()?.len();
    let mut points = Vec::with_capacity(count * across);
    let mut net_weights = Vec::with_capacity(count * across);
    for i in 0..count {
        for (c, wc) in curves.iter().zip(&wcurves) {
            let p = c.control_points()?.get(i).copied().ok_or_else(stage)?;
            let w = wc
                .control_points()?
                .get(i)
                .map(|c| c.x)
                .filter(|w| *w > 0.0)
                .ok_or_else(stage)?;
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

/// The points along a rung, root excluded: the share of each leg.
const RUNG_SHARES: [f64; 4] = [0.25, 0.5, 0.75, 1.0];

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
        let made = self.add_span(k, &mut body, legs, sagitta, tol, [lo, hi])?;
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
        let mut rungs: Vec<[BSplineCurve; 2]> = Vec::with_capacity(cuts.len());
        let mut sections: Vec<BSplineCurve> = Vec::with_capacity(cuts.len());
        for &t in &cuts {
            let (r1, r2, section) = self.cut_curves(k, t, legs, sagitta)?;
            rungs.push([r1, r2]);
            sections.push(section);
        }
        let mut made = Vec::with_capacity(pieces);
        for (j, pair) in cuts.windows(2).enumerate() {
            let &[a, b] = pair else { continue };
            let seg = self.segment(k, legs, sagitta, tol, a, b)?;
            let (Some(ra), Some(rb), Some(sa), Some(sb)) = (
                rungs.get(j),
                rungs.get(j + 1),
                sections.get(j),
                sections.get(j + 1),
            ) else {
                return Err(SolidError::blend(OP, BlendFailure::StageFailed));
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
                .ok_or_else(|| SolidError::blend(OP, BlendFailure::StageFailed))?
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
        let mut rung_points: [Vec<Point3>; 2] = [vec![full.root], vec![full.root]];
        for share in RUNG_SHARES {
            let scaled = legs.map(|z| z * share);
            let s = self.station(k, t, scaled, Some(&full))?;
            for (points, foot) in rung_points.iter_mut().zip(s.feet) {
                points.push(foot);
            }
        }
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
        let [p1, p2] = &rung_points;
        let (r1, r2) = (rung(p1)?, rung(p2)?);
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
                return Err(SolidError::blend(OP, BlendFailure::StageFailed));
            };
            let uv = Point2::new(unwrap(uv.x, prev.x, pu), unwrap(uv.y, prev.y, pv));
            uvs.push(uv);
            prev = uv;
        }
    }
    let mut area = 0.0;
    for (i, a) in uvs.iter().enumerate() {
        let Some(b) = uvs.get((i + 1) % uvs.len()) else {
            return Err(SolidError::blend(OP, BlendFailure::StageFailed));
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
        .ok_or_else(|| SolidError::blend(OP, BlendFailure::StageFailed))
}

/// Closes the weld bead faces added to `body` ([`FootBlend::add_span`]): stitched within
/// `tol` (or the body's own tolerance, if larger) into closed shells, oriented as a solid, and
/// its geometry named by its own topology, a body of its own.
///
/// # Errors
///
/// A defect ([`BlendFailure::StageFailed`]) where the faces do not close into a manifold
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
        return Err(SolidError::blend(OP, BlendFailure::StageFailed));
    }
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
    /// Native invalid input for a scale that is not uniform (as a draft's): the legs, measured
    /// straight from the root, would no longer be the net's.
    pub fn transform(
        &mut self,
        rotate_n_move: &crate::geom::axis2_placement::Axis2Placement,
        scale: Option<Vec3>,
    ) -> Result<()> {
        if let Some(sc) = scale {
            if !(sc.x == sc.y && sc.x == sc.z && sc.x > 0.0 && sc.x.is_finite()) {
                return Err(SolidError::invalid_input(
                    OP,
                    "a weld bead moves rigidly or by a uniform scale",
                ));
            }
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
