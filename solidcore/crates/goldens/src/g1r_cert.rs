//! Certificate of a G1R side of a rational extension (#21383), read off the result's own
//! control net: the property the derivation at `ruled_far_row` in
//! `solidcore::nurbs::srf_adv::p3` turns into G1.
//!
//! For a side extended by G1R, with `B` the boundary row (the row the side's knot of
//! multiplicity `p` interpolates), `I` the next row in, `A` the next row out and `Q` the
//! far row, all in homogeneous coordinates, the certificate checks:
//!
//! * the ruled span: `Q` is `p` rows past `B` and `A = (1 - 1/p) B + Q / p`;
//! * `Q + t I = k B` in every component of every column, for one `k` and the `t > 0`
//!   that the knots give (`p (uu - u_r) / (u_r - u_n)` at the end, `p (u_0 - uu) /
//!   (u_{p+1} - u_0)` at the start, upstream's step, from the base's knots);
//! * every weight of the net positive.
//!
//! Then `A - B = (Q - B) / p = (t (B - I) + (k - 1 - t) B) / p`, so the extension's
//! homogeneous cross derivative at the side is a positive multiple of the base's plus a
//! multiple of the side's own homogeneous point, which adds nothing to the space
//! derivative: the extension is G1 across the side (PROVEN for the exact row; see Rounding). Upstream's
//! row for equal weight ratios `c` is the case `k = c (1 + t)`; the new row is
//! `k = cb (1 + t)`, `cb` the largest ratio.
//!
//! Rounding: on a side with mixed ratios the far row is replayed bitwise from the
//! library's formula with the library's own `k`, so `Q + t I - k B` is within `gamma(5)`
//! of the terms' magnitudes; on an equal-ratio side (upstream's row) the allowance is
//! `gamma(14)`, and `A`'s is `gamma(9)`; each is counted where it is checked. What a pass
//! proves is then that bound: the extension's cross derivative is a positive multiple
//! of the base's plus `s A` plus a residual within those allowances, so the one-sided
//! normals agree to within the residual relative to the cross derivative (ARGUED; G1 up
//! to rounding, and exactly G1 for the exact row the replay rounds).

use solidcore::nurbs::math::{gamma, CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;

/// What a side's certificate found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SideCert {
    /// The side's knot has multiplicity `p - 1`: upstream's conditional knot removal took
    /// one (moving the net by at most `MODEL_TOL`), and the extension is C1 there by
    /// structure.
    KnotRemoved,
    /// The ruled span checks above hold with this `t` and `k`; `mixed` when the side's
    /// weight ratios `w_I / w_B` differ between columns (the new row).
    Ruled { t: f64, k: f64, mixed: bool },
}

/// The homogeneous components of `c` (`z` is `0` in a planar net, whose `z` is `NOZ`
/// in every point, so the checks on it hold trivially).
fn h(c: CPoint) -> Result<[f64; 4], String> {
    if c.w == NOW {
        return Err("not a rational net".into());
    }
    let z = if c.z == NOZ { 0.0 } else { c.z };
    Ok([c.x, c.y, z, c.w])
}

