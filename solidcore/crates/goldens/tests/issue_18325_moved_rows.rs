//! #18325: every golden element the exact iso-line drop moves is CLOSER to the exact value
//! of the ideal pipeline than main's, EQUAL to main's, or EQUAL-at-rounding: within the
//! a-priori bound of the kernel's own float evaluation. No element is FARTHER.
//!
//! The fix makes `drop_curve` return the iso line `(f, t)` of a B-spline surface, with
//! ends exactly `(f, t0)` and `(f, t1)`, when the curve is that iso line. It moves 2429
//! elements of `topology.edgeuse_p2` (42), `topology.edgeuse_p3` (2331) and
//! `topology.face_p3` (56). `data/issue_18325_moved_rows.tsv` lists them (`E unit case key
//! i main new class`, main's and the new golden's values and the pinned class), with the
//! stations the drop quality maxima are taken over (`DC`/`DU` the edge's 3D curve or the UV
//! line of its `CrvOnSurf`, `DI` the `drop_iso_curve` stations, `DG` the
//! `surface_curve_max_distance_between` stations, as the kernel evaluated them), the cached
//! edge/face gap stations (`G`) and the blade hub face's surface (`S`).
//!
//! The test replays every listed case through the goldens runner. Each element must
//! reproduce its new value bit for bit. It then gets its certificate and class:
//!
//! * The reference `r` is the value of the ideal pipeline, the exact iso line `(f, t)`:
//!   `f` the face's domain side, `t` the parameter the op evaluates (computed by the kernel
//!   as on main: the trim fix does not touch it). Trim control points and samples, uv
//!   points and derivatives have exact double references. Surface values at `(f, t)`
//!   (normal, binormal, uv vector, `CrvOnSurf` point and derivative, lifted samples) are
//!   enclosed in narrow outward-rounded intervals of the exact rational value.
//! * CLOSER: `|new − r| < |main − r|`, decided exactly (dyadic arithmetic) for a double
//!   reference, or `r`'s whole enclosure on new's side of the midpoint of new and main.
//! * EQUAL: `new == main`, or the two at the same exact distance from `r`.
//! * EQUAL-at-rounding: `new` lies in the a-priori enclosure `E` of the kernel's own float
//!   result, so `|new − r| <= B = sup_{x in E} |x − r|`, with `B = L e_in + γ_k M`:
//!   - a degree-1 trim evaluation: `B = 64 ε max(|f|, |t0|, |t1|)` (`nurbs_crv.rs`
//!     `crv_point_error`);
//!   - a surface evaluation at the record's own uv: the exact homogeneous sums widened by
//!     `γ_K Σ|terms|`, `K = 9p + 9q + (p+1)(q+1) + 8` (the basis recurrences, 3 roundings
//!     per level and the derivative scaling per direction, a Bézier extraction allowance
//!     `3p + 3q`, the tensor sum, 2 products per term), then one rounding per operation of
//!     the rational quotient; a cross product `γ_2` per component, a dot product of `n`
//!     terms `γ_n`, a normalisation `γ_3` (dot), `u` (square root) and `γ_2` (division),
//!     the uv-vector solve `γ_2` per 2×2 minor and `u` per division;
//!   - a `CrvOnSurf` point or derivative at `(f, t)`: `L e_in + γ_K M` with `L` the hull
//!     bound of the surface's first (second, for a derivative) derivative net, `e_in` the
//!     trim evaluation's `64 ε M` plus the parameter's own `γ_3 (|lo| + |hi|)`, `M` the net's
//!     largest coordinate;
//!   - a drop quality value (a maximum over stations): every station's `|dist|`, `dev`,
//!     `line_param` and `drop_line_dist` enclosed from the station's own uv through the
//!     float surface, normal and difference chain (each value of the dump checked inside
//!     its enclosure), the reported value the maximum, or the kernel's `sqrt(d² + v²)` with
//!     one rounding per operation. The reference is the exact maximum over the same
//!     stations of the ideal pipeline: at `(f, t)` the exact split of `C(s) − S(f, t)`
//!     along and across the normal, at a gap station at the curve's certified closest
//!     approach (a sign change of the projection's derivative bracketed in interval
//!     arithmetic, widened by the curve's Lipschitz bound times the bracket). The
//!     enclosure is taken at each station's own uv, so it bounds the rounding only when
//!     that uv is `(f, param)` within the trim evaluation's `64 ε M` (checked per station).
//!     `|max x − max r| <= max_j sup |E_j − R_j|`; a station off the iso line (on the
//!     drop's own Newton line, as on main) keeps the at-rounding class only if it cannot
//!     set the maximum or its own `sup |E_j − R_j|` is within the on-iso stations' largest
//!     (else the element must be CLOSER or EQUAL). At a gap station `e_in` also holds the
//!     closest-point solver's residual in the curve parameter (the kernel's `s` against
//!     the certified `s*`): the same solver as on main;
//!   - a cached edge/face gap (`G`): `|S(trim(t)) − S(uv)|` at the closest-point solver's
//!     uv, the reference `0` (the ideal `CrvOnSurf` is `S(f, ·)` itself); `e_in` is the
//!     solver's uv residual, the same solver on main. `eu1`/`eu7` of the goc cases are
//!     at-rounding against `0` for that reason, `eu2`/`eu6` CLOSER.
//! * A binormal row's vertex/face-gap-cached flag (`1 → 0`): the reference is `0`. Only
//!   `sm_FixUVTrimCurves` computes that gap on this path, and only when the trims meeting
//!   at the vertex miss each other by more than `EFF_ZERO (1 + |uv|)`; the ideal trims, the
//!   exact iso lines of the domain sides, meet exactly at the corner.
//! * FARTHER: none of the above, a defect. The pinned classes hold none.
//!
//! Run: `cargo test --profile goldens -p goldens --test issue_18325_moved_rows -- --nocapture`
//! (prints the table: element classes per unit and kind, the largest `|new − r| / B` of the
//! at-rounding elements, and the row classes).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::print_stderr,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::suboptimal_flops,
    missing_docs
)] // test

use std::collections::BTreeMap;

use goldens::case::repo_root;
use goldens::{registry, CaseContext};
use serde_json::Value;

const DATA: &str = include_str!("data/issue_18325_moved_rows.tsv");

const UNITS: &[&str] = &[
    "topology.edgeuse_p2",
    "topology.edgeuse_p3",
    "topology.face_p3",
];

const BLADE: &str = "own_reader_blade_hub_create_eu36_rebuild";

// ---- exact dyadic comparison ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// `|new − ref| < |main − ref|` (or `<=` with the two different).
    Closer,
    /// `new == main`, or the two equally far from an exact reference.
    Equal,
    /// Neither nearer: `new` lies in the a-priori enclosure `E` of the kernel's own float
    /// result, so `|new − ref| <= B = sup_{x in E} |x − ref|` (see the module docs).
    AtRounding,
    /// None of the above: a defect.
    Farther,
}

/// An exact dyadic rational `m 2^e`, wide enough for the differences of two doubles of
/// nearby magnitude.
#[derive(Debug, Clone, Copy)]
struct Dy {
    m: i128,
    e: i32,
}

impl Dy {
    fn f(x: f64) -> Dy {
        assert!(x.is_finite(), "{x}");
        if x == 0.0 {
            return Dy { m: 0, e: 0 };
        }
        let bits = x.to_bits();
        let sign = if bits >> 63 == 1 { -1 } else { 1 };
        let exp = ((bits >> 52) & 0x7ff) as i32;
        let frac = i128::from(bits & ((1 << 52) - 1));
        let (m, e) = if exp == 0 {
            (frac, -1074)
        } else {
            (frac | (1 << 52), exp - 1075)
        };
        Dy { m: sign * m, e }.norm()
    }
    fn norm(self) -> Dy {
        if self.m == 0 {
            return Dy { m: 0, e: 0 };
        }
        let z = self.m.trailing_zeros();
        Dy {
            m: self.m >> z,
            e: self.e + z as i32,
        }
    }
    /// `|a − b|` against `|c − d|` exactly (all four doubles), or `None` when the exponents
    /// are too far apart for `i128` (then the caller falls back to intervals).
    fn cmp_dist(a: f64, b: f64, c: f64, d: f64) -> Option<std::cmp::Ordering> {
        let xs = [Dy::f(a), Dy::f(b), Dy::f(c), Dy::f(d)];
        let e = xs
            .iter()
            .filter(|x| x.m != 0)
            .map(|x| x.e)
            .min()
            .unwrap_or(0);
        let mut ms = [0i128; 4];
        for (k, x) in xs.iter().enumerate() {
            if x.m == 0 {
                continue;
            }
            let s = u32::try_from(x.e - e).ok()?;
            if s >= 120 || x.m.unsigned_abs().leading_zeros() <= s + 2 {
                return None;
            }
            ms[k] = x.m << s;
        }
        let l = ms[0].checked_sub(ms[1])?.checked_abs()?;
        let r = ms[2].checked_sub(ms[3])?.checked_abs()?;
        Some(l.cmp(&r))
    }
}

// ---- outward-rounded intervals --------------------------------------------------------------

/// A closed interval of doubles; every operation rounds to nearest and then widens one ulp
/// each way, so it holds the exact result for all members of its operands.
#[derive(Debug, Clone, Copy)]
struct Iv {
    lo: f64,
    hi: f64,
}

