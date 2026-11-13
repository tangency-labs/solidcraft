//! A perforated spherical quad grid attached to the ordered branch rings.

use super::{bad, validate_branch_count};
use crate::eval::vec::VecExt;
use crate::geom::form_cage::{CageGeometry, MAX_FACES, MAX_POINTS};
use crate::{Result, Vec3};
use std::collections::{BTreeMap, BTreeSet};

/// An ordered port with existing control indices and their coordinates.
pub struct Port {
    /// Branch direction away from the junction.
    pub direction: Vec3,
    /// Ordered ring of existing controls, with their coordinates.
    pub ring: Vec<(usize, Vec3)>,
    /// Branch radius.
    pub radius: f64,
    /// Whether the attachment consumes the branch ring in reverse order.
    pub reverse: bool,
}

const AXES: [Vec3; 6] = [
    Vec3::X,
    Vec3::new(-1.0, 0.0, 0.0),
    Vec3::Y,
    Vec3::new(0.0, -1.0, 0.0),
    Vec3::Z,
    Vec3::new(0.0, 0.0, -1.0),
];

fn assignments(ports: &[Port]) -> Result<Vec<usize>> {
    fn search(
        ports: &[Port],
        used: u8,
        chosen: &mut Vec<usize>,
        score: f64,
        best: &mut (f64, Vec<usize>),
    ) {
        let Some(port) = ports.get(chosen.len()) else {
            if score > best.0 {
                *best = (score, chosen.clone());
            }
            return;
        };
        for (axis, direction) in AXES.into_iter().enumerate() {
            if used & (1 << axis) == 0 {
                chosen.push(axis);
                search(
                    ports,
                    used | (1 << axis),
                    chosen,
                    score + port.direction.dot(direction),
                    best,
                );
                chosen.pop();
            }
        }
    }
    let mut best = (f64::NEG_INFINITY, Vec::new());
    search(ports, 0, &mut Vec::new(), 0.0, &mut best);
    if best.1.len() != ports.len() {
        return Err(bad("pipe junction has no distinct port assignment"));
    }
    Ok(best.1)
}

