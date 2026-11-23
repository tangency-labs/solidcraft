//! #17896 (retiring `keep_operand_uv_trims`): the golden rows the affine fix moves
//! (`CPoint::affine2` in `nurbs/math.rs`, a knot insertion keeping a coordinate exactly
//! when both control points it combines share it), replayed and classed against exact
//! references. (`boolean.api_brep`'s rows are main's, bit for bit, since a plane keeps
//! `MakeFace`'s trim of a line edge.)
//!
//! Run: `cargo test --profile goldens -p goldens --test keepuv_moved_rows`.
//!
//! Each row is re-run through its unit's runner and every moved element must reproduce the
//! new golden value bit for bit (the row the expectations commit writes). Each moved
//! element then gets a class against the true value of the geometry the case describes:
//!
//! * **closer** / **farther**: the new value is strictly nearer to / farther from the
//!   reference than the value on `main`, decided in exact arithmetic ([`exact_class`]: both
//!   doubles and the rational reference scaled to a common power of two in `i128`), or by
//!   a margin far above the reference's own error bound ([`Iv`], outward-rounded
//!   intervals);
//! * **equal**: a discrete choice among values that are equal in exact arithmetic (the
//!   index of a maximum among gaps that are all 0, the kind of the record holding it).
//!
//! A row is closer when every element is closer or equal, farther when every element is
//! farther or equal, and mixed otherwise; the expected class of every element and row is
//! pinned, so a later change to any of them fails here.
//!
//! The references (all PROVEN by the closed forms named per row):
//! * the gap between two curves, or a curve and a surface, that the case builds as the
//!   same exact geometry (iso curves of one surface meeting at a corner, a trim and its
//!   surface's boundary) is 0;
//! * a trim's control points, knots and parameters are the case's decimal inputs (`0.1`,
//!   `0.9`, `t = 0.55`, `0.438`) as exact rationals; a unit derivative is 1;
//!
//! Elements with no reference narrower than their move are [`Ref::Open`] and listed, not
//! classed.
//!
//! Seven elements are pinned FARTHER (in rows pinned mixed). Each is a gap whose reference is
//! 0 and which is the difference of two evaluated points; each move is below one ulp of the
//! largest coordinate of those points, the rounding of the gap's own evaluation (MEASURED;
//! no a-priori bound is claimed here):
//! * `topology.edgeuse_p1` `face_bss_tiny_gaps3_{0,2}_{0,1}` `state[4|5]` (4 elements):
//!   6.562021981706704e-13 to 6.562023637809558e-13, a move of 1.7e-19 on a net whose
//!   coordinates reach 0.003 (one ulp 4.3e-19). The gap itself, main's too, is the drop's
//!   residual at the corner, not rounding.
//! * `topology.vertex` `patch_sub_v1_gaps_g{0,1}` `state[16]` (2 elements): 2.8e-17 to
//!   5.6e-17, on a net whose coordinates reach 2 (one ulp 4.4e-16).
//! * `heal.heal_data_p2` `gen0161` `out[93]` (1 element): 4.5e-19 to 9.1e-19, a move of
//!   4.6e-19 on a net whose `x` reaches 0.005 (one ulp 8.7e-19).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    missing_docs
)]

use goldens::{case::repo_root, CaseContext, Output};
use std::collections::BTreeMap;

// ---- exact comparison --------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Closer,
    Equal,
    Farther,
}

/// An exact dyadic rational `m 2^e`.
#[derive(Debug, Clone, Copy)]
struct Dy {
    m: i128,
    e: i32,
}

impl Dy {
    /// `x` exactly.
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

    /// Strips trailing zero bits (keeps the magnitudes small).
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

    /// Both at the least exponent.
    fn align(a: Dy, b: Dy) -> (i128, i128, i32) {
        let e = a.e.min(b.e);
        let up = |d: Dy| {
            if d.m == 0 {
                return 0;
            }
            let s = u32::try_from(d.e - e).unwrap();
            assert!(
                s < 127 && d.m.unsigned_abs().leading_zeros() > s + 1,
                "i128 overflow"
            );
            d.m << s
        };
        (up(a), up(b), e)
    }

    fn add(self, o: Dy) -> Dy {
        let (a, b, e) = Dy::align(self, o);
        Dy {
            m: a.checked_add(b).expect("i128"),
            e,
        }
        .norm()
    }

    fn sub(self, o: Dy) -> Dy {
        self.add(Dy { m: -o.m, e: o.e })
    }

    fn mul(self, o: Dy) -> Dy {
        Dy {
            m: self.m.checked_mul(o.m).expect("i128"),
            e: self.e + o.e,
        }
        .norm()
    }

    fn neg(self) -> Dy {
        Dy {
            m: -self.m,
            e: self.e,
        }
    }

    fn half(self) -> Dy {
        Dy {
            m: self.m,
            e: self.e - 1,
        }
        .norm()
    }

    fn cmp(self, o: Dy) -> std::cmp::Ordering {
        let (a, b, _) = Dy::align(self, o);
        a.cmp(&b)
    }
}

/// The class of `new` against `main` for an exact reference `r`: `|new − r|` against
/// `|main − r|`, in exact arithmetic.
fn exact_class(new: f64, main: f64, r: Dy) -> Class {
    let dn = Dy::f(new).sub(r);
    let dm = Dy::f(main).sub(r);
    let abs = |d: Dy| Dy {
        m: d.m.abs(),
        e: d.e,
    };
    match abs(dn).cmp(abs(dm)) {
        std::cmp::Ordering::Less => Class::Closer,
        std::cmp::Ordering::Equal => Class::Equal,
        std::cmp::Ordering::Greater => Class::Farther,
    }
}

