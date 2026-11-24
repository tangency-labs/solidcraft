//! WR-646 (#18917): a variable round's span is checked against the fillet solved at its
//! test shares, so the round follows its radius law between traced points. The seven
//! `law-*` rows it moves keep their status and counts and change only their geometry and
//! mass values. Each row is checked here, on the finished body, one test per row:
//! - the fillet succeeds and the vertex, edge and face counts are the golden's, unchanged;
//! - `check_brep` Level 2 reports nothing and the body is one closed manifold solid;
//! - the volume the mass integrator converges to lies in the body's certified volume
//!   enclosure ([`enclose_volume`]), and every other mass value lies in its certified
//!   band ([`mass_bands`]). This is a consistency check: it certifies that the values are
//!   this body's, not that the body is the true round.
//! - the linear-section row's volume is certified against its closed form. A linear
//!   section cuts the triangle `r^2 / 2` from the box's section, so with the law
//!   `r(u) = 4 - 4u + 4u^2` along the 20-long edge the true round removes
//!   `20 * int_0^1 r^2 du / 2 = 20 * 11.2 / 2 = 112`. The body's skin is certified within
//!   `T = 4 * tol` (the edge's tolerance) of that chord surface ([`linear_skin_bound`]:
//!   Bernstein bounds on the net's polynomial patches). In the section at `y`, with `d1`,
//!   `d2` the depths below the two box faces, the skin's curve then lies in the band
//!   `|d1 + d2 - r| <= sqrt(2) T`. Every point below the band joins the corner, and every
//!   point above it the body's interior, by a ray from the corner, on which `d1 + d2` only
//!   grows, so it never meets the curve. So the removed area lies between
//!   `(r - sqrt(2) T)^2 / 2` and `(r + sqrt(2) T)^2 / 2`, within `sqrt(2) r T + T^2` of
//!   `r^2 / 2`, and the volume lies within `B = sqrt(2) T int_0^20 r + 20 T^2 =
//!   sqrt(2) T 200 / 3 + 20 T^2` of `8000 - 112`. The body's certified enclosure lies in
//!   that band. Main's body removed `112.0517` (off by `5.2e-2`, past `B`), so main's
//!   value is refuted. (The edges lie on the box planes to within their tolerance, which
//!   moves the volume by `O(tol^2)`, far below `B`.)
//! - each of the six ball-section rows is compared with the volume `V*` the balls'
//!   envelope takes off the box. `V*` is enclosed in rounded interval arithmetic
//!   ([`envelope::envelope`]; the derivation is in that module). The body's certified
//!   enclosure and the golden's new volume are both certifiably closer to `box - V*` than
//!   main's volume was, which needs no premise about the skin. Both also lie within
//!   `T * A_lo` of it (`A_lo <= ` the envelope's area). That band is a consistency check
//!   with a premise, the skin and the envelope within `T` of each other. Skin to envelope
//!   (G2) is certified for four of the rows by an ignored proof run in
//!   `solidcore/crates/solidcore/tests/wr646_variable_round_follows_law.rs`; envelope to
//!   body (G3) is argued there, not certified. So the band is reported, not a proof, and
//!   main's values lie inside it too: what refutes main is the closeness above.
//!
//! [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume
//! [`mass_bands`]: super::round_definition_proof::mass_bands

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::round_definition_proof::{mass_bands, volume_band};
use super::round_definition_rows::{counts, inputs};
use super::*;
use solidcore::blend::fillet_section::interval::Ivl;
use solidcore::mass::enclosure::enclose_volume;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};

#[path = "wr646_envelope.rs"]
mod envelope;

/// A moved row: name, main's volume (the golden's before), and its status and counts
/// (the same before and after).
type Row = (&'static str, f64, [i64; 4]);

const ROWS: &[Row] = &[
    ("law-box10x20x30-e0-deg2-edge-map-reversed", 5986.9198957359595, [0, 10, 15, 7]),
    ("law-box10x20x30-e0-deg2-edge-map-sub-range", 5986.9198957359595, [0, 10, 15, 7]),
    ("law-cube20-e0-deg2-smooth-4-2-4", 7952.165588609178, [0, 10, 15, 7]),
    ("law-cube20-e0-deg2-smooth-g2", 7952.165588609178, [0, 10, 15, 7]),
    ("law-cube20-e0-deg2-smooth-xsect-linear", 7887.9482893284285, [0, 10, 15, 7]),
    ("law-cube20-e0-deg3-asymmetric", 7953.6430498032405, [0, 10, 15, 7]),
    ("law-cube20-e7-deg3-asymmetric", 7953.643049803992, [0, 10, 15, 7]),
];

/// The linear-section row, whose removed volume has a closed form.
const LINEAR: &str = "law-cube20-e0-deg2-smooth-xsect-linear";

/// One test per moved row, so the test runner spreads the rows over its threads.
macro_rules! row_tests {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                check($name);
            }
        )*

        /// The rows the generated tests take, in [`ROWS`]'s order.
        const ROW_TESTS: &[&str] = &[$($name),*];
    };
}

