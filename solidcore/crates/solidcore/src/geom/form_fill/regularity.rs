//! Conservative regularity proof for a positive rational tensor cap.
//!
//! Bernstein convex hulls enclose the reduced rational normal everywhere.
//! Exact input knot restrictions reuse the ray query's outward interval insertion.
//! A work limit or uncertain sign refuses the chart; samples never establish acceptance.
//!
//! One part of the chart is left unproven: a piece that keeps an original chart
//! corner, is at most 2^-12 of its tile along each axis, and whose control hull is
//! bounded inside `CORNER_JOIN_MM` of that corner.
//! Two rim sides that meet tangentially give a corner normal that is zero in exact
//! arithmetic, and any rounded rotation of such a cap turns it into a normal of a
//! few ulps with either sign, so the chart folds inside a few ulps of the corner.
//! That fold is smaller than the distance at which two side ends already count as
//! one corner, so the same length bounds what the proof leaves out.

use crate::eval::ray_spline::{reserved, Axis, Grid, MAX_CONTROLS};
use crate::eval::ray_tensor::{Control, Interval as I};
use crate::geom::expansion;
use crate::{Result, SolidError, Vec3};

const OP: &str = "geom::form_fill::regularity";
const MAX_WORK: usize = 20_000_000;
const MAX_DEPTH: usize = 24;
// A piece that keeps an original chart corner may be halved this often, so that
// it can shrink inside `CORNER_JOIN_MM` of the corner: 48 halvings per axis reach
// the spacing of adjacent doubles in the unit parameter square.
const MAX_CORNER_DEPTH: usize = 96;
// Halvings left to a piece cut off such a corner piece beyond `MAX_DEPTH`.
const BESIDE_CORNER_SPLITS: usize = 6;
// A corner piece is left unproven only once it was halved this often along each
// axis. The ordinary proof has then had its whole depth, and what is left out
// reaches at most 2^-12 of the tile along either edge, so an edge that collapses
// onto its corner is still refused: the rest of that edge has to be proven.
const MIN_CORNER_HALVINGS: usize = MAX_DEPTH / 2;
const CORNER_JOIN_MM: f64 = super::CORNER_JOIN_MM;
// On fully inserted clamped axes nu=p*spans_u+1 and nv=q*spans_v+1.
// Three reduced-normal components hold 3*(3p)*(3q) coefficients per tile,
// so their total is strictly below 27 times the already bounded native grid.
const MAX_RETAINED_NORMAL_COEFFICIENTS: usize = 27 * MAX_CONTROLS;
const MAX_COEFFICIENTS: usize = 10_000;
const MAX_DEGREE: usize = 96;

fn bad(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}
fn spend(work: &mut usize, cost: usize) -> Result<()> {
    *work = work
        .checked_sub(cost)
        .ok_or_else(|| bad("regularity proof work limit"))?;
    Ok(())
}
fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>> {
    if count > MAX_COEFFICIENTS {
        return Err(bad("regularity coefficient limit"));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| bad("regularity allocation"))?;
    output.resize(count, value);
    Ok(output)
}

#[derive(Clone)]
struct Poly {
    degree: [usize; 2],
    values: Vec<I>,
}
impl Poly {
    fn new(degree: [usize; 2], values: Vec<I>) -> Result<Self> {
        if degree.iter().any(|&p| p > MAX_DEGREE)
            || (degree[0] + 1).checked_mul(degree[1] + 1) != Some(values.len())
            || values.len() > MAX_COEFFICIENTS
            || values.iter().any(|x| !x.is_finite())
        {
            return Err(bad("invalid bounded regularity polynomial"));
        }
        Ok(Self { degree, values })
    }
    fn at(&self, u: usize, v: usize) -> Result<I> {
        if u > self.degree[0] || v > self.degree[1] {
            return Err(bad("regularity polynomial index"));
        }
        self.values
            .get(u + (self.degree[0] + 1) * v)
            .copied()
            .ok_or_else(|| bad("regularity polynomial index"))
    }
    fn derivative(&self, axis: usize, work: &mut usize) -> Result<Self> {
        let mut degree = self.degree;
        let n = *degree
            .get(axis)
            .ok_or_else(|| bad("regularity derivative axis"))?;
        if n == 0 {
            return Self::new([0, 0], vec![I::ZERO]);
        }
        *degree
            .get_mut(axis)
            .ok_or_else(|| bad("regularity derivative axis"))? -= 1;
        let mut values = filled((degree[0] + 1) * (degree[1] + 1), I::ZERO)?;
        spend(work, values.len())?;
        for (k, out) in values.iter_mut().enumerate() {
            let (u, v) = (k % (degree[0] + 1), k / (degree[0] + 1));
            let (next_u, next_v) = (u + usize::from(axis == 0), v + usize::from(axis == 1));
            *out = self.at(next_u, next_v)?.sub(self.at(u, v)?).scale(n as f64);
        }
        Self::new(degree, values)
    }
    fn subtract(&self, other: &Self, work: &mut usize) -> Result<Self> {
        if self.degree != other.degree {
            return Err(bad("incompatible regularity polynomial degrees"));
        }
        spend(work, self.values.len())?;
        Self::new(
            self.degree,
            self.values
                .iter()
                .zip(&other.values)
                .map(|(&a, &b)| a.sub(b))
                .collect(),
        )
    }
    fn product(&self, other: &Self, work: &mut usize) -> Result<Self> {
        let degree = [
            self.degree[0] + other.degree[0],
            self.degree[1] + other.degree[1],
        ];
        if degree.iter().any(|&p| p > MAX_DEGREE) {
            return Err(bad("regularity product degree limit"));
        }
        spend(
            work,
            self.values
                .len()
                .checked_mul(other.values.len())
                .ok_or_else(|| bad("regularity product limit"))?,
        )?;
        let weights_u = product_weights(self.degree[0], other.degree[0])?;
        let weights_v = product_weights(self.degree[1], other.degree[1])?;
        let mut values = filled((degree[0] + 1) * (degree[1] + 1), I::ZERO)?;
        for (a, &x) in self.values.iter().enumerate() {
            let (i, j) = (a % (self.degree[0] + 1), a / (self.degree[0] + 1));
            for (b, &y) in other.values.iter().enumerate() {
                let (k, l) = (b % (other.degree[0] + 1), b / (other.degree[0] + 1));
                let wu = *weights_u
                    .get(i * (other.degree[0] + 1) + k)
                    .ok_or_else(|| bad("regularity product weight"))?;
                let wv = *weights_v
                    .get(j * (other.degree[1] + 1) + l)
                    .ok_or_else(|| bad("regularity product weight"))?;
                let out = values
                    .get_mut(i + k + (degree[0] + 1) * (j + l))
                    .ok_or_else(|| bad("regularity product index"))?;
                *out = out.add(x.mul(y).mul(wu).mul(wv));
            }
        }
        Self::new(degree, values)
    }
    fn boundary(&self, axis: usize, high: bool) -> Result<Self> {
        let n = *self
            .degree
            .get(axis)
            .ok_or_else(|| bad("regularity boundary axis"))?;
        let other = usize::from(axis == 0);
        let count = *self
            .degree
            .get(other)
            .ok_or_else(|| bad("regularity boundary axis"))?;
        let mut values = filled(count + 1, I::ZERO)?;
        for (k, out) in values.iter_mut().enumerate() {
            let end = if high { n } else { 0 };
            *out = if axis == 0 {
                self.at(end, k)?
            } else {
                self.at(k, end)?
            };
        }
        Self::new([count, 0], values)
    }
    fn halve(&self, axis: usize, work: &mut usize) -> Result<[Self; 2]> {
        let [p, q] = self.degree;
        let (n, rows) = match axis {
            0 => (p, q + 1),
            1 => (q, p + 1),
            _ => return Err(bad("regularity subdivision axis")),
        };
        spend(work, rows * (n + 1) * (n + 1))?;
        let mut left = filled(self.values.len(), I::ZERO)?;
        let mut right = filled(self.values.len(), I::ZERO)?;
        for row in 0..rows {
            let mut line = filled(n + 1, I::ZERO)?;
            let index = |k| {
                if axis == 0 {
                    k + (p + 1) * row
                } else {
                    row + (p + 1) * k
                }
            };
            for (k, out) in line.iter_mut().enumerate() {
                *out = *self
                    .values
                    .get(index(k))
                    .ok_or_else(|| bad("regularity subdivision index"))?;
            }
            for level in 0..=n {
                *left
                    .get_mut(index(level))
                    .ok_or_else(|| bad("regularity subdivision index"))? = *line
                    .first()
                    .ok_or_else(|| bad("regularity subdivision row"))?;
                *right
                    .get_mut(index(n - level))
                    .ok_or_else(|| bad("regularity subdivision index"))? = *line
                    .get(n - level)
                    .ok_or_else(|| bad("regularity subdivision row"))?;
                for k in 0..n - level {
                    let a = *line
                        .get(k)
                        .ok_or_else(|| bad("regularity subdivision row"))?;
                    let b = *line
                        .get(k + 1)
                        .ok_or_else(|| bad("regularity subdivision row"))?;
                    *line
                        .get_mut(k)
                        .ok_or_else(|| bad("regularity subdivision row"))? =
                        a.scale(0.5).add(b.scale(0.5));
                }
            }
        }
        Ok([
            Self::new(self.degree, left)?,
            Self::new(self.degree, right)?,
        ])
    }
    fn middle(&self, work: &mut usize) -> Result<I> {
        let [half, _] = self.halve(0, work)?;
        let edge = half.boundary(0, true)?;
        let [half, _] = edge.halve(0, work)?;
        half.at(half.degree[0], 0)
    }
}

