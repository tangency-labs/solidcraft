//! Small dense linear solves, on the stack: every Newton step and velocity
//! here is a system of at most 4 rows and 4 columns. Heap vectors for these
//! cost more than the arithmetic, at a dozen per marching step.

pub(crate) const MAX: usize = 4;

/// J x = r with m rows and n columns, m and n at most 4.
#[derive(Clone, Copy, Debug)]
pub(crate) struct System {
    pub m: usize,
    pub n: usize,
    pub j: [[f64; MAX]; MAX],
    pub r: [f64; MAX],
}

impl System {
    /// m rows of n columns, every entry zero.
    pub fn new(m: usize, n: usize) -> System {
        System {
            m,
            n,
            j: [[0.0; MAX]; MAX],
            r: [0.0; MAX],
        }
    }

    /// The 3 × 2 system [a b] x = r: a vector's coordinates along two directions.
    pub fn columns2(a: [f64; 3], b: [f64; 3], r: [f64; 3]) -> System {
        let mut s = System::new(3, 2);
        for ((row, rk), ((&ak, &bk), &rv)) in
            s.j.iter_mut().zip(&mut s.r).zip(a.iter().zip(&b).zip(&r))
        {
            *row = [ak, bk, 0.0, 0.0];
            *rk = rv;
        }
        s
    }
}

/// Entry (r, c) of a stack matrix; NaN outside it, which fails every pivot test.
fn entry(a: &[[f64; MAX]; MAX], r: usize, c: usize) -> f64 {
    a.get(r)
        .and_then(|row| row.get(c))
        .copied()
        .unwrap_or(f64::NAN)
}

/// Solve the square n × n system A x = b by Gaussian elimination with partial
/// pivoting; None when singular or not finite.
fn solve_square(n: usize, mut a: [[f64; MAX]; MAX], mut b: [f64; MAX]) -> Option<[f64; MAX]> {
    if n > MAX {
        return None;
    }
    for col in 0..n {
        let mut pivot = col;
        for r in col + 1..n {
            if entry(&a, r, col).abs() > entry(&a, pivot, col).abs() {
                pivot = r;
            }
        }
        if !(entry(&a, pivot, col).abs() > 1e-300) {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let (head, tail) = a.split_at_mut(col + 1);
        let (b_head, b_tail) = b.split_at_mut(col + 1);
        let (pivot_row, pivot_b) = (head.last()?, *b_head.last()?);
        let pivot_value = *pivot_row.get(col)?;
        for (row, br) in tail.iter_mut().zip(b_tail).take(n - col - 1) {
            let f = *row.get(col)? / pivot_value;
            for (x, &y) in row.iter_mut().zip(pivot_row).take(n).skip(col) {
                *x -= f * y;
            }
            *br -= f * pivot_b;
        }
    }
    let mut x = [0.0; MAX];
    for r in (0..n).rev() {
        let row = a.get(r)?;
        let mut sum = *b.get(r)?;
        for (ark, xk) in row.iter().zip(&x).take(n).skip(r + 1) {
            sum -= ark * xk;
        }
        *x.get_mut(r)? = sum / row.get(r)?;
    }
    x.get(..n)?.iter().all(|v| v.is_finite()).then_some(x)
}

/// `(JᵀJ + damping I) x = Jᵀr`.
pub(crate) fn solve_regularized(s: &System, damping: f64) -> Option<[f64; MAX]> {
    let (rows, rhs) = (s.j.get(..s.m)?, s.r.get(..s.m)?);
    let column = |row: &[f64; MAX], p: usize| row.get(p).copied().unwrap_or(f64::NAN);
    let mut a = [[0.0; MAX]; MAX];
    let mut b = [0.0; MAX];
    for (p, (a_row, bp)) in a.iter_mut().zip(&mut b).enumerate().take(s.n) {
        for (q, apq) in a_row.iter_mut().enumerate().take(s.n) {
            *apq = rows.iter().map(|row| column(row, p) * column(row, q)).sum();
        }
        *a_row.get_mut(p)? += damping;
        *bp = rows
            .iter()
            .zip(rhs)
            .map(|(row, rk)| column(row, p) * rk)
            .sum();
    }
    solve_square(s.n, a, b)
}

/// The damped least-squares step for J x = r, damped by |r|^2 (Levenberg and
/// Marquardt with Yamashita and Fukushima's choice of damping): the Newton step
/// where J has full rank, and a step onto the nearest solution where it does
/// not. An edge lying along the other surface makes the curve/surface Jacobian
/// rank 2, and a plain solve then throws its root away. The damping vanishes
/// with the residual, so a grazing double root still converges at Newton's
/// rate; a fixed damping of 1e-12 of the normal matrix stalled there 2e-7 to
/// 1e-6 short of the touch (ssi-corpus `c5_hemisphere_equator`, where a plane
/// piece's edge grazes the circle), and each stalled root traced the arc again.
pub(crate) fn solve_damped(s: &System) -> Option<[f64; MAX]> {
    solve_regularized(s, s.r.iter().take(s.m).map(|rk| rk * rk).sum())
}

/// The Newton step for J x = r: solved directly where J is square (least
/// squares where it is tall), and damped only where that fails or would throw
/// the parameters out of range (`fits` false). The direct solve comes first
/// because the normal equations square J's conditioning: at a crossing of
/// 2e-6 rad, a loop of radius 1e-6 (ssi-corpus c7_tiny_loop_parab_1e-6), that
/// lost the corrector its convergence. The damped step is for a singular J: an
/// edge lying along the other surface, or the free parameter at a pole.
pub(crate) fn newton_step(s: &System, fits: impl Fn(&[f64]) -> bool) -> Option<[f64; MAX]> {
    let direct = if s.m == s.n {
        solve_square(s.n, s.j, s.r)
    } else {
        solve_regularized(s, 0.0)
    };
    match direct {
        Some(step) if step.get(..s.n).is_some_and(&fits) => Some(step),
        _ => solve_damped(s),
    }
}
