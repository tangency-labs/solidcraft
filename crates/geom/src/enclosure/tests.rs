use std::cmp::Ordering;

use super::consts::{HALF_PI_HEAD, HALF_PI_TAIL, LN_2, LN_2_HEAD, LN_2_TAIL, LN_10, PI};
use super::*;

/// Each operation holds its exact result (checked with a fused multiply-add, which rounds once,
/// so its sign is the exact one's), including where it rounds below the normal range or to zero,
/// and where `1 − α` loses most of α's digits; and a midpoint is held even there.
#[test]
fn every_operation_holds_its_exact_result() {
    let holds = |x: Interval, k: f64, target: f64| x.lo.mul_add(k, -target) <= 0.0 && x.hi.mul_add(k, -target) >= 0.0;
    let third = Interval::point(1.0) / Interval::point(3.0);
    assert!(holds(third, 3.0, 1.0), "{third:?}");
    let tiny = Interval::point(f64::from_bits(1)) * Interval::point(0.5);
    assert!(tiny.lo < f64::from_bits(1) && tiny.hi >= f64::from_bits(1) && tiny.lo <= 0.0, "{tiny:?}");
    // α = (1.5 − 2⁻⁵²)/1.5; the exact 1 − α is 2⁻⁵²/1.5.
    let rest = Interval::point(1.0) - Interval::point(1.5 - 2f64.powi(-52)) / Interval::point(1.5);
    assert!(holds(rest, 1.5, 2f64.powi(-52)), "{rest:?}");
    assert_eq!(Interval::point(1.0) / Interval::ZERO, Interval::ENTIRE);
    assert!(!(Interval::point(f64::MAX) + Interval::point(f64::MAX)).is_finite());
    // `lo/2 + hi/2` rounds the smallest subnormal to 0, outside [η, η].
    for x in [f64::from_bits(1), f64::from_bits(3)] {
        assert_eq!(Interval::point(x).mid(), x);
    }
}

/// A result that is exact keeps its bounds: `√(400 − 20²)` is `[0, 0]`, so it can be proven at
/// the end of its domain. Where a zero error can't be trusted (below 2⁻⁹⁶⁸), both bounds step,
/// as for the two cases in #10414: `η × ½` and the root of `2⁻¹⁰²²(1 + 2⁻⁵¹)`.
#[test]
fn exact_results_keep_their_bounds_and_untrusted_ones_step() {
    let p = |x: f64| Interval::point(x);
    assert_eq!(p(400.0) - p(20.0).sqr(), Interval::ZERO);
    assert_eq!((p(400.0) - p(20.0) * p(20.0)).sqrt_nonneg(), Interval::ZERO);
    assert_eq!(p(4.0).sqrt_nonneg(), p(2.0));
    assert_eq!(p(0.5) * p(0.5), p(0.25));
    assert_eq!(p(1.0) / p(4.0), p(0.25));
    assert_ne!(p(0.1) + p(0.2) - p(0.1) - p(0.2), Interval::ZERO, "0.1 + 0.2 rounds, so the sum steps");
    let eta = f64::from_bits(1);
    let half = p(eta) * p(0.5);
    assert!(half.lo <= 0.0 && half.hi >= eta, "η/2 lies strictly between: {half:?}");
    let x = 2f64.powi(-1022) * (1.0 + 2f64.powi(-51));
    let r = p(x).sqrt_nonneg();
    assert!(r.lo < r.hi, "a root below 2⁻⁹⁶⁸ steps: {r:?}");
    // The other side of each guard: from 2⁻⁹⁶⁸ up, a zero residual means exact.
    let floor = interval::EXACT_FLOOR;
    assert_eq!(p(floor).sqrt_nonneg(), p(2f64.powi(-484)));
    assert_eq!(p(floor) * p(1.0), p(floor));
}

/// A value held in fixed point with 120 fraction bits, from series whose truncations each lose
/// less than one unit, so it is within [`ERR`] units of the true value.
mod fixed {
    pub const SCALE: u32 = 120;
    pub const ERR: i128 = 1 << 12;

