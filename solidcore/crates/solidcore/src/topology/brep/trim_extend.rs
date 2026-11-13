//! A face on a plane extended past one of its edges by growing its trim within the plane
//! ([`extend_trim`]), by a distance or, for a straight edge, through a point
//! ([`extend_trim_through`]).
//!
//! Not upstream. A plane continued past any edge is the plane itself, its own tangent and
//! curvature continuation, so extending its face moves the trim, not the surface: the picked
//! edge moves `distance` outward within the plane (a line along its in-plane normal; a curve to
//! its offset, the kernel's own: a line's exact, an arc's exact, a spline's approximated
//! within a tenth of the body's tolerance), and each neighbouring edge is carried along its
//! end tangent to meet it (a line lengthened; a curve kept, with a straight edge added along
//! its tangent). Every other edge and every inner loop is kept. The face is built again on a
//! copy of its plane (its domain widened to hold the new trim, its frame and orientation
//! kept), and the result is checked valid.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::BTreeMap;

use crate::curve::curve::p2::CurveKind;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::USE_DEFAULT;
use crate::intersect::api_intersectors::intersect_curves;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::primitive::api_curves::create_line_segment;
use crate::surface::plane::Plane;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::brep::validation::{check_brep_with_keys, CheckLevel};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as fp2;
use crate::topology::loopuse::{self, notify_ref};
use crate::topology::{Brep, EdgeId, FaceId};
use crate::EntityRef;

const OP: &str = "topology::brep::trim_extend::extend_trim";

/// How far, as a multiple of the face's size and the distance, a neighbour is carried along its
/// tangent to look for the moved edge: a neighbour meeting it at a shallower angle than about a
/// hundredth of a radian is refused.
const REACH: f64 = 100.0;

/// The face extended by [`extend_trim`]: a new one-face body.
#[derive(Debug)]
pub struct TrimExtension {
    /// The new body.
    pub brep: Brep,
    /// Its face, on a copy of the old face's plane.
    pub face: FaceId,
    /// Each old edge of the face and its edge in the new body (the picked edge's is the moved
    /// edge; a straight neighbour's the lengthened edge).
    pub edges: Vec<(EdgeId, EdgeId)>,
    /// The straight edges added along a curved neighbour's tangent.
    pub added: Vec<EdgeId>,
}

/// One use of an edge in a loop, as the loop runs: its edge, a copy of its curve over the edge's
/// interval, and whether it runs along the curve.
#[derive(Clone)]
struct Use {
    edge: EdgeId,
    curve: Curve,
    same: bool,
    interval: Extent1d,
}

impl Use {
    /// The point where the use starts, as the loop runs.
    fn start(&self, k: &Kernel) -> Result<Point3> {
        let t = if self.same {
            self.interval.min()
        } else {
            self.interval.max()
        };
        self.curve.evaluate_point(k, t)
    }

    /// The point where the use ends, as the loop runs.
    fn end(&self, k: &Kernel) -> Result<Point3> {
        let t = if self.same {
            self.interval.max()
        } else {
            self.interval.min()
        };
        self.curve.evaluate_point(k, t)
    }

    /// The unit direction the loop runs in at the use's start (`at_end` false) or end.
    fn direction(&self, k: &Kernel, at_end: bool) -> Result<Vec3> {
        let at_max = self.same == at_end;
        let t = if at_max {
            self.interval.max()
        } else {
            self.interval.min()
        };
        let d = self.curve.evaluate(k, t, 1, at_max, true)?;
        let d = *d
            .get(1)
            .ok_or_else(|| SolidError::invalid_input(OP, "a curve without a tangent"))?;
        let d = if self.same { d } else { -d };
        d.unitize()
            .map_err(|_| SolidError::invalid_input(OP, "a curve with no tangent at an end"))
    }

    fn orientation(&self) -> OrientType {
        if self.same {
            OrientType::Same
        } else {
            OrientType::Opposite
        }
    }
}

/// A curve of the new face, the old edge it continues (none for an added edge), and how the
/// loop runs along it.
struct NewCurve {
    from: Option<EdgeId>,
    curve: Curve,
    orientation: OrientType,
}

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// Whether `c` is straight: a line, or a B-spline whose control points all lie within `tol` of
/// the line through its first and last (a skin's rails and flat sections are such splines).
/// Any other curve is not.
fn is_straight(c: &Curve, tol: f64) -> Result<bool> {
    let b = match c.kind() {
        CurveKind::Line(_) => return Ok(true),
        CurveKind::BSpline(b) => b,
        _ => return Ok(false),
    };
    let (points, _) = b.control_polygon()?;
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return Ok(false);
    };
    let Ok(u) = (last - first).unitize() else {
        return Ok(false);
    };
    Ok(points.iter().all(|&p| {
        let d = p - first;
        (d - u * d.dot(u)).length() <= tol
    }))
}

