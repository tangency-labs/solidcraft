//! The closed flux surface of a solid's faces: their trim loops, the gap strips joining two
//! faces' boundaries along an edge, and the openings left at the vertices (not upstream,
//! #14170).
//!
//! A body's volume and its moments are integrated as fluxes of fields `F = (0, 0, f)`
//! (`df/dz` the integrand) out of its faces, face by face
//! ([`super::integrator::properties`]). That is the body's volume only where the faces
//! close: where two faces' boundaries along an edge are apart (a gap within the edge's
//! tolerance) the flux is taken over an open surface, and its value depends on the origin
//! it is taken about by `-(1/3) C . O`, `C` the open surface's vector area (#14170: the
//! 2907c hull, gaps up to 4.7e-6, `C_z` 9.8e-5, its volume 1.77e-2 apart between two
//! origins). This module builds what closes it, once, for every caller:
//!
//! - **Faces**: each face's material side, its sign as the integrator signs it, and its
//!   loops in the order and direction the integrator walks them, every edge use's pcurve
//!   with its walked ends.
//! - **Strips**: each edge's face sides in pairs of opposite sense along it, joined over
//!   the pcurves' common parameters by the ruled strip `X_1 + s (X_2 - X_1)` (`X_i` the two
//!   boundaries in 3D, one edge parameter).
//! - **Openings**: what faces and strips leave open: the loops' closing uv segments
//!   between consecutive curves' ends, the parts of an edge's pcurves past their common
//!   parameters, the strips' two ends, and the whole of every side of an edge left without
//!   a side of opposite sense to pair with (an open edge; or two pcurves sharing no
//!   parameters). Their total 3D length `L` is bounded; they are
//!   closed curves capped by cones of area at most `L^2 / 4`, every point of them within
//!   `L` of the points recorded.
//!
//! A face that is not material on exactly one side (or on a region the kernel is unsure
//! of), or whose surface or pcurves have no NURBS form, cannot be part of a closed
//! surface: the closure is refused, with the face and the reason ([`ClosureRefused`]).
//!
//! [`super::enclosure`] encloses the flux of faces, strips and caps; the estimate integrates
//! the faces' and strips' fluxes and bounds the caps' (`ClosureBound`). Both measure the
//! surface built here.
//!
//! Deviations: none (not upstream).

use std::collections::BTreeMap;

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::{as_b_spline, knot, Nurbs};
use crate::kernel::Kernel;
use crate::mass::integrator::properties::region_material;
use crate::nurbs::b_spline_curve::p1::BSplineCurve;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, RegionId};
use crate::{Result, SolidError};

const OP: &str = "mass::closure";

/// One edge use of a loop as the integrator walks it.
pub(crate) struct LoopCurve {
    /// The edge.
    pub edge: EdgeId,
    /// Its pcurve.
    pub cn: NurbsCurve,
    /// 1 where the walk follows the pcurve, -1 against it.
    pub dir: f64,
    /// The walk's start and end in uv, enclosed.
    pub ends: (V3<Ivl>, V3<Ivl>),
}

/// A face of the closed surface.
pub(crate) struct ClosureFace {
    /// The face.
    pub face: FaceId,
    /// Its surface as NURBS data.
    pub cn: Nurbs,
    /// -1 where the material side is the upward face use's (`Same`), as the integrator
    /// signs it ([`super::integrator::properties::boundary_properties`]).
    pub sign: f64,
    /// The upward use's loops, each edge use's pcurve forward where it is `Same`.
    pub loops: Vec<Vec<LoopCurve>>,
}

/// One side of a strip: a face's boundary curve along the edge.
pub(crate) struct StripSide {
    /// The face's index in [`Closure::faces`].
    pub face: usize,
    /// Its pcurve.
    pub cn: NurbsCurve,
}

/// The gap strip of an edge between two of its faces' sides, the first along the closed
/// surface's sense, the second against it.
pub(crate) struct ClosureStrip {
    /// The two sides.
    pub sides: [StripSide; 2],
    /// The pcurves' common parameters.
    pub t: (f64, f64),
}

/// What the faces and strips leave open.
pub(crate) struct Openings {
    /// A bound on their total 3D length (`L`).
    pub length: Ivl,
    /// Points on them, enclosed: every point of the openings is within `L` of one.
    pub points: Vec<[Ivl; 3]>,
    /// The strips' end points on their first side (the subset of `points` the enclosure
    /// measures its caps from).
    pub strip_ends: Vec<[Ivl; 3]>,
}

