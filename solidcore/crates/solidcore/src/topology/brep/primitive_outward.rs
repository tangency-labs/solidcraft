//! The volume's sign of a primitive the kernel's constructors built, proved from the template's
//! closed form instead of the enclosure's cells (not upstream; #14830).
//!
//! A primitive's B-rep is its template `T` (a box, a cylinder, a sphere or a ring torus, from
//! the constructor's inputs) up to rounding: a fitted cap tilts by 1e-16, a rational circle's net
//! holds irrational numbers. The volume [`crate::mass::enclosure::volume_sign`] decides is the
//! flux of `f = (0, 0, z - z0)` out of the enclosure's closed surface: the faces (trimmed by
//! their pcurves and the straight uv segments closing a loop), the strips joining two faces'
//! boundaries along every edge, and the caps over what is left open at the vertices. The
//! template's flux is its closed-form volume `V(T)`. So
//!
//! ```text
//! V(B) >= V(T) - sum_f |Phi_B,f - Phi_T,f| - max |z - z0| (A_strips + A_caps)
//! ```
//!
//! and the volume is positive when the right side's lower bound is, in rounded interval
//! arithmetic ([`Ivl`]). Per face (`g = (z - z0) (S_u x S_v)_z` in the face's own parameters),
//! with `D_B` the region its trims bound and `D_T` the template face's region in the same
//! parameters:
//!
//! ```text
//! |Phi_B,f - Phi_T,f| <= area(dom) max |g_B - g_T| + (2 eps P + pi eps^2) max(|g_B|, |g_T|)
//! ```
//!
//! `eps` is the largest distance between a trim and its template trim at the same parameter, `P`
//! a bound on the template trims' length: the straight-line homotopy from one to the other stays
//! within `eps` of `D_T`'s boundary, so `D_B` and `D_T` differ only in that tube. Each template
//! face is the constructor's recipe written on the B-rep face's own NURBS layout (its degrees and
//! knots; the recipe's spans are Bezier segments, so any increasing knots give the same image):
//! a plane's corners projected onto the template plane, a circle's, cylinder's, sphere's or
//! torus's control points and weights in closed form, enclosed (`sqrt(2) / 2`, `pi`). Its image
//! is exactly the template's face, so the template faces' fluxes add up to `V(T)`.
//! `|g_B - g_T|` comes from the nets' deviation through the basis' bounds (partition of unity,
//! `sum |N_i'| <= 2 p / h`), the strips' and caps' areas from the enclosure's own bounds
//! (`int |D| (|X_1'| + |D'|)`, `L^2 / 4`).
//!
//! A body that is not its template (another face count or kind, a face whose degrees, net size,
//! knot structure or weights are not the recipe's, a trim that is no template trim, a face whose
//! sense is not the template's) gets no certificate: the full proof runs, as for every other
//! body. The certificate asserts the volume's true sign: where the enclosure would leave a valid
//! primitive undecided (a tiny one), it is Positive.
//!
//! | Rust | Upstream |
//! |---|---|
//! | [`Template`], [`is_outward_primitive`], [`certify`] | (not upstream) |
//!
//! Deviations: none (not upstream). Work: O(faces + control points + edges), no subdivision,
//! no recursion. Design: the swarm's primitive proof certificate note, reviewed.

use crate::blend::fillet_section::certificate::NurbsCurve;
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::{as_b_spline, Nurbs};
use crate::kernel::Kernel;
use crate::mass::integrator::properties::region_material;
use crate::math::Point3;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::{Brep, EdgeId, FaceId};
use crate::Result;

const OP: &str = "topology::brep::primitive_outward";

/// The closed form a primitive was built from: the constructor's inputs, in its frame (the world
/// axes, at the given point).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Template {
    /// The axis-aligned box from its least corner, each size positive.
    Block {
        /// The least corner.
        origin: Point3,
        /// The sizes along x, y and z.
        size: [f64; 3],
    },
    /// The cylinder about +Z from its base centre.
    Cylinder {
        /// The base centre.
        origin: Point3,
        /// The radius, positive.
        radius: f64,
        /// The height along +Z, positive.
        height: f64,
    },
    /// The sphere about its centre.
    Sphere {
        /// The centre.
        center: Point3,
        /// The radius, positive.
        radius: f64,
    },
    /// The ring torus about +Z through its centre.
    Torus {
        /// The centre.
        center: Point3,
        /// The major radius, above the minor.
        major: f64,
        /// The minor radius, positive.
        minor: f64,
    },
}

/// Whether the closed body `b`, built by the kernel's constructor for `t`, is a valid solid
/// facing out: the validity check and no unsure region as [`super::is_valid_and_outward`] runs
/// them, then the volume's sign by [`certify`] in place of the enclosure. `false` where any
/// test fails, the certificate's included: then nothing is proved and the full proof runs.
///
/// # Errors
///
/// Those of the manifold test, the validity check and the unsure-region test (as
/// [`super::is_outward_solid`]'s); those of reading the faces' NURBS forms and pcurves.
pub fn is_outward_primitive(k: &mut Kernel, b: &mut Brep, t: &Template) -> Result<bool> {
    if !super::p2::is_manifold_solid(b)? {
        return Ok(false);
    }
    let c = super::validation::check_brep(k, b, super::validation::CheckLevel::Level0)?;
    if !c.pointers.is_passed() || c.assert_valid != Some(true) {
        return Ok(false);
    }
    crate::mass::integrator::properties::no_unsure_region(b)?;
    certify(k, b, t)
}

/// The certificate alone: whether the flux comparison with `t` proves `b`'s volume positive.
/// Pcurves a face lacks are made first, as the enclosure makes them.
///
/// # Errors
///
/// Those of reading the faces' NURBS forms and pcurves; the kernel's cancel.
pub fn certify(k: &Kernel, b: &mut Brep, t: &Template) -> Result<bool> {
    k.check_cancelled(OP)?;
    let Some(model) = model(t) else {
        return Ok(false);
    };
    let faces = super::p1::faces(b, None)?;
    if faces.len() != model.faces.len() {
        return Ok(false);
    }
    let mut used = vec![false; model.faces.len()];
    let mut total = Ivl::from(0.0);
    let mut sides: Vec<(EdgeId, Side)> = Vec::new();
    let mut open = Ivl::from(0.0);
    let mut zmax = Ivl::from(0.0);
    for f in faces {
        k.check_cancelled(OP)?;
        let Some(fc) = face_check(k, b, f, &model, &mut used, &mut sides, &mut open)? else {
            return Ok(false);
        };
        total = total + fc.term;
        zmax = hull_mag(zmax, fc.zmax);
    }
    // the strips: two sides of every edge, in opposite senses
    let mut strips = Ivl::from(0.0);
    let mut edges: Vec<EdgeId> = sides.iter().map(|s| s.0).collect();
    edges.sort_unstable();
    edges.dedup();
    for e in edges {
        let mine: Vec<&Side> = sides.iter().filter(|s| s.0 == e).map(|s| &s.1).collect();
        let (Some(a), Some(c), 2) = (mine.first(), mine.get(1), mine.len()) else {
            return Ok(false);
        };
        let Some((area, d)) = strip(a, c) else {
            return Ok(false);
        };
        strips = strips + area;
        open = open + Ivl::from(2.0) * d;
    }
    // the caps: closed curves of total length L at most, a cone over them of area L^2 / 4, its
    // points within L of the curves
    let l = Ivl::from(open.hi);
    let caps = l * l / Ivl::from(4.0);
    let z = zmax + l;
    let bound = model.volume - total - z * (strips + caps);
    Ok(bound.lo > 0.0)
}

// ---- intervals ------------------------------------------------------------------------------

/// A point of three enclosed coordinates.
type P3 = [Ivl; 3];

const IZ: Ivl = Ivl::new(0.0, 0.0);

/// `sqrt(2) / 2`, enclosed.
fn half_root2() -> Ivl {
    Ivl::from(2.0).sqrt() / Ivl::from(2.0)
}

/// `pi`, enclosed (f64's PI is below it).
fn pi() -> Ivl {
    Ivl::outward_ulp(core::f64::consts::PI, core::f64::consts::PI)
}

