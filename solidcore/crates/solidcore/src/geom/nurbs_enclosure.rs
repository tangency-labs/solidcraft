//! Interval enclosures of a NURBS surface's partial derivatives over a box of its
//! parameters: one polynomial piece ([`nurbs_partials_over_piece`]) and a box cut at the
//! knot lines inside it ([`nurbs_partials_over_box`]).
//!
//! Fork: `sm_NurbsPartialsOverPiece` and its helpers in
//! `source/SMLib/src/SmFilletSection.cpp` (tangency-labs/smlib); the box form and the knot
//! cuts are not upstream. Moved, unchanged, from `blend::fillet_section::certificate`, which
//! re-exports them: they are read by the blend's certificate, the offset's reach, the
//! surface intersector's contact identity and the certified line/surface solve
//! ([`crate::surface::surface::certified_line`]), none of which should reach into the blend
//! for a surface's enclosure. They sit beside [`crate::geom::krawczyk`], whose tests they
//! feed.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`nurbs_partials_over_piece`], [`nurbs_partials_over_piece_about`], `partials_over_piece` | `sm_NurbsPartialsOverPiece` (the fork's) |
//! | [`nurbs_partials_over_box`], `knot_cuts`, `below_continuity`, `partials_hull` | (not upstream) |
//!
//! Deviations: none from where they were, but one refusal: an order past `MAX_K` (the
//! highest a `Partials` holds, which the fork never asked for) is refused before any table
//! is sized by it. The errors keep the operation name they had there, so no message
//! changes. Knots and nets are read by checked indices (a knot or a
//! coordinate outside reads NaN, which shows in the result). No recursion.

use crate::blend::fillet_section::certificate::no_part_of_the_box;
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::{v_hull, MAX_K, V3, VZ};
use crate::blend::fillet_section::station::{ders_basis, knot, span, Nurbs, Partials};
use crate::error::blend::{BlendFailure, SectionProof};
use crate::error::{Result, SolidError};

/// The operation the errors name: where these functions were, kept so that no message
/// changes with the move.
const OP: &str = "blend::fillet_section::certificate";

/// `crPw[i]` (NaN outside).
#[inline]
pub(crate) fn pw_at(pw: &[f64], i: usize) -> f64 {
    pw.get(i).copied().unwrap_or(f64::NAN)
}

/// `rM[i][j]` of a nested vector (0 outside; the fork's indices stay inside).
#[inline]
pub(crate) fn at2<T: Copy>(m: &[Vec<T>], i: usize, j: usize, zero: T) -> T {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(zero)
}

/// `v[i]` of a vector (0 outside).
#[inline]
pub(crate) fn at1<T: Copy>(v: &[T], i: usize, zero: T) -> T {
    v.get(i).copied().unwrap_or(zero)
}

/// The point interval 0.
pub(crate) const IZ: Ivl = Ivl::new(0.0, 0.0);

/// `sm_NurbsPartialsOverPiece`: the partials `P[a][b]`, `a + b <= lk`, of one polynomial
/// piece (spans `ku`, `kv`) over `[ua, ub] x [va, vb]`, enclosed: the homogeneous surface's
/// Taylor polynomials at the box's middle, from exact point derivatives (it is of degree
/// p, q there), then the rational quotient's (the NURBS book's A4.4) in intervals.
///
/// # Errors
///
/// `SM_ERR` where the weights are not proved positive over the box ("a face's weights are
/// not positive over a box"); [`SolidError::Unsupported`] for an order `lk` past
/// [`MAX_K`], the highest a [`Partials`] holds.
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_over_piece(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    partials_over_piece(cn, ku, kv, ua, ub, va, vb, lk, None, Shift::None)
}

/// [`nurbs_partials_over_piece`] of the surface moved by `−about`: the partials of
/// `S − about`, the homogeneous net's point derivatives taken less `about` times the
/// weight's, in intervals, before the quotient. The partials of order above 0 are those of
/// `S`; enclosed about a point near the box they are far narrower where the weights vary
/// (the quotient's `w_ij S` terms are of the size of `S − about`, not of `S`'s distance
/// from the origin).
///
/// # Errors
///
/// As [`nurbs_partials_over_piece`].
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_over_piece_about(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
    about: [f64; 3],
) -> Result<Partials<Ivl>> {
    partials_over_piece(cn, ku, kv, ua, ub, va, vb, lk, None, Shift::About(about))
}

