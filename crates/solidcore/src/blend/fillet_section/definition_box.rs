//! A face that is a blend by its definition, enclosed over a box of its `(u, v)` for the
//! certificate of a blend on it (opt-in, not upstream): its feet over the box's s by
//! Krawczyk's operator on the polynomial pieces of the faces under them, then its arc in
//! rounded interval arithmetic, in the centred form where one order more is held. Only a
//! round of the ball's circle (a constant radius) between NURBS faces is enclosed.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/src/SmFilletSection.cpp`
//! (tangency-labs/smlib@c5b6bb7, smlib#101, merged at 852dd8c). Only
//! [`crate::kernel::UpstreamFixes::fillet_round_on_round_as_definition`] builds a blend
//! on such a face.
//!
//! | Rust                                  | Fork (`SmFilletSection.cpp`)                     |
//! |---------------------------------------|--------------------------------------------------|
//! | [`DefPiece`]                          | `sm_GnDefPiece`                                  |
//! | [`GnDefFace`], [`GnDefs`]             | `sm_GnDefFace`, `sm_GnDefs`                      |
//! | [`definition_enclosable`]             | `sm_DefinitionEnclosable`                        |
//! | [`gn_def_face_of`]                    | `sm_GnDefFaceOf`                                 |
//! | `def_piece_at` (private)              | `sm_DefPieceAt`                                  |
//! | [`bern_taylor_over`]                  | `sm_BernTaylorOver`                              |
//! | [`definition_partials_naive`]         | `sm_DefinitionPartialsNaive`                     |
//! | [`definition_partials_over_box`]      | `sm_DefinitionPartialsOverBox`                   |
//!
//! Deviations:
//! - **Ownership.** `sm_GnDefFace` points at the definition and its supports; here they
//!   are borrowed for the certificate's lifetime. Its pieces' cache is a `Vec` grown to
//!   the net's knot count on first use, as the fork's.
//! - **Out-params.** Every function returns its results; `SM_ERR` is
//!   [`SolidError::sm_err`], which the certificate takes as nothing proved.
//! - **Bounds.** The enclosure of the inner definition's feet takes no definition of its
//!   own (its supports are NURBS faces), so the nesting is one deep.

use super::certificate::{gn_enclose_over, NurbsCurve, SpanAt};
use super::definition::{t2_pow, tv_dot, T2Of, TvOf};
use super::interval::{cmax, cmin, Ivl};
use super::series::{Trunc, MAX_S, V3};
use super::station::{as_b_spline, gn_solve_station, smax, span, Nurbs, Partials};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::surface::procedural_blend::{ProceduralBlend, PROCEDURAL_BLEND_TYPE};
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "blend::fillet_section::definition_box";

/// `sm_T2Of<sm_Ivl>`: a series in `(sigma, dt)` enclosed.
type T2 = T2Of<Ivl>;

/// `sm_TVT<sm_Ivl>`.
type Tv = TvOf<Ivl>;

/// `sm_GnDefPiece`: the station at the middle of one piece of a definition's net along s
/// (between two of its stations), solved once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DefPiece {
    /// `lState`: 0 not solved yet, 1 solved, -1 the station did not solve.
    state: i8,
    /// `dS`: the piece's middle.
    s: f64,
    /// `ax`: the feet there, their first s-derivatives and second over 2.
    ax: [[f64; 4]; 3],
    /// `aku`, `akv`: the faces' polynomial pieces the feet lie on over the piece.
    ku: [usize; 2],
    kv: [usize; 2],
    /// `lLeader`: the leader's.
    leader: usize,
}

impl DefPiece {
    /// Not solved yet.
    const NONE: Self = Self {
        state: 0,
        s: 0.0,
        ax: [[0.0; 4]; 3],
        ku: [0; 2],
        kv: [0; 2],
        leader: 0,
    };
}