/// The upper bound `max(|a|, |b|)` as an interval `[0, hi]`.
fn hull_mag(a: Ivl, b: Ivl) -> Ivl {
    Ivl::new(0.0, a.mag().max(b.mag()))
}

/// `[0, hi]` of a non-negative bound.
fn up(x: Ivl) -> Ivl {
    Ivl::new(0.0, x.hi)
}

/// `|x|` of an interval, as an interval `[mig, mag]`.
fn abs(x: Ivl) -> Ivl {
    Ivl::new(x.mig(), x.mag())
}

/// The Euclidean norm's enclosure of a vector of intervals.
fn norm(v: &[Ivl]) -> Ivl {
    let mut s = IZ;
    for x in v {
        let a = abs(*x);
        s = s + a * a;
    }
    s.sqrt()
}

fn sub3(a: P3, c: P3) -> P3 {
    [a[0] - c[0], a[1] - c[1], a[2] - c[2]]
}

fn add3(a: P3, c: P3) -> P3 {
    [a[0] + c[0], a[1] + c[1], a[2] + c[2]]
}

fn scale3(a: P3, s: Ivl) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot3(a: P3, c: P3) -> Ivl {
    a[0] * c[0] + a[1] * c[1] + a[2] * c[2]
}

fn cross3(a: P3, c: P3) -> P3 {
    [
        a[1] * c[2] - a[2] * c[1],
        a[2] * c[0] - a[0] * c[2],
        a[0] * c[1] - a[1] * c[0],
    ]
}

fn ivl3(p: [f64; 3]) -> P3 {
    [Ivl::from(p[0]), Ivl::from(p[1]), Ivl::from(p[2])]
}

/// The sign of an interval, `None` where it holds 0.
fn sign_of(x: Ivl) -> Option<f64> {
    if x.lo > 0.0 {
        Some(1.0)
    } else if x.hi < 0.0 {
        Some(-1.0)
    } else {
        None
    }
}

// ---- the template ----------------------------------------------------------------------------

/// The unit square's circle in its recipe: nine control points from angle 0 counterclockwise
/// in 45-degree steps (the odd ones the quarter arcs' corners, `(+-1, +-1)`), weights 1 and
/// `sqrt(2) / 2` alternating.
fn circle_dirs() -> Vec<([Ivl; 2], Ivl)> {
    let (one, z, h) = (Ivl::from(1.0), IZ, half_root2());
    let m = Ivl::from(-1.0);
    vec![
        ([one, z], one),
        ([one, one], h),
        ([z, one], one),
        ([m, one], h),
        ([m, z], one),
        ([m, m], h),
        ([z, m], one),
        ([one, m], h),
        ([one, z], one),
    ]
}

/// The recipe of a revolved patch's net: `(u, v)`-indexed points and weights, u-major, `nu`
/// along the circle about +Z and `nv` along the profile.
struct PatchNet {
    nu: usize,
    nv: usize,
    p: usize,
    q: usize,
    /// Euclidean points and weights, u-major.
    pts: Vec<(P3, Ivl)>,
}

/// A template face.
enum ModelFace {
    /// A plane `x[axis] = at` with outward normal sign `out` along `axis`, its region a
    /// rectangle (its four corners) or a disc (centre, radius: its circle in the recipe).
    Plane {
        axis: usize,
        at: Ivl,
        out: f64,
        region: Region,
    },
    /// A revolved patch over its whole domain: the net, the outward direction at a point, and
    /// which domain sides are one point (a pole).
    Patch { net: PatchNet, kind: PatchKind },
}

enum Region {
    Rect(Vec<P3>),
    Disc { center: P3, radius: Ivl },
}

#[derive(Clone, Copy)]
enum PatchKind {
    /// The cylinder's side about the axis through `axis` (x, y).
    Side { axis: [Ivl; 2] },
    /// The sphere about its centre.
    Sphere { center: P3 },
    /// The torus about its centre and axis +Z, major radius.
    Torus { center: P3, major: Ivl },
}

/// A template: its faces, its closed-form volume, and the `z0` of the flux.
struct Model {
    faces: Vec<ModelFace>,
    volume: Ivl,
    z0: f64,
}

fn finite(xs: &[f64]) -> bool {
    xs.iter().all(|x| x.is_finite())
}

/// The template's faces and volume, or `None` for inputs out of the constructors' domain.
#[allow(clippy::too_many_lines)]
fn model(t: &Template) -> Option<Model> {
    match *t {
        Template::Block { origin, size } => {
            if !size.iter().all(|s| s.is_finite() && *s > 0.0)
                || !finite(&[origin.x, origin.y, origin.z])
            {
                return None;
            }
            let o = ivl3([origin.x, origin.y, origin.z]);
            let s = ivl3(size);
            let lo = |k: usize| o.get(k).copied().unwrap_or(Ivl::whole());
            let hi = |k: usize| lo(k) + s.get(k).copied().unwrap_or(Ivl::whole());
            let mut faces = Vec::new();
            for axis in 0..3 {
                for (at, out) in [(lo(axis), -1.0), (hi(axis), 1.0)] {
                    let (a, c) = match axis {
                        0 => (1, 2),
                        1 => (2, 0),
                        _ => (0, 1),
                    };
                    let mut corners = Vec::new();
                    for (ea, ec) in [
                        (lo(a), lo(c)),
                        (hi(a), lo(c)),
                        (lo(a), hi(c)),
                        (hi(a), hi(c)),
                    ] {
                        let mut p = [IZ; 3];
                        for (kk, slot) in p.iter_mut().enumerate() {
                            *slot = if kk == axis {
                                at
                            } else if kk == a {
                                ea
                            } else {
                                ec
                            };
                        }
                        corners.push(p);
                    }
                    faces.push(ModelFace::Plane {
                        axis,
                        at,
                        out,
                        region: Region::Rect(corners),
                    });
                }
            }
            Some(Model {
                faces,
                volume: s[0] * s[1] * s[2],
                z0: origin.z + 0.5 * size[2],
            })
        }
        Template::Cylinder {
            origin,
            radius,
            height,
        } => {
            if !(radius.is_finite()
                && radius > 0.0
                && height.is_finite()
                && height > 0.0
                && finite(&[origin.x, origin.y, origin.z]))
            {
                return None;
            }
            let o = ivl3([origin.x, origin.y, origin.z]);
            let (r, h) = (Ivl::from(radius), Ivl::from(height));
            let top = o[2] + h;
            // the side: the circle about the axis, swept up from the base (u along the circle,
            // v up, degree 1)
            let mut pts = Vec::new();
            for (d, w) in circle_dirs() {
                for z in [o[2], top] {
                    pts.push(([o[0] + d[0] * r, o[1] + d[1] * r, z], w));
                }
            }
            let side = ModelFace::Patch {
                net: PatchNet {
                    nu: 9,
                    nv: 2,
                    p: 2,
                    q: 1,
                    pts,
                },
                kind: PatchKind::Side { axis: [o[0], o[1]] },
            };
            let disc = |at: Ivl, out: f64| ModelFace::Plane {
                axis: 2,
                at,
                out,
                region: Region::Disc {
                    center: [o[0], o[1], at],
                    radius: r,
                },
            };
            Some(Model {
                faces: vec![side, disc(o[2], -1.0), disc(top, 1.0)],
                volume: pi() * r * r * h,
                z0: origin.z + 0.5 * height,
            })
        }
        Template::Sphere { center, radius } => {
            if !(radius.is_finite() && radius > 0.0 && finite(&[center.x, center.y, center.z])) {
                return None;
            }
            let c = ivl3([center.x, center.y, center.z]);
            let r = Ivl::from(radius);
            let m = Ivl::from(-1.0);
            let (one, h) = (Ivl::from(1.0), half_root2());
            // the profile: the half circle from the south pole up to the north, in (rho, z)
            let profile = [
                ([IZ, m], one),
                ([one, m], h),
                ([one, IZ], one),
                ([one, one], h),
                ([IZ, one], one),
            ];
            let mut pts = Vec::new();
            for (d, wu) in circle_dirs() {
                for (pr, wv) in profile {
                    let rho = pr[0] * r;
                    pts.push((
                        [c[0] + d[0] * rho, c[1] + d[1] * rho, c[2] + pr[1] * r],
                        wu * wv,
                    ));
                }
            }
            Some(Model {
                faces: vec![ModelFace::Patch {
                    net: PatchNet {
                        nu: 9,
                        nv: 5,
                        p: 2,
                        q: 2,
                        pts,
                    },
                    kind: PatchKind::Sphere { center: c },
                }],
                volume: Ivl::from(4.0) / Ivl::from(3.0) * pi() * r * r * r,
                z0: center.z,
            })
        }
        Template::Torus {
            center,
            major,
            minor,
        } => {
            if !(minor.is_finite()
                && minor > 0.0
                && major.is_finite()
                && major > minor
                && finite(&[center.x, center.y, center.z]))
            {
                return None;
            }
            let c = ivl3([center.x, center.y, center.z]);
            let (bigr, r) = (Ivl::from(major), Ivl::from(minor));
            let mut pts = Vec::new();
            for (d, wu) in circle_dirs() {
                for (t, wv) in circle_dirs() {
                    let rho = bigr + t[0] * r;
                    pts.push((
                        [c[0] + d[0] * rho, c[1] + d[1] * rho, c[2] + t[1] * r],
                        wu * wv,
                    ));
                }
            }
            Some(Model {
                faces: vec![ModelFace::Patch {
                    net: PatchNet {
                        nu: 9,
                        nv: 9,
                        p: 2,
                        q: 2,
                        pts,
                    },
                    kind: PatchKind::Torus {
                        center: c,
                        major: bigr,
                    },
                }],
                volume: Ivl::from(2.0) * pi() * pi() * bigr * r * r,
                z0: center.z,
            })
        }
    }
}

