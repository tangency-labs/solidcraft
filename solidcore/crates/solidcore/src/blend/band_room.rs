//! The room a round's band has along a straight edge between two planar faces: at a station
//! of the edge, the largest radius whose band (the strip a round of that radius takes off
//! each face, from the edge to its contact line) stays inside both trimmed faces, and the room
//! at the edge's ends. A taper (a variable radius following the room) is built on it, and a
//! variable round whose radius is larger than the room at a station is refused there.
//!
//! Not upstream: SolidCraft's `blend_variable.rs` (`EdgeSetup::{band_fits, room, end_room}`,
//! `extension_fits`, and the refusals of its `taper`), made the kernel's and certified.
//!
//! | Rust | SolidCraft |
//! |------|-----------|
//! | [`band_room`], [`BandRoom`] | `taper`'s `end_room` at both ends, `room` at its stations, and its refusals |
//! | [`law_fits_band`] | `taper`'s "the room runs out inside the edge", for any radius law |
//! | `EdgeBand::room_at` (private) | `EdgeSetup::room` (`band_fits`, `extension_fits`) |
//! | `EdgeBand::end` (private) | `EdgeSetup::end_room` |
//!
//! A round of radius `r` between faces whose in-face directions away from the edge make the
//! angle `phi` touches each face at `r / tan(phi / 2)` from the edge. At a station `p` of the
//! edge, face `i` has room `s_i` along its direction `t_i` (square to the edge, into the face)
//! up to the first point where its boundary, other than the edge, meets the line `p + s t_i`:
//! the section plane through `p` square to the edge meets each boundary edge there. The room
//! as a radius is `min_i s_i tan(phi / 2)`.
//!
//! Certified: every room is an interval [`Room`] that holds the exact room of the faces as
//! their data give them (straight edges between their vertices, curves by their control
//! points), the rounding of every step bounded (Higham's `gamma_k`). A straight boundary edge
//! meets the section exactly; a curved one is bounded by its control polygon's hull (a
//! rational B-spline with positive weights lies in it), which gives the room's lower end only.
//! A station where a boundary may meet the section behind or at the edge, within the bounds,
//! has lower end 0. An end of the edge is square, closes to a point, or leans as its side's
//! far end lies within, before or past the plane square to the edge at the end vertex, by the
//! edges' tolerance, proved ([`Bounded::at_most`]); where the bounds do not decide it the end
//! is [`EndRoom::Undecided`]. Deviations from SolidCraft: a curved side at an end is not
//! taken as closing to a point (its lean is not measured); the faces are the kernel's, not
//! chorded polylines. No recursion.

use crate::blend::fillet_executive::FilletLaw;
use crate::blend::round_room::{refused, RoundRefusal};
use crate::curve::curve::{Curve, CurveKind};
use crate::geom::certified::{unit, unit_error, unit_error_of, Bounded, Verdict};
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::math::{gamma, NOW, NOZ};
use crate::surface::implicit::{lies_on, Implicit};
use crate::surface::surface::SurfaceKind;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, face, EdgeId, FaceId, VertexId};
use crate::{Brep, Kernel, Result, SolidError};

const OP: &str = "blend::band_room";

fn unsupported(reason: &'static str) -> SolidError {
    SolidError::Unsupported {
        operation: OP,
        reason,
    }
}

/// A room as a radius, certified: every radius at most `lower` has its band inside both
/// faces, and no radius above `upper` has (`upper` is infinite where no boundary is proved to
/// meet the band's line).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    /// The radius every band up to which is proved to fit.
    pub lower: f64,
    /// The radius past which no band fits.
    pub upper: f64,
}

/// The room at an end of the edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EndRoom {
    /// Both faces' sides at the end are straight and square to the edge: the room their
    /// reach along the faces gives.
    Square(Room),
    /// A face's side runs back along the edge: the face closes to a point, no room.
    ClosesToPoint,
    /// The bounds do not decide whether a side is square to the edge.
    Undecided,
}

/// The room along a straight edge between planes ([`band_room`]).
#[derive(Debug, Clone, PartialEq)]
pub struct BandRoom {
    /// The edge's start vertex, where the stations start.
    pub start: VertexId,
    /// Its end vertex.
    pub end: VertexId,
    /// The room at the start.
    pub at_start: EndRoom,
    /// The room at the end.
    pub at_end: EndRoom,
    /// The room at each station asked (in order).
    pub stations: Vec<Room>,
}

