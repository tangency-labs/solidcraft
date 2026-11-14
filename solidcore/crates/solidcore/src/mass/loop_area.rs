//! A certified enclosure `[lo, hi]` of the signed area of a closed loop of planar NURBS
//! curves, and the area's sign decided from it (not upstream).
//!
//! The loop is the curves in turn, each taken forwards or backwards, and the straight
//! segments from each curve's end to the next one's start (the last back to the first):
//! a closed curve, whatever gaps its curves leave. Its signed area (positive counter-
//! clockwise, each point counted by its winding number) is, by Green's theorem,
//! `oint (x - x0) dy` for any `x0`; `x0` is taken at the loop's first point, so no term
//! carries the loop's distance from the origin.
//!
//! - **Pieces.** On a polynomial piece of a curve (one knot span) the integrand
//!   `f(t) = (x(t) - x0) y'(t)` is a Taylor model on a cell `[a, b]` of the span [1, 2]:
//!   its Taylor polynomial of degree `ORDER - 1` at the cell's middle `m`, every
//!   coefficient enclosed at `m` (the curve's derivatives there, [`curve_derivs_over`],
//!   rounded interval arithmetic, [`Ivl`]) and integrated exactly, plus the Lagrange
//!   remainder `|f^(ORDER)| / ORDER! int |t - m|^ORDER`, the derivative enclosed over the
//!   whole cell. On a polynomial (non-rational) piece the derivatives past its degree are
//!   0 and the remainder vanishes: the cell is exact up to rounding.
//! - **Closing segments.** From `P` to `Q`, `((P_x - x0) + (Q_x - x0)) (Q_y - P_y) / 2`
//!   exactly, with `P` and `Q` the curves' end points enclosed.
//!
//! The enclosure refines the cell of widest enclosure, one bisection at a time, until the
//! sign is decided (the enclosure excludes 0), or [`CELL_BUDGET`] cells are spent, or no
//! cell is left to cut; then the sign is [`LoopAreaSign::Undecided`]. A loop whose area is
//! 0 (a curve and its way back, two equal lobes of opposite sense) never decides: every
//! enclosure holds its area.
//!
//! References: [1] G. F. Corliss, L. B. Rall, Adaptive, self-validating numerical
//! quadrature, *SIAM J. Sci. Stat. Comput.* 8(5) (1987); [2] K. Makino, M. Berz, Taylor
//! models and other validated functional inclusion methods, *Int. J. Pure Appl. Math.* 4
//! (2003).
//!
//! Deviations: none (not upstream). Bounded work: at most [`CELL_BUDGET`] cells and
//! [`MAX_DEPTH`] bisections of one cell, no recursion (a work list ordered by width, then by
//! insertion), the kernel's cancel token polled every 64 cells.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::knot;
use crate::kernel::Kernel;
use crate::{Result, SolidError};

const OP: &str = "mass::loop_area";

/// The degree of a cell's remainder: its Taylor polynomial has degree `ORDER - 1`.
pub const ORDER: usize = 8;

/// The cells past which an enclosure is no longer refined. A bound on the work of one
/// decision, not fitted to a case: a loop whose area is not near 0 decides on its starting
/// cells or within a few bisections of each (the quarter annulus of radius 1e4 and width
/// 1e-3, area 15.7 against a box of 1e8, takes under a hundred).
pub const CELL_BUDGET: usize = 1 << 12;

/// The most bisections from a starting cell to one of its cells: past it a cell is kept
/// as it is.
pub const MAX_DEPTH: u32 = 60;

/// An enclosure of a loop's signed area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopArea {
    /// A lower bound of the area.
    pub lo: f64,
    /// An upper bound of the area.
    pub hi: f64,
    /// Cells evaluated, the starting ones included.
    pub cells: usize,
}

/// A loop's signed area's sign, with the enclosure that decides it (or does not).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoopAreaSign {
    /// The area is above 0 (`lo > 0`): the loop runs counter-clockwise.
    Positive(LoopArea),
    /// The area is below 0 (`hi < 0`): the loop runs clockwise.
    Negative(LoopArea),
    /// The enclosure holds 0 (or is not finite) when the budget is spent or no cell is
    /// left to cut.
    Undecided(LoopArea),
}