/// `sm_GnDefFace`: a face that is a blend by its definition, for the certificate of a
/// blend on it: its definition's partials over a box of its `(u, v)` enclosed
/// ([`definition_partials_over_box`]), on the polynomial pieces of the faces it rolls on
/// that its piece of the net lies on.
#[derive(Clone, Debug)]
pub struct GnDefFace<'a> {
    /// `pDef`.
    def: &'a ProceduralBlend,
    /// `apSup`: the faces it rolls on.
    sup: [&'a Surface; 2],
    /// `asSup`: their NURBS data.
    sup_n: [Nurbs; 2],
    /// `sLeader`.
    leader: NurbsCurve,
    /// `adR`, `adSide`.
    r: [f64; 2],
    side: [f64; 2],
    /// `adA`, `adB`, `bSwapped`: its `(s, t) = (A + B p, ...)`
    /// ([`ProceduralBlend::definition_map`]).
    a: [f64; 2],
    b: [f64; 2],
    swapped: bool,
    /// `sPieces`: by the net's knot span along s.
    pieces: Vec<DefPiece>,
}

/// `sm_GnDefs`: the enclosed definitions among a blend's two faces (`None`: a NURBS
/// face).
#[derive(Clone, Debug, Default)]
pub struct GnDefs<'a> {
    /// `ap`.
    pub faces: [Option<GnDefFace<'a>>; 2],
}

/// `sm_DefinitionEnclosable`: a definition the certificate encloses, the ball's circle of
/// one radius between two NURBS faces (its partials of total order up to `SM_GN_MAXS`: the
/// order its feet's series hold in s).
#[must_use]
pub fn definition_enclosable(surface: &Surface) -> bool {
    let SurfaceKind::ProceduralBlend(d) = surface.kind() else {
        return false;
    };
    // (the leader and the seeds are always there)
    if d.order() != 1 || d.law().is_some() {
        return false;
    }
    for i in 0..2 {
        let Some(sup) = d.support(i) else {
            return false;
        };
        let nurbs = sup.is_kind_of(BSplineSurface::TYPE).unwrap_or(false);
        let blend = sup.is_kind_of(PROCEDURAL_BLEND_TYPE).unwrap_or(true);
        if !nurbs || blend {
            return false;
        }
    }
    true
}

/// `sm_GnDefFaceOf`: the face `surface`, a blend by its definition, for the certificate.
///
/// # Errors
///
/// `SM_ERR_INVALID_INPUT` for a definition that is not enclosed ("a blend by its definition
/// that is not enclosed (only the ball's circle of one radius between NURBS faces is)");
/// those of the supports' and the leader's NURBS data.
pub fn gn_def_face_of<'a>(k: &Kernel, surface: &'a Surface) -> Result<GnDefFace<'a>> {
    let def = match surface.kind() {
        SurfaceKind::ProceduralBlend(d) if definition_enclosable(surface) => d.as_ref(),
        _ => {
            return Err(SolidError::invalid_input(
                OP,
                "a blend by its definition that is not enclosed (only the ball's circle of one radius between NURBS faces is)",
            ))
        }
    };
    let (Some(s0), Some(s1)) = (def.support(0), def.support(1)) else {
        return Err(SolidError::sm_err(OP));
    };
    let nurbs_of = |s: &Surface| -> Result<Nurbs> {
        let bs = as_b_spline(k, s)?.ok_or_else(|| SolidError::sm_err(OP))?;
        Nurbs::of(&bs)
    };
    let sup_n = [nurbs_of(s0)?, nurbs_of(s1)?];
    let (a, b, swapped) = def.definition_map();
    Ok(GnDefFace {
        def,
        sup: [s0, s1],
        sup_n,
        leader: NurbsCurve::of(def.leader())?,
        r: [
            def.offset(0).unwrap_or(f64::NAN),
            def.offset(1).unwrap_or(f64::NAN),
        ],
        side: [
            def.side(0).unwrap_or(f64::NAN),
            def.side(1).unwrap_or(f64::NAN),
        ],
        a,
        b,
        swapped,
        pieces: Vec::new(),
    })
}