    fn atan_inv(n: i128) -> i128 {
        let (mut total, mut term, mut k, mut sign) = (0i128, (1i128 << SCALE) / n, 0i128, 1i128);
        while term != 0 {
            total += sign * (term / (2 * k + 1));
            term /= n * n;
            k += 1;
            sign = -sign;
        }
        total
    }
    fn atanh_inv(n: i128) -> i128 {
        let (mut total, mut term, mut k) = (0i128, (1i128 << SCALE) / n, 0i128);
        while term != 0 {
            total += term / (2 * k + 1);
            term /= n * n;
            k += 1;
        }
        total
    }
    /// π by Machin's formula, ln 2 = 2 atanh(1/3), ln 10 = 3 ln 2 + 2 atanh(1/9).
    pub fn pi() -> i128 {
        4 * (4 * atan_inv(5) - atan_inv(239))
    }
    pub fn ln2() -> i128 {
        2 * atanh_inv(3)
    }
    pub fn ln10() -> i128 {
        3 * ln2() + 2 * atanh_inv(9)
    }
    /// A positive finite double in these units, exactly (it must be a multiple of 2⁻¹²⁰).
    pub fn of(x: f64) -> i128 {
        let bits = x.to_bits();
        let e = ((bits >> 52) & 0x7ff) as i32 - 1075;
        let m = ((bits & 0x000f_ffff_ffff_ffff) | (1 << 52)) as i128;
        let shift = e + SCALE as i32;
        assert!((0..70).contains(&shift), "{x} isn't exact in fixed point");
        m << shift
    }
}

/// Every constant's bounds are on either side of its value, computed independently.
#[test]
fn constants_hold_their_values() {
    let holds = |i: Interval, v: i128| fixed::of(i.lo) + fixed::ERR < v && v < fixed::of(i.hi) - fixed::ERR;
    assert!(holds(PI, fixed::pi()));
    assert!(holds(LN_2, fixed::ln2()));
    assert!(holds(LN_10, fixed::ln10()));
    assert!(holds(Interval::point(HALF_PI_HEAD) + HALF_PI_TAIL, fixed::pi() / 2));
    assert!(fixed::of(HALF_PI_HEAD) + fixed::of(HALF_PI_TAIL.lo) + fixed::ERR < fixed::pi() / 2);
    assert!(fixed::pi() / 2 < fixed::of(HALF_PI_HEAD) + fixed::of(HALF_PI_TAIL.hi) - fixed::ERR);
    assert!(fixed::of(LN_2_HEAD) + fixed::of(LN_2_TAIL.lo) + fixed::ERR < fixed::ln2());
    assert!(fixed::ln2() < fixed::of(LN_2_HEAD) + fixed::of(LN_2_TAIL.hi) - fixed::ERR);
    // The heads have 30 significant bits, so a whole multiple below 2²³ is exact.
    for head in [HALF_PI_HEAD, LN_2_HEAD] {
        assert_eq!(head.to_bits() & ((1 << 23) - 1), 0, "{head}");
    }
}

/// The reference the functions are checked against: double-double (about 106 bits), by other
/// means than the enclosures use (Newton's method on `exp` and `sin` for the inverses, scaling
/// and squaring for `exp`), and not proven: it only needs to be far more precise than an
/// enclosure is wide.
mod reference {
    #[derive(Clone, Copy, Debug)]
    pub struct Dd(pub f64, pub f64);

    fn two_sum(a: f64, b: f64) -> (f64, f64) {
        let s = a + b;
        let v = s - a;
        (s, (a - (s - v)) + (b - v))
    }
    fn fast(a: f64, b: f64) -> Dd {
        let s = a + b;
        Dd(s, b - (s - a))
    }
    pub const PI: Dd = Dd(std::f64::consts::PI, 1.2246467991473532e-16);
    pub const LN2: Dd = Dd(std::f64::consts::LN_2, 2.3190468138462996e-17);

