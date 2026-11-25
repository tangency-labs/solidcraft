//! The certificate of the loop normal (`compute_crvs_normal`, `SmCurve::ComputeCrvsNormal`):
//! a success is the direction of the EXACT area vector of the sampled polygon (expansion
//! arithmetic) within a derived rounding bound, or, for an open chain whose area has no
//! certain direction, a normal of the plane of every sample; a refusal is an exact area
//! vector shorter than `SM_EFF_ZERO` within those bounds (`Unitize`'s refusal), a single
//! linear curve, or a failed evaluation of a sample ([`certify`]).
//!
//! `curve.curve_p3`'s `wr587_every_normal_row_is_the_area_normal` certifies every
//! committed `Normal` row with it; [`plane_normals_of_open_chains_certify`] exercises the
//! plane normal branch no such row reaches.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stderr
)] // test

use solidcore::curve::curve::p2::Curve;
use solidcore::geom::vector3d::Vec3;
use solidcore::math::EFF_ZERO;
use solidcore::Kernel;

// ---- exact arithmetic ------------------------------------------------------------------

/// `a + b` as the rounded sum and its exact error (Knuth's two-sum).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bv = s - a;
    let av = s - bv;
    (s, (a - av) + (b - bv))
}

/// `a * b` as the rounded product and its exact error (exact by the fused multiply-add).
fn two_product(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

/// An exact sum of doubles, a nonoverlapping expansion (Shewchuk's `grow_expansion`): its
/// components add up to the value with no rounding.
#[derive(Clone, Default)]
struct Exact(Vec<f64>);

impl Exact {
    fn add(&mut self, b: f64) {
        let mut q = b;
        let mut out = Vec::with_capacity(self.0.len() + 1);
        for &e in &self.0 {
            let (s, h) = two_sum(q, e);
            if h != 0.0 {
                out.push(h);
            }
            q = s;
        }
        out.push(q);
        self.0 = out;
    }

    /// `+= sign (a1 - a0) (b1 - b0)`, exactly (`sign` is 1 or -1).
    fn add_product_of_differences(&mut self, a1: f64, a0: f64, b1: f64, b0: f64, sign: f64) {
        let (a, ae) = two_sum(a1, -a0);
        let (b, be) = two_sum(b1, -b0);
        for (x, y) in [(a, b), (a, be), (ae, b), (ae, be)] {
            let (p, pe) = two_product(x, y);
            self.add(sign * p);
            self.add(sign * pe);
        }
    }

    fn is_zero(&self) -> bool {
        self.0.iter().all(|&c| c == 0.0)
    }

    /// The value rounded (its components summed) and a bound on that rounding: each of the
    /// `n` additions rounds by at most `u` times the running sum, which is at most the sum
    /// of the absolute components.
    fn estimate(&self) -> (f64, f64) {
        let (mut sum, mut abs) = (0.0, 0.0);
        for &c in &self.0 {
            sum += c;
            abs += c.abs();
        }
        let n = self.0.len() as f64;
        (sum, (n + 1.0) * f64::EPSILON * abs)
    }
}

// ---- the certificate of the kernel's answer ---------------------------------------------

/// An answer of the loop normal, as a call returns it or a golden row records it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Answer {
    /// Success, with the normal.
    Normal(Vec3),
    /// `SM_ERR_INVALID_INPUT` (status 1007) with `(0, 0, 1)` written: `Unitize` refused.
    ZeroNormal,
    /// Another refusal.
    OtherRefusal,
}

/// How the kernel's answer at a call is certified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Cert {
    /// Success: the direction of the exact area vector of the samples.
    AreaNormal,
    /// `SM_ERR_INVALID_INPUT` with `(0, 0, 1)` written: the exact area vector is zero.
    ZeroArea,
    /// `SM_ERR_INVALID_INPUT` with `(0, 0, 1)` written: the exact area vector is not zero
    /// but shorter than `SM_EFF_ZERO` (within the double sum's rounding), where `Unitize`
    /// refuses.
    BelowEffZero,
    /// `SM_ERR_INVALID_INPUT` with `(0, 0, 1)` written: the exact area vector is within the
    /// kernel's rounding bound of zero, so it has no direction.
    WithinRounding,
    /// Success for a closed loop whose area direction is not certain to `1e-6`: the
    /// direction of the exact area vector within its (larger) bound.
    UncertainArea,
    /// Success for an open chain whose area direction is not certain: a unit normal of the
    /// plane every sample lies in (each `|n . (p - p0)|` within `2e-6 |p - p0|`, the kernel's
    /// angular zero and its rounding).
    PlaneNormal,
    /// The single linear curve's status.
    SingleLinear,
    /// A sample's evaluation failed: its status.
    FailedEvaluation,
}