/// The certificate of the side `side` of a G1R step in `ext`'s direction u (`udir`) or
/// v, grown past the end (`end`) or the start of the surface the step extended. The
/// step's `t` is read off `ext`'s knots, as upstream computes it from the step's input:
/// with `uu` the knot next past `side` and `a` the knot next inside it,
/// `t = p |uu - side| / |side - a|`. That holds for every step of a chain (each step
/// keeps its input's knots up to `side`, `side` once less, and adds `uu` `p + 1` times).
pub(crate) fn certify_side(
    ext: &NlSurface,
    udir: bool,
    end: bool,
    side: f64,
) -> Result<SideCert, String> {
    let (p, knots) = if udir {
        (ext.degrees().0, ext.u_knots())
    } else {
        (ext.degrees().1, ext.v_knots())
    };
    let net = ext.cpts();
    for row in &net {
        for c in row {
            if c.w.is_nan() || c.w <= 0.0 {
                return Err(format!("weight {}", c.w));
            }
        }
    }
    if p == 0 {
        return Err("degree 0".into());
    }
    let mult = knots.iter().filter(|&&t| t == side).count();
    if mult + 1 == p {
        return Ok(SideCert::KnotRemoved);
    }
    if mult != p {
        return Err(format!("side knot multiplicity {mult}, degree {p}"));
    }
    let first = knots.iter().position(|&t| t == side).ok_or("side knot")?;
    let ib = first - 1;
    let rows = if udir { net.len() } else { net[0].len() };
    let above = knots
        .iter()
        .copied()
        .find(|&t| t > side)
        .ok_or("no knot past the side")?;
    let below = knots
        .iter()
        .rev()
        .copied()
        .find(|&t| t < side)
        .ok_or("no knot before the side")?;
    let pf = p as f64;
    let (ii, ia, iq, t) = if end {
        (
            ib - 1,
            ib + 1,
            ib + p,
            pf * ((above - side) / (side - below)),
        )
    } else {
        if ib < p {
            return Err(format!("boundary row {ib}, degree {p}"));
        }
        (
            ib + 1,
            ib - 1,
            ib - p,
            pf * ((side - below) / (above - side)),
        )
    };
    if iq >= rows || ii >= rows {
        return Err(format!("rows {ii}, {iq} of {rows}"));
    }
    if t.is_nan() || t <= 0.0 {
        return Err(format!("step {t}"));
    }
    let along = if udir { net[0].len() } else { net.len() };
    let at = |i: usize, j: usize| if udir { net[i][j] } else { net[j][i] };
    // The side's weight ratios, rounded as `mixed_ratio` rounds them (rows `B` and `I`
    // are the step's input rows, kept bitwise; `certify_step` checks that).
    let mut ratios = Vec::with_capacity(along);
    for j in 0..along {
        ratios.push(h(at(ii, j))?[3] / h(at(ib, j))?[3]);
    }
    let first_ratio = *ratios.first().ok_or("empty side")?;
    let mixed = ratios.iter().any(|&c| c != first_ratio);
    let (k, allow_q) = if mixed {
        // The library's own `k`, and `Q` replayed bitwise: then `Q`'s components carry
        // two roundings (`k b`, `t i`, the difference: |error| <= gamma(2) (|k b| +
        // |t i|)) and the residual below three more.
        let cb = ratios.iter().copied().fold(first_ratio, f64::max);
        let k = cb * (1.0 + t);
        for j in 0..along {
            let (b, i, q) = (at(ib, j), at(ii, j), at(iq, j));
            let z = if b.z != NOZ && i.z != NOZ {
                k * b.z - t * i.z
            } else {
                NOZ
            };
            let want = CPoint::new(k * b.x - t * i.x, k * b.y - t * i.y, z, k * b.w - t * i.w);
            if !same_point(q, want) {
                return Err(format!("column {j}: far row is not cb (1 + t) B - t I"));
            }
        }
        (k, gamma(5))
    } else {
        // Upstream's row: `P = B / w_B`, `P' = I / w_I` (one rounding each), `P + t (P
        // - P')` (three), times `w_I` (one), so each component is within gamma(6) (|k
        // b| + |t i|) of `c (1 + t) B - t I` for the exact ratio `c` of its column. The
        // rounded ratios are equal, so the exact ones are within gamma(2) of column 0's,
        // and `k` below is column 0's `c (1 + t)` within gamma(3) (`q_w = i_w` exactly):
        // gamma(5) on `|k b|`. The residual adds three roundings: gamma(14) in all.
        let (b, i, q) = (h(at(ib, 0))?, h(at(ii, 0))?, h(at(iq, 0))?);
        ((q[3] + t * i[3]) / b[3], gamma(14))
    };
    // `A = combine2(1 - d, Q, d, B)` with `d = (p - 1) / p` (start) or `1 / p` (end):
    // coefficients within gamma(2), two products and a sum, gamma(4) on `|b| + |q|`; the
    // check's coefficients again gamma(2) and its products, sum and difference three
    // more: gamma(9).
    let allow_a = gamma(9);
    for j in 0..along {
        let (b, i, a, q) = (h(at(ib, j))?, h(at(ii, j))?, h(at(ia, j))?, h(at(iq, j))?);
        for x in 0..4 {
            let r = q[x] + t * i[x] - k * b[x];
            let allow = allow_q * (k.abs() * b[x].abs() + t * i[x].abs() + q[x].abs());
            if r.abs() > allow {
                return Err(format!(
                    "column {j}: Q + t I - k B = {r:e} > {allow:e} (t {t}, k {k})"
                ));
            }
            let ra = a[x] - ((1.0 - 1.0 / pf) * b[x] + q[x] / pf);
            let allow = allow_a * (b[x].abs() + q[x].abs());
            if ra.abs() > allow {
                return Err(format!(
                    "column {j}: A off the ruled span by {ra:e} > {allow:e}"
                ));
            }
        }
    }
    Ok(SideCert::Ruled { t, k, mixed })
}