/// `sm_DefPieceAt`: the station at the middle of the net's piece `kk` along s, solved
/// once: its feet's pieces of the faces, the leader's piece and the feet's Taylor terms
/// there (the guesses over the piece).
///
/// # Errors
///
/// `SM_ERR` for a piece that is not one (an index outside, or two equal knots) or whose
/// station did not solve; those of the seeds' and the leader's evaluations.
fn def_piece_at(k: &Kernel, d: &mut GnDefFace<'_>, net: &Nurbs, kk: usize) -> Result<DefPiece> {
    let knots = if d.swapped { &net.v } else { &net.u };
    let (Some(&k0), Some(&k1)) = (knots.get(kk), knots.get(kk + 1)) else {
        return Err(SolidError::sm_err(OP));
    };
    let piece = k1 > k0;
    if !piece {
        return Err(SolidError::sm_err(OP));
    }
    if d.pieces.len() < knots.len() {
        d.pieces.resize(knots.len(), DefPiece::NONE);
    }
    let state = d.pieces.get(kk).map_or(-1, |p| p.state);
    if state == 0 {
        let mut p = DefPiece::NONE;
        p.state = -1;
        if let Some(slot) = d.pieces.get_mut(kk) {
            *slot = p;
        }
        let def = d.def;
        p.s = d.a[0] + d.b[0] * 0.5 * (k0 + k1);
        let mut seed = [0.0_f64; 4];
        for f in 0..2 {
            let w = def
                .seed(f)
                .ok_or_else(|| SolidError::sm_err(OP))?
                .evaluate_point(p.s)?;
            if let Some(x) = seed.get_mut(2 * f) {
                *x = w.x;
            }
            if let Some(x) = seed.get_mut(2 * f + 1) {
                *x = w.y;
            }
        }
        let l = def.leader().evaluate_point(p.s)?;
        let tr = Trunc::new(2, 0)?;
        let tol = 1.0e-9 * smax(1.0, l.length());
        if let Ok(st) = gn_solve_station(k, d.sup, def.leader(), p.s, seed, d.r, d.side, tr, tol) {
            for (i, row) in p.ax.iter_mut().enumerate() {
                for (q, x) in row.iter_mut().enumerate() {
                    *x = st.x.get(q).map_or(f64::NAN, |sq| sq.at(i, 0));
                }
            }
            for (f, n) in d.sup_n.iter().enumerate() {
                let [x0, ..] = p.ax;
                let fu = x0.get(2 * f).copied().unwrap_or(f64::NAN);
                let fv = x0.get(2 * f + 1).copied().unwrap_or(f64::NAN);
                if let (Some(u), Some(v)) = (p.ku.get_mut(f), p.kv.get_mut(f)) {
                    *u = span(&n.u, n.p, n.nu, fu);
                    *v = span(&n.v, n.q, n.nv, fv);
                }
            }
            p.leader = span(&d.leader.u, d.leader.p, d.leader.n, p.s);
            p.state = 1;
            if let Some(slot) = d.pieces.get_mut(kk) {
                *slot = p;
            }
        }
    }
    match d.pieces.get(kk) {
        Some(p) if p.state == 1 => Ok(*p),
        _ => Err(SolidError::sm_err(OP)),
    }
}