fn binomials(n: usize) -> Result<Vec<I>> {
    let mut row = filled(n + 1, I::ZERO)?;
    *row.first_mut()
        .ok_or_else(|| bad("regularity binomial row"))? = I::point(1.0);
    for k in 1..=n {
        for i in (1..=k).rev() {
            let a = *row.get(i).ok_or_else(|| bad("regularity binomial index"))?;
            let b = *row
                .get(i - 1)
                .ok_or_else(|| bad("regularity binomial index"))?;
            *row.get_mut(i)
                .ok_or_else(|| bad("regularity binomial index"))? = a.add(b);
        }
    }
    Ok(row)
}
fn product_weights(p: usize, q: usize) -> Result<Vec<I>> {
    let (a, b, c) = (binomials(p)?, binomials(q)?, binomials(p + q)?);
    let mut weights = filled((p + 1) * (q + 1), I::ZERO)?;
    for (i, &a) in a.iter().enumerate() {
        for (j, &b) in b.iter().enumerate() {
            let denominator = *c
                .get(i + j)
                .ok_or_else(|| bad("regularity product denominator"))?;
            *weights
                .get_mut(i * (q + 1) + j)
                .ok_or_else(|| bad("regularity product coefficient"))? = a.mul(b).div(denominator);
        }
    }
    Ok(weights)
}
type Normal = [Poly; 3];
fn cross(a: &Normal, b: &Normal, work: &mut usize) -> Result<Normal> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    Ok([
        ay.product(bz, work)?
            .subtract(&az.product(by, work)?, work)?,
        az.product(bx, work)?
            .subtract(&ax.product(bz, work)?, work)?,
        ax.product(by, work)?
            .subtract(&ay.product(bx, work)?, work)?,
    ])
}
fn scalar_times(w: &Poly, a: &Normal, work: &mut usize) -> Result<Normal> {
    let [x, y, z] = a;
    Ok([
        w.product(x, work)?,
        w.product(y, work)?,
        w.product(z, work)?,
    ])
}
/// The homogeneous coordinates X, Y, Z, W of one tile as four polynomials.
type Position = [Poly; 4];
fn homogeneous(degree: [usize; 2], controls: &[Control]) -> Result<Position> {
    let component = |axis| {
        Poly::new(
            degree,
            controls
                .iter()
                .map(|c| {
                    c.get(axis)
                        .copied()
                        .ok_or_else(|| bad("regularity homogeneous coordinate"))
                })
                .collect::<Result<_>>()?,
        )
    };
    Ok([component(0)?, component(1)?, component(2)?, component(3)?])
}
fn reduced_normal(position: &Position, work: &mut usize) -> Result<Normal> {
    let [a, b, c, w] = position;
    let x = [a.clone(), b.clone(), c.clone()];
    let [a, b, c] = &x;
    let xu = [
        a.derivative(0, work)?,
        b.derivative(0, work)?,
        c.derivative(0, work)?,
    ];
    let xv = [
        a.derivative(1, work)?,
        b.derivative(1, work)?,
        c.derivative(1, work)?,
    ];
    let first = scalar_times(w, &cross(&xu, &xv, work)?, work)?;
    let second = scalar_times(&w.derivative(1, work)?, &cross(&xu, &x, work)?, work)?;
    let third = scalar_times(&w.derivative(0, work)?, &cross(&x, &xv, work)?, work)?;
    let [a, b, c] = first;
    let [d, e, f] = second;
    let [g, h, i] = third;
    Ok([
        a.subtract(&d, work)?.subtract(&g, work)?,
        b.subtract(&e, work)?.subtract(&h, work)?,
        c.subtract(&f, work)?.subtract(&i, work)?,
    ])
}

