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
    range: Ivl,
    zone: f64,
    parent: usize,
}

pub(super) struct Levels {
    vertices: Vec<(VertexId, [f64; 3])>,
    face_roots: Vec<(FaceId, usize)>,
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

/// A shared fan can carry multiple faces with the same dominant axis.
/// Join only supports whose complete chart domains agree with a shared level.
fn link(faces: &mut [Support], frame: &Frame, first: usize, second: usize) -> Option<bool> {
    let a = representative(faces, first)?;
    let b = representative(faces, second)?;
    if a == b {
        return Some(true);
    }
    let (first, second) = (faces.get(a)?, faces.get(b)?);
    let gap = frame.plane_gap(first.axis, first.range, second.coordinate.mid())?;
    let reverse = frame.plane_gap(second.axis, second.range, first.coordinate.mid())?;
    let zone = first.zone.min(second.zone);
    if !(zone.is_finite() && gap.max(reverse) <= zone) {
        return Some(false);
    }
    faces.get_mut(a.max(b))?.parent = a.min(b);
    Some(true)
}

impl Levels {
    pub(super) fn build(k: &Kernel, b: &Brep, frame: &Frame) -> Result<Option<Self>> {
        let mut supports = Vec::new();
        reserve(&mut supports, b.face_list().len())?;
        for &face in b.face_list() {
            k.check_cancelled(OP)?;
            let Some(plane) = read_plane(b, face)? else {
                continue;
            };
            let domain = face2::uv_domain(b, face)?;
            let Some((axis, coordinate, range)) = frame.plane_coordinate(plane, &domain) else {
                continue;
            };
            let zone = face2::tolerance(b, face)?.min(b.tolerance());
            if !frame
                .plane_gap(axis, range, coordinate.mid())
                .is_some_and(|gap| zone.is_finite() && gap <= zone)
            {
                continue;
            }
            supports.push(Support {
                face,
                axis,
                coordinate,
                range,
                zone,
                parent: 0,
            });
        }
        supports.sort_unstable_by_key(|support| support.face);
        for (i, support) in supports.iter_mut().enumerate() {
            support.parent = i;
        }

        let mut fans = Vec::new();
        reserve(&mut fans, b.vertex_list().len())?;
        'vertices: for &v in b.vertex_list() {
            k.check_cancelled(OP)?;
            let mut axes = [None; 3];
            let tagged = vertex::faces(b, v)?;
            if tagged.iter().any(|face| {
                supports
                    .binary_search_by_key(face, |support| support.face)
                    .is_err()
            }) {
                continue;
            }
            for face in tagged {
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
                    match link(&mut supports, frame, first, index) {
                        Some(true) => {}
                        // This fan cannot supply a unique placed point.
                        Some(false) => continue 'vertices,
                        None => return Ok(None),
                    }
                } else {
                    *slot = Some(index);
                }
            }
            let [Some(a), Some(b), Some(c)] = axes else {
                continue;
            };
            fans.push((v, [a, b, c]));
        }

        let mut faces = Vec::new();
        reserve(&mut faces, supports.len())?;
        let mut face_roots: Vec<(FaceId, usize)> = Vec::new();
        reserve(&mut face_roots, supports.len())?;
        for (i, support) in supports.iter().enumerate() {
            k.check_cancelled(OP)?;
            // Parents precede their children, so the cached parent coordinate
            // already belongs to the lowest-ID face of the whole class.
            let coordinate = if support.parent == i {
                support.coordinate.mid()
            } else {
                let Some(&coordinate) = faces.get(support.parent) else {
                    return Ok(None);
                };
                coordinate
            };
            let Some(gap) = frame.plane_gap(support.axis, support.range, coordinate) else {
                return Ok(None);
            };
            if !(support.zone.is_finite() && gap <= support.zone) {
                return Ok(None);
            }
            faces.push(coordinate);
            let root = if support.parent == i {
                i
            } else {
                let Some(&(_, root)) = face_roots.get(support.parent) else {
                    return Ok(None);
                };
                root
            };
            face_roots.push((support.face, root));
        }

        // Separate classes may share coordinates. The corridor bounds its
        // trims and tests foreign contacts locally, where proximity matters.
        let mut vertices = Vec::new();
        reserve(&mut vertices, fans.len())?;
        for (vertex, [a, b, c]) in fans {
            k.check_cancelled(OP)?;
            let (Some(a), Some(b), Some(c)) = (faces.get(a), faces.get(b), faces.get(c)) else {
                return Ok(None);
            };
            vertices.push((vertex, [*a, *b, *c]));
        }
        vertices.sort_unstable_by_key(|(vertex, _)| *vertex);

        Ok(Some(Self {
            vertices,
            face_roots,
        }))
    }

    pub(super) fn same_support(&self, a: FaceId, b: FaceId) -> bool {
        let root = |face| {
            let i = self
                .face_roots
                .binary_search_by_key(&face, |&(id, _)| id)
                .ok()?;
            self.face_roots.get(i).map(|&(_, root)| root)
        };
        root(a).zip(root(b)).is_some_and(|(a, b)| a == b)
    }

    pub(super) fn vertex(&self, vertex: VertexId) -> Option<[f64; 3]> {
        let index = self
            .vertices
            .binary_search_by_key(&vertex, |(vertex, _)| *vertex)
            .ok()?;
        self.vertices.get(index).map(|&(_, point)| point)
    }
}