const U: f64 = 1.0 / 9_007_199_254_740_992.0; // 2^-53, the unit roundoff

impl Iv {
    fn p(x: f64) -> Iv {
        Iv { lo: x, hi: x }
    }
    fn out(lo: f64, hi: f64) -> Iv {
        Iv {
            lo: lo.next_down(),
            hi: hi.next_up(),
        }
    }
    fn hull(a: Iv, b: Iv) -> Iv {
        Iv {
            lo: a.lo.min(b.lo),
            hi: a.hi.max(b.hi),
        }
    }
    fn add(self, o: Iv) -> Iv {
        Iv::out(self.lo + o.lo, self.hi + o.hi)
    }
    fn sub(self, o: Iv) -> Iv {
        Iv::out(self.lo - o.hi, self.hi - o.lo)
    }
    fn neg(self) -> Iv {
        Iv {
            lo: -self.hi,
            hi: -self.lo,
        }
    }
    fn mul(self, o: Iv) -> Iv {
        let c = [
            self.lo * o.lo,
            self.lo * o.hi,
            self.hi * o.lo,
            self.hi * o.hi,
        ];
        Iv::out(
            c.iter().copied().fold(f64::INFINITY, f64::min),
            c.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn div(self, o: Iv) -> Iv {
        assert!(
            o.lo > 0.0 || o.hi < 0.0,
            "division by an interval holding 0: {o:?}"
        );
        let c = [
            self.lo / o.lo,
            self.lo / o.hi,
            self.hi / o.lo,
            self.hi / o.hi,
        ];
        Iv::out(
            c.iter().copied().fold(f64::INFINITY, f64::min),
            c.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn sqrt(self) -> Iv {
        let lo = self.lo.max(0.0);
        assert!(self.hi >= 0.0, "sqrt of {self:?}");
        Iv::out(lo.sqrt(), self.hi.sqrt()).clamp0()
    }
    fn clamp0(self) -> Iv {
        Iv {
            lo: self.lo.max(0.0),
            hi: self.hi,
        }
    }
    fn abs(self) -> Iv {
        if self.lo >= 0.0 {
            self
        } else if self.hi <= 0.0 {
            self.neg()
        } else {
            Iv {
                lo: 0.0,
                hi: (-self.lo).max(self.hi),
            }
        }
    }
    /// The largest magnitude (exact: no rounding).
    fn mag(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    /// Widened by `r >= 0` each way.
    fn widen(self, r: f64) -> Iv {
        assert!(r >= 0.0);
        Iv::out(self.lo - r, self.hi + r)
    }
    /// One kernel rounding chain of relative size `g`: widened by `g |x|`.
    fn rel(self, g: f64) -> Iv {
        self.widen(up_mul(g, self.mag()))
    }
    fn contains(self, x: f64) -> bool {
        self.lo <= x && x <= self.hi
    }
}

/// `a b` rounded up (`a, b >= 0`).
fn up_mul(a: f64, b: f64) -> f64 {
    Iv::p(a).mul(Iv::p(b)).hi
}
/// `a + b` rounded up (`a, b >= 0`).
fn up_add(a: f64, b: f64) -> f64 {
    Iv::p(a).add(Iv::p(b)).hi
}

/// `γ_n = n u / (1 − n u)`, rounded up: the relative error bound of a chain of `n` roundings.
fn gam(n: usize) -> f64 {
    let nu = Iv::p(n as f64).mul(Iv::p(U));
    nu.div(Iv::p(1.0).sub(nu)).hi
}

type V3 = [Iv; 3];

/// A float dot product of `n` terms: the exact sum widened by `γ_n Σ |terms|`.
fn kdot(a: &V3, b: &V3, n: usize) -> Iv {
    let mut s = Iv::p(0.0);
    let mut m = 0.0;
    for (x, y) in a.iter().zip(b) {
        let t = x.mul(*y);
        s = s.add(t);
        m = up_add(m, t.mag());
    }
    s.widen(up_mul(gam(n), m))
}

/// A float cross product: each component's two products and difference, `γ_2`.
fn kcross(a: &V3, b: &V3) -> V3 {
    let comp = |i: usize, j: usize| {
        let p = a[i].mul(b[j]);
        let q = a[j].mul(b[i]);
        p.sub(q).widen(up_mul(gam(2), up_add(p.mag(), q.mag())))
    };
    [comp(1, 2), comp(2, 0), comp(0, 1)]
}

/// A float normalisation `c / |c|`: the dot (`γ_3`), the square root (`u`), the division (`γ_2`,
/// which also covers a reciprocal-and-multiply).
fn kunit(c: &V3) -> V3 {
    let q = kdot(c, c, 3);
    let ln = q.sqrt().rel(U);
    [
        c[0].div(ln).rel(gam(2)),
        c[1].div(ln).rel(gam(2)),
        c[2].div(ln).rel(gam(2)),
    ]
}

fn cross(a: &V3, b: &V3) -> V3 {
    [
        a[1].mul(b[2]).sub(a[2].mul(b[1])),
        a[2].mul(b[0]).sub(a[0].mul(b[2])),
        a[0].mul(b[1]).sub(a[1].mul(b[0])),
    ]
}

fn dot(a: &V3, b: &V3) -> Iv {
    a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
}

fn sub3(a: &V3, b: &V3) -> V3 {
    [a[0].sub(b[0]), a[1].sub(b[1]), a[2].sub(b[2])]
}

// ---- B-spline bases, surfaces and curves ----------------------------------------------------

/// The degree-`p` basis functions over `knots` at the double `t`, their first derivatives
/// and the sums `|terms|` of each derivative (enclosures of the exact values). The span is
/// right-continuous (the last nonempty one at the end, the first before the start), as the
/// kernel's.
fn basis(knots: &[f64], p: usize, tt: Iv) -> (Vec<Iv>, Vec<Iv>, Vec<Iv>) {
    let n = knots.len() - p - 1;
    let t = tt.lo;
    let span = (p..n)
        .rev()
        .find(|&i| knots[i] <= t && t < knots[i + 1])
        .unwrap_or(if t < knots[p] { p } else { n - 1 });
    // the whole interval in that span's polynomial piece
    assert!(
        tt.hi == tt.lo || tt.hi <= knots[span + 1] || span == n - 1,
        "parameter {tt:?} straddles a knot"
    );
    let zero = Iv::p(0.0);
    let k = |i: usize| Iv::p(knots[i]);
    let mut nb: Vec<Iv> = (0..knots.len() - 1)
        .map(|i| if i == span { Iv::p(1.0) } else { zero })
        .collect();
    let mut lower = vec![zero; knots.len()];
    for d in 1..=p {
        lower.clone_from(&nb);
        let mut next = vec![zero; knots.len() - 1 - d];
        for (i, slot) in next.iter_mut().enumerate() {
            let mut acc = zero;
            if knots[i + d] != knots[i] {
                acc = acc.add(tt.sub(k(i)).div(k(i + d).sub(k(i))).mul(nb[i]));
            }
            if knots[i + d + 1] != knots[i + 1] {
                acc = acc.add(
                    k(i + d + 1)
                        .sub(tt)
                        .div(k(i + d + 1).sub(k(i + 1)))
                        .mul(nb[i + 1]),
                );
            }
            *slot = acc;
        }
        nb = next;
    }
    let pf = Iv::p(p as f64);
    let mut der = Vec::with_capacity(n);
    let mut dab = Vec::with_capacity(n);
    for i in 0..n {
        let mut a = zero;
        let mut b = zero;
        if p > 0 {
            if knots[i + p] != knots[i] {
                let x = pf.mul(lower[i]).div(k(i + p).sub(k(i)));
                a = a.add(x);
                b = b.add(x.abs());
            }
            if knots[i + p + 1] != knots[i + 1] {
                let x = pf.mul(lower[i + 1]).div(k(i + p + 1).sub(k(i + 1)));
                a = a.sub(x);
                b = b.add(x.abs());
            }
        }
        der.push(a);
        dab.push(b);
    }
    nb.truncate(n);
    (nb, der, dab)
}

fn num(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

fn nums(v: &Value) -> Vec<f64> {
    v.as_array().expect("array").iter().map(num).collect()
}

/// A B-spline surface of a case's input (`bspline`: homogeneous control net `net[i][j] =
/// [w x, w y, w z, w]`, `w = −MAX` for a non-rational net; `i` along `u`).
struct Srf {
    net: Vec<Vec<[f64; 4]>>,
    rational: bool,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// `S`, `S_u`, `S_v`.
type Eval = (V3, V3, V3);

impl Srf {
    fn from_input(inputs: &Value) -> Srf {
        let b = &inputs["brep"]["surface"]["bspline"];
        let net: Vec<Vec<[f64; 4]>> = b["net"]
            .as_array()
            .expect("net")
            .iter()
            .map(|row| {
                row.as_array()
                    .expect("row")
                    .iter()
                    .map(|cp| {
                        let c = nums(cp);
                        [c[0], c[1], c[2], c[3]]
                    })
                    .collect()
            })
            .collect();
        let rational = net.iter().flatten().any(|cp| cp[3] > -1e300);
        Srf {
            net,
            rational,
            p: b["p"].as_u64().expect("p") as usize,
            q: b["q"].as_u64().expect("q") as usize,
            u: nums(&b["u"]),
            v: nums(&b["v"]),
        }
    }

    /// The homogeneous sums at `(u, v)` and their `|terms|` sums: `(A, A_u, A_v)` and
    /// `(|A|, |A_u|, |A_v|)`.
    fn sums(&self, u: Iv, v: Iv) -> ([[Iv; 4]; 3], [[f64; 4]; 3]) {
        let (nu, du, dua) = basis(&self.u, self.p, u);
        let (nv, dv, dva) = basis(&self.v, self.q, v);
        let mut a = [[Iv::p(0.0); 4]; 3];
        let mut m = [[0.0f64; 4]; 3];
        for (i, row) in self.net.iter().enumerate() {
            for (j, cp) in row.iter().enumerate() {
                for c in 0..4 {
                    let x = if c == 3 && !self.rational { 1.0 } else { cp[c] };
                    let xi = Iv::p(x);
                    let ax = Iv::p(x.abs());
                    let terms = [
                        (nu[i].mul(nv[j]), nu[i].mul(nv[j]).abs()),
                        (du[i].mul(nv[j]), dua[i].mul(nv[j].abs())),
                        (nu[i].mul(dv[j]), nu[i].abs().mul(dva[j])),
                    ];
                    for (d, (w, wa)) in terms.iter().enumerate() {
                        a[d][c] = a[d][c].add(w.mul(xi));
                        m[d][c] = up_add(m[d][c], wa.mul(ax).hi);
                    }
                }
            }
        }
        (a, m)
    }

    /// The exact `S, S_u, S_v` anywhere in `(u, v)` (narrow intervals), enclosed.
    fn exact(&self, u: Iv, v: Iv) -> Eval {
        let (a, _) = self.sums(u, v);
        self.quotient(a, false)
    }

    /// An enclosure of the kernel's float `S, S_u, S_v` at the doubles `(u, v)`: the exact
    /// sums widened by `γ_K Σ|terms|`, `K = 9p + 9q + (p+1)(q+1) + 8` (the basis recurrences,
    /// 3 roundings per level and the derivative scaling, per direction; a Bézier extraction
    /// allowance of `3p + 3q`; the tensor sum of `(p+1)(q+1)` terms; 2 products per term),
    /// then the rational quotient one rounding per operation.
    fn kernel(&self, u: f64, v: f64) -> Eval {
        let (mut a, m) = self.sums(Iv::p(u), Iv::p(v));
        let g = gam(9 * self.p + 9 * self.q + (self.p + 1) * (self.q + 1) + 8);
        for d in 0..3 {
            for c in 0..4 {
                a[d][c] = a[d][c].widen(up_mul(g, m[d][c]));
            }
        }
        self.quotient(a, true)
    }

    fn quotient(&self, a: [[Iv; 4]; 3], round: bool) -> Eval {
        let r = |x: Iv| if round { x.rel(U) } else { x };
        if !self.rational {
            let f = |d: usize| [a[d][0], a[d][1], a[d][2]];
            return (f(0), f(1), f(2));
        }
        let w = a[0][3];
        let s: V3 = std::array::from_fn(|c| r(a[0][c].div(w)));
        let der = |d: usize| -> V3 {
            std::array::from_fn(|c| r(r(a[d][c].sub(r(a[d][3].mul(s[c])))).div(w)))
        };
        (s, der(1), der(2))
    }
}

/// A dumped B-spline curve (`deg`, the full knot vector, homogeneous control points
/// `[w x, w y, w z, w]`).
#[derive(Debug, Clone)]
struct Crv {
    p: usize,
    knots: Vec<f64>,
    cps: Vec<[f64; 4]>,
}

impl Crv {
    fn rational(&self) -> bool {
        self.cps.iter().any(|c| c[3] != 1.0)
    }
    /// The exact `C(s)` and `C'(s)` at the double `s`, enclosed.
    fn exact(&self, s: f64) -> (V3, V3) {
        let (n, d, _) = basis(&self.knots, self.p, Iv::p(s));
        let mut a = [Iv::p(0.0); 4];
        let mut ad = [Iv::p(0.0); 4];
        for (i, cp) in self.cps.iter().enumerate() {
            for q in 0..4 {
                a[q] = a[q].add(n[i].mul(Iv::p(cp[q])));
                ad[q] = ad[q].add(d[i].mul(Iv::p(cp[q])));
            }
        }
        let w = a[3];
        let c: V3 = std::array::from_fn(|q| a[q].div(w));
        let cd: V3 = std::array::from_fn(|q| ad[q].sub(ad[3].mul(c[q])).div(w));
        (c, cd)
    }
    /// An enclosure of the kernel's float `C(s)`: the exact sums widened by `γ_K Σ|terms|`,
    /// `K = 9p + (p+1) + 8`, then the rational division (`γ_2`).
    fn kernel(&self, s: f64) -> V3 {
        let (n, _, _) = basis(&self.knots, self.p, Iv::p(s));
        let mut a = [Iv::p(0.0); 4];
        let mut m = [0.0f64; 4];
        for (i, cp) in self.cps.iter().enumerate() {
            for q in 0..4 {
                a[q] = a[q].add(n[i].mul(Iv::p(cp[q])));
                m[q] = up_add(m[q], n[i].abs().mul(Iv::p(cp[q].abs())).hi);
            }
        }
        let g = gam(9 * self.p + (self.p + 1) + 8);
        let a: [Iv; 4] = std::array::from_fn(|q| a[q].widen(up_mul(g, m[q])));
        if !self.rational() {
            return [a[0], a[1], a[2]];
        }
        std::array::from_fn(|q| a[q].div(a[3]).rel(gam(2)))
    }
    /// A bound on `|C'|` over the curve: `2 p max|P| / h_min` (non-rational), times
    /// `4 w_max² / w_min²` for a rational curve.
    fn lipschitz(&self) -> f64 {
        let mx = self
            .cps
            .iter()
            .flat_map(|c| c[..3].iter().map(|x| x.abs()))
            .fold(0.0, f64::max);
        let hmin = self
            .knots
            .windows(2)
            .filter(|w| w[1] > w[0])
            .map(|w| Iv::p(w[1]).sub(Iv::p(w[0])).lo)
            .fold(f64::INFINITY, f64::min);
        let mut l = Iv::p(mx)
            .mul(Iv::p(2.0 * self.p as f64))
            .div(Iv::p(hmin))
            .hi;
        if self.rational() {
            let wmax = self.cps.iter().map(|c| c[3]).fold(0.0, f64::max);
            let wmin = self.cps.iter().map(|c| c[3]).fold(f64::INFINITY, f64::min);
            let wm = Iv::p(wmax);
            let wn = Iv::p(wmin);
            l = Iv::p(l).mul(Iv::p(4.0)).mul(wm.mul(wm)).div(wn.mul(wn)).hi;
        }
        l
    }
}

/// Hull bounds of a non-rational surface: `max|P|`, `max|S_u|`, `max|S_v|` and the second
/// derivative nets' maxima per coordinate (the derivative nets of the control net), and the
/// shortest nonempty knot spans.
struct Bounds {
    m: [f64; 3],
    lu: [f64; 3],
    lv: [f64; 3],
    luu: [f64; 3],
    luv: [f64; 3],
    lvv: [f64; 3],
    hu: f64,
    hv: f64,
}

fn deriv_net(net: &[Vec<[Iv; 3]>], kn: &[f64], p: usize, along_u: bool) -> Vec<Vec<[Iv; 3]>> {
    let pf = Iv::p(p as f64);
    if along_u {
        (0..net.len() - 1)
            .map(|i| {
                (0..net[0].len())
                    .map(|j| {
                        let d = Iv::p(kn[i + p + 1]).sub(Iv::p(kn[i + 1]));
                        std::array::from_fn(|c| pf.mul(net[i + 1][j][c].sub(net[i][j][c])).div(d))
                    })
                    .collect()
            })
            .collect()
    } else {
        (0..net.len())
            .map(|i| {
                (0..net[0].len() - 1)
                    .map(|j| {
                        let d = Iv::p(kn[j + p + 1]).sub(Iv::p(kn[j + 1]));
                        std::array::from_fn(|c| pf.mul(net[i][j + 1][c].sub(net[i][j][c])).div(d))
                    })
                    .collect()
            })
            .collect()
    }
}

fn hull_max(net: &[Vec<[Iv; 3]>]) -> [f64; 3] {
    std::array::from_fn(|c| {
        net.iter()
            .flatten()
            .map(|cp| cp[c].mag())
            .fold(0.0, f64::max)
    })
}

impl Srf {
    fn bounds(&self) -> Bounds {
        assert!(!self.rational, "hull bounds are for non-rational nets");
        let net: Vec<Vec<[Iv; 3]>> = self
            .net
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| [Iv::p(c[0]), Iv::p(c[1]), Iv::p(c[2])])
                    .collect()
            })
            .collect();
        let pu = deriv_net(&net, &self.u, self.p, true);
        let pv = deriv_net(&net, &self.v, self.q, false);
        let ui = &self.u[1..self.u.len() - 1];
        let vi = &self.v[1..self.v.len() - 1];
        let hmin = |kn: &[f64]| {
            kn.windows(2)
                .filter(|w| w[1] > w[0])
                .map(|w| Iv::p(w[1]).sub(Iv::p(w[0])).lo)
                .fold(f64::INFINITY, f64::min)
        };
        Bounds {
            m: hull_max(&net),
            lu: hull_max(&pu),
            lv: hull_max(&pv),
            luu: hull_max(&deriv_net(&pu, ui, self.p - 1, true)),
            luv: hull_max(&deriv_net(&pu, &self.v, self.q, false)),
            lvv: hull_max(&deriv_net(&pv, vi, self.q - 1, false)),
            hu: hmin(&self.u),
            hv: hmin(&self.v),
        }
    }
}

/// An enclosure of the kernel's float `CrvOnSurf` point `S(trim(s))`, `trim` a degree-1 UV
/// line: the exact surface point at the exact line point, widened by the line evaluation's
/// error `64 ε max|cp|` through the hull Lipschitz bounds, and by the surface rounding
/// `γ_K max|P|`.
fn cos_point(srf: &Srf, b: &Bounds, trim: &Crv, s: f64) -> V3 {
    assert!(trim.p == 1 && trim.cps.len() == 2 && !trim.rational());
    let (t0, t1) = (trim.knots[0], trim.knots[trim.knots.len() - 1]);
    let a = Iv::p(s).sub(Iv::p(t0)).div(Iv::p(t1).sub(Iv::p(t0)));
    let uv: [Iv; 2] = std::array::from_fn(|q| {
        Iv::p(trim.cps[0][q]).add(a.mul(Iv::p(trim.cps[1][q]).sub(Iv::p(trim.cps[0][q]))))
    });
    let mx = trim
        .cps
        .iter()
        .flat_map(|c| [c[0].abs(), c[1].abs()])
        .fold(0.0, f64::max);
    let e = up_mul(64.0 * f64::EPSILON, mx);
    // the exact surface point over the line point's enclosure, widened through the hull
    // Lipschitz bounds by the evaluation error, and by the surface's rounding
    let (p, _, _) = srf.exact(uv[0], uv[1]);
    let g = gam(9 * srf.p + 9 * srf.q + (srf.p + 1) * (srf.q + 1) + 8);
    std::array::from_fn(|q| {
        let r = up_add(
            up_add(up_mul(b.lu[q], e), up_mul(b.lv[q], e)),
            up_mul(g, b.m[q]),
        );
        p[q].widen(r)
    })
}

// ---- certificates ---------------------------------------------------------------------------

/// What an element is checked against.
#[derive(Debug, Clone)]
enum Cert {
    /// An exact reference (a double), and the a-priori bound `B` of the kernel's error for
    /// it where one applies (`|new − r| <= B` is then at rounding).
    Exact {
        r: f64,
        b: Option<f64>,
        what: &'static str,
    },
    /// A reference known to lie in `r`, and the enclosure `e` of the kernel's float result.
    Enc {
        r: Iv,
        e: Option<Iv>,
        what: &'static str,
    },
}

impl Cert {
    fn what(&self) -> &'static str {
        match self {
            Cert::Exact { what, .. } | Cert::Enc { what, .. } => what,
        }
    }
}

fn classify(new: f64, main: f64, c: &Cert) -> Class {
    if new.to_bits() == main.to_bits() {
        return Class::Equal;
    }
    match c {
        Cert::Exact { r, b, .. } => match Dy::cmp_dist(new, *r, main, *r) {
            Some(std::cmp::Ordering::Less) => Class::Closer,
            Some(std::cmp::Ordering::Equal) => Class::Equal,
            _ => match b {
                Some(b) if Iv::p(new).sub(Iv::p(*r)).abs().hi <= *b => Class::AtRounding,
                _ => Class::Farther,
            },
        },
        Cert::Enc { r, e, .. } => {
            // closer: every point of r is nearer to new than to main (r beyond the
            // midpoint, on new's side)
            let mid = Iv::p(new).add(Iv::p(main)).mul(Iv::p(0.5));
            let closer = if new > main {
                r.lo > mid.hi
            } else {
                r.hi < mid.lo
            };
            if closer {
                Class::Closer
            } else if e.is_some_and(|e| e.contains(new)) {
                Class::AtRounding
            } else {
                Class::Farther
            }
        }
    }
}

/// A UV trim record of a golden row (`util::trim_curve`): `1, type, n, degree, m`, the
/// `n + 1` homogeneous control points, the `m + 1` knots, the interval and 3 samples
/// (`0, u, v, 0` or a failure status).
struct Trim {
    cp: usize,
    n: usize,
    deg: usize,
    ivl: (f64, f64),
    samples: [Option<usize>; 3],
    end: usize,
}

fn parse_trim(v: &[f64], i: usize) -> Option<Trim> {
    let g = |j: usize| v.get(j).copied();
    if g(i)? != 1.0 || g(i + 1)? != 14033.0 {
        return None;
    }
    let (n, deg, m) = (g(i + 2)?, g(i + 3)?, g(i + 4)?);
    if n < 0.0 || deg < 1.0 || m < 1.0 || n.fract() != 0.0 || deg.fract() != 0.0 {
        return None;
    }
    let (n, deg, m) = (n as usize, deg as usize, m as usize);
    let cp = i + 5;
    let mut j = cp + 4 * (n + 1) + m + 1;
    let ivl = (g(j)?, g(j + 1)?);
    j += 2;
    let mut samples = [None; 3];
    for s in &mut samples {
        if g(j)? == 0.0 {
            *s = Some(j + 1);
            j += 4;
        } else {
            j += 1;
        }
    }
    Some(Trim {
        cp,
        n,
        deg,
        ivl,
        samples,
        end: j,
    })
}

/// The iso line a row's UV trim is, when it is one: the fixed coordinate `c` (`0`: `u`), its
/// value `f` and the trim interval `[t0, t1]`.
#[derive(Debug, Clone, Copy)]
struct Iso {
    c: usize,
    f: f64,
    ivl: (f64, f64),
}

/// The bound of a degree-1 trim evaluation in the kernel (`nurbs_crv.rs`
/// `crv_point_error`): `64 ε max(|f|, |t0|, |t1|)`.
fn line_bound(iso: &Iso) -> f64 {
    let mx = iso.f.abs().max(iso.ivl.0.abs()).max(iso.ivl.1.abs());
    up_mul(64.0 * f64::EPSILON, mx)
}

/// Labels the elements of every UV trim record in `v` that is an iso line of the face's
/// surface with the exact references of the ideal pipeline: control points and samples
/// `(f, t)`, `f` a side of the face's domain `dom` (`None`: a file B-rep, whose domain the
/// row does not hold; the trim's own fixed value) and `t` the trim's own parameter.
fn label_trims(v: &[f64], dom: Option<[f64; 4]>, out: &mut BTreeMap<usize, Cert>) -> Option<Iso> {
    let mut first = None;
    let mut i = 0;
    while i < v.len() {
        let Some(tr) = parse_trim(v, i) else {
            i += 1;
            continue;
        };
        i = tr.end;
        if tr.n != 1 || tr.deg != 1 {
            continue;
        }
        let p = [(v[tr.cp], v[tr.cp + 1]), (v[tr.cp + 4], v[tr.cp + 5])];
        let (t0, t1) = tr.ivl;
        for c in 0..2 {
            let at = |k: usize| if c == 0 { p[k].0 } else { p[k].1 };
            let mv = |k: usize| if c == 0 { p[k].1 } else { p[k].0 };
            if at(0) != at(1) || mv(0) != t0 || mv(1) != t1 {
                continue;
            }
            let f = at(0);
            if let Some(d) = dom {
                if f != d[c] && f != d[c + 2] {
                    continue;
                }
            }
            let iso = Iso { c, f, ivl: tr.ivl };
            first.get_or_insert(iso);
            let b = line_bound(&iso);
            for (k, t) in [(0usize, t0), (1, t1)] {
                out.insert(
                    tr.cp + 4 * k + c,
                    Cert::Exact {
                        r: f,
                        b: None,
                        what: "trim cp fixed",
                    },
                );
                out.insert(
                    tr.cp + 4 * k + 1 - c,
                    Cert::Exact {
                        r: t,
                        b: None,
                        what: "trim cp param",
                    },
                );
            }
            for (q, frac) in [0.0, 0.5, 1.0].into_iter().enumerate() {
                let Some(j) = tr.samples[q] else { continue };
                let t = ev(t0, t1, frac);
                out.insert(
                    j + c,
                    Cert::Exact {
                        r: f,
                        b: Some(b),
                        what: "trim sample fixed",
                    },
                );
                out.insert(
                    j + 1 - c,
                    Cert::Exact {
                        r: t,
                        b: Some(b),
                        what: "trim sample param",
                    },
                );
            }
            break;
        }
    }
    first
}

/// `Extent1d::evaluate`: `lo + f (hi − lo)` (the ends exactly at `f = 0, 1`).
fn ev(lo: f64, hi: f64, f: f64) -> f64 {
    if f == 0.0 {
        lo
    } else if f == 1.0 {
        hi
    } else {
        lo + f * (hi - lo)
    }
}

fn pts(v: &[f64], i: usize) -> V3 {
    [Iv::p(v[i]), Iv::p(v[i + 1]), Iv::p(v[i + 2])]
}

fn sign_of(a: &V3, b: &V3) -> f64 {
    if dot(a, b).lo > 0.0 {
        1.0
    } else {
        assert!(dot(a, b).hi < 0.0, "orientation undecided");
        -1.0
    }
}

fn scale3(a: &V3, s: f64) -> V3 {
    std::array::from_fn(|q| a[q].mul(Iv::p(s)))
}

fn norm(a: &V3) -> Iv {
    dot(a, a).sqrt()
}

/// `EvaluateBinormal` at a point of the iso line (record `[point, binormal, tangent,
/// normal, uv, uv_vector]`): the references at the exact `(f, t)` (`t` the edge interval's
/// `evaluate(frac)`, as the kernel's), the enclosures from the row's own uv, normal and
/// binormal through the surface's float evaluation.
fn label_binormal(v: &[f64], srf: &Srf, iso: &Iso, frac: f64, out: &mut BTreeMap<usize, Cert>) {
    let n = v.len();
    let (lo, hi) = (v[n - 3], v[n - 2]);
    let t = ev(lo, hi, frac).clamp(lo, hi);
    let mut uv = [t, t];
    uv[iso.c] = iso.f;
    let b = line_bound(iso);
    for (q, r) in uv.into_iter().enumerate() {
        out.insert(
            12 + q,
            Cert::Exact {
                r,
                b: Some(b),
                what: "bin uv",
            },
        );
    }
    // references
    let (_, su, sv) = srf.exact(Iv::p(uv[0]), Iv::p(uv[1]));
    let cn = cross(&su, &sv);
    let sg = sign_of(&pts(v, 9), &cn);
    let nn = scale3(&cn, sg);
    let ln = norm(&nn);
    let t3 = pts(v, 6);
    let bb = cross(&nn, &t3);
    let lb = norm(&bb);
    let a00 = dot(&su, &su);
    let a01 = dot(&su, &sv);
    let a11 = dot(&sv, &sv);
    let b0 = dot(&su, &bb);
    let b1 = dot(&sv, &bb);
    let det = a00.mul(a11).sub(a01.mul(a01));
    let uvv = [
        b0.mul(a11).sub(b1.mul(a01)).div(det).div(lb),
        a00.mul(b1).sub(a01.mul(b0)).div(det).div(lb),
    ];
    // enclosures
    let (_, ksu, ksv) = srf.kernel(v[12], v[13]);
    let kc = kcross(&ksu, &ksv);
    let kn = scale3(&kunit(&kc), sign_of(&pts(v, 9), &kc));
    let kb = kunit(&kcross(&pts(v, 9), &t3));
    let br = pts(v, 3);
    let sub2 = |x: Iv, y: Iv| x.sub(y).widen(up_mul(gam(2), up_add(x.mag(), y.mag())));
    let ka00 = kdot(&ksu, &ksu, 3);
    let ka01 = kdot(&ksu, &ksv, 3);
    let ka11 = kdot(&ksv, &ksv, 3);
    let kb0 = kdot(&ksu, &br, 3);
    let kb1 = kdot(&ksv, &br, 3);
    let kdet = sub2(ka00.mul(ka11), ka01.mul(ka01));
    let kuv = [
        sub2(kb0.mul(ka11), kb1.mul(ka01)).div(kdet).rel(U),
        sub2(ka00.mul(kb1), ka01.mul(kb0)).div(kdet).rel(U),
    ];
    for q in 0..3 {
        out.insert(
            9 + q,
            Cert::Enc {
                r: nn[q].div(ln),
                e: Some(kn[q]),
                what: "bin normal",
            },
        );
        out.insert(
            3 + q,
            Cert::Enc {
                r: bb[q].div(lb),
                e: Some(kb[q]),
                what: "bin binormal",
            },
        );
    }
    for q in 0..2 {
        out.insert(
            14 + q,
            Cert::Enc {
                r: uvv[q],
                e: Some(kuv[q]),
                what: "bin uv_vector",
            },
        );
    }
}

/// The 3D point and derivative of an edge whose curve is `CrvOnSurf(trim, S)` (the
/// `cos_open` fixtures), at the parameter the op evaluates: the references `S(f, t*)` and
/// `±S_w(f, t*)` (the ideal trim is the iso line itself), the enclosures those values
/// widened by the uv error of the float trim evaluation (`64 ε M`) and of the parameter
/// (`γ_3 (|lo| + |hi|)`) through the hull bounds, and by the surface rounding.
fn label_cos(v: &[f64], inputs: &Value, srf: &Srf, iso: &Iso, out: &mut BTreeMap<usize, Cert>) {
    let n = v.len();
    let (lo, hi) = (v[n - 3], v[n - 2]);
    let b = srf.bounds();
    let g = gam(9 * srf.p + 9 * srf.q + (srf.p + 1) * (srf.q + 1) + 8);
    let e_uv = line_bound(iso);
    let mv = 1 - iso.c;
    let binormal = inputs["op"] == "binormal";
    let (t, e_t, didx) = if binormal {
        let frac = num(&inputs["frac"]);
        let t = ev(lo, hi, frac).clamp(lo, hi);
        let e_t = if frac == 0.0 || frac == 1.0 {
            0.0
        } else {
            up_mul(gam(3), up_add(lo.abs(), hi.abs()))
        };
        (Iv::p(t), e_t, 6)
    } else {
        // orientation: the record's derivative against the surface's along the moving axis
        let mut mid = [Iv::p(iso.f); 2];
        mid[mv] = Iv::p(lo).add(Iv::p(hi)).mul(Iv::p(0.5));
        let (_, su0, sv0) = srf.exact(mid[0], mid[1]);
        let smv0 = if mv == 1 { sv0 } else { su0 };
        let opp = (v[3 + mv] < 0.0) != (smv0[mv].mid() < 0.0);
        let tt = if opp {
            1.0 - num(&inputs["t"])
        } else {
            num(&inputs["t"])
        };
        let t = Iv::p(lo).add(Iv::p(tt).mul(Iv::p(hi).sub(Iv::p(lo))));
        (t, up_mul(gam(3), up_add(lo.abs(), hi.abs())), 3)
    };
    let mut uv = [Iv::p(iso.f); 2];
    uv[mv] = t;
    let (s, su, sv) = srf.exact(uv[0], uv[1]);
    let smv = if mv == 1 { sv } else { su };
    let sg = sign_of(&pts(v, didx), &smv);
    let mut r = [e_uv; 2];
    r[mv] = up_add(e_t, e_uv);
    // the parameter's own enclosure width moves the reference too
    let wt = t.hi - t.lo;
    r[mv] = up_add(r[mv], wt);
    for q in 0..3 {
        let rad = up_add(
            up_add(up_mul(b.lu[q], r[0]), up_mul(b.lv[q], r[1])),
            up_mul(g, b.m[q]),
        );
        out.insert(
            q,
            Cert::Enc {
                r: s[q],
                e: Some(s[q].widen(rad)),
                what: "cos point",
            },
        );
        let (l0, l1, sc) = if mv == 1 {
            (
                b.luv[q],
                b.lvv[q],
                Iv::p(2.0 * srf.q as f64).div(Iv::p(b.hv)).hi,
            )
        } else {
            (
                b.luu[q],
                b.luv[q],
                Iv::p(2.0 * srf.p as f64).div(Iv::p(b.hu)).hi,
            )
        };
        let rad = up_add(
            up_add(up_mul(l0, r[0]), up_mul(l1, r[1])),
            up_mul(up_mul(g, b.m[q]), sc),
        );
        let tq = smv[q].mul(Iv::p(sg));
        out.insert(
            didx + q,
            Cert::Enc {
                r: tq,
                e: Some(tq.widen(rad)),
                what: "cos derivative",
            },
        );
    }
}

impl Iv {
    fn mid(self) -> f64 {
        0.5 * self.lo + 0.5 * self.hi
    }
}

// ---- drop quality maxima ----------------------------------------------------------------------

/// The stations behind a row's reported drop quality (the data file's `D*` lines): the 3D
/// curve the drop ran on, and the stations of `drop_iso_curve` (`p, u, v` and its
/// `drop_line_dist`, `line_param`) and of `CrvOnSurf::surface_curve_max_distance_between`
/// (`t, u, v, s` and its `dist`, `dev`), as the kernel evaluated them.
#[derive(Debug, Default)]
struct Drop {
    /// The 3D edge curve (`None`: a `CrvOnSurf` over [`Drop::other_uv`]).
    curve: Option<Crv>,
    other_uv: Option<Crv>,
    isos: Vec<[f64; 5]>,
    gaps: Vec<[f64; 6]>,
}

impl Drop {
    fn point(&self, srf: &Srf, b: Option<&Bounds>, s: f64) -> V3 {
        match (&self.curve, &self.other_uv, b) {
            (Some(c), _, _) => c.kernel(s),
            (None, Some(t), Some(b)) => cos_point(srf, b, t, s),
            _ => panic!("drop data without a curve"),
        }
    }
}

/// `|x|` of an enclosure.
fn absiv(x: Iv) -> Iv {
    x.abs()
}

fn imax(xs: &[Iv]) -> Iv {
    Iv {
        lo: xs.iter().map(|x| x.lo).fold(0.0, f64::max),
        hi: xs.iter().map(|x| x.hi).fold(0.0, f64::max),
    }
}

/// The kernel's `eval_dist_and_dev` from its surface point and normal at `uv` and the curve
/// point `c`: `(|dist|, dev)` enclosed.
fn station_enc(srf: &Srf, uv: (f64, f64), c: &V3) -> (Iv, Iv) {
    let (p, su, sv) = srf.kernel(uv.0, uv.1);
    let n = kunit(&kcross(&su, &sv));
    let gap: V3 = std::array::from_fn(|q| c[q].sub(p[q]).rel(U));
    let d = kdot(&gap, &n, 3);
    let npt: V3 = std::array::from_fn(|q| d.mul(n[q]).rel(U).add(p[q]).rel(U));
    let diff: V3 = std::array::from_fn(|q| c[q].sub(npt[q]).rel(U));
    let dev = kdot(&diff, &diff, 3).clamp0().sqrt().rel(U);
    (absiv(d), dev)
}

/// `line_point_distance(S(uv), n(uv), c)`: `(|line_param|, drop_line_dist)` enclosed (`n`
/// unit, so `line_param = (c − S).n / |n|²` with `|n|²` within rounding of 1).
fn iso_enc(srf: &Srf, uv: (f64, f64), c: &V3) -> (Iv, Iv) {
    let (p, su, sv) = srf.kernel(uv.0, uv.1);
    let n = kunit(&kcross(&su, &sv));
    let gap: V3 = std::array::from_fn(|q| c[q].sub(p[q]).rel(U));
    let lp = kdot(&gap, &n, 3).div(kdot(&n, &n, 3)).rel(U);
    let pnt: V3 = std::array::from_fn(|q| lp.mul(n[q]).rel(U).add(p[q]).rel(U));
    let diff: V3 = std::array::from_fn(|q| pnt[q].sub(c[q]).rel(U));
    let dld = kdot(&diff, &diff, 3).clamp0().sqrt().rel(U);
    (absiv(lp), dld)
}

/// The split of `C(s) − P` along and across `nvec`: `(|dist|, dev)`, exact enclosures.
fn split(c: &V3, p: &V3, nvec: &V3) -> (Iv, Iv) {
    let w = sub3(c, p);
    let nn = dot(nvec, nvec);
    let wn = dot(&w, nvec);
    let dist = absiv(wn).div(nn.sqrt());
    let dev = dot(&w, &w).sub(wn.mul(wn).div(nn)).clamp0().sqrt();
    (dist, dev)
}

/// The closest approach `s*` of `curve` to the line `P + R nvec`, near `s0`: a root of
/// `h(s) = (w − (w.n̂)n̂).C'(s)`, `w = C(s) − P`, located by Newton steps in doubles and then
/// bracketed by a sign change of `h` certified in interval arithmetic.
fn closest_param(curve: &Crv, p: &V3, nvec: &V3, s0: f64) -> (f64, f64) {
    let h = |s: f64| {
        let (c, cd) = curve.exact(s);
        let w = sub3(&c, p);
        let nn = dot(nvec, nvec);
        let wn = dot(&w, nvec);
        let perp: V3 = std::array::from_fn(|q| w[q].sub(wn.mul(nvec[q]).div(nn)));
        dot(&perp, &cd)
    };
    let (mut a, mut b) = (s0, s0 + 2f64.powi(-30) * s0.abs().max(1.0));
    let (mut fa, mut fb) = (h(a).mid(), h(b).mid());
    for _ in 0..80 {
        if fb == fa || fb == 0.0 {
            break;
        }
        let c = b - fb * (b - a) / (fb - fa);
        a = b;
        fa = fb;
        b = c;
        fb = h(b).mid();
        if (b - a).abs() <= 4.0 * f64::EPSILON * b.abs().max(1e-300) {
            break;
        }
    }
    let mut d = 2f64.powi(-50) * b.abs().max(2f64.powi(-20));
    for _ in 0..40 {
        let (lo, hi) = (b - d, b + d);
        let (hl, hh) = (h(lo), h(hi));
        if (hl.hi <= 0.0 && hh.lo >= 0.0) || (hl.lo >= 0.0 && hh.hi <= 0.0) {
            return (lo, hi);
        }
        d *= 2.0;
    }
    panic!("no certified bracket near {b}");
}

/// The references of the ideal pipeline at the stations (the UV curve the exact iso line
/// `(f, ·)`): `(|line_param|, drop_line_dist)` at an iso station and `(|dist|, dev)` at a gap
/// station, enclosed; `0` for a `CrvOnSurf` edge (its ideal curve is `S(f, ·)` itself).
fn station_refs(srf: &Srf, iso: &Iso, d: &Drop, param: f64, gap: bool) -> (Iv, Iv) {
    let Some(curve) = &d.curve else {
        return (Iv::p(0.0), Iv::p(0.0));
    };
    let mut uv = [Iv::p(param); 2];
    uv[iso.c] = Iv::p(iso.f);
    let (p, su, sv) = srf.exact(uv[0], uv[1]);
    let nvec = cross(&su, &sv);
    if !gap {
        let (c, _) = curve.exact(param);
        return split(&c, &p, &nvec);
    }
    let (lo, hi) = closest_param(curve, &p, &nvec, param);
    let (d0, v0) = split(&curve.exact(lo).0, &p, &nvec);
    let (d1, v1) = split(&curve.exact(hi).0, &p, &nvec);
    let r = up_mul(curve.lipschitz(), hi - lo);
    (
        Iv::hull(d0, d1).widen(r).clamp0(),
        Iv::hull(v0, v1).widen(r).clamp0(),
    )
}

/// `sqrt(d² + v²)` as the kernel rounds it (`round`) or exactly.
fn comb(d: Iv, v: Iv, round: bool) -> Iv {
    let r = |x: Iv| if round { x.rel(U) } else { x };
    let q = r(r(d.mul(d)).add(r(v.mul(v))));
    r(q.clamp0().sqrt())
}

/// The certificates of a row's reported drop quality (`state[0]`, or `state[0..2]`), from
/// its stations. Every station's dumped value must lie in its enclosure (the data are the
/// kernel's).
fn label_drop(case: &str, srf: &Srf, iso: &Iso, d: &Drop, out: &mut BTreeMap<usize, Cert>) {
    let b = d.curve.is_none().then(|| srf.bounds());
    let mut e = (Vec::new(), Vec::new());
    let mut r = (Vec::new(), Vec::new());
    // the enclosures below are of the kernel's evaluation at each station's own uv; they
    // bound the rounding against the reference only when that uv is the exact `(f, param)`
    // up to the float trim evaluation's `line_bound` (the certified input error). A station
    // off by more (on the drop's own Newton line, as on main) may not set the reported
    // maximum: if its enclosure reaches the maximum's lower bound, the element gets no
    // at-rounding class and must be CLOSER or EQUAL.
    let lb = line_bound(iso);
    let uv_ok = |param: f64, u: f64, v: f64| {
        let mut want = [param; 2];
        want[iso.c] = iso.f;
        [u, v]
            .into_iter()
            .zip(want)
            .all(|(got, want)| Iv::p(got).sub(Iv::p(want)).abs().hi <= lb)
    };
    let mut ok = Vec::new();
    for st in &d.isos {
        ok.push(uv_ok(st[0], st[1], st[2]));
    }
    for st in &d.gaps {
        ok.push(uv_ok(st[0], st[1], st[2]));
    }
    for st in &d.isos {
        let [p, u, v, dld, lp] = *st;
        let c = d.point(srf, b.as_ref(), p);
        let (ed, ev) = iso_enc(srf, (u, v), &c);
        assert!(
            ed.contains(lp.abs()) && ev.contains(dld),
            "{case}: iso station {st:?} outside {ed:?} {ev:?}"
        );
        let (rd, rv) = station_refs(srf, iso, d, p, false);
        e.0.push(ed);
        e.1.push(ev);
        r.0.push(rd);
        r.1.push(rv);
    }
    for st in &d.gaps {
        let [t, u, v, s, dist, dev] = *st;
        let c = d.point(srf, b.as_ref(), s);
        let (ed, ev) = station_enc(srf, (u, v), &c);
        assert!(
            ed.contains(dist) && ev.contains(dev),
            "{case}: gap station {st:?} outside {ed:?} {ev:?}"
        );
        let (rd, rv) = station_refs(srf, iso, d, t, true);
        e.0.push(ed);
        e.1.push(ev);
        r.0.push(rd);
        r.1.push(rv);
    }
    let (ed, ev, rd, rv) = (imax(&e.0), imax(&e.1), imax(&r.0), imax(&r.1));
    // `|max x − max r| <= max_j |x_j − r_j| <= max_j sup |E_j − R_j|`. An on-iso station's
    // `sup |E_j − R_j|` is a rounding bound (certified inputs). An off-iso station keeps the
    // at-rounding class only if it cannot set the maximum (its enclosure lies below the
    // maximum's lower bound) or its own `sup |E_j − R_j|` is no larger than the on-iso
    // stations' largest: then it adds nothing beyond that rounding bound.
    let settled = |es: &[Iv], rs: &[Iv], mx: Iv| {
        let gap = |x: &Iv, y: &Iv| up_add(x.hi.max(y.hi), -(x.lo.min(y.lo)));
        let cert = es
            .iter()
            .zip(rs)
            .zip(&ok)
            .filter(|(_, ok)| **ok)
            .map(|((x, y), _)| gap(x, y))
            .fold(0.0, f64::max);
        es.iter()
            .zip(rs)
            .zip(&ok)
            .all(|((x, y), ok)| *ok || x.hi < mx.lo || gap(x, y) <= cert)
    };
    let (ok_d, ok_v) = (settled(&e.0, &r.0, ed), settled(&e.1, &r.1, ev));
    if ["create_plain", "goc", "create_dev0"]
        .iter()
        .any(|k| case.contains(k))
    {
        let what = "drop quality sqrt(dist² + dev²), max over stations";
        out.insert(
            0,
            Cert::Enc {
                r: comb(rd, rv, false),
                e: (ok_d && ok_v).then(|| comb(ed, ev, true)),
                what,
            },
        );
    } else {
        out.insert(
            0,
            Cert::Enc {
                r: rd,
                e: ok_d.then_some(ed),
                what: "drop distance, max over stations",
            },
        );
        out.insert(
            1,
            Cert::Enc {
                r: rv,
                e: ok_v.then_some(ev),
                what: "drop deviation, max over stations",
            },
        );
    }
}

// ---- the data file ---------------------------------------------------------------------------

struct El {
    key: String,
    i: usize,
    main: f64,
    new: f64,
    want: String,
}

#[derive(Default)]
struct Data {
    /// `(unit, case)` -> moved elements.
    els: BTreeMap<(String, String), Vec<El>>,
    drops: BTreeMap<String, Drop>,
    /// The cached edge/face gap station: `t, u, v, gap`.
    gocs: BTreeMap<String, [f64; 4]>,
    /// The blade hub face's surface and domain.
    blade: Option<(Srf, [f64; 4])>,
}

fn f(s: &str) -> f64 {
    s.parse().unwrap_or_else(|_| panic!("number {s}"))
}

fn fs(s: &str) -> Vec<f64> {
    s.split(',').map(f).collect()
}

fn cps(s: &str) -> Vec<[f64; 4]> {
    s.split(';')
        .map(|c| {
            let c = fs(c);
            [c[0], c[1], c[2], c[3]]
        })
        .collect()
}

/// `deg|knots|cps`: degree, full knot vector, homogeneous control points.
fn crv(s: &str) -> Crv {
    let p: Vec<&str> = s.split('|').collect();
    Crv {
        p: p[0].parse().unwrap(),
        knots: fs(p[1]),
        cps: cps(p[2]),
    }
}

/// `p|q|u knots|v knots|net (rows along u joined by '/')|domain`.
fn srf(s: &str) -> (Srf, [f64; 4]) {
    let p: Vec<&str> = s.split('|').collect();
    let net: Vec<Vec<[f64; 4]>> = p[4].split('/').map(cps).collect();
    let rational = net.iter().flatten().any(|cp| cp[3] > -1e300);
    let d = fs(p[5]);
    let s = Srf {
        net,
        rational,
        p: p[0].parse().unwrap(),
        q: p[1].parse().unwrap(),
        u: fs(p[2]),
        v: fs(p[3]),
    };
    (s, [d[0], d[1], d[2], d[3]])
}

fn data() -> Data {
    let mut d = Data::default();
    for line in DATA
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let c: Vec<&str> = line.split('\t').collect();
        match c[0] {
            "E" => d
                .els
                .entry((c[1].to_owned(), c[2].to_owned()))
                .or_default()
                .push(El {
                    key: c[3].to_owned(),
                    i: c[4].parse().unwrap(),
                    main: f(c[5]),
                    new: f(c[6]),
                    want: c[7].to_owned(),
                }),
            "DC" => d.drops.entry(c[1].to_owned()).or_default().curve = Some(crv(c[2])),
            "DU" => d.drops.entry(c[1].to_owned()).or_default().other_uv = Some(crv(c[2])),
            "DI" => {
                let x: Vec<f64> = c[2..7].iter().map(|s| f(s)).collect();
                d.drops
                    .entry(c[1].to_owned())
                    .or_default()
                    .isos
                    .push([x[0], x[1], x[2], x[3], x[4]]);
            }
            "DG" => {
                let x: Vec<f64> = c[2..8].iter().map(|s| f(s)).collect();
                d.drops
                    .entry(c[1].to_owned())
                    .or_default()
                    .gaps
                    .push([x[0], x[1], x[2], x[3], x[4], x[5]]);
            }
            "G" => {
                let x: Vec<f64> = c[2..6].iter().map(|s| f(s)).collect();
                d.gocs.insert(c[1].to_owned(), [x[0], x[1], x[2], x[3]]);
            }
            "S" => d.blade = Some(srf(c[2])),
            k => panic!("unknown line kind {k}"),
        }
    }
    d
}

// ---- the labels the prototypes left to the driver --------------------------------------------

/// `NormalizedEvaluate` in parameter space on an iso trim: the point `(f, s)` at the
/// parameter the op evaluates (`s` from the normalised `t` over the trim interval as the
/// kernel maps it, `1 − t` when the edge use runs against the trim), the derivative the unit
/// vector along the moving axis.
fn label_norm_uv(v: &[f64], inputs: &Value, iso: &Iso, out: &mut BTreeMap<usize, Cert>) {
    let c = iso.c;
    let (t0, t1) = iso.ivl;
    let d = v[3 + 1 - c];
    let opp = (d < 0.0) == (t1 > t0);
    let t = num(&inputs["t"]);
    let tt = if opp { 1.0 - t } else { t };
    let mut pt = [ev(t0, t1, tt); 2];
    pt[c] = iso.f;
    let b = line_bound(iso);
    for (q, r) in pt.into_iter().enumerate() {
        out.insert(
            q,
            Cert::Exact {
                r,
                b: Some(b),
                what: "norm uv point",
            },
        );
    }
    if inputs["want_deriv"] == true {
        let mut dd = [0.0; 2];
        dd[1 - c] = if d < 0.0 { -1.0 } else { 1.0 };
        for (q, r) in dd.into_iter().enumerate() {
            out.insert(
                3 + q,
                Cert::Exact {
                    r,
                    b: None,
                    what: "norm uv deriv",
                },
            );
        }
    }
}

/// The degree-1 UV trim `iso` is, as a curve.
fn iso_curve(iso: &Iso) -> Crv {
    let (t0, t1) = iso.ivl;
    let mut a = [t0, t0, 0.0, 1.0];
    let mut b = [t1, t1, 0.0, 1.0];
    a[iso.c] = iso.f;
    b[iso.c] = iso.f;
    Crv {
        p: 1,
        knots: vec![t0, t0, t1, t1],
        cps: vec![a, b],
    }
}

/// The cached edge/face gap of a `CrvOnSurf` edge (`get_or_create` records it at 0):
/// `|S(trim(t)) − S(uv)|` at the closest-point solver's `uv`, enclosed; the reference `0`.
fn label_goc(srf: &Srf, iso: &Iso, g: &[f64; 4], v: &[f64], out: &mut BTreeMap<usize, Cert>) {
    let [t, u, w, gap] = *g;
    assert_eq!(
        v[0].to_bits(),
        gap.to_bits(),
        "the cached gap is not the station's"
    );
    let b = srf.bounds();
    let c = cos_point(srf, &b, &iso_curve(iso), t);
    let (p, _, _) = srf.kernel(u, w);
    let diff: V3 = std::array::from_fn(|q| c[q].sub(p[q]).rel(U));
    let e = kdot(&diff, &diff, 3).clamp0().sqrt().rel(U);
    assert!(e.contains(gap), "gap {gap} outside {e:?}");
    out.insert(
        0,
        Cert::Enc {
            r: Iv::p(0.0),
            e: Some(e),
            what: "cached edge/face gap",
        },
    );
}

/// The blade hub edge (a file B-rep): its 3D curve `C` is the face's `v = f` boundary
/// curve itself (same degree, knots and homogeneous control points: checked), so the ideal
/// drop's distance and deviation are `0`; the lifted samples' references are `S(t, f)`.
fn label_blade(v: &[f64], srf: &Srf, iso: &Iso, curve: &Crv, out: &mut BTreeMap<usize, Cert>) {
    assert_eq!(iso.c, 1, "the blade trim is a v iso line");
    let j = if iso.f == srf.v[0] {
        0
    } else {
        assert_eq!(iso.f, srf.v[srf.v.len() - 1]);
        srf.net[0].len() - 1
    };
    assert!(
        srf.v[..=srf.q].iter().all(|&x| x == srf.v[0]),
        "v knots not clamped"
    );
    assert!(srf.v[srf.v.len() - 1 - srf.q..]
        .iter()
        .all(|&x| x == srf.v[srf.v.len() - 1]));
    assert_eq!(
        (srf.p, &srf.u),
        (curve.p, &curve.knots),
        "the boundary is not C"
    );
    for (i, row) in srf.net.iter().enumerate() {
        assert_eq!(
            row[j].map(f64::to_bits),
            curve.cps[i].map(f64::to_bits),
            "net row {i}"
        );
    }
    out.insert(
        0,
        Cert::Exact {
            r: 0.0,
            b: None,
            what: "drop distance (C is the iso)",
        },
    );
    out.insert(
        1,
        Cert::Exact {
            r: 0.0,
            b: None,
            what: "drop deviation (C is the iso)",
        },
    );
    let b = line_bound(iso);
    // `put_created_lift`: `5, type`, then per sample `0, t, C(t), uv, S(uv)`
    for k in 0..5 {
        let base = 35 + 10 * k;
        assert_eq!(v[base], 0.0, "lift sample {k}");
        let t = v[base + 1];
        let mut uv = [Iv::p(t); 2];
        uv[iso.c] = Iv::p(iso.f);
        out.insert(
            base + 5 + iso.c,
            Cert::Exact {
                r: iso.f,
                b: Some(b),
                what: "lift uv fixed",
            },
        );
        out.insert(
            base + 6 - iso.c,
            Cert::Exact {
                r: t,
                b: Some(b),
                what: "lift uv param",
            },
        );
        let (s, _, _) = srf.exact(uv[0], uv[1]);
        let (e, _, _) = srf.kernel(v[base + 5], v[base + 6]);
        for q in 0..3 {
            out.insert(
                base + 7 + q,
                Cert::Enc {
                    r: s[q],
                    e: Some(e[q]),
                    what: "lift point",
                },
            );
        }
    }
}

// ---- the driver --------------------------------------------------------------------------------

fn certificates(
    unit: &str,
    case: &str,
    inputs: &Value,
    status: i64,
    v: &[f64],
    d: &Data,
) -> BTreeMap<usize, Cert> {
    let mut out = BTreeMap::new();
    let dom = inputs["brep"].get("dom").map(|x| {
        let a = nums(x);
        [a[0], a[1], a[2], a[3]]
    });
    let dom = dom.or_else(|| (case == BLADE).then(|| d.blade.as_ref().expect("S line").1));
    let iso = label_trims(v, dom, &mut out);
    if unit == "topology.face_p3" {
        // `uv_trim`'s parameter-space joint gaps: the mean over the joints off the pole, the
        // maximum including the pole, whose parameter gap is the whole side, 1
        out.insert(
            6,
            Cert::Exact {
                r: 0.0,
                b: None,
                what: "mean joint uv gap",
            },
        );
        out.insert(
            7,
            Cert::Exact {
                r: 1.0,
                b: None,
                what: "max joint uv gap (the pole)",
            },
        );
        return out;
    }
    if unit == "topology.edgeuse_p3" && inputs["op"] == "binormal" && status == 0 && v[16] == 1.0 {
        // `put_eu_state` after the 16 binormal values: `1`, the trim record, the owner, the
        // three drop-fail values, then whether the vertex use's vertex/face gap is cached.
        // Only `sm_FixUVTrimCurves` computes it on this path (`CreateUVTrimCurves` for the
        // face's trimmed surface cache), and only when the trims meeting at the vertex miss
        // each other by more than `EFF_ZERO (1 + |uv|)`. The ideal pipeline's trims are the
        // exact iso lines of the domain sides, which meet exactly at the corner: the gap is
        // never computed, the reference is `0`.
        if let Some(tr) = parse_trim(v, 17) {
            out.insert(
                tr.end + 4,
                Cert::Exact {
                    r: 0.0,
                    b: None,
                    what: "vertex/face gap cached (FixUVTrimCurves ran)",
                },
            );
        }
    }
    let (Some(iso), 0) = (iso, status) else {
        return out;
    };
    if unit != "topology.edgeuse_p3" {
        return out;
    }
    if case == BLADE {
        let (s, _) = d.blade.as_ref().unwrap();
        let curve = d.drops[case].curve.as_ref().unwrap();
        label_blade(v, s, &iso, curve, &mut out);
        return out;
    }
    let srf = Srf::from_input(inputs);
    let cos = case.starts_with("cos_open");
    match inputs["op"].as_str().unwrap() {
        "binormal" => {
            label_binormal(v, &srf, &iso, num(&inputs["frac"]), &mut out);
            if cos {
                label_cos(v, inputs, &srf, &iso, &mut out);
            }
        }
        "normalized" => {
            if inputs["parameter_space"] == true {
                label_norm_uv(v, inputs, &iso, &mut out);
            } else if cos {
                label_cos(v, inputs, &srf, &iso, &mut out);
            }
        }
        _ => {
            if let Some(g) = d.gocs.get(case) {
                label_goc(&srf, &iso, g, v, &mut out);
            }
            if let Some(dr) = d.drops.get(case) {
                label_drop(case, &srf, &iso, dr, &mut out);
            }
        }
    }
    out
}

/// `|new − r| / B` of an at-rounding element.
fn ratio(new: f64, c: &Cert) -> f64 {
    match c {
        Cert::Exact { r, b: Some(b), .. } => Iv::p(new).sub(Iv::p(*r)).abs().hi / b,
        Cert::Enc { r, e: Some(e), .. } => {
            let d = Iv::p(new).sub(*r).abs().hi;
            let b = e.sub(*r).abs().hi;
            d / b
        }
        _ => f64::NAN,
    }
}

#[test]
fn moved_elements_are_closer_equal_or_at_rounding() {
    let d = data();
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let filter = std::env::var("ISSUE_18325_FILTER").ok();
    let mut bad = Vec::new();
    let mut pins = Vec::new();
    let mut tally: BTreeMap<(String, &'static str, Class), (usize, f64)> = BTreeMap::new();
    let mut rows: BTreeMap<(String, Class), usize> = BTreeMap::new();
    let mut seen = 0usize;
    for runner in registry() {
        if !UNITS.contains(&runner.id()) {
            continue;
        }
        let unit = runner.id().to_owned();
        let cases: BTreeMap<String, goldens::Case> = runner
            .cases(&ctx)
            .unwrap()
            .into_iter()
            .map(|c| (c.name.clone(), c))
            .collect();
        for ((u, name), els) in d.els.range((unit.clone(), String::new())..) {
            if *u != unit {
                break;
            }
            if filter.as_ref().is_some_and(|x| !name.contains(x.as_str())) {
                continue;
            }
            let case = cases
                .get(name)
                .unwrap_or_else(|| panic!("{unit}: no case {name}"));
            let out = runner.run_rust(case).unwrap();
            let key = &els[0].key;
            assert!(els.iter().all(|e| e.key == *key), "{name}: one key per row");
            let v = &out.values[key];
            let status = out
                .ints
                .get("status")
                .and_then(|s| s.first().copied())
                .unwrap_or(0);
            let certs = certificates(&unit, name, &case.inputs, status, v, &d);
            let mut worst = Class::Equal;
            for e in els {
                seen += 1;
                let got = v[e.i];
                if got.to_bits() != e.new.to_bits() {
                    bad.push(format!(
                        "{unit}/{name} {key}[{}]: replays {got:?}, the golden holds {:?}",
                        e.i, e.new
                    ));
                }
                let Some(c) = certs.get(&e.i) else {
                    bad.push(format!("{unit}/{name} {key}[{}]: no certificate", e.i));
                    continue;
                };
                let class = classify(e.new, e.main, c);
                let slot = tally
                    .entry((unit.clone(), c.what(), class))
                    .or_insert((0, 0.0));
                slot.0 += 1;
                if class == Class::AtRounding {
                    slot.1 = slot.1.max(ratio(e.new, c));
                }
                worst = match (worst, class) {
                    (_, Class::Farther) | (Class::Farther, _) => Class::Farther,
                    (_, Class::AtRounding) | (Class::AtRounding, _) => Class::AtRounding,
                    (_, Class::Closer) | (Class::Closer, _) => Class::Closer,
                    _ => Class::Equal,
                };
                let name_of = format!("{class:?}");
                if e.want == "?" {
                    pins.push(format!("{unit}\t{name}\t{key}\t{}\t{name_of}", e.i));
                } else if e.want != name_of {
                    bad.push(format!(
                        "{unit}/{name} {key}[{}] {:?} -> {:?}: {name_of}, pinned {} ({c:?})",
                        e.i, e.main, e.new, e.want
                    ));
                }
                if class == Class::Farther {
                    bad.push(format!(
                        "{unit}/{name} {key}[{}] {:?} -> {:?}: FARTHER ({c:?})",
                        e.i, e.main, e.new
                    ));
                }
            }
            *rows.entry((unit.clone(), worst)).or_default() += 1;
        }
    }
    eprintln!("element classes (unit, kind, class: count, max |new − r| / B):");
    for ((u, what, class), (n, r)) in &tally {
        if *class == Class::AtRounding {
            eprintln!("  {u}\t{what}\t{class:?}\t{n}\t{r:.3}");
        } else {
            eprintln!("  {u}\t{what}\t{class:?}\t{n}");
        }
    }
    eprintln!("row classes (the worst element's):");
    for ((u, class), n) in &rows {
        eprintln!("  {u}\t{class:?}\t{n}");
    }
    if let Ok(path) = std::env::var("ISSUE_18325_PINS") {
        std::fs::write(path, pins.join("\n") + "\n").unwrap();
    }
    if filter.is_none() {
        assert_eq!(seen, d.els.values().map(Vec::len).sum::<usize>());
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    assert!(
        pins.is_empty(),
        "{} elements without a pinned class",
        pins.len()
    );
}