fn expansion_product(a: &[f64], b: &[f64]) -> Option<Vec<f64>> {
    b.iter().try_fold(vec![0.0], |sum, &x| {
        // product/scale already reject underflowing residuals, subnormal
        // nonzero factors and overflow. Also refuse nonfinite sum intermediates.
        let sum = expansion::add(&sum, &expansion::scale(a, x)?);
        sum.iter().all(|x| x.is_finite()).then_some(sum)
    })
}
fn exact_corner_cross(c: [f64; 4], u: [f64; 4], v: [f64; 4]) -> Option<[Vec<f64>; 3]> {
    let tangent = |next: [f64; 4]| -> Option<[Vec<f64>; 3]> {
        let [x, y, z, w] = c;
        let [nx, ny, nz, nw] = next;
        let tangent = [
            expansion::sub(&expansion::product(w, nx)?, &expansion::product(nw, x)?),
            expansion::sub(&expansion::product(w, ny)?, &expansion::product(nw, y)?),
            expansion::sub(&expansion::product(w, nz)?, &expansion::product(nw, z)?),
        ];
        tangent
            .iter()
            .flatten()
            .all(|x| x.is_finite())
            .then_some(tangent)
    };
    let [a, b, c] = tangent(u)?;
    let [d, e, f] = tangent(v)?;
    let component = |a: &[f64], b: &[f64], c: &[f64], d: &[f64]| {
        let cross = expansion::sub(&expansion_product(a, b)?, &expansion_product(c, d)?);
        cross.iter().all(|x| x.is_finite()).then_some(cross)
    };
    Some([
        component(&b, &f, &c, &e)?,
        component(&c, &d, &a, &f)?,
        component(&a, &e, &b, &d)?,
    ])
}
#[cfg(test)]
fn exact_corner_zero(c: [f64; 4], u: [f64; 4], v: [f64; 4]) -> bool {
    exact_corner_cross(c, u, v)
        .is_some_and(|normal| normal.iter().all(|component| expansion::is_zero(component)))
}
fn corner_normal(
    c: [f64; 4],
    u: [f64; 4],
    v: [f64; 4],
    degree: [usize; 2],
    reverse: bool,
) -> Option<([I; 3], bool)> {
    let cross = exact_corner_cross(c, u, v)?;
    let zero = cross.iter().all(|component| expansion::is_zero(component));
    let factor = degree[0] as f64 * degree[1] as f64 * if reverse { -1.0 } else { 1.0 };
    let bounds = cross.map(|component| {
        component
            .into_iter()
            .fold(I::ZERO, |sum, value| sum.add(I::point(value)))
            .div(I::point(c[3]))
            .scale(factor)
    });
    bounds
        .iter()
        .all(|bound| bound.is_finite())
        .then_some((bounds, zero))
}
fn positive(poly: &Poly, corners: [bool; 4]) -> Result<bool> {
    if poly.values.iter().any(|x| !x.is_finite() || x.lo < 0.0)
        || !poly.values.iter().any(|x| x.lo > 0.0)
    {
        return Ok(false);
    }
    let [p, q] = poly.degree;
    for ((u, v), allowed) in [(0, 0), (p, 0), (0, q), (p, q)].into_iter().zip(corners) {
        let value = poly.at(u, v)?;
        if !(value.lo > 0.0 || allowed && value.lo == 0.0 && value.hi == 0.0) {
            return Ok(false);
        }
    }
    for axis in 0..2 {
        for high in [false, true] {
            if !poly.boundary(axis, high)?.values.iter().any(|x| x.lo > 0.0) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn split_axis(normal: &Normal) -> usize {
    let mut variation = [0.0_f64; 2];
    for poly in normal {
        for (k, value) in poly.values.iter().enumerate() {
            let (u, v) = (k % (poly.degree[0] + 1), k / (poly.degree[0] + 1));
            for (axis, change) in variation.iter_mut().enumerate() {
                let next = if axis == 0 {
                    poly.at(u + 1, v)
                } else {
                    poly.at(u, v + 1)
                };
                if let Ok(next) = next {
                    let a = value.lo * 0.5 + value.hi * 0.5;
                    let b = next.lo * 0.5 + next.hi * 0.5;
                    *change = change.max((a - b).abs());
                }
            }
        }
    }
    usize::from(variation[1] > variation[0])
}
/// The unit direction at equal angles from three vectors, on the side all three face.
/// It is only a candidate: `positive` alone decides whether it separates a tile.
fn equal_angle_direction(vectors: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let unit = |v: [f64; 3]| Some(Vec3::from_array(v).length_and_unit()?.1);
    let [a, b, c] = vectors;
    let (a, b, c) = (unit(a)?, unit(b)?, unit(c)?);
    // With M the matrix of rows a, b, c, the sum of the three cyclic cross
    // products is det(M) times the solution r of M r = (1, 1, 1).
    let sum = a.cross(b) + b.cross(c) + c.cross(a);
    let side = a.triple_product(b, c);
    let direction = if side > 0.0 {
        sum
    } else if side < 0.0 {
        -sum
    } else {
        return None;
    };
    Some(direction.length_and_unit()?.1.to_array())
}
/// Whether the Bernstein hull of `direction . normal` keeps the tile's normal
/// strictly inside one open halfspace, except at permitted exactly zero corners.
fn separated_along(
    normal: &Normal,
    direction: [f64; 3],
    corners: [bool; 4],
    work: &mut usize,
) -> Result<bool> {
    let [x, y, z] = normal;
    spend(work, x.values.len())?;
    let values = x
        .values
        .iter()
        .zip(&y.values)
        .zip(&z.values)
        .map(|((&x, &y), &z)| {
            x.scale(direction[0])
                .add(y.scale(direction[1]))
                .add(z.scale(direction[2]))
        })
        .collect();
    positive(&Poly::new(x.degree, values)?, corners)
}
/// Whether the whole piece is bounded inside `CORNER_JOIN_MM` of one of its
/// corners that is an original chart corner. With positive weights the piece lies
/// in the convex hull of its Cartesian controls, and a ball is convex, so every
/// control inside the ball puts every point of the piece inside it.
fn joins_corner(position: &Position, original: [bool; 4], work: &mut usize) -> Result<bool> {
    let [x, y, z, w] = position;
    if w.values.iter().any(|w| w.lo <= 0.0) {
        return Ok(false);
    }
    let [p, q] = w.degree;
    let limit = CORNER_JOIN_MM * CORNER_JOIN_MM;
    for ((u, v), original) in [(0, 0), (p, 0), (0, q), (p, q)].into_iter().zip(original) {
        if !original {
            continue;
        }
        spend(work, w.values.len())?;
        let weight = w.at(u, v)?;
        let corner = [
            x.at(u, v)?.div(weight),
            y.at(u, v)?.div(weight),
            z.at(u, v)?.div(weight),
        ];
        let inside = x
            .values
            .iter()
            .zip(&y.values)
            .zip(&z.values)
            .zip(&w.values)
            .all(|(((&x, &y), &z), &w)| {
                let square =
                    [x, y, z]
                        .into_iter()
                        .zip(corner)
                        .fold(I::ZERO, |sum, (value, corner)| {
                            let offset = value.div(w).sub(corner);
                            sum.add(offset.mul(offset))
                        });
                square.is_finite() && square.hi <= limit
            });
        if inside {
            return Ok(true);
        }
    }
    Ok(false)
}
/// One part of a tile still to be proven. `zeros` marks its corners where an
/// exactly zero normal is allowed, `original` its corners that are original chart
/// corners, and `position` is kept only while it has such a corner. `halved`
/// counts its halvings along U and V; `depth` is what its depth limit is held to.
struct Piece {
    normal: Normal,
    zeros: [bool; 4],
    original: [bool; 4],
    position: Option<Position>,
    halved: [usize; 2],
    depth: usize,
}
fn prove_normal(
    normal: Normal,
    corners: [bool; 4],
    original: [bool; 4],
    position: Option<Position>,
    work: &mut usize,
) -> Result<bool> {
    let mut stack = vec![Piece {
        normal,
        zeros: corners,
        original,
        position: position.filter(|_| original.contains(&true)),
        halved: [0, 0],
        depth: 0,
    }];
    while let Some(Piece {
        normal,
        zeros: corners,
        original,
        position,
        halved,
        depth,
    }) = stack.pop()
    {
        spend(work, 1)?;
        let [x, y, z] = &normal;
        let center = [x.middle(work)?, y.middle(work)?, z.middle(work)?];
        let reference = center.map(|x| x.lo * 0.5 + x.hi * 0.5);
        if reference.iter().any(|x| !x.is_finite()) || reference.iter().all(|&x| x == 0.0) {
            return Ok(false);
        }
        let mut separated = separated_along(&normal, reference, corners, work)?;
        if !separated {
            // A tiny but nonzero corner normal can point away from the centre
            // normal while an entire coordinate component keeps a strict sign.
            // Each bounded alternative proves the same Bernstein halfspace claim.
            for component in &normal {
                spend(work, component.values.len())?;
                if positive(component, corners)? {
                    separated = true;
                    break;
                }
                spend(work, 2 * component.values.len())?;
                let opposite = Poly::new(
                    component.degree,
                    component
                        .values
                        .iter()
                        .map(|value| value.scale(-1.0))
                        .collect(),
                )?;
                if positive(&opposite, corners)? {
                    separated = true;
                    break;
                }
            }
        }
        if !separated {
            // Beside a corner whose exact normal is nonzero but microscopic, the
            // normal turns from that corner value to the interior direction within
            // a distance no bounded subdivision reaches, and it may oppose the
            // centre normal in all three coordinates. The normal is then
            // n0 + u a + v b to first order, which stays in one halfspace whenever
            // n0, a and b are independent. The direction at equal angles from
            // three of the four corner coefficients approaches that halfspace's
            // axis under subdivision. It is rounded, hence only proposed here: the
            // same outward Bernstein bounds as above accept or refuse it.
            let [p, q] = x.degree;
            let mut ends = [[0.0; 3]; 4];
            for (end, (u, v)) in ends.iter_mut().zip([(0, 0), (p, 0), (0, q), (p, q)]) {
                *end = [
                    x.at(u, v)?.midpoint(),
                    y.at(u, v)?.midpoint(),
                    z.at(u, v)?.midpoint(),
                ];
            }
            let [c00, c10, c01, c11] = ends;
            for triple in [
                [c00, c10, c01],
                [c10, c01, c11],
                [c00, c01, c11],
                [c00, c10, c11],
            ] {
                let Some(direction) = equal_angle_direction(triple) else {
                    continue;
                };
                if separated_along(&normal, direction, corners, work)? {
                    separated = true;
                    break;
                }
            }
        }
        if separated {
            continue;
        }
        // The signs are unresolved. A piece that keeps an original chart corner
        // is left unproven once it is small along both axes and all of it is
        // bounded inside the corner join length; until then it is halved further
        // than any other piece.
        if let Some(position) = &position {
            if halved.iter().all(|&count| count >= MIN_CORNER_HALVINGS)
                && joins_corner(position, original, work)?
            {
                continue;
            }
        }
        let limit = if position.is_some() {
            MAX_CORNER_DEPTH
        } else {
            MAX_DEPTH
        };
        if depth >= limit {
            return Ok(false);
        }
        let [halved_u, halved_v] = halved;
        // Past the ordinary depth a corner piece is halved evenly, so that it
        // shrinks onto its corner and not along one edge.
        let axis = if position.is_some() && depth >= MAX_DEPTH && halved_u != halved_v {
            usize::from(halved_v < halved_u)
        } else {
            split_axis(&normal)
        };
        let halved = if axis == 0 {
            [halved_u + 1, halved_v]
        } else {
            [halved_u, halved_v + 1]
        };
        let [a, b, c] = normal;
        let [a0, a1] = a.halve(axis, work)?;
        let [b0, b1] = b.halve(axis, work)?;
        let [c0, c1] = c.halve(axis, work)?;
        let halves = |[f00, f10, f01, f11]: [bool; 4]| {
            if axis == 0 {
                ([f00, false, f01, false], [false, f10, false, f11])
            } else {
                ([f00, f10, false, false], [false, false, f01, f11])
            }
        };
        let (left, right) = halves(corners);
        let (left_original, right_original) = halves(original);
        let (left_position, right_position) = match &position {
            Some([x, y, z, w]) => {
                let [x0, x1] = x.halve(axis, work)?;
                let [y0, y1] = y.halve(axis, work)?;
                let [z0, z1] = z.halve(axis, work)?;
                let [w0, w1] = w.halve(axis, work)?;
                (
                    Some([x0, y0, z0, w0]).filter(|_| left_original.contains(&true)),
                    Some([x1, y1, z1, w1]).filter(|_| right_original.contains(&true)),
                )
            }
            None => (None, None),
        };
        // A piece cut off a corner piece that was already past the ordinary
        // depth keeps a few halvings of its own.
        let next = |kept: &Option<Position>| {
            if position.is_some() && kept.is_none() {
                (depth + 1).min(MAX_DEPTH - BESIDE_CORNER_SPLITS)
            } else {
                depth + 1
            }
        };
        let (left_depth, right_depth) = (next(&left_position), next(&right_position));
        stack.push(Piece {
            normal: [a1, b1, c1],
            zeros: right,
            original: right_original,
            position: right_position,
            halved,
            depth: right_depth,
        });
        stack.push(Piece {
            normal: [a0, b0, c0],
            zeros: left,
            original: left_original,
            position: left_position,
            halved,
            depth: left_depth,
        });
    }
    Ok(true)
}
fn prove_seam(a: &Normal, b: &Normal, axis: usize, work: &mut usize) -> Result<bool> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    let terms = [
        ax.boundary(axis, true)?
            .product(&bx.boundary(axis, false)?, work)?,
        ay.boundary(axis, true)?
            .product(&by.boundary(axis, false)?, work)?,
        az.boundary(axis, true)?
            .product(&bz.boundary(axis, false)?, work)?,
    ];
    let [x, y, z] = terms;
    let values = x
        .values
        .iter()
        .zip(&y.values)
        .zip(&z.values)
        .map(|((&x, &y), &z)| x.add(y).add(z))
        .collect();
    let mut stack = vec![(Poly::new(x.degree, values)?, 0)];
    while let Some((poly, depth)) = stack.pop() {
        spend(work, 1)?;
        if poly.values.iter().all(|x| x.is_finite() && x.lo > 0.0) {
            continue;
        }
        if depth >= MAX_DEPTH {
            return Ok(false);
        }
        let [a, b] = poly.halve(0, work)?;
        stack.push((a, depth + 1));
        stack.push((b, depth + 1));
    }
    Ok(true)
}

/// Certify local regularity throughout a positive homogeneous tensor cap, including
/// consistent directed normals across every native knot seam. At the four original
/// chart corners alone an exactly zero normal is allowed; edge interiors must be regular.
///
/// One piece at each original chart corner may stay unproven: it is at most
/// 2^-12 of its tile along each axis and is bounded inside `CORNER_JOIN_MM` of the
/// corner. A corner normal of a few ulps with the wrong sign, as a rounded rotation
/// of a tangent corner leaves, folds the chart there. All the rest is proven regular.
///
/// Returns false when the chart is singular or its signs remain unresolved within
/// the fixed budget. Invalid inputs, allocations, and budget exhaustion return errors.
/// Controls vary fastest in U, and knots must be clamped without discontinuities.
pub fn certify(
    degree: [usize; 2],
    knots: [&[f64]; 2],
    size: [usize; 2],
    controls: &[[f64; 4]],
) -> Result<bool> {
    if size[0].checked_mul(size[1]) != Some(controls.len())
        || controls.len() > 16_384
        || controls
            .iter()
            .any(|p| p.iter().any(|x| !x.is_finite()) || p[3] <= 0.0)
    {
        return Err(bad("invalid regularity tensor"));
    }
    let mut work = MAX_WORK;
    let [ku, kv] = knots;
    let mut u = Axis::new(ku, size[0], &mut work).map_err(|_| bad("invalid regularity U space"))?;
    let mut v = Axis::new(kv, size[1], &mut work).map_err(|_| bad("invalid regularity V space"))?;
    if [u.degree(), v.degree()] != degree {
        return Err(bad("regularity tensor degree mismatch"));
    }
    let mut grid = Grid::new(size, controls.iter().map(|p| p.map(I::point)).collect())
        .map_err(|_| bad("invalid regularity control grid"))?;
    let du = u.domain().map_err(|_| bad("regularity U domain"))?;
    let dv = v.domain().map_err(|_| bad("regularity V domain"))?;
    let us = u
        .spans(&mut grid, 0, du, &mut work)
        .map_err(|_| bad("regularity U restriction"))?;
    let vs = v
        .spans(&mut grid, 1, dv, &mut work)
        .map_err(|_| bad("regularity V restriction"))?;
    let tile_count = us
        .len()
        .checked_mul(vs.len())
        .filter(|&n| n <= MAX_CONTROLS)
        .ok_or_else(|| bad("regularity native grid tile limit"))?;
    let coefficients_per_tile = 27usize
        .checked_mul(degree[0])
        .and_then(|n| n.checked_mul(degree[1]))
        .ok_or_else(|| bad("regularity retained coefficient overflow"))?;
    let retained = tile_count
        .checked_mul(coefficients_per_tile)
        .ok_or_else(|| bad("regularity retained coefficient overflow"))?;
    if retained > MAX_RETAINED_NORMAL_COEFFICIENTS {
        return Err(bad("regularity retained coefficient limit"));
    }
    let mut normals = reserved(tile_count).map_err(|_| bad("regularity tile allocation"))?;
    let exact_corner = |right: bool, top: bool| -> Result<Option<([I; 3], bool)>> {
        let [nu, nv] = size;
        let (i, j) = (if right { nu - 1 } else { 0 }, if top { nv - 1 } else { 0 });
        let iu = if right { i - 1 } else { i + 1 };
        let jv = if top { j - 1 } else { j + 1 };
        let c = *controls
            .get(i + nu * j)
            .ok_or_else(|| bad("regularity corner index"))?;
        let a = *controls
            .get(iu + nu * j)
            .ok_or_else(|| bad("regularity corner tangent"))?;
        let b = *controls
            .get(i + nu * jv)
            .ok_or_else(|| bad("regularity corner tangent"))?;
        // Restriction inserts only the existing interior knots. On each original
        // clamped end, the corner and its immediately adjacent controls remain
        // unchanged. For the native unit Bezier tile the reduced normal is
        // p*q*((W*a-Wa*c) cross (W*b-Wb*c))/W; inward end tangents need sign flips.
        Ok(corner_normal(c, a, b, degree, right != top))
    };
    let exact = [
        exact_corner(false, false)?,
        exact_corner(true, false)?,
        exact_corner(false, true)?,
        exact_corner(true, true)?,
    ];
    let zeros = exact.map(|corner| corner.is_some_and(|(_, zero)| zero));
    for (j, v) in vs.iter().enumerate() {
        for (i, u) in us.iter().enumerate() {
            let mut tile = filled((degree[0] + 1) * (degree[1] + 1), [I::ZERO; 4])?;
            grid.copy_span(
                [u.start, v.start],
                [degree[0] + 1, degree[1] + 1],
                &mut tile,
            )
            .map_err(|_| bad("regularity tile extraction"))?;
            if tile.iter().any(|p| p[3].lo <= 0.0) {
                return Ok(false);
            }
            let position = homogeneous(degree, &tile)?;
            let mut normal = reduced_normal(&position, &mut work)?;
            let allowed = [
                i == 0 && j == 0 && zeros[0],
                i + 1 == us.len() && j == 0 && zeros[1],
                i == 0 && j + 1 == vs.len() && zeros[2],
                i + 1 == us.len() && j + 1 == vs.len() && zeros[3],
            ];
            let original = [
                i == 0 && j == 0,
                i + 1 == us.len() && j == 0,
                i == 0 && j + 1 == vs.len(),
                i + 1 == us.len() && j + 1 == vs.len(),
            ];
            for (axis, component) in normal.iter_mut().enumerate() {
                let [p, q] = component.degree;
                for (((u, v), original), exact) in [(0, 0), (p, 0), (0, q), (p, q)]
                    .into_iter()
                    .zip(original)
                    .zip(exact)
                {
                    if let Some((bounds, _)) = exact.filter(|_| original) {
                        let coefficient = component
                            .values
                            .get_mut(u + (p + 1) * v)
                            .ok_or_else(|| bad("regularity exact corner coefficient"))?;
                        let bound = *bounds
                            .get(axis)
                            .ok_or_else(|| bad("regularity corner component"))?;
                        *coefficient = coefficient
                            .intersect(bound)
                            .ok_or_else(|| bad("regularity exact corner bounds disagree"))?;
                    }
                }
            }
            if !prove_normal(normal.clone(), allowed, original, Some(position), &mut work)? {
                return Ok(false);
            }
            normals.push(normal);
        }
    }
    for j in 0..vs.len() {
        for i in 0..us.len() {
            let a = normals
                .get(i + us.len() * j)
                .ok_or_else(|| bad("regularity seam tile"))?;
            if i + 1 < us.len()
                && !prove_seam(
                    a,
                    normals
                        .get(i + 1 + us.len() * j)
                        .ok_or_else(|| bad("regularity seam tile"))?,
                    0,
                    &mut work,
                )?
            {
                return Ok(false);
            }
            if j + 1 < vs.len()
                && !prove_seam(
                    a,
                    normals
                        .get(i + us.len() * (j + 1))
                        .ok_or_else(|| bad("regularity seam tile"))?,
                    1,
                    &mut work,
                )?
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polynomial(degree: [usize; 2], controls: &[[f64; 4]]) -> Result<bool> {
        let [p, q] = degree;
        let u: Vec<_> = std::iter::repeat_n(0.0, p + 1)
            .chain(std::iter::repeat_n(1.0, p + 1))
            .collect();
        let v: Vec<_> = std::iter::repeat_n(0.0, q + 1)
            .chain(std::iter::repeat_n(1.0, q + 1))
            .collect();
        certify(degree, [&u, &v], [p + 1, q + 1], controls)
    }

    #[test]
    fn accepts_a_legal_high_span_planar_cap_with_seven_hundred_eighty_four_cells() {
        // Four source sides each have16 linear pieces. Bottom/right use i/16;
        // reversed top/left use 1-(i/16)^2. Their compatible axes have28 spans.
        let mut breaks = (0..=16).map(|i| f64::from(i) / 16.0).collect::<Vec<_>>();
        breaks.extend((0..=16).map(|i| {
            let t = f64::from(i) / 16.0;
            1.0 - t * t
        }));
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        assert_eq!(breaks.len(), 29);
        assert_eq!((breaks.len() - 1) * (breaks.len() - 1), 784);
        let knots = std::iter::once(0.0)
            .chain(breaks.iter().copied())
            .chain(std::iter::once(1.0))
            .collect::<Vec<_>>();
        let controls = breaks
            .iter()
            .flat_map(|&v| breaks.iter().map(move |&u| [u, v, 0.0, 1.0]))
            .collect::<Vec<_>>();
        assert_eq!(controls.len(), 841);
        assert!(certify([1, 1], [&knots, &knots], [29, 29], &controls).unwrap());
    }

    #[test]
    fn exact_corner_qualification_refuses_underflow_overflow_and_nonfinite_intermediates() {
        let origin = [0.0, 0.0, 0.0, 1.0];
        assert!(exact_corner_zero(
            origin,
            [1.0, 0.0, 0.0, 1.0],
            [2.0, 0.0, 0.0, 1.0]
        ));
        for magnitude in [
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            1e-200,
            1e-160,
            1e200,
            1e300,
        ] {
            // The exact normal is negative and nonzero even if its product
            // underflows to signed zero or overflows in ordinary f64 arithmetic.
            assert!(
                !exact_corner_zero(
                    origin,
                    [magnitude, 0.0, 0.0, 1.0],
                    [0.0, -magnitude, 0.0, 1.0]
                ),
                "magnitude={magnitude}"
            );
        }
        assert!(!exact_corner_zero(
            [f64::MAX, f64::MAX, 0.0, 1.0],
            [-f64::MAX, f64::MAX, 0.0, 1.0],
            [f64::MAX, -f64::MAX, 0.0, 1.0]
        ));
        assert!(!exact_corner_zero(
            [0.0, 0.0, 0.0, 1e200],
            [1e200, 0.0, 0.0, 1e200],
            [0.0, -1e200, 0.0, 1e200]
        ));
    }
    #[test]
    fn refuses_a_narrow_reversal_between_all_twenty_four_normal_samples() {
        let a = 1.0_f64 / 16.0;
        let e = 0.01_f64;
        let k = a * a - e * e;
        let normalization = 1.0 / 3.0 - a + k;
        let x = [
            0.0,
            k / (3.0 * normalization),
            (2.0 * k - a) / (3.0 * normalization),
            1.0,
        ];
        for i in 0..=24 {
            let u = f64::from(i) / 24.0;
            assert!(((u - a).powi(2) - e * e) / normalization > 0.0);
        }
        assert!(
            -e * e / normalization < 0.0,
            "the interior band reverses orientation"
        );
        let controls: Vec<_> = [0.0, 1.0]
            .into_iter()
            .flat_map(|v| x.map(|x| [x, v, 0.0, 1.0]))
            .collect();
        assert!(!matches!(polynomial([3, 1], &controls), Ok(true)));
    }

    #[test]
    fn accepts_positive_rational_quarter_cylinder_without_dropping_weights() {
        let w = 0.5_f64.sqrt();
        let controls: Vec<_> = [0.0, 1.0]
            .into_iter()
            .flat_map(|v| [[1.0, v, 0.0, 1.0], [w, v * w, w, w], [0.0, v, 1.0, 1.0]])
            .collect();
        assert!(polynomial([2, 1], &controls).unwrap());
    }

    #[test]
    fn accepts_exact_isolated_planar_and_nonplanar_rounded_corners() {
        // S=(u+v, uv+v², z uv). Its directed Z normal is u+v, strictly
        // positive except at the original corner (0,0), including both edge interiors.
        for nonplanar in [false, true] {
            let controls: Vec<_> = (0..3)
                .flat_map(|j| {
                    (0..2).map(move |i| {
                        let (u, v) = (f64::from(i), f64::from(j) / 2.0);
                        [
                            u + v,
                            u * v + if j == 2 { 1.0 } else { 0.0 },
                            if nonplanar { u * v } else { 0.0 },
                            1.0,
                        ]
                    })
                })
                .collect();
            assert!(
                polynomial([1, 2], &controls).unwrap(),
                "nonplanar={nonplanar}"
            );
        }
    }

    /// S=(u+v, uv+v², 0) of the test above, with the control beside the corner
    /// (0,0) along V lifted in Y, then turned about Z and moved by `shift`.
    /// Neither turned nor moved, its directed corner normal is exactly 2*lift.
    fn tangent_corner(lift: f64, turn: f64, shift: [f64; 2]) -> Vec<[f64; 4]> {
        let (sin, cos) = turn.sin_cos();
        (0..3)
            .flat_map(|j| {
                (0..2).map(move |i| {
                    let (u, v) = (f64::from(i), f64::from(j) / 2.0);
                    let x = u + v;
                    let y = u * v
                        + if j == 2 { 1.0 } else { 0.0 }
                        + if (i, j) == (0, 1) { lift } else { 0.0 };
                    [
                        x * cos - y * sin + shift[0],
                        x * sin + y * cos + shift[1],
                        0.0,
                        1.0,
                    ]
                })
            })
            .collect()
    }

    #[test]
    fn accepts_a_tangent_corner_whose_reversed_normal_folds_inside_the_corner_join() {
        // The corner normal is -2^-39 against an interior normal of u+v, so the
        // chart folds where u+v is below about 2^-39: 2e-12 mm from the corner,
        // far inside the 1e-9 mm at which two side ends are one corner.
        let controls = tangent_corner(-(2.0_f64.powi(-40)), 0.0, [0.0, 0.0]);
        let ([_, _, z], zero) =
            corner_normal(controls[0], controls[1], controls[2], [1, 2], false).unwrap();
        assert!(!zero && z.hi < 0.0, "the corner normal is reversed: {z:?}");
        assert!(polynomial([1, 2], &controls).unwrap());
    }

    #[test]
    fn refuses_a_reversed_corner_whose_fold_reaches_past_the_corner_join() {
        // The same corner with a normal of -2^-19 folds the chart out to about
        // 2e-6 mm from the corner, two thousand times the corner join length.
        // The piece inside the join length is left out; the fold beside it is not.
        let controls = tangent_corner(-(2.0_f64.powi(-20)), 0.0, [0.0, 0.0]);
        assert!(!matches!(polynomial([1, 2], &controls), Ok(true)));
    }

    #[test]
    fn accepts_a_tangent_corner_cap_turned_by_any_whole_degree() {
        // Turning about a point away from the corner rounds its three collinear
        // controls, which leaves a corner normal of a few ulps with either sign.
        // Every turned copy is the same cap, so every one is accepted.
        let mut reversed = 0;
        for degrees in 0..360 {
            let controls = tangent_corner(0.0, f64::from(degrees).to_radians(), [37.25, -11.5]);
            let ([_, _, z], _) =
                corner_normal(controls[0], controls[1], controls[2], [1, 2], false).unwrap();
            reversed += usize::from(z.hi < 0.0);
            assert!(
                polynomial([1, 2], &controls).unwrap(),
                "turned by {degrees} degrees, corner normal {z:?}"
            );
        }
        assert!(reversed > 0, "no turn reversed the corner normal");
    }

    #[test]
    fn refuses_an_entire_collapsed_edge_despite_exact_zero_corners() {
        let controls = [
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 0.0, 1.0],
        ];
        assert!(!matches!(polynomial([1, 1], &controls), Ok(true)));
    }

    #[test]
    fn refuses_individually_regular_pieces_with_reversing_c0_seam_normals() {
        let controls: Vec<_> = [0.0, 1.0]
            .into_iter()
            .flat_map(|v| [0.0, 1.0, 0.0].map(|u| [u, v, 0.0, 1.0]))
            .collect();
        assert!(!matches!(
            certify(
                [1, 1],
                [&[0.0, 0.0, 0.5, 1.0, 1.0], &[0.0, 0.0, 1.0, 1.0]],
                [3, 2],
                &controls
            ),
            Ok(true)
        ));
    }

    #[test]
    fn hostile_tensor_sizes_degrees_knots_and_weights_refuse() {
        let controls = [[0.0, 0.0, 0.0, 1.0]; 4];
        let knots = [0.0, 0.0, 1.0, 1.0];
        assert!(certify([usize::MAX, 1], [&knots, &knots], [2, 2], &controls).is_err());
        assert!(certify([1, 1], [&knots, &knots], [usize::MAX, 2], &controls).is_err());
        assert!(certify(
            [1, 1],
            [&[0.0, 0.0, f64::NAN, 1.0], &knots],
            [2, 2],
            &controls
        )
        .is_err());
        let mut negative = controls;
        negative[0][3] = -1.0;
        assert!(polynomial([1, 1], &negative).is_err());
    }

    #[test]
    fn exact_nonzero_rounded_corner_bounds_keep_the_microscopic_normal_rotation() {
        // The first native tile of the retained sculpted rounded-rim cap.
        // Its corner is not singular: X/Y normal components are positive while
        // Z is negative. A centre-only +Z separator cannot prove this whole tile.
        let controls = [
            [
                -14.666666666666666,
                -14.666666666666666,
                0.03333333333333335,
                1.0,
            ],
            [-14.333333333333334, -15.0, 0.03333333333333334, 1.0],
            [-13.916666666666668, -15.25, 0.03229166666666668, 1.0],
            [
                -13.458333333333334,
                -15.416666666666666,
                0.03059895833333334,
                1.0,
            ],
            [-15.0, -14.333333333333334, 0.03333333333333334, 1.0],
            [
                -14.611111111111114,
                -14.611111111111116,
                0.03287037037037038,
                1.0,
            ],
            [
                -14.13888888888889,
                -14.819444444444443,
                0.031495949074074076,
                1.0,
            ],
            [
                -13.625000000000004,
                -14.958333333333334,
                0.029557291666666666,
                1.0,
            ],
            [-15.25, -13.916666666666668, 0.03229166666666668, 1.0],
            [
                -14.819444444444443,
                -14.138888888888891,
                0.031495949074074076,
                1.0,
            ],
            [
                -14.305555555555557,
                -14.305555555555559,
                0.029918981481481484,
                1.0,
            ],
            [-13.75, -14.416666666666664, 0.02786458333333334, 1.0],
            [
                -15.416666666666666,
                -13.458333333333334,
                0.03059895833333334,
                1.0,
            ],
            [
                -14.958333333333336,
                -13.625000000000004,
                0.029557291666666666,
                1.0,
            ],
            [
                -14.416666666666664,
                -13.750000000000002,
                0.02786458333333333,
                1.0,
            ],
            [
                -13.833333333333334,
                -13.833333333333334,
                0.025781250000000006,
                1.0,
            ],
        ];
        let (normal, zero) =
            corner_normal(controls[0], controls[1], controls[4], [3, 3], false).unwrap();
        assert!(!zero);
        assert!(normal[0].lo > 0.0 && normal[1].lo > 0.0 && normal[2].hi < 0.0);
        assert!(normal[0].hi < 1e-15 && normal[2].lo > -1e-12);
        assert!(polynomial([3, 3], &controls).unwrap());
    }

    #[test]
    fn accepts_a_regular_corner_whose_microscopic_normal_opposes_the_centre_on_every_axis() {
        // Before the linear map, S=(u+v, uv+v²-ev, u+v+ev) with e=2^-40 has the
        // normal (e-u-v+ev, -e, u+v-e). Its Y component never vanishes, so S is
        // regular, while X and Z change sign within e of the corner (0,0).
        // M has orthogonal integer rows of length 7: it is 7 times a rotation,
        // exact in f64, and maps the normal n to 7 M n. Its rows applied to that
        // normal give 4s-7e+2ev, -s+7e+3ev and -9s+7e+6ev with s=u+v, so every
        // coordinate of the mapped normal changes sign, and the corner normal
        // 49e(-1,1,1) opposes the centre normal. Halving cannot reach e within
        // the depth limit; only a direction fitted to the corner separates it.
        let e = 2.0_f64.powi(-40);
        let m = [[2.0, 3.0, 6.0], [3.0, -6.0, 2.0], [6.0, 2.0, -3.0]];
        let controls: Vec<_> = (0..3)
            .flat_map(|j| {
                (0..2).map(move |i| {
                    let (u, v) = (f64::from(i), f64::from(j) / 2.0);
                    let s = [
                        u + v,
                        u * v + if j == 2 { 1.0 } else { 0.0 } - e * v,
                        u + v + e * v,
                    ];
                    let [x, y, z] = m.map(|row| row[0] * s[0] + row[1] * s[1] + row[2] * s[2]);
                    [x, y, z, 1.0]
                })
            })
            .collect();
        let (normal, zero) =
            corner_normal(controls[0], controls[1], controls[2], [1, 2], false).unwrap();
        assert!(!zero);
        let expected = [-49.0 * e, 49.0 * e, 49.0 * e];
        for (bound, expected) in normal.iter().zip(expected) {
            assert!(bound.lo <= expected && expected <= bound.hi);
            assert!(bound.hi - bound.lo <= 1e-24);
        }
        assert!(polynomial([1, 2], &controls).unwrap());
    }

    #[test]
    fn equal_angle_direction_faces_three_vectors_and_refuses_dependent_ones() {
        // (1,1,1)/sqrt(3) makes the same angle with each coordinate axis.
        let axes = [[2.0, 0.0, 0.0], [0.0, 1e-20, 0.0], [0.0, 0.0, 1e20]];
        let direction = equal_angle_direction(axes).unwrap();
        for value in direction {
            assert!((value - 1.0 / 3.0_f64.sqrt()).abs() < 1e-15);
        }
        // Reordering changes the orientation of the triple, not the side faced.
        let swapped = equal_angle_direction([axes[1], axes[0], axes[2]]).unwrap();
        for (a, b) in direction.iter().zip(swapped) {
            assert!((a - b).abs() < 1e-15);
        }
        assert!(equal_angle_direction([axes[0], axes[1], [1.0, 1.0, 0.0]]).is_none());
        assert!(equal_angle_direction([axes[0], axes[1], [0.0; 3]]).is_none());
        assert!(equal_angle_direction([axes[0], axes[1], [f64::NAN, 0.0, 1.0]]).is_none());
    }

    #[test]
    fn exact_original_end_normals_survive_simple_knot_restriction_with_both_end_signs() {
        let knots = vec![0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0];
        let stations = [0.0, 0.25, 0.75, 1.0];
        let controls = stations
            .iter()
            .flat_map(|&v| stations.iter().map(move |&u| [u, v, 0.0, 1.0]))
            .collect::<Vec<_>>();
        for right in [false, true] {
            for top in [false, true] {
                let i = if right { 3 } else { 0 };
                let j = if top { 3 } else { 0 };
                let iu = if right { 2 } else { 1 };
                let jv = if top { 2 } else { 1 };
                let (normal, zero) = corner_normal(
                    controls[i + 4 * j],
                    controls[iu + 4 * j],
                    controls[i + 4 * jv],
                    [2, 2],
                    right != top,
                )
                .unwrap();
                assert!(!zero);
                assert_eq!(normal[0].pair(), [0.0; 2]);
                assert_eq!(normal[1].pair(), [0.0; 2]);
                assert!(normal[2].lo > 0.0 && normal[2].lo <= 0.25 && normal[2].hi >= 0.25);
            }
        }
        assert!(certify([2, 2], [&knots, &knots], [4, 4], &controls).unwrap());
    }
}