/// The samples of `curves` the kernel sums (upstream's: one chord for a two-point curve,
/// else the six points at `j / 5` of the natural interval), or `None` when an evaluation
/// fails.
fn samples(k: &Kernel, curves: &[Curve]) -> Option<Vec<Vec3>> {
    let mut out = Vec::new();
    for c in curves {
        let domain = c.natural_interval();
        let n: u32 = if c.number_control_points().ok()? == 2 {
            1
        } else {
            5
        };
        for j in 0..=n {
            out.push(
                c.evaluate_point(k, domain.evaluate(f64::from(j) / f64::from(n)))
                    .ok()?,
            );
        }
    }
    Some(out)
}

/// Certifies `own`, the kernel's answer for `curves`, and classes it; panics when it is not
/// the certified area normal.
///
/// A success must be the direction of the exact fan area vector `A` of the samples about the
/// first, within `2 (E + e) / (|A| - e) + 16 eps`. `E = (N + 8) u S` bounds the double sum
/// the kernel makes of the same `N` terms: each term `a_j b_l - a_l b_j` of rounded
/// differences rounds three times (`3 u` of the absolute products, `S` their sum, rounding
/// of the differences adding `2 u` more), and the running sum `N` times (`N u` of the
/// same). `e` is the rounding of the exact value's estimate. The unit vectors of two vectors
/// `d` apart are at most `2 d / |A|` apart, and `Unitize` rounds by a few `u` (the
/// `16 eps`). A refusal for a zero normal must have `|A| < SM_EFF_ZERO` within `E + e`.
pub(crate) fn certify(k: &Kernel, curves: &[Curve], own: Answer) -> Cert {
    let single_linear = curves.len() == 1
        && curves[0]
            .is_linear(k, EFF_ZERO, false)
            .is_ok_and(|l| l.linear);
    if single_linear {
        assert!(
            !matches!(own, Answer::Normal(_)),
            "a single line has no normal"
        );
        return Cert::SingleLinear;
    }
    let Some(points) = samples(k, curves) else {
        assert!(!matches!(own, Answer::Normal(_)), "an evaluation failed");
        return Cert::FailedEvaluation;
    };
    let Some(first) = points.first() else {
        // no curves: an empty sum, exactly zero
        assert_eq!(own, Answer::ZeroNormal, "no curves have no normal");
        return Cert::ZeroArea;
    };
    let u = f64::EPSILON / 2.0;
    let o = first.to_array();
    let mut exact = [Exact::default(), Exact::default(), Exact::default()];
    let mut abs = [0.0_f64; 3];
    let mut s_rule = 0.0_f64;
    // component i of a x b is a[j] b[l] - a[l] b[j]
    let axes = [(1, 2), (2, 0), (0, 1)];
    for w in points.windows(2) {
        let (a, b) = (w[0].to_array(), w[1].to_array());
        for (i, &(j, l)) in axes.iter().enumerate() {
            exact[i].add_product_of_differences(a[j], o[j], b[l], o[l], 1.0);
            exact[i].add_product_of_differences(a[l], o[l], b[j], o[j], -1.0);
            abs[i] += ((a[j] - o[j]) * (b[l] - o[l])).abs() + ((a[l] - o[l]) * (b[j] - o[j])).abs();
        }
        // the kernel's own rounding bound sums `|a - o| |b - o|`
        s_rule += (w[0] - *first).length() * (w[1] - *first).length();
    }
    let terms = points.len().saturating_sub(1) as f64;
    // the kernel's `E` (the smaller of its two bounds, the second `e_kernel`), which
    // decides whether it answers an area of no certain direction
    let e_rule = (terms + 8.0) * u * s_rule * 1.01;
    // (`S` is itself rounded: the factor 1.01 covers that many times over)
    let e_kernel = Vec3::new(abs[0], abs[1], abs[2]).length() * (terms + 8.0) * u * 1.01;
    let est = exact.iter().map(Exact::estimate).collect::<Vec<_>>();
    let a = Vec3::new(est[0].0, est[1].0, est[2].0);
    let e_hat = Vec3::new(est[0].1, est[1].1, est[2].1).length() * 1.01;
    let len = a.length();
    match own {
        Answer::OtherRefusal => panic!("a refusal of a loop with samples is a zero normal"),
        Answer::ZeroNormal => {
            if exact.iter().all(Exact::is_zero) {
                return Cert::ZeroArea;
            }
            if len < EFF_ZERO + e_kernel + e_hat {
                return Cert::BelowEffZero;
            }
            // the kernel refuses a rounded area `|A| <= E`, its `E` at most `e_kernel`
            // (computed from the same terms, the factor 1.01 covering both roundings); the
            // exact one is within `e_kernel + e_hat` of it: an area this independent bound
            // does not put within rounding of zero may not be refused
            assert!(
                len <= 2.0 * e_kernel * 1.01 + e_hat,
                "refused a loop whose area vector {len:e} is not within its rounding of zero"
            );
            Cert::WithinRounding
        }
        Answer::Normal(n) => {
            let e = e_kernel + e_hat;
            let bound = 2.0 * e / (len - e_hat) + 16.0 * f64::EPSILON;
            if len + e >= EFF_ZERO && len > e_hat && bound < 1e-6 {
                // the bound certifies a direction, not only a unit vector (any two unit
                // vectors are within 2 of each other): the answer is within 1e-6 of the
                // exact area normal, so the sense of every `Reversed` call is certified
                // (the measured largest is 2.3e-12)
                let dev = (n - a / len).length();
                if dev <= bound {
                    return Cert::AreaNormal;
                }
                // the kernel's coarser bound may leave it uncertain: then only the plane
                // normal on the area's side is allowed
                assert!(
                    n.dot(a) > 0.0,
                    "normal {n:?} is {dev:e} off the area normal {:?} (bound {bound:e})",
                    a / len
                );
                return certify_plane(&points, n, a, e);
            }
            let (first, last) = (points[0], points[points.len() - 1]);
            // an open chain in the answer's plane may have the plane normal
            if !first.approx_eq(last) && in_plane(&points, n) {
                return certify_plane(&points, n, a, e);
            }
            // otherwise (a closed loop, or a chain in no plane) the kernel keeps its
            // area's direction, which has one: the kernel answers it only above its `E`
            assert!(
                len + e_kernel + e_hat > e_rule.min(e_kernel) && len > e_hat,
                "a success of no direction"
            );
            let dev = (n - a / len).length();
            assert!(
                dev <= bound,
                "normal {n:?} is {dev:e} off the area normal {:?} (bound {bound:e})",
                a / len
            );
            Cert::UncertainArea
        }
    }
}