/// The straight segment from `a` to `b`, run from `a`.
fn segment(a: Point3, b: Point3) -> Result<NewCurve> {
    Ok(NewCurve {
        from: None,
        curve: Curve::from(create_line_segment(a, b)?),
        orientation: OrientType::Same,
    })
}

/// The loops of face `f` as its upward use runs them, the outer loops first.
fn loops_of(k: &Kernel, b: &Brep, f: FaceId) -> Result<Vec<Vec<Use>>> {
    let fu = fp2::upward_faceuse(b, f)?
        .ok_or_else(|| SolidError::invalid_input(OP, "a face without its upward use"))?;
    let outer = fp2::outer_loops(b, f)?;
    let mut first = Vec::new();
    let mut rest = Vec::new();
    for lu in eu1::faceuse_loopuses(b, fu, false, None)? {
        let l = b
            .loopuse(lu)?
            .loop_
            .ok_or_else(|| SolidError::invalid_input(OP, "a loop use without its loop"))?;
        let mut uses = Vec::new();
        for eu in loopuse::edgeuses(b, lu, None, None)? {
            let e = eu1::edge(b, eu)?
                .ok_or_else(|| SolidError::invalid_input(OP, "an edge use without its edge"))?;
            let edge = b.edge(e)?;
            let mut curve = edge
                .curve
                .as_ref()
                .ok_or_else(|| SolidError::invalid_input(OP, "an edge without a curve"))?
                .copy(k)?;
            let mut interval = edge.interval;
            curve.trim(k, &mut interval, TrimMode::Silent)?;
            uses.push(Use {
                edge: e,
                curve,
                same: eu1::orientation(b, eu)? == OrientType::Same,
                interval,
            });
        }
        if uses.is_empty() {
            return Err(unsupported("a face with a vertex loop"));
        }
        if outer.contains(&l) {
            first.push(uses);
        } else {
            rest.push(uses);
        }
    }
    first.extend(rest);
    Ok(first)
}

/// `c` offset `distance` to its right seen from `normal` (`CreateSimpleOffset`): a line
/// exactly, an arc in the plane exactly, a spline within `tol` (refused past it).
fn offset(k: &Kernel, c: &Curve, normal: Vec3, distance: f64, tol: f64) -> Result<Curve> {
    let (made, gap) = match c.kind() {
        CurveKind::Line(l) => {
            let (l, gap) = l.create_simple_offset(normal, distance)?;
            (Curve::from(l), gap)
        }
        CurveKind::Ellipse(e) => e.create_simple_offset(k, tol, normal, distance, false)?,
        CurveKind::BSpline(s) => {
            let (s, gap) = s.create_simple_offset(k, tol, normal, distance, false)?;
            (Curve::from(s), gap)
        }
        _ => return Err(unsupported("an edge curve the kernel cannot offset")),
    };
    // (a NaN gap is not within it either)
    if !matches!(
        gap.partial_cmp(&tol),
        Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
    ) {
        // the offset of the edge is not within a tenth of the tolerance
        return Err(SolidError::NotConverged {
            operation: OP,
            iterations: 0,
        });
    }
    Ok(made)
}

/// The point where the ray from `from` along `along` (unit) for `reach` first meets `curve`,
/// past `tol` from `from`, and the parameter on `curve` there.
fn hit(
    k: &Kernel,
    from: Point3,
    along: Vec3,
    reach: f64,
    curve: &Curve,
    tol: f64,
) -> Result<(Point3, f64)> {
    let ray = Curve::from(create_line_segment(from, from + along * reach)?);
    let found = intersect_curves(k, &ray, curve, true).map_err(|_| {
        unsupported("a neighbouring edge carried along its tangent never meets the moved edge")
    })?;
    let points = found.points.unwrap_or_default();
    let mut best: Option<(f64, Point3, f64)> = None;
    for (p, t) in points.iter().zip(&found.params2) {
        let d = (*p - from).length();
        if d > tol && best.is_none_or(|(bd, _, _)| d < bd) {
            best = Some((d, *p, *t));
        }
    }
    best.map(|(_, p, t)| (p, t)).ok_or_else(|| {
        unsupported("a neighbouring edge carried along its tangent never meets the moved edge")
    })
}

