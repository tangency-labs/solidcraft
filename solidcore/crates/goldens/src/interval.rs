//! Interval arithmetic for the tests' certificates (#18195): every operation rounds its
//! bounds outward by one unit in the last place. Round to nearest is off by less than one
//! unit and is monotone, so an interval result holds both the exact result and the
//! floating-point result (rounded to nearest, in the same order of operations) of the same
//! operations on any operands inside the operand intervals.

use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::math::{gamma, NOW};

/// A closed interval `[lo, hi]`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Iv {
    /// The lower bound.
    pub lo: f64,
    /// The upper bound.
    pub hi: f64,
}

impl Iv {
    /// `[0, 0]`.
    pub const ZERO: Self = Self { lo: 0.0, hi: 0.0 };

    /// `[x, x]`.
    pub fn exact(x: f64) -> Self {
        Self { lo: x, hi: x }
    }

    /// `[x - e, x + e]`, rounded outward.
    pub fn around(x: f64, e: f64) -> Self {
        Self::out(x - e, x + e)
    }

    fn out(lo: f64, hi: f64) -> Self {
        Self {
            lo: lo.next_down(),
            hi: hi.next_up(),
        }
    }

    fn hull(c: [f64; 4]) -> Self {
        Self::out(
            c.iter().copied().fold(f64::INFINITY, f64::min),
            c.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )
    }

    /// The sum.
    pub fn add(self, o: Self) -> Self {
        Self::out(self.lo + o.lo, self.hi + o.hi)
    }

    /// The difference.
    pub fn sub(self, o: Self) -> Self {
        Self::out(self.lo - o.hi, self.hi - o.lo)
    }

    /// The product.
    pub fn mul(self, o: Self) -> Self {
        Self::hull([
            self.lo * o.lo,
            self.lo * o.hi,
            self.hi * o.lo,
            self.hi * o.hi,
        ])
    }

    /// The quotient by an interval of positive numbers.
    pub fn div(self, o: Self) -> Self {
        assert!(o.lo > 0.0, "a divisor interval not positive: {o:?}");
        Self::hull([
            self.lo / o.lo,
            self.lo / o.hi,
            self.hi / o.lo,
            self.hi / o.hi,
        ])
    }

    /// The square root (of the interval's non-negative part).
    pub fn sqrt(self) -> Self {
        Self::out(self.lo.max(0.0).sqrt(), self.hi.sqrt())
    }

    /// The largest `|x|` over the interval.
    pub fn mag(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }

    /// The least `|x|` over the interval.
    pub fn mig(self) -> f64 {
        if self.lo <= 0.0 && 0.0 <= self.hi {
            0.0
        } else {
            self.lo.abs().min(self.hi.abs())
        }
    }

    /// Whether `x` lies in the interval.
    pub fn contains(self, x: f64) -> bool {
        self.lo <= x && x <= self.hi
    }
}

/// `a + b` exactly, as the rounded sum and its error (Knuth's `TwoSum`, round to nearest).
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// `sin` and `cos` of `deg` degrees, enclosed: `x = deg pi / 180` with `pi` enclosed by
/// its neighbours, then the Taylor series summed in intervals until the next term is
/// below `1e-30`, plus that next term's magnitude, which bounds the Lagrange remainder
/// `|x|^m / m!` of the series cut before power `m`.
pub(crate) fn sin_cos_deg(deg: f64) -> (Iv, Iv) {
    let pi = Iv {
        lo: std::f64::consts::PI.next_down(),
        hi: std::f64::consts::PI.next_up(),
    };
    let x = Iv::exact(deg).mul(pi).div(Iv::exact(180.0));
    let x2 = x.mul(x);
    let series = |first: Iv, power: usize| {
        let (mut sum, mut term, mut m) = (Iv::ZERO, first, power);
        loop {
            sum = sum.add(term);
            term = Iv::ZERO
                .sub(term.mul(x2))
                .div(Iv::exact(((m + 1) * (m + 2)) as f64));
            m += 2;
            if m > 2 + x.mag() as usize && term.mag() < 1e-30 {
                let r = term.mag();
                return sum.add(Iv { lo: -r, hi: r });
            }
        }
    };
    (series(x, 1), series(Iv::exact(1.0), 0))
}