// ---- bounds on a NURBS patch and its template --------------------------------------------------

/// `sum |N_i'| <= 2 p / h` over a knot vector of degree `p`, `h` its least positive
/// `u[i + p] - u[i]` (0 for degree 0).
fn basis_slope(u: &[f64], p: usize) -> Option<Ivl> {
    if p == 0 {
        return Some(IZ);
    }
    let mut h = f64::INFINITY;
    for (a, z) in u.iter().zip(u.iter().skip(p)) {
        let d = (Ivl::from(*z) - Ivl::from(*a)).lo;
        if d > 0.0 {
            h = h.min(d);
        }
    }
    if !h.is_finite() {
        return None;
    }
    let pf = Ivl::from(f64::from(u32::try_from(p).ok()?));
    Some(Ivl::from(2.0) * pf / Ivl::from(h))
}

/// Bounds over a whole patch domain, B's and its template's (`_t`): `|S|`, `|S_u|`, `|S_v|`,
/// and the differences `|S_B - S_T|`, `|S_u,B - S_u,T|`, `|S_v,B - S_v,T|`, from the two
/// homogeneous nets (same degrees and knots): each coordinate of the homogeneous polynomial
/// `A` is a convex combination of the net, `A_u` at most the net's magnitude times the basis
/// slope; `S = A / w`, `S_u = (A_u - S w_u) / w`, with `w` at least the least weight.
#[derive(Clone, Copy)]
struct NetBounds {
    s: Ivl,
    su: Ivl,
    sv: Ivl,
    su_t: Ivl,
    sv_t: Ivl,
    ds: Ivl,
    dsu: Ivl,
    dsv: Ivl,
}

/// The homogeneous net's magnitudes: `max |(x w, y w, z w)|`, `max w`, `min w`.
fn net_mags(pw: &[[Ivl; 4]]) -> (Ivl, Ivl, Ivl) {
    let (mut a, mut wmax, mut wmin) = (IZ, IZ, f64::INFINITY);
    for c in pw {
        a = hull_mag(a, norm(&[c[0], c[1], c[2]]));
        wmax = hull_mag(wmax, c[3]);
        wmin = wmin.min(c[3].lo);
    }
    (a, wmax, Ivl::from(wmin))
}

fn net_bounds(b: &[[Ivl; 4]], t: &[[Ivl; 4]], ku: Ivl, kv: Ivl) -> Option<NetBounds> {
    let (ab, wb, wminb) = net_mags(b);
    let (at, wt, wmint) = net_mags(t);
    if wminb.lo <= 0.0 || wmint.lo <= 0.0 || b.len() != t.len() {
        return None;
    }
    let mut da = IZ;
    let mut dw = IZ;
    for (x, y) in b.iter().zip(t) {
        da = hull_mag(da, norm(&[x[0] - y[0], x[1] - y[1], x[2] - y[2]]));
        dw = hull_mag(dw, x[3] - y[3]);
    }
    // B's and T's own magnitudes
    let s = ab / wminb;
    let st = at / wmint;
    let one = |a: Ivl, w: Ivl, wmin: Ivl, k: Ivl, s: Ivl| (a * k + s * w * k) / wmin;
    let (su, sv) = (one(ab, wb, wminb, ku, s), one(ab, wb, wminb, kv, s));
    let (su_t, sv_t) = (one(at, wt, wmint, ku, st), one(at, wt, wmint, kv, st));
    // differences: w_B dS = dA - S_T dw; w_B dS_u + dw S_u,T = dA_u - dS w_u,B - S_T dw_u
    let ds = (da + st * dw) / wminb;
    let d1 = |k: Ivl, sx_t: Ivl| (da * k + ds * wb * k + st * dw * k + dw * sx_t) / wminb;
    Some(NetBounds {
        s: up(s),
        su: up(su),
        sv: up(sv),
        su_t: up(su_t),
        sv_t: up(sv_t),
        ds: up(ds),
        dsu: up(d1(ku, su_t)),
        dsv: up(d1(kv, sv_t)),
    })
}

/// The same for a curve's homogeneous net (uv or 3D in the first coordinates): `|c|`, `|c'|`,
/// and `|c_B - c_T|` over its whole range.
fn curve_bounds(b: &[[Ivl; 4]], t: &[[Ivl; 4]], k: Ivl) -> Option<NetBounds> {
    net_bounds(b, t, k, IZ)
}

// ---- one face --------------------------------------------------------------------------------

/// One face's share of the certificate: its flux term and its greatest `|z - z0|`.
struct FaceCheck {
    term: Ivl,
    zmax: Ivl,
}

/// A strip side: one face's view of an edge.
#[derive(Clone, Debug)]
struct Side {
    /// The pcurve's knots, bit for bit (the edge's parameter).
    knots: Vec<f64>,
    /// Its distance from its template pcurve at the same parameter.
    eps: Ivl,
    /// The template edge: the template face along the template pcurve, as a homogeneous 3D
    /// curve on `knots` (degree `p`).
    edge: Vec<[Ivl; 4]>,
    p: usize,
    /// `|S_u| + |S_v|` (a Lipschitz bound in uv), `|S_B - S_T|` on the face, `|c'|` in uv.
    lip: Ivl,
    ds: Ivl,
    speed: Ivl,
    forward: bool,
}

/// A homogeneous net `(x w, y w, z w, w)` read four at a time.
fn quads(v: &[f64]) -> Vec<[Ivl; 4]> {
    let mut out = Vec::new();
    let mut it = v.iter();
    while let (Some(a), Some(b), Some(c), Some(d)) = (it.next(), it.next(), it.next(), it.next()) {
        out.push([Ivl::from(*a), Ivl::from(*b), Ivl::from(*c), Ivl::from(*d)]);
    }
    out
}

fn hom_of(p: P3, w: Ivl) -> [Ivl; 4] {
    [p[0] * w, p[1] * w, p[2] * w, w]
}