/// Face `f` of `b`, on a plane, with its edge `e` (on
/// its outer loop) moved `distance` outward within the plane and its neighbouring edges carried
/// along their end tangents to meet it (module docs), as a new one-face body. `b` is unchanged.
///
/// # Errors
///
/// - `InvalidInput`: a distance not finite and positive; `e` not an edge of `f`'s outer loop;
/// - `Unsupported`: a surface that does not reach past its trims
///   ([`Surface::reaches_past_trims`]), a vertex loop, an outer loop of two edges, an edge curve
///   that cannot be offset, a neighbour that never meets the moved edge (it leaves the corner
///   along the edge's own line, or at a reflex corner into the face), neighbours that cross
///   before the distance, a moved edge that crosses the face's other edges or that the kernel's
///   check refuses;
/// - `NotConverged`: a spline edge whose offset is not within a tenth of the tolerance;
/// - those of the curve, intersection and face constructors.
pub fn extend_trim(
    k: &mut Kernel,
    b: &Brep,
    f: FaceId,
    e: EdgeId,
    distance: f64,
) -> Result<TrimExtension> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a distance not finite and positive",
        ));
    }
    let g = grown(k, b, f, e, Move::By(distance))?;
    let mut nb = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    nb.set_tolerance(g.tol);
    let (face, edges, added) = build_into(k, &mut nb, g)?;
    let check = check_brep_with_keys(k, &mut nb, CheckLevel::Level2, &notify_ref)?;
    if !check.is_valid() {
        return Err(unsupported(
            "the extended trim crosses the face's other edges or is not valid",
        ));
    }
    Ok(TrimExtension {
        brep: nb,
        face,
        edges,
        added,
    })
}

/// Face `f` of `b`, on a plane, with its straight edge `e` (on its outer loop) moved outward,
/// parallel, until it passes through `through`, its neighbouring edges carried along their end
/// tangents to meet it, as [`extend_trim`] moves it by a distance: the trim grown to a corner
/// another face has moved to (an arch's leg grown to meet its extended top). One of the carried
/// neighbours must meet the moved edge at `through`, within ten of the body's tolerance. `b` is
/// unchanged.
///
/// # Errors
///
/// As [`extend_trim`], and `Unsupported` for a curved edge or one without two neighbours, a
/// point not outside the edge, or no carried neighbour meeting the moved edge at the point.
pub fn extend_trim_through(
    k: &mut Kernel,
    b: &Brep,
    f: FaceId,
    e: EdgeId,
    through: Point3,
) -> Result<TrimExtension> {
    let g = grown(k, b, f, e, Move::Through(through))?;
    let mut nb = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    nb.set_tolerance(g.tol);
    let (face, edges, added) = build_into(k, &mut nb, g)?;
    let check = check_brep_with_keys(k, &mut nb, CheckLevel::Level2, &notify_ref)?;
    if !check.is_valid() {
        return Err(unsupported(
            "the extended trim crosses the face's other edges or is not valid",
        ));
    }
    Ok(TrimExtension {
        brep: nb,
        face,
        edges,
        added,
    })
}

/// How far the picked edge moves: by a distance, or (a straight edge) to pass through a point.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Move {
    /// Outward by this distance.
    By(f64),
    /// Outward, parallel, to pass through this point, which one of its carried neighbours must
    /// meet it at.
    Through(Point3),
}

/// A face's trim grown ([`grown`]), not yet built: the plane widened to hold it, its loops'
/// curves (the outer loop first) with their counts, and the body's tolerance.
pub(crate) struct Grown {
    surface: Surface,
    domain: Extent2d,
    counts: Vec<usize>,
    curves: Vec<NewCurve>,
    /// The tolerance of the body the face came from.
    pub(crate) tol: f64,
}

/// The unit direction from `v0` to `v1` and the in-plane normal to its right seen from
/// `normal`: the way a straight edge from `v0` to `v1` moves out.
fn outward(v0: Point3, v1: Point3, normal: Vec3) -> Result<(Vec3, Vec3)> {
    let u = (v1 - v0)
        .unitize()
        .map_err(|_| unsupported("an edge of no length"))?;
    let out = u
        .cross(normal)
        .unitize()
        .map_err(|_| unsupported("an edge along the plane's normal"))?;
    Ok((u, out))
}