row_tests! {
    law_box10x20x30_e0_deg2_edge_map_reversed => "law-box10x20x30-e0-deg2-edge-map-reversed",
    law_box10x20x30_e0_deg2_edge_map_sub_range => "law-box10x20x30-e0-deg2-edge-map-sub-range",
    law_cube20_e0_deg2_smooth_4_2_4 => "law-cube20-e0-deg2-smooth-4-2-4",
    law_cube20_e0_deg2_smooth_g2 => "law-cube20-e0-deg2-smooth-g2",
    law_cube20_e0_deg2_smooth_xsect_linear => "law-cube20-e0-deg2-smooth-xsect-linear",
    law_cube20_e0_deg3_asymmetric => "law-cube20-e0-deg3-asymmetric",
    law_cube20_e7_deg3_asymmetric => "law-cube20-e7-deg3-asymmetric",
}

/// Every row of [`ROWS`] has exactly one test.
#[test]
fn every_moved_row_has_its_own_test() {
    let rows: Vec<&str> = ROWS.iter().map(|row| row.0).collect();
    assert_eq!(rows, ROW_TESTS);
}

/// The [`ROWS`] row named `row`, as the module header says.
fn check(row: &str) {
    let picked: Vec<&Row> = ROWS.iter().filter(|r| r.0 == row).collect();
    assert_eq!(picked.len(), 1, "{row}: one row of that name");
    for &&(name, old_volume, before) in &picked {
        let input = inputs(name).real_solid.expect("a real solid");
        assert_eq!(input.checkpoint, Checkpoint::Full, "{name}");
        assert!(input.radius_law.is_some(), "{name}: a radius law");
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input).unwrap().unwrap();
        let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
        assert_eq!(selected.len(), 1, "{name}");
        let tol = edge::tolerance(&body, selected[0]).unwrap();
        let len = edge_length(&k, &body, selected[0], input.dimensions.unwrap());
        let ends = {
            let ivl = edge::interval(&body, selected[0]).unwrap();
            let c = edge::curve(&body, selected[0]).unwrap().unwrap();
            [c.evaluate_point(&k, ivl.min()).unwrap(), c.evaluate_point(&k, ivl.max()).unwrap()]
        };
        let result = fillet_selected(&mut k, &mut body, &selected, &input).ok().unwrap();
        assert!(result.is_ok(), "{name}: {result:?}");
        assert_eq!(counts(&body, 0), before, "{name}");
        let lines = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap().lines();
        assert!(lines.is_empty(), "{name}: Level 2: {lines:?}");
        assert!(is_manifold_solid(&body).unwrap(), "{name}: not one closed manifold solid");
        // the law rounds are bare nets, not definitions: no spread to add
        let estimates = volume_band(&k, &mut body, &[], old_volume, name);
        mass_bands(&k, &mut body, &estimates, name);
        if name == LINEAR {
            let law = input.radius_law.as_ref().unwrap();
            assert_eq!(law.edge_map, [0.0, 1.0], "{name}: the law spans the edge");
            let dims = input.dimensions.unwrap();
            let skin = linear_skin_bound(&k, &body, ends, dims, &law.radii, 4.0 * tol);
            linear_holds_its_closed_form(&k, &mut body, (4.0 * tol, skin, len), old_volume);
        } else {
            let law = input.radius_law.as_ref().unwrap();
            let degree = law.degree as usize;
            assert_eq!(law.radii.len(), degree + 1, "{name}: one Bezier span");
            assert_eq!(law.knots, [0.0, 1.0], "{name}: one Bezier span");
            let m = law.degree + 1;
            assert_eq!(law.multiplicities, [m, m], "{name}: one Bezier span");
            let boxed = input.dimensions.unwrap().iter().product::<f64>();
            let golden = estimates.golden.properties.volume;
            let args = (len, 4.0 * tol, boxed, old_volume, golden);
            balls_hold_their_envelope(&k, &mut body, name, &law.radii, law.edge_map, args);
        }
    }
}