    impl Dd {
        pub fn of(x: f64) -> Dd {
            Dd(x, 0.0)
        }
        pub fn add(self, o: Dd) -> Dd {
            let (s, e) = two_sum(self.0, o.0);
            let (t, f) = two_sum(self.1, o.1);
            let Dd(s, e) = fast(s, e + t);
            fast(s, e + f)
        }
        pub fn neg(self) -> Dd {
            Dd(-self.0, -self.1)
        }
        pub fn sub(self, o: Dd) -> Dd {
            self.add(o.neg())
        }
        pub fn mul(self, o: Dd) -> Dd {
            let p = self.0 * o.0;
            let e = self.0.mul_add(o.0, -p);
            fast(p, e + (self.0 * o.1 + self.1 * o.0))
        }
        pub fn div(self, o: Dd) -> Dd {
            let q1 = self.0 / o.0;
            let r = self.sub(o.mul(Dd::of(q1)));
            let q2 = r.0 / o.0;
            let r = r.sub(o.mul(Dd::of(q2)));
            fast(q1, q2).add(Dd::of(r.0 / o.0))
        }
        pub fn scale(self, k: i32) -> Dd {
            let f = 2f64.powi(k);
            Dd(self.0 * f, self.1 * f)
        }
    }

    fn series_expm1(r: Dd) -> Dd {
        let (mut sum, mut term) = (Dd::of(0.0), Dd::of(1.0));
        for n in 1..40 {
            term = term.mul(r).div(Dd::of(f64::from(n)));
            sum = sum.add(term);
        }
        sum
    }
    pub fn exp(x: Dd) -> Dd {
        let k = (x.0 / std::f64::consts::LN_2).round();
        let r = x.sub(LN2.mul(Dd::of(k))).scale(-8);
        let mut e = series_expm1(r).add(Dd::of(1.0));
        for _ in 0..8 {
            e = e.mul(e);
        }
        let k = k as i32;
        e.scale(k / 2).scale(k - k / 2)
    }
    pub fn expm1(x: Dd) -> Dd {
        if x.0.abs() < 0.5 { series_expm1(x) } else { exp(x).sub(Dd::of(1.0)) }
    }
    pub fn ln(x: Dd) -> Dd {
        let mut y = Dd::of(x.0.ln());
        for _ in 0..3 {
            y = y.add(x.mul(exp(y.neg())).sub(Dd::of(1.0)));
        }
        y
    }
    pub fn sqrt(x: Dd) -> Dd {
        if x.0 == 0.0 {
            return x;
        }
        let mut y = Dd::of(x.0.sqrt());
        for _ in 0..3 {
            y = y.add(x.div(y)).scale(-1);
        }
        y
    }
    pub fn sin_cos(x: Dd) -> (Dd, Dd) {
        let half = PI.scale(-1);
        let k = (x.0 / std::f64::consts::FRAC_PI_2).round();
        let r = x.sub(half.mul(Dd::of(k)));
        let (mut s, mut c, mut term) = (Dd::of(0.0), Dd::of(0.0), Dd::of(1.0));
        for n in 0..60 {
            if n % 4 == 0 {
                c = c.add(term);
            } else if n % 4 == 1 {
                s = s.add(term);
            } else if n % 4 == 2 {
                c = c.sub(term);
            } else {
                s = s.sub(term);
            }
            term = term.mul(r).div(Dd::of(f64::from(n + 1)));
        }
        match (k as i64).rem_euclid(4) {
            0 => (s, c),
            1 => (c, s.neg()),
            2 => (s.neg(), c.neg()),
            _ => (c.neg(), s),
        }
    }
    pub fn atan(x: Dd) -> Dd {
        let mut y = Dd::of(x.0.atan());
        for _ in 0..3 {
            let (s, c) = sin_cos(y);
            y = y.sub(s.sub(x.mul(c)).div(c.add(x.mul(s))));
        }
        y
    }
    pub fn asin(x: Dd) -> Dd {
        let mut y = Dd::of(x.0.asin());
        for _ in 0..3 {
            let (s, c) = sin_cos(y);
            if c.0 == 0.0 {
                break;
            }
            y = y.sub(s.sub(x).div(c));
        }
        y
    }
}

use reference::Dd;