/// The knot vector's structure for a Bezier-spanned recipe: `p + 1` ends, interior knots of
/// multiplicity `p` (each span its own Bezier segment), increasing; and its first and last.
fn bezier_knots(u: &[f64], p: usize, n: usize) -> Option<(f64, f64)> {
    if u.len() != n.checked_add(p)?.checked_add(1)? {
        return None;
    }
    let a = *u.first()?;
    let z = *u.last()?;
    if a.partial_cmp(&z) != Some(core::cmp::Ordering::Less) {
        return None;
    }
    let mut it = u.iter().copied().peekable();
    let mut runs = Vec::new();
    while let Some(x) = it.next() {
        let mut m = 1_usize;
        while it.peek() == Some(&x) {
            it.next();
            m = m.saturating_add(1);
        }
        runs.push((x, m));
    }
    let (Some(first), Some(last)) = (runs.first(), runs.last()) else {
        return None;
    };
    let ends = p.checked_add(1)?;
    if first.1 != ends || last.1 != ends {
        return None;
    }
    for (_, m) in runs.iter().skip(1).take(runs.len().saturating_sub(2)) {
        if *m != p {
            return None;
        }
    }
    if runs
        .iter()
        .zip(runs.iter().skip(1))
        .any(|(x, y)| x.0 >= y.0)
    {
        return None;
    }
    Some((a, z))
}

/// The face's sense as the enclosure takes it: -1 where its material side's use is `Same`.
fn face_sign(b: &Brep, f: FaceId) -> Result<Option<(f64, crate::topology::FaceuseId)>> {
    let Some(up) = crate::topology::face::p2::upward_faceuse(b, f)? else {
        return Ok(None);
    };
    let Some(mate) = b.faceuse(up)?.mate else {
        return Ok(None);
    };
    let material = |fu| -> Result<bool> {
        match eu1::faceuse_region(b, fu)? {
            Some(r) => region_material(b, r),
            None => Ok(false),
        }
    };
    let (mu, mm) = (material(up)?, material(mate)?);
    if mu == mm {
        return Ok(None);
    }
    let fu = if mu { up } else { mate };
    let sign = if b.faceuse(fu)?.orientation == OrientType::Same {
        -1.0
    } else {
        1.0
    };
    Ok(Some((sign, up)))
}

/// The face's one trim loop, as walked: each edge use's edge, pcurve and sense.
fn the_loop(
    k: &Kernel,
    b: &mut Brep,
    up: crate::topology::FaceuseId,
) -> Result<Option<Vec<(EdgeId, NurbsCurve, bool)>>> {
    let mut lps = Vec::new();
    for lu in eu1::faceuse_loopuses(b, up, false, None)? {
        if !crate::topology::loopuse::is_vertex_loopuse(b, lu)? {
            lps.push(lu);
        }
    }
    let [lu] = lps.as_slice() else {
        return Ok(None);
    };
    let mut walk = Vec::new();
    for eu in eu1::loopuse_edgeuses(b, *lu)? {
        let same = eu1::orientation(b, eu)? == OrientType::Same;
        let Some(e) = b.edgeuse(eu)?.edge else {
            return Ok(None);
        };
        let Some(c) = pcurve(k, b, eu)? else {
            return Ok(None);
        };
        walk.push((e, c, same));
    }
    Ok(Some(walk))
}

/// Checks face `f` against an unused template face and returns its term, or `None` (no
/// certificate). Its pcurves' sides are pushed to `sides`, the closing pieces' 3D lengths
/// added to `open`.
fn face_check(
    k: &Kernel,
    b: &mut Brep,
    f: FaceId,
    model: &Model,
    used: &mut [bool],
    sides: &mut Vec<(EdgeId, Side)>,
    open: &mut Ivl,
) -> Result<Option<FaceCheck>> {
    let Some((sign, up)) = face_sign(b, f)? else {
        return Ok(None);
    };
    let Some(surface) = b.face(f)?.surface.clone() else {
        return Ok(None);
    };
    let Some(bs) = as_b_spline(k, &surface)? else {
        return Ok(None);
    };
    let cn = Nurbs::of(&bs)?;
    let Some(walk) = the_loop(k, b, up)? else {
        return Ok(None);
    };
    let bnet = quads(&cn.pw);
    // the unused template faces, a plane by its distance from B's net (nearest first)
    let mut order: Vec<(f64, usize)> = Vec::new();
    for (i, mf) in model.faces.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        let score = match mf {
            ModelFace::Plane { axis, at, .. } => bnet
                .iter()
                .map(|c| (c.get(*axis).copied().unwrap_or(Ivl::whole()) / c[3] - *at).mag())
                .fold(0.0_f64, f64::max),
            ModelFace::Patch { .. } => 0.0,
        };
        order.push((score, i));
    }
    order.sort_by(|a, c| a.0.total_cmp(&c.0).then(a.1.cmp(&c.1)));
    for (_, i) in order {
        let Some(mf) = model.faces.get(i) else {
            continue;
        };
        let found = match mf {
            ModelFace::Plane {
                axis,
                at,
                out,
                region,
            } => plane_face(&cn, &bnet, &walk, sign, *axis, *at, *out, region, model.z0),
            ModelFace::Patch { net, kind } => {
                patch_face(&cn, &bnet, &walk, sign, net, *kind, model.z0)
            }
        };
        if let Some((fc, mut s, o)) = found {
            if let Some(u) = used.get_mut(i) {
                *u = true;
            }
            sides.append(&mut s);
            *open = *open + o;
            return Ok(Some(fc));
        }
    }
    Ok(None)
}

/// The flux term `area(dom) max |dg| + tube max |g|`, with `g = (z - z0) (S_u x S_v)_z`.
fn flux_term(nb: &NetBounds, z0: f64, area: Ivl, eps: Ivl, perimeter: Ivl) -> FaceCheck {
    let zb = nb.s + abs(Ivl::from(z0));
    let zt = (nb.s + nb.ds) + abs(Ivl::from(z0));
    let g = zb * nb.su * nb.sv;
    let gt = zt * nb.su_t * nb.sv_t;
    let dg = nb.ds * nb.su_t * nb.sv_t + zb * (nb.dsu * nb.sv + nb.su_t * nb.dsv);
    let tube = Ivl::from(2.0) * eps * perimeter + pi() * eps * eps;
    let term = area * dg + tube * hull_mag(g, gt);
    FaceCheck {
        term: up(term),
        zmax: up(hull_mag(zb, zt)),
    }
}

type Found = Option<(FaceCheck, Vec<(EdgeId, Side)>, Ivl)>;