/// The class of `new` against `main` for a reference known only to lie in `r`: decided
/// when all of `r` is on one side of the two values' exact midpoint; otherwise the two are
/// indistinguishable within the reference's enclosure, and the class is equal.
fn enclosed_class(new: f64, main: f64, r: Iv) -> Class {
    if new == main {
        return Class::Equal;
    }
    let mid = Dy::f(new).add(Dy::f(main)).half();
    let lo = Dy::f(r.lo).cmp(mid);
    let hi = Dy::f(r.hi).cmp(mid);
    let below = hi == std::cmp::Ordering::Less; // all of r below the midpoint
    let above = lo == std::cmp::Ordering::Greater;
    match (below, above) {
        (true, _) => {
            if new < main {
                Class::Closer
            } else {
                Class::Farther
            }
        }
        (_, true) => {
            if new > main {
                Class::Closer
            } else {
                Class::Farther
            }
        }
        _ => Class::Equal,
    }
}

// ---- intervals ------------------------------------------------------------------------------

/// A closed interval `[lo, hi]` with outward rounding: every operation rounds to nearest
/// (IEEE, correctly rounded for `+ − × ÷ √`) and widens by one ulp each way, so the exact
/// result of the operation on any members lies inside.
#[derive(Debug, Clone, Copy)]
struct Iv {
    lo: f64,
    hi: f64,
}

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
    fn add(self, o: Iv) -> Iv {
        Iv::out(self.lo + o.lo, self.hi + o.hi)
    }
    fn sub(self, o: Iv) -> Iv {
        Iv::out(self.lo - o.hi, self.hi - o.lo)
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
        assert!(self.lo >= 0.0);
        Iv::out(self.lo.sqrt(), self.hi.sqrt())
    }
}

/// A non-rational B-spline surface (control net `net[i][j]`, `i` along u), the cases'
/// `bspline` input.
struct Bss {
    net: Vec<Vec<[f64; 3]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// The basis functions of degree `p` over `knots` and their first derivatives at `t`
/// (Cox–de Boor in intervals; `t` lies in one knot span, checked).
fn basis(knots: &[f64], p: usize, t: Iv) -> (Vec<Iv>, Vec<Iv>) {
    let n = knots.len() - p - 1;
    // the span holding t (the last nonempty one for t at the end)
    let span = (p..n)
        .find(|&i| knots[i] <= t.lo && t.hi < knots[i + 1])
        .or_else(|| (t.lo == knots[n] && t.hi == knots[n]).then_some(n - 1))
        .expect("t inside one knot span");
    let zero = Iv::p(0.0);
    let k = |i: usize| Iv::p(knots[i]);
    // degree-0
    let mut nb: Vec<Iv> = (0..knots.len() - 1)
        .map(|i| if i == span { Iv::p(1.0) } else { zero })
        .collect();
    let mut lower = nb.clone();
    for d in 1..=p {
        lower.clone_from(&nb);
        let mut next = vec![zero; knots.len() - 1 - d];
        for (i, slot) in next.iter_mut().enumerate() {
            let mut acc = zero;
            let den1 = knots[i + d] - knots[i];
            if den1 != 0.0 {
                acc = acc.add(t.sub(k(i)).div(k(i + d).sub(k(i))).mul(nb[i]));
            }
            let den2 = knots[i + d + 1] - knots[i + 1];
            if den2 != 0.0 {
                acc = acc.add(
                    k(i + d + 1)
                        .sub(t)
                        .div(k(i + d + 1).sub(k(i + 1)))
                        .mul(nb[i + 1]),
                );
            }
            *slot = acc;
        }
        nb = next;
    }
    // derivatives from the degree p - 1 functions
    let pf = Iv::p(p as f64);
    let der: Vec<Iv> = (0..n)
        .map(|i| {
            let mut acc = zero;
            if knots[i + p] != knots[i] {
                acc = acc.add(pf.mul(lower[i]).div(k(i + p).sub(k(i))));
            }
            if knots[i + p + 1] != knots[i + 1] {
                acc = acc.sub(pf.mul(lower[i + 1]).div(k(i + p + 1).sub(k(i + 1))));
            }
            acc
        })
        .collect();
    (nb[..n].to_vec(), der)
}

type V3 = [Iv; 3];

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1].mul(b[2]).sub(a[2].mul(b[1])),
        a[2].mul(b[0]).sub(a[0].mul(b[2])),
        a[0].mul(b[1]).sub(a[1].mul(b[0])),
    ]
}

fn unit(a: V3) -> V3 {
    let len = a[0]
        .mul(a[0])
        .add(a[1].mul(a[1]))
        .add(a[2].mul(a[2]))
        .sqrt();
    [a[0].div(len), a[1].div(len), a[2].div(len)]
}

fn scale(a: V3, s: f64) -> V3 {
    [a[0].mul(Iv::p(s)), a[1].mul(Iv::p(s)), a[2].mul(Iv::p(s))]
}

impl Bss {
    /// `(S, S_u, S_v)` at `(u, v)`, enclosed.
    fn eval(&self, u: Iv, v: Iv) -> (V3, V3, V3) {
        let (nu, du) = basis(&self.u, self.p, u);
        let (nv, dv) = basis(&self.v, self.q, v);
        let zero = [Iv::p(0.0); 3];
        let (mut s, mut su, mut sv) = (zero, zero, zero);
        for (i, row) in self.net.iter().enumerate() {
            for (j, cp) in row.iter().enumerate() {
                for c in 0..3 {
                    let x = Iv::p(cp[c]);
                    s[c] = s[c].add(nu[i].mul(nv[j]).mul(x));
                    su[c] = su[c].add(du[i].mul(nv[j]).mul(x));
                    sv[c] = sv[c].add(nu[i].mul(dv[j]).mul(x));
                }
            }
        }
        (s, su, sv)
    }
}

/// The binormal evaluation `edgeuse::p2::evaluate_binormal` reports at a point of an edge
/// use lying on the iso curve of its face's surface: `[point, binormal, tangent,
/// face_normal]` with the tangent the iso curve's own derivative `s_t S_w` (`w` the iso's
/// running parameter, `along_u`), the face normal `s_n (S_u × S_v)/|S_u × S_v|` and the
/// binormal `n × t/|t|`. The signs are the edge use's and face's orientation.
fn binormal_record(s: &Bss, u: Iv, v: Iv, along_u: bool, s_t: f64, s_n: f64) -> [Iv; 12] {
    let (p, su, sv) = s.eval(u, v);
    let t = scale(if along_u { su } else { sv }, s_t);
    let n = scale(unit(cross(su, sv)), s_n);
    let b = cross(n, unit(t));
    [
        p[0], p[1], p[2], b[0], b[1], b[2], t[0], t[1], t[2], n[0], n[1], n[2],
    ]
}

