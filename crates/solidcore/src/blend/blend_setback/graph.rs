//! The G1 fill of a corner as a graph over the corner's axis (the fork's `sm_SbGraphG1`,
//! `sm_SbFillFrom`; tangency-labs/smlib 590a7f3, 08d4b02, f1d81f4; smlib#104).
//!
//! The fill keeps the setback split's layout and its G1 by construction (the rounds' rows,
//! Hahn's connecting map, the faces' columns), but every patch is a graph over the plane
//! square to the corner's axis e (into the material at a convex corner, into the void at a
//! concave one): the interior map is `F = xi_1 a_1 + xi_2 a_2 + h(xi) e` over the projection
//! of the base points, and each setback patch's projection along e is fixed by its round's
//! rows, its speed off the round and F's projection; only heights along e are free. Each
//! patch's `l`, `m`, `n` are then affine in the heights, and the side its normal faces, `o`,
//! depends on the projection only: certified of one sign, or the fill folds and is refused.
//! The body stays convex where `s [[l, m], [m, n]]` is positive semidefinite on every patch;
//! linear sufficient conditions on the Bernstein coefficient matrices of the patches' pieces
//! (sector conditions over directions on a half turn) hold the heights to it, by the dual
//! active-set method ([`super::qp`]), at degree 9 first (proven infeasible there, at every
//! lower degree: the search stops), then up from the least degree, each result certified
//! ([`super::certify::certified_convex`]) before it is taken.
//!
//! Deviations: the fork's `Probe *` is `Option<&mut Probe>`; its `Fill::m_lSolves` and
//! `m_bUncertified` are [`super::FillStats`]. A probe that asks neither the fill nor its
//! margin answers feasibility with an empty fill, as the fork's leaves `rFill` empty.

use super::certify::{
    affine_fundamentals, affine_net, certified_convex, net_surface, t_make, t_nonneg, t_pieces,
    t_scale, AffinePatch,
};
use super::qp::{dual_qp_rows, QpOutcome};
use super::{
    affine_solutions, along_line, axpy, bern_elevate, bern_mul, binomial, el, el_mut, el_ref,
    monomials, over_patch, second, sm_err, smax, tensor_elevate, Face, Fill, FillStats, Hom, Net,
    Probe, Refusal, Round, Tensor, GW, GX, HOM_ZERO,
};
use crate::error::{Result, SolidError};
use crate::geom::vector3d::sqrt;
use crate::math::{Point3, Vec3, PI};

/// `SM_SB_GRAPH_DEPTH`: the certificate's halvings.
pub(super) const GRAPH_DEPTH: usize = 6;
/// `SM_SB_GRAPH_LEVELS`: the sector conditions' halvings.
const GRAPH_LEVELS: usize = 1;
/// `SM_SB_GRAPH_DIRECTIONS`: their directions over a half turn.
const GRAPH_DIRECTIONS: usize = 16;
/// The cubic across: rows 0, 1 the round's, 2, 3 F's.
const NC: usize = 3;

/// A 2D point or vector (`SmPoint2d`, `SmVector2d`).
#[derive(Debug, Clone, Copy, Default)]
struct P2 {
    x: f64,
    y: f64,
}

/// `sm_SbFillFrom`: the fill's surfaces from its nets.
fn fill_from(
    nets: &[Net],
    round: &[usize],
    face: &[usize],
    rev: &[bool],
    degree: usize,
) -> Result<Fill> {
    let mut patches = Vec::with_capacity(nets.len());
    for net in nets {
        patches.push(net_surface(net)?);
    }
    Ok(Fill {
        patches,
        round: round.to_vec(),
        face: face.to_vec(),
        reversed: rev.to_vec(),
        degree,
    })
}