/// The point the rational quotient of [`partials_over_piece`] is taken about.
#[derive(Clone, Copy)]
pub(crate) enum Shift {
    /// The origin: the partials of `S`.
    None,
    /// `about`: the partials of `S − about` ([`nurbs_partials_over_piece_about`]).
    About([f64; 3]),
    /// The surface's point at the box's middle `o`, added back: the partials of `S`
    /// (`nurbs_partials_within_span_centred` of the blend's certificate).
    Middle,
}

/// [`nurbs_partials_over_piece`], the weight's enclosure cut to `weights` where given, the
/// quotient taken about the point `shift` names.
#[allow(clippy::too_many_arguments)]
pub(crate) fn partials_over_piece(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
    weights: Option<Ivl>,
    shift: Shift,
) -> Result<Partials<Ivl>> {
    // A Partials holds the orders up to MAX_K; a higher one is refused before any table
    // is sized by it (an order near usize::MAX would overflow `lk + 1` or loop for ever).
    if lk > MAX_K {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "partial derivatives past the highest order an enclosure holds",
        });
    }
    let um = 0.5 * (ua + ub);
    let vm = 0.5 * (va + vb);
    let nu = ders_basis(&cn.u, ku, cn.p, Ivl::from(um), cn.p);
    let nv = ders_basis(&cn.v, kv, cn.q, Ivl::from(vm), cn.q);
    let (la, lb) = (cn.p, cn.q);
    // homogeneous point derivatives Hm[a][b] (x, y, z, w) at the middle
    let mut hm = vec![vec![[IZ; 4]; lb + 1]; la + 1];
    for (a, row) in hm.iter_mut().enumerate() {
        for (b, hab) in row.iter_mut().enumerate() {
            let mut h = [IZ; 4];
            for i in 0..=cn.p {
                for j in 0..=cn.q {
                    // ku >= p and kv >= q (sm_Span)
                    let li = 4 * ((ku + i - cn.p) * cn.nv + (kv + j - cn.q));
                    let c = at2(&nu, a, i, IZ) * at2(&nv, b, j, IZ);
                    for (e, he) in h.iter_mut().enumerate() {
                        *he = *he + c * Ivl::from(pw_at(&cn.pw, li + e));
                    }
                }
            }
            *hab = h;
        }
    }
    // the point the quotient is taken about, and whether it is added back to P[0][0]
    let (about, add_back) = match shift {
        Shift::None => (None, false),
        Shift::About(o) => (Some(o), false),
        Shift::Middle => {
            // the surface's point at the middle, as doubles: any finite point is sound,
            // this one makes S − o smallest over the box
            let h = at2(&hm, 0, 0, [IZ; 4]);
            let w = h[3].mid();
            let o = [h[0].mid() / w, h[1].mid() / w, h[2].mid() / w];
            let finite = o.iter().all(|x| x.is_finite());
            (finite.then_some(o), finite)
        }
    };
    if let Some(o) = about {
        for row in &mut hm {
            for h in row.iter_mut() {
                // (A − o w): the homogeneous form of S − o
                let w = h[3];
                for (he, &oe) in h.iter_mut().zip(&o) {
                    *he = *he - Ivl::from(oe) * w;
                }
            }
        }
    }
    let du = Ivl::new(ua, ub) - Ivl::from(um);
    let dv = Ivl::new(va, vb) - Ivl::from(vm);
    let mut pu = vec![IZ; la + 1];
    let mut pv = vec![IZ; lb + 1];
    if let (Some(u0), Some(v0)) = (pu.first_mut(), pv.first_mut()) {
        *u0 = Ivl::from(1.0);
        *v0 = Ivl::from(1.0);
    }
    for i in 1..=la {
        let x = at1(&pu, i - 1, IZ) * du * (Ivl::from(1.0) / Ivl::from(i as f64));
        if let Some(pi) = pu.get_mut(i) {
            *pi = x;
        }
    }
    for j in 1..=lb {
        let x = at1(&pv, j - 1, IZ) * dv * (Ivl::from(1.0) / Ivl::from(j as f64));
        if let Some(pj) = pv.get_mut(j) {
            *pj = x;
        }
    }
    let mut av = vec![vec![VZ; lk + 1]; lk + 1];
    let mut wv = vec![vec![IZ; lk + 1]; lk + 1];
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let mut h = [IZ; 4];
            for i in a..=la {
                for j in b..=lb {
                    let c = at1(&pu, i - a, IZ) * at1(&pv, j - b, IZ);
                    let hij = at2(&hm, i, j, [IZ; 4]);
                    for (he, &x) in h.iter_mut().zip(&hij) {
                        *he = *he + x * c;
                    }
                }
            }
            if let (Some(ab), Some(wab)) = (
                av.get_mut(a).and_then(|r| r.get_mut(b)),
                wv.get_mut(a).and_then(|r| r.get_mut(b)),
            ) {
                let [x, y, z, w] = h;
                *ab = V3 { x, y, z };
                *wab = w;
            }
        }
    }
    let w00 = at2(&wv, 0, 0, IZ);
    let w00 = match weights {
        Some(h) => Ivl::new(w00.lo.max(h.lo), w00.hi.min(h.hi)),
        None => w00,
    };
    let positive = w00.lo > 0.0;
    if !positive {
        // "SmGnSectionFSG: a face's weights are not positive over a box" (status SM_ERR
        // and this operation: `is_certificate_sm_err` subdivides a wide box on it)
        return Err(SolidError::blend(
            OP,
            BlendFailure::SectionNotProved(SectionProof::FaceWeightPositive),
        ));
    }
    let mut out = Partials::default();
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let mut v3 = at2(&av, a, b, VZ);
            let mut cka = 1.0_f64;
            for i in 0..=a {
                if i > 0 {
                    cka = cka * (a - i + 1) as f64 / i as f64;
                }
                let mut cbj = 1.0_f64;
                for j in 0..=b {
                    if j > 0 {
                        cbj = cbj * (b - j + 1) as f64 / j as f64;
                    }
                    if i == 0 && j == 0 {
                        continue;
                    }
                    let c = Ivl::from(cka * cbj) * at2(&wv, i, j, IZ);
                    let pij = out.at(a - i, b - j);
                    v3.x = v3.x - c * pij.x;
                    v3.y = v3.y - c * pij.y;
                    v3.z = v3.z - c * pij.z;
                }
            }
            out.set(
                a,
                b,
                V3 {
                    x: v3.x / w00,
                    y: v3.y / w00,
                    z: v3.z / w00,
                },
            );
        }
    }
    if let (true, Some(o)) = (add_back, about) {
        // S = (S − o) + o, rounded outward
        let p = out.at(0, 0);
        out.set(
            0,
            0,
            V3 {
                x: p.x + Ivl::from(o[0]),
                y: p.y + Ivl::from(o[1]),
                z: p.z + Ivl::from(o[2]),
            },
        );
    }
    Ok(out)
}