/// A plane face: bilinear, non-rational. The template parameterisation is B's corners projected
/// onto the template plane (for a rectangle, the rectangle's corners themselves).
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn plane_face(
    cn: &Nurbs,
    bnet: &[[Ivl; 4]],
    walk: &[(EdgeId, NurbsCurve, bool)],
    sign: f64,
    axis: usize,
    at: Ivl,
    out: f64,
    region: &Region,
    z0: f64,
) -> Found {
    if cn.p != 1 || cn.q != 1 || cn.nu != 2 || cn.nv != 2 {
        return None;
    }
    if bnet.iter().any(|c| c[3] != Ivl::from(1.0)) {
        return None;
    }
    let pts: Vec<P3> = bnet.iter().map(|c| [c[0], c[1], c[2]]).collect();
    // on this plane: every corner near it (within a tenth of the net's spread)
    let spread = pts
        .iter()
        .flat_map(|p| pts.iter().map(move |q| norm(&sub3(*p, *q)).hi))
        .fold(0.0_f64, f64::max);
    let off = |p: &P3| (p.get(axis).copied().unwrap_or(Ivl::whole()) - at).mag();
    if pts.iter().any(|p| off(p) > 0.1 * spread) {
        return None;
    }
    // the template net
    let tnet: Vec<P3> = match region {
        Region::Rect(corners) => {
            let mut t = Vec::new();
            let mut taken = [false; 4];
            for p in &pts {
                let mut near: Option<(usize, f64)> = None;
                for (ci, c) in corners.iter().enumerate() {
                    let d = norm(&sub3(*p, *c)).hi;
                    if near.is_none_or(|(_, x)| d < x) {
                        near = Some((ci, d));
                    }
                }
                let (ci, _) = near?;
                match (taken.get_mut(ci), corners.get(ci)) {
                    (Some(tk), Some(c)) if !*tk => {
                        *tk = true;
                        t.push(*c);
                    }
                    _ => return None,
                }
            }
            t
        }
        Region::Disc { .. } => pts
            .iter()
            .map(|p| {
                let mut q = *p;
                if let Some(c) = q.get_mut(axis) {
                    *c = at;
                }
                q
            })
            .collect(),
    };
    let (Some(t00), Some(t01), Some(t10), Some(t11)) =
        (tnet.first(), tnet.get(1), tnet.get(2), tnet.get(3))
    else {
        return None;
    };
    // affine: no twist
    let twist = sub3(add3(*t00, *t11), add3(*t01, *t10));
    if twist.iter().any(|x| x.lo > 0.0 || x.hi < 0.0) {
        return None;
    }
    let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) = (
        cn.u.get(cn.p),
        cn.u.get(cn.nu),
        cn.v.get(cn.q),
        cn.v.get(cn.nv),
    ) else {
        return None;
    };
    let (du, dv) = (Ivl::from(u1) - Ivl::from(u0), Ivl::from(v1) - Ivl::from(v0));
    if du.lo <= 0.0 || dv.lo <= 0.0 {
        return None;
    }
    // the template's orientation against the outward normal
    let (a, c) = (sub3(*t10, *t00), sub3(*t01, *t00));
    let n = cross3(a, c);
    let sigma = sign_of(n.get(axis).copied().unwrap_or(Ivl::whole()) * Ivl::from(out))?;
    let tnet_h: Vec<[Ivl; 4]> = tnet.iter().map(|p| hom_of(*p, Ivl::from(1.0))).collect();
    let (Some(ku), Some(kv)) = (basis_slope(&cn.u, 1), basis_slope(&cn.v, 1)) else {
        return None;
    };
    let nb = net_bounds(bnet, &tnet_h, ku, kv)?;
    let lip = nb.su + nb.sv;
    // the template map T(u, v) = t00 + a (u - u0) / du + c (v - v0) / dv and its inverse in the
    // plane's two other coordinates
    let (ia, ic) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let g = |p: &P3, i: usize| p.get(i).copied().unwrap_or(Ivl::whole());
    let det = g(&a, ia) * g(&c, ic) - g(&a, ic) * g(&c, ia);
    sign_of(det)?;
    let map = |uv: [Ivl; 2]| -> P3 {
        let s = (uv[0] - Ivl::from(u0)) / du;
        let r = (uv[1] - Ivl::from(v0)) / dv;
        add3(*t00, add3(scale3(a, s), scale3(c, r)))
    };
    let unmap = |p: &P3| -> [Ivl; 2] {
        let d = sub3(*p, *t00);
        let s = (g(&d, ia) * g(&c, ic) - g(&d, ic) * g(&c, ia)) / det;
        let r = (g(&a, ia) * g(&d, ic) - g(&a, ic) * g(&d, ia)) / det;
        [Ivl::from(u0) + s * du, Ivl::from(v0) + r * dv]
    };
    let mut sides = Vec::new();
    let (eps, perimeter, wind, open);
    match region {
        Region::Rect(_) => {
            // the domain's sides, as for a patch, with no pole
            let corners_uv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
            let at3 = |ci: usize| -> Option<P3> {
                match ci {
                    0 => Some(*t00),
                    1 => Some(*t10),
                    2 => Some(*t11),
                    3 => Some(*t01),
                    _ => None,
                }
            };
            let no_pole = |_: usize| false;
            let r = domain_loop(walk, sign, &corners_uv, &no_pole, &at3, lip, nb.ds, None)?;
            (eps, wind) = (r.eps, r.wind);
            sides = r.sides;
            open = r.open;
            perimeter = Ivl::from(2.0) * (du + dv);
        }
        Region::Disc { center, radius } => {
            // one closed pcurve, the template circle's preimage in the recipe's layout
            let [(e, c, same)] = walk else {
                return None;
            };
            let dirs = circle_dirs();
            if c.p != 2 || c.n != dirs.len() || bezier_knots(&c.u, 2, c.n).is_none() {
                return None;
            }
            let circle: Vec<(P3, Ivl)> = dirs
                .iter()
                .map(|(d, w)| {
                    let mut p = *center;
                    if let (Some(pa), Some(pc)) = (p.get(ia).copied(), p.get(ic).copied()) {
                        if let Some(s) = p.get_mut(ia) {
                            *s = pa + d[0] * *radius;
                        }
                        if let Some(s) = p.get_mut(ic) {
                            *s = pc + d[1] * *radius;
                        }
                    }
                    (p, *w)
                })
                .collect();
            let bc = quads(&c.pw);
            // B's pcurve points in 3D (on its own plane), against the circle in either sense
            let b3: Vec<P3> = bc.iter().map(|h| map([h[0] / h[3], h[1] / h[3]])).collect();
            let fwd = b3
                .iter()
                .zip(&circle)
                .map(|(p, q)| norm(&sub3(*p, q.0)).hi)
                .fold(0.0, f64::max);
            let rev = b3
                .iter()
                .zip(circle.iter().rev())
                .map(|(p, q)| norm(&sub3(*p, q.0)).hi)
                .fold(0.0, f64::max);
            let order: Vec<(P3, Ivl)> = if fwd <= rev {
                circle.clone()
            } else {
                circle.iter().rev().copied().collect()
            };
            // the template pcurve: the circle's points mapped back, the recipe's weights
            let ct: Vec<[Ivl; 4]> = order
                .iter()
                .map(|(p, w)| {
                    let uv = unmap(p);
                    [uv[0] * *w, uv[1] * *w, IZ, *w]
                })
                .collect();
            let (Some(&ta), Some(&tb)) = (c.u.first(), c.u.last()) else {
                return None;
            };
            let span = Ivl::from(tb) - Ivl::from(ta);
            let kc = basis_slope(&c.u, c.p)?;
            let cb = curve_bounds(&bc, &ct, kc)?;
            eps = cb.ds;
            perimeter = span * cb.su_t;
            // the winding: the template control polygon's signed area (convex, as the circle)
            let mut area = IZ;
            for (x, y) in ct.iter().zip(ct.iter().skip(1)) {
                area = area + (x[0] / x[3]) * (y[1] / y[3]) - (y[0] / y[3]) * (x[1] / x[3]);
            }
            let w = match sign_of(area) {
                Some(w) if *same => w,
                Some(w) => -w,
                None => return None,
            };
            wind = w;
            // the closing gap: its own start and end, within eps of the circle's one point
            open = lip * Ivl::from(2.0) * eps;
            let edge: Vec<[Ivl; 4]> = order.iter().map(|(p, w)| hom_of(*p, *w)).collect();
            sides.push((
                *e,
                Side {
                    knots: c.u.clone(),
                    eps,
                    edge,
                    p: 2,
                    lip,
                    ds: nb.ds,
                    speed: cb.su,
                    forward: *same == (sign > 0.0),
                },
            ));
        }
    }
    if sign * wind != sigma {
        return None;
    }
    let fc = flux_term(&nb, z0, du * dv, eps, perimeter);
    Some((fc, sides, open))
}

/// A patch's view for [`domain_loop`]: its recipe net, B's NURBS form, the template edge along a
/// side (`row(side, along_u)`), and `|S_u,B - S_u,T|`, `|S_v,B - S_v,T|`.
type PatchCtx<'a> = (
    &'a PatchNet,
    &'a Nurbs,
    &'a dyn Fn(usize, bool) -> Option<Vec<[Ivl; 4]>>,
    Ivl,
    Ivl,
);

/// What a loop around the domain's sides gives: its largest trim deviation, its winding, its
/// sides and its closing pieces' 3D length.
struct DomainLoop {
    eps: Ivl,
    wind: f64,
    sides: Vec<(EdgeId, Side)>,
    open: Ivl,
}

/// The next corner of the domain, counterclockwise from (u0, v0), (u1, v0), (u1, v1), (u0, v1).
const fn next(i: usize) -> usize {
    match i {
        0 => 1,
        1 => 2,
        2 => 3,
        _ => 0,
    }
}

/// The side from corner `a` to the next or previous one, by its lower corner.
fn side_of(a: usize, z: usize) -> Option<usize> {
    if next(a) == z {
        Some(a)
    } else if next(z) == a {
        Some(z)
    } else {
        None
    }
}