/// Every function of one argument the enclosures have, with its reference and where to try it.
fn unary() -> Vec<(&'static str, fn(Interval) -> Proven<Interval>, fn(Dd) -> Dd, (f64, f64))> {
    use reference as r;
    const ONE: Dd = Dd(1.0, 0.0);
    vec![
        ("exp", Real::exp, r::exp, (-700.0, 709.0)),
        ("ln", Real::ln, r::ln, (1e-300, 1e300)),
        ("log10", Real::log10, |x| r::ln(x).div(r::ln(Dd::of(10.0))), (1e-300, 1e300)),
        ("sqrt", Real::sqrt, r::sqrt, (0.0, 1e300)),
        ("sin", Real::sin, |x| r::sin_cos(x).0, (-1e5, 1e5)),
        ("cos", Real::cos, |x| r::sin_cos(x).1, (-1e5, 1e5)),
        (
            "tan",
            Real::tan,
            |x| {
                let (s, c) = r::sin_cos(x);
                s.div(c)
            },
            (-1e5, 1e5),
        ),
        ("asin", Real::asin, r::asin, (-1.0, 1.0)),
        ("acos", Real::acos, |x| r::PI.scale(-1).sub(r::asin(x)), (-1.0, 1.0)),
        ("atan", Real::atan, r::atan, (-1e300, 1e300)),
        ("sinh", Real::sinh, |x| r::expm1(x).sub(r::expm1(x.neg())).scale(-1), (-700.0, 700.0)),
        ("cosh", Real::cosh, |x| r::exp(x).add(r::exp(x.neg())).scale(-1), (-700.0, 700.0)),
        (
            "tanh",
            Real::tanh,
            |x| {
                let e = r::expm1(x.scale(1));
                e.div(e.add(Dd::of(2.0)))
            },
            (-30.0, 30.0),
        ),
        (
            "asinh",
            Real::asinh,
            |x| {
                let a = Dd(x.0.abs(), x.1 * x.0.signum());
                let v = r::ln(a.add(r::sqrt(a.mul(a).add(ONE))));
                if x.0 < 0.0 { v.neg() } else { v }
            },
            (-1e15, 1e15),
        ),
        ("acosh", Real::acosh, |x| r::ln(x.add(r::sqrt(x.sub(ONE).mul(x.add(ONE))))), (1.0, 1e15)),
        ("atanh", Real::atanh, |x| r::ln(ONE.add(x)).sub(r::ln(ONE.sub(x))).scale(-1), (-0.999999, 0.999999)),
    ]
}

/// A deterministic stream of numbers in `[0, 1)`.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Spread over `[lo, hi]`, half the time evenly and half logarithmically in magnitude (with
    /// either sign where the span has both), so small and large magnitudes both come up.
    fn within(&mut self, (lo, hi): (f64, f64)) -> f64 {
        let u = self.next();
        if self.next() < 0.5 {
            return (lo + (hi - lo) * u).clamp(lo, hi);
        }
        let negative = hi <= 0.0 || (lo < 0.0 && self.next() < 0.5);
        let (a, b) = if negative { (hi.min(0.0).abs(), lo.abs()) } else { (lo.max(0.0), hi) };
        let (a, b) = (a.max(1e-12_f64.min(b)), b.max(1e-300));
        let m = (a.ln() + (b.ln() - a.ln()) * u).exp();
        (if negative { -m } else { m }).clamp(lo, hi)
    }
}

/// Whether `want` is in `got`, allowing only for the reference's own error.
fn holds(got: Interval, want: Dd) -> bool {
    let slack = want.0.abs() * 1e-26 + 1e-300;
    got.lo - slack <= want.0 && want.0 <= got.hi + slack
}

/// At single numbers, each function's enclosure holds the reference value, and is a few units
/// wide, so the proofs it carries are tight enough to use.
#[test]
fn every_function_holds_its_value() {
    let mut rng = Lcg(10414);
    for (name, f, want, span) in unary() {
        let mut picks: Vec<f64> = (0..3000).map(|_| rng.within(span)).collect();
        picks.extend(
            [span.0, span.1, 0.5, 1.0, 1e-8, -1e-8, 0.0]
                .into_iter()
                .chain((-40..=40).map(|k| f64::from(k) * 0.37))
                .filter(|x| span.0 <= *x && *x <= span.1),
        );
        for x in picks {
            let got = match f(Interval::point(x)) {
                Ok(i) => i,
                Err(e) => panic!("{name}({x:e}): {e:?}"),
            };
            let w = want(Dd::of(x));
            assert!(holds(got, w), "{name}({x:e}) = {:e} not in {got:?}", w.0);
            let scale = w.0.abs().max(if matches!(name, "exp" | "cosh" | "sqrt" | "sinh" | "ln" | "log10") { 1e-300 } else { 1.0 });
            assert!(got.width() <= scale * 1e-13, "{name}({x:e}): {got:?} is wide");
        }
    }
}