// ---- rows -----------------------------------------------------------------------------------

/// The reference of one moved element.
#[derive(Debug, Clone)]
enum Ref {
    /// An exact dyadic value.
    X(Dy),
    /// The exact rational `p / q` (`q > 0`).
    Q(Dy, Dy),
    /// A value known to lie in the interval.
    I(Iv),
    /// A discrete choice among values equal in exact arithmetic (an argmax over gaps that
    /// are all 0, or the kind of the record holding it): equal.
    Tie,
    /// The exact rational value (see [`Rat`]).
    R(Rat),
    /// A heal edge-face gap whose value on `main` and now both lie in its certified
    /// enclosure (`heal_heal_data_p2::tests::keepuv_affine_edge_face_gaps`, the
    /// `certificates::edge_face_gaps` enclosures of the WR-56 rows): equal.
    Cert,
    /// No reference: the element has no closed form and no enclosure narrower than its
    /// move; it is reported, not classed, and the row is classed by its other elements.
    Open(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowClass {
    Closer,
    Equal,
    Farther,
    Mixed,
    /// Every element is [`Ref::Open`].
    Undecided,
}

/// One moved element: `key[i]` of the row's output, its value on `main` and now, its
/// reference and its expected class.
#[derive(Debug, Clone)]
struct El {
    key: &'static str,
    i: usize,
    main: f64,
    new: f64,
    r: Ref,
    want: Option<Class>,
}

fn el(key: &'static str, i: usize, main: f64, new: f64, r: Ref, want: Class) -> El {
    El {
        key,
        i,
        main,
        new,
        r,
        want: Some(want),
    }
}

fn open(key: &'static str, i: usize, main: f64, new: f64, why: &'static str) -> El {
    El {
        key,
        i,
        main,
        new,
        r: Ref::Open(why),
        want: None,
    }
}

fn class_of(e: &El) -> Option<Class> {
    match &e.r {
        Ref::X(r) => Some(exact_class(e.new, e.main, *r)),
        Ref::Q(p, q) => {
            assert!(q.m > 0);
            // |new - p/q| against |main - p/q|, times q > 0
            let dn = Dy::f(e.new).mul(*q).sub(*p);
            let dm = Dy::f(e.main).mul(*q).sub(*p);
            let abs = |d: Dy| Dy {
                m: d.m.abs(),
                e: d.e,
            };
            Some(match abs(dn).cmp(abs(dm)) {
                std::cmp::Ordering::Less => Class::Closer,
                std::cmp::Ordering::Equal => Class::Equal,
                std::cmp::Ordering::Greater => Class::Farther,
            })
        }
        Ref::I(r) => Some(enclosed_class(e.new, e.main, *r)),
        Ref::R(r) => Some(r.class(e.new, e.main)),
        Ref::Tie | Ref::Cert => Some(Class::Equal),
        Ref::Open(_) => None,
    }
}

fn row_class(els: &[El], classes: &[Option<Class>]) -> RowClass {
    let classes: Vec<Option<Class>> = els
        .iter()
        .zip(classes)
        .filter(|(e, _)| !matches!(e.r, Ref::Open(_)))
        .map(|(_, c)| *c)
        .collect();
    if classes.is_empty() {
        return RowClass::Undecided;
    }
    let closer = classes.contains(&Some(Class::Closer));
    let farther = classes.contains(&Some(Class::Farther));
    let skipped = classes.contains(&None);
    match (closer, farther) {
        (true, true) => RowClass::Mixed,
        _ if skipped => panic!("a row with an unclassed element must be mixed by the others"),
        (true, false) => RowClass::Closer,
        (false, true) => RowClass::Farther,
        (false, false) => RowClass::Equal,
    }
}

struct Row {
    case: &'static str,
    els: Vec<El>,
    want: RowClass,
}

/// Replays every row of `unit`: each moved element reproduces its new value bit for bit
/// and gets its expected class; the row its expected class. Prints the table.
fn check(unit: &str, rows: &[Row]) {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let runner = goldens::registry()
        .into_iter()
        .find(|u| u.id() == unit)
        .expect("unit");
    let cases: BTreeMap<String, goldens::Case> = runner
        .cases(&ctx)
        .expect("cases")
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect();
    let mut bad = Vec::new();
    for row in rows {
        let case = cases
            .get(row.case)
            .unwrap_or_else(|| panic!("{unit}/{}: no such case", row.case));
        let out: Output = runner.run_rust(case).expect("rust");
        let mut classes = Vec::new();
        for e in &row.els {
            let got = out
                .values
                .get(e.key)
                .and_then(|v| v.get(e.i).copied())
                .or_else(|| {
                    out.ints
                        .get(e.key)
                        .and_then(|v| v.get(e.i))
                        .map(|&x| x as f64)
                })
                .unwrap_or_else(|| panic!("{unit}/{}: no {}[{}]", row.case, e.key, e.i));
            if got.to_bits() != e.new.to_bits() {
                bad.push(format!(
                    "{unit}/{} {}[{}]: replays {got:e}, the new golden holds {:e}",
                    row.case, e.key, e.i, e.new
                ));
            }
            let c = class_of(e);
            match &e.r {
                Ref::Open(why) => eprintln!(
                    "  {}[{}] {:e} -> {:e}: open ({why})",
                    e.key, e.i, e.main, e.new
                ),
                _ => eprintln!("  {}[{}] {:e} -> {:e}: {c:?}", e.key, e.i, e.main, e.new),
            }
            if c != e.want {
                bad.push(format!(
                    "{unit}/{} {}[{}] {:e} -> {:e}: class {c:?}, expected {:?} ({:?})",
                    row.case, e.key, e.i, e.main, e.new, e.want, e.r
                ));
            }
            classes.push(c);
        }
        let rc = row_class(&row.els, &classes);
        eprintln!("{unit}/{}: {rc:?} ({} elements)", row.case, row.els.len());
        if rc != row.want {
            bad.push(format!(
                "{unit}/{}: row class {rc:?}, expected {:?}",
                row.case, row.want
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

fn d(x: f64) -> Dy {
    Dy::f(x)
}

/// The same as an interval of doubles.
fn lerp_iv(a: f64, b: f64, f: f64) -> Iv {
    Iv::p(a).add(Iv::p(f).mul(Iv::p(b).sub(Iv::p(a))))
}

// ---- exact rationals ----------------------------------------------------------------------

/// A signed big integer (little-endian 32-bit limbs, no leading zero limbs; zero is empty).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Big {
    neg: bool,
    mag: Vec<u32>,
}

fn trim(mut v: Vec<u32>) -> Vec<u32> {
    while v.last() == Some(&0) {
        v.pop();
    }
    v
}

fn mag_cmp(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    a.len()
        .cmp(&b.len())
        .then_with(|| a.iter().rev().cmp(b.iter().rev()))
}

fn mag_add(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len().max(b.len()) + 1);
    let mut carry = 0u64;
    for i in 0..a.len().max(b.len()) {
        let s = u64::from(*a.get(i).unwrap_or(&0)) + u64::from(*b.get(i).unwrap_or(&0)) + carry;
        out.push(s as u32);
        carry = s >> 32;
    }
    out.push(carry as u32);
    trim(out)
}

/// `a - b` for `a >= b`.
fn mag_sub(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for (i, &x) in a.iter().enumerate() {
        let mut s = i64::from(x) - i64::from(*b.get(i).unwrap_or(&0)) - borrow;
        borrow = 0;
        if s < 0 {
            s += 1 << 32;
            borrow = 1;
        }
        out.push(s as u32);
    }
    assert_eq!(borrow, 0);
    trim(out)
}

fn mag_mul(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = vec![0u64; a.len() + b.len() + 1];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            let t = out[i + j] + u64::from(x) * u64::from(y) + carry;
            out[i + j] = t & 0xffff_ffff;
            carry = t >> 32;
        }
        let mut k = i + b.len();
        while carry != 0 {
            let t = out[k] + carry;
            out[k] = t & 0xffff_ffff;
            carry = t >> 32;
            k += 1;
        }
    }
    trim(out.into_iter().map(|x| x as u32).collect())
}

impl Big {
    fn from_i128(x: i128) -> Big {
        let mut m = x.unsigned_abs();
        let mut mag = Vec::new();
        while m != 0 {
            mag.push(m as u32);
            m >>= 32;
        }
        Big { neg: x < 0, mag }
    }
    /// `2^k`.
    fn pow2(k: u32) -> Big {
        let mut mag = vec![0u32; (k / 32) as usize];
        mag.push(1 << (k % 32));
        Big { neg: false, mag }
    }
    fn add(&self, o: &Big) -> Big {
        if self.neg == o.neg {
            return Big {
                neg: self.neg,
                mag: mag_add(&self.mag, &o.mag),
            };
        }
        if mag_cmp(&self.mag, &o.mag) == std::cmp::Ordering::Less {
            Big {
                neg: o.neg,
                mag: mag_sub(&o.mag, &self.mag),
            }
        } else {
            let mag = mag_sub(&self.mag, &o.mag);
            Big {
                neg: self.neg && !mag.is_empty(),
                mag,
            }
        }
    }
    fn neg(&self) -> Big {
        Big {
            neg: !self.neg && !self.mag.is_empty(),
            mag: self.mag.clone(),
        }
    }
    fn mul(&self, o: &Big) -> Big {
        let mag = mag_mul(&self.mag, &o.mag);
        Big {
            neg: self.neg != o.neg && !mag.is_empty(),
            mag,
        }
    }
}

/// An exact rational `n / d`, `d > 0`: [`Ref::R`] references too wide for [`Dy`] (the
/// B-spline basis at an exact parameter).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rat {
    n: Big,
    d: Big,
}

impl Rat {
    /// `x` exactly.
    fn f(x: f64) -> Rat {
        let y = Dy::f(x);
        let m = Big::from_i128(y.m);
        if y.e >= 0 {
            Rat {
                n: m.mul(&Big::pow2(y.e as u32)),
                d: Big::from_i128(1),
            }
        } else {
            Rat {
                n: m,
                d: Big::pow2((-y.e) as u32),
            }
        }
    }
    fn add(&self, o: &Rat) -> Rat {
        Rat {
            n: self.n.mul(&o.d).add(&o.n.mul(&self.d)),
            d: self.d.mul(&o.d),
        }
    }
    fn sub(&self, o: &Rat) -> Rat {
        self.add(&Rat {
            n: o.n.neg(),
            d: o.d.clone(),
        })
    }
    fn mul(&self, o: &Rat) -> Rat {
        Rat {
            n: self.n.mul(&o.n),
            d: self.d.mul(&o.d),
        }
    }
    fn div(&self, o: &Rat) -> Rat {
        assert!(!o.n.mag.is_empty(), "division by 0");
        let n = self.n.mul(&o.d);
        let d = self.d.mul(&o.n);
        if d.neg {
            Rat {
                n: n.neg(),
                d: d.neg(),
            }
        } else {
            Rat { n, d }
        }
    }
    fn is_zero(&self) -> bool {
        self.n.mag.is_empty()
    }
    /// `|new − self|` against `|main − self|`, exactly.
    fn class(&self, new: f64, main: f64) -> Class {
        let a = Rat::f(new).sub(self);
        let b = Rat::f(main).sub(self);
        // |a.n| b.d against |b.n| a.d (both denominators positive)
        match mag_cmp(&mag_mul(&a.n.mag, &b.d.mag), &mag_mul(&b.n.mag, &a.d.mag)) {
            std::cmp::Ordering::Less => Class::Closer,
            std::cmp::Ordering::Equal => Class::Equal,
            std::cmp::Ordering::Greater => Class::Farther,
        }
    }
}

/// The degree-`p` B-spline basis over `knots` at the exact parameter `t` (Cox–de Boor in
/// exact rationals; `t` inside a knot span).
fn basis_exact(knots: &[f64], p: usize, t: f64) -> Vec<Rat> {
    let k: Vec<Rat> = knots.iter().map(|&x| Rat::f(x)).collect();
    let zero = Rat::f(0.0);
    let n = knots.len() - p - 1;
    assert!(knots[p] <= t && t < knots[n], "t inside the domain");
    let mut nb: Vec<Rat> = (0..knots.len() - 1)
        .map(|i| {
            if knots[i] <= t && t < knots[i + 1] {
                Rat::f(1.0)
            } else {
                zero.clone()
            }
        })
        .collect();
    let t = Rat::f(t);
    for d in 1..=p {
        let mut next = Vec::new();
        for i in 0..knots.len() - 1 - d {
            let mut acc = zero.clone();
            let den1 = k[i + d].sub(&k[i]);
            if !den1.is_zero() {
                acc = acc.add(&t.sub(&k[i]).div(&den1).mul(&nb[i]));
            }
            let den2 = k[i + d + 1].sub(&k[i + 1]);
            if !den2.is_zero() {
                acc = acc.add(&k[i + d + 1].sub(&t).div(&den2).mul(&nb[i + 1]));
            }
            next.push(acc);
        }
        nb = next;
    }
    nb.truncate(n);
    nb
}

/// One coordinate of a heal case's face corner: the surface's net has that coordinate
/// equal to `vals[i]` along the knot direction `knots` (degree `p`) whatever the other
/// index, so the corner's coordinate is `Σ N_i(t) vals[i]` at the domain bound `t`, exactly.
fn corner(knots: &[f64], p: usize, vals: &[f64], t: f64) -> Rat {
    basis_exact(knots, p, t)
        .iter()
        .zip(vals)
        .fold(Rat::f(0.0), |acc, (b, &v)| acc.add(&b.mul(&Rat::f(v))))
}

// ---- the binormal record's orientation -------------------------------------------------

/// [`binormal_record`] with the edge use's and face's orientation signs read off the
/// output: the one sign pair whose record lies within `1e-9` of `got` (12 values) in every
/// component (the pairs differ by the size of the unit vectors in some component, so at
/// most one fits).
fn fitted_record(s: &Bss, u: Iv, v: Iv, along_u: bool, got: &[f64]) -> [Iv; 12] {
    let fits: Vec<[Iv; 12]> = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)]
        .into_iter()
        .map(|(st, sn)| binormal_record(s, u, v, along_u, st, sn))
        .filter(|r| {
            r.iter()
                .zip(got)
                .all(|(iv, &g)| (0.5 * (iv.lo + iv.hi) - g).abs() <= 1e-9)
        })
        .collect();
    assert_eq!(fits.len(), 1, "exactly one orientation fits {got:?}");
    fits[0]
}

