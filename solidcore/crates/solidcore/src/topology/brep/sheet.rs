//! A one-face sheet on a surface's whole natural box ([`sheet_on`]), the kernel's one
//! arena-notify `CreateFaceFromSurface` wrapper, a face on a box of a surface's parameters
//! ([`face_on`]), a sheet with its faces merged where they are one face ([`merged_faces`]),
//! and a sheet turned over ([`turn_over_shell`], [`turn_over_sheet`]).
//!
//! Not upstream: the composition an operation that rebuilds a face on a new surface (a sheet
//! extended past a side) makes of `new_brep` and `SmBrep::CreateFaceFromSurface`, kept in the
//! kernel so the modelling layer holds no notification plumbing; and the turn-over of every
//! face of an open shell ([`crate::topology::face::p1::turn_over`]).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::BTreeSet;

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::USE_DEFAULT;
use crate::kernel::Kernel;
use crate::surface::surface::Surface;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use crate::topology::face::p1::turn_over;
use crate::topology::topology_traverser::Marks;
use crate::topology::{Brep, FaceId, ShellId};
use crate::EntityRef;

/// A new body of one face on `surface` over its whole natural box, at `tolerance`, and that
/// face.
///
/// # Errors
///
/// Those of the box query, `new_brep` and `CreateFaceFromSurface`.
pub fn sheet_on(k: &mut Kernel, surface: Surface, tolerance: f64) -> Result<(Brep, FaceId)> {
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    b.set_tolerance(tolerance);
    let f = face_on(k, &mut b, surface, &bx)?;
    b.refresh_cached_topology();
    Ok((b, f))
}

/// `SmBrep::CreateFaceFromSurface(surface, bx, face)`: a new face of `b` on `surface` over the
/// box `bx` of its parameters, bounded by one loop of the box's sides, notifying through an
/// arena-only store (default records, no attributes, no context), the new face owning its
/// surface under its arena identity. The kernel's one such wrapper; the caller refreshes `b`'s
/// cached topology when it reads it next.
///
/// # Errors
///
/// Those of `CreateFaceFromSurface`.
pub fn face_on(k: &mut Kernel, b: &mut Brep, surface: Surface, bx: &Extent2d) -> Result<FaceId> {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, b, Some(surface), bx, &mut fenv)
}

const OP_MERGED: &str = "topology::brep::sheet::merged_faces";

/// A sheet with its faces merged where they are one face ([`merged_faces`]), and where each of
/// the sheet's faces and edges went.
#[derive(Debug)]
pub struct Merged {
    /// The merged copy.
    pub brep: Brep,
    /// Each face of the sheet and the face of `brep` it is now part of.
    pub faces: Vec<(FaceId, FaceId)>,
    /// Each edge of the sheet and the edge of `brep` it is now part of, or `None` for an edge
    /// that is gone (between two faces merged into one).
    pub edges: Vec<(crate::topology::EdgeId, Option<crate::topology::EdgeId>)>,
}