/// Over ranges, each enclosure holds the value at every number in it, the critical points
/// inside (a peak of `sin`, the minimum of `cosh`) and both ends.
#[test]
fn every_function_holds_its_range() {
    let mut rng = Lcg(9410);
    for (name, f, want, span) in unary() {
        for _ in 0..400 {
            let (a, b) = (rng.within(span), rng.within(span));
            let (a, b) = (a.min(b), a.max(b));
            // Narrow ones too, where the function is nearly linear.
            let b = if rng.next() < 0.5 { b.min(a + (b - a) * 1e-6 + a.abs() * 1e-9) } else { b };
            let x = Interval::new(a, b);
            let got = match f(x) {
                Ok(i) => i,
                Err(Unproven::Jump) if name == "tan" => continue,
                Err(e) => panic!("{name}([{a:e}, {b:e}]): {e:?}"),
            };
            for k in 0..=40 {
                let t = a + (b - a) * f64::from(k) / 40.0;
                let w = want(Dd::of(t.clamp(a, b)));
                assert!(holds(got, w), "{name}({t:e}) = {:e} not in {got:?} over [{a:e}, {b:e}]", w.0);
            }
        }
    }
    // The extremes inside.
    let p = |a: f64, b: f64| Interval::new(a, b);
    assert_eq!(Real::sin(p(1.0, 2.0)).map(|i| i.hi), Ok(1.0));
    assert_eq!(Real::cos(p(3.0, 3.3)).map(|i| i.lo), Ok(-1.0));
    assert_eq!(Real::cosh(p(-1.0, 2.0)).map(|i| i.lo), Ok(1.0));
    assert_eq!(Real::abs(p(-1.0, 2.0)), Ok(p(0.0, 2.0)));
    assert_eq!(Real::pow(p(-3.0, 2.0), Interval::point(2.0)).map(|i| i.lo), Ok(0.0));
}

/// The functions of two arguments, over boxes, hold their values at points in them.
#[test]
fn two_argument_functions_hold_their_ranges() {
    use reference as r;
    let mut rng = Lcg(2829);
    let cases: [(&str, fn(Interval, Interval) -> Proven<Interval>, fn(Dd, Dd) -> Option<Dd>, (f64, f64), (f64, f64)); 4] = [
        ("hypot", Real::hypot, |a, b| Some(r::sqrt(a.mul(a).add(b.mul(b)))), (-1e3, 1e3), (-1e3, 1e3)),
        ("pow", Real::pow, |x, e| Some(r::exp(e.mul(r::ln(x)))), (1e-3, 1e3), (-8.0, 8.0)),
        (
            "atan2",
            Real::atan2,
            |y, x| {
                let a = r::atan(y.div(x));
                Some(if x.0 > 0.0 {
                    a
                } else if y.0 >= 0.0 {
                    a.add(r::PI)
                } else {
                    a.sub(r::PI)
                })
            },
            (-1e3, 1e3),
            (0.01, 1e3),
        ),
        ("mod", Real::rem_euclid, |x, y| Some(x.sub(Dd::of(y.0.abs()).mul(Dd::of(x.div(Dd::of(y.0.abs())).0.floor())))), (-1e3, 1e3), (0.5, 7.0)),
    ];
    for (name, f, want, sa, sb) in cases {
        for _ in 0..2000 {
            let (a0, a1, b0, b1) = (rng.within(sa), rng.within(sa), rng.within(sb), rng.within(sb));
            let narrow = rng.next() < 0.7;
            let a = if narrow { Interval::new(a0, a0 + a0.abs() * 1e-7) } else { Interval::new(a0, a1) };
            let b = if narrow { Interval::new(b0, b0 + b0.abs() * 1e-7) } else { Interval::new(b0, b1) };
            let got = match f(a, b) {
                Ok(i) => i,
                Err(Unproven::Jump) if name == "mod" => continue,
                Err(e) => panic!("{name}({a:?}, {b:?}): {e:?}"),
            };
            for (i, j) in [(0.0, 0.0), (1.0, 1.0), (0.3, 0.7), (1.0, 0.0), (0.0, 1.0), (0.5, 0.5)] {
                let (x, y) = ((a.lo + (a.hi - a.lo) * i).clamp(a.lo, a.hi), (b.lo + (b.hi - b.lo) * j).clamp(b.lo, b.hi));
                if let Some(w) = want(Dd::of(x), Dd::of(y)) {
                    assert!(holds(got, w), "{name}({x:e}, {y:e}) = {:e} not in {got:?}", w.0);
                }
            }
        }
    }
    // Whole powers of any sign, and roots.
    for (x, e, v) in [(-2.0, 3.0, -8.0), (-2.0, 2.0, 4.0), (-2.0, -1.0, -0.5), (0.0, 0.0, 1.0), (8.0, 1.0 / 3.0, 2.0)] {
        let got = Real::pow(Interval::point(x), Interval::point(e)).unwrap_or(Interval::ENTIRE);
        assert!(got.contains(v) && got.width() < 1e-14, "{x}^{e}: {got:?}");
    }
}