/// `a b` exactly, as the rounded product and its error (the fused multiply-add rounds
/// once, and the error of a rounded product is representable).
pub(crate) fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    (p, a.mul_add(b, -p))
}

/// The exact sum of `terms`, enclosed: Shewchuk's grow-expansion (*Adaptive Precision
/// Floating-Point Arithmetic and Fast Robust Geometric Predicates*, 1997) keeps it as
/// non-overlapping components exactly, which are then added in interval arithmetic,
/// smallest first, so the interval is a few units wide in the last place of the sum.
pub(crate) fn exact_sum(terms: &[f64]) -> Iv {
    let mut parts: Vec<f64> = Vec::new();
    for &t in terms {
        let mut q = t;
        let mut next = Vec::new();
        for &c in &parts {
            let (s, e) = two_sum(q, c);
            if e != 0.0 {
                next.push(e);
            }
            q = s;
        }
        next.push(q);
        parts = next;
    }
    parts.iter().fold(Iv::ZERO, |acc, &c| acc.add(Iv::exact(c)))
}

/// [`gamma`] (the kernel's `gamma_n = n u / (1 - n u)`, Higham, *Accuracy and Stability of
/// Numerical Algorithms*, 2nd ed., section 3.1: a sum of `n + 1` terms in any order is
/// within `gamma_n` times the sum of their magnitudes of the exact sum), one unit up for
/// its own two roundings, which are far below a unit of the result.
pub(crate) fn gamma_up(n: usize) -> f64 {
    gamma(n).next_up().next_up()
}

/// `C(n, k)`, exact: computed in integers (each partial product `i C(n, i)` is checked) and
/// asserted below 2^53, so the `f64` is the integer itself.
pub(crate) fn binom(n: usize, k: usize) -> f64 {
    let c = (1..=k as u64).fold(1_u64, |a, i| {
        a.checked_mul(n as u64 + 1 - i)
            .expect("a binomial past u64")
            / i
    });
    assert!(c < 1 << 53, "C({n}, {k}) is not exact in f64");
    c as f64
}

/// A tensor polynomial in Bernstein form of degrees `(p, q)` on `[0, 1]^2`,
/// coefficient `(i, j)` at `i * (q + 1) + j` (a curve has `q = 0`).
#[derive(Clone, Debug)]
pub(crate) struct Bern {
    /// The degree in u.
    pub p: usize,
    /// The degree in v (0 for a curve).
    pub q: usize,
    /// The coefficients.
    pub c: Vec<Iv>,
}

impl Bern {
    pub(crate) fn zip(&self, o: &Self, f: impl Fn(Iv, Iv) -> Iv) -> Self {
        assert_eq!((self.p, self.q), (o.p, o.q));
        let c = self.c.iter().zip(&o.c).map(|(&a, &b)| f(a, b)).collect();
        Self { c, ..*self }
    }

    pub(crate) fn scale(&self, s: f64) -> Self {
        let c = self.c.iter().map(|&a| a.mul(Iv::exact(s))).collect();
        Self { c, ..*self }
    }

    /// The product: `c_kl = sum a_ij b_ab C(p,i) C(p',a) C(q,j) C(q',b) /
    /// (C(p+p', k) C(q+q', l))` over `i + a = k`, `j + b = l` (Farouki and Rajan 1988).
    pub(crate) fn mul(&self, o: &Self) -> Self {
        let (p, q) = (self.p + o.p, self.q + o.q);
        let mut c = vec![Iv::ZERO; (p + 1) * (q + 1)];
        for i in 0..=self.p {
            for j in 0..=self.q {
                for a in 0..=o.p {
                    for b in 0..=o.q {
                        // (each factor exact; their products and quotient enclosed, as
                        // they round once past 2^53)
                        let f = Iv::exact(binom(self.p, i))
                            .mul(Iv::exact(binom(o.p, a)))
                            .mul(Iv::exact(binom(self.q, j)))
                            .mul(Iv::exact(binom(o.q, b)));
                        let d = Iv::exact(binom(p, i + a)).mul(Iv::exact(binom(q, j + b)));
                        let w = f.div(d);
                        let t = self.c[i * (self.q + 1) + j]
                            .mul(o.c[a * (o.q + 1) + b])
                            .mul(w);
                        let at = (i + a) * (q + 1) + j + b;
                        c[at] = c[at].add(t);
                    }
                }
            }
        }
        Self { p, q, c }
    }