/// An end of the edge, as [`EdgeBand::end`] finds it.
enum End {
    Room(EndRoom),
    /// A side leans away from the edge (its far end's lean past the square plane), or is
    /// not straight (`None`).
    Leans(Option<f64>),
}

/// A bounded value: `value` within `bound` of exact.
#[derive(Clone, Copy)]
struct B {
    value: f64,
    bound: f64,
}

impl B {
    fn lo(self) -> f64 {
        self.value - self.bound
    }
    fn hi(self) -> f64 {
        self.value + self.bound
    }
}

/// `a . t` for a computed `a` whose components are each within `ea` of exact and a unit `t`
/// whose components are within `et`: the inputs move it by at most
/// `3 (ea (1 + et) + |a|_max et)`, its rounding by `gamma_3` of its terms; the bound itself
/// is rounded up by `gamma_8`.
fn dot_b(a: Vec3, ea: f64, t: Vec3, et: f64) -> B {
    let value = a.dot(t);
    let terms = (a.x * t.x).abs() + (a.y * t.y).abs() + (a.z * t.z).abs();
    let amax = a.x.abs().max(a.y.abs()).max(a.z.abs()) + ea;
    let bound = (3.0 * (ea * (1.0 + et) + amax * et) + gamma(3) * terms) * (1.0 + gamma(8));
    B { value, bound }
}

/// The largest absolute component of `p`.
fn max_abs(p: Point3) -> f64 {
    p.x.abs().max(p.y.abs()).max(p.z.abs())
}

/// The B-spline form of a curve kind built on one (a line, a conic, a B-spline), `None` for
/// the others.
fn b_spline_of(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(c) => Some(c),
        CurveKind::Line(c) => Some(c.base()),
        CurveKind::Ellipse(c) => Some(c.base()),
        CurveKind::Hyperbola(c) => Some(&c.base),
        CurveKind::Parabola(c) => Some(c.base()),
        _ => None,
    }
}