/// One G1R step's certificate plus the checks that the step keeps its input `s`'s knots
/// (other direction, and this direction's inner knots) and, for a ruled step, every
/// control row of `s` (so the step is `s` on `s`'s domain), returning the side's
/// finding. A knot-removed step moves the rows next to the side (by at most `MODEL_TOL`,
/// upstream's removal tolerance), so its rows are not compared.
pub(crate) fn certify_step(
    s: &NlSurface,
    e: &NlSurface,
    udir: bool,
    end: bool,
) -> Result<SideCert, String> {
    let kb = if udir { s.u_knots() } else { s.v_knots() };
    let side = if end {
        *kb.last().ok_or("no knots")?
    } else {
        *kb.first().ok_or("no knots")?
    };
    let cert = certify_side(e, udir, end, side)?;
    let p = if udir { s.degrees().0 } else { s.degrees().1 };
    // Both kinds: `side` is the input's clamped end knot (multiplicity `p + 1`), the
    // other direction's knots are unchanged, and the extension keeps the input's knots
    // on the input's side of `side` (the knot removal only lowers `side` itself).
    if kb.iter().filter(|&&u| u == side).count() != p + 1 {
        return Err(format!("side {side} is not the input's clamped end knot"));
    }
    let (ob, oe) = if udir {
        (s.v_knots(), e.v_knots())
    } else {
        (s.u_knots(), e.u_knots())
    };
    if ob != oe {
        return Err("other direction's knots changed".into());
    }
    let ke = if udir { e.u_knots() } else { e.v_knots() };
    let inner = |k: &[f64]| -> Vec<f64> {
        k.iter()
            .copied()
            .filter(|&u| if end { u < side } else { u > side })
            .collect()
    };
    if inner(kb) != inner(ke) {
        return Err("extension changed the input's inner knots".into());
    }
    if let SideCert::Ruled { .. } = cert {
        let (bn, en) = (s.cpts(), e.cpts());
        let off = if end { 0 } else { p };
        for (j, row) in bn.iter().enumerate() {
            for (i, b) in row.iter().enumerate() {
                let x = if udir {
                    en.get(j + off).and_then(|r| r.get(i))
                } else {
                    en.get(j).and_then(|r| r.get(i + off))
                };
                let x = x.ok_or("extension lost a row")?;
                if !same_point(*b, *x) {
                    return Err(format!("input row moved at {j},{i}"));
                }
            }
        }
    }
    Ok(cert)
}

/// `solidcore::nurbs::srf_adv::p1`'s private `extend_to` replayed step by step (same
/// step choice, `0.9` of the current domain at most), each G1R step certified by
/// [`certify_step`]. Returns the final surface and each step's finding.
pub(crate) fn extend_to_certified(
    sur: &NlSurface,
    t: f64,
    lo: f64,
    hi: f64,
    dir: solidcore::nurbs::srf_basic::Direction,
) -> Result<(NlSurface, Vec<SideCert>), String> {
    use solidcore::nurbs::crv_adv::p1::CurveEnd;
    use solidcore::nurbs::srf_adv::p3::{extend_by_param_dist, ExtendContinuity};
    use solidcore::nurbs::srf_basic::Direction;
    let udir = dir == Direction::U;
    let (mut lo, mut hi) = (lo, hi);
    let mut s = sur.clone();
    let mut certs = Vec::new();
    let mut step = |s: &mut NlSurface, tt: f64, end: CurveEnd| -> Result<(), String> {
        let e = extend_by_param_dist(s, tt, dir, end, ExtendContinuity::G1R)
            .map_err(|e| e.to_string())?;
        certs.push(certify_step(s, &e, udir, end == CurveEnd::End)?);
        *s = e;
        Ok(())
    };
    if t < lo {
        loop {
            let tt = if lo - t <= 0.9 * (hi - lo) {
                t
            } else {
                lo - (0.9 * (hi - lo))
            };
            step(&mut s, tt, CurveEnd::Start)?;
            lo = tt;
            if t >= lo {
                break;
            }
        }
    } else {
        loop {
            let tt = if t - hi <= 0.9 * (hi - lo) {
                t
            } else {
                hi + (0.9 * (hi - lo))
            };
            step(&mut s, tt, CurveEnd::End)?;
            hi = tt;
            if t <= hi {
                break;
            }
        }
    }
    Ok((s, certs))
}

/// `true` when two nets and knot vectors are bit for bit equal.
/// Bitwise equality of two control points.
fn same_point(x: CPoint, y: CPoint) -> bool {
    x.x.to_bits() == y.x.to_bits()
        && x.y.to_bits() == y.y.to_bits()
        && x.z.to_bits() == y.z.to_bits()
        && x.w.to_bits() == y.w.to_bits()
}