/// A bicubic net `x = 0.001 i`, `y = 0.001 j` with the given heights (the `face_bss_tiny`
/// fixture of `topology.edgeuse_*`, knots `[0,0,0,0,1,1,1,1]` both ways).
fn tiny_bicubic() -> Bss {
    let z = [
        [-0.0004, 0.0002, -0.0002, 0.0004],
        [0.0, -0.0004, 0.0002, -0.0002],
        [0.0004, 0.0, -0.0004, 0.0002],
        [-0.0002, 0.0004, 0.0, -0.0004],
    ];
    let c = [0.0, 0.001, 0.002, 0.003];
    Bss {
        net: (0..4)
            .map(|i| (0..4).map(|j| [c[i], c[j], z[i][j]]).collect())
            .collect(),
        p: 3,
        q: 3,
        u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    }
}

// ---- heal corners ----------------------------------------------------------------------

/// `heal.heal_data_p2/gen0161`: the u knots (degree 3) of its surface, whose net has
/// `x = 0.001 i`.
const GEN0161_X: [f64; 10] = [
    0.0,
    0.0,
    0.0,
    0.0,
    0.26179478588024485,
    0.5822897670658597,
    1.0,
    1.0,
    1.0,
    1.0,
];
/// `heal.heal_data_p2/gen0181`: the v knots (degree 3) of its surface, whose net has
/// `y = 0.001 j`.
const GEN0181_V: [f64; 10] = [
    0.0,
    0.0,
    0.0,
    0.0,
    0.09333091863767201,
    0.8901272216798009,
    1.0,
    1.0,
    1.0,
    1.0,
];

