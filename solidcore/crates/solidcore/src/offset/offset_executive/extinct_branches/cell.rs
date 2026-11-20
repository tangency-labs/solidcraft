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
use crate::math::Vec3;
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
fn cross([ax, ay, az]: &V, [bx, by, bz]: &V) -> Option<V> {
    Some([
        sub(&mul(ay, bz)?, &mul(az, by)?)?,
        sub(&mul(az, bx)?, &mul(ax, bz)?)?,
        sub(&mul(ax, by)?, &mul(ay, bx)?)?,
    ])
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
pub(super) fn convex(
    kernel: &Kernel,
    b: &Brep,
    attachment: LoopId,
    base: Halfspace,
    mut facets: Vec<(LoopId, Halfspace)>,
) -> Result<bool> {
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
            return Ok(false);
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
            return Ok(false);
        };
        let Some(p) = Point::intersection(planes) else {
            return Ok(false);
        };
        for &(_, plane) in &cycles {
            if !matches!(p.side(plane), Some(-1 | 0)) {
                return Ok(false);
            }
        }
        raised |= p.side(cap) == Some(-1);
        points.insert(v, p);
    }
    if !raised {
        return Ok(false);
    }
    for (i, a) in points.values().enumerate() {
        kernel.check_cancelled(OP)?;
        for c in points.values().skip(i + 1) {
            let Some(delta) = a.delta(c) else {
                return Ok(false);
            };
            if delta.iter().all(|e| ex::is_zero(e)) {
                return Ok(false);
            }
        }
    }
    for planes in edges.values() {
        let [a, c] = planes.as_slice() else {
            return Ok(false);
        };
        let Some(v) = ex::cross(a.outward, c.outward) else {
            return Ok(false);
        };
        if !v.iter().all(|e| e.iter().all(|c| c.is_finite())) || v.iter().all(|e| ex::is_zero(e)) {
            return Ok(false);
        }
    }
    for (cycle, plane) in cycles {
        let mut orientation = None;
        for (&a, &c) in cycle.iter().zip(cycle.iter().cycle().skip(1)) {
            kernel.check_cancelled(OP)?;
            let pa = points
                .get(&a)
                .ok_or_else(|| missing("cell vertex absent"))?;
            let pc = points
                .get(&c)
                .ok_or_else(|| missing("cell vertex absent"))?;
            for v in cycle.iter().filter(|&&v| v != a && v != c) {
                let pv = points.get(v).ok_or_else(|| missing("cell vertex absent"))?;
                let sign = || {
                    Some(ex::sign(&dot(
                        &cross(&pa.delta(pc)?, &pa.delta(pv)?)?,
                        plane.outward,
                    )?))
                };
                let Some(s @ (-1 | 1)) = sign() else {
                    return Ok(false);
                };
                if orientation.is_some_and(|o| o != s) {
                    return Ok(false);
                }
                orientation = Some(s);
            }
        }
    }
    Ok(true)
}