/// What can't be proven says why: a domain left, a jump crossed, or an argument too large.
#[test]
fn refusals_name_their_reason() {
    let p = |a: f64, b: f64| Interval::new(a, b);
    let up = p(1.0, f64::INFINITY);
    let cases: [(&str, Proven<Interval>, Unproven); 22] = [
        ("sqrt", Real::sqrt(p(-1.0, 1.0)), Unproven::Domain),
        ("ln", Real::ln(p(0.0, 1.0)), Unproven::Domain),
        ("asin", Real::asin(p(0.5, 1.5)), Unproven::Domain),
        ("acosh", Real::acosh(p(0.5, 2.0)), Unproven::Domain),
        ("atanh", Real::atanh(p(0.0, 1.0)), Unproven::Domain),
        ("div", Real::div(p(1.0, 2.0), p(-1.0, 1.0)), Unproven::Domain),
        ("pow", Real::pow(p(-1.0, 1.0), Interval::point(0.5)), Unproven::Domain),
        ("pow neg", Real::pow(p(-1.0, 1.0), Interval::point(-2.0)), Unproven::Domain),
        ("floor", Real::floor(p(0.5, 1.5)), Unproven::Jump),
        ("round", Real::round(p(0.4, 0.6)), Unproven::Jump),
        ("sign", Real::sign(p(-1.0, 1.0)), Unproven::Jump),
        ("sign up to 0", Real::sign(p(-1.0, 0.0)), Unproven::Jump),
        ("mod", Real::rem_euclid(p(0.5, 1.5), Interval::point(1.0)), Unproven::Jump),
        ("tan", Real::tan(p(1.0, 2.0)), Unproven::Jump),
        ("atan2", Real::atan2(p(-1.0, 1.0), p(-2.0, -1.0)), Unproven::Jump),
        ("sin", Real::sin(Interval::point(1e7)), Unproven::Uncertified),
        ("ln unbounded", Real::ln(up), Unproven::Uncertified),
        ("log10 unbounded", Real::log10(up), Unproven::Uncertified),
        ("pow unbounded", Real::pow(up, Interval::point(0.5)), Unproven::Uncertified),
        ("pow neg unbounded", Real::pow(up, Interval::point(-0.5)), Unproven::Uncertified),
        ("pow whole unbounded", Real::pow(up, Interval::point(2.0)), Unproven::Uncertified),
        ("pow varying unbounded", Real::pow(up, p(0.5, 1.5)), Unproven::Uncertified),
    ];
    for (name, got, want) in cases {
        assert_eq!(got, Err(want), "{name}");
    }
    assert_eq!(Real::exp(Interval::point(710.0)), Err(Unproven::Uncertified));
    assert_eq!(Real::floor(p(0.5, 0.9)), Ok(Interval::ZERO));
    assert_eq!(Real::rem_euclid(p(0.5, 0.9), Interval::point(1.0)).map(|i| i.contains(0.7)), Ok(true));
}