/// Why a face cannot be part of a closed flux surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosureRefusal {
    /// Its two sides are both material or both void: it bounds no solid (a sheet, a fin,
    /// a partition between two solids).
    NotOneMaterialSide,
    /// A side's region is one the kernel is unsure of (material or void).
    UnsureRegion(RegionId),
    /// Its surface, or one of its pcurves, has no NURBS form.
    NoNurbsForm,
    /// It has a thickness: its volume is its area times its thickness, not a flux.
    Thick,
    /// The volume policy counts thicknesses only (`face_volume` 1): no face's volume is a
    /// flux.
    VolumePolicy,
    /// An opening or a strip could not be bounded or evaluated (a surface whose weights
    /// are not proved positive there, a loop's end that is not finite).
    Unbounded,
    /// Its trims may leave the domain its surface's evaluation clamps to, and the surface
    /// continued past it (as the closure builds the face) is rational with its weights not
    /// proved positive over the trims' box: the face is integrated clamped, as upstream
    /// integrates it, and no closed surface stands for it (`brep_properties` only; the
    /// closure itself never makes it).
    WeightsNotProved,
    /// Its trims may leave the domain its surface's evaluation clamps to, and their
    /// excursion cannot be bounded (a trim with no B-spline form, or a weight not proved
    /// positive), while the surface is rational: its continuation over the trims cannot be
    /// certified. Integrated clamped, as `WeightsNotProved`.
    TrimNotBounded,
}

/// A refused closure: the face that cannot be closed (none for [`ClosureRefusal::VolumePolicy`])
/// and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosureRefused {
    /// The face.
    pub face: Option<FaceId>,
    /// Why.
    pub why: ClosureRefusal,
}

/// The closed flux surface of a solid's faces.
pub(crate) struct Closure {
    /// The faces, in the B-rep's face order.
    pub faces: Vec<ClosureFace>,
    /// The strips, by edge, then pair.
    pub strips: Vec<ClosureStrip>,
    /// The sides left without a side of opposite sense to pair with, whole: openings.
    pub unpaired: Vec<StripSide>,
}