/// Whether the edge `x` of `b` is straight: a line, or a curve whose control points all lie
/// on the line through its vertices within the rounding of measuring it (as
/// [`crate::surface::implicit::lies_on`] judges a support), so that it is the segment between
/// them.
fn straight(b: &Brep, x: EdgeId) -> Result<bool> {
    let Some(curve) = b.edge(x)?.curve.as_ref() else {
        return Ok(false);
    };
    if matches!(curve.kind(), CurveKind::Line(_)) {
        return Ok(true);
    }
    let Some(base) = b_spline_of(curve) else {
        return Ok(false);
    };
    let (Some(va), Some(vb)) = edge::start_end_vertices(b, x)? else {
        return Ok(false);
    };
    let (pa, pb) = (b.vertex(va)?.point, b.vertex(vb)?.point);
    let Some(d) = unit(pb - pa) else {
        return Ok(false);
    };
    let ed = unit_error_of(gamma(1) * max_abs(pa).max(max_abs(pb)), (pb - pa).length());
    for cp in base.control_points()? {
        let w = if cp.w == NOW { 1.0 } else { cp.w };
        if !(w > 0.0 && w.is_finite()) || cp.z == NOZ {
            return Ok(false);
        }
        let q = Point3::new(cp.x / w, cp.y / w, cp.z / w);
        let a = q - pa;
        // |a x d| with a's components within gamma_2 of exact and d's within ed: each
        // component moves by 2 (|a| ed + ea) and rounds by gamma_2 of its terms
        let ea = gamma(2) * (max_abs(q) + max_abs(pa));
        let off = a.cross(d).length();
        let bound =
            (4.0 * (ea + (max_abs(a) + ea) * ed) + 2.0 * gamma(2) * max_abs(a) + gamma(5) * off)
                * (1.0 + gamma(8));
        if off > bound {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The normal of the planar surface of face `f`: a plane's, or the normal of the plane a
/// B-spline surface is proved to lie on ([`lies_on`], its candidate from the surface's
/// `IsPlanar`); `None` for a face not proved planar.
fn plane_normal(k: &Kernel, b: &Brep, f: FaceId) -> Result<Option<Vec3>> {
    let Some(surface) = b.face(f)?.surface.as_ref() else {
        return Ok(None);
    };
    match surface.kind() {
        SurfaceKind::Plane(p) => Ok(Some(p.is_planar().1)),
        SurfaceKind::BSpline(form) => {
            let planar = surface.is_planar(k, f64::INFINITY)?;
            let Some(normal) = unit(planar.normal) else {
                return Ok(None);
            };
            let plane = Implicit::Plane {
                origin: planar.point,
                normal,
            };
            Ok(lies_on(form, &plane)?.then_some(normal))
        }
        _ => Ok(None),
    }
}

/// One face of the edge: its in-face unit direction square to the edge, into the face, and
/// the boundary edges other than the edge.
struct Side {
    t: Vec3,
    others: Vec<EdgeId>,
}

/// A straight edge between two planar faces, as the band's room reads it.
struct EdgeBand {
    edge: EdgeId,
    v: [VertexId; 2],
    p0: Point3,
    p1: Point3,
    /// The unit direction from the start to the end, and the error of its components.
    d: Vec3,
    /// The error of each component of `d` and of each face's `t`.
    ed: f64,
    et: f64,
    sides: [Side; 2],
    /// `tan(phi / 2)` bounded below and above.
    tan_half: [f64; 2],
    /// The edge's tolerance.
    tol: f64,
}

/// The in-face direction of face `f` of edge `e`: `n x d` (`n` its plane's normal) signed to
/// point into the face as the edge use's binormal does there.
fn into_face(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    f: FaceId,
    d: Vec3,
    ed: f64,
) -> Result<(Vec3, f64)> {
    let normal =
        plane_normal(k, b, f)?.ok_or_else(|| unsupported("a face of the edge is not planar"))?;
    let n = unit(normal).ok_or_else(|| unsupported("a face's plane has no normal"))?;
    let c = n.cross(d);
    let en = unit_error();
    // each component of the cross: two products of unit components within en and ed, and
    // their difference: 2 (en + ed + en ed) moved, gamma_2 rounded (terms at most 2)
    let ec = 2.0 * (en + ed + en * ed) + 2.0 * gamma(2);
    let len = c.length();
    let t = unit(c).ok_or_else(|| unsupported("a face's plane holds no square to the edge"))?;
    let et = unit_error_of(ec, len);
    let mut eu = None;
    for u in edge::edgeuses(b, e, None, None)? {
        let face = match eu1::faceuse(b, u)? {
            Some(fu) => b.faceuse(fu)?.face,
            None => None,
        };
        if face == Some(f) {
            eu = Some(u);
            break;
        }
    }
    let eu = eu.ok_or_else(|| unsupported("a face of the edge has no edge use of it"))?;
    let mid = b.edge(e)?.interval.evaluate(0.5);
    let into = evaluate_binormal(k, b, eu, mid, false, &BinormalOptions::default())?.binormal;
    // the binormal is a unit vector in the face's plane square to the edge: +-t, far from 0
    let along = into.dot(t);
    if along > 0.5 {
        Ok((t, et))
    } else if along < -0.5 {
        Ok((t * -1.0, et))
    } else {
        Err(SolidError::port_invariant(
            OP,
            "the edge use's binormal is not square to the edge in its face",
        ))
    }
}

impl EdgeBand {
    /// The edge `e`; `None` for an edge that is not straight, or not between two planar faces.
    fn of(k: &Kernel, b: &mut Brep, e: EdgeId) -> Result<Option<Self>> {
        if !straight(b, e)? {
            return Ok(None);
        }
        let (Some(v0), Some(v1)) = edge::start_end_vertices(b, e)? else {
            return Err(unsupported("the edge has no two vertices"));
        };
        let (p0, p1) = (b.vertex(v0)?.point, b.vertex(v1)?.point);
        let d = unit(p1 - p0).ok_or_else(|| unsupported("the edge has no length"))?;
        // the difference rounds once per component before it is made unit
        let len = (p1 - p0).length();
        let ed = unit_error_of(gamma(1) * max_abs(p0).max(max_abs(p1)), len);
        let faces = edge::faces(b, e, None)?;
        let [f0, f1] = faces[..] else {
            return Ok(None);
        };
        for f in [f0, f1] {
            if plane_normal(k, b, f)?.is_none() {
                return Ok(None);
            }
        }
        let mut sides = Vec::new();
        for f in [f0, f1] {
            let edges = face::p4::edges(b, f, None, false, None)?;
            if edges.iter().filter(|&&x| x == e).count() != 1 {
                return Err(unsupported("the edge bounds a face twice"));
            }
            let (t, et) = into_face(k, b, e, f, d, ed)?;
            sides.push((
                Side {
                    t,
                    others: edges.into_iter().filter(|&x| x != e).collect(),
                },
                et,
            ));
        }
        let (Some((s1, et1)), Some((s0, et0))) = (sides.pop(), sides.pop()) else {
            return Err(SolidError::port_invariant(OP, "two sides were made"));
        };
        let et = et0.max(et1);
        // cos phi = t0 . t1, within 2 (2 et + et^2) + gamma_3 (unit vectors)
        let c = s0.t.dot(s1.t);
        let ec = 2.0 * (2.0 * et + et * et) + gamma(3);
        if !(c - ec > -1.0 && c + ec < 1.0) {
            return Err(unsupported("the faces at the edge are tangent"));
        }
        // tan(phi / 2) = sqrt((1 - c) / (1 + c)), falling with c; the quotient and root
        // round three times
        let tan = |c: f64| ((1.0 - c) / (1.0 + c)).sqrt();
        let tan_half = [
            tan((c + ec).min(1.0)) * (1.0 - gamma(4)),
            tan((c - ec).max(-1.0)) * (1.0 + gamma(4)),
        ];
        let tol = b.edge(e)?.tolerance;
        Ok(Some(Self {
            edge: e,
            v: [v0, v1],
            p0,
            p1,
            d,
            ed,
            et,
            sides: [s0, s1],
            tan_half,
            tol,
        }))
    }

    /// The station at `u` (0 at the start, 1 at the end) and the error of its components.
    fn station(&self, u: f64) -> (Point3, f64) {
        let p = self.p0 + (self.p1 - self.p0) * u;
        (p, gamma(6) * (max_abs(self.p0) + max_abs(self.p1)))
    }

    /// The room along `side`'s direction from the station `p` (components within `ep`),
    /// before any boundary edge but this one meets the section: `[lower, upper]` as distances
    /// along the face.
    fn reach(&self, b: &Brep, side: &Side, p: Point3, ep: f64) -> Result<[f64; 2]> {
        let (mut lower, mut upper) = (f64::INFINITY, f64::INFINITY);
        // a point's place: its height over the section plane, its reach along the face
        let place = |q: Point3| {
            let ea = ep + gamma(1) * (max_abs(q) + max_abs(p));
            (
                dot_b(q - p, ea, self.d, self.ed),
                dot_b(q - p, ea, side.t, self.et),
            )
        };
        for &x in &side.others {
            let edge = b.edge(x)?;
            if straight(b, x)? {
                let (Some(va), Some(vb)) = edge::start_end_vertices(b, x)? else {
                    return Err(unsupported("a boundary edge has no two vertices"));
                };
                let ((ha, sa), (hb, sb)) = (place(b.vertex(va)?.point), place(b.vertex(vb)?.point));
                if (ha.lo() > 0.0 && hb.lo() > 0.0) || (ha.hi() < 0.0 && hb.hi() < 0.0) {
                    continue; // wholly on one side of the section
                }
                let proper = (ha.lo() > 0.0 && hb.hi() < 0.0) || (ha.hi() < 0.0 && hb.lo() > 0.0);
                if !proper {
                    // an end on the section within the bounds: it may meet it anywhere
                    // between its ends
                    lower = lower.min(sa.lo().min(sb.lo()).max(0.0));
                    continue;
                }
                // the crossing's fraction from a, lam = ha / (ha - hb), over the corners of
                // the heights' bounds (it is monotone in each), padded by its rounding
                let mut lam = [f64::INFINITY, f64::NEG_INFINITY];
                for h0 in [ha.lo(), ha.hi()] {
                    for h1 in [hb.lo(), hb.hi()] {
                        let l = h0 / (h0 - h1);
                        lam = [lam[0].min(l), lam[1].max(l)];
                    }
                }
                let lam = [(lam[0] - gamma(3)).max(0.0), (lam[1] + gamma(3)).min(1.0)];
                // the crossing's reach (1 - lam) sa + lam sb, linear in lam
                let pad = gamma(4) * (sa.value.abs() + sb.value.abs() + sa.bound + sb.bound);
                let at = |l: f64, a: f64, c: f64| (1.0 - l) * a + l * c;
                let lo = at(lam[0], sa.lo(), sb.lo()).min(at(lam[1], sa.lo(), sb.lo())) - pad;
                let hi = at(lam[0], sa.hi(), sb.hi()).max(at(lam[1], sa.hi(), sb.hi())) + pad;
                if hi <= 0.0 {
                    continue; // behind the edge
                }
                lower = lower.min(lo.max(0.0));
                if lo > 0.0 {
                    upper = upper.min(hi);
                }
            } else {
                let Some(base) = edge.curve.as_ref().and_then(b_spline_of) else {
                    // no control points to bound it by: no room proved here
                    return Ok([0.0, upper]);
                };
                let (mut above, mut below, mut least, mut behind) =
                    (true, true, f64::INFINITY, true);
                for cp in base.control_points()? {
                    let w = if cp.w == NOW { 1.0 } else { cp.w };
                    if !(w > 0.0 && w.is_finite()) || cp.z == NOZ {
                        // no hull to bound it by: no room proved here
                        return Ok([0.0, upper]);
                    }
                    // the point divided out: one rounding per component
                    let q = Point3::new(cp.x / w, cp.y / w, cp.z / w);
                    let ea = ep + gamma(2) * (max_abs(q) + max_abs(p));
                    let (h, s) = (
                        dot_b(q - p, ea, self.d, self.ed),
                        dot_b(q - p, ea, side.t, self.et),
                    );
                    above &= h.lo() > 0.0;
                    below &= h.hi() < 0.0;
                    behind &= s.hi() <= 0.0;
                    least = least.min(s.lo());
                }
                if above || below || behind {
                    continue;
                }
                lower = lower.min(least.max(0.0));
            }
        }
        Ok([lower, upper])
    }

    /// The room at the station `u` strictly inside the edge.
    fn room_at(&self, b: &Brep, u: f64) -> Result<Room> {
        let (p, ep) = self.station(u);
        let mut room = Room {
            lower: f64::INFINITY,
            upper: f64::INFINITY,
        };
        for side in &self.sides {
            let [lo, hi] = self.reach(b, side, p, ep)?;
            room.lower = room.lower.min(lo * self.tan_half[0] * (1.0 - gamma(1)));
            room.upper = room.upper.min(hi * self.tan_half[1] * (1.0 + gamma(1)));
        }
        Ok(room)
    }

    /// The room at end `which` (0 the start, 1 the end).
    fn end(&self, b: &Brep, which: usize) -> Result<End> {
        let (v, out) = if which == 0 {
            (self.v[0], self.d * -1.0)
        } else {
            (self.v[1], self.d)
        };
        let pv = b.vertex(v)?.point;
        let mut room = Room {
            lower: f64::INFINITY,
            upper: f64::INFINITY,
        };
        let mut undecided = false;
        for side in &self.sides {
            let mut at_v = Vec::new();
            for &x in &side.others {
                let (s, t) = edge::start_end_vertices(b, x)?;
                if s == Some(v) {
                    at_v.push((x, t));
                } else if t == Some(v) {
                    at_v.push((x, s));
                }
            }
            let [(x, Some(far))] = at_v[..] else {
                return Ok(End::Leans(None));
            };
            if !straight(b, x)? {
                return Ok(End::Leans(None));
            }
            let q = b.vertex(far)?.point;
            let ea = gamma(1) * (max_abs(q) + max_abs(pv));
            let lean = dot_b(q - pv, ea, out, self.ed);
            let tol = self.tol.max(b.edge(x)?.tolerance);
            match (Bounded {
                value: lean.value.abs(),
                bound: lean.bound,
            })
            .at_most(tol)
            {
                Verdict::Yes => {
                    let reach = dot_b(q - pv, ea, side.t, self.et);
                    room.lower = room
                        .lower
                        .min(reach.lo().max(0.0) * self.tan_half[0] * (1.0 - gamma(1)));
                    room.upper = room
                        .upper
                        .min(reach.hi() * self.tan_half[1] * (1.0 + gamma(1)));
                }
                Verdict::No if lean.value < 0.0 => return Ok(End::Room(EndRoom::ClosesToPoint)),
                Verdict::No => return Ok(End::Leans(Some(lean.value))),
                Verdict::Undecided => undecided = true,
            }
        }
        Ok(End::Room(if undecided {
            EndRoom::Undecided
        } else {
            EndRoom::Square(room)
        }))
    }
}

/// The room a round's band has along the straight edge `e` between two planar faces: at its
/// start, at its end, and at each of `stations` (each a fraction of the way from the start
/// vertex to the end, strictly between 0 and 1), each a certified [`Room`].
///
/// # Errors
/// [`SolidError::RoundRefused`] with [`RoundRefusal::TaperEndUnsupported`] for an end where a
/// face's side leans away from the edge or is not straight, and
/// [`RoundRefusal::FacesCloseAtBothEnds`] where the faces close to a point at both ends;
/// [`SolidError::Unsupported`] for an edge that is not straight, not between two planar faces
/// it bounds once each, of no length, or whose faces are tangent; [`SolidError::InvalidInput`]
/// for a station not strictly inside (0, 1); the evaluators' errors.
pub fn band_room(k: &Kernel, b: &mut Brep, e: EdgeId, stations: &[f64]) -> Result<BandRoom> {
    if !stations.iter().all(|&u| u > 0.0 && u < 1.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a station lies strictly inside the edge",
        ));
    }
    let band = EdgeBand::of(k, b, e)?.ok_or_else(|| {
        unsupported("a band's room is measured along a straight edge between two planar faces")
    })?;
    let mut ends = [EndRoom::Undecided; 2];
    for (which, slot) in ends.iter_mut().enumerate() {
        *slot = match band.end(b, which)? {
            End::Room(r) => r,
            End::Leans(lean) => {
                let vertex = if which == 0 { band.v[0] } else { band.v[1] };
                return Err(refused(
                    OP,
                    RoundRefusal::TaperEndUnsupported { vertex, lean },
                ));
            }
        };
    }
    if ends == [EndRoom::ClosesToPoint; 2] {
        return Err(refused(OP, RoundRefusal::FacesCloseAtBothEnds { edge: e }));
    }
    let mut rooms = Vec::new();
    for &u in stations {
        rooms.push(band.room_at(b, u)?);
    }
    let [at_start, at_end] = ends;
    Ok(BandRoom {
        start: band.v[0],
        end: band.v[1],
        at_start,
        at_end,
        stations: rooms,
    })
}

/// Whether the variable round of the edge `e` with the radius `law` fits its faces at each of
/// `stations` (fractions of the way from the edge's start vertex, in [0, 1], the law's own
/// parameter; the ends' rooms where they are decided), on a straight edge between two planar
/// faces: refused at the first station where the law's radius, less its evaluation's
/// rounding, is above the room's upper end. A station where it is not proved too large
/// passes, and an edge that is not straight between two planar faces is not judged: this
/// decides only what it proves.
///
/// # Errors
/// [`SolidError::RoundRefused`] with [`RoundRefusal::RoomRunsOut`] (the station, and the
/// room's lower end there as the largest radius proved to fit); [`SolidError::InvalidInput`]
/// for a station outside [0, 1] or a law that is not a B-spline radius; those of the room's
/// measure (not [`band_room`]'s refusals: an end that leans is not judged) and of the law's
/// evaluation.
pub fn law_fits_band(
    k: &Kernel,
    b: &mut Brep,
    e: EdgeId,
    stations: &[f64],
    law: &FilletLaw,
) -> Result<()> {
    if !stations.iter().all(|&u| (0.0..=1.0).contains(&u)) {
        return Err(SolidError::invalid_input(OP, "a station lies on the edge"));
    }
    let curve = law
        .law_curve()
        .ok_or_else(|| SolidError::invalid_input(OP, "the law is a B-spline radius"))?;
    // de Boor's evaluation of the radius (x) rounds within gamma_{2p+2} of its control radii
    let largest = curve
        .control_points()?
        .iter()
        .fold(0.0_f64, |m, c| m.max(c.x.abs()));
    let radius_error = gamma(2 * curve.degree()? + 2) * largest;
    let Some(band) = EdgeBand::of(k, b, e)? else {
        return Ok(());
    };
    for &u in stations {
        let room = if u == 0.0 || u == 1.0 {
            match band.end(b, usize::from(u == 1.0))? {
                End::Room(EndRoom::Square(r)) => r,
                End::Room(EndRoom::ClosesToPoint) => Room {
                    lower: 0.0,
                    upper: 0.0,
                },
                End::Room(EndRoom::Undecided) | End::Leans(_) => continue,
            }
        } else {
            band.room_at(b, u)?
        };
        let r = curve.evaluate_point(u)?.x;
        if r - radius_error > room.upper {
            return Err(refused(
                OP,
                RoundRefusal::RoomRunsOut {
                    edge: band.edge,
                    at: u,
                    largest: room.lower,
                },
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "band_room_tests.rs"]
mod tests;