/// By the mean value theorem, the slope between any two numbers of a range is the derivative
/// somewhere between them, so the derivative's enclosure over the range holds it. Checked for
/// every function, and for formulas built from them, including kinks.
#[test]
fn derivatives_hold_every_slope() {
    let mut rng = Lcg(6001);
    let formulas: Vec<(&str, fn(Jet<Interval>) -> Proven<Jet<Interval>>, (f64, f64))> = vec![
        ("exp", |x| x.exp(), (-5.0, 5.0)),
        ("ln", |x| x.ln(), (0.1, 50.0)),
        ("log10", |x| x.log10(), (0.1, 50.0)),
        ("sqrt", |x| x.sqrt(), (0.1, 50.0)),
        ("sin", |x| x.sin(), (-10.0, 10.0)),
        ("cos", |x| x.cos(), (-10.0, 10.0)),
        ("tan", |x| x.tan(), (-1.4, 1.4)),
        ("asin", |x| x.asin(), (-0.9, 0.9)),
        ("acos", |x| x.acos(), (-0.9, 0.9)),
        ("atan", |x| x.atan(), (-10.0, 10.0)),
        ("sinh", |x| x.sinh(), (-5.0, 5.0)),
        ("cosh", |x| x.cosh(), (-5.0, 5.0)),
        ("tanh", |x| x.tanh(), (-5.0, 5.0)),
        ("asinh", |x| x.asinh(), (-10.0, 10.0)),
        ("acosh", |x| x.acosh(), (1.1, 10.0)),
        ("atanh", |x| x.atanh(), (-0.9, 0.9)),
        ("abs", |x| x.abs(), (-3.0, 3.0)),
        ("x^3", |x| x.pow(Jet::constant(Interval::point(3.0))?), (-3.0, 3.0)),
        ("x^-2", |x| x.pow(Jet::constant(Interval::point(-2.0))?), (0.2, 3.0)),
        ("x^1.5", |x| x.pow(Jet::constant(Interval::point(1.5))?), (0.2, 3.0)),
        ("x^x", |x| x.pow(x), (0.2, 3.0)),
        ("hypot", |x| x.hypot(Jet::exact(1.0)?), (-3.0, 3.0)),
        ("hypot at 0", |x| x.hypot(x.mul(Jet::exact(2.0)?)?), (-3.0, 3.0)),
        ("atan2", |x| Jet::exact(1.0)?.atan2(x), (-3.0, 3.0)),
        ("min", |x| x.min(x.mul(x)?), (-2.0, 2.0)),
        ("max", |x| x.sin()?.max(x.cos()?), (-3.0, 3.0)),
        ("mod", |x| x.rem_euclid(Jet::exact(0.7)?), (-3.0, 3.0)),
        ("mod by x", |x| Jet::exact(5.0)?.rem_euclid(x), (1.1, 1.9)),
        ("hypex", |x| Jet::exact(40.0)?.mul(x.mul(x)?.add(Jet::exact(1.0)?)?.sqrt()?)?.div(x.add(Jet::exact(5.0)?)?), (-4.0, 4.0)),
        ("sin(64 pi x)", |x| x.mul(Jet::exact(64.0 * std::f64::consts::PI)?)?.sin(), (0.0, 1.0)),
    ];
    let value = |f: fn(Jet<Interval>) -> Proven<Jet<Interval>>, t: f64| f(Jet::variable(Interval::point(t))?).map(|j| j.v);
    for (name, f, span) in formulas {
        for _ in 0..300 {
            let (a, b) = (rng.within(span), rng.within(span));
            let x = Interval::new(a.min(b), a.max(b));
            let Ok(jet) = f(Jet::variable(x).unwrap_or(Jet { v: x, d: Interval::ONE })) else { continue };
            for _ in 0..8 {
                let (s, t) = (x.lo + (x.hi - x.lo) * rng.next(), x.lo + (x.hi - x.lo) * rng.next());
                if (t - s).abs() < (x.hi - x.lo) * 1e-3 || (t - s).abs() < 1e-9 {
                    continue;
                }
                let (Ok(fs), Ok(ft)) = (value(f, s), value(f, t)) else { continue };
                // The secant's slopes over the enclosures of both ends hold the true one.
                let slope = (ft - fs) / (Interval::point(t) - Interval::point(s));
                assert!(slope.overlaps(jet.d), "{name} over {x:?}: slope {slope:?} outside {:?}", jet.d);
            }
            assert!(jet.v.overlaps(value(f, x.mid()).unwrap_or(jet.v)), "{name}");
        }
    }
}