    /// The two halves of a Bernstein row at 1/2 (de Casteljau).
    fn halves(row: &[Iv]) -> (Vec<Iv>, Vec<Iv>) {
        let mut work = row.to_vec();
        let n = work.len();
        let (mut left, mut right) = (vec![work[0]], vec![work[n - 1]]);
        for r in 1..n {
            for i in 0..n - r {
                work[i] = work[i].add(work[i + 1]).mul(Iv::exact(0.5));
            }
            left.push(work[0]);
            right.push(work[n - 1 - r]);
        }
        right.reverse();
        (left, right)
    }

    /// The halves at u = 1/2 (`along_u`) or v = 1/2.
    pub(crate) fn split(&self, along_u: bool) -> (Self, Self) {
        let (p, q) = (self.p, self.q);
        let (mut a, mut b) = (self.clone(), self.clone());
        let lines: Vec<Vec<usize>> = if along_u {
            (0..=q)
                .map(|j| (0..=p).map(|i| i * (q + 1) + j).collect())
                .collect()
        } else {
            (0..=p)
                .map(|i| (0..=q).map(|j| i * (q + 1) + j).collect())
                .collect()
        };
        for line in lines {
            let row: Vec<Iv> = line.iter().map(|&at| self.c[at]).collect();
            let (l, r) = Self::halves(&row);
            for (n, &at) in line.iter().enumerate() {
                a.c[at] = l[n];
                b.c[at] = r[n];
            }
        }
        (a, b)
    }

    /// The largest `|c|` over the pieces after `depth` halvings each way: every value
    /// of the polynomial lies in the convex hull of a piece's coefficients.
    pub(crate) fn hull_mag(&self, depth: u32) -> f64 {
        let mut pieces = vec![self.clone()];
        for _ in 0..depth {
            let mut next = Vec::new();
            for piece in pieces {
                let (a, b) = piece.split(true);
                for half in [a, b] {
                    if half.q == 0 {
                        next.push(half);
                    } else {
                        let (c, d) = half.split(false);
                        next.extend([c, d]);
                    }
                }
            }
            pieces = next;
        }
        pieces
            .iter()
            .flat_map(|b| b.c.iter())
            .map(|c| c.mag())
            .fold(0.0, f64::max)
    }
}

/// The control net of one rational Bézier piece, rows `i` (u) by columns `j` (v),
/// homogeneous (a non-rational point has weight 1).
pub(crate) fn homogeneous(net: &[Vec<CPoint>]) -> [Bern; 4] {
    let p = net.len() - 1;
    let q = net[0].len() - 1;
    let mut out: [Bern; 4] = std::array::from_fn(|_| Bern {
        p,
        q,
        c: Vec::new(),
    });
    for row in net {
        for c in row {
            let w = if c.w == NOW { 1.0 } else { c.w };
            for (o, x) in out.iter_mut().zip([c.x, c.y, c.z, w]) {
                o.c.push(Iv::exact(x));
            }
        }
    }
    out
}

impl Bern {
    /// The value at `(u, v)` by de Casteljau in interval arithmetic: it holds the
    /// polynomial's exact value there.
    pub(crate) fn at(&self, u: f64, v: f64) -> Iv {
        let at = |row: &[Iv], t: f64| {
            let mut work = row.to_vec();
            let (t, s) = (Iv::exact(t), Iv::exact(1.0).sub(Iv::exact(t)));
            for r in 1..work.len() {
                for i in 0..work.len() - r {
                    work[i] = work[i].mul(s).add(work[i + 1].mul(t));
                }
            }
            work[0]
        };
        let cols: Vec<Iv> = (0..=self.p)
            .map(|i| at(&self.c[i * (self.q + 1)..(i + 1) * (self.q + 1)], v))
            .collect();
        at(&cols, u)
    }
}