/// A loop of degree-1 pcurves along the domain's sides, each its own side's template (a domain
/// side with the same parameter), in turn round the domain; a side the loop skips must be a pole
/// of the template (`pole(side)`), crossed by the enclosure's closing segment. `edge_of(ci, cz)`
/// gives the template edge along a side (for a plane, its two 3D corners).
#[allow(clippy::too_many_arguments)]
fn domain_loop(
    walk: &[(EdgeId, NurbsCurve, bool)],
    sign: f64,
    corners_uv: &[[f64; 2]; 4],
    pole: &dyn Fn(usize) -> bool,
    at3: &dyn Fn(usize) -> Option<P3>,
    lip: Ivl,
    ds: Ivl,
    patch: Option<PatchCtx<'_>>,
) -> Option<DomainLoop> {
    let mut eps = IZ;
    let mut seen = [false; 4];
    let mut wind = IZ;
    let mut sides = Vec::new();
    let mut open = IZ;
    let mut prev_end: Option<usize> = None;
    let mut first_start: Option<usize> = None;
    let mut eps_ends = Vec::new();
    let shoelace = |s: usize, e: usize| -> Option<Ivl> {
        let (ps, pe) = (corners_uv.get(s)?, corners_uv.get(e)?);
        Some(Ivl::from(ps[0]) * Ivl::from(pe[1]) - Ivl::from(pe[0]) * Ivl::from(ps[1]))
    };
    // a skipped side, crossed by a closing segment from `pe` to `ps`
    let close = |pe: usize, ps: usize, seen: &mut [bool; 4], wind: &mut Ivl, open: &mut Ivl| {
        if pe == ps {
            return Some(());
        }
        let s = side_of(pe, ps)?;
        if !pole(s) || seen.get(s).copied().unwrap_or(true) {
            return None;
        }
        if let Some(x) = seen.get_mut(s) {
            *x = true;
        }
        *wind = *wind + shoelace(pe, ps)?;
        // a pole's closing segment: the template is one point along it, so its 3D length is at
        // most its uv length times |S_B,u - S_T,u| (or v)
        let (a, z) = (corners_uv.get(pe)?, corners_uv.get(ps)?);
        let len = norm(&[
            Ivl::from(z[0]) - Ivl::from(a[0]),
            Ivl::from(z[1]) - Ivl::from(a[1]),
        ]);
        let d = patch.map_or(ds, |(_, _, _, dsu, dsv)| hull_mag(dsu, dsv));
        *open = *open + len * d;
        Some(())
    };
    for (e, c, same) in walk {
        if c.p != 1 || c.n != 2 {
            return None;
        }
        let pts = quads(&c.pw);
        if pts.iter().any(|h| h[3] != Ivl::from(1.0)) {
            return None;
        }
        let mut snapped = Vec::new();
        let mut e_here = IZ;
        for h in &pts {
            let mut near: Option<(usize, Ivl)> = None;
            for (ci, q) in corners_uv.iter().enumerate() {
                let d = norm(&[h[0] - Ivl::from(q[0]), h[1] - Ivl::from(q[1])]);
                if near.is_none_or(|(_, x)| d.hi < x.hi) {
                    near = Some((ci, d));
                }
            }
            let (ci, d) = near?;
            snapped.push(ci);
            e_here = hull_mag(e_here, d);
        }
        let (Some(&a), Some(&z)) = (snapped.first(), snapped.get(1)) else {
            return None;
        };
        let side = side_of(a, z)?;
        let (start, end) = if *same { (a, z) } else { (z, a) };
        if seen.get(side).copied().unwrap_or(true) {
            return None;
        }
        if let Some(s) = seen.get_mut(side) {
            *s = true;
        }
        match prev_end {
            Some(pe) => {
                close(pe, start, &mut seen, &mut wind, &mut open)?;
            }
            None => first_start = Some(start),
        }
        prev_end = Some(end);
        let sl = shoelace(start, end)?;
        wind = wind + sl;
        eps = hull_mag(eps, e_here);
        eps_ends.push(e_here);
        let (Some(&ta), Some(&tb)) = (c.u.first(), c.u.last()) else {
            return None;
        };
        let tspan = Ivl::from(tb) - Ivl::from(ta);
        if tspan.lo <= 0.0 {
            return None;
        }
        let [p0, p1] = pts.as_slice() else {
            return None;
        };
        let speed = norm(&[p1[0] - p0[0], p1[1] - p0[1]]) / tspan;
        // the template edge on the edge's parameter
        let edge = match patch {
            None => {
                // a plane: the template map is affine, so the edge is the segment between the
                // corners at the pcurve's two ends
                let (Some(x0), Some(x1)) = (at3(a), at3(z)) else {
                    return None;
                };
                vec![hom_of(x0, Ivl::from(1.0)), hom_of(x1, Ivl::from(1.0))]
            }
            Some((_, cn, row, _, _)) => {
                // a patch: the pcurve must run along its side with the surface's own
                // parameter (an iso line, t the side's u or v), and the edge is that row
                // or column of the template net
                let along_u = side == 0 || side == 2;
                let (knots, lo, hi) = if along_u {
                    (&cn.u, cn.u.first(), cn.u.last())
                } else {
                    (&cn.v, cn.v.first(), cn.v.last())
                };
                if c.u.first() != lo || c.u.last() != hi {
                    return None;
                }
                // increasing along the side's parameter as t increases
                let (q0, q1) = (corners_uv.get(a), corners_uv.get(z));
                let increasing = match (q0, q1) {
                    (Some(q0), Some(q1)) => {
                        if along_u {
                            q0[0] < q1[0]
                        } else {
                            q0[1] < q1[1]
                        }
                    }
                    _ => return None,
                };
                if !increasing || knots.len() < 2 {
                    return None;
                }
                row(side, along_u)?
            }
        };
        let p = if patch.is_some() {
            if side == 0 || side == 2 {
                patch.map_or(1, |(n, _, _, _, _)| n.p)
            } else {
                patch.map_or(1, |(n, _, _, _, _)| n.q)
            }
        } else {
            1
        };
        let knots = match patch {
            Some((_, cn, _, _, _)) if side == 0 || side == 2 => cn.u.clone(),
            Some((_, cn, _, _, _)) => cn.v.clone(),
            None => c.u.clone(),
        };
        sides.push((
            *e,
            Side {
                knots,
                eps: e_here,
                edge,
                p,
                lip,
                ds,
                speed,
                forward: *same == (sign > 0.0),
            },
        ));
    }
    match (prev_end, first_start) {
        (Some(pe), Some(fs)) => {
            close(pe, fs, &mut seen, &mut wind, &mut open)?;
        }
        _ => return None,
    }
    if seen.iter().any(|s| !s) {
        return None;
    }
    // the gaps between one pcurve's end and the next one's start: within their two deviations
    for (x, y) in eps_ends.iter().zip(eps_ends.iter().cycle().skip(1)) {
        open = open + lip * (*x + *y);
    }
    let w = sign_of(wind)?;
    Some(DomainLoop {
        eps,
        wind: w,
        sides,
        open,
    })
}