/// Face `f` of `b` with its edge `e` moved as `how` says and its neighbours carried (module
/// docs): the grown trim's curves and the plane that holds them. `b` is only read.
pub(crate) fn grown(k: &Kernel, b: &Brep, f: FaceId, e: EdgeId, how: Move) -> Result<Grown> {
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::invalid_input(OP, "a face without a surface"))?;
    if !surface.reaches_past_trims() {
        return Err(unsupported("a surface that does not reach past its trims"));
    }
    let SurfaceKind::Plane(plane) = surface.kind() else {
        return Err(unsupported(
            "a surface that reaches past its trims but is not a plane",
        ));
    };
    let (origin, x, y) = plane.position().canonical();
    let normal = x.cross(y);
    let scale = plane.uv_scale();
    let tol = b.tolerance();
    let loops = loops_of(k, b, f)?;
    let outer = loops
        .first()
        .cloned()
        .ok_or_else(|| SolidError::invalid_input(OP, "a face without loops"))?;
    let Some(at) = outer.iter().position(|u| u.edge == e) else {
        return Err(SolidError::invalid_input(
            OP,
            "an edge not on the face's outer loop",
        ));
    };
    let picked = outer
        .get(at)
        .cloned()
        .ok_or_else(|| SolidError::invalid_input(OP, "the picked edge"))?;
    let n = outer.len();
    let straight = is_straight(&picked.curve, tol)?;
    let distance = match how {
        Move::By(d) => d,
        Move::Through(p) => {
            if !straight || n < 3 {
                return Err(unsupported(
                    "a curved edge, or one without two neighbours, moved through a point",
                ));
            }
            let v0 = picked.start(k)?;
            let (_, out) = outward(v0, picked.end(k)?, normal)?;
            let d = (p - v0).dot(out);
            if !(d.is_finite() && d > tol) {
                return Err(unsupported("a point not outside the edge"));
            }
            d
        }
    };

    // the face's size, for how far a neighbour is followed
    let mut size = 0.0;
    for u in &outer {
        size += (u.end(k)? - u.start(k)?).length();
    }
    let reach = REACH * (size + distance);

    // the moved edge, run as the picked one, to its right seen from the face's normal: a straight
    // edge (a line, or a spline whose control points lie on its chord, as a skin's
    // rails and flat sections are) moves exactly along the in-plane normal; a curved one to its
    // offset (to its right seen from the side it runs with: the face's normal when it runs along
    // its curve)
    let moved = if straight {
        None
    } else {
        let side = if picked.same { normal } else { -normal };
        Some(offset(k, &picked.curve, side, distance, 0.1 * tol)?)
    };

    let mut new_outer: Vec<NewCurve> = Vec::new();
    let mut corners: Vec<Point3> = Vec::new();
    if n == 1 {
        // a closed edge: no neighbours, the offset alone
        let Some(moved) = moved else {
            return Err(unsupported("a loop of one straight edge"));
        };
        new_outer.push(NewCurve {
            from: Some(e),
            curve: moved,
            orientation: picked.orientation(),
        });
    } else if n == 2 {
        return Err(unsupported("an outer loop of two edges"));
    } else {
        // the loop from the edge after the picked one round to the edge before it
        let round: Vec<Use> = outer
            .iter()
            .cycle()
            .skip(at + 1)
            .take(n - 1)
            .cloned()
            .collect();
        let after = round
            .first()
            .cloned()
            .ok_or_else(|| SolidError::invalid_input(OP, "the edge after"))?;
        let before = round
            .last()
            .cloned()
            .ok_or_else(|| SolidError::invalid_input(OP, "the edge before"))?;
        let (v0, v1) = (picked.start(k)?, picked.end(k)?);
        // the moved edge far enough along to meet the neighbours: a line extended both ways;
        // a curve as its offset over its own interval
        let target = match &moved {
            None => {
                // the line `distance` out along the in-plane normal to the right of the run
                let (u, out) = outward(v0, v1, normal)?;
                let (p, q) = (v0 + out * distance, v1 + out * distance);
                Curve::from(create_line_segment(p - u * reach, q + u * reach)?)
            }
            Some(moved) => moved.copy(k)?,
        };
        let (w0, t0) = hit(k, v0, before.direction(k, true)?, reach, &target, tol)?;
        let (w1, t1) = hit(k, v1, -after.direction(k, false)?, reach, &target, tol)?;
        // a straight moved edge still runs the way the picked one did, its new ends in order
        // along it (a curved one is checked by its parameters at the ends, below: its chord
        // need not run along its start tangent, as an arc's does not)
        if straight {
            let run = picked.direction(k, false)?;
            if (w1 - w0).dot(run) <= tol {
                return Err(unsupported("the neighbours cross before the distance"));
            }
        }
        if let Move::Through(p) = how {
            let at = |w: Point3| (w - p).length() <= 10.0 * tol;
            if !(at(w0) || at(w1)) {
                return Err(unsupported(
                    "a neighbour carried along its tangent does not meet the moved edge at the point",
                ));
            }
        }
        corners.push(w0);
        corners.push(w1);
        // the curves from the edge after the picked one round to the edge before it, then the
        // edge before carried, the moved edge, the edge after carried
        let carried_before = is_straight(&before.curve, tol)?;
        let carried_after = is_straight(&after.curve, tol)?;
        let last = round.len().saturating_sub(1);
        for (i, u) in round.iter().enumerate().skip(1) {
            if i == last {
                // the edge before the picked one
                if carried_before {
                    let mut c = segment(u.start(k)?, w0)?;
                    c.from = Some(u.edge);
                    new_outer.push(c);
                } else {
                    new_outer.push(NewCurve {
                        from: Some(u.edge),
                        curve: u.curve.copy(k)?,
                        orientation: u.orientation(),
                    });
                    new_outer.push(segment(v0, w0)?);
                }
            } else {
                new_outer.push(NewCurve {
                    from: Some(u.edge),
                    curve: u.curve.copy(k)?,
                    orientation: u.orientation(),
                });
            }
        }
        if let Some(moved) = moved {
            let mut c = moved;
            let mut ivl = Extent1d::from_raw(t0.min(t1), t0.max(t1));
            c.trim(k, &mut ivl, TrimMode::Silent)?;
            if (t1 > t0) != picked.same {
                return Err(unsupported(
                    "the neighbours meet the moved edge the other way round",
                ));
            }
            new_outer.push(NewCurve {
                from: Some(e),
                curve: c,
                orientation: picked.orientation(),
            });
        } else {
            let mut c = segment(w0, w1)?;
            c.from = Some(e);
            new_outer.push(c);
        }
        // the edge after the picked one
        if carried_after {
            let mut c = segment(w1, after.end(k)?)?;
            c.from = Some(after.edge);
            new_outer.push(c);
        } else {
            new_outer.push(segment(w1, v1)?);
            new_outer.push(NewCurve {
                from: Some(after.edge),
                curve: after.curve.copy(k)?,
                orientation: after.orientation(),
            });
        }
    }

    // the plane widened to hold the new trim: its box grown by the distance, and the new corners
    let natural = surface.natural_uv_domain_with_nurb(k)?;
    let param = |p: Point3| {
        let d = p - origin;
        (d.dot(x) / scale.x, d.dot(y) / scale.y)
    };
    let (gu, gv) = (distance / scale.x.abs(), distance / scale.y.abs());
    let (mut u0, mut v0, mut u1, mut v1) = (
        natural.u_min() - gu,
        natural.v_min() - gv,
        natural.u_max() + gu,
        natural.v_max() + gv,
    );
    for p in &corners {
        let (u, v) = param(*p);
        (u0, v0, u1, v1) = (u0.min(u), v0.min(v), u1.max(u), v1.max(v));
    }
    let (mu, mv) = (0.01 * (u1 - u0), 0.01 * (v1 - v0));
    let domain = Extent2d::from_bounds(u0 - mu, v0 - mv, u1 + mu, v1 + mv);
    if !(u0.is_finite() && v0.is_finite() && u1.is_finite() && v1.is_finite()) {
        return Err(SolidError::invalid_input(OP, "a plane box not finite"));
    }
    let grown = Surface::from(Plane::new(origin, x, y, scale, &domain));
    let grown_domain = grown.natural_uv_domain_with_nurb(k)?;

    // the loops' curves, the outer loop first
    let mut all: Vec<NewCurve> = new_outer;
    let mut counts = vec![all.len()];
    for l in loops.into_iter().skip(1) {
        counts.push(l.len());
        for u in l {
            all.push(NewCurve {
                from: Some(u.edge),
                orientation: u.orientation(),
                curve: u.curve,
            });
        }
    }
    Ok(Grown {
        surface: grown,
        domain: grown_domain,
        counts,
        curves: all,
        tol,
    })
}

