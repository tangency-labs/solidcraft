//! Exact analytic cell certificate. Topology supplies incidence, never positions.
//!
//! Upstream: none. Cramer's rule in homogeneous expansion coordinates avoids
//! division and rounding. This first scope admits simple, strictly convex facets;
//! split edges, coplanar facets and non-simple vertices are left unclassified.

use std::collections::BTreeMap;

use super::{missing, Halfspace, OP};
use crate::error::Result;
use crate::geom::expansion as ex;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::topology::{loopuse, Brep, LoopId, VertexId};

type E = Vec<f64>;
type V = [E; 3];

fn checked(mut e: E) -> Option<E> {
    if !e.iter().all(|c| c.is_finite()) {
        return None;
    }
    e.retain(|&c| c != 0.0);
    Some(e)
}
fn add(a: &[f64], b: &[f64]) -> Option<E> {
    checked(ex::add(a, b))
}
fn sub(a: &[f64], b: &[f64]) -> Option<E> {
    checked(ex::sub(a, b))
}
fn mul(a: &[f64], b: &[f64]) -> Option<E> {
    b.iter()
        .try_fold(E::new(), |s, &c| add(&s, &ex::scale(a, c)?))
}
fn dot([x, y, z]: &V, n: Vec3) -> Option<E> {
    add(
        &add(&ex::scale(x, n.x)?, &ex::scale(y, n.y)?)?,
        &ex::scale(z, n.z)?,
    )
}
fn constant(p: Halfspace) -> Option<E> {
    checked(ex::dot(
        p.outward,
        Vec3::new(p.origin.x, p.origin.y, p.origin.z),
    )?)
}

struct Point {
    numerator: V,
    denominator: E,
}
impl Point {
    fn intersection([a, b, c]: [Halfspace; 3]) -> Option<Self> {
        let mut denominator = checked(ex::det3(a.outward, b.outward, c.outward)?)?;
        let sign = ex::sign(&denominator);
        if sign == 0 {
            return None;
        }
        let ca = checked(constant(a)?)?;
        let cb = checked(constant(b)?)?;
        let cc = checked(constant(c)?)?;
        let ab = ex::cross(a.outward, b.outward)?;
        let bc = ex::cross(b.outward, c.outward)?;
        let ca_cross = ex::cross(c.outward, a.outward)?;
        let mut coordinates = Vec::new();
        for ((x, y), z) in bc.iter().zip(&ca_cross).zip(&ab) {
            let n = add(&add(&mul(x, &ca)?, &mul(y, &cb)?)?, &mul(z, &cc)?)?;
            coordinates.push(checked(ex::scale(&n, f64::from(sign))?)?);
        }
        let numerator: V = coordinates.try_into().ok()?;
        denominator = checked(ex::scale(&denominator, f64::from(sign))?)?;
        Some(Self {
            numerator,
            denominator,
        })
    }
    fn side(&self, plane: Halfspace) -> Option<i32> {
        Some(ex::sign(&sub(
            &dot(&self.numerator, plane.outward)?,
            &mul(&self.denominator, &constant(plane)?)?,
        )?))
    }
    fn delta(&self, other: &Self) -> Option<V> {
        let mut out = Vec::new();
        for (a, b) in self.numerator.iter().zip(&other.numerator) {
            out.push(sub(
                &mul(b, &self.denominator)?,
                &mul(a, &other.denominator)?,
            )?);
        }
        out.try_into().ok()
    }
}

/// The same supporting plane, bit for bit: an edge's two incident planes are its facets'.
fn same(a: &Halfspace, b: &Halfspace) -> bool {
    a.outward == b.outward && a.origin == b.origin
}

pub(super) fn normal_defines_plane(normal: Vec3, x: Vec3, y: Vec3) -> bool {
    let proof = || {
        Some(
            ex::is_zero(&checked(ex::dot(normal, x)?)?)
                && ex::is_zero(&checked(ex::dot(normal, y)?)?)
                && ex::sign(&checked(ex::det3(x, y, normal)?)?) > 0,
        )
    };
    proof() == Some(true)
}