impl Closure {
    /// The faces and strips of `b`, or the first face that cannot be closed and why.
    /// Missing UV trim curves are made on `b`, as the integrator's trim boundary makes
    /// them.
    ///
    /// # Errors
    ///
    /// [`SolidError::Cancelled`]; those of the evaluations.
    pub(crate) fn of(
        k: &Kernel,
        b: &mut Brep,
    ) -> Result<core::result::Result<Self, ClosureRefused>> {
        let mut faces = Vec::new();
        let mut sides: BTreeMap<EdgeId, Vec<(usize, NurbsCurve, bool)>> = BTreeMap::new();
        for f in crate::topology::brep::p1::faces(b, None)? {
            k.check_cancelled(OP)?;
            let face = match closure_face(k, b, f)? {
                Ok(face) => face,
                Err(why) => return Ok(Err(ClosureRefused { face: Some(f), why })),
            };
            let index = faces.len();
            for lp in &face.loops {
                for c in lp {
                    // the side's sense along the edge's curve on the closed surface: its
                    // walk times the face's sign
                    let forward = c.dir * face.sign > 0.0;
                    sides
                        .entry(c.edge)
                        .or_default()
                        .push((index, c.cn.clone(), forward));
                }
            }
            faces.push(face);
        }
        // each edge's face sides in pairs of opposite sense along it, a strip for each pair
        // (two sides at a manifold edge; four where solids touch along it); a side left
        // over (an open edge, or more sides one way than the other), or a pair whose
        // pcurves share no parameters, is an opening, whole
        let mut strips = Vec::new();
        let mut unpaired = Vec::new();
        let side = |x: (usize, NurbsCurve, bool)| StripSide { face: x.0, cn: x.1 };
        for (_, s) in sides {
            let (forward, backward): (Vec<_>, Vec<_>) = s.into_iter().partition(|x| x.2);
            let n = forward.len().min(backward.len());
            let mut forward = forward.into_iter();
            let mut backward = backward.into_iter();
            for (a, c) in forward.by_ref().zip(backward.by_ref()).take(n) {
                let range =
                    |cn: &NurbsCurve| (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.n as isize));
                let (ra, rc) = (range(&a.1), range(&c.1));
                let t = (ra.0.max(rc.0), ra.1.min(rc.1));
                if t.0.partial_cmp(&t.1) != Some(core::cmp::Ordering::Less) {
                    unpaired.push(side(a));
                    unpaired.push(side(c));
                    continue;
                }
                strips.push(ClosureStrip {
                    sides: [side(a), side(c)],
                    t,
                });
            }
            unpaired.extend(forward.chain(backward).map(side));
        }
        Ok(Ok(Self {
            faces,
            strips,
            unpaired,
        }))
    }

    /// The face of strip side `side`.
    pub(crate) fn side<'a>(
        &'a self,
        strip: &'a ClosureStrip,
        side: usize,
    ) -> Result<(&'a ClosureFace, &'a NurbsCurve)> {
        let s = strip
            .sides
            .get(side)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such strip side"))?;
        let f = self
            .faces
            .get(s.face)
            .ok_or_else(|| SolidError::mass_failed(OP, "no such face"))?;
        Ok((f, &s.cn))
    }

    /// What the faces and strips leave open, in the order the enclosure adds it: each
    /// face's closing segments loop by loop, then each strip's parts past the common
    /// parameters and its ends.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a loop's end that is not finite; those of the
    /// bounds ([`segment_length`], [`curve_length`], [`point`]).
    pub(crate) fn openings(&self) -> Result<Openings> {
        let mut out = Openings {
            length: Ivl::from(0.0),
            points: Vec::new(),
            strip_ends: Vec::new(),
        };
        for face in &self.faces {
            for lp in &face.loops {
                for (i, _) in lp.iter().enumerate() {
                    let Some((p, q)) = closing(lp, i) else {
                        continue;
                    };
                    for end in [p, q] {
                        out.points.push(point_at(&face.cn, end)?);
                    }
                    let (a, z) = ([p.x.mid(), p.y.mid()], [q.x.mid(), q.y.mid()]);
                    if a != z {
                        let seg = segment(a, z)?;
                        out.length = out.length + segment_length(&face.cn, &seg)?;
                    }
                }
            }
        }
        for strip in &self.strips {
            for side in 0..2 {
                let (f, cn) = self.side(strip, side)?;
                let (lo, hi) = (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.n as isize));
                for (x0, x1) in [(lo, strip.t.0), (strip.t.1, hi)] {
                    if x0 < x1 {
                        out.length = out.length + curve_length(&f.cn, cn, x0, x1)?;
                    }
                }
            }
            for t in [strip.t.0, strip.t.1] {
                let (fa, ca) = self.side(strip, 0)?;
                let (fc, cc) = self.side(strip, 1)?;
                let (x, y) = (point(&fa.cn, ca, t)?, point(&fc.cn, cc, t)?);
                let d = [y[0] - x[0], y[1] - x[1], y[2] - x[2]];
                out.length = out.length + Ivl::from(super::enclosure::mag3(d));
                out.points.push(x);
                out.strip_ends.push(x);
            }
        }
        for u in &self.unpaired {
            let f = self
                .faces
                .get(u.face)
                .ok_or_else(|| SolidError::mass_failed(OP, "no such face"))?;
            let (lo, hi) = (
                knot(&u.cn.u, u.cn.p as isize),
                knot(&u.cn.u, u.cn.n as isize),
            );
            if lo < hi {
                out.length = out.length + curve_length(&f.cn, &u.cn, lo, hi)?;
            }
            for t in [lo, hi] {
                out.points.push(point(&f.cn, &u.cn, t)?);
            }
        }
        Ok(out)
    }
}

/// The ends of the closing segment before curve `i` of a loop: the previous curve's end
/// and this one's start, as walked.
pub(crate) fn closing(lp: &[LoopCurve], i: usize) -> Option<(V3<Ivl>, V3<Ivl>)> {
    let before = if i == 0 {
        lp.len().checked_sub(1)?
    } else {
        i - 1
    };
    Some((lp.get(before)?.ends.1, lp.get(i)?.ends.0))
}