/// One cell of a curve piece.
#[derive(Clone, Copy, Debug)]
struct Cell {
    /// The curve.
    curve: usize,
    /// Its knot span.
    span: usize,
    /// The parameter range.
    a: f64,
    b: f64,
    depth: u32,
    /// The enclosure of `int (x - x0) y' dt` over `[a, b]`, signed by the curve's sense.
    enc: Ivl,
}

/// `1 / n!`, enclosed.
fn inverse_factorial(n: usize) -> Ivl {
    let mut f = 1.0_f64;
    for i in 2..=n {
        f *= i as f64; // exact through 22!
    }
    Ivl::from(1.0) / Ivl::from(f)
}

/// `C(n, i)`, exact for the orders used here.
fn binomial(n: usize, i: usize) -> f64 {
    let mut c = 1.0_f64;
    for j in 1..=i {
        c = c * (n - j + 1) as f64 / j as f64;
    }
    c
}

/// `int x^i` over `[xl, xh]`, enclosed.
fn moment(xl: Ivl, xh: Ivl, i: usize) -> Ivl {
    let mut a = Ivl::from(1.0);
    let mut b = Ivl::from(1.0);
    for _ in 0..=i {
        a = a * xh;
        b = b * xl;
    }
    (a - b) / Ivl::from((i + 1) as f64)
}

/// `int |x|^i` over `[xl, xh]` (`xl <= 0 <= xh`), enclosed.
fn abs_moment(xl: Ivl, xh: Ivl, i: usize) -> Ivl {
    let mut a = Ivl::from(1.0);
    let mut b = Ivl::from(1.0);
    let nl = Ivl::from(0.0) - xl;
    for _ in 0..=i {
        a = a * xh;
        b = b * nl;
    }
    (a + b) / Ivl::from((i + 1) as f64)
}

/// The `n`-th derivative of `f = (x - x0) y'` from the curve's derivatives `d` (to order
/// `n + 1`), by Leibniz's rule.
fn f_derivative(d: &[V3<Ivl>], x0: Ivl, n: usize) -> Ivl {
    let x = |i: usize| {
        let xi = d.get(i).map_or(Ivl::whole(), |v| v.x);
        if i == 0 {
            xi - x0
        } else {
            xi
        }
    };
    let y = |i: usize| d.get(i).map_or(Ivl::whole(), |v| v.y);
    let mut s = Ivl::from(0.0);
    for i in 0..=n {
        s = s + Ivl::from(binomial(n, i)) * x(i) * y(n - i + 1);
    }
    s
}

/// The enclosure of `int (x - x0) y' dt` over `[a, b]` on the curve's piece `span`.
fn piece_integral(cn: &NurbsCurve, span: usize, a: f64, b: f64, x0: Ivl) -> Result<Ivl> {
    let m = 0.5 * (a + b);
    let at_m = curve_derivs_over(cn, Ivl::from(m), ORDER + 1, Some(span))?;
    let over = curve_derivs_over(cn, Ivl::new(a, b), ORDER + 1, Some(span))?;
    let (xl, xh) = (Ivl::from(a) - Ivl::from(m), Ivl::from(b) - Ivl::from(m));
    let mut sum = Ivl::from(0.0);
    for j in 0..ORDER {
        sum = sum + f_derivative(&at_m, x0, j) * inverse_factorial(j) * moment(xl, xh, j);
    }
    let top = f_derivative(&over, x0, ORDER);
    let r = Ivl::from(top.mag()) * inverse_factorial(ORDER) * abs_moment(xl, xh, ORDER);
    // a NaN magnitude (an unbounded derivative) makes the remainder the whole line
    let r = if r.hi.is_nan() { Ivl::whole() } else { r };
    Ok(sum + Ivl::new(-r.hi, r.hi))
}

