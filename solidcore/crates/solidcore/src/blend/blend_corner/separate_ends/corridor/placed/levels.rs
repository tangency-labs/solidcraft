//! Recover shared support levels from the incidence of a placed planar body.

use super::{read_plane, Frame};
use crate::blend::fillet_section::interval::Ivl;
use crate::error::topology::TopologyFailure;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::topology::face::p2 as face2;
use crate::topology::{vertex, Brep, FaceId, VertexId};

const OP: &str = "blend::blend_corner::separate_ends::placed_levels";

struct Support {
    face: FaceId,
    axis: usize,
    coordinate: Ivl,
    parent: usize,
}

pub(super) struct Levels {
    vertices: Vec<(VertexId, [f64; 3])>,
}

fn reserve<T>(items: &mut Vec<T>, count: usize) -> Result<()> {
    items
        .try_reserve_exact(count)
        .map_err(|_| SolidError::topology(OP, TopologyFailure::AllocationFailed))
}

fn representative(faces: &[Support], mut current: usize) -> Option<usize> {
    loop {
        let parent = faces.get(current)?.parent;
        if parent == current {
            return Some(current);
        }
        // Every link points to the earlier face, so this cannot cycle.
        if parent > current {
            return None;
        }
        current = parent;
    }
}

fn link(faces: &mut [Support], first: usize, second: usize) -> Option<()> {
    let a = representative(faces, first)?;
    let b = representative(faces, second)?;
    faces.get_mut(a.max(b))?.parent = a.min(b);
    Some(())
}

impl Levels {
    pub(super) fn build(k: &Kernel, b: &Brep, frame: &Frame) -> Result<Option<Self>> {
        let mut supports = Vec::new();
        reserve(&mut supports, b.face_list().len())?;
        for &face in b.face_list() {
            k.check_cancelled(OP)?;
            let Some((axis, coordinate)) =
                read_plane(b, face)?.and_then(|plane| frame.plane_coordinate(plane))
            else {
                return Ok(None);
            };
            supports.push(Support {
                face,
                axis,
                coordinate,
                parent: 0,
            });
        }
        supports.sort_unstable_by_key(|support| support.face);
        for (i, support) in supports.iter_mut().enumerate() {
            support.parent = i;
        }

        let mut fans = Vec::new();
        reserve(&mut fans, b.vertex_list().len())?;
        for &v in b.vertex_list() {
            k.check_cancelled(OP)?;
            let mut axes = [None; 3];
            for face in vertex::faces(b, v)? {
                let Ok(index) = supports.binary_search_by_key(&face, |support| support.face) else {
                    return Ok(None);
                };
                let Some(slot) = supports
                    .get(index)
                    .and_then(|support| axes.get_mut(support.axis))
                else {
                    return Ok(None);
                };
                if let Some(first) = *slot {
                    if link(&mut supports, first, index).is_none() {
                        return Ok(None);
                    }
                } else {
                    *slot = Some(index);
                }
            }
            let [Some(a), Some(b), Some(c)] = axes else {
                return Ok(None);
            };
            fans.push((v, [a, b, c]));
        }

        let mut faces = Vec::new();
        reserve(&mut faces, supports.len())?;
        let Some(margin) = frame.width(2.0 * b.tolerance()) else {
            return Ok(None);
        };
        let mut levels = Vec::new();
        reserve(&mut levels, supports.len())?;
        for (i, support) in supports.iter().enumerate() {
            let Some(rep) = representative(&supports, i) else {
                return Ok(None);
            };
            let Some(source) = supports.get(rep) else {
                return Ok(None);
            };
            let coordinate = source.coordinate.mid();
            let zone = face2::tolerance(b, support.face)?.min(b.tolerance());
            let Some(gap) = frame.plane_gap(support.axis, support.coordinate, coordinate) else {
                return Ok(None);
            };
            if !zone.is_finite() || gap > zone {
                return Ok(None);
            }
            if rep == i {
                levels.push((support.axis, coordinate));
            }
            faces.push(coordinate);
        }

        // Distinct levels must remain ordered throughout their admitted bands.
        levels.sort_unstable_by(|(a, x), (b, z)| a.cmp(b).then_with(|| x.total_cmp(z)));
        for pair in levels.windows(2) {
            let [(a, x), (b, z)] = pair else {
                return Ok(None);
            };
            if a == b && Ivl::from(*z).sub_ulp(Ivl::from(*x)).lo <= margin {
                return Ok(None);
            }
        }

        let mut vertices = Vec::new();
        reserve(&mut vertices, fans.len())?;
        for (vertex, [a, b, c]) in fans {
            let (Some(a), Some(b), Some(c)) = (faces.get(a), faces.get(b), faces.get(c)) else {
                return Ok(None);
            };
            vertices.push((vertex, [*a, *b, *c]));
        }
        vertices.sort_unstable_by_key(|(vertex, _)| *vertex);

        Ok(Some(Self { vertices }))
    }

    pub(super) fn vertex(&self, vertex: VertexId) -> Option<[f64; 3]> {
        let index = self
            .vertices
            .binary_search_by_key(&vertex, |(vertex, _)| *vertex)
            .ok()?;
        self.vertices.get(index).map(|&(_, point)| point)
    }
}