/// The solution of the 3 x 3 system `A x = b` by Cramer's rule, `None` where
/// `|det A| <= 1e-12` (the fork's corner vertex: the faces' planes' common point, least
/// squares; `sm_SbGraphG1` and `sm_SbModelRounds` alike).
pub(super) fn faces_common_point(faces: &[Face]) -> Option<Vec3> {
    let mut a = [[0.0; 3]; 3];
    let mut b = [0.0; 3];
    for face in faces {
        let nrm = face.normal;
        let c = [nrm.x, nrm.y, nrm.z];
        let d = nrm.dot(face.point);
        for ((ai, bi), &ci) in a.iter_mut().zip(b.iter_mut()).zip(&c) {
            for (aij, &cj) in ai.iter_mut().zip(&c) {
                *aij += ci * cj;
            }
            *bi += ci * d;
        }
    }
    let det3 = |m: &[[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let det = det3(&a);
    if !(det.abs() > 1.0e-12) {
        return None;
    }
    let mut x = [0.0; 3];
    for (k, xk) in x.iter_mut().enumerate() {
        let mut m = [[0.0; 3]; 3];
        for ((mi, ai), &bi) in m.iter_mut().zip(&a).zip(&b) {
            for (j, (mij, &aij)) in mi.iter_mut().zip(ai).enumerate() {
                *mij = if j == k { bi } else { aij };
            }
        }
        *xk = det3(&m) / det;
    }
    Some(Vec3::new(x[0], x[1], x[2]))
}

/// A face point snapped onto its plane: `h` moved along the normal by its offset, and a
/// coordinate set exactly where the normal is an axis.
fn snap(h: &mut Hom, face: &Face) {
    let nrm = face.normal;
    let off = nrm.x * h[0] + nrm.y * h[1] + nrm.z * h[2] - nrm.dot(face.point) * h[3];
    h[0] -= off * nrm.x;
    h[1] -= off * nrm.y;
    h[2] -= off * nrm.z;
    snap_axes(h, face);
}

/// The coordinates of `h` set exactly to the face's point where its normal is an axis.
fn snap_axes(h: &mut Hom, face: &Face) {
    let nrm = face.normal;
    let w = h[3];
    for (x, hx) in h.iter_mut().take(3).enumerate() {
        if nrm.component(x).abs() == 1.0 {
            *hx = face.point.component(x) * w;
        }
    }
}

/// The offset of `h` from the face's plane (homogeneous).
fn offset(h: &Hom, face: &Face) -> f64 {
    let nrm = face.normal;
    nrm.x * h[0] + nrm.y * h[1] + nrm.z * h[2] - nrm.dot(face.point) * h[3]
}

/// What the corner's G1 fill is built from (`sm_SbGraphG1`'s arguments).
pub(super) struct GraphInput<'a, 'r> {
    /// `crWorldFaces`.
    pub faces: &'a [Face],
    /// `crRounds`.
    pub rounds: &'a [Round<'r>],
    /// `crWorldJets`.
    pub jets: &'a [Vec<Vec<Hom>>],
    /// `crQ`.
    pub qs: &'a [usize],
    /// `crRound`.
    pub round: &'a [usize],
    /// `crFace`.
    pub face: &'a [usize],
    /// `crRev`.
    pub rev: &'a [bool],
    /// `dTol`.
    pub tol: f64,
}

/// `sm_SbGraphG1`: the G1 fill (module docs).
#[allow(clippy::too_many_lines)] // the fork's one function
pub(super) fn graph_g1(
    input: &GraphInput<'_, '_>,
    stats: &mut FillStats,
    mut probe: Option<&mut Probe>,
) -> core::result::Result<Fill, Refusal> {
    let GraphInput {
        faces: world_faces,
        rounds,
        jets: world_jets,
        qs,
        round: cycle_round,
        face: cycle_face,
        rev: cycle_rev,
        tol,
    } = *input;
    let n = cycle_round.len();
    let face_at = |m: usize| -> Result<usize> { el(cycle_face, m % n) };
    // the fill is built and certified about the corner's vertex O, the faces' common point
    // (least squares): its rounding that of the corner's size, not of its place, and a face
    // through O an axis plane exactly where it is one in the world
    let o = faces_common_point(world_faces).ok_or_else(|| {
        Refusal::invalid("SmBlendSetback: the corner's faces meet in no one point")
    })?;
    let mut jets: Vec<Vec<Vec<Hom>>> = world_jets.to_vec();
    for jr in &mut jets {
        for jk in jr.iter_mut() {
            for h in jk.iter_mut() {
                h[0] -= h[3] * o.x;
                h[1] -= h[3] * o.y;
                h[2] -= h[3] * o.z;
            }
        }
    }
    let mut faces: Vec<Face> = world_faces.to_vec();
    for f in &mut faces {
        f.point -= o;
    }
    // the nets moved back, the faces' points snapped onto their planes there
    let to_world = |nets: &mut Vec<Net>| -> Result<()> {
        for net in nets.iter_mut() {
            for h in &mut net.pts {
                h[0] += h[3] * o.x;
                h[1] += h[3] * o.y;
                h[2] += h[3] * o.z;
            }
        }
        for m in 0..n {
            let net = el_mut(nets, m)?;
            let qv = net.q;
            for i in 2..=NC {
                for c in 0..2 {
                    for e in 0..2 {
                        let col = if e == 0 { c } else { qv - c };
                        let face = el_ref(world_faces, face_at(m + e)?)?;
                        snap(el_mut(&mut net.pts, i * (qv + 1) + col)?, face);
                    }
                }
            }
        }
        Ok(())
    };

    // each cycle patch's round rows (columns from face m to face m + 1), the rails' ends and
    // their directions toward the corner
    let mut h0s: Vec<Vec<Hom>> = vec![Vec::new(); n];
    let mut h1s: Vec<Vec<Hom>> = vec![Vec::new(); n];
    let mut end0 = vec![Point3::ZERO; n];
    let mut end1 = vec![Point3::ZERO; n];
    let mut mids = vec![Point3::ZERO; n];
    let mut dir0 = vec![Vec3::ZERO; n];
    let mut dir1 = vec![Vec3::ZERO; n];
    let mut speed0 = vec![0.0; n];
    let mut speed1 = vec![0.0; n];
    for m in 0..n {
        let r = el(cycle_round, m)?;
        let q = el(qs, r)?;
        let sign = if el_ref(rounds, r)?.corner_above {
            1.0
        } else {
            -1.0
        };
        let jr = el_ref(&jets, r)?;
        let (j0, j1) = (el_ref(jr, 0)?, el_ref(jr, 1)?);
        let rev = el(cycle_rev, m)?;
        let mut h0 = vec![HOM_ZERO; q + 1];
        let mut h1 = vec![HOM_ZERO; q + 1];
        for c in 0..=q {
            let cc = if rev { q - c } else { c };
            *el_mut(&mut h0, c)? = el(j0, cc)?;
            *el_mut(&mut h1, c)? = axpy(&HOM_ZERO, sign, el_ref(j1, cc)?);
        }
        // the round's end columns exactly on their faces (within tol): its rail end on the
        // face (row 0) and its derivative along it (row 1), as the fill's own face columns
        // are
        for e in 0..2 {
            let face = el_ref(&faces, face_at(m + e)?)?;
            let col = if e == 0 { 0 } else { q };
            for k in 0..2 {
                let w0 = el_ref(&h0, col)?[3];
                let h = if k == 0 {
                    el_mut(&mut h0, col)?
                } else {
                    el_mut(&mut h1, col)?
                };
                let off = offset(h, face);
                if off.abs() > tol * smax(1.0, w0.abs()) {
                    return Err(Refusal::invalid(
                        "SmBlendSetback: a round does not meet its faces at its end",
                    ));
                }
                snap(h, face);
            }
        }
        for e in 0..2 {
            let col = if e == 0 { 0 } else { q };
            let a = el(&h0, col)?;
            let b = el(&h1, col)?;
            let p = Point3::new(a[0] / a[3], a[1] / a[3], a[2] / a[3]);
            let t = Vec3::new(
                (b[0] - p.x * b[3]) / a[3],
                (b[1] - p.y * b[3]) / a[3],
                (b[2] - p.z * b[3]) / a[3],
            );
            *el_mut(if e == 0 { &mut speed0 } else { &mut speed1 }, m)? = t.length();
            let t = t.unitize()?;
            *el_mut(if e == 0 { &mut end0 } else { &mut end1 }, m)? = p;
            *el_mut(if e == 0 { &mut dir0 } else { &mut dir1 }, m)? = t;
        }
        // the section's middle
        let mut mh = HOM_ZERO;
        for (c, hc) in h0.iter().enumerate() {
            mh = axpy(&mh, binomial(q, c)? / 2.0_f64.powf(q as f64), hc);
        }
        *el_mut(&mut mids, m)? = Point3::new(mh[0] / mh[3], mh[1] / mh[3], mh[2] / mh[3]);
        *el_mut(&mut h0s, m)? = h0;
        *el_mut(&mut h1s, m)? = h1;
    }
    // convex (the rounds take material away) or concave (they add it), and the axis e into
    // the convex side
    let mut sum = Vec3::ZERO;
    let mut convex = 0;
    for m in 0..n {
        let face = el_ref(&faces, el(cycle_face, m)?)?;
        sum += face.normal;
        let side = if (el(&mids, m)? - face.point).dot(face.normal) < 0.0 {
            1
        } else {
            -1
        };
        if convex != 0 && side != convex {
            return Err(Refusal::invalid(
                "SmBlendSetback: a convex G1 fill needs rounds all convex or all concave",
            ));
        }
        convex = side;
    }
    let e = (sum * if convex > 0 { -1.0 } else { 1.0 }).unitize()?;
    let mut a1 = Vec3::new(1.0, 0.0, 0.0);
    if e.y.abs() < e.x.abs() && e.y.abs() <= e.z.abs() {
        a1 = Vec3::new(0.0, 1.0, 0.0);
    } else if e.z.abs() < e.x.abs() && e.z.abs() < e.y.abs() {
        a1 = Vec3::new(0.0, 0.0, 1.0);
    }
    a1 = a1 - e * a1.dot(e);
    let a1 = a1.unitize()?;
    let a2 = e.cross(a1);
    let in_plane = |v: Vec3| P2 {
        x: v.dot(a1),
        y: v.dot(a2),
    };

    // the base points, the domain (their projections), the cross fields and the speeds
    let mut xi = vec![P2::default(); n];
    let mut sigma = vec![P2::default(); n];
    let mut to_x0 = vec![0.0; n];
    let mut to_x1 = vec![0.0; n];
    for m in 0..n {
        let mp = (m + n - 1) % n;
        let pa = el(&end0, m)?; // patch m's rail on face m
        let pb = el(&end1, mp)?; // patch m - 1's rail on face m
        let (ta, tb) = (el(&dir0, m)?, el(&dir1, mp)?);
        // the rails' crossing: Pa + s Ta = Pb + t Tb, least squares
        let a11 = ta.dot(ta);
        let a12 = -ta.dot(tb);
        let a22 = tb.dot(tb);
        let d = pb - pa;
        let b1 = ta.dot(d);
        let b2 = -tb.dot(d);
        let det = a11 * a22 - a12 * a12;
        if !(det.abs() > 1.0e-12) {
            return Err(Refusal::invalid(
                "SmBlendSetback: two rails on a face run parallel (no corner region between them)",
            ));
        }
        let s = (b1 * a22 - a12 * b2) / det;
        let t = (a11 * b2 - a12 * b1) / det;
        let x = (pa + ta * s + pb + tb * t) * 0.5;
        let mid_c = (pa + pb) * 0.5;
        if !(s > 0.0) || !(t > 0.0) {
            return Err(Refusal::invalid(
                "SmBlendSetback: two rounds' rails on a face cross behind their ends (no corner region between them)",
            ));
        }
        // the spring curve on face m: the parabola (Pa, X, Pb), as two cubic halves meeting
        // at its middle B = (Pa + 2 X + Pb) / 4, its tangent there (Pa - Pb) / 2 (sigma_m),
        // each half's second control point on its rail a third of the way to X (lambda)
        let base = mid_c + (x - mid_c) * 0.5;
        *el_mut(&mut xi, m)? = in_plane(base);
        let sg = in_plane(pa - pb);
        *el_mut(&mut sigma, m)? = P2 {
            x: sg.x * 0.5,
            y: sg.y * 0.5,
        };
        *el_mut(&mut to_x0, m)? = s / el(&speed0, m)?;
        *el_mut(&mut to_x1, mp)? = t / el(&speed1, mp)?;
    }
    let nf = n as f64;
    let mut centre = P2::default();
    let mut rho = 0.0;
    for p in &xi {
        centre = P2 {
            x: centre.x + p.x / nf,
            y: centre.y + p.y / nf,
        };
    }
    for p in &xi {
        let (dx, dy) = (p.x - centre.x, p.y - centre.y);
        rho = smax(rho, sqrt((dx * dx) + (dy * dy)));
    }
    if !(rho > 0.0) {
        return Err(sm_err().into());
    }
    // the domain scaled: x = (xi - centre) / rho
    let bs: Vec<P2> = xi
        .iter()
        .map(|p| P2 {
            x: (p.x - centre.x) / rho,
            y: (p.y - centre.y) / rho,
        })
        .collect();

    // The degrees: 9 first, then up from the least. Proven unmet at 9 (the QP's
    // certificate), the sector conditions are unmet at every degree, and the search stops.
    // Else the least degree whose fill is certified, or 9's.
    let mut top: Vec<Net> = Vec::new();
    let mut top_degree = 0;
    let probe_only = probe.as_ref().is_some_and(|p| !p.fill);
    let probe_fill = probe.as_ref().is_some_and(|p| p.fill);
    let mut degrees: Vec<usize> = Vec::new();
    if probe.is_none() || probe_only {
        degrees.push(9);
    }
    if !probe_only {
        for d in NC..=9 {
            if d < 9 || probe_fill {
                degrees.push(d);
            }
        }
    }
    'degrees: for &d in &degrees {
        let mono = monomials(d);
        let big_m = mono.len();
        // the unknowns: h's coefficients
        let unknowns = big_m;
        let mut qvs = vec![0; n];
        for (m, qv) in qvs.iter_mut().enumerate() {
            let q1 = el(qs, el(cycle_round, m)?)? + 1;
            *qv = if q1 > d { q1 } else { d };
        }
        let mut patches: Vec<AffinePatch> = vec![AffinePatch::default(); 2 * n];
        for m in 0..n {
            let qv = el(&qvs, m)?;
            // the cubic across: rows 0, 1 the round's, at the speed lambda(v) off it (linear
            // across, from each rail's distance to its rails' crossing), 2, 3 F's
            let mut rows = vec![vec![HOM_ZERO; qv + 1]; NC + 1];
            let lam = [el(&to_x0, m)? / NC as f64, el(&to_x1, m)? / NC as f64];
            let (h0, h1) = (el_ref(&h0s, m)?, el_ref(&h1s, m)?);
            for c in 0..4 {
                let mut s0 = vec![0.0; h0.len()];
                let mut s1 = vec![0.0; h0.len()];
                for j in 0..s0.len() {
                    *el_mut(&mut s0, j)? = el(el_ref(h0, j)?, c)?;
                    *el_mut(&mut s1, j)? = el(el_ref(h1, j)?, c)?;
                }
                let s1 = bern_mul(&lam, &s1)?;
                let s0 = bern_elevate(&s0, qv)?;
                let s1 = bern_elevate(&s1, qv)?;
                for j in 0..=qv {
                    *el_mut(el_mut(el_mut(&mut rows, 0)?, j)?, c)? = el(&s0, j)?;
                    *el_mut(el_mut(el_mut(&mut rows, 1)?, j)?, c)? = el(&s0, j)? + el(&s1, j)?;
                }
            }
            // F's projection along edge m and its derivative along the field d (linear in v)
            let x0 = el(&xi, m)?;
            let x1 = el(&xi, (m + 1) % n)?;
            let d0 = el(&sigma, m)?;
            let s1n = el(&sigma, (m + 1) % n)?;
            // `sSigma[(m + 1) % n] * -1.0`: negation, bit for bit the product
            let d1 = P2 {
                x: -s1n.x,
                y: -s1n.y,
            };
            for c in 0..3 {
                let da1 = a1.component(c);
                let da2 = a2.component(c);
                let sp = [x0.x * da1 + x0.y * da2, x1.x * da1 + x1.y * da2];
                let sdp = [d0.x * da1 + d0.y * da2, d1.x * da1 + d1.y * da2];
                let s3 = bern_elevate(&sp, qv)?;
                let sde = bern_elevate(&sdp, qv)?;
                for j in 0..=qv {
                    *el_mut(el_mut(el_mut(&mut rows, 3)?, j)?, c)? = el(&s3, j)?;
                    *el_mut(el_mut(el_mut(&mut rows, 2)?, j)?, c)? =
                        el(&s3, j)? + el(&sde, j)? / NC as f64;
                }
            }
            for j in 0..=qv {
                el_mut(el_mut(&mut rows, 2)?, j)?[3] = 1.0;
                el_mut(el_mut(&mut rows, 3)?, j)?[3] = 1.0;
            }
            // h's monomials along the edge (scaled coordinates) and along the field (d / rho)
            let (bm, bn) = (el(&bs, m)?, el(&bs, (m + 1) % n)?);
            let sx = [bm.x, bn.x];
            let sy = [bm.y, bn.y];
            let sdx = [d0.x / rho, d1.x / rho];
            let sdy = [d0.y / rho, d1.y / rho];
            let mut z2: Vec<Vec<f64>> = vec![Vec::new(); big_m];
            let mut z3: Vec<Vec<f64>> = vec![Vec::new(); big_m];
            for (j, &(a, b)) in mono.iter().enumerate() {
                let mut s1 = vec![0.0; d + 1];
                if a >= 1 {
                    let t =
                        bern_elevate(&bern_mul(&along_line(a - 1, b, &sx, &sy, d - 1)?, &sdx)?, d)?;
                    for i in 0..=d {
                        *el_mut(&mut s1, i)? += a as f64 * el(&t, i)?;
                    }
                }
                if b >= 1 {
                    let t =
                        bern_elevate(&bern_mul(&along_line(a, b - 1, &sx, &sy, d - 1)?, &sdy)?, d)?;
                    for i in 0..=d {
                        *el_mut(&mut s1, i)? += b as f64 * el(&t, i)?;
                    }
                }
                let v0 = bern_elevate(&along_line(a, b, &sx, &sy, d)?, qv)?;
                let v1 = bern_elevate(&s1, qv)?;
                let mut z2j = vec![0.0; qv + 1];
                for i in 0..=qv {
                    *el_mut(&mut z2j, i)? = el(&v0, i)? + el(&v1, i)? / NC as f64;
                }
                *el_mut(&mut z3, j)? = v0;
                *el_mut(&mut z2, j)? = z2j;
            }
            // the net
            let patch = el_mut(&mut patches, m)?;
            patch.base.p = NC;
            patch.base.q = qv;
            patch.base.pts = vec![HOM_ZERO; (NC + 1) * (qv + 1)];
            for j in 0..=qv {
                for i in 0..=NC {
                    *el_mut(&mut patch.base.pts, i * (qv + 1) + j)? = el(el_ref(&rows, i)?, j)?;
                }
            }
            for jm in 0..big_m {
                let mut z = t_make(NC, qv);
                for j in 0..=qv {
                    *el_mut(&mut z.c, 2 * (qv + 1) + j)? = el(el_ref(&z2, jm)?, j)?;
                    *el_mut(&mut z.c, 3 * (qv + 1) + j)? = el(el_ref(&z3, jm)?, j)?;
                }
                patch.index.push(jm);
                patch.z.push(z);
            }
        }
        for q in 0..n {
            // interior part q: corners (0, 0) the centre, (0, 1) the middle of edge q - 1,
            // (1, 0) the middle of edge q, (1, 1) B_q
            let bq = el(&bs, q)?;
            let bnext = el(&bs, (q + 1) % n)?;
            let bprev = el(&bs, (q + n - 1) % n)?;
            let mq = P2 {
                x: (bq.x + bnext.x) * 0.5,
                y: (bq.y + bnext.y) * 0.5,
            };
            let mp = P2 {
                x: (bq.x + bprev.x) * 0.5,
                y: (bq.y + bprev.y) * 0.5,
            };
            let sx = Tensor {
                p: 1,
                q: 1,
                c: vec![0.0, mp.x, mq.x, bq.x],
            };
            let sy = Tensor {
                p: 1,
                q: 1,
                c: vec![0.0, mp.y, mq.y, bq.y],
            };
            let patch = el_mut(&mut patches, n + q)?;
            patch.base.p = d;
            patch.base.q = d;
            patch.base.pts = vec![HOM_ZERO; (d + 1) * (d + 1)];
            let sxd = tensor_elevate(&sx, d, d)?;
            let syd = tensor_elevate(&sy, d, d)?;
            for (i, pt) in patch.base.pts.iter_mut().enumerate() {
                let xi1 = centre.x + rho * el(&sxd.c, i)?;
                let xi2 = centre.y + rho * el(&syd.c, i)?;
                *pt = [
                    xi1 * a1.x + xi2 * a2.x,
                    xi1 * a1.y + xi2 * a2.y,
                    xi1 * a1.z + xi2 * a2.z,
                    1.0,
                ];
            }
            for (j, &(a, b)) in mono.iter().enumerate() {
                patch.index.push(j);
                patch.z.push(over_patch(a, b, &sx, &sy, d)?);
            }
        }
        // the equalities: each setback patch's rows 2, 3, its first two columns on face m's
        // plane and its last two on face m + 1's
        let mut aeq: Vec<f64> = Vec::new();
        let mut beq: Vec<f64> = Vec::new();
        for m in 0..n {
            let patch = el_ref(&patches, m)?;
            let qv = patch.base.q;
            for i in 2..=NC {
                for c in 0..2 {
                    for ee in 0..2 {
                        let col = if ee == 0 { c } else { qv - c };
                        let idx = i * (qv + 1) + col;
                        let face = el_ref(&faces, face_at(m + ee)?)?;
                        let h = el(&patch.base.pts, idx)?;
                        let nrm = face.normal;
                        let mut row = vec![0.0; unknowns];
                        for (k, &ix) in patch.index.iter().enumerate() {
                            *el_mut(&mut row, ix)? +=
                                el(&el_ref(&patch.z, k)?.c, idx)? * nrm.dot(e);
                        }
                        aeq.extend_from_slice(&row);
                        beq.push(-offset(&h, face));
                    }
                }
            }
        }
        let mut size = 0.0;
        for &x in &beq {
            size = smax(size, x.abs());
        }
        let Some(affine) =
            affine_solutions(aeq, beq.clone(), beq.len(), unknowns, 1.0e-9 * (1.0 + size))?
        else {
            continue;
        };
        if affine.z.is_empty() {
            continue;
        }
        let (yp, zn) = (affine.x0, affine.z);
        let free = zn.len();

        // the energy, affine in y: its rows at y = 0 (e0) and their change per unknown (ae,
        // le x unknowns)
        let energy_rows = |net: &Net| -> Result<Vec<f64>> {
            let mut rows = Vec::new();
            for (&g1x, &g1w) in GX.iter().zip(&GW) {
                for (&g2x, &g2w) in GX.iter().zip(&GW) {
                    let r = second(net, g1x, g2x)?;
                    let w = sqrt(g1w * g2w);
                    for c in 0..3 {
                        rows.push(w * r[0].component(c));
                        rows.push(w * sqrt(2.0) * r[1].component(c));
                        rows.push(w * r[2].component(c));
                    }
                }
            }
            Ok(rows)
        };
        let mut e0: Vec<f64> = Vec::new();
        let mut e_start: Vec<usize> = Vec::new();
        let mut patch_e0: Vec<Vec<f64>> = Vec::with_capacity(2 * n);
        for patch in &patches {
            let rows = energy_rows(&patch.base)?;
            e_start.push(e0.len());
            e0.extend_from_slice(&rows);
            patch_e0.push(rows);
        }
        let le = e0.len();
        let mut ae = vec![0.0; le * unknowns];
        {
            let mut unit = vec![0.0; unknowns];
            for (p, patch) in patches.iter().enumerate() {
                for &u in &patch.index {
                    *el_mut(&mut unit, u)? = 1.0;
                    let rows = energy_rows(&affine_net(patch, &unit, e)?)?;
                    *el_mut(&mut unit, u)? = 0.0;
                    let start = el(&e_start, p)?;
                    let base = el_ref(&patch_e0, p)?;
                    for (i, &ri) in rows.iter().enumerate() {
                        *el_mut(&mut ae, (start + i) * unknowns + u)? += ri - el(base, i)?;
                    }
                }
            }
        }
        // over y = yp + Zn t: B = AE Zn, c = E0 + AE yp; the energy |B t + c|^2
        let mut bm = vec![0.0; le * free];
        let mut cv = vec![0.0; le];
        for i in 0..le {
            let mut s = el(&e0, i)?;
            for u in 0..unknowns {
                s += el(&ae, i * unknowns + u)? * el(&yp, u)?;
            }
            *el_mut(&mut cv, i)? = s;
            for f in 0..free {
                let mut t = 0.0;
                let znf = el_ref(&zn, f)?;
                for u in 0..unknowns {
                    t += el(&ae, i * unknowns + u)? * el(znf, u)?;
                }
                *el_mut(&mut bm, i * free + f)? = t;
            }
        }
        // the nets at t, their face points snapped exactly onto their planes
        let nets_at = |t: &[f64]| -> Result<Option<Vec<Net>>> {
            let mut y = yp.clone();
            for f in 0..free {
                let znf = el_ref(&zn, f)?;
                for (u, yu) in y.iter_mut().enumerate() {
                    *yu += el(znf, u)? * el(t, f)?;
                }
            }
            let mut nets = Vec::with_capacity(2 * n);
            for patch in &patches {
                nets.push(affine_net(patch, &y, e)?);
            }
            for m in 0..n {
                let net = el_mut(&mut nets, m)?;
                let qv = net.q;
                for i in 2..=NC {
                    for c in 0..2 {
                        for ee in 0..2 {
                            let col = if ee == 0 { c } else { qv - c };
                            let face = el_ref(&faces, face_at(m + ee)?)?;
                            let h = el_mut(&mut net.pts, i * (qv + 1) + col)?;
                            let off = offset(h, face);
                            if off.abs() > tol * h[3] {
                                return Ok(None);
                            }
                            snap(h, face);
                        }
                    }
                }
            }
            Ok(Some(nets))
        };

        // each patch's curvature determinants, affine in y, and the side its normal faces
        // (o of one sign, certified, or the fill folds)
        let mut q0s: Vec<[Tensor; 3]> = Vec::with_capacity(2 * n);
        let mut qjs: Vec<[Vec<Tensor>; 3]> = Vec::with_capacity(2 * n);
        let mut signs = vec![0.0; 2 * n];
        for (p, patch) in patches.iter().enumerate() {
            let af = affine_fundamentals(patch, e)?;
            let mut neg_o = af.o.clone();
            for x in &mut neg_o.c {
                *x = -*x;
            }
            let d_o = 1.0e-12 * t_scale(&af.o);
            let s = if t_nonneg(&af.o, d_o, GRAPH_DEPTH)? == 1 {
                1.0
            } else if t_nonneg(&neg_o, d_o, GRAPH_DEPTH)? == 1 {
                -1.0
            } else {
                0.0
            };
            *el_mut(&mut signs, p)? = s;
            if s == 0.0 {
                return Err(Refusal::sm_err(
                    "SmBlendSetback: the fill folds (a patch's normal turns across the corner's axis)",
                ));
            }
            q0s.push(af.q0);
            qjs.push(af.qj);
        }
        // the energy as a quadratic programme: G = 2 B'B (a least nudge where it leaves a
        // direction free: the energy only chooses), g = 2 B'c
        let mut gm = vec![0.0; free * free];
        let mut gv = vec![0.0; free];
        let mut trace = 0.0;
        for a in 0..free {
            for b in a..free {
                let mut s = 0.0;
                for i in 0..le {
                    s += el(&bm, i * free + a)? * el(&bm, i * free + b)?;
                }
                *el_mut(&mut gm, b * free + a)? = 2.0 * s;
                *el_mut(&mut gm, a * free + b)? = 2.0 * s;
            }
            let mut s = 0.0;
            for i in 0..le {
                s += el(&bm, i * free + a)? * el(&cv, i)?;
            }
            *el_mut(&mut gv, a)? = 2.0 * s;
            trace += el(&gm, a * free + a)?;
        }
        for a in 0..free {
            *el_mut(&mut gm, a * free + a)? += 1.0e-12 * trace / free as f64;
        }
        // each interior part's extra direction: at its base point (its (1, 1)) F's Hessian is
        // forced singular along sigma (the faces' conditions there); sigma in its (s, t)
        let mut kernel = vec![-1.0; 2 * n];
        for q in 0..n {
            let bq = el(&bs, q)?;
            let bprev = el(&bs, (q + n - 1) % n)?;
            let bnext = el(&bs, (q + 1) % n)?;
            let ds = P2 {
                x: (bq.x - bprev.x) * 0.5,
                y: (bq.y - bprev.y) * 0.5,
            };
            let dt = P2 {
                x: (bq.x - bnext.x) * 0.5,
                y: (bq.y - bnext.y) * 0.5,
            };
            let det = ds.x * dt.y - ds.y * dt.x;
            if !(det.abs() > 0.0) {
                return Err(sm_err().into());
            }
            let sq = el(&sigma, q)?;
            let a = (sq.x * dt.y - sq.y * dt.x) / det;
            let b = (ds.x * sq.y - ds.y * sq.x) / det;
            let mut th = b.atan2(a);
            if th < 0.0 {
                th += PI;
            }
            if th >= PI {
                th -= PI;
            }
            *el_mut(&mut kernel, n + q)? = th;
        }

        // the sector conditions on every piece's coefficient matrices, in t: C t >= b
        let mut cm: Vec<f64> = Vec::new();
        let mut bv: Vec<f64> = Vec::new();
        for (p, patch) in patches.iter().enumerate() {
            let mut theta: Vec<f64> = (0..GRAPH_DIRECTIONS)
                .map(|k| PI * k as f64 / GRAPH_DIRECTIONS as f64)
                .collect();
            let kp = el(&kernel, p)?;
            if kp >= 0.0 {
                theta.push(kp);
            }
            theta.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
            theta.push(el(&theta, 0)? + PI);
            let q0p = el_ref(&q0s, p)?;
            let qjp = el_ref(&qjs, p)?;
            let (mut big_p, mut big_q) = (0, 0);
            for t in q0p {
                big_p = if big_p > t.p { big_p } else { t.p };
                big_q = if big_q > t.q { big_q } else { t.q };
            }
            // l, m, n over t: at t = 0 (y = yp) and per free direction, at common degrees, in
            // pieces
            let mut pc0: Vec<Vec<Tensor>> = vec![Vec::new(); 3];
            let mut pcf: Vec<Vec<Tensor>> = vec![Vec::new(); 3 * free];
            let mut scale = 0.0;
            for k in 0..3 {
                let q0k = el_ref(q0p, k)?;
                let mut s0 = tensor_elevate(q0k, big_p, big_q)?;
                let mut sf = vec![t_make(big_p, big_q); free];
                let qjk = el_ref(qjp, k)?;
                for (t, &u) in patch.index.iter().enumerate() {
                    let sj = tensor_elevate(el_ref(qjk, t)?, big_p, big_q)?;
                    scale = smax(scale, t_scale(&sj));
                    let ypu = el(&yp, u)?;
                    for (s0i, &sji) in s0.c.iter_mut().zip(&sj.c) {
                        *s0i += ypu * sji;
                    }
                    for (f, sff) in sf.iter_mut().enumerate() {
                        let z = el(el_ref(&zn, f)?, u)?;
                        if z == 0.0 {
                            continue;
                        }
                        for (sfi, &sji) in sff.c.iter_mut().zip(&sj.c) {
                            *sfi += z * sji;
                        }
                    }
                }
                scale = smax(scale, t_scale(q0k));
                *el_mut(&mut pc0, k)? = t_pieces(&s0, GRAPH_LEVELS)?;
                for (f, sff) in sf.iter().enumerate() {
                    *el_mut(&mut pcf, k * free + f)? = t_pieces(sff, GRAPH_LEVELS)?;
                }
            }
            let sp = el(&signs, p)?;
            let mut row_t = vec![0.0; free];
            let pc00 = el_ref(&pc0, 0)?;
            for pc in 0..pc00.len() {
                for i in 0..el_ref(pc00, pc)?.c.len() {
                    for a in 0..theta.len() - 1 {
                        for pair in 0..2 {
                            let ta = el(&theta, a)?;
                            let (c0, s0) = (ta.cos(), ta.sin());
                            let (c1, s1) = if pair == 0 {
                                (c0, s0)
                            } else {
                                let tb = el(&theta, a + 1)?;
                                (tb.cos(), tb.sin())
                            };
                            let w = [c0 * c1, c0 * s1 + s0 * c1, s0 * s1];
                            let mut cst = 0.0;
                            let mut dmax = 0.0;
                            for (k, &wk) in w.iter().enumerate() {
                                cst += sp * wk * el(&el_ref(el_ref(&pc0, k)?, pc)?.c, i)?;
                            }
                            for (f, rf) in row_t.iter_mut().enumerate() {
                                let mut s = 0.0;
                                for (k, &wk) in w.iter().enumerate() {
                                    s += sp
                                        * wk
                                        * el(&el_ref(el_ref(&pcf, k * free + f)?, pc)?.c, i)?;
                                }
                                *rf = s;
                                dmax = smax(dmax, s.abs());
                            }
                            if !(dmax > 1.0e-11 * scale) {
                                continue; // no free height in it: the certificate decides it
                            }
                            for &rf in &row_t {
                                cm.push(rf / dmax);
                            }
                            bv.push((-cst + 1.0e-12 * scale) / dmax); // held a little inside
                        }
                    }
                }
            }
        }
        stats.solves += 1;
        if let Some(pr) = probe.as_deref_mut().filter(|p| !p.fill && p.margin) {
            // the least tau with C t + tau >= b (tau >= -1): min tau + eps (|t|^2 + tau^2)
            let ln = free + 1;
            let lm = bv.len();
            let mut gmm = vec![0.0; ln * ln];
            let mut gvm = vec![0.0; ln];
            let mut cmm = vec![0.0; lm * ln + ln];
            let mut bvm = bv.clone();
            for a in 0..ln {
                *el_mut(&mut gmm, a * ln + a)? = 1.0e-9;
            }
            *el_mut(&mut gvm, free)? = 1.0;
            for i in 0..lm {
                for f in 0..free {
                    *el_mut(&mut cmm, i * ln + f)? = el(&cm, i * free + f)?;
                }
                *el_mut(&mut cmm, i * ln + free)? = 1.0;
            }
            for f in 0..ln {
                *el_mut(&mut cmm, lm * ln + f)? = if f == free { 1.0 } else { 0.0 };
            }
            bvm.push(-1.0);
            return match dual_qp_rows(&gmm, &gvm, &cmm, &bvm, ln, lm + 1, 1.0e-13)? {
                QpOutcome::Solved(tm) => {
                    pr.margin_value = el(&tm, free)?;
                    Ok(Fill::default())
                }
                // SER(eM)
                QpOutcome::NotDefinite => Err(SolidError::invalid_input(
                    "blend::blend_setback::graph_g1",
                    "the margin's programme is not positive definite",
                )
                .into()),
                QpOutcome::Infeasible { .. } => Err(sm_err().into()),
            };
        }
        let outcome = dual_qp_rows(&gm, &gv, &cm, &bv, free, bv.len(), 1.0e-13)?;
        if let Some(pr) = probe.as_deref_mut().filter(|p| !p.fill) {
            pr.feasible = matches!(outcome, QpOutcome::Solved(_));
            return Ok(Fill::default());
        }
        let t = match outcome {
            QpOutcome::Solved(t) => t,
            QpOutcome::Infeasible { proven: true } if d == 9 => break 'degrees,
            _ => continue,
        };
        let Some(mut nets) = nets_at(&t)? else {
            // SER(netsAt): SM_ERR
            return Err(sm_err().into());
        };
        if certified_convex(&nets, e, GRAPH_DEPTH)?.is_err() {
            continue;
        }
        if d == 9 && probe.is_none() {
            top = nets;
            top_degree = d;
            continue;
        }
        to_world(&mut nets)?;
        return Ok(fill_from(&nets, cycle_round, cycle_face, cycle_rev, d)?);
    }
    if !top.is_empty() {
        to_world(&mut top)?;
        return Ok(fill_from(
            &top,
            cycle_round,
            cycle_face,
            cycle_rev,
            top_degree,
        )?);
    }
    stats.uncertified = true;
    Err(Refusal::sm_err(
        "SmBlendSetback: no G1 fill up to degree 9 is certified convex at these setbacks",
    ))
}
