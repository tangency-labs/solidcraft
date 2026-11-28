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
    level: f64,
    allowed: Ivl,
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
fn link(faces: &mut [Support], first: usize, second: usize) -> Option<bool> {
    let a = representative(faces, first)?;
    let b = representative(faces, second)?;
    if a == b {
        return Some(true);
    }
    let (root, joined) = (a.min(b), a.max(b));
    let first = faces.get(root)?.allowed;
    let second = faces.get(joined)?.allowed;
    let lo = first.lo.max(second.lo);
    let hi = first.hi.min(second.hi);
    if lo > hi {
        return Some(false);
    }
    faces.get_mut(root)?.allowed = Ivl::new(lo, hi);
    faces.get_mut(joined)?.parent = root;
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
            let Some(allowed) = frame.level_window(axis, range, zone) else {
                continue;
            };
            let level = coordinate.mid();
            if !(allowed.lo <= level && level <= allowed.hi) {
                continue;
            }
            supports.push(Support {
                face,
                axis,
                level,
                allowed,
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
                    match link(&mut supports, first, index) {
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
            // Parents precede their children, so each non-root gets the
            // level selected from the entire class's admissible interval.
            let coordinate = if support.parent == i {
                let allowed = support.allowed;
                if allowed.lo <= support.level && support.level <= allowed.hi {
                    support.level
                } else {
                    allowed.lo.midpoint(allowed.hi)
                }
            } else {
                let Some(&coordinate) = faces.get(support.parent) else {
                    return Ok(None);
                };
                coordinate
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;

    #[test]
    fn support_joins_keep_a_level_admissible_for_the_whole_class() {
        let frame = Frame {
            axes: [Vec3::X, Vec3::Y, Vec3::Z].map(|axis| super::super::Axis::new(axis).unwrap()),
        };
        let mut faces = [(0, 0.0), (1, 0.75), (2, 1.5), (3, 3.0)].map(|(i, level)| Support {
            face: FaceId::from_index(i),
            axis: 0,
            level,
            allowed: frame.level_window(0, Ivl::from(level), 1.0).unwrap(),
            parent: usize::try_from(i).unwrap(),
        });
        assert_eq!(link(&mut faces, 1, 2), Some(true));
        assert_eq!(link(&mut faces, 0, 1), Some(true));
        let allowed = faces[0].allowed;
        assert!(allowed.lo > 0.0 && allowed.lo < 0.75 && 0.75 < allowed.hi);
        assert!(allowed.hi <= 1.0);
        assert_eq!(link(&mut faces, 1, 3), Some(false));
        assert_eq!(faces[0].allowed, allowed);
        assert_eq!(representative(&faces, 2), Some(0));
        assert_eq!(representative(&faces, 3), Some(3));
    }
}