/// `sm_BernTaylorOver`: `p^(j)(t) / j!`, `j <= lj`, of the polynomial of Bernstein
/// coefficients `b`, at every t in `t`.
#[must_use]
pub fn bern_taylor_over(b: &[f64], t: Ivl, lj: usize) -> Vec<Ivl> {
    let mut out = vec![Ivl::from(0.0); lj + 1];
    let mut d: Vec<Ivl> = b.iter().map(|&x| Ivl::from(x)).collect();
    let mut fact = 1.0_f64;
    let mut j_fact = 1.0_f64;
    let mut j = 0;
    while j <= lj && !d.is_empty() {
        if j > 0 {
            j_fact *= j as f64;
        }
        // de Casteljau at every t of the interval
        let mut w = d.clone();
        for r in 1..w.len() {
            for q in 0..(w.len() - r) {
                let (Some(&a), Some(&c)) = (w.get(q), w.get(q + 1)) else {
                    continue;
                };
                if let Some(x) = w.get_mut(q) {
                    *x = (Ivl::from(1.0) - t) * a + t * c;
                }
            }
        }
        if let (Some(o), Some(&w0)) = (out.get_mut(j), w.first()) {
            *o = Ivl::from(fact) * w0 / Ivl::from(j_fact);
        }
        // the derivative's coefficients: deg (b_{q+1} - b_q)
        let deg = (d.len() - 1) as f64;
        let next: Vec<Ivl> = d
            .windows(2)
            .map(|p| match p {
                [a, b] => *b - *a,
                _ => Ivl::from(0.0),
            })
            .collect();
        fact *= deg;
        d = next;
        j += 1;
    }
    out
}

/// `sm_DefinitionPartialsNaive`: the definition's partials over the box
/// `[ua, ub] x [va, vb]` of its `(u, v)`, to total order `lk`, enclosed: `P[a][b]` holds
/// `d^(a+b) S / du^a dv^b` at every point of the box. On the net's piece along s that
/// `(ku, kv)` is in (between two of the definition's stations) its feet lie on one
/// polynomial piece of each face it rolls on (a station stands where a foot crosses a knot
/// line), and the definition there is that of the pieces' polynomials, continued over the
/// rest of the box as the faces' pieces are ([`gn_enclose_over`]): its feet's series in s
/// over the box's s enclosed by Krawczyk's operator on those pieces, the leader's piece,
/// then the arc in `(sigma, dt)` over the box's t, as `EvaluateDefinition` makes it (the
/// circle's centre the ball's: a constant radius), in rounded interval arithmetic.
///
/// The station invariant is the construction's and is not checked here (the certificate's
/// boxes rely on it); [`definition_partials_piecewise`] needs none.
///
/// # Errors
///
/// `SM_ERR` where an enclosure is not proved, or `lk` is past `SM_GN_MAXS`.
#[allow(clippy::too_many_arguments)]
pub fn definition_partials_naive(
    k: &Kernel,
    d: &mut GnDefFace<'_>,
    net: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    partials_naive(k, d, net, [ku, kv], [ua, ub, va, vb], lk, false)
}

/// [`definition_partials_naive`] without the station invariant: the faces it rolls on are
/// enclosed piece by piece over the feet's boxes
/// ([`super::certificate::nurbs_partials_over_box`]: cut at the knot lines a box crosses,
/// each part on its own piece, the hull), so a foot may cross a face's knot line (at a
/// station, or anywhere on a refitted net) and the enclosure is still of the definition,
/// certified to the faces' continuity there; the leader's series over the box's s likewise
/// ([`super::certificate::leader_series_piecewise`]). The partials up to `lk` are each part's
/// hull, which encloses them at every point of the box whatever the continuity at a crossed
/// line, so `lk` sets no continuity; only Krawczyk's mean-value test over the feet's box needs
/// the faces C^2 across a crossed line ([`super::certificate::FEET_BOX_CONTINUITY`]), and the
/// leader continuous. Refused where a crossed knot line is below that, or the box is past a
/// face's or the leader's knots. Not upstream (the certified normal's and point's query,
/// `query::normal`).
///
/// # Errors
///
/// `SM_ERR` where an enclosure is not proved (a crossed knot line refused among them);
/// those of [`definition_partials_naive`].
#[allow(clippy::too_many_arguments)]
pub fn definition_partials_piecewise(
    k: &Kernel,
    d: &mut GnDefFace<'_>,
    net: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    partials_naive(k, d, net, [ku, kv], [ua, ub, va, vb], lk, true)
}