/// A revolved patch over its whole domain: the recipe's degrees and net size, Bezier knots, the
/// template net on B's knots, its orientation from one regular point.
fn patch_face(
    cn: &Nurbs,
    bnet: &[[Ivl; 4]],
    walk: &[(EdgeId, NurbsCurve, bool)],
    sign: f64,
    net: &PatchNet,
    kind: PatchKind,
    z0: f64,
) -> Found {
    if cn.p != net.p || cn.q != net.q || cn.nu != net.nu || cn.nv != net.nv {
        return None;
    }
    let (Some((u0, u1)), Some((v0, v1))) = (
        bezier_knots(&cn.u, cn.p, cn.nu),
        bezier_knots(&cn.v, cn.q, cn.nv),
    ) else {
        return None;
    };
    let tnet: Vec<[Ivl; 4]> = net.pts.iter().map(|(p, w)| hom_of(*p, *w)).collect();
    let (Some(ku), Some(kv)) = (basis_slope(&cn.u, cn.p), basis_slope(&cn.v, cn.q)) else {
        return None;
    };
    let nb = net_bounds(bnet, &tnet, ku, kv)?;
    let at = |i: usize, j: usize| net.pts.get(i.checked_mul(net.nv)?.checked_add(j)?).copied();
    // the orientation: at the net point (0, j) on the surface (a span end), T_u along
    // P(1, j) - P(0, j) and T_v along P(0, j + 1) - P(0, j), against the outward normal there
    let j = match kind {
        PatchKind::Sphere { .. } => 2,
        _ => 0,
    };
    let (Some((p0, _)), Some((pu, _)), Some((pv, _))) =
        (at(0, j), at(1, j), at(0, j.saturating_add(1)))
    else {
        return None;
    };
    let n = cross3(sub3(pu, p0), sub3(pv, p0));
    let outward = match kind {
        PatchKind::Side { axis } => [p0[0] - axis[0], p0[1] - axis[1], IZ],
        PatchKind::Sphere { center } => sub3(p0, center),
        PatchKind::Torus { center, major } => {
            // from the core circle's nearest point
            let d = [p0[0] - center[0], p0[1] - center[1]];
            let rho = norm(&d);
            if rho.lo <= 0.0 {
                return None;
            }
            let core = [
                center[0] + d[0] / rho * major,
                center[1] + d[1] / rho * major,
                center[2],
            ];
            sub3(p0, core)
        }
    };
    let sigma = sign_of(dot3(n, outward))?;
    // the domain's sides: a side whose template row is one point is a pole
    let corners_uv = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let row = |side: usize, along_u: bool| -> Option<Vec<[Ivl; 4]>> {
        let pts: Vec<(P3, Ivl)> = if along_u {
            let j = if side == 0 { 0 } else { net.nv.checked_sub(1)? };
            (0..net.nu).map(|i| at(i, j)).collect::<Option<Vec<_>>>()?
        } else {
            let i = if side == 1 { net.nu.checked_sub(1)? } else { 0 };
            (0..net.nv).map(|j| at(i, j)).collect::<Option<Vec<_>>>()?
        };
        Some(pts.iter().map(|(p, w)| hom_of(*p, *w)).collect())
    };
    let pole = |side: usize| -> bool {
        let along_u = side == 0 || side == 2;
        row(side, along_u).is_some_and(|r| {
            r.first().is_some_and(|f| {
                r.iter().all(|h| {
                    let (x, y) = (
                        [h[0] / h[3], h[1] / h[3], h[2] / h[3]],
                        [f[0] / f[3], f[1] / f[3], f[2] / f[3]],
                    );
                    norm(&sub3(x, y)).hi == 0.0
                        || x.iter().zip(&y).all(|(a, c)| a.lo <= c.hi && c.lo <= a.hi)
                })
            })
        })
    };
    let lip = nb.su + nb.sv;
    let none3 = |_: usize| -> Option<P3> { None };
    let r = domain_loop(
        walk,
        sign,
        &corners_uv,
        &pole,
        &none3,
        lip,
        nb.ds,
        Some((net, cn, &row, nb.dsu, nb.dsv)),
    )?;
    if sign * r.wind != sigma {
        return None;
    }
    let (du, dv) = (Ivl::from(u1) - Ivl::from(u0), Ivl::from(v1) - Ivl::from(v0));
    let fc = flux_term(&nb, z0, du * dv, r.eps, Ivl::from(2.0) * (du + dv));
    Some((fc, r.sides, r.open))
}

// ---- strips ----------------------------------------------------------------------------------

/// The strip along one edge between its two sides: its area bound and its width `|D|` bound,
/// or `None` where the sides do not pair (the same parameter bit for bit, opposite senses, the
/// same template edge).
fn strip(a: &Side, c: &Side) -> Option<(Ivl, Ivl)> {
    if a.knots != c.knots || a.forward == c.forward || a.p != c.p || a.edge.len() != c.edge.len() {
        return None;
    }
    // the two template edges on the same knots and degree: their difference from their nets'
    let k = basis_slope(&a.knots, a.p)?;
    let gap = curve_bounds(&a.edge, &c.edge, k)?.ds;
    // |D| <= |X_a - X_T,a| + |X_T,a - X_T,c| + |X_T,c - X_c|
    let d = up(a.lip * a.eps + a.ds + gap + c.lip * c.eps + c.ds);
    // area <= int |D| (|X_a'| + |D'|) <= length * |D| * (2 |X_a'| + |X_c'|), |X'| <= lip |c'|
    let xa = a.lip * a.speed;
    let xc = c.lip * c.speed;
    let len = Ivl::from(*a.knots.last()?) - Ivl::from(*a.knots.first()?);
    let area = len * d * (Ivl::from(2.0) * xa + xc);
    Some((up(area), d))
}