/// The linear-section row: its skin is certified within `T` of the chord surface
/// ([`linear_skin_bound`]), so its volume lies within `B = T sqrt 2 int r + len T^2` of
/// `8000 - 112` (the module header derives both). Its certified volume enclosure lies in
/// that band; main's volume did not.
fn linear_holds_its_closed_form(
    k: &Kernel,
    body: &mut Brep,
    (t, skin, len): (f64, f64, f64),
    old_volume: f64,
) {
    assert!(skin <= t, "{LINEAR}: the skin is {skin:e} off the chord, past T = {t:e}");
    let exact = Ivl::from(8000.0) - Ivl::from(112.0);
    // int_0^len r = len * 10 / 3 for r(u) = 4 - 4u + 4u^2
    let (tt, l) = (Ivl::from(t), Ivl::from(len));
    let bound = (tt * Ivl::from(2.0).sqrt() * l * Ivl::from(10.0) / Ivl::from(3.0) + l * tt * tt).hi;
    let e = enclose_volume(k, body, 1e-9 * exact.hi).unwrap();
    eprintln!(
        "{LINEAR}: skin within {skin:e} of the chord (T = {t:e}); volume in [{}, {}], closed \
         form {}, bound {bound:e}, main {old_volume}",
        e.lo, e.hi, exact.hi
    );
    assert!(
        (exact - Ivl::from(bound)).hi <= e.lo && e.hi <= (exact + Ivl::from(bound)).lo,
        "{LINEAR}: [{}, {}] not within {bound:e} of {}",
        e.lo,
        e.hi,
        exact.hi
    );
    assert!(
        (Ivl::from(old_volume) - exact).mig() > bound,
        "{LINEAR}: main's {old_volume} was already within {bound:e} of {}",
        exact.hi
    );
}

/// The length of the box edge `e`, which is one of the box's `dims` (asserted): that
/// exact value.
fn edge_length(k: &Kernel, body: &Brep, e: EdgeId, dims: [f64; 3]) -> f64 {
    let ivl = edge::interval(body, e).unwrap();
    let curve = edge::curve(body, e).unwrap().unwrap();
    let a = curve.evaluate_point(k, ivl.min()).unwrap();
    let b = curve.evaluate_point(k, ivl.max()).unwrap();
    let len = a.distance(b);
    let exact: Vec<f64> = dims.into_iter().filter(|d| (d - len).abs() < 1e-9).collect();
    assert!(!exact.is_empty(), "the edge is {len} long, none of {dims:?}");
    exact[0]
}

/// A ball-section row's certified volume enclosure and the golden's new volume lie within
/// `T * A_lo` of the box less the balls' envelope volume `V*`, certified by
/// [`envelope::envelope`] (`A_lo` is at most the envelope's area). Both are also
/// certifiably closer to it than main's volume was.
fn balls_hold_their_envelope(
    k: &Kernel,
    body: &mut Brep,
    name: &str,
    radii: &[f64],
    edge_map: [f64; 2],
    (len, t, boxed, old_volume, golden): (f64, f64, f64, f64, f64),
) {
    let env = envelope::envelope(radii, edge_map, len);
    let reference = Ivl::from(boxed) - env.volume;
    let band = (Ivl::from(t) * Ivl::from(env.area_lo)).lo;
    let e = enclose_volume(k, body, 1e-9 * boxed).unwrap();
    // the farthest a value in [lo, hi] can be from the reference, and the nearest
    let far = |lo: f64, hi: f64| {
        (Ivl::from(hi) - reference)
            .hi
            .max((reference - Ivl::from(lo)).hi)
    };
    let near = |v: f64| {
        if v < reference.lo {
            (reference - Ivl::from(v)).lo
        } else if v > reference.hi {
            (Ivl::from(v) - reference).lo
        } else {
            0.0
        }
    };
    let (body_far, golden_far, main_near) = (far(e.lo, e.hi), far(golden, golden), near(old_volume));
    eprintln!(
        "{name}: V* in [{}, {}] (width {:e}, {} cells), box - V* in [{}, {}]; body in [{}, {}], \
         within {body_far:e}; golden {golden} within {golden_far:e}; main {old_volume} at least \
         {main_near:e} off; band T * A_lo = {t:e} x {} = {band:e}",
        env.volume.lo,
        env.volume.hi,
        env.volume.hi - env.volume.lo,
        env.cells,
        reference.lo,
        reference.hi,
        e.lo,
        e.hi,
        env.area_lo,
    );
    assert!(body_far <= band, "{name}: the body is {body_far:e} off, past {band:e}");
    assert!(golden_far <= band, "{name}: the golden is {golden_far:e} off, past {band:e}");
    assert!(
        body_far < main_near && golden_far < main_near,
        "{name}: not certifiably closer than main ({body_far:e}, {golden_far:e} against {main_near:e})"
    );
}