/// Certifies a success for an open chain whose area direction is not certain: `n` is a
/// unit normal of the plane of every sample, `|n . (p - p0)| <= 2e-6 |p - p0|` (the
/// kernel's angular zero, 1.75e-6, and rounding), on the side of the exact area vector
/// `a` when `|a . n|` exceeds its error `e` (a few `u` of `|a|` covering `n`'s rounding);
/// else its sense is the kernel's documented tie-break (the chain's turns), positive
/// where their sum is certain.
fn certify_plane(points: &[Vec3], n: Vec3, a: Vec3, e: f64) -> Cert {
    let (first, last) = (points[0], points[points.len() - 1]);
    assert!(
        !first.approx_eq(last),
        "a closed loop of no certain area has no plane normal"
    );
    assert!(
        (n.length() - 1.0).abs() < 1e-12,
        "a plane normal is a unit vector"
    );
    assert!(
        in_plane(points, n),
        "the plane normal {n:?} is not normal to every chord from the first sample"
    );
    // the samples span a plane: some offset from the first is off the line to the
    // farthest by more than the kernel's angular zero (1.75e-6) and its rounding
    let mut d = Vec3::ZERO;
    for &p in points {
        if (p - first).length() > d.length() {
            d = p - first;
        }
    }
    assert!(
        points.iter().any(|&p| {
            let v = p - first;
            d.cross(v).length() > 1.7e-6 * d.length() * v.length()
        }),
        "collinear samples span no plane"
    );
    let along = n.dot(a);
    if along.abs() > e + 8.0 * f64::EPSILON * a.length() {
        assert!(
            along > 0.0,
            "the plane normal {n:?} is against the area {a:?}"
        );
    } else {
        // no certain area side: a certain sum of the chords' turns about `n` is positive
        // (each turn `n . (c_i x c_i+1)` is off by at most its size `|c_i||c_i+1|` times
        // `n`'s 2e-6 tilt out of the plane, and by rounding)
        let (mut turns, mut weight, mut count) = (0.0, 0.0, 0.0);
        let chords: Vec<Vec3> = points
            .windows(2)
            .map(|w| w[1] - w[0])
            .filter(|&c| c != Vec3::ZERO)
            .collect();
        for w in chords.windows(2) {
            turns += n.dot(w[0].cross(w[1]));
            weight += w[0].length() * w[1].length();
            count += 1.0;
        }
        if turns.abs() > ((count + 8.0) * 8.0 * f64::EPSILON + 4e-6) * weight {
            assert!(
                turns > 0.0,
                "the plane normal {n:?} is against the chain's turns {turns:e}"
            );
        }
    }
    Cert::PlaneNormal
}