/// Bitwise equality of two surfaces (degrees, knots and every control point).
pub(crate) fn same_surface(a: &NlSurface, b: &NlSurface) -> bool {
    let (na, nb) = (a.cpts(), b.cpts());
    a.u_knots() == b.u_knots()
        && a.v_knots() == b.v_knots()
        && a.degrees() == b.degrees()
        && na.len() == nb.len()
        && na.iter().zip(&nb).all(|(ra, rb)| {
            ra.len() == rb.len() && ra.iter().zip(rb).all(|(&x, &y)| same_point(x, y))
        })
}

/// The extension a fresh `eval_pt_derivs_unbounded(sur, u, v, 0, G1R, &mut None)` builds
/// (as `evaluate_point_on_extended_surface` and the offset's `extend_toward` call it),
/// replayed with [`extend_to_certified`] in each direction it does not wrap and required
/// to be the evaluator's surface bit for bit. Returns the number of mixed-ratio steps.
pub(crate) fn certify_unbounded(sur: &NlSurface, u: f64, v: f64) -> Result<usize, String> {
    use solidcore::nurbs::srf_adv::p1::eval_pt_derivs_unbounded;
    use solidcore::nurbs::srf_adv::p3::ExtendContinuity;
    use solidcore::nurbs::srf_basic::Direction;
    let mut ext = None;
    eval_pt_derivs_unbounded(sur, u, v, 0, ExtendContinuity::G1R, &mut ext)
        .map_err(|e| e.to_string())?;
    let ext = ext.ok_or("no extension (the point is inside the domain)")?;
    let b = sur.parameter_bounds();
    let mut x = sur.clone();
    let mut mixed = 0;
    for (t, lo, hi, d) in [(u, b.ul, b.ur, Direction::U), (v, b.vb, b.vt, Direction::V)] {
        let wraps = matches!(
            (ext.closed, d),
            (Some(Direction::UV), _)
                | (Some(Direction::U), Direction::U)
                | (Some(Direction::V), Direction::V)
        );
        if (t < lo || t > hi) && !wraps {
            let (e, certs) = extend_to_certified(&x, t, lo, hi, d)?;
            mixed += certs
                .iter()
                .filter(|c| matches!(c, SideCert::Ruled { mixed: true, .. }))
                .count();
            x = e;
        }
    }
    if !same_surface(&x, &ext.surface) {
        return Err("replay differs from the evaluator's extension".into());
    }
    Ok(mixed)
}

/// The G1R extension of `s`'s NURB (through any offsets to their base) one domain size
/// past each side, the four steps in turn as an offset's extension grows it, each step
/// certified by [`certify_step`]. `None` when the base has no rational NURB (G1R keeps
/// upstream's row there); otherwise the number of mixed-ratio sides.
pub(crate) fn certify_surface_sides(
    k: &solidcore::kernel::Kernel,
    s: &solidcore::surface::surface::Surface,
) -> Result<Option<usize>, String> {
    use solidcore::nurbs::crv_adv::p1::CurveEnd;
    use solidcore::nurbs::srf_adv::p3::{extend_by_param_dist, ExtendContinuity};
    use solidcore::nurbs::srf_basic::Direction;
    use solidcore::surface::surface::SurfaceKind;
    let mut base = s.clone();
    while let SurfaceKind::OffsetSurface(o) = base.kind() {
        base = o.base_surface().clone();
    }
    let bn = match base.kind_with_nurb(k).map_err(|e| e.to_string())? {
        SurfaceKind::BSpline(b) => b.gw_nurb().cloned(),
        SurfaceKind::SurfOfRevolution(r) => r.base().gw_nurb().cloned(),
        SurfaceKind::SurfOfExtrusion(r) => r.base().gw_nurb().cloned(),
        _ => None,
    };
    let Some(bn) = bn.filter(NlSurface::is_rational) else {
        return Ok(None);
    };
    let d = base
        .natural_uv_domain_with_nurb(k)
        .map_err(|e| e.to_string())?;
    let (lo, hi, sz) = (d.min(), d.max(), d.size());
    let mut e = bn;
    let mut mixed = 0;
    for (par, dir, end) in [
        (lo.x - sz.x, Direction::U, CurveEnd::Start),
        (hi.x + sz.x, Direction::U, CurveEnd::End),
        (lo.y - sz.y, Direction::V, CurveEnd::Start),
        (hi.y + sz.y, Direction::V, CurveEnd::End),
    ] {
        let x = extend_by_param_dist(&e, par, dir, end, ExtendContinuity::G1R)
            .map_err(|e| e.to_string())?;
        if let SideCert::Ruled { mixed: true, .. } =
            certify_step(&e, &x, dir == Direction::U, end == CurveEnd::End)?
        {
            mixed += 1;
        }
        e = x;
    }
    Ok(Some(mixed))
}