/// A copy of `b` with its faces merged across every edge between two faces of one surface and
/// its edges joined at every vertex between two tangent edges
/// (`SmBrep::RemoveTopologicalEdgesAndVertices`, which keeps geometry and merges the merged
/// entities' attributes): a flat sheet the skin split into plane faces becomes one plane face.
/// Each of `b`'s edges maps to the edge it is still part of: itself where it survives, else the
/// edge through its middle (within ten of the body's tolerance; a joined piece), else `None`
/// (an edge removed between two merged faces). Each face maps to the face it was merged into:
/// the one surviving face of the faces joined across removed edges. `b` is unchanged.
///
/// # Errors
///
/// Those of the copy, the merge and the edge queries; [`SolidError::topology_failed`] for a
/// group of merged faces that does not leave exactly one face.
pub fn merged_faces(k: &mut Kernel, b: &Brep) -> Result<Merged> {
    use crate::topology::brep::p1::{edges, faces};
    use crate::topology::EdgeId;
    use std::collections::BTreeMap;

    let (mut c, map) = crate::topology::brep::p3::brep_copy_mapped(k, b)?;
    // the map holds each of `b`'s entities and its copy (source to copy)
    let copy_of = |r: EntityRef| -> Result<Option<EntityRef>> { map.get_at(Some(r)) };
    // before the merge: each edge of `b`, its copy, the copy's middle and its faces
    let mut copied_edges = Vec::new();
    for oe in edges(b, None)? {
        let Some(EntityRef::Edge(ce)) = copy_of(EntityRef::Edge(oe))? else {
            return Err(SolidError::topology_failed(
                OP_MERGED,
                "an edge without its copy",
            ));
        };
        let record = c.edge(ce)?;
        let curve = record
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::topology_missing(OP_MERGED, "an edge's curve"))?;
        let middle = curve.evaluate_point(k, record.interval.mid())?;
        let mut on = Vec::new();
        for eu in crate::topology::edge::edgeuses(&c, ce, None, None)? {
            if let Some(f) = crate::topology::edgeuse::p1::face(&c, eu)? {
                if !on.contains(&f) {
                    on.push(f);
                }
            }
        }
        copied_edges.push((oe, ce, middle, on));
    }
    let mut copied_faces = Vec::new();
    for of in faces(b, None)? {
        let Some(EntityRef::Face(cf)) = copy_of(EntityRef::Face(of))? else {
            return Err(SolidError::topology_failed(
                OP_MERGED,
                "a face without its copy",
            ));
        };
        copied_faces.push((of, cf));
    }

    crate::topology::brep::p5::remove_topological_edges_and_vertices(k, &mut c, None, None)?;
    c.refresh_cached_topology();

    let live_edges = edges(&c, None)?;
    let live_faces = faces(&c, None)?;
    let near = 10.0 * c.tolerance().max(1e-9);
    // each edge: itself, else the edge through its middle, else gone
    let mut edge_map = Vec::new();
    let mut removed: Vec<&[FaceId]> = Vec::new();
    for (oe, ce, middle, on) in &copied_edges {
        let now = if live_edges.contains(ce) {
            Some(*ce)
        } else {
            let mut best: Option<(f64, EdgeId)> = None;
            for &e in &live_edges {
                let Ok(at) = crate::query::api_queries::edge_closest_point(k, &c, e, *middle)
                else {
                    continue;
                };
                let d = at.closest.distance;
                if d <= near && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, e));
                }
            }
            best.map(|(_, e)| e)
        };
        if now.is_none() {
            removed.push(on.as_slice());
        }
        edge_map.push((*oe, now));
    }
    // the faces joined across removed edges, each group one label
    let mut label: BTreeMap<FaceId, usize> = copied_faces
        .iter()
        .enumerate()
        .map(|(i, &(_, cf))| (cf, i))
        .collect();
    for on in removed {
        let mut groups: Vec<usize> = on.iter().filter_map(|f| label.get(f).copied()).collect();
        groups.sort_unstable();
        groups.dedup();
        if let Some((&keep, rest)) = groups.split_first() {
            for v in label.values_mut() {
                if rest.contains(v) {
                    *v = keep;
                }
            }
        }
    }
    let mut face_map = Vec::new();
    for &(of, cf) in &copied_faces {
        let group = label.get(&cf).copied();
        let mut survivors = live_faces
            .iter()
            .filter(|&f| label.get(f).copied() == group);
        let (Some(&now), None) = (survivors.next(), survivors.next()) else {
            return Err(SolidError::topology_failed(
                OP_MERGED,
                "merged faces that do not leave exactly one face",
            ));
        };
        face_map.push((of, now));
    }
    Ok(Merged {
        brep: c,
        faces: face_map,
        edges: edge_map,
    })
}