#[allow(clippy::too_many_lines)]
fn partials_naive(
    k: &Kernel,
    d: &mut GnDefFace<'_>,
    net: &Nurbs,
    [ku, kv]: [usize; 2],
    [ua, ub, va, vb]: [f64; 4],
    lk: usize,
    piecewise: bool,
) -> Result<Partials<Ivl>> {
    if lk > MAX_S {
        return Err(SolidError::sm_err(OP));
    }
    let piece = def_piece_at(k, d, net, if d.swapped { kv } else { ku })?;
    // the box in (s, t)
    let (ps, pt) = if d.swapped {
        (Ivl::new(va, vb), Ivl::new(ua, ub))
    } else {
        (Ivl::new(ua, ub), Ivl::new(va, vb))
    };
    let s = Ivl::from(d.a[0]) + Ivl::from(d.b[0]) * ps;
    let t = Ivl::from(d.a[1]) + Ivl::from(d.b[1]) * pt;
    let finite = s.lo.is_finite() && s.hi.is_finite() && t.lo.is_finite() && t.hi.is_finite();
    if !finite {
        return Err(SolidError::sm_err(OP));
    }
    // the feet over the box's s, guessed at its ends from the piece's middle
    let [x0, x1, x2] = piece.ax;
    let mut at = SpanAt {
        s,
        xa: [0.0; 4],
        xb: [0.0; 4],
        ku: piece.ku,
        kv: piece.kv,
        leader: Some(piece.leader),
        piecewise,
    };

    let da = s.lo - piece.s;
    let db = s.hi - piece.s;
    for (q, (xa, xb)) in at.xa.iter_mut().zip(at.xb.iter_mut()).enumerate() {
        let (c0, c1, c2) = (
            x0.get(q).copied().unwrap_or(f64::NAN),
            x1.get(q).copied().unwrap_or(f64::NAN),
            x2.get(q).copied().unwrap_or(f64::NAN),
        );
        *xa = c0 + da * (c1 + da * c2);
        *xb = c0 + db * (c1 + db * c2);
    }
    let [n0, n1] = &d.sup_n;
    let Ok(Some(e)) = gn_enclose_over(k, [n0, n1], &d.leader, &at, d.r, d.side, 0, lk, None, None)
    else {
        return Err(SolidError::sm_err(OP));
    };

    // the arc from the first rail to the second about the ball's centre, weights 1,
    // cos(theta / 2), 1 (to total order lk: the rest is not asked for)
    let one = Ivl::from(1.0);
    let mut rho1 = Tv::zero_with_order(lk, lk, lk);
    let mut rho2 = rho1.clone();
    let mut m = rho1.clone();
    let [f0, f1] = &e.f;
    for c in 0..3 {
        for i in 0..=lk {
            let get = |v: &super::series::VSer<Ivl>| {
                v.a.get(c).map_or_else(|| Ivl::from(0.0), |x| x.at(i, 0))
            };
            if let (Some(r1), Some(r2), Some(mc)) =
                (rho1.a.get_mut(c), rho2.a.get_mut(c), m.a.get_mut(c))
            {
                r1.set(i, 0, get(f0));
                r2.set(i, 0, get(f1));
                mc.set(i, 0, get(&e.v));
            }
        }
    }
    let b1 = rho1.sub(&m);
    let b2 = rho2.sub(&m);
    let i1 = t2_pow(&tv_dot(&b1, &b1), -0.5)?;
    let i2 = t2_pow(&tv_dot(&b2, &b2), -0.5)?;
    // cos^2(theta / 2)
    let half = tv_dot(&b1, &b2)
        .mul(&i1)
        .mul(&i2)
        .add(&T2::with_order(lk, lk, one, lk))
        .scale(Ivl::from(0.5));
    let w = t2_pow(&half, 0.5)?;
    let inv_w2 = t2_pow(&half, -1.0)?;
    let p1 = m.add(&rho1.add(&rho2).scale(Ivl::from(0.5)).sub(&m).times(&inv_w2));
    // the quadratic Bernstein basis about every t of the box: B0 = (1 - t)^2,
    // B1 = 2 t (1 - t), B2 = t^2
    let unit = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let lj = if lk < 2 { lk } else { 2 };
    let basis = unit.map(|u| {
        let mut bb = T2::with_order(lk, lk, Ivl::from(0.0), lk);
        for (j, &x) in bern_taylor_over(&u, t, lj).iter().enumerate() {
            bb.set(0, j, x);
        }
        bb
    });
    let [bs0, bs1, bs2] = &basis;
    let wb1 = w.mul(bs1);
    let inv_den = t2_pow(&bs0.add(&wb1).add(bs2), -1.0)?;
    let sum = rho1
        .times(bs0)
        .add(&p1.times(&wb1))
        .add(&rho2.times(bs2))
        .times(&inv_den);

    // to the surface's (u, v): d/du = b_s d/ds (b_t d/dt when swapped), d/dv the other
    let mut out = Partials::default();
    let mut fa = 1.0_f64;
    for a in 0..=lk {
        if a > 0 {
            fa *= a as f64;
        }
        let mut fb = 1.0_f64;
        for b in 0..=(lk - a) {
            if b > 0 {
                fb *= b as f64;
            }
            let (i, j) = if d.swapped { (b, a) } else { (a, b) };
            let mut scale = Ivl::from(fa * fb);
            for _ in 0..i {
                scale = scale * Ivl::from(d.b[0]);
            }
            for _ in 0..j {
                scale = scale * Ivl::from(d.b[1]);
            }
            let [sx, sy, sz] = &sum.a;
            out.set(
                a,
                b,
                V3 {
                    x: sx.at(i, j) * scale,
                    y: sy.at(i, j) * scale,
                    z: sz.at(i, j) * scale,
                },
            );
        }
    }
    Ok(out)
}