/// A face of the closed surface: its material side, sign and loops; or why it cannot be.
fn closure_face(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
) -> Result<core::result::Result<ClosureFace, ClosureRefusal>> {
    use crate::topology::edgeuse::p1 as eu1;
    let up = crate::topology::face::p2::upward_faceuse(b, f)?
        .ok_or_else(|| SolidError::mass_missing(OP, "the face's upward faceuse"))?;
    let mate = b
        .faceuse(up)?
        .mate
        .ok_or_else(|| SolidError::mass_missing(OP, "the faceuse's mate"))?;
    let material = |fu| -> Result<core::result::Result<bool, ClosureRefusal>> {
        match eu1::faceuse_region(b, fu)? {
            Some(r) => match region_material(b, r) {
                Ok(m) => Ok(Ok(m)),
                Err(SolidError::UnsureRegion { region, .. }) => {
                    Ok(Err(ClosureRefusal::UnsureRegion(region)))
                }
                Err(e) => Err(e),
            },
            None => Ok(Ok(false)),
        }
    };
    let (mu, mm) = match (material(up)?, material(mate)?) {
        (Ok(a), Ok(z)) => (a, z),
        (Err(why), _) | (_, Err(why)) => return Ok(Err(why)),
    };
    if mu == mm {
        return Ok(Err(ClosureRefusal::NotOneMaterialSide));
    }
    let fu = if mu { up } else { mate };
    let sign = if b.faceuse(fu)?.orientation == OrientType::Same {
        -1.0
    } else {
        1.0
    };
    let surface = b
        .face(f)?
        .surface
        .as_ref()
        .ok_or_else(|| SolidError::mass_missing(OP, "the face's surface"))?;
    let cn = match as_b_spline(k, surface)? {
        Some(bs) => Nurbs::of(&bs)?,
        None => return Ok(Err(ClosureRefusal::NoNurbsForm)),
    };
    // the loops in the order and direction the integrator walks them
    // (`create_oriented_trim_boundary`): the upward use's loops, each edge use's pcurve
    // forward where it is `Same`, backward where `Opposite`
    let mut loops = Vec::new();
    for lu in eu1::faceuse_loopuses(b, up, false, None)? {
        if crate::topology::loopuse::is_vertex_loopuse(b, lu)? {
            continue;
        }
        let mut lp = Vec::new();
        for eu in eu1::loopuse_edgeuses(b, lu)? {
            let dir = if eu1::orientation(b, eu)? == OrientType::Same {
                1.0
            } else {
                -1.0
            };
            let edge = b
                .edgeuse(eu)?
                .edge
                .ok_or_else(|| SolidError::mass_missing(OP, "the edgeuse's edge"))?;
            let Some(cn) = pcurve(k, b, eu)? else {
                return Ok(Err(ClosureRefusal::NoNurbsForm));
            };
            // the curve's start and end point (as walked), enclosed
            let (t0, t1) = (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.n as isize));
            let (a, z) = if dir > 0.0 { (t0, t1) } else { (t1, t0) };
            let first = |d: &[V3<Ivl>]| {
                d.first()
                    .copied()
                    .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no point"))
            };
            let pa = first(&curve_derivs_over(&cn, Ivl::from(a), 0, None)?)?;
            let pz = first(&curve_derivs_over(&cn, Ivl::from(z), 0, None)?)?;
            lp.push(LoopCurve {
                edge,
                cn,
                dir,
                ends: (pa, pz),
            });
        }
        loops.push(lp);
    }
    Ok(Ok(ClosureFace {
        face: f,
        cn,
        sign,
        loops,
    }))
}

/// An edge use's pcurve as NURBS data, made first where it is missing (as the integrator's
/// trim boundary makes it); `None` for a pcurve with no NURBS form.
fn pcurve(k: &Kernel, b: &mut Brep, eu: EdgeuseId) -> Result<Option<NurbsCurve>> {
    let fetch =
        crate::topology::edgeuse::p3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
    fetch.status?;
    let c = crate::topology::edgeuse::p1::uv_trim_curve(b, eu)?
        .0
        .ok_or_else(|| SolidError::mass_missing(OP, "the edgeuse's pcurve"))?;
    let bs: Option<&BSplineCurve> = c.b_spline_base();
    bs.map(NurbsCurve::of).transpose()
}

/// The straight uv segment from `a` to `z` as a degree 1 curve on `[0, 1]`.
pub(crate) fn segment(a: [f64; 2], z: [f64; 2]) -> Result<NurbsCurve> {
    let ok = a.iter().chain(&z).all(|x| x.is_finite());
    if !ok {
        return Err(SolidError::invalid_input(
            OP,
            "a loop's end that is not finite",
        ));
    }
    Ok(NurbsCurve {
        p: 1,
        n: 2,
        u: vec![0.0, 0.0, 1.0, 1.0],
        pw: vec![a[0], a[1], 0.0, 1.0, z[0], z[1], 0.0, 1.0],
    })
}