/// An edge use's pcurve as NURBS data, made first where it is missing (as the enclosure
/// makes it); `None` where it has none or no NURBS form.
fn pcurve(k: &Kernel, b: &mut Brep, eu: crate::topology::EdgeuseId) -> Result<Option<NurbsCurve>> {
    let fetch =
        crate::topology::edgeuse::p3::get_or_create_uv_trim_curve(k, b, eu, false, false, true)?;
    if fetch.status.is_err() {
        return Ok(None);
    }
    let Some(c) = eu1::uv_trim_curve(b, eu)?.0 else {
        return Ok(None);
    };
    let Some(bs) = c.b_spline_base() else {
        return Ok(None);
    };
    Ok(NurbsCurve::of(bs).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives as p;

    fn pt(o: [f64; 3]) -> Point3 {
        Point3::new(o[0], o[1], o[2])
    }

    fn agree(k: &mut Kernel, b: &mut Brep, t: &Template, what: &str) {
        assert!(is_outward_primitive(k, b, t).unwrap(), "{what}");
        let sign = crate::mass::enclosure::volume_sign(k, b).unwrap();
        assert!(
            matches!(sign, crate::mass::enclosure::VolumeSign::Positive(_)),
            "{what}"
        );
    }

    /// Each primitive the constructors build is certified, at several sizes and places, and
    /// the full proof agrees (Positive).
    #[test]
    fn each_primitive_is_certified_by_its_closed_form() {
        for (o, s) in [
            ([1.0, 2.0, 3.0], [2.0, 3.0, 4.0]),
            ([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]),
            ([-5.5, 0.1, 7.3], [0.3, 12.0, 0.7]),
            ([1e3, -2e3, 5e2], [10.0, 20.0, 30.0]),
        ] {
            let mut k = Kernel::default();
            let mut b = p::create_box(&mut k, pt(o), s[0], s[1], s[2]).unwrap();
            agree(
                &mut k,
                &mut b,
                &Template::Block {
                    origin: pt(o),
                    size: s,
                },
                "box",
            );
        }
        for (o, r, h) in [
            ([1.0, 2.0, 3.0], 3.0, 10.0),
            ([0.0, 0.0, 0.0], 5.0, 10.0),
            ([-4.0, 7.5, 0.25], 0.4, 12.0),
        ] {
            let mut k = Kernel::default();
            let mut b = p::create_cylinder(&mut k, pt(o), r, h).unwrap();
            let t = Template::Cylinder {
                origin: pt(o),
                radius: r,
                height: h,
            };
            agree(&mut k, &mut b, &t, "cylinder");
        }
        for (c, r) in [([1.0, 2.0, 3.0], 5.0), ([0.0, 0.0, 0.0], 0.75)] {
            let mut k = Kernel::default();
            let mut b = p::create_sphere(&mut k, pt(c), r).unwrap();
            agree(
                &mut k,
                &mut b,
                &Template::Sphere {
                    center: pt(c),
                    radius: r,
                },
                "sphere",
            );
        }
        for (c, bigr, r) in [([1.0, 2.0, 3.0], 10.0, 3.0), ([0.0, 0.0, 0.0], 2.0, 1.5)] {
            let mut k = Kernel::default();
            let mut b = p::create_torus(&mut k, pt(c), bigr, r).unwrap();
            let t = Template::Torus {
                center: pt(c),
                major: bigr,
                minor: r,
            };
            agree(&mut k, &mut b, &t, "torus");
        }
    }

    /// The deviation terms are what certify: a template far from the body has V(T) > 0 alone,
    /// but its faces' terms exceed it, so nothing is certified; nor for a template elsewhere,
    /// or of another kind.
    #[test]
    fn a_body_far_from_its_template_is_not_certified() {
        let mut k = Kernel::default();
        let mut b = p::create_box(&mut k, pt([0.0, 0.0, 0.0]), 1.0, 1.0, 1.0).unwrap();
        for t in [
            Template::Block {
                origin: pt([0.0, 0.0, 0.0]),
                size: [1000.0, 1000.0, 1000.0],
            },
            Template::Block {
                origin: pt([500.0, 0.0, 0.0]),
                size: [1.0, 1.0, 1.0],
            },
            Template::Sphere {
                center: pt([0.0, 0.0, 0.0]),
                radius: 1.0,
            },
        ] {
            assert!(!certify(&k, &mut b, &t).unwrap(), "{t:?}");
        }
        let mut b = p::create_cylinder(&mut k, pt([0.0, 0.0, 0.0]), 1.0, 1.0).unwrap();
        for t in [
            Template::Cylinder {
                origin: pt([0.0, 0.0, 0.0]),
                radius: 300.0,
                height: 1.0,
            },
            Template::Cylinder {
                origin: pt([0.0, 0.0, 50.0]),
                radius: 1.0,
                height: 1.0,
            },
        ] {
            assert!(!certify(&k, &mut b, &t).unwrap(), "{t:?}");
        }
    }

    /// A mutation: one control point of the cylinder side's net moved far from its recipe value
    /// (by ten radii) makes that face's term exceed V(T), so the certificate refuses; unmoved,
    /// the same face's term is far below V(T).
    #[test]
    fn a_side_net_moved_from_its_recipe_is_refused() {
        let mut k = Kernel::default();
        let (o, r, h) = (pt([1.0, 2.0, 3.0]), 3.0, 10.0);
        let mut b = p::create_cylinder(&mut k, o, r, h).unwrap();
        let model = model(&Template::Cylinder {
            origin: o,
            radius: r,
            height: h,
        })
        .unwrap();
        let ModelFace::Patch { net, kind } = model.faces.first().unwrap() else {
            panic!("the side comes first");
        };
        let side = super::super::p1::faces(&b, None)
            .unwrap()
            .into_iter()
            .find(|&f| {
                let s = b.face(f).unwrap().surface.clone().unwrap();
                Nurbs::of(&as_b_spline(&k, &s).unwrap().unwrap()).unwrap().p == 2
            })
            .unwrap();
        let (sign, up) = face_sign(&b, side).unwrap().unwrap();
        let s = b.face(side).unwrap().surface.clone().unwrap();
        let cn = Nurbs::of(&as_b_spline(&k, &s).unwrap().unwrap()).unwrap();
        let walk = the_loop(&k, &mut b, up).unwrap().unwrap();
        let bnet = quads(&cn.pw);
        let (fc, _, _) = patch_face(&cn, &bnet, &walk, sign, net, *kind, model.z0).unwrap();
        assert!(
            fc.term.hi < 1e-6 * model.volume.lo,
            "unmoved: {:?}",
            fc.term
        );
        let mut moved = bnet.clone();
        let w = moved[2][3];
        moved[2][0] = moved[2][0] + Ivl::from(10.0 * r) * w;
        let (fc, _, _) = patch_face(&cn, &moved, &walk, sign, net, *kind, model.z0).unwrap();
        assert!(
            fc.term.hi > model.volume.hi,
            "moved: {:?} against {:?}",
            fc.term,
            model.volume
        );
    }

    /// Inputs out of the constructors' domain give no template, so no certificate.
    #[test]
    fn no_template_out_of_domain() {
        let mut k = Kernel::default();
        let mut b = p::create_box(&mut k, pt([0.0, 0.0, 0.0]), 1.0, 1.0, 1.0).unwrap();
        for t in [
            Template::Block {
                origin: pt([0.0, 0.0, 0.0]),
                size: [0.0, 1.0, 1.0],
            },
            Template::Block {
                origin: pt([0.0, 0.0, 0.0]),
                size: [f64::NAN, 1.0, 1.0],
            },
            Template::Torus {
                center: pt([0.0, 0.0, 0.0]),
                major: 1.0,
                minor: 1.0,
            },
            Template::Cylinder {
                origin: pt([0.0, 0.0, 0.0]),
                radius: -1.0,
                height: 1.0,
            },
        ] {
            assert!(!certify(&k, &mut b, &t).unwrap(), "{t:?}");
        }
    }

    /// The behaviour change on tiny primitives. The certificate either certifies or refuses
    /// (never an error), a certified body is never Negative under the full proof, and the 1e-3
    /// box and cylinder are certified. Below the constructors' size resolution they refuse:
    /// `InvalidInput` for the box at 1e-12, and at 1e-6 and 1e-9 the typed failure of the
    /// face that cannot be made (WR-596; main built no face there and returned the empty
    /// body). The size that builds is 1e-3: a closed six-face box that passes the level-2
    /// check. What the full proof says at each size is printed for the change list.
    #[test]
    fn tiny_primitives_and_the_full_proof() {
        fn sign(k: &Kernel, b: &mut Brep) -> &'static str {
            match crate::mass::enclosure::volume_sign(k, b) {
                Ok(crate::mass::enclosure::VolumeSign::Positive(_)) => "Positive",
                Ok(crate::mass::enclosure::VolumeSign::Negative(_)) => "Negative",
                Ok(crate::mass::enclosure::VolumeSign::Undecided(_)) => "Undecided",
                Err(_) => "Err",
            }
        }
        for s in [1e-3, 1e-6, 1e-9, 1e-12] {
            let o = pt([0.0, 0.0, 0.0]);
            let mut k = Kernel::default();
            match p::create_box(&mut k, o, s, s, s) {
                Ok(mut b) => {
                    let t = Template::Block {
                        origin: o,
                        size: [s, s, s],
                    };
                    let cert = is_outward_primitive(&mut k, &mut b, &t).unwrap();
                    let full = sign(&k, &mut b);
                    println!("TINY box {s:e}: certificate {cert} full proof {full}");
                    assert!(!(cert && full == "Negative"), "box {s:e}");
                    assert!(cert || s < 1e-3, "box {s:e} not certified");
                    // the size the refusals below name builds: six faces, level-2 clean
                    assert_eq!(crate::topology::brep::p1::faces(&b, None).unwrap().len(), 6);
                    let lines = crate::topology::brep::validation::check_brep(
                        &mut k,
                        &mut b,
                        crate::topology::brep::validation::CheckLevel::Level2,
                    )
                    .unwrap()
                    .lines();
                    assert!(lines.is_empty(), "box {s:e}: {lines:?}");
                }
                Err(e) => {
                    println!("TINY box {s:e}: constructor refuses: {e:?}");
                    assert!(s < 1e-3, "box {s:e}: {e:?}");
                    if s < 1e-9 {
                        assert!(
                            matches!(e, crate::SolidError::InvalidInput { .. }),
                            "box {s:e}: {e:?}"
                        );
                    } else {
                        // the face construction's failure is kept as the cause, and gives
                        // the class
                        let cause = e.cause().expect("the face construction's failure");
                        assert_eq!(
                            e.topology_failure(),
                            Some(&crate::error::topology::TopologyFailure::FaceNotMade {
                                class: cause.kind()
                            }),
                            "box {s:e}"
                        );
                        assert_eq!(e.operation(), "topology::brep::p8", "box {s:e}");
                        assert_eq!(
                            e.failed_routine_status(),
                            Some(crate::error::SM_ERR),
                            "box {s:e}"
                        );
                        assert_eq!(e.kind(), cause.kind(), "box {s:e}");
                    }
                }
            }
            let mut k = Kernel::default();
            match p::create_cylinder(&mut k, o, s, s) {
                Ok(mut b) => {
                    let t = Template::Cylinder {
                        origin: o,
                        radius: s,
                        height: s,
                    };
                    let cert = is_outward_primitive(&mut k, &mut b, &t).unwrap();
                    let full = sign(&k, &mut b);
                    println!("TINY cylinder {s:e}: certificate {cert} full proof {full}");
                    assert!(!(cert && full == "Negative"), "cylinder {s:e}");
                    assert!(cert || s < 1e-3, "cylinder {s:e} not certified");
                }
                Err(e) => {
                    println!("TINY cylinder {s:e}: constructor refuses: {e:?}");
                }
            }
        }
    }
}