/// `sm_DefinitionPartialsOverBox`: [`definition_partials_naive`] in the centred form where
/// the box has room and one order more is held: the partials at the box's middle plus the
/// next order's over the box times the box's half widths (the mean value theorem), within
/// the direct enclosure, whose interval arithmetic widens with the box much more than the
/// centred form's does.
///
/// # Errors
///
/// `SM_ERR` where an enclosure is not proved, or the two enclosures do not meet.
#[allow(clippy::too_many_arguments)]
pub fn definition_partials_over_box(
    k: &Kernel,
    d: &mut GnDefFace<'_>,
    net: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    if !(lk < MAX_S && (ub > ua || vb > va)) {
        return definition_partials_naive(k, d, net, ku, kv, ua, ub, va, vb, lk);
    }
    let um = 0.5 * (ua + ub);
    let vm = 0.5 * (va + vb);
    let wide = definition_partials_naive(k, d, net, ku, kv, ua, ub, va, vb, lk + 1)?;
    let mid = definition_partials_naive(k, d, net, ku, kv, um, um, vm, vm, lk)?;
    let du = Ivl::new(ua, ub) - Ivl::from(um);
    let dv = Ivl::new(va, vb) - Ivl::from(vm);
    let mut out = Partials::default();
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let (cm, cu, cv, cw) = (
                mid.at(a, b),
                wide.at(a + 1, b),
                wide.at(a, b + 1),
                wide.at(a, b),
            );
            let comps = [
                (cm.x, cu.x, cv.x, cw.x),
                (cm.y, cu.y, cv.y, cw.y),
                (cm.z, cu.z, cv.z, cw.z),
            ];
            let mut got = [Ivl::from(0.0); 3];
            for (g, (m, u, v, w)) in got.iter_mut().zip(comps) {
                let c = m + u * du + v * dv;
                let x = Ivl::new(cmax(c.lo, w.lo), cmin(c.hi, w.hi));
                let meet = x.lo <= x.hi;
                if !meet {
                    return Err(SolidError::sm_err(OP));
                }
                *g = x;
            }
            let [x, y, z] = got;
            out.set(a, b, V3 { x, y, z });
        }
    }
    Ok(out)
}