/// The grown face `g` made in `nb` (on its own copy of the plane, the upward use the same way
/// as the plane): the face, each old edge with the edge it is now (the picked edge's the moved
/// edge, a straight neighbour's the lengthened one) and the edges added along a curved
/// neighbour's tangent. `nb`'s other faces are untouched; its cached topology is refreshed.
///
/// # Errors
///
/// `Unsupported` where the curves make no face or a curve makes no edge of it; those of the
/// face constructor.
pub(crate) fn build_into(
    k: &mut Kernel,
    nb: &mut Brep,
    g: Grown,
) -> Result<(FaceId, Vec<(EdgeId, EdgeId)>, Vec<EdgeId>)> {
    let Grown {
        surface: grown,
        domain: grown_domain,
        counts,
        curves: all,
        tol,
    } = g;
    let mut curves = Vec::new();
    let mut orientations = Vec::new();
    let mut ends: Vec<(Option<EdgeId>, Point3, Point3)> = Vec::new();
    for c in &all {
        let iv = c.curve.natural_interval();
        let (a, z) = (
            c.curve.evaluate_point(k, iv.min())?,
            c.curve.evaluate_point(k, iv.max())?,
        );
        ends.push((c.from, a, z));
        curves.push(Some(c.curve.copy(k)?));
        orientations.push(c.orientation);
    }

    let made = {
        let mut state = ArenaNotifyState::new(&*nb);
        let mut env = state.env(None);
        let key_of = env.key_of;
        let this_face = |f: FaceId| key_of(EntityRef::Face(f));
        let region = nb.infinite_region();
        make_face_with_curves(
            k,
            nb,
            region,
            FaceCurvesInput {
                loops: counts,
                curves_3d: Some(curves),
                uv_curves: None,
                orientations,
                loop_points: Vec::new(),
            },
            grown,
            &grown_domain,
            OrientType::Same,
            None,
            &this_face,
            &mut env,
        )?
    };
    let face = made
        .face
        .ok_or_else(|| unsupported("the extended trim makes no face"))?;
    nb.refresh_cached_topology();

    // each new edge by its ends: the old edge it continues, or an added one
    let mut by_ends: BTreeMap<EdgeId, (Point3, Point3)> = BTreeMap::new();
    for ne in crate::topology::face::p4::edges(nb, face, None, false, None)? {
        let [a, z] = nb.edge(ne)?.vertices;
        by_ends.insert(ne, (nb.vertex(a)?.point, nb.vertex(z)?.point));
    }
    let close = |p: Point3, q: Point3| (p - q).length() <= 10.0 * tol;
    let mut edges = Vec::new();
    let mut added = Vec::new();
    for (from, a, z) in ends {
        let found = by_ends
            .iter()
            .find(|(_, (p, q))| (close(*p, a) && close(*q, z)) || (close(*p, z) && close(*q, a)))
            .map(|(ne, _)| *ne)
            .ok_or_else(|| unsupported("a curve of the extended trim made no edge"))?;
        match from {
            Some(old) => edges.push((old, found)),
            None => added.push(found),
        }
    }
    Ok((face, edges, added))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_curves::create_arc;
    use crate::primitive::api_primitives::create_plane;
    use crate::primitive::api_trim_ops::create_planar_faces;

    fn line(a: [f64; 2], b: [f64; 2]) -> Curve {
        Curve::from(
            create_line_segment(Point3::new(a[0], a[1], 0.0), Point3::new(b[0], b[1], 0.0))
                .expect("line"),
        )
    }

    /// The one face of the planar body bounded by `curves`.
    fn planar(k: &mut Kernel, curves: &[Curve]) -> (Brep, FaceId) {
        let b = create_planar_faces(k, curves).expect("planar face");
        let f = *b.face_list().first().expect("a face");
        (b, f)
    }

    /// The edge of `b` whose ends are `p` and `q` (either way round).
    fn edge_between(b: &Brep, p: [f64; 2], q: [f64; 2]) -> EdgeId {
        let at = |v, w: [f64; 2]| {
            let pt = b.vertex(v).expect("vertex").point;
            (pt.x - w[0]).abs() < 1e-9 && (pt.y - w[1]).abs() < 1e-9
        };
        *b.edge_list()
            .iter()
            .find(|&&e| {
                let [a, z] = b.edge(e).expect("edge").vertices;
                (at(a, p) && at(z, q)) || (at(a, q) && at(z, p))
            })
            .expect("the edge")
    }

    /// The box of the body's vertices, `[x0, y0, x1, y1]`.
    fn bounds(b: &Brep) -> [f64; 4] {
        let mut r = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for &v in b.vertex_list() {
            let p = b.vertex(v).expect("vertex").point;
            r = [r[0].min(p.x), r[1].min(p.y), r[2].max(p.x), r[3].max(p.y)];
        }
        r
    }

    fn near(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    /// Each side of the 9 by 8 rectangle sheet moves 5 out, its neighbours lengthened: every old
    /// edge has its new edge, none added.
    #[test]
    fn a_rectangle_grows_on_the_side_of_its_edge() {
        let mut k = Kernel::default();
        let b = create_plane(&mut k, Point3::new(0.0, 0.0, 0.0), 9.0, 8.0).expect("plane");
        let f = *b.face_list().first().expect("face");
        for (p, q, want) in [
            ([0.0, 0.0], [9.0, 0.0], [0.0, -5.0, 9.0, 8.0]),
            ([9.0, 0.0], [9.0, 8.0], [0.0, 0.0, 14.0, 8.0]),
            ([9.0, 8.0], [0.0, 8.0], [0.0, 0.0, 9.0, 13.0]),
            ([0.0, 8.0], [0.0, 0.0], [-5.0, 0.0, 9.0, 8.0]),
        ] {
            let e = edge_between(&b, p, q);
            let out = extend_trim(&mut k, &b, f, e, 5.0).expect("extended");
            assert!(
                near(bounds(&out.brep), want),
                "{p:?}-{q:?}: {:?}",
                bounds(&out.brep)
            );
            assert_eq!(out.edges.len(), 4);
            assert!(out.added.is_empty());
            assert!(matches!(
                out.brep
                    .face(out.face)
                    .expect("face")
                    .surface
                    .as_ref()
                    .map(Surface::kind),
                Some(SurfaceKind::Plane(_))
            ));
        }
    }

    /// The trapezoid (0, 0)-(10, 0)-(7, 6)-(2, 6): its top moved up 5, its leaning sides carried
    /// up to y = 11 (the top gets shorter: from (2 + 5/3, 11) to (7 - 5/2, 11)).
    #[test]
    fn a_trapezoid_s_sides_are_carried_along() {
        let mut k = Kernel::default();
        let (b, f) = planar(
            &mut k,
            &[
                line([0.0, 0.0], [10.0, 0.0]),
                line([10.0, 0.0], [7.0, 6.0]),
                line([7.0, 6.0], [2.0, 6.0]),
                line([2.0, 6.0], [0.0, 0.0]),
            ],
        );
        let e = edge_between(&b, [7.0, 6.0], [2.0, 6.0]);
        let out = extend_trim(&mut k, &b, f, e, 5.0).expect("extended");
        assert!(
            near(bounds(&out.brep), [0.0, 0.0, 10.0, 11.0]),
            "{:?}",
            bounds(&out.brep)
        );
        let top = out
            .edges
            .iter()
            .find(|(old, _)| *old == e)
            .map(|(_, new)| *new)
            .expect("the moved edge");
        let [a, z] = out.brep.edge(top).expect("edge").vertices;
        let mut xs = [
            out.brep.vertex(a).expect("vertex").point.x,
            out.brep.vertex(z).expect("vertex").point.x,
        ];
        xs.sort_by(f64::total_cmp);
        assert!((xs[0] - (2.0 + 5.0 / 3.0)).abs() < 1e-9, "{xs:?}");
        assert!((xs[1] - 4.5).abs() < 1e-9, "{xs:?}");
    }

    /// The arched sheet (a line and a half circle of radius 5 about (5, 5)): its arc moves to
    /// the circle of radius 10, its sides carried up to meet it at y = 5 + 5 sqrt 3.
    #[test]
    fn an_arc_is_offset_and_its_neighbours_meet_it() {
        let mut k = Kernel::default();
        let arc =
            Curve::from(create_arc(Point3::new(5.0, 5.0, 0.0), 5.0, 0.0, 180.0).expect("arc"));
        let (b, f) = planar(
            &mut k,
            &[
                line([0.0, 0.0], [10.0, 0.0]),
                line([10.0, 0.0], [10.0, 5.0]),
                arc,
                line([0.0, 5.0], [0.0, 0.0]),
            ],
        );
        let e = edge_between(&b, [10.0, 5.0], [0.0, 5.0]);
        let out = extend_trim(&mut k, &b, f, e, 5.0).expect("extended");
        let top = 5.0 + 5.0 * 3f64.sqrt();
        assert!(
            near(bounds(&out.brep), [0.0, 0.0, 10.0, top]),
            "{:?}",
            bounds(&out.brep)
        );
        assert!(out.added.is_empty());
        // the arc's side refuses: the arc leaves its corner along x = 0
        let side = edge_between(&b, [0.0, 5.0], [0.0, 0.0]);
        assert!(matches!(
            extend_trim(&mut k, &b, f, side, 5.0),
            Err(SolidError::Unsupported { .. })
        ));
    }

    /// The quarter disc (0, 0)-(10, 0), the arc about the origin to (0, 10), back down x = 0:
    /// its bottom moved down 5, the arc kept with a straight edge added along its tangent at
    /// (10, 0) down to (10, -5).
    #[test]
    fn a_curved_neighbour_gets_an_edge_along_its_tangent() {
        let mut k = Kernel::default();
        let arc =
            Curve::from(create_arc(Point3::new(0.0, 0.0, 0.0), 10.0, 0.0, 90.0).expect("arc"));
        let (b, f) = planar(
            &mut k,
            &[
                line([0.0, 0.0], [10.0, 0.0]),
                arc,
                line([0.0, 10.0], [0.0, 0.0]),
            ],
        );
        let e = edge_between(&b, [0.0, 0.0], [10.0, 0.0]);
        let out = extend_trim(&mut k, &b, f, e, 5.0).expect("extended");
        assert!(
            near(bounds(&out.brep), [0.0, -5.0, 10.0, 10.0]),
            "{:?}",
            bounds(&out.brep)
        );
        assert_eq!(out.added.len(), 1);
        assert_eq!(out.edges.len(), 3);
    }

    /// Moved through a point, an edge goes where the distance to that point would take it: the
    /// rectangle's top through (3, 13) is its top moved up 5, edge for edge; a point inside the
    /// face, or one off every carried neighbour's line, is refused, and so is a curved edge.
    #[test]
    fn an_edge_moved_through_a_point_is_the_edge_moved_by_its_distance() {
        let mut k = Kernel::default();
        let b = create_plane(&mut k, Point3::new(0.0, 0.0, 0.0), 9.0, 8.0).expect("plane");
        let f = *b.face_list().first().expect("face");
        let top = edge_between(&b, [9.0, 8.0], [0.0, 8.0]);
        let by = extend_trim(&mut k, &b, f, top, 5.0).expect("by the distance");
        // the corner the left side, carried up, meets the moved top at
        let through =
            extend_trim_through(&mut k, &b, f, top, Point3::new(0.0, 13.0, 0.0)).expect("through");
        assert!(
            near(bounds(&through.brep), bounds(&by.brep)),
            "{:?}",
            bounds(&through.brep)
        );
        assert_eq!(through.edges.len(), by.edges.len());
        assert!(through.added.is_empty());
        // inside the face
        assert!(matches!(
            extend_trim_through(&mut k, &b, f, top, Point3::new(3.0, 4.0, 0.0)),
            Err(SolidError::Unsupported { .. })
        ));
        // outside, but on no carried neighbour: the sides run up x = 0 and x = 9
        assert!(matches!(
            extend_trim_through(&mut k, &b, f, top, Point3::new(3.0, 13.0, 0.0)),
            Err(SolidError::Unsupported { .. })
        ));
        // a curved edge
        let arc =
            Curve::from(create_arc(Point3::new(5.0, 5.0, 0.0), 5.0, 0.0, 180.0).expect("arc"));
        let (b, f) = planar(
            &mut k,
            &[
                line([0.0, 0.0], [10.0, 0.0]),
                line([10.0, 0.0], [10.0, 5.0]),
                arc,
                line([0.0, 5.0], [0.0, 0.0]),
            ],
        );
        let e = edge_between(&b, [10.0, 5.0], [0.0, 5.0]);
        assert!(matches!(
            extend_trim_through(&mut k, &b, f, e, Point3::new(5.0, 15.0, 0.0)),
            Err(SolidError::Unsupported { .. })
        ));
    }

    #[test]
    fn a_distance_not_positive_is_refused() {
        let mut k = Kernel::default();
        let b = create_plane(&mut k, Point3::new(0.0, 0.0, 0.0), 9.0, 8.0).expect("plane");
        let f = *b.face_list().first().expect("face");
        let e = *b.edge_list().first().expect("edge");
        for d in [0.0, -1.0, f64::NAN] {
            assert!(matches!(
                extend_trim(&mut k, &b, f, e, d),
                Err(SolidError::InvalidInput { .. })
            ));
        }
    }
}