/// Certify a closed convex polyhedron from a simple source incidence complex.
/// All analytic vertices are intersections of exactly three incident planes.
/// Each facet's edges must strictly support all its other vertices; this also
/// rules out self-intersecting cycles, which same-sign consecutive turns do not.
/// `None` when the certificate is not proved; otherwise the cell's exact vertices.
pub(super) fn convex(
    kernel: &Kernel,
    b: &Brep,
    attachment: LoopId,
    base: Halfspace,
    mut facets: Vec<(LoopId, Halfspace)>,
) -> Result<Option<Cell>> {
    let cap = Halfspace {
        origin: base.origin,
        outward: -base.outward,
    };
    facets.push((attachment, cap));
    let mut cycles = Vec::new();
    let mut incident = BTreeMap::<VertexId, Vec<Halfspace>>::new();
    let mut edges = BTreeMap::new();
    for (l, plane) in facets {
        kernel.check_cancelled(OP)?;
        let cycle = loopuse::loop_vertices(b, l)?
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| missing("cell loop without vertex"))?;
        if cycle.len() < 3 {
            return Ok(None);
        }
        for &v in &cycle {
            incident.entry(v).or_default().push(plane);
        }
        for (&a, &c) in cycle.iter().zip(cycle.iter().cycle().skip(1)) {
            let key = if a < c { (a, c) } else { (c, a) };
            edges.entry(key).or_insert_with(Vec::new).push(plane);
        }
        cycles.push((cycle, plane));
    }
    let mut points = BTreeMap::new();
    let mut raised = false;
    for (v, planes) in incident {
        kernel.check_cancelled(OP)?;
        let Ok(planes) = <Vec<Halfspace> as TryInto<[Halfspace; 3]>>::try_into(planes) else {
            return Ok(None);
        };
        let Some(p) = Point::intersection(planes) else {
            return Ok(None);
        };
        for &(_, plane) in &cycles {
            if !matches!(p.side(plane), Some(-1 | 0)) {
                return Ok(None);
            }
        }
        raised |= p.side(cap) == Some(-1);
        points.insert(v, p);
    }
    if !raised {
        return Ok(None);
    }
    for (i, a) in points.values().enumerate() {
        kernel.check_cancelled(OP)?;
        for c in points.values().skip(i + 1) {
            let Some(delta) = a.delta(c) else {
                return Ok(None);
            };
            if delta.iter().all(|e| ex::is_zero(e)) {
                return Ok(None);
            }
        }
    }
    for planes in edges.values() {
        let [a, c] = planes.as_slice() else {
            return Ok(None);
        };
        let Some(v) = ex::cross(a.outward, c.outward) else {
            return Ok(None);
        };
        if !v.iter().all(|e| e.iter().all(|c| c.is_finite())) || v.iter().all(|e| ex::is_zero(e)) {
            return Ok(None);
        }
    }
    // Strict support of each facet edge, decided on planes, not on point differences: the
    // edge lies on its facet's plane and on one other plane, which must have every other
    // vertex of the facet strictly inside. All other vertices strictly on one side of each
    // edge line makes the cycle a simple convex polygon. This is the side test above
    // (degree four in the coefficients), where a turn test on vertex differences is of
    // degree thirteen and leaves the exact products' range for a branch turned by a
    // rotation whose cosines round (6e-17 for a quarter turn, #22784).
    for (cycle, plane) in &cycles {
        for (&a, &c) in cycle.iter().zip(cycle.iter().cycle().skip(1)) {
            kernel.check_cancelled(OP)?;
            let key = if a < c { (a, c) } else { (c, a) };
            let others = edges.get(&key).ok_or_else(|| missing("cell edge absent"))?;
            let mut beside = others.iter().filter(|e| !same(e, plane));
            let (Some(other), None) = (beside.next(), beside.next()) else {
                return Ok(None);
            };
            for v in cycle.iter().filter(|&&v| v != a && v != c) {
                let pv = points.get(v).ok_or_else(|| missing("cell vertex absent"))?;
                if pv.side(*other) != Some(-1) {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(Cell {
        points: points.into_values().collect(),
    }))
}

/// A certified convex source cell: the convex hull of its exact vertices.
pub(super) struct Cell {
    points: Vec<Point>,
}

/// An offset sheet's plane as it is defined: through `origin`, spanned by its axes
/// `x` and `y`. Its exact normal is `x × y` in expansion arithmetic (not the rounded
/// unit normal), so the halfspace below is the plane's own, whatever its tilt.
pub(super) struct Sheet {
    pub(super) origin: Point3,
    pub(super) x: Vec3,
    pub(super) y: Vec3,
    /// The source face's outward normal: it fixes which side is material.
    pub(super) outward: Vec3,
}

impl Sheet {
    /// `(N, s)`: the exact normal `x × y` and the sign `s` that points it along
    /// `outward`. `None` when that sign is not proved (an exact zero).
    fn normal(&self) -> Option<(V, f64)> {
        let n = ex::cross(self.x, self.y)?;
        let n: V = [
            checked(n[0].clone())?,
            checked(n[1].clone())?,
            checked(n[2].clone())?,
        ];
        match ex::sign(&dot(&n, self.outward)?) {
            1 => Some((n, 1.0)),
            -1 => Some((n, -1.0)),
            _ => None,
        }
    }

    /// The exact sign-scaled value `s N·(p - origin)` at a homogeneous point, times its
    /// positive denominator: positive strictly outside the material halfspace.
    fn excess(&self, (n, s): &(V, f64), p: &Point) -> Option<E> {
        let mut at = E::new();
        for (c, nc) in p.numerator.iter().zip(n) {
            at = add(&at, &mul(c, nc)?)?;
        }
        let mut offset = E::new();
        for (o, nc) in [self.origin.x, self.origin.y, self.origin.z]
            .into_iter()
            .zip(n)
        {
            offset = add(&offset, &ex::scale(nc, o)?)?;
        }
        checked(ex::scale(&sub(&at, &mul(&p.denominator, &offset)?)?, *s)?)
    }
}

impl Cell {
    /// Whether the material halfspaces of sheets `a` and `b` have no common interior
    /// point in this cell: a Farkas certificate with unit multipliers. Each material
    /// halfspace is `f <= 0` for the affine `f` of [`Sheet::excess`]; if `f_a + f_b >= 0`
    /// at every vertex, it is non-negative on the cell, their convex hull, so no point
    /// of the cell has both `f_a < 0` and `f_b < 0`: the two halfspaces' common part in
    /// the cell has no interior, and the regularized cavity there is empty. Where the sum
    /// is zero the two sheets coincide (a pad inset by exactly half its width, #22783):
    /// what lies in both is a part of that one plane, of zero volume, which the
    /// regularized offset does not keep. For exactly opposite normals `f_a + f_b` is the
    /// constant crossing of the two planes; for normals opposite only up to rounding (a
    /// turned branch) the plane's own tilt is decided exactly instead of being compared
    /// bit for bit.
    pub(super) fn separates(&self, a: &Sheet, b: &Sheet) -> bool {
        let proof = || {
            let na = a.normal()?;
            let nb = b.normal()?;
            if self.points.is_empty() {
                return Some(false);
            }
            for p in &self.points {
                let sum = add(&a.excess(&na, p)?, &b.excess(&nb, p)?)?;
                if ex::sign(&sum) < 0 {
                    return Some(false);
                }
            }
            Some(true)
        };
        proof() == Some(true)
    }
}