/// The linear-section row's skin, certified within `T` of the chord surface: every point
/// `p` of the round's net (and so of the face, a part of it) has `|phi(p)| <= T`, with
/// `phi = (d1 + d2 - r(w)) / sqrt 2`. Here `d1`, `d2` are `p`'s depths below the two box
/// faces at the edge, and `w` is `p`'s fraction along the edge. In the section across the
/// edge through `p`, `|phi|` is `p`'s distance from the chord `d1 + d2 = r`.
///
/// The net is polynomial (asserted: not rational), so `phi` is a polynomial on each Bezier
/// patch: `d1 + d2` and `w` have the patch's degrees `(p, q)`, and `w^2` has `(2p, 2q)`.
/// Its Bernstein coefficients at `(2p, 2q)` (degree elevation and the Bernstein product
/// rule, in rounded interval arithmetic) bound it. A patch whose bound is too loose is
/// halved in u by de Casteljau, in the same arithmetic. Returns the certified `sup |phi|`.
fn linear_skin_bound(
    k: &Kernel,
    body: &Brep,
    ends: [solidcore::math::Point3; 2],
    dims: [f64; 3],
    radii: &[f64],
    t: f64,
) -> f64 {
    use solidcore::nurbs::b_spline_surface::SurfParamType;
    use solidcore::surface::surface::SurfaceKind;
    let _ = k;
    let c = |p: solidcore::math::Point3| [p.x, p.y, p.z];
    let (a, b) = (c(ends[0]), c(ends[1]));
    let along: Vec<usize> = (0..3).filter(|&j| a[j] != b[j]).collect();
    assert_eq!(along.len(), 1, "the edge runs along one axis");
    let i = along[0];
    // the two faces at the edge: the box's planes `x_j = a_j`, at 0 or the box's size
    let depth: Vec<(usize, f64)> = (0..3)
        .filter(|&j| j != i)
        .map(|j| {
            assert!(a[j] == 0.0 || a[j] == dims[j], "the edge lies on the box");
            (j, if a[j] == 0.0 { 1.0 } else { -1.0 })
        })
        .collect();
    // r(w) = R0 (1 - w)^2 + 2 R1 w (1 - w) + R2 w^2 = c0 + c1 w + c2 w^2
    assert_eq!(radii.len(), 3, "a quadratic law");
    let r = |j: usize| Ivl::from(radii[j]);
    let two = Ivl::from(2.0);
    let c0 = r(0);
    let c1 = two * (r(1) - r(0));
    let c2 = r(0) - two * r(1) + r(2);
    let mut nets = 0;
    let mut sup = 0.0_f64;
    for f in p1::faces(body, None).unwrap() {
        let s = body.face(f).unwrap().surface.clone().unwrap();
        let net = match s.kind() {
            SurfaceKind::Plane(_) => continue,
            SurfaceKind::BSpline(net) => net,
            _ => panic!("{LINEAR}: {f:?} is neither a box plane nor the round's net"),
        };
        nets += 1;
        assert!(!net.is_rational(), "{LINEAR}: the round's net is polynomial");
        let (p, q) = (net.degree(SurfParamType::U), net.degree(SurfParamType::V));
        let (nu, nv, pts) = net.control_points().unwrap();
        // Bezier patches: every interior knot has multiplicity p (q), the ends p + 1 (q + 1)
        let spans = |d: SurfParamType, deg: usize, n: usize| {
            let (_, mult) = net.knots(d, None).unwrap();
            let last = mult.len() - 1;
            for (j, &m) in mult.iter().enumerate() {
                let want = if j == 0 || j == last { deg + 1 } else { deg };
                assert_eq!(m, want, "{LINEAR}: the net is Bezier patches");
            }
            assert_eq!(n, last * deg + 1, "{LINEAR}: the net is Bezier patches");
            last
        };
        let (su, sv) = (spans(SurfParamType::U, p, nu), spans(SurfParamType::V, q, nv));
        for pu in 0..su {
            for pv in 0..sv {
                // per control point: D = d1 + d2 and W = w
                let mut dd = vec![vec![Ivl::from(0.0); q + 1]; p + 1];
                let mut ww = dd.clone();
                for (x, (drow, wrow)) in dd.iter_mut().zip(ww.iter_mut()).enumerate() {
                    for (y, (d, w)) in drow.iter_mut().zip(wrow.iter_mut()).enumerate() {
                        let cp = pts[(pu * p + x) * nv + pv * q + y];
                        let cp = [cp.x, cp.y, cp.z];
                        *d = depth.iter().fold(Ivl::from(0.0), |acc, &(j, sign)| {
                            acc + Ivl::from(sign) * (Ivl::from(cp[j]) - Ivl::from(a[j]))
                        });
                        *w = (Ivl::from(cp[i]) - Ivl::from(a[i])) / (Ivl::from(b[i]) - Ivl::from(a[i]));
                    }
                }
                let one = vec![vec![Ivl::from(1.0); q + 1]; p + 1];
                let e_d = bernstein_product(&dd, &one, p, q);
                let e_w = bernstein_product(&ww, &one, p, q);
                let w2 = bernstein_product(&ww, &ww, p, q);
                let phi: Vec<Vec<Ivl>> = e_d
                    .iter()
                    .zip(&e_w)
                    .zip(&w2)
                    .map(|((d, w), w2)| {
                        d.iter()
                            .zip(w)
                            .zip(w2)
                            .map(|((&d, &w), &w2)| {
                                (d - c0 - c1 * w - c2 * w2) / two.sqrt()
                            })
                            .collect()
                    })
                    .collect();
                sup = sup.max(patch_sup(phi, t, 0));
            }
        }
    }
    assert_eq!(nets, 1, "{LINEAR}: one round face");
    sup
}