/// The polynomial pieces of a curve: `(span, start, end)` for every non-empty knot span.
fn curve_spans(cn: &NurbsCurve) -> Vec<(usize, f64, f64)> {
    let mut out = Vec::new();
    for k in cn.p..cn.n {
        let (a, b) = (knot(&cn.u, k as isize), knot(&cn.u, k as isize + 1));
        if a < b {
            out.push((k, a, b));
        }
    }
    out
}

/// The curve's start and end points `(x, y)`, enclosed.
fn ends(cn: &NurbsCurve) -> Result<([Ivl; 2], [Ivl; 2])> {
    let spans = curve_spans(cn);
    let (Some(&(k0, a, _)), Some(&(k1, _, b))) = (spans.first(), spans.last()) else {
        return Err(SolidError::invalid_input(OP, "a curve with no knot span"));
    };
    let point = |k: usize, t: f64| -> Result<[Ivl; 2]> {
        let d = curve_derivs_over(cn, Ivl::from(t), 0, Some(k))?;
        let p = d
            .first()
            .ok_or_else(|| SolidError::invalid_input(OP, "a curve point not evaluated"))?;
        Ok([p.x, p.y])
    };
    Ok((point(k0, a)?, point(k1, b)?))
}

/// [`LoopAreaMemo`]'s cap on the words its keys hold (8 bytes each): past it the memo
/// starts again. A guard on memory only: clearing the memo changes no result.
const LOOP_AREA_MEMO_WORDS: usize = 1 << 20;

/// The memo of loop area signs a kernel keeps (not upstream): [`loop_area_sign`]'s result
/// for each loop it certified, by every bit of the loop's input (each curve's degree,
/// knots and homogeneous control points, and its sense). The certificate is a function of
/// that input alone (the kernel is read only for its cancel token), so a hit returns, to
/// the bit, what certifying the loop again returns. A body's check certifies each face's
/// loops twice (both of its pointer checks), and the checks of a body and of the bodies
/// made from it meet the same loops again; each is certified once. Only a certificate that
/// finished is kept (a cancelled one is not). [`Kernel::memoize_curve_caches`] turns it
/// off, with the curve cache memo.
#[derive(Debug, Default)]
pub struct LoopAreaMemo {
    signs: std::collections::HashMap<Vec<u64>, LoopAreaSign>,
    words: usize,
}

impl LoopAreaMemo {
    /// The loops the memo keeps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.signs.len()
    }

    /// Whether it keeps none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.signs.is_empty()
    }
}

/// The memo key of a loop: every bit of its input, each curve's sense, degree and counts
/// first so that no two inputs share a key.
fn memo_key(curves: &[(&NurbsCurve, bool)]) -> Vec<u64> {
    let mut key = Vec::new();
    for &(cn, forwards) in curves {
        key.push(u64::from(forwards));
        key.push(cn.p as u64);
        key.push(cn.n as u64);
        key.push(cn.u.len() as u64);
        key.push(cn.pw.len() as u64);
        key.extend(cn.u.iter().map(|x| x.to_bits()));
        key.extend(cn.pw.iter().map(|x| x.to_bits()));
    }
    key
}

/// The certified signed area of the closed loop of `curves` (each `(curve, forwards)`) and
/// the straight segments joining each curve's end to the next one's start, and its sign
/// (see the module). A loop the kernel certified before is answered from its memo
/// ([`LoopAreaMemo`]), to the bit as certifying it again.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for no curves or a curve with no knot span;
/// [`SolidError::Cancelled`] when the kernel's cancel token fires.
pub fn loop_area_sign(k: &Kernel, curves: &[(&NurbsCurve, bool)]) -> Result<LoopAreaSign> {
    if !k.memoize_curve_caches || curves.is_empty() {
        return certify(k, curves);
    }
    let key = memo_key(curves);
    if let Some(&hit) = k.loop_area_memo.borrow().signs.get(&key) {
        return Ok(hit);
    }
    let sign = certify(k, curves)?;
    let mut memo = k.loop_area_memo.borrow_mut();
    if memo.words + key.len() > LOOP_AREA_MEMO_WORDS {
        memo.signs.clear();
        memo.words = 0;
    }
    memo.words += key.len();
    memo.signs.insert(key, sign);
    Ok(sign)
}