/// `topology.edgeuse_p1` (20 rows): corner gaps between iso edges of one surface (ref 0),
/// and the parameter `0.9` of the drop at the corner.
#[test]
fn edgeuse_p1_corner_gaps() {
    let rows = vec![
        Row {
            case: "face_bss_tiny_gaps0_1_0",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                4,
                1.1175700191704681e-19,
                2.710505431213761e-20,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps0_1_1",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                4,
                1.1175700191704681e-19,
                2.710505431213761e-20,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps0_3_0",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                5,
                1.1175700191704681e-19,
                2.710505431213761e-20,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps0_3_1",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                5,
                1.1175700191704681e-19,
                2.710505431213761e-20,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps1_1_0",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                4,
                6.776263578034403e-21,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps1_1_1",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                4,
                6.776263578034403e-21,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps1_3_0",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                5,
                6.776263578034403e-21,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps1_3_1",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                5,
                6.776263578034403e-21,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps2_1_0",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                4,
                1.3011131911468772e-18,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps2_1_1",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                4,
                1.3011131911468772e-18,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps2_3_0",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                5,
                1.3011131911468772e-18,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps2_3_1",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                5,
                1.3011131911468772e-18,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps3_0_0",
            want: RowClass::Mixed,
            els: vec![
                el(
                    "state",
                    4,
                    6.562021981706704e-13,
                    6.562023637809558e-13,
                    Ref::X(d(0.0)),
                    Class::Farther,
                ),
                el(
                    "state",
                    7,
                    0.9000000000929206,
                    0.9000000000929202,
                    Ref::X(d(0.9)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "face_bss_tiny_gaps3_0_1",
            want: RowClass::Mixed,
            els: vec![
                el(
                    "state",
                    4,
                    6.562021981706704e-13,
                    6.562023637809558e-13,
                    Ref::X(d(0.0)),
                    Class::Farther,
                ),
                el(
                    "state",
                    7,
                    0.9000000000929206,
                    0.9000000000929202,
                    Ref::X(d(0.9)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "face_bss_tiny_gaps3_1_0",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                4,
                1.7366282543562488e-18,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps3_1_1",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                4,
                1.7366282543562488e-18,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps3_2_0",
            want: RowClass::Mixed,
            els: vec![
                el(
                    "state",
                    5,
                    6.562021981706704e-13,
                    6.562023637809558e-13,
                    Ref::X(d(0.0)),
                    Class::Farther,
                ),
                el(
                    "state",
                    8,
                    0.9000000000929206,
                    0.9000000000929202,
                    Ref::X(d(0.9)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "face_bss_tiny_gaps3_2_1",
            want: RowClass::Mixed,
            els: vec![
                el(
                    "state",
                    5,
                    6.562021981706704e-13,
                    6.562023637809558e-13,
                    Ref::X(d(0.0)),
                    Class::Farther,
                ),
                el(
                    "state",
                    8,
                    0.9000000000929206,
                    0.9000000000929202,
                    Ref::X(d(0.9)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "face_bss_tiny_gaps3_3_0",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                5,
                1.7366282543562488e-18,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face_bss_tiny_gaps3_3_1",
            want: RowClass::Closer,
            els: vec![el(
                "state",
                5,
                1.7366282543562488e-18,
                0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
    ];
    check("topology.edgeuse_p1", &rows);
}

/// `topology.vertex` (12 rows): the vertex's gaps to its edges and face (ref 0); the index of
/// the largest among gaps that are all 0 is a tie.
#[test]
fn vertex_gaps() {
    let rows = vec![
        Row {
            case: "patch_sub_v0_gaps_g0",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    2.2887833992611187e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "state",
                    13,
                    2.2887833992611187e-16,
                    1.3877787807814457e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "state",
                    16,
                    2.2247786310271853e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v0_gaps_g1",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    2.2887833992611187e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "state",
                    13,
                    2.2887833992611187e-16,
                    1.3877787807814457e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "state",
                    16,
                    2.2247786310271853e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v0_gaps_noarr_g0",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    2.2887833992611187e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v0_gaps_noarr_g1",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    2.2887833992611187e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v0_gaps_tol_g0",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    2.2887833992611187e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v0_gaps_tol_g1",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    2.2887833992611187e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v1_gaps_g0",
            want: RowClass::Mixed,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    1.1443916996305594e-16,
                    5.551115123125783e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "state",
                    13,
                    1.1443916996305594e-16,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "state",
                    16,
                    2.7755575615628914e-17,
                    5.551115123125783e-17,
                    Ref::X(d(0.0)),
                    Class::Farther,
                ),
            ],
        },
        Row {
            case: "patch_sub_v1_gaps_g1",
            want: RowClass::Mixed,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    1.1443916996305594e-16,
                    5.551115123125783e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "state",
                    13,
                    1.1443916996305594e-16,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "state",
                    16,
                    2.7755575615628914e-17,
                    5.551115123125783e-17,
                    Ref::X(d(0.0)),
                    Class::Farther,
                ),
            ],
        },
        Row {
            case: "patch_sub_v1_gaps_noarr_g0",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    1.1443916996305594e-16,
                    5.551115123125783e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v1_gaps_noarr_g1",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    1.1443916996305594e-16,
                    5.551115123125783e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v1_gaps_tol_g0",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    1.1443916996305594e-16,
                    5.551115123125783e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "patch_sub_v1_gaps_tol_g1",
            want: RowClass::Closer,
            els: vec![
                el("state", 1, 0.0, 1.0, Ref::Tie, Class::Equal),
                el(
                    "state",
                    3,
                    1.1443916996305594e-16,
                    5.551115123125783e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
    ];
    check("topology.vertex", &rows);
}

/// `topology.loopuse` (3 rows): the loop's closing gap (ref 0).
#[test]
fn loopuse_gaps() {
    let rows = vec![
        Row {
            case: "extra00214",
            want: RowClass::Closer,
            els: vec![
                el("q", 1, 3.0, 0.0, Ref::Tie, Class::Equal),
                el(
                    "gap_tol",
                    0,
                    4.441434166503682e-16,
                    1.3877787807814457e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "face06_001_closed3d",
            want: RowClass::Closer,
            els: vec![el(
                "gap_tol",
                0,
                1.7366282543562488e-18,
                2.710505431213761e-20,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
        Row {
            case: "face06_011_closed3d",
            want: RowClass::Closer,
            els: vec![el(
                "gap_tol",
                0,
                1.7366282543562488e-18,
                2.710505431213761e-20,
                Ref::X(d(0.0)),
                Class::Closer,
            )],
        },
    ];
    check("topology.loopuse", &rows);
}

/// `topology.edgeuse_p2/face_bspline6_so3`: the at-edge binormal record of the failed
/// step-off at `u = 0.1 + 0.4 (0.7 − 0.1)`, `v = 0.9` on the tiny bicubic (enclosed), and
/// the tangent's `y`, `−S_u·y = 0` exactly (the net's `y` does not depend on `i`).
#[test]
fn edgeuse_p2_step_off() {
    let got = [
        0.0010199999999999999,
        0.0027,
        2.3484400000000012e-5,
        -0.012215141921114881,
        -0.9949621571848674,
        -0.09950425155681722,
        -0.003,
        -0.0,
        0.00036827999999999996,
        0.12123148992347355,
        -0.10025121330954226,
        0.987548793772186,
    ];
    let rec = fitted_record(
        &tiny_bicubic(),
        lerp_iv(0.1, 0.7, 0.4),
        Iv::p(0.9),
        true,
        &got,
    );
    let rows = vec![Row {
        case: "face_bspline6_so3",
        want: RowClass::Closer,
        els: vec![
            el(
                "state",
                2,
                0.0027000000000000006,
                0.0027,
                Ref::I(rec[1]),
                Class::Equal,
            ),
            el(
                "state",
                4,
                -0.012215141921115306,
                -0.012215141921114881,
                Ref::I(rec[3]),
                Class::Equal,
            ),
            el(
                "state",
                6,
                -0.09950425155681716,
                -0.09950425155681722,
                Ref::I(rec[5]),
                Class::Equal,
            ),
            el(
                "state",
                8,
                1.3010426069826053e-18,
                -0.0,
                Ref::X(d(0.0)),
                Class::Closer,
            ),
        ],
    }];
    check("topology.edgeuse_p2", &rows);
}

/// `query.edge` (5 rows): bounding boxes of iso edges. A coordinate the edge does not
/// move in is the iso parameter mapped through a linear net (`x = 1e5 u`, `y = 200 v`, the
/// first span of a degree-1 `v`: `y = 1e5 v / v_1`); a frame component of the iso
/// direction is 0.
#[test]
fn query_edge_bounds() {
    let rows = vec![
        Row {
            case: "gen0092",
            want: RowClass::Closer,
            els: vec![
                el(
                    "polar",
                    1,
                    -3.5742292322907466e-17,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "polar",
                    5,
                    2.196827488852294e-17,
                    -0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "polar",
                    6,
                    6.382930325415839e-17,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "polar",
                    7,
                    5.726494211114558e-17,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "gen0206",
            want: RowClass::Closer,
            els: vec![
                el(
                    "normal",
                    0,
                    3299.051808158839,
                    3299.0518081588393,
                    Ref::X(d(1e5).mul(d(0.03299051808158839))),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    1,
                    -8.974509622624073e-17,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    3,
                    8.974340237896693e-17,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    5,
                    5.5138562190516385e-19,
                    -0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                open(
                    "pseudo",
                    9,
                    3314.5426176583233,
                    3314.542617658327,
                    "min of the edge across the rotated frame: no closed form",
                ),
                el(
                    "pseudo",
                    11,
                    41736.30105899899,
                    41736.301058998986,
                    Ref::Q(d(1e5).mul(d(0.26881434727912235)), d(0.6440780338897854)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    12,
                    41736.30105899899,
                    41736.301058998986,
                    Ref::Q(d(1e5).mul(d(0.26881434727912235)), d(0.6440780338897854)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "gen0251",
            want: RowClass::Closer,
            els: vec![
                el(
                    "pseudo",
                    0,
                    -1.24018733608619e-16,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    3,
                    -5.65406748516537e-20,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    7,
                    -1.2401874649718695e-16,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    8,
                    4.939905957198947e-32,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                open(
                    "pseudo",
                    9,
                    0.19999476191092086,
                    0.19999476191092094,
                    "min of the edge across the rotated frame: no closed form",
                ),
                el(
                    "pseudo",
                    13,
                    -0.630355232614607,
                    -0.6303552326146066,
                    Ref::Q(d(0.22600382972921754).neg(), d(0.35853407417876426)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    14,
                    -0.6303552326146069,
                    -0.6303552326146066,
                    Ref::Q(d(0.22600382972921754).neg(), d(0.35853407417876426)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "gen0333",
            want: RowClass::Closer,
            els: vec![
                el(
                    "normal",
                    1,
                    21.91135284130186,
                    21.911352841301863,
                    Ref::X(d(200.0).mul(d(0.10955676420650934))),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    1,
                    2.346121032598851e-17,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    4,
                    9.758658242656762e-17,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    6,
                    -4.3126415412265675e-17,
                    -0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    8,
                    9.062936474093946e-17,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    13,
                    21.911352841301852,
                    21.91135284130186,
                    Ref::X(d(200.0).mul(d(0.10955676420650934))),
                    Class::Closer,
                ),
                el(
                    "pseudo",
                    14,
                    21.911352841301856,
                    21.91135284130186,
                    Ref::X(d(200.0).mul(d(0.10955676420650934))),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "gen0355",
            want: RowClass::Undecided,
            els: vec![open(
                "closest",
                1,
                1.038246192185557e-12,
                1.0378527319431887e-12,
                "the distance of a point 1e-12 off the face: no closed form",
            )],
        },
    ];
    check("query.edge", &rows);
}

/// `heal.heal_data_p1` (gen0063, gen0242): edge-edge gaps at the face's corners (ref 0;
/// the edges are iso curves of one surface), their argmax index (a tie), and an
/// edge-face gap ([`Ref::Cert`]). gen0242 (coordinates to 4000): the corner gaps shrink
/// (4.5e-13 to 8.9e-15 and 1.4e-14), CLOSER.
#[test]
fn heal_p1_gaps() {
    let rows = vec![
        Row {
            case: "gen0063",
            want: RowClass::Closer,
            els: vec![
                el("out", 110, 1.0, 7.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    111,
                    3.3308497693360055e-16,
                    1.3877787807814457e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el("out", 136, 6.0, 2.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    137,
                    3.3308497693360055e-16,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "gen0242",
            want: RowClass::Closer,
            els: vec![
                el(
                    "out",
                    234,
                    4.547473508864641e-13,
                    8.881784197001252e-15,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    286,
                    8.526512829121202e-14,
                    1.4210854715202004e-14,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el("out", 311, 3.0, 5.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    312,
                    4.547473508864641e-13,
                    1.4210854715202004e-14,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    314,
                    2.5550568800550892e-09,
                    2.555052069280629e-09,
                    Ref::Cert,
                    Class::Equal,
                ),
            ],
        },
    ];
    check("heal.heal_data_p1", &rows);
}

/// `heal.heal_data_p2` (6 rows): vertex-edge, vertex-face and edge-edge gaps at the face's
/// corners (ref 0), argmax indices (ties), edge-face gaps ([`Ref::Cert`]), and a corner
/// vertex coordinate in the voxel list and vertex box, exactly [`corner`].
#[test]
fn heal_p2_gaps() {
    let rows = vec![
        Row {
            case: "gen0085",
            want: RowClass::Equal,
            els: vec![el(
                "out",
                289,
                1.4828392756372229e-12,
                1.482683538041431e-12,
                Ref::Cert,
                Class::Equal,
            )],
        },
        Row {
            case: "gen0138",
            want: RowClass::Closer,
            els: vec![
                el(
                    "out",
                    88,
                    4.0194366942304644e-14,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el("out", 165, 5.0, 3.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    166,
                    1.1718571004216928e-13,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    168,
                    1.668595474901957e-10,
                    1.6640740971431822e-10,
                    Ref::Cert,
                    Class::Equal,
                ),
            ],
        },
        Row {
            case: "gen0161",
            want: RowClass::Mixed,
            els: vec![
                el(
                    "out",
                    91,
                    4.345270763724971e-19,
                    2.710505431213761e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    93,
                    4.503029819097855e-19,
                    9.075134872523191e-19,
                    Ref::X(d(0.0)),
                    Class::Farther,
                ),
                el(
                    "out",
                    107,
                    1.21217487951527e-19,
                    5.421010862427522e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    109,
                    2.439454888092385e-19,
                    1.5570668253389024e-19,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    139,
                    4.337338053226252e-19,
                    1.3552527156068805e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el("out", 161, 4.0, 0.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    162,
                    4.345270763724971e-19,
                    5.421010862427522e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    164,
                    4.700439481382968e-16,
                    4.702354842745572e-16,
                    Ref::Cert,
                    Class::Equal,
                ),
                el(
                    "out",
                    188,
                    1.21217487951527e-19,
                    5.421010862427522e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    214,
                    4.337338053226252e-19,
                    1.3552527156068805e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    240,
                    4.345270763724971e-19,
                    2.710505431213761e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    242,
                    4.3613132892691186e-13,
                    4.3613130612250355e-13,
                    Ref::Cert,
                    Class::Equal,
                ),
                el(
                    "out",
                    3082,
                    0.0005183962187553675,
                    0.0005183962187553674,
                    Ref::R(corner(
                        &GEN0161_X,
                        3,
                        &[0.0, 0.001, 0.002, 0.003, 0.004, 0.005],
                        0.050261185643038477,
                    )),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "gen0181",
            want: RowClass::Closer,
            els: vec![
                el(
                    "out",
                    94,
                    8.690541527449943e-19,
                    5.421010862427522e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    126,
                    4.84869951806108e-19,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    128,
                    4.87890977618477e-19,
                    3.114133650677805e-19,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    165,
                    8.690541527449943e-19,
                    5.421010862427522e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    191,
                    4.84869951806108e-19,
                    0.0,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    193,
                    9.081025587056003e-13,
                    9.08102641617676e-13,
                    Ref::Cert,
                    Class::Equal,
                ),
                el("out", 216, 6.0, 2.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    217,
                    4.84869951806108e-19,
                    2.710505431213761e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    243,
                    8.690541527449943e-19,
                    5.421010862427522e-20,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    245,
                    2.328384760786022e-13,
                    2.328376082512667e-13,
                    Ref::Cert,
                    Class::Equal,
                ),
                el(
                    "out",
                    3073,
                    0.0016014032259786147,
                    0.0016014032259786149,
                    Ref::R(corner(
                        &GEN0181_V,
                        3,
                        &[0.0, 0.001, 0.002, 0.003, 0.004, 0.005],
                        0.2128918451599648,
                    )),
                    Class::Closer,
                ),
                el(
                    "out",
                    3091,
                    0.0016014032259786147,
                    0.0016014032259786149,
                    Ref::R(corner(
                        &GEN0181_V,
                        3,
                        &[0.0, 0.001, 0.002, 0.003, 0.004, 0.005],
                        0.2128918451599648,
                    )),
                    Class::Closer,
                ),
                el(
                    "out",
                    3096,
                    0.0016014032259786147,
                    0.0016014032259786149,
                    Ref::R(corner(
                        &GEN0181_V,
                        3,
                        &[0.0, 0.001, 0.002, 0.003, 0.004, 0.005],
                        0.2128918451599648,
                    )),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "gen0536",
            want: RowClass::Closer,
            els: vec![
                el("out", 108, 1.0, 7.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    109,
                    1.1107649934270853e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    135,
                    1.1107649934270853e-16,
                    3.469446951953614e-18,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el("out", 160, 1.0, 7.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    161,
                    1.1107649934270853e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    187,
                    1.1107649934270853e-16,
                    3.469446951953614e-18,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el("out", 212, 1.0, 7.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    213,
                    1.1107649934270853e-16,
                    2.7755575615628914e-17,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    239,
                    1.1107649934270853e-16,
                    3.469446951953614e-18,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
            ],
        },
        Row {
            case: "gen0576",
            want: RowClass::Closer,
            els: vec![
                el("out", 96, 5.0, 3.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    97,
                    5.084229945850415e-13,
                    1.4210854715202004e-14,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el("out", 122, 2.0, 6.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    123,
                    5.084229945850415e-13,
                    2.842170943040401e-14,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el("out", 148, 1.0, 7.0, Ref::Tie, Class::Equal),
                el(
                    "out",
                    149,
                    2.360884497795176e-13,
                    5.684341886080802e-14,
                    Ref::X(d(0.0)),
                    Class::Closer,
                ),
                el(
                    "out",
                    151,
                    9.600589354804734e-12,
                    9.543813662690915e-12,
                    Ref::Cert,
                    Class::Equal,
                ),
            ],
        },
    ];
    check("heal.heal_data_p2", &rows);
}