/// A square inside a derivative is taken from `|x|`, so a range across zero keeps the derivative
/// away from zero where it truly is: over [−2, 2], atan' = 1/(1 + x²) is in [1/5, 1], asinh' =
/// 1/√(1 + x²) in [1/√5, 1], and d atan2(1, x)/dx = −1/(1 + x²) in [−1, −1/5].
#[test]
fn derivatives_across_zero_square_the_magnitude() {
    let x = || Jet::variable(Interval::new(-2.0, 2.0));
    let cases: [(&str, Proven<Jet<Interval>>, (f64, f64)); 3] = [
        ("atan", x().and_then(|x| x.atan()), (0.2, 1.0)),
        ("asinh", x().and_then(|x| x.asinh()), (1.0 / 5f64.sqrt(), 1.0)),
        ("atan2", x().and_then(|x| Jet::exact(1.0)?.atan2(x)), (-1.0, -0.2)),
    ];
    for (name, jet, (lo, hi)) in cases {
        let d = jet.map(|j| j.d).unwrap_or(Interval::ENTIRE);
        assert!(d.contains(lo) && d.contains(hi) && d.width() < (hi - lo) * 1.01, "{name}: {d:?}");
    }
}

/// An unbounded interval still has a finite number inside to split at, so its halves keep
/// bounds that are not NaN.
#[test]
fn unbounded_intervals_split_at_a_number_they_hold() {
    for i in [Interval::ENTIRE, Interval::new(1.0, f64::INFINITY), Interval::new(f64::NEG_INFINITY, -1.0)] {
        let m = i.mid();
        assert!(m.is_finite() && i.contains(m), "{i:?}: {m}");
        let (a, b) = i.bisect();
        assert!(!a.lo.is_nan() && !a.hi.is_nan() && !b.lo.is_nan() && !b.hi.is_nan(), "{i:?}");
        assert_eq!((a.lo, a.hi, b.lo, b.hi), (i.lo, m, m, i.hi));
    }
    assert_eq!(Interval::ENTIRE.mid(), 0.0);
}

/// `f64` gives the standard library's results and never refuses: code generic over [`Real`]
/// computes what it did before it was generic.
#[test]
fn plain_numbers_are_the_standard_librarys() {
    let x = 0.7f64;
    assert_eq!(Real::sin(x), Ok(x.sin()));
    assert_eq!(Real::pow(x, 2.5), Ok(x.powf(2.5)));
    assert_eq!(Real::rem_euclid(-1e-20, 1.0), Ok(1.0));
    assert_eq!(Real::sign(-0.0f64), Ok(0.0));
    assert_eq!(Real::sign(Interval::ZERO), Ok(Interval::ZERO));
    assert_eq!(Real::sign(Interval::new(0.5, 2.0)), Ok(Interval::ONE));
    assert_eq!(Real::min(2.0f64, 1.0), Ok(1.0));
    assert_eq!(Real::cmp_f64(x, 1.0), Some(Ordering::Less));
    // A kink in a plain number takes one slope, not a refusal.
    assert_eq!(Jet::variable(0.0f64).and_then(|x| x.abs()).map(|j| j.d), Ok(1.0));
}

/// A power whose exponent varies takes the exponent's derivative even where the exponent's value
/// is a single number: d(2ˣ)/dx = 2ˣ ln 2 at x = 0 and x = 1, not the fixed-power rule's 0 or 1.
#[test]
fn a_varying_exponent_keeps_its_derivative() {
    use std::f64::consts::LN_2;
    for (x, want) in [(0.0, LN_2), (1.0, 2.0 * LN_2)] {
        let jet = Jet::exact(2.0).and_then(|two: Jet<Interval>| two.pow(Jet::variable(Interval::point(x))?));
        let d = jet.map(|j| j.d).unwrap_or(Interval::ENTIRE);
        assert!(d.contains(want) && d.width() < 1e-14, "d(2^x) at {x}: {d:?}");
    }
    // A fixed exponent still uses the power rule: d(x²) = 2x.
    let sq = Jet::variable(Interval::point(3.0)).and_then(|x| x.pow(Jet::exact(2.0)?)).map(|j| j.d);
    assert_eq!(sq, Ok(Interval::point(6.0)));
}