/// The Bernstein coefficients at degrees `(2p, 2q)` of the product of two patches of
/// degrees `(p, q)`: `c_ik = sum C(p, a) C(p, i - a) / C(2p, i) * C(q, e) C(q, k - e) /
/// C(2q, k) * f_ae g_(i-a)(k-e)`. With `g = 1` it is `f` elevated to `(2p, 2q)`.
fn bernstein_product(f: &[Vec<Ivl>], g: &[Vec<Ivl>], p: usize, q: usize) -> Vec<Vec<Ivl>> {
    let binom = |n: usize, k: usize| (0..k).fold(1.0, |acc, j| acc * (n - j) as f64 / (j + 1) as f64);
    let mut out = vec![vec![Ivl::from(0.0); 2 * q + 1]; 2 * p + 1];
    for (i, row) in out.iter_mut().enumerate() {
        for (kk, c) in row.iter_mut().enumerate() {
            for a in i.saturating_sub(p)..=i.min(p) {
                for e in kk.saturating_sub(q)..=kk.min(q) {
                    let wt = Ivl::from(binom(p, a) * binom(p, i - a) * binom(q, e) * binom(q, kk - e))
                        / Ivl::from(binom(2 * p, i) * binom(2 * q, kk));
                    *c = *c + wt * f[a][e] * g[i - a][kk - e];
                }
            }
        }
    }
    out
}

/// `sup |phi|` over a patch, by its Bernstein coefficients' hull; halved in u (de
/// Casteljau at 1/2) while that bound exceeds `t`, to depth 16.
fn patch_sup(phi: Vec<Vec<Ivl>>, t: f64, depth: u32) -> f64 {
    let hull = phi.iter().flatten().fold(0.0_f64, |m, c| m.max(c.mag()));
    if hull <= t || depth == 16 {
        return hull;
    }
    let half = Ivl::from(0.5);
    let n = phi.len();
    let mut tri = phi;
    let (mut left, mut right) = (Vec::with_capacity(n), Vec::with_capacity(n));
    for level in 0..n {
        left.push(tri[0].clone());
        right.push(tri[n - 1 - level].clone());
        tri = tri
            .windows(2)
            .map(|w| w[0].iter().zip(&w[1]).map(|(&x, &y)| half * (x + y)).collect())
            .collect();
    }
    right.reverse();
    patch_sup(left, t, depth + 1).max(patch_sup(right, t, depth + 1))
}