/// [`loop_area_sign`] computed: the certificate itself.
fn certify(k: &Kernel, curves: &[(&NurbsCurve, bool)]) -> Result<LoopAreaSign> {
    if curves.is_empty() {
        return Err(SolidError::invalid_input(OP, "a loop with no curves"));
    }
    // each curve's start and end in the loop's direction
    let mut loop_ends = Vec::new();
    for &(cn, forwards) in curves {
        let (s, e) = ends(cn)?;
        loop_ends.push(if forwards { (s, e) } else { (e, s) });
    }
    let x0 = Ivl::from(loop_ends.first().map_or(f64::NAN, |(s, _)| s[0].mid()));
    if x0.lo.is_nan() {
        return Ok(LoopAreaSign::Undecided(LoopArea {
            lo: f64::NEG_INFINITY,
            hi: f64::INFINITY,
            cells: 0,
        }));
    }

    // the closing segments, from each curve's end to the next one's start
    let mut fixed = Ivl::from(0.0);
    for ((_, p), (q, _)) in loop_ends.iter().zip(loop_ends.iter().cycle().skip(1)) {
        fixed = fixed + ((p[0] - x0) + (q[0] - x0)) * (q[1] - p[1]) * Ivl::from(0.5);
    }

    let mut cells: Vec<Cell> = Vec::new();
    let mut alive: Vec<bool> = Vec::new();
    let mut heap: BinaryHeap<(u64, Reverse<usize>)> = BinaryHeap::new();
    let push = |cells: &mut Vec<Cell>,
                alive: &mut Vec<bool>,
                heap: &mut BinaryHeap<(u64, Reverse<usize>)>,
                curve: usize,
                span: usize,
                a: f64,
                b: f64,
                depth: u32|
     -> Result<()> {
        let (cn, forwards) = curves
            .get(curve)
            .copied()
            .ok_or_else(|| SolidError::invalid_input(OP, "a cell of no curve"))?;
        let mut enc = piece_integral(cn, span, a, b, x0)?;
        if !forwards {
            enc = Ivl::new(-enc.hi, -enc.lo);
        }
        let w = enc.hi - enc.lo;
        // the widest first; an unbounded or undefined one before every bounded one
        let key = if w >= 0.0 { w } else { f64::INFINITY };
        heap.push((key.to_bits(), Reverse(cells.len())));
        cells.push(Cell {
            curve,
            span,
            a,
            b,
            depth,
            enc,
        });
        alive.push(true);
        Ok(())
    };
    for (ci, &(cn, _)) in curves.iter().enumerate() {
        for (span, a, b) in curve_spans(cn) {
            push(&mut cells, &mut alive, &mut heap, ci, span, a, b, 0)?;
        }
    }

    let total = |cells: &[Cell], alive: &[bool]| {
        let mut sum = fixed;
        for (c, &on) in cells.iter().zip(alive) {
            if on {
                sum = sum + c.enc;
            }
        }
        sum
    };
    loop {
        let t = total(&cells, &alive);
        let area = LoopArea {
            lo: t.lo,
            hi: t.hi,
            cells: cells.len(),
        };
        // written so that a NaN end decides nothing
        if t.lo > 0.0 && t.hi.is_finite() {
            return Ok(LoopAreaSign::Positive(area));
        }
        if t.hi < 0.0 && t.lo.is_finite() {
            return Ok(LoopAreaSign::Negative(area));
        }
        if cells.len() >= CELL_BUDGET {
            return Ok(LoopAreaSign::Undecided(area));
        }
        // cut the widest cell that can be cut
        let cut = loop {
            let Some((_, Reverse(i))) = heap.pop() else {
                break None;
            };
            let Some(c) = cells.get(i).copied() else {
                continue;
            };
            let tm = 0.5 * (c.a + c.b);
            if c.depth < MAX_DEPTH && c.a < tm && tm < c.b {
                break Some((i, c, tm));
            }
        };
        let Some((i, c, tm)) = cut else {
            return Ok(LoopAreaSign::Undecided(area));
        };
        if let Some(on) = alive.get_mut(i) {
            *on = false;
        }
        push(
            &mut cells,
            &mut alive,
            &mut heap,
            c.curve,
            c.span,
            c.a,
            tm,
            c.depth + 1,
        )?;
        push(
            &mut cells,
            &mut alive,
            &mut heap,
            c.curve,
            c.span,
            tm,
            c.b,
            c.depth + 1,
        )?;
        if cells.len() % 64 < 2 {
            k.check_cancelled(OP)?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The straight segment from `a` to `z` as a degree 1 curve on `[0, 1]`.
    fn segment(a: [f64; 2], z: [f64; 2]) -> NurbsCurve {
        NurbsCurve {
            p: 1,
            n: 2,
            u: vec![0.0, 0.0, 1.0, 1.0],
            pw: vec![a[0], a[1], 0.0, 1.0, z[0], z[1], 0.0, 1.0],
        }
    }

    /// The quarter circle of radius `r` about `(cx, cy)` from angle 0 to 90 degrees, as a
    /// rational quadratic.
    fn quarter_arc(cx: f64, cy: f64, r: f64) -> NurbsCurve {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        NurbsCurve {
            p: 2,
            n: 3,
            u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            pw: vec![
                cx + r,
                cy,
                0.0,
                1.0,
                (cx + r) * w,
                (cy + r) * w,
                0.0,
                w,
                cx,
                cy + r,
                0.0,
                1.0,
            ],
        }
    }

    fn holds(a: &LoopArea, v: f64) -> bool {
        a.lo <= v && v <= a.hi
    }

    /// A square decides positive counter-clockwise and negative clockwise, its enclosure
    /// holding +-16, the same far from the origin.
    #[test]
    fn a_square_decides_both_ways() {
        let k = Kernel::default();
        for o in [0.0, 1e6] {
            let c = [[o, o], [o + 4.0, o], [o + 4.0, o + 4.0], [o, o + 4.0]];
            let sides: Vec<NurbsCurve> = (0..4).map(|i| segment(c[i], c[(i + 1) % 4])).collect();
            let ccw: Vec<(&NurbsCurve, bool)> = sides.iter().map(|s| (s, true)).collect();
            match loop_area_sign(&k, &ccw).unwrap() {
                LoopAreaSign::Positive(a) => assert!(holds(&a, 16.0), "{a:?}"),
                other => panic!("{other:?}"),
            }
            let cw: Vec<(&NurbsCurve, bool)> = sides.iter().rev().map(|s| (s, false)).collect();
            match loop_area_sign(&k, &cw).unwrap() {
                LoopAreaSign::Negative(a) => assert!(holds(&a, -16.0), "{a:?}"),
                other => panic!("{other:?}"),
            }
        }
    }

    /// The quarter annulus between radii 1e4 and 1e4 + 1e-3 about the origin (area
    /// `pi/4 ((r + t)^2 - r^2)` = 15.708, its box 1e8): positive, its enclosure holding the
    /// closed form; the same loop reversed is negative.
    #[test]
    fn a_far_thin_quarter_annulus_decides_around_its_closed_form() {
        let k = Kernel::default();
        let (r, t) = (1e4, 1e-3);
        let area = std::f64::consts::FRAC_PI_4 * ((r + t) * (r + t) - r * r);
        let outer = quarter_arc(0.0, 0.0, r + t);
        let inner = quarter_arc(0.0, 0.0, r);
        let bottom = segment([r, 0.0], [r + t, 0.0]);
        let left = segment([0.0, r + t], [0.0, r]);
        let ccw = [
            (&bottom, true),
            (&outer, true),
            (&left, true),
            (&inner, false),
        ];
        match loop_area_sign(&k, &ccw).unwrap() {
            LoopAreaSign::Positive(a) => {
                assert!(holds(&a, area), "{a:?} misses {area}");
                assert!(a.cells < 200, "{a:?}");
            }
            other => panic!("{other:?}"),
        }
        let cw = [
            (&inner, true),
            (&left, false),
            (&outer, false),
            (&bottom, false),
        ];
        match loop_area_sign(&k, &cw).unwrap() {
            LoopAreaSign::Negative(a) => assert!(holds(&a, -area), "{a:?} misses {}", -area),
            other => panic!("{other:?}"),
        }
    }

    /// Loops of area 0 never decide: a curve and its way back, and a figure of eight of two
    /// equal squares of opposite sense. Their enclosures hold 0.
    #[test]
    fn a_loop_of_zero_area_stays_undecided() {
        let k = Kernel::default();
        let arc = quarter_arc(1e4, 0.0, 3.0);
        match loop_area_sign(&k, &[(&arc, true), (&arc, false)]).unwrap() {
            LoopAreaSign::Undecided(a) => assert!(holds(&a, 0.0), "{a:?}"),
            other => panic!("{other:?}"),
        }
        let c = [
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [2.0, 2.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ];
        let sides: Vec<NurbsCurve> = (0..8).map(|i| segment(c[i], c[(i + 1) % 8])).collect();
        let eight: Vec<(&NurbsCurve, bool)> = sides.iter().map(|s| (s, true)).collect();
        match loop_area_sign(&k, &eight).unwrap() {
            LoopAreaSign::Undecided(a) => assert!(holds(&a, 0.0), "{a:?}"),
            other => panic!("{other:?}"),
        }
    }

    /// A NaN in a curve decides nothing.
    #[test]
    fn a_nan_point_decides_nothing() {
        let k = Kernel::default();
        let a = segment([0.0, 0.0], [1.0, 0.0]);
        let b = segment([1.0, 0.0], [f64::NAN, 1.0]);
        let c = segment([f64::NAN, 1.0], [0.0, 0.0]);
        let r = loop_area_sign(&k, &[(&a, true), (&b, true), (&c, true)]).unwrap();
        assert!(matches!(r, LoopAreaSign::Undecided(_)), "{r:?}");
    }

    /// The kernel's memo answers a loop it certified before with the certificate itself, to
    /// the bit (the enclosure and its cell count too): a positive, a negative and an
    /// undecided loop, each certified with the memo off, then twice with it on. A loop and
    /// its reverse are two entries; with the memo off nothing is kept.
    #[test]
    fn a_remembered_loop_area_is_the_certificate() {
        let (r, t) = (1e4, 1e-3);
        let outer = quarter_arc(0.0, 0.0, r + t);
        let inner = quarter_arc(0.0, 0.0, r);
        let bottom = segment([r, 0.0], [r + t, 0.0]);
        let left = segment([0.0, r + t], [0.0, r]);
        let arc = quarter_arc(1e4, 0.0, 3.0);
        let loops: [Vec<(&NurbsCurve, bool)>; 3] = [
            vec![
                (&bottom, true),
                (&outer, true),
                (&left, true),
                (&inner, false),
            ],
            vec![
                (&inner, true),
                (&left, false),
                (&outer, false),
                (&bottom, false),
            ],
            vec![(&arc, true), (&arc, false)],
        ];
        let off = Kernel {
            memoize_curve_caches: false,
            ..Kernel::default()
        };
        let on = Kernel {
            memoize_curve_caches: true,
            ..Kernel::default()
        };
        for (i, lp) in loops.iter().enumerate() {
            let fresh = format!("{:?}", loop_area_sign(&off, lp).unwrap());
            let first = format!("{:?}", loop_area_sign(&on, lp).unwrap());
            assert_eq!(on.loop_area_memo.borrow().len(), i + 1);
            let again = format!("{:?}", loop_area_sign(&on, lp).unwrap());
            assert_eq!(
                on.loop_area_memo.borrow().len(),
                i + 1,
                "a hit adds nothing"
            );
            assert_eq!(first, fresh);
            assert_eq!(again, fresh);
        }
        assert!(off.loop_area_memo.borrow().is_empty());
        assert!(matches!(
            loop_area_sign(&on, &loops[2]).unwrap(),
            LoopAreaSign::Undecided(_)
        ));
    }
}
