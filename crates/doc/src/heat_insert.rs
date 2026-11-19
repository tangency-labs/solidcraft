//! The wall and floor a Heat-Set Insert needs (#12378), measured on a body's exact geometry.
//!
//! The material is measured against the air around it: the part of a box around the insert that
//! is not the body, cut at the face the hole starts on. The wall is thick enough when a cylinder
//! `min_wall` wider than the insert, over its length, shares no volume with the air between the
//! face and the hole's bottom (and, around a chamfer wider than the insert, a cylinder `min_wall`
//! wider than its rim, as deep as it); the floor, when a disc `min_floor` thick under the insert
//! shares none with the air below the bottom. Both are exact Booleans; a thin one is measured by
//! halving.

use solidcore_record::{Plane, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, BoolOp};

use crate::{DocError, Result};

/// Where one insert goes and what it needs, in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InsertPlace {
    /// The centre of the hole on its face.
    pub at: Vec3,
    /// Into the material, a unit vector.
    pub into: Vec3,
    /// The insert's outer radius and length.
    pub outer_radius: f64,
    pub length: f64,
    /// A chamfer wider than the insert: its rim's radius and its depth.
    pub mouth: Option<(f64, f64)>,
    /// The hole's depth.
    pub depth: f64,
    pub min_wall: f64,
    pub min_floor: f64,
}

/// The wall around the insert and the floor below its hole, in mm, where they are thinner than
/// `min_wall` and `min_floor` (`None`: at least that thick).
pub fn wall_and_floor(body: &Body, p: &InsertPlace) -> Result<(Option<f64>, Option<f64>)> {
    let InsertPlace { at, into, outer_radius: outer_r, length, mouth, depth, min_wall, min_floor } = *p;
    // An odd size, so the box's faces don't meet the body's on a round grid.
    let widest = mouth.map_or(outer_r, |(r, _)| r.max(outer_r));
    let h = (depth + widest + min_wall.max(min_floor)) * 1.0137 + 1.37;
    let around = solidcraft_kernel::box_solid(at - Vec3::new(h, h, h), at + Vec3::new(h, h, h))?;
    let Some(air) = solidcraft_kernel::boolean(&around, body, BoolOp::Cut)? else { return Ok((None, None)) };
    let plane = |o: Vec3| Plane::from_normal(o, into).ok_or_else(|| DocError::Invalid("heat-set insert: no plane along the hole".into()));
    let Some(inside) = solidcraft_kernel::half_space(&air, &plane(at)?, true)?.0 else { return Ok((None, None)) };
    let bottom = at + into * depth;
    let below = solidcraft_kernel::half_space(&inside, &plane(bottom)?, true)?.0;
    let beside = solidcraft_kernel::half_space(&inside, &plane(bottom)?, false)?.0;
    // The wall: a cylinder `w` wider than the insert, over its length, meets the air beside it.
    // Around a chamfer wider than the insert, a cylinder `w` wider than its rim, as deep as it.
    let wall = match &beside {
        Some(a) => {
            let insert = thinner(a, min_wall, |w| solidcraft_kernel::cylinder(at, into, outer_r + w, length))?;
            let rim = match mouth {
                Some((r, c)) => thinner(a, min_wall, |w| solidcraft_kernel::cylinder(at, into, r + w, c))?,
                None => None,
            };
            match (insert, rim) {
                (Some(i), Some(r)) => Some(i.min(r)),
                (i, r) => i.or(r),
            }
        }
        None => None,
    };
    // The floor: a disc under the insert's footprint, `w` thick, meets the air below the hole. Air
    // below the bottom but off to the side is a wall, not a floor.
    let floor = match &below {
        Some(a) => thinner(a, min_floor, |w| solidcraft_kernel::cylinder(bottom, into, outer_r, w))?,
        None => None,
    };
    Ok((wall, floor))
}

/// The thickness at which the tool `grown(w)` first meets `air`, when that is less than `least`
/// (to a thousandth of `least`), by halving; `None` when `grown(least)` stays clear of it.
fn thinner(air: &Body, least: f64, grown: impl Fn(f64) -> solidcraft_kernel::Result<Body>) -> Result<Option<f64>> {
    let meets = |w: f64| -> Result<bool> {
        Ok(match solidcraft_kernel::boolean(&grown(w)?, air, BoolOp::Intersect)? {
            Some(shared) => solidcraft_kernel::measure_mass(&shared)?.volume > 1e-9,
            None => false,
        })
    };
    if !meets(least)? {
        return Ok(None);
    }
    let (mut clear, mut met) = (0.0, least);
    while met - clear > least * 1e-3 {
        let mid = 0.5 * (clear + met);
        if meets(mid)? {
            met = mid;
        } else {
            clear = mid;
        }
    }
    Ok(Some(0.5 * (clear + met)))
}