/// The parts of `[lo, hi]` between the knots strictly inside it, each with the span whose
/// closed interval holds it: a knot crossed where the direction is continuous to less than
/// `continuity` (degree minus multiplicity) is refused, and so is an interval past the
/// knots' range (the face or curve is not defined there). A point `lo = hi` on an inner
/// knot lies in both spans' closed intervals: it is returned twice, once with each, so a
/// hull over the parts holds both one-sided values there (#2570).
pub(crate) fn knot_cuts(
    knots: &[f64],
    p: usize,
    n: usize,
    lo: f64,
    hi: f64,
    continuity: usize,
) -> Result<Vec<(f64, f64, usize)>> {
    let refused = |reason| SolidError::Unsupported {
        operation: OP,
        reason,
    };
    let (first, last) = (knot(knots, p as isize), knot(knots, n as isize));
    if !(lo >= first && hi <= last && lo <= hi) {
        return Err(refused("a box past the face's knots"));
    }
    let after = span(knots, p, n, lo);
    if lo == hi && after > p && knot(knots, after as isize) == lo {
        // the span that ends at the knot: the last one before it that is not empty
        let mut before = after - 1;
        while before > p && knot(knots, before as isize) >= knot(knots, before as isize + 1) {
            before -= 1;
        }
        return Ok(vec![(lo, hi, before), (lo, hi, after)]);
    }
    let mut at = vec![lo];
    let mut prev = f64::NAN;
    for &x in knots {
        if x > lo && x < hi && x != prev {
            if below_continuity(knots, p, x, continuity) {
                return Err(refused(
                    "a box crosses a knot line where the face or curve is not continuous enough",
                ));
            }
            at.push(x);
        }
        prev = x;
    }
    at.push(hi);
    Ok(at
        .iter()
        .zip(at.iter().skip(1))
        // The left endpoint selects the span to its right. A rounded midpoint
        // can land on the next knot when the subinterval is only one ulp wide.
        .map(|(&a, &b)| (a, b, span(knots, p, n, a)))
        .collect())
}