/// The surface point at a uv box, enclosed.
fn point_at(cn: &Nurbs, uv: V3<Ivl>) -> Result<[Ivl; 3]> {
    let s = super::enclosure::partials_at(cn, uv.x, uv.y, 0)?.at(0, 0);
    Ok([s.x, s.y, s.z])
}

/// A bound on the 3D length of a closing segment on a face with surface `cn`: along a side
/// of the net whose control points are nearly one point (a pole), from their spread; else
/// from the partials over its box.
pub(crate) fn segment_length(cn: &Nurbs, seg: &NurbsCurve) -> Result<Ivl> {
    use super::enclosure::{mag3, partials_at, piece_hull, side_length, width};
    let (u, v) = piece_hull(seg, seg.p);
    if let Some(len) = side_length(cn, u, v) {
        return Ok(len);
    }
    let p = partials_at(cn, u, v, 1)?;
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    Ok(Ivl::from(mag3([su.x, su.y, su.z])) * Ivl::from(width(u))
        + Ivl::from(mag3([sv.x, sv.y, sv.z])) * Ivl::from(width(v)))
}

/// The 3D point of pcurve `pc` on surface `cn` at `t`, enclosed.
pub(crate) fn point(cn: &Nurbs, pc: &NurbsCurve, t: f64) -> Result<[Ivl; 3]> {
    let d = curve_derivs_over(pc, Ivl::from(t), 0, None)?;
    let p = d
        .first()
        .copied()
        .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no point"))?;
    point_at(cn, p)
}

/// A bound on the 3D length of pcurve `pc` on surface `cn` over `[t0, t1]`: the sum over
/// its polynomial pieces in that range (one piece's expansion holds only on its span).
pub(crate) fn curve_length(cn: &Nurbs, pc: &NurbsCurve, t0: f64, t1: f64) -> Result<Ivl> {
    let mut cuts = vec![t0, t1];
    for &x in &pc.u {
        if x > t0 && x < t1 {
            cuts.push(x);
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut sum = Ivl::from(0.0);
    for pair in cuts.windows(2) {
        if let &[a, z] = pair {
            let piece =
                crate::blend::fillet_section::station::span(&pc.u, pc.p, pc.n, 0.5 * (a + z));
            sum = sum + piece_length(cn, pc, piece, a, z)?;
        }
    }
    Ok(sum)
}

/// [`curve_length`] on the polynomial piece `piece` over `[t0, t1]` within it.
fn piece_length(cn: &Nurbs, pc: &NurbsCurve, piece: usize, t0: f64, t1: f64) -> Result<Ivl> {
    use super::enclosure::{mag3, partials_at};
    let d = curve_derivs_over(pc, Ivl::new(t0, t1), 1, Some(piece))?;
    let (p, dp) = (
        d.first()
            .copied()
            .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no point"))?,
        d.get(1)
            .copied()
            .ok_or_else(|| SolidError::mass_failed(OP, "the curve has no derivative"))?,
    );
    let s = partials_at(cn, p.x, p.y, 1)?;
    let (su, sv) = (s.at(1, 0), s.at(0, 1));
    let speed = Ivl::from(mag3([su.x, su.y, su.z])) * Ivl::from(dp.x.mag())
        + Ivl::from(mag3([sv.x, sv.y, sv.z])) * Ivl::from(dp.y.mag());
    Ok(speed * (Ivl::from(t1) - Ivl::from(t0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A side without a side of opposite sense to pair with is an opening, its whole
    /// length: taking a box's strip apart adds both its sides' lengths (each edge at least
    /// 4 long) to `L`.
    #[test]
    fn an_unpaired_side_is_an_opening_its_whole_length() {
        let mut k = Kernel::default();
        let mut b = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::ZERO,
            4.0,
            5.0,
            6.0,
        )
        .unwrap();
        let mut c = Closure::of(&k, &mut b).unwrap().unwrap();
        assert!(c.unpaired.is_empty() && c.strips.len() == 12);
        let closed = c.openings().unwrap().length.hi;
        let strip = c.strips.pop().unwrap();
        c.unpaired.extend(strip.sides);
        let open = c.openings().unwrap().length.hi;
        assert!(open - closed >= 8.0 - 1e-9, "{closed} -> {open}");
    }
}
