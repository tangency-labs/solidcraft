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
/// in every point, so the checks on it hold trivially; the weight is `1` in a
/// polynomial net, whose `w` is `NOW` in every point, which evaluates as that).
fn h(c: CPoint) -> [f64; 4] {
    let z = if c.z == NOZ { 0.0 } else { c.z };
    let w = if c.w == NOW { 1.0 } else { c.w };
    [c.x, c.y, z, w]
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
    // (every acceptance test below is written to accept, so a NaN anywhere fails it)
    for row in &net {
        for c in row {
            if !h(*c).iter().all(|x| x.is_finite()) {
                return Err(format!("non-finite control point {c:?}"));
            }
            if h(*c)[3].partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
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
    if !(t.is_finite() && t > 0.0) {
        return Err(format!("step {t}"));
    }
    let along = if udir { net[0].len() } else { net.len() };
    let at = |i: usize, j: usize| if udir { net[i][j] } else { net[j][i] };
    // The side's weight ratios, rounded as `mixed_ratio` rounds them (rows `B` and `I`
    // are the step's input rows, kept bitwise; `certify_step` checks that).
    let mut ratios = Vec::with_capacity(along);
    for j in 0..along {
        ratios.push(h(at(ii, j))[3] / h(at(ib, j))[3]);
    }
    let first_ratio = *ratios.first().ok_or("empty side")?;
    let mixed = ratios.iter().any(|&c| c != first_ratio);
    let (k, allow_q) = if mixed {
        // The library's own `k`, and `Q` replayed bitwise. Its `x, y, z` carry two
        // roundings (`k b`, `t i`, the difference: |error| <= gamma(2) (|k b| + |t i|)).
        // Its weight `w_B (cb + t (cb - c))`, with `c = w_I / w_B` rounded once (an error
        // of at most `t w_I u` once times `t w_B`), carries four more on terms whose
        // magnitudes are bounded by `k w_B + t w_I`: within gamma(5) of `k w_B - t w_I`.
        // The residual below adds three: gamma(8).
        let cb = ratios.iter().copied().fold(first_ratio, f64::max);
        let k = cb * (1.0 + t);
        for j in 0..along {
            let (b, i, q) = (at(ib, j), at(ii, j), at(iq, j));
            let z = if b.z != NOZ && i.z != NOZ {
                k * b.z - t * i.z
            } else {
                NOZ
            };
            let c = *ratios.get(j).ok_or("ratio")?;
            let w = b.w * (cb + t * (cb - c));
            let want = CPoint::new(k * b.x - t * i.x, k * b.y - t * i.y, z, w);
            if !same_point(q, want) {
                return Err(format!("column {j}: far row is not cb (1 + t) B - t I"));
            }
        }
        (k, gamma(8))
    } else {
        // Upstream's row: `P = B / w_B`, `P' = I / w_I` (one rounding each), `P + t (P
        // - P')` (three), times `w_I` (one), so each component is within gamma(6) (|k
        // b| + |t i|) of `c (1 + t) B - t I` for the exact ratio `c` of its column. The
        // rounded ratios are equal, so the exact ones are within gamma(2) of column 0's,
        // and `k` below is column 0's `c (1 + t)` within gamma(3) (`q_w = i_w` exactly):
        // gamma(5) on `|k b|`. The residual adds three roundings: gamma(14) in all.
        let (b, i, q) = (h(at(ib, 0)), h(at(ii, 0)), h(at(iq, 0)));
        ((q[3] + t * i[3]) / b[3], gamma(14))
    };
    // `A = combine2(1 - d, Q, d, B)` with `d = (p - 1) / p` (start) or `1 / p` (end):
    // coefficients within gamma(2), two products and a sum, gamma(4) on `|b| + |q|`; the
    // check's coefficients again gamma(2) and its products, sum and difference three
    // more: gamma(9).
    let allow_a = gamma(9);
    if !k.is_finite() {
        return Err(format!("k {k}"));
    }
    for j in 0..along {
        let (b, i, a, q) = (h(at(ib, j)), h(at(ii, j)), h(at(ia, j)), h(at(iq, j)));
        for x in 0..4 {
            let r = q[x] + t * i[x] - k * b[x];
            let allow = allow_q * (k.abs() * b[x].abs() + t * i[x].abs() + q[x].abs());
            // (an overflow makes both sides infinite, and `inf <= inf` would accept it)
            if !(r.is_finite()
                && allow.is_finite()
                && matches!(
                    r.abs().partial_cmp(&allow),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ))
            {
                return Err(format!(
                    "column {j}: Q + t I - k B = {r:e} > {allow:e} (t {t}, k {k})"
                ));
            }
            let ra = a[x] - ((1.0 - 1.0 / pf) * b[x] + q[x] / pf);
            let allow = allow_a * (b[x].abs() + q[x].abs());
            if !(ra.is_finite()
                && allow.is_finite()
                && matches!(
                    ra.abs().partial_cmp(&allow),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ))
            {
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
/// finding. A knot-removed step moves the rows next to the side, so it is checked by
/// [`certify_removed`] instead: within `MODEL_TOL` (G1R's removal tolerance) of `s` on
/// `s`'s domain, an enclosure.
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
    if let SideCert::KnotRemoved = cert {
        certify_removed(s, e, udir, end, side)?;
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

/// A knot-removed step `e` of `s` (the side's knot once less than the degree) agrees with
/// `s` on `s`'s domain to within `MODEL_TOL` (G1R's removal tolerance: `srf_adv::p3`'s
/// ruled extension removes the side's knot once, at `MODEL_TOL`), an enclosure. The
/// side's knot is inserted
/// back once (`insert_knot`, giving the ruled step's knots: `s`'s up to `side`, `side`
/// `p` times), so on `s`'s domain only the rows of `s`'s positions are live, with `s`'s
/// basis functions there (each span's basis reads `p` knots past it: `side` `p` times in
/// both). For two rational surfaces on one basis with homogeneous nets `H1` (the
/// reinserted one) and `H2` (`s`'s), positive weights, at every parameter
/// `S1 - S2 = sum_ij N_i M_j ((dX_ij - c_ij dw_ij) - (S2 - c_ij) dw_ij) / w1` for any
/// points `c_ij` (the `xyz` parts `X`, `sum N M = 1`): with `c_ij` `s`'s control point
/// `P_ij` and `R_ij` the largest `|P - P_ij|` over `s`'s control points within `p` rows
/// and `q` columns of it (where `N_i M_j` is nonzero, `S2` is in their convex hull),
/// `|S1 - S2| <= sum_i max N_i max_j (|dX_ij - P_ij dw_ij| + R_ij |dw_ij|) / w_min`,
/// `w_min` the least live weight of the reinserted net (a bound of `w1` below) and
/// `max N_i` (on the span) bounded above by [`basis_max_bound`], span by span of `s`'s
/// domain. The insertion's convex combinations
/// round each component by at most gamma(10) of the larger magnitude of that component in
/// the two points of `e` they combine (`mx` over the coordinates, `mw` the weight); the
/// exact reinsertion is the surface of `e`, so each point's term adds
/// `gamma(10) (sqrt 3 mx + (|P_ij| + R_ij) mw)` (the reinsertion's resolution, bounded
/// span by span like the terms but on its own) and its weight is lowered by
/// `gamma(10) mw` for `w_min`. The resolution is relative to the net's magnitude (a
/// net at 1e9 resolves no finer than about 1e-6, which the kernel's own removal
/// arithmetic on that net shares), so the step is accepted when the rest of the bound is
/// within `MODEL_TOL`: `|S1 - S2|` is then within `MODEL_TOL` plus that resolution (a
/// rounding of the net's magnitude: its largest homogeneous coordinate, or weight times
/// its largest point, over its least live weight, which a rational net's small weights
/// enlarge; by construction at most 4 gamma(10) (1 + gamma) of it, about 4.4e-15, and
/// checked against 1e-13 of it as a consistency check of the bookkeeping). Each term's
/// own evaluation (with `P` rounded from its homogeneous form) is widened by gamma(8) of
/// its inputs' magnitudes (`R` the largest `|P|`), and the sum by gamma(n + 4), `n` the
/// number of rows. The bound is taken span by span ([`removed_bound`]: the live rows and
/// least live weight of each span of `s`'s domain).
fn certify_removed(
    s: &NlSurface,
    e: &NlSurface,
    udir: bool,
    end: bool,
    side: f64,
) -> Result<(), String> {
    use solidcore::nurbs::math::MODEL_TOL;
    use solidcore::nurbs::srf_basic::Direction;
    use solidcore::nurbs::srf_tool::p1::insert_knot;
    let dir = if udir { Direction::U } else { Direction::V };
    let r = insert_knot(e, side, 1, dir).map_err(|x| format!("reinsertion: {x}"))?;
    let p = if udir { s.degrees().0 } else { s.degrees().1 };
    let (kr, ks) = if udir {
        (r.u_knots(), s.u_knots())
    } else {
        (r.v_knots(), s.v_knots())
    };
    let mine = |k: &[f64]| -> Vec<f64> {
        k.iter()
            .copied()
            .filter(|&u| if end { u <= side } else { u >= side })
            .collect()
    };
    // the reinserted knots on `s`'s side: `s`'s, with `side` `p` times (not `p + 1`)
    let mut want = mine(ks);
    if end {
        want.pop();
    } else if !want.is_empty() {
        want.remove(0);
    }
    if mine(kr) != want {
        return Err("reinserted knots are not the ruled step's".into());
    }
    let off = if end { 0 } else { p };
    // (G1R's removal tolerance: its ruled extension removes the side's knot once, at
    // `MODEL_TOL`), past the resolution of the reinsertion at the net's magnitude
    let (bound, resolution) = removed_bound(s, &r, udir, off, &e.cpts())?;
    if bound <= MODEL_TOL {
        return Ok(());
    }
    Err(format!(
        "knot-removed step off its input by up to {bound:e} > {MODEL_TOL:e} \
         (past the reinsertion's resolution {resolution:e})"
    ))
}

/// The bound of `|S1 - S2|` on `s`'s domain (see [`certify_removed`]) for `s` and `r` on
/// one basis there, `s`'s row `j` (in the removal direction) `r`'s row `j + off`: the
/// largest over the spans of that domain of the smaller of two bounds of
/// `sum_i N_i d_i` (the live rows' `d_i`, each its largest term over the live columns):
/// `sum_i max N_i d_i`, and `max_i d_i` (the `N_i` sum to one), over the span's least
/// live weight. Rounding: each reinserted point's own convex combination (`en`, `e`'s
/// net, `r` its one reinsertion: gamma(10) of its two inputs).
fn removed_bound(
    s: &NlSurface,
    r: &NlSurface,
    udir: bool,
    off: usize,
    en: &[Vec<CPoint>],
) -> Result<(f64, f64), String> {
    let (sn, rn) = (s.cpts(), r.cpts());
    let p = if udir { s.degrees().0 } else { s.degrees().1 };
    let kr = if udir { r.u_knots() } else { r.v_knots() };
    let ko = if udir { s.v_knots() } else { s.u_knots() };
    let mut big_r = 0.0_f64;
    for c in sn.iter().flatten() {
        big_r = big_r.max(c.to_euclid().length());
    }
    let q = if udir { s.degrees().1 } else { s.degrees().0 };
    let (pr, pc) = if udir { (p, q) } else { (q, p) };
    let eu: Vec<Vec<solidcore::math::Vec3>> = sn
        .iter()
        .map(|r| r.iter().map(|c| c.to_euclid()).collect())
        .collect();
    // `d` and the lowered weight by (row in the removal direction of `r`, column)
    let mut cells: Vec<(usize, usize, f64, f64, f64)> = Vec::new();
    // the largest homogeneous coordinate and weight of `e`'s rows the reinsertion combines
    let (mut mx_hi, mut mw_hi) = (0.0_f64, 0.0_f64);
    for (j, row) in sn.iter().enumerate() {
        for (i, b) in row.iter().enumerate() {
            let (at, other, x) = if udir {
                (j + off, i, rn.get(j + off).and_then(|r| r.get(i)))
            } else {
                (i + off, j, rn.get(j).and_then(|r| r.get(i + off)))
            };
            let x = *x.ok_or("reinserted net lost a row")?;
            let (hb, hx) = (h(*b), h(x));
            let c = *eu.get(j).and_then(|r| r.get(i)).ok_or("net")?;
            let dw = hx[3] - hb[3];
            let rr = [
                hx[0] - hb[0] - c.x * dw,
                hx[1] - hb[1] - c.y * dw,
                hx[2] - hb[2] - c.z * dw,
            ];
            let mut rij = 0.0_f64;
            for jj in j.saturating_sub(pr)..=j + pr {
                for ii in i.saturating_sub(pc)..=i + pc {
                    if let Some(o) = eu.get(jj).and_then(|r| r.get(ii)) {
                        rij = rij.max((*o - c).length());
                    }
                }
            }
            let dx = [hx[0] - hb[0], hx[1] - hb[1], hx[2] - hb[2]];
            let mags = (dx[0] * dx[0] + dx[1] * dx[1] + dx[2] * dx[2]).sqrt()
                + (c.length() + rij + big_r) * dw.abs();
            // the insertion's rounding: each reinserted point is a convex combination
            // `a y1 + (1 - a) y2` of `e`'s two points at `at - 1` and `at` (or a copy of
            // one); `a` is a quotient of knot differences (within gamma(3) relatively),
            // `1 - a` is formed from it (within gamma(4)), and the products and the sum
            // add gamma(2), so each component is within gamma(10) of the larger of its
            // two inputs' magnitudes: `mx` for the coordinates, `mw` for the weight
            let (mut mx, mut mw) = (0.0_f64, 0.0_f64);
            for a in at.saturating_sub(1)..=at {
                let y = if udir {
                    en.get(a).and_then(|r| r.get(i))
                } else {
                    en.get(j).and_then(|r| r.get(a))
                };
                if let Some(y) = y {
                    let z = h(*y);
                    mx = mx.max(z[0].abs()).max(z[1].abs()).max(z[2].abs());
                    mw = mw.max(z[3].abs());
                }
            }
            let (ix, iw) = (gamma(10) * mx, gamma(10) * mw);
            let di = (rr[0] * rr[0] + rr[1] * rr[1] + rr[2] * rr[2]).sqrt()
                + rij * dw.abs()
                + gamma(8) * (mags + big_r * dw.abs());
            let ri = 3.0_f64.sqrt() * ix + (c.length() + rij) * iw;
            (mx_hi, mw_hi) = (mx_hi.max(mx), mw_hi.max(mw));
            cells.push((at, other, di, ri, hx[3] - iw));
        }
    }
    let n_rows = if udir {
        sn.len()
    } else {
        sn.first().map_or(0, Vec::len)
    };
    let (mut worst, mut worst_r) = (0.0_f64, 0.0_f64);
    let mut spans = 0usize;
    // spans `k` of `r` (removal direction) whose live rows `k - p ..= k` are all `s`'s,
    // by spans `l` of the other direction (live columns `l - q ..= l`)
    for k in p..kr.len().saturating_sub(p + 1) {
        let (a, b) = (*kr.get(k).ok_or("knot")?, *kr.get(k + 1).ok_or("knot")?);
        if a.partial_cmp(&b) != Some(std::cmp::Ordering::Less) || k < p + off || k >= off + n_rows {
            continue;
        }
        let maxn: Vec<f64> = (k - p..=k)
            .map(|i| basis_max_bound(kr, p, i, (a, b)))
            .collect::<Result<_, _>>()?;
        for l in q..ko.len().saturating_sub(q + 1) {
            let (c0, c1) = (*ko.get(l).ok_or("knot")?, *ko.get(l + 1).ok_or("knot")?);
            if c0.partial_cmp(&c1) != Some(std::cmp::Ordering::Less) {
                continue;
            }
            let live = |x: &&(usize, usize, f64, f64, f64)| {
                x.0 + p >= k && x.0 <= k && x.1 + q >= l && x.1 <= l
            };
            let mut w_min = f64::INFINITY;
            let (mut by_max, mut by_sum) = (0.0_f64, 0.0_f64);
            let (mut r_max, mut r_sum) = (0.0_f64, 0.0_f64);
            for (n, row) in (k - p..=k).enumerate() {
                let (d, rd) = cells
                    .iter()
                    .filter(live)
                    .filter(|x| x.0 == row)
                    .fold((0.0_f64, 0.0_f64), |m, x| (m.0.max(x.2), m.1.max(x.3)));
                let nb = maxn.get(n).copied().unwrap_or(1.0);
                by_sum += d * nb;
                by_max = by_max.max(d);
                r_sum += rd * nb;
                r_max = r_max.max(rd);
            }
            for x in cells.iter().filter(live) {
                w_min = w_min.min(x.4);
            }
            if w_min.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return Err("a non-positive live weight".into());
            }
            spans += 1;
            worst = worst.max(by_sum.min(by_max) / w_min * (1.0 + gamma(p + 6)));
            worst_r = worst_r.max(r_sum.min(r_max) / w_min * (1.0 + gamma(p + 6)));
        }
    }
    // every span of `s`'s domain, in both directions, bounded
    let distinct = |k: &[f64]| {
        k.windows(2)
            .filter(|w| matches!(w, [a, b] if a < b))
            .count()
    };
    let ks = if udir { s.u_knots() } else { s.v_knots() };
    if spans != distinct(ks) * distinct(ko) {
        return Err(format!(
            "{spans} spans bounded of the input's {}",
            distinct(ks) * distinct(ko)
        ));
    }
    if spans == 0 || !worst.is_finite() || !worst_r.is_finite() {
        return Err("no span of the input's domain bounded".into());
    }
    // (rounding relative to the net's magnitude, not a deviation: its largest homogeneous
    // coordinate, or weight times its largest point, over its least live weight, which
    // bound every resolution term; a consistency check, the terms being at most
    // 4 gamma(10) (1 + gamma) of it by construction)
    let w_lo = cells.iter().fold(f64::INFINITY, |m, x| m.min(x.4));
    let scale = ((3.0_f64.sqrt() * mx_hi).max(mw_hi * big_r) / w_lo).max(big_r);
    if worst_r > 1e-13 * scale.max(1.0) {
        return Err(format!(
            "the reinsertion's resolution {worst_r:e} is not rounding of a net at {scale:e}"
        ));
    }
    Ok((worst, worst_r))
}

/// An upper bound of the B-spline basis function `N_i` of degree `p` on `knots` over the
/// interval `on`: its coefficients (the unit vector `e_i`) refined by inserting 31 equally
/// spaced knots into each nonzero span of its support, the largest of those whose refined
/// functions are nonzero somewhere on `on` (on `on`, `N_i` is the sum of those functions
/// times their coefficients, the functions non-negative and summing to at most one, so
/// their largest coefficient bounds it; the refined coefficients are `N_i`'s on the finer
/// knots). Each insertion forms convex
/// combinations of values in `[0, 1]`, rounding each by at most gamma(3); the bound adds
/// that for every insertion.
fn basis_max_bound(knots: &[f64], p: usize, i: usize, on: (f64, f64)) -> Result<f64, String> {
    let lo = *knots.get(i).ok_or("basis index")?;
    let hi = *knots.get(i + p + 1).ok_or("basis index")?;
    let mut k: Vec<f64> = knots.to_vec();
    let mut c: Vec<f64> = vec![0.0; knots.len().saturating_sub(p + 1)];
    *c.get_mut(i).ok_or("basis index")? = 1.0;
    let spans: Vec<(f64, f64)> = knots
        .windows(2)
        .filter_map(|w| match w {
            [a, b] if *a >= lo && *b <= hi && a < b => Some((*a, *b)),
            _ => None,
        })
        .collect();
    let mut inserted = 0usize;
    for (a, b) in spans {
        for step in 1..32 {
            let t = a + (b - a) * (f64::from(step) / 32.0);
            if !(t > a && t < b) {
                continue;
            }
            // Boehm: the span `kk` with k[kk] <= t < k[kk + 1]
            let kk = k.iter().rposition(|&x| x <= t).ok_or("insertion span")?;
            let mut q = Vec::with_capacity(c.len() + 1);
            for j in 0..=c.len() {
                let v = if j + p <= kk {
                    *c.get(j).ok_or("coefficient")?
                } else if j > kk {
                    *c.get(j - 1).ok_or("coefficient")?
                } else {
                    let (kj, kjp) = (*k.get(j).ok_or("knot")?, *k.get(j + p).ok_or("knot")?);
                    let al = (t - kj) / (kjp - kj);
                    let (c0, c1) = (
                        *c.get(j - 1).ok_or("coefficient")?,
                        *c.get(j).ok_or("coefficient")?,
                    );
                    (1.0 - al) * c0 + al * c1
                };
                q.push(v);
            }
            k.insert(kk + 1, t);
            c = q;
            inserted += 1;
        }
    }
    // (the refined coefficients whose functions are nonzero somewhere on `on`)
    let mut top = 0.0_f64;
    for (j, &x) in c.iter().enumerate() {
        let (a, b) = (*k.get(j).ok_or("knot")?, *k.get(j + p + 1).ok_or("knot")?);
        if a < on.1 && b > on.0 {
            top = top.max(x);
        }
    }
    Ok((top + gamma(3) * inserted as f64).min(1.0))
}

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
    replay_unbounded(sur, u, v).map(|(_, mixed)| mixed)
}

/// [`certify_unbounded`], also returning the certified extension.
fn replay_unbounded(sur: &NlSurface, u: f64, v: f64) -> Result<(NlSurface, usize), String> {
    use solidcore::nurbs::srf_adv::p1::eval_pt_derivs_unbounded;
    use solidcore::nurbs::srf_adv::p3::ExtendContinuity;
    use solidcore::nurbs::srf_basic::Direction;
    let mut ext = None;
    // (as the offset's two tries, which keep the extension between them: a first try
    // that fails after making it, as on a closedness check that cannot measure an angle,
    // leaves it to the second)
    if let Err(e) = eval_pt_derivs_unbounded(sur, u, v, 0, ExtendContinuity::G1R, &mut ext) {
        if ext.is_none() {
            return Err(e.to_string());
        }
        eval_pt_derivs_unbounded(sur, u, v, 0, ExtendContinuity::G1R, &mut ext)
            .map_err(|e| e.to_string())?;
    }
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
    Ok((x, mixed))
}

/// The NURB an offset extends (`nurb_of` in `solidcore::surface::offset_surface`): that of
/// its base when the base is of a B-spline kind, else `None` (the offset never extends).
fn offset_base_nurb(
    k: &solidcore::kernel::Kernel,
    o: &solidcore::surface::offset_surface::OffsetSurface,
) -> Result<Option<NlSurface>, String> {
    use solidcore::surface::surface::SurfaceKind;
    // (the surface the offset evaluates and extends: its base, or a nested offset's
    // base, as `nested_base`)
    let base = match o.base_surface().kind() {
        SurfaceKind::OffsetSurface(b) => b.base_surface(),
        _ => o.base_surface(),
    };
    let n = match base
        .kind_with_nurb(k)
        .map_err(|e| format!("the base's NURB ({:?}): {e}", base.type_code()))?
    {
        SurfaceKind::BSpline(b) => b.gw_nurb().cloned(),
        SurfaceKind::Plane(p) => p
            .base_with_nurb()
            .map_err(|e| e.to_string())?
            .gw_nurb()
            .cloned(),
        SurfaceKind::SurfOfRevolution(s) => s.base().gw_nurb().cloned(),
        SurfaceKind::SurfOfExtrusion(s) => s.base().gw_nurb().cloned(),
        SurfaceKind::Cone(s) => s.rev().base().gw_nurb().cloned(),
        SurfaceKind::Sphere(s) => s.base().base().gw_nurb().cloned(),
        SurfaceKind::Torus(s) => s.base().base().gw_nurb().cloned(),
        _ => None,
    };
    Ok(n)
}

/// The NURB of an offset's current extension (`GetExtendedBaseSurface`), if any.
fn offset_extension_nurb(
    o: &solidcore::surface::offset_surface::OffsetSurface,
) -> Result<Option<NlSurface>, String> {
    use solidcore::surface::surface::SurfaceKind;
    o.with_extended_base_surface(
        |e| match e.map(solidcore::surface::surface::Surface::kind) {
            None => Ok(None),
            Some(SurfaceKind::BSpline(b)) => Ok(b.gw_nurb().cloned()),
            Some(_) => Err("the extension is not a B-spline surface".to_string()),
        },
    )
}

/// One change of an offset's extension, from `prev` (its base's NURB, or the extension
/// it held before) to `next`, replayed: `next` must be, bit for bit, the extension a
/// fresh G1R evaluation of `prev` builds toward the corner `next`'s domain grew to (the
/// offset's `extend_surface` evaluates one point a domain size past the one asked for,
/// and `extend_to` ends each direction exactly at that point's parameter), every step
/// certified by [`certify_step`]; or, when several evaluations grew it between two
/// observations, a chain of them ([`eval_chain`]) or of single certified steps
/// ([`chain_search`]) that is `next` bit for bit. Returns the number of mixed-ratio
/// steps.
fn certify_growth(prev: &NlSurface, next: &NlSurface) -> Result<usize, String> {
    let (b, n) = (prev.parameter_bounds(), next.parameter_bounds());
    // (whether any side grew)
    let grew = n.ul < b.ul || n.ur > b.ur || n.vb < b.vb || n.vt > b.vt;
    let pick = |lo: f64, hi: f64, nlo: f64, nhi: f64| match (nlo < lo, nhi > hi) {
        (true, false) => Some(nlo),
        (false, true) => Some(nhi),
        (false, false) => Some(0.5 * (lo + hi)),
        (true, true) => None,
    };
    // one evaluation's extension: toward the new corner (or side)
    let mut first = None;
    if let (Some(u), Some(v)) = (pick(b.ul, b.ur, n.ul, n.ur), pick(b.vb, b.vt, n.vb, n.vt)) {
        match replay_unbounded(prev, u, v) {
            Ok((x, mixed)) if same_surface(&x, next) => return Ok(mixed),
            Ok(_) => {
                first = Some(format!(
                    "the replay toward ({u}, {v}) is not the offset's extension"
                ));
            }
            Err(e) => first = Some(format!("the replay toward ({u}, {v}): {e}")),
        }
    }
    // `extend_from_base`'s own two evaluations, when a direction grows at both ends:
    // toward the low corner (a side that does not grow at `prev`'s middle), then from that
    // extension toward the high corner
    if grew && first.is_none() {
        let mid = |lo: f64, hi: f64| 0.5 * (lo + hi);
        let low = (
            if n.ul < b.ul { n.ul } else { mid(b.ul, b.ur) },
            if n.vb < b.vb { n.vb } else { mid(b.vb, b.vt) },
        );
        let high = (
            if n.ur > b.ur { n.ur } else { mid(b.ul, b.ur) },
            if n.vt > b.vt { n.vt } else { mid(b.vb, b.vt) },
        );
        let inside = |x: &NlSurface, (u, v): (f64, f64)| {
            let d = x.parameter_bounds();
            d.ul <= u && u <= d.ur && d.vb <= v && v <= d.vt
        };
        let two = (|| -> Result<(NlSurface, usize), String> {
            let (x, m0) = if inside(prev, low) {
                (prev.clone(), 0)
            } else {
                replay_unbounded(prev, low.0, low.1)
                    .map_err(|e| format!("toward the low corner {low:?}: {e}"))?
            };
            if inside(&x, high) {
                return Ok((x, m0));
            }
            let (y, m1) = replay_unbounded(&x, high.0, high.1)
                .map_err(|e| format!("toward the high corner {high:?}: {e}"))?;
            Ok((y, m0 + m1))
        })();
        match two {
            Ok((x, mixed)) if same_surface(&x, next) => return Ok(mixed),
            Ok(_) => first = Some("the two-corner replay is not the offset's extension".into()),
            Err(e) => first = Some(format!("the two-corner replay: {e}")),
        }
    }
    // several evaluations' extensions between two observations: a chain of certified
    // steps, searched for (any chain of certified G1R steps from `prev` that is `next`
    // bitwise proves `next` a G1R extension of it, however the evaluations ran)
    if grew {
        let mut budget = 2_000_usize;
        if let Some(mixed) = eval_chain(prev, next, 0, &mut budget) {
            return Ok(mixed);
        }
        let mut budget = 20_000_usize;
        if let Some(mixed) = chain_search(prev, next, 0, &mut budget)? {
            return Ok(mixed);
        }
    }
    Err(first
        .unwrap_or_else(|| "no chain of certified evaluations or steps replays the growth".into()))
}

/// A chain of at most 4 evaluations from `x` whose extension is `next` bitwise (its
/// mixed-ratio steps), searched depth first: each evaluation is replayed by
/// [`replay_unbounded`] (every step certified, the result the evaluator's extension
/// bit for bit), toward a corner whose parameter in each direction is either inside
/// `x`'s domain or one of `next`'s knots past it (an evaluation's extension ends at its
/// parameter, which stays a knot). `None` when none is found within `budget`
/// evaluations.
fn eval_chain(x: &NlSurface, next: &NlSurface, depth: usize, budget: &mut usize) -> Option<usize> {
    if same_surface(x, next) {
        return Some(0);
    }
    if depth >= 4 {
        return None;
    }
    let b = x.parameter_bounds();
    let targets = |lo: f64, hi: f64, knots: &[f64]| {
        let mut t = vec![0.5 * (lo + hi)];
        for &k in knots {
            if (k < lo || k > hi) && !t.contains(&k) {
                t.push(k);
            }
        }
        t
    };
    let us = targets(b.ul, b.ur, next.u_knots());
    let vs = targets(b.vb, b.vt, next.v_knots());
    for (i, &u) in us.iter().enumerate() {
        for (j, &v) in vs.iter().enumerate() {
            if i == 0 && j == 0 {
                continue;
            }
            if *budget == 0 {
                return None;
            }
            *budget -= 1;
            let Ok((e, here)) = replay_unbounded(x, u, v) else {
                continue;
            };
            if let Some(m) = eval_chain(&e, next, depth + 1, budget) {
                return Some(m + here);
            }
        }
    }
    None
}

/// A chain of certified G1R steps from `x` that is `next` bitwise (its mixed-ratio
/// sides), searched depth first over the order of the sides: each step grows one side
/// of `x` that `next` passes, to `next`'s nearest knot past it (every step's bound
/// stays a knot unless knot removal drops it; then the `0.9`-domain step or `next`'s
/// bound). `None` when no chain of at most 8 steps (within `budget` steps
/// tried) is.
fn chain_search(
    x: &NlSurface,
    next: &NlSurface,
    depth: usize,
    budget: &mut usize,
) -> Result<Option<usize>, String> {
    use solidcore::nurbs::crv_adv::p1::CurveEnd;
    use solidcore::nurbs::srf_adv::p3::{extend_by_param_dist, ExtendContinuity};
    use solidcore::nurbs::srf_basic::Direction;
    if same_surface(x, next) {
        return Ok(Some(0));
    }
    if depth >= 8 {
        return Ok(None);
    }
    let (b, n) = (x.parameter_bounds(), next.parameter_bounds());
    for (dir, lo, hi, nlo, nhi, nk) in [
        (Direction::U, b.ul, b.ur, n.ul, n.ur, next.u_knots()),
        (Direction::V, b.vb, b.vt, n.vb, n.vt, next.v_knots()),
    ] {
        for (end, grown) in [(CurveEnd::Start, nlo < lo), (CurveEnd::End, nhi > hi)] {
            if !grown {
                continue;
            }
            let start = end == CurveEnd::Start;
            // (each step ends at a bound that stays a knot of every later extension, so
            // the next step of this side ends at `next`'s nearest knot past `x`'s bound)
            let beyond = |t: &&f64| if start { **t < lo } else { **t > hi };
            let near = if start {
                nk.iter().rev().find(beyond)
            } else {
                nk.iter().find(beyond)
            };
            // (or, where knot removal dropped that bound, the `0.9`-domain step
            // `extend_to` takes toward a farther target, or `next`'s own bound)
            let (step, past) = if start {
                (lo - 0.9 * (hi - lo), nlo)
            } else {
                (hi + 0.9 * (hi - lo), nhi)
            };
            let inside = if start { step > past } else { step < past };
            let mut cands: Vec<f64> = near.copied().into_iter().collect();
            for t in [inside.then_some(step), Some(past)].into_iter().flatten() {
                if !cands.contains(&t) {
                    cands.push(t);
                }
            }
            cands.sort_by(|a, b| {
                let (da, db) = if start {
                    (lo - a, lo - b)
                } else {
                    (a - hi, b - hi)
                };
                da.total_cmp(&db)
            });
            for tt in cands {
                if *budget == 0 {
                    return Ok(None);
                }
                *budget -= 1;
                let Ok(e) = extend_by_param_dist(x, tt, dir, end, ExtendContinuity::G1R) else {
                    continue;
                };
                let Ok(cert) = certify_step(x, &e, dir == Direction::U, !start) else {
                    continue;
                };
                if let Some(m) = chain_search(&e, next, depth + 1, budget)? {
                    let here = usize::from(matches!(cert, SideCert::Ruled { mixed: true, .. }));
                    return Ok(Some(m + here));
                }
            }
        }
    }
    Ok(None)
}

/// Edits the extension surface the offset makes of `s`
/// (`BSplineSurface::from_gw_nurb`, as `extend_surface`) by `edit` (the step's own
/// library edit, as the offset applies it to its extension); its NURB, or `None` when
/// the edit fails or leaves no B-spline surface.
fn edited(s: &NlSurface, edit: &Edit<'_>) -> Option<NlSurface> {
    use solidcore::nurbs::b_spline_surface::BSplineSurface;
    use solidcore::surface::surface::{Surface, SurfaceKind};
    let mut e = Surface::from(BSplineSurface::from_gw_nurb(s));
    edit(&mut e).ok()?;
    match e.kind() {
        SurfaceKind::BSpline(b) => b.gw_nurb().cloned(),
        _ => None,
    }
}

/// A step's library edit of a surface (`SwapUV`, `Reverse`, `Transform`,
/// `TrimWithDomain`), which an offset applies to its extension too.
pub(crate) type Edit<'a> =
    dyn Fn(&mut solidcore::surface::surface::Surface) -> solidcore::Result<()> + 'a;

/// `s` with its parameters swapped (`SwapUV`): degrees, knots and net transposed.
fn transposed(s: &NlSurface) -> Result<NlSurface, String> {
    let net = s.cpts();
    let cols = net.first().map_or(0, Vec::len);
    let t: Vec<Vec<CPoint>> = (0..cols)
        .map(|j| net.iter().filter_map(|r| r.get(j).copied()).collect())
        .collect();
    let (p, q) = s.degrees();
    NlSurface::new(t, q, p, s.v_knots().to_vec(), s.u_knots().to_vec()).map_err(|e| e.to_string())
}

/// The extensions an offset (and each offset nested in its base) holds, observed after
/// each operation of a real case and each certified on change by [`certify_growth`]: from
/// the extension seen before, else (a new chain) from the base's NURB.
#[derive(Clone, Default)]
pub(crate) struct ExtensionWatch {
    /// The last extension seen at each nesting level (`0` the outermost offset).
    seen: Vec<Option<NlSurface>>,
    /// Changes certified.
    pub(crate) growths: usize,
    /// Mixed-ratio steps among them (the new row).
    pub(crate) mixed: usize,
    /// Changes certified as a base extended as itself (`crate::as_itself_cert`).
    pub(crate) as_itself: usize,
}

impl ExtensionWatch {
    /// Observes `o` and every offset nested in its base.
    pub(crate) fn observe(
        &mut self,
        k: &solidcore::kernel::Kernel,
        o: &solidcore::surface::offset_surface::OffsetSurface,
    ) -> Result<(), String> {
        self.observe_after(k, o, None)
    }

    /// [`ExtensionWatch::observe`] after a step whose library edit `edit` the offset
    /// applied to each extension it holds (and each nested offset to its own).
    pub(crate) fn observe_after(
        &mut self,
        k: &solidcore::kernel::Kernel,
        o: &solidcore::surface::offset_surface::OffsetSurface,
        edit: Option<&Edit<'_>>,
    ) -> Result<(), String> {
        use solidcore::surface::surface::SurfaceKind;
        let mut level = 0;
        let mut cur = Some(o);
        while let Some(o) = cur {
            if self.seen.len() <= level {
                self.seen.push(None);
            }
            let next = offset_extension_nurb(o)?;
            let prev = self.seen.get(level).cloned().flatten();
            // (a swap of the offset's parameters transposes its extension; an edit step
            // (`edit`: swap, reverse, transform, trim) applies the same library edit to
            // it, replayed here on the last extension: the same extension edited, no
            // growth)
            let changed = match (&prev, &next) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(p), Some(n)) => {
                    !same_surface(p, n)
                        && !same_surface(&transposed(p)?, n)
                        && !edit
                            .and_then(|f| edited(p, f))
                            .is_some_and(|t| same_surface(&t, n))
                }
            };
            if let (true, Some(n)) = (changed, &next) {
                // (from the last extension, or its transpose after a swap)
                // (from the last extension, or its transpose after a swap: grown as it is
                // or in the swapped parameters)
                let from_prev = prev.as_ref().map(|p| {
                    certify_growth(p, n).or_else(|e| {
                        let tp = transposed(p)?;
                        certify_growth(&tp, n)
                            .or_else(|_| certify_growth(p, &transposed(n)?))
                            .map_err(|_| e)
                    })
                });
                let mixed = match from_prev {
                    Some(Ok(m)) => m,
                    other => {
                        let base =
                            offset_base_nurb(k, o)?.ok_or("an extension of a base with no NURB")?;
                        // (the base as it is, or in swapped parameters: the same surface)
                        certify_growth(&base, n)
                            .or_else(|e| {
                                let tb = transposed(&base)?;
                                certify_growth(&tb, n)
                                    .or_else(|_| certify_growth(&base, &transposed(n)?))
                                    .map_err(|_| e)
                            })
                            .or_else(|e| {
                                // (an offset's base extended as itself: no G1R replay,
                                // certified from its own net, `crate::as_itself_cert`)
                                crate::as_itself_cert::certify_as_itself(k, o.base_surface(), n)
                                    .or_else(|_| {
                                        crate::as_itself_cert::certify_as_itself(
                                            k,
                                            o.base_surface(),
                                            &transposed(n)?,
                                        )
                                    })
                                    .map(|_| {
                                        self.as_itself += 1;
                                        0
                                    })
                                    .map_err(|a| format!("{e}; as itself: {a}"))
                            })
                            .map_err(|e| match other {
                                Some(Err(p)) => {
                                    format!("from the last extension: {p}; from the base: {e}")
                                }
                                _ => e,
                            })?
                    }
                };
                self.growths += 1;
                self.mixed += mixed;
            }
            if let Some(slot) = self.seen.get_mut(level) {
                *slot = next;
            }
            cur = match o.base_surface().kind() {
                SurfaceKind::OffsetSurface(b) => Some(b),
                _ => None,
            };
            level += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::nurbs::crv_adv::p1::CurveEnd;
    use solidcore::nurbs::srf_adv::p3::{extend_by_param_dist, ExtendContinuity};
    use solidcore::nurbs::srf_basic::Direction;

    /// A rational degree-2 x degree-1 net with knots `[0,0,0,1,1,2,2,2]` in u (a
    /// ruled-like layout whose side `1` has multiplicity `p`), weights 1, `z` 0.
    fn net_with(x: impl Fn(usize) -> f64) -> NlSurface {
        let net = (0..5)
            .map(|i| {
                (0..2)
                    .map(|j| CPoint::new(x(i), f64::from(j), 0.0, 1.0))
                    .collect()
            })
            .collect();
        NlSurface::new(
            net,
            2,
            1,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap()
    }

    /// #21383 review: a NaN coordinate fails the certificate (every comparison is
    /// written to accept, and every coordinate must be finite), where `r.abs() > allow`
    /// let a NaN residual through.
    #[test]
    fn a_nan_coordinate_fails_the_side_certificate() {
        let good = net_with(|i| i as f64);
        assert!(certify_side(&good, true, true, 1.0).is_ok());
        let bad = net_with(|i| if i == 4 { f64::NAN } else { i as f64 });
        let e = certify_side(&bad, true, true, 1.0).unwrap_err();
        assert!(e.contains("non-finite"), "{e}");
    }

    /// #21383 review: an infinite coordinate fails the certificate, and so does a finite
    /// net whose residual overflows (`k B` past `f64::MAX`), where `inf <= inf` let both
    /// the residual and its allowance through.
    #[test]
    fn an_infinite_or_overflowing_residual_fails_the_side_certificate() {
        let inf = net_with(|i| if i == 4 { f64::INFINITY } else { i as f64 });
        let e = certify_side(&inf, true, true, 1.0).unwrap_err();
        assert!(e.contains("non-finite"), "{e}");
        let huge = net_with(|i| i as f64 * 4.0e307);
        assert!(huge.cpts().iter().flatten().all(|c| c.x.is_finite()));
        let e = certify_side(&huge, true, true, 1.0).unwrap_err();
        assert!(e.contains("Q + t I - k B"), "{e}");
    }

    /// A planar degree-2 surface with unit weights and a linear parametrisation, whose
    /// G1R step continues it linearly, so upstream's knot removal succeeds.
    fn plane() -> NlSurface {
        let net = (0..3)
            .map(|i| {
                (0..3)
                    .map(|j| CPoint::new(0.5 * f64::from(i), 0.5 * f64::from(j), 0.0, 1.0))
                    .collect()
            })
            .collect();
        NlSurface::new(
            net,
            2,
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap()
    }

    /// #21383 review: a knot-removed step is checked against its input (an enclosure of
    /// the distance on the input's domain), so the real step certifies and the same step
    /// translated by 1 does not, where the knot-removed side skipped every row check.
    #[test]
    fn a_translated_knot_removed_step_fails() {
        let s = plane();
        let e = extend_by_param_dist(&s, 1.5, Direction::U, CurveEnd::End, ExtendContinuity::G1R)
            .unwrap();
        let cert = certify_step(&s, &e, true, true).unwrap();
        assert!(
            matches!(cert, SideCert::KnotRemoved),
            "not a knot-removed step"
        );
        let moved: Vec<Vec<CPoint>> = e
            .cpts()
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| CPoint::new(c.x + c.w, c.y, c.z, c.w))
                    .collect()
            })
            .collect();
        let t = NlSurface::new(
            moved,
            e.degrees().0,
            e.degrees().1,
            e.u_knots().to_vec(),
            e.v_knots().to_vec(),
        )
        .unwrap();
        let err = certify_step(&s, &t, true, true).unwrap_err();
        assert!(err.contains("knot-removed step off its input"), "{err}");
    }
}