/// Every sample lies in the plane through the first with unit normal `n`:
/// `|n . (p - p0)| <= 2e-6 |p - p0|` (the kernel's angular zero, 1.75e-6, and rounding).
fn in_plane(points: &[Vec3], n: Vec3) -> bool {
    let first = points[0];
    points.iter().all(|&p| {
        let d = p - first;
        n.dot(d).abs() <= 2e-6 * d.length()
    })
}

/// The polyline through `points` as line segments (B-splines with two control points).
fn polyline(points: &[[f64; 3]]) -> Vec<Curve> {
    points
        .windows(2)
        .map(|w| {
            Curve::from(
                solidcore::nurbs::b_spline_curve::p1::BSplineCurve::create_line_segment(
                    3,
                    Vec3::from_array(w[0]),
                    Vec3::from_array(w[1]),
                )
                .unwrap(),
            )
        })
        .collect()
}

/// No `curve.curve_p3` row reaches the plane normal, so its certificate is exercised here: the Z path, whose area cancels exactly, at the
/// origin and tilted, offset and scaled, and the Z path with its end raised (area along
/// +Z, of no certain direction), certify as plane normals; the reversed answer does not.
#[test]
fn plane_normals_of_open_chains_certify() {
    let k = Kernel::default();
    let u = Vec3::new(0.3_f64.cos(), 0.3_f64.sin(), 0.0);
    let v = Vec3::new(
        -0.3_f64.sin() * 0.7_f64.cos(),
        0.3_f64.cos() * 0.7_f64.cos(),
        0.7_f64.sin(),
    );
    let mut chains = vec![polyline(&[
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [2.0, -2.0, 0.0],
        [2.0, 1e-10, 0.0],
    ])];
    for scale in [1.0, 100.0, 1e4] {
        let off = Vec3::new(123.456, 7.89, -3.21) * scale;
        let pts: Vec<[f64; 3]> = [[0.0, 0.0], [1.0, 1.0], [2.0, -2.0], [2.0, 0.0]]
            .iter()
            .map(|p: &[f64; 2]| (off + (u * p[0] + v * p[1]) * scale).to_array())
            .collect();
        chains.push(polyline(&pts));
    }
    for (i, curves) in chains.iter().enumerate() {
        let n = solidcore::curve::curve::p3::compute_crvs_normal(&k, curves).unwrap();
        assert_eq!(certify(&k, curves, Answer::Normal(n)), Cert::PlaneNormal);
        // the raised path's area is along +Z; the placed paths' area is exactly zero and
        // their turns (-4, then +2 about u x v) sum against u x v
        let expected = if i == 0 { Vec3::Z } else { -u.cross(v) };
        assert!(n.dot(expected) > 1.0 - 1e-12, "{i}: {n:?}");
        if i > 0 {
            let reversed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                certify(&k, curves, Answer::Normal(-n))
            }));
            assert!(
                reversed.is_err(),
                "{i}: a plane normal against the turns certifies"
            );
        }
    }
    // collinear samples of two curves span no plane: a normal for them fails
    let collinear = polyline(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    let line = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        certify(&k, &collinear, Answer::Normal(Vec3::Z))
    }));
    assert!(line.is_err(), "a normal of collinear samples certifies");
    // the raised Z path's area is certainly along +Z: the reversed plane normal fails
    let reversed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        certify(&k, &chains[0], Answer::Normal(-Vec3::Z))
    }));
    assert!(
        reversed.is_err(),
        "a plane normal against the area certifies"
    );
    // an open chain in no plane whose area has no certain direction answers that
    // direction (the kernel's last step): not a plane normal, the area's direction
    let skew = polyline(&[
        [0.0, 0.0, 0.0],
        [-16.0, 8.0, 8.0],
        [16.0, -16.0, 8.0],
        [0.0, -8.0, 16.0],
        [-16.0, 0.0, -16.0],
        [-8.0, -8.0 + 2f64.powi(-32), 8.0],
    ]);
    let n = solidcore::curve::curve::p3::compute_crvs_normal(&k, &skew).unwrap();
    assert_eq!(certify(&k, &skew, Answer::Normal(n)), Cert::UncertainArea);
    // a certain area far along X whose turns are opposed keeps the area's side
    let far = polyline(&[
        [0.0, 0.0, 0.0],
        [1e6, 0.0, 0.0],
        [1e6 + 1.0, 1.0, 0.0],
        [1e6 + 2.0, -2.0, 0.0],
        [1e6 + 2.0, -1e-9, 0.0],
    ]);
    let n = solidcore::curve::curve::p3::compute_crvs_normal(&k, &far).unwrap();
    assert_eq!(n, -Vec3::Z);
    certify(&k, &far, Answer::Normal(n));
}