#[cfg(test)]
mod merged_tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::math::Point3;
    use crate::primitive::api_curves::{create_arc, create_line_segment};
    use crate::sweep::api_surfaces::create_skin_primitive;
    use crate::topology::EdgeId;

    /// The flat arch: the line from (10, 0) to (0, 0) and the half circle about (5, 5) of
    /// radius 5 from (10, 5) over (5, 10) to (0, 5) (two quadratic spans at a double knot),
    /// skinned in the plane z = 0.
    fn flat_arch(k: &mut Kernel) -> Brep {
        let line = Curve::from(
            create_line_segment(Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)).unwrap(),
        );
        let arc = Curve::from(create_arc(Point3::new(5.0, 5.0, 0.0), 5.0, 0.0, 180.0).unwrap());
        create_skin_primitive(k, &[1, 1], &[Some(&line), Some(&arc)], 1, false)
            .unwrap()
            .brep
    }

    /// The edge of `b` through `p`.
    fn edge_through(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
        let p = Point3::new(p[0], p[1], p[2]);
        crate::topology::brep::p1::edges(b, None)
            .unwrap()
            .into_iter()
            .find(|&e| {
                crate::query::api_queries::edge_closest_point(k, b, e, p)
                    .is_ok_and(|c| c.closest.distance < 1e-9)
            })
            .unwrap_or_else(|| panic!("no edge through {p:?}"))
    }

    fn mapped(m: &Merged, e: EdgeId) -> Option<EdgeId> {
        m.edges.iter().find(|(o, _)| *o == e).unwrap().1
    }

    #[test]
    fn the_flat_arch_merges_into_one_face_its_halves_into_one_edge() {
        let mut k = Kernel::default();
        let b = flat_arch(&mut k);
        // the skin split the wall at the arc's double knot: two faces, their seam x = 5
        assert_eq!(b.face_list().len(), 2);
        let d = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
        let halves = [
            edge_through(&k, &b, [5.0 + d, 5.0 + d, 0.0]),
            edge_through(&k, &b, [5.0 - d, 5.0 + d, 0.0]),
        ];
        let bottom = [
            edge_through(&k, &b, [7.5, 0.0, 0.0]),
            edge_through(&k, &b, [2.5, 0.0, 0.0]),
        ];
        let seam = edge_through(&k, &b, [5.0, 5.0, 0.0]);
        assert_ne!(halves[0], halves[1]);
        let (faces_before, edges_before) = (b.face_list().to_vec(), b.edge_list().to_vec());

        let m = merged_faces(&mut k, &b).unwrap();
        // one face of four edges: the bottom, the two sides and the arc
        let faces = crate::topology::brep::p1::faces(&m.brep, None).unwrap();
        assert_eq!(faces.len(), 1);
        assert_eq!(
            crate::topology::brep::p1::edges(&m.brep, None)
                .unwrap()
                .len(),
            4
        );
        // both half arcs are one edge now, and so are both halves of the bottom
        let arc = mapped(&m, halves[0]);
        assert!(arc.is_some());
        assert_eq!(mapped(&m, halves[1]), arc);
        let line = mapped(&m, bottom[0]);
        assert!(line.is_some() && line != arc);
        assert_eq!(mapped(&m, bottom[1]), line);
        // the seam is gone
        assert_eq!(mapped(&m, seam), None);
        // both faces are the merged face
        assert_eq!(m.faces.len(), 2);
        assert!(m.faces.iter().all(|&(_, f)| f == faces[0]));
        // every edge of the sheet is mapped, and the sheet is unchanged
        assert_eq!(m.edges.len(), edges_before.len());
        assert_eq!(b.face_list(), faces_before.as_slice());
        assert_eq!(b.edge_list(), edges_before.as_slice());
    }
}

const OP_TURN: &str = "topology::brep::sheet::turn_over_shell";

/// Turns the open shell `s` over: each of its faces ([`turn_over`], `SmFace::SwapUV`), so each
/// face's side is the other one, its edges as they were; the body stays valid. Turning a shell
/// over twice gives it back.
///
/// A loop of [`turn_over`], not `SmBrep::SwapUV` or `OrientSheetSurfaces(same = false)`: those
/// ignore a face's failed swap (upstream's `sm_SwapFaceUV` is void), and a turn-over that stops
/// part way must say so and leave the shell as it was to the caller's rollback.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a closed shell (one whose faces' other uses lie in another
/// shell: turned over, the body would be inside out), before anything changes;
/// [`SolidError::InvalidInput`] for a shell of no faces; those of [`turn_over`].
pub fn turn_over_shell(k: &Kernel, b: &mut Brep, s: ShellId) -> Result<()> {
    let faces = open_shell_faces(b, s)?;
    for f in faces {
        turn_over(k, b, f)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// Turns every shell of faces of `b` over ([`turn_over_shell`]): a sheet turned over.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a body with a closed shell (a solid), before anything
/// changes; those of [`turn_over`].
pub fn turn_over_sheet(k: &Kernel, b: &mut Brep) -> Result<()> {
    let mut faces = Vec::new();
    let shells: Vec<ShellId> = b.shells().ids().collect();
    for s in shells {
        if b.shell(s)?.is_faceuse_shell() {
            faces.extend(open_shell_faces(b, s)?);
        }
    }
    for f in faces {
        turn_over(k, b, f)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// The faces of the open shell `s`, each once, in the order of their uses.
fn open_shell_faces(b: &Brep, s: ShellId) -> Result<Vec<FaceId>> {
    let shell = b.shell(s)?;
    if !shell.is_faceuse_shell() {
        return Err(SolidError::invalid_input(OP_TURN, "a shell of no faces"));
    }
    if shell.is_closed(b, s)? {
        return Err(SolidError::Unsupported {
            operation: OP_TURN,
            reason: "a closed shell: turned over, the body would be inside out",
        });
    }
    let mut seen = BTreeSet::new();
    let mut faces = Vec::new();
    for fu in shell.get_faceuses()? {
        if let Some(f) = b.faceuse(fu)?.face {
            if seen.insert(f) {
                faces.push(f);
            }
        }
    }
    Ok(faces)
}