/// A NURBS face's partials `P[a][b]`, `a + b <= lk`, over `[ua, ub] x [va, vb]` wherever
/// it lies on the face's knots, enclosed (not upstream): the box cut at the knot lines
/// strictly inside it, each part enclosed on its own polynomial piece
/// ([`nurbs_partials_over_piece`] over the part, its closed span), and the enclosures'
/// hull. Every point of the box lies in some closed part, where that piece is the face, so
/// the hull holds, at every point, each partial the face has there: an enclosure point by
/// point, whatever the continuity. On a knot line strictly inside the box, and for a box
/// of zero width on a knot line, it holds the partials from either side; a box of nonzero
/// width that ends on a knot line holds there only the partials from its own side (the
/// piece the box lies on), not the other piece's one-sided values. A use that also
/// integrates over the box (a mean-value form) needs the face continuous to some order
/// across the lines it crosses: a crossed line where the face's continuity (degree minus
/// the knot's multiplicity) is below `continuity` is refused, as is a box past the knots'
/// range. No station invariant is assumed.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a crossed knot line of too low a continuity or a box
/// past the knots; those of [`nurbs_partials_over_piece`].
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_over_box(
    cn: &Nurbs,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
    continuity: usize,
) -> Result<Partials<Ivl>> {
    over_box(cn, ua, ub, va, vb, lk, continuity, Shift::None)
}

/// [`nurbs_partials_over_box`], each piece's quotient taken about the point `shift` names.
#[allow(clippy::too_many_arguments)]
fn over_box(
    cn: &Nurbs,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
    continuity: usize,
    shift: Shift,
) -> Result<Partials<Ivl>> {
    let us = knot_cuts(&cn.u, cn.p, cn.nu, ua, ub, continuity)?;
    let vs = knot_cuts(&cn.v, cn.q, cn.nv, va, vb, continuity)?;
    let mut out: Option<Partials<Ivl>> = None;
    for &(a0, a1, ku) in &us {
        for &(b0, b1, kv) in &vs {
            let p = partials_over_piece(cn, ku, kv, a0, a1, b0, b1, lk, None, shift)?;
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    out.ok_or_else(|| no_part_of_the_box(OP))
}

/// The hull of two enclosures of partials, `a + b <= lk`.
pub(crate) fn partials_hull(mut h: Partials<Ivl>, p: &Partials<Ivl>, lk: usize) -> Partials<Ivl> {
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            h.set(a, b, v_hull(h.at(a, b), p.at(a, b)));
        }
    }
    h
}

/// Whether the knot `x` of `knots` (degree `p`) is below `continuity` by count: degree minus
/// its multiplicity. [`knot_cuts`] refuses a crossed line by it, and the cell path's
/// trigger reads it, so the two cannot drift apart.
pub(crate) fn below_continuity(knots: &[f64], p: usize, x: f64, continuity: usize) -> bool {
    let m = knots.iter().filter(|&&y| y == x).count();
    p.checked_sub(m).is_none_or(|c| c < continuity)
}

/// [`nurbs_partials_over_box`] of the surface moved by `−about`
/// ([`nurbs_partials_over_piece_about`] on each part): the partials of `S − about`, far
/// narrower than those of `S` where the box lies far from the origin, with the same
/// contract on knot lines (an endpoint of a box of nonzero width takes its own side's
/// piece). Not upstream.
///
/// # Errors
///
/// As [`nurbs_partials_over_box`].
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_over_box_about(
    cn: &Nurbs,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
    continuity: usize,
    about: [f64; 3],
) -> Result<Partials<Ivl>> {
    over_box(cn, ua, ub, va, vb, lk, continuity, Shift::About(about))
}