/// Construct only the added controls and faces. Face indices refer to the
/// combined cage, so existing branch controls and face slots retain their IDs.
pub fn append_geometry(
    point_count: usize,
    face_count: usize,
    center: Vec3,
    ports: &[Port],
    density: usize,
) -> Result<CageGeometry> {
    validate_branch_count(ports.len())?;
    if ports.len() < 3
        || !(1..=12).contains(&density)
        || ports.iter().any(|p| p.ring.len() != 8 * density)
    {
        return Err(bad("a pipe junction needs 3..6 compatible branch rings"));
    }
    if point_count > MAX_POINTS || face_count > MAX_FACES {
        return Err(bad("pipe control cage exceeds its editing budget"));
    }
    let mut rings = BTreeSet::new();
    if !center.is_finite()
        || ports.iter().any(|port| {
            !port.direction.is_finite()
                || !(0.05..=5_000.0).contains(&port.radius)
                || port.ring.iter().any(|&(id, point)| {
                    id >= point_count || !point.is_finite() || !rings.insert(id)
                })
        })
    {
        return Err(bad(
            "pipe junction needs finite directions and distinct existing controls",
        ));
    }
    let assigned = assignments(ports)?;
    let low = i32::try_from(density).map_err(|_| bad("pipe junction density is out of range"))?;
    let high = 3 * low;
    let n = 4 * low;
    let frames = [
        ([n, 0, 0], [0, 1, 0], [0, 0, 1]),
        ([0, 0, 0], [0, 0, 1], [0, 1, 0]),
        ([0, n, n], [1, 0, 0], [0, 0, -1]),
        ([0, 0, 0], [1, 0, 0], [0, 0, 1]),
        ([0, 0, n], [1, 0, 0], [0, 1, 0]),
        ([0, 0, 0], [0, 1, 0], [1, 0, 0]),
    ];
    let key = |axis, i, j| -> Result<_> {
        let &([ox, oy, oz], [ux, uy, uz], [vx, vy, vz]) = frames
            .get(axis)
            .ok_or_else(|| bad("invalid pipe collar axis"))?;
        Ok([
            ox + ux * i + vx * j,
            oy + uy * i + vy * j,
            oz + uz * i + vz * j,
        ])
    };
    let added_faces = (6 * 4 * 4 - ports.len() * 2 * 2) * density * density;
    if face_count + added_faces > MAX_FACES {
        return Err(bad("pipe control cage exceeds its editing budget"));
    }
    let radius = ports.iter().map(|p| p.radius).fold(0.0_f64, f64::max) * 1.35;
    let mut vertices = BTreeMap::new();
    let mut points = Vec::new();
    let mut faces = Vec::new();
    for axis in 0..6 {
        for j in 0..n {
            for i in 0..n {
                if assigned.contains(&axis) && (low..high).contains(&i) && (low..high).contains(&j)
                {
                    continue;
                }
                let mut face = Vec::with_capacity(4);
                for (s, t) in [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)] {
                    let k @ [x, y, z] = key(axis, s, t)?;
                    let id = if let Some(&id) = vertices.get(&k) {
                        id
                    } else {
                        let direction = Vec3::new(f64::from(x), f64::from(y), f64::from(z))
                            * (2.0 / f64::from(n))
                            - Vec3::new(1.0, 1.0, 1.0);
                        let unit = direction
                            .normalized()
                            .ok_or_else(|| bad("pipe junction has no radial direction"))?;
                        let id = points.len();
                        points.push(center + unit * radius);
                        vertices.insert(k, id);
                        id
                    };
                    face.push(id);
                }
                faces.push(face);
            }
        }
    }
    let mut mapped = BTreeMap::new();
    for (port, &axis) in ports.iter().zip(&assigned) {
        let wire = (low..high)
            .map(|i| (i, low))
            .chain((low..high).map(|j| (high, j)))
            .chain((low + 1..=high).rev().map(|i| (i, high)))
            .chain((low + 1..=high).rev().map(|j| (low, j)))
            .map(|(i, j)| {
                vertices
                    .get(&key(axis, i, j)?)
                    .copied()
                    .ok_or_else(|| bad("pipe junction lost a collar vertex"))
            })
            .collect::<Result<Vec<_>>>()?;
        let at = |offset: usize, i: usize| {
            let index = if port.reverse {
                offset + wire.len() - i
            } else {
                offset + i
            };
            index
                .checked_rem(wire.len())
                .and_then(|index| port.ring.get(index))
                .ok_or_else(|| bad("pipe junction lost a branch rim control"))
        };
        let offset = (0..wire.len())
            .map(|offset| {
                let mut distance = 0.0;
                for (i, &v) in wire.iter().enumerate() {
                    let &(_, control) = at(offset, i)?;
                    let point = points
                        .get(v)
                        .copied()
                        .ok_or_else(|| bad("pipe junction lost a collar vertex"))?;
                    distance += (point - control).len2();
                }
                Ok((distance, offset))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|v| v.0.is_finite())
            .map(|v| v.1)
            .ok_or_else(|| bad("pipe junction cannot align its branch rim"))?;
        for (i, &v) in wire.iter().enumerate() {
            let &(id, _) = at(offset, i)?;
            if mapped.insert(v, id).is_some() {
                return Err(bad("pipe junction collars overlap"));
            }
        }
    }
    if point_count + points.len() - mapped.len() > MAX_POINTS {
        return Err(bad("pipe control cage exceeds its editing budget"));
    }
    let mut added_points = Vec::new();
    let ids: Vec<_> = points
        .into_iter()
        .enumerate()
        .map(|(id, point)| {
            mapped.get(&id).copied().unwrap_or_else(|| {
                let index = point_count + added_points.len();
                added_points.push(point);
                index
            })
        })
        .collect();
    let faces = faces
        .into_iter()
        .map(|face| {
            face.into_iter()
                .map(|v| {
                    ids.get(v)
                        .copied()
                        .ok_or_else(|| bad("pipe junction lost a face control"))
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(CageGeometry {
        points: added_points,
        faces,
    })
}
