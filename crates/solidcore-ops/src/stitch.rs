//! Sewing and merging: a body's faces sewn along the vertices and edges that coincide within a
//! tolerance, then oriented; and faces merged across the edges between them.
//!
//! A stitch is `SmBrep::StitchFaces` at the caller's tolerance, once, then
//! `SmBrep::OrientTrimmedSurfaces`: outward where the faces close into a solid, else only made to
//! agree across their shared edges. `StitchAndOrient` is not used: it caps the tolerance and
//! stitches a second time, so the caller's tolerance would not be the one that decides.
//!
//! `StitchFaces` glues an edge only to an edge one face uses: a sheet laid on edges two faces of
//! a solid already share (a cover over a box's top) is left beside the solid. Edges still lamina
//! after that pass are glued at the same tolerance to the edges they lie on whatever uses those
//! (`bAllowSpineEdges`), on a copy ([`close_onto_shared_edges`]), and the faces that gluing put
//! inside the material are removed. A face that was between two material regions before the
//! gluing (a partition the body came with) stays, and so do the edges of more than two faces
//! the body came with.
//!
//! **Closing a space.** Any space the gluing closes off from the outside becomes material: a cap
//! grows the solid, a lid fills a pocket. The kernel decides which closed spaces are material
//! (`StitchFaces` sets the void flags by nesting, alternating from the infinite region), and a
//! space reached from the outside through the sheet is material; the faces left between it and
//! the solid (the cube's top under a cap, a pocket's walls and floor under a lid) become
//! partitions the gluing made and are removed. The other reading, an enclosed cavity, would
//! leave the sheet void on both sides, a sheet face and not a solid. A space closed only
//! through material (a sheet inside a cavity) comes out void by the same parity: no new
//! material, so the manifold stitch's result is kept, reported not closed.
//!
//! Each takes its body by value and gives the result: an application that keeps its input copies
//! it first ([`Body::copy`]). While a failure callback is installed, the body is written before
//! the call, so a failure reports it as it came in.

use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::region_material;
use solidcore::topology::brep::p1 as bl;
use solidcore::topology::brep::p2::{is_manifold_solid, ArenaNotifyState};
use solidcore::topology::brep::p3::{
    brep_copy, delete_face, orient_trimmed_surfaces, with_kernel_callbacks, DeleteFaceOptions,
};
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::topology::brep::p5::remove_topological_edges_and_vertices;
use solidcore::topology::face::p2::{self as face_p2, face_side_regions};
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{edge, Brep, EdgeId, FaceId, FaceuseId, RegionId};

use crate::body::{finish, Body, Expect};
use crate::error::status;
use crate::order::{self, Order};
use crate::session::run_taking;
use crate::{Error, Result};

type Out<T> = std::result::Result<T, Error>;

/// `body`'s faces sewn along the vertices and edges that coincide within `tolerance` (finite,
/// positive), then oriented; with whether the result is a closed solid (no lamina edge left and
/// a manifold solid).
///
/// # Errors
/// [`Error::Argument`] for a tolerance that is not finite and positive; [`Error::Unsupported`]
/// where gluing the lamina edges onto shared edges would open the solid or leave a new edge
/// with more than two faces ([`close_onto_shared_edges`]); [`Error::UnsureRegion`] for a region
/// the kernel is unsure is material, where the second pass judges the gluing; the kernel's
/// status.
// The body is consumed: its B-rep is taken out under the kernel's lock.
#[allow(clippy::needless_pass_by_value)]
pub fn stitch(body: Body, tolerance: f64) -> Result<(Body, bool)> {
    if !tolerance.is_finite() || tolerance.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return Err(Error::Argument);
    }
    if body.is_poisoned() {
        return Err(Error::Internal(
            "SmBrep::StitchFaces: the body was left unfinished by an earlier panic".into(),
        ));
    }
    run_taking("SmBrep::StitchFaces", &[("body", &body)], |k| {
        let mut b = std::mem::replace(&mut *body.lock(), Brep::new());
        let mut lamina = match stitch_faces(k, &mut b, tolerance, None, false) {
            Ok(o) => o.lamina_edges,
            Err(e) => return Ok(Err(status(e))),
        };
        if lamina != 0 {
            b = match close_onto_shared_edges(k, b, tolerance, &mut lamina)? {
                Ok(b) => b,
                Err(e) => return Ok(Err(e)),
            };
        }
        let oriented = with_kernel_callbacks(k, &mut b, |k, b, env| {
            let mut marks = Marks::default();
            orient_trimmed_surfaces(k, b, env, &mut marks, true, lamina != 0, None)
        });
        if let Err(e) = oriented {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        let closed = lamina == 0 && is_manifold_solid(&b)?;
        Ok(Ok((Body::new(b), closed)))
    })?
}

/// Whether region `r` (a side's, `None` for a side with no region: void) is material
/// (`region_material`: a region the kernel is unsure of is refused).
fn material(b: &Brep, r: Option<RegionId>) -> solidcore::Result<bool> {
    r.map_or(Ok(false), |r| region_material(b, r))
}

/// The faces of `b` lying between two material regions.
fn faces_between_material(b: &Brep) -> solidcore::Result<Vec<FaceId>> {
    let mut inside = Vec::new();
    for f in bl::faces(b, None)? {
        if let [Some(r1), Some(r2)] = face_side_regions(b, f)? {
            if r1 != r2 && material(b, Some(r1))? && material(b, Some(r2))? {
                inside.push(f);
            }
        }
    }
    Ok(inside)
}

/// Each face use of `b` with whether the region on its side is material: a material region
/// is the union of the sides of the faces that bound it.
fn material_sides(b: &Brep) -> solidcore::Result<Vec<(FaceuseId, bool)>> {
    let mut sides = Vec::new();
    for f in bl::faces(b, None)? {
        let (x, y) = face_p2::faceuses(b, f)?;
        let [rx, ry] = face_side_regions(b, f)?;
        for (u, r) in [(x, rx), (y, ry)] {
            if let Some(u) = u {
                sides.push((u, material(b, r)?));
            }
        }
    }
    Ok(sides)
}

/// What a gluing did to the material, side by side against `before` ([`material_sides`]):
/// whether a side that was material is not now (a material region opened to the outside, or
/// a face use the gluing did not keep), and whether a side that was not material is now (a
/// space closed into a material region).
fn material_change(before: &[(FaceuseId, bool)], after: &Brep) -> solidcore::Result<(bool, bool)> {
    let now: Vec<(FaceuseId, bool)> = material_sides(after)?;
    let (mut opened, mut closed) = (false, false);
    for &(u, was) in before {
        match now.iter().find(|(v, _)| *v == u) {
            Some(&(_, is)) => {
                opened |= was && !is;
                closed |= !was && is;
            }
            None => opened |= was,
        }
    }
    Ok((opened, closed))
}

/// The edges of `b` with more than two faces (`edge::is_spine`).
fn spine_edges(b: &Brep) -> solidcore::Result<Vec<EdgeId>> {
    let mut spines = Vec::new();
    for e in bl::edges(b, None)? {
        if edge::is_spine(b, e)? {
            spines.push(e);
        }
    }
    Ok(spines)
}

/// The faces `inside` of `b` are inside the part and removed (`DeleteFace(f, TRUE, TRUE)` with
/// the B-rep open for editing): their regions are joined, and their edges and vertices nothing
/// else uses go with them.
fn remove_faces(k: &mut Kernel, b: &mut Brep, inside: Vec<FaceId>) -> Out<()> {
    let editing = b.editing_enabled();
    b.set_editing_enabled(true);
    let mut deleted = Ok(());
    {
        let mut state = ArenaNotifyState::new(b);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        for f in inside {
            let options = DeleteFaceOptions {
                delete_edges_vertices: true,
                do_region_nesting: true,
                ..DeleteFaceOptions::default()
            };
            if let Err(e) = delete_face(k, b, &mut env, &mut marks, f, options) {
                deleted = Err(e);
                break;
            }
        }
    }
    b.set_editing_enabled(editing);
    b.refresh_cached_topology();
    deleted.map_err(status)
}

/// `b`, stitched as a manifold with `lamina` edges left free, those glued at `tolerance` to the
/// edges they lie on whatever uses those (`StitchFaces` with `bAllowSpineEdges`), on a copy,
/// judged side by side: each face use's side material or not, before the gluing and after
/// ([`material_change`]):
/// - nothing glued: `b` as it is;
/// - a side that was material is not after (the gluing opens a material region to the
///   outside: a sheet with no sector between the faces at its rim), whatever else it closed:
///   refused, [`Error::Unsupported`];
/// - no side that was not material is after (the sheet bounds nothing with the solid's
///   faces): `b` as the manifold stitch left it, its sheet's edges lamina, so the stitch reports
///   it not closed. Gluing onto shared edges is for closing a space; this is the contract, not a
///   fallback: nothing of the gluing is kept that the result does not say;
/// - otherwise the faces the gluing put between two material regions (those that were not
///   between two before it: a partition the body came with stays) are removed; an edge with
///   more than two faces that the body did not have before (one it came with, where a
///   partition meets the boundary, stays) is refused, [`Error::Unsupported`]; else the copy
///   is the result, with its count of lamina edges.
fn close_onto_shared_edges(
    k: &mut Kernel,
    b: Brep,
    tolerance: f64,
    lamina: &mut u32,
) -> solidcore::Result<Out<Brep>> {
    let mut c = brep_copy(k, &b)?;
    let sides = material_sides(&c)?;
    let partitions = faces_between_material(&c)?;
    let spines = spine_edges(&c)?;
    let outcome = match stitch_faces(k, &mut c, tolerance, None, true) {
        Ok(o) => o,
        Err(e) => return Ok(Err(status(e))),
    };
    if outcome.stitched_edges == 0 {
        return Ok(Ok(b));
    }
    let (opened, closed) = material_change(&sides, &c)?;
    if opened {
        return Ok(Err(Error::Unsupported(
            "gluing a sheet onto shared edges would open the solid",
        )));
    }
    if !closed {
        return Ok(Ok(b));
    }
    let inside = faces_between_material(&c)?
        .into_iter()
        .filter(|f| !partitions.contains(f))
        .collect();
    if let Err(e) = remove_faces(k, &mut c, inside) {
        return Ok(Err(e));
    }
    if spine_edges(&c)?.iter().any(|e| !spines.contains(e)) {
        return Ok(Err(Error::Unsupported(
            "gluing a sheet onto shared edges leaves a new edge of more than two faces",
        )));
    }
    *lamina = outcome.lamina_edges;
    Ok(Ok(c))
}

/// `body`'s faces merged across their edges (`SmBrep::RemoveTopologicalEdgesAndVertices`),
/// keeping record edges `keep` (`order`; none: the kernel decides every edge); then the result
/// contract: faces left, and a solid in gives a solid.
///
/// # Errors
/// [`Error::Argument`] for an index past the edges; [`Error::Empty`], [`Error::NotSolid`]; the
/// kernel's status.
// The body is consumed: its B-rep is taken out under the kernel's lock.
#[allow(clippy::needless_pass_by_value)]
pub fn merge_faces(body: Body, order: &Order, keep: &[usize]) -> Result<Body> {
    let kept = keep
        .iter()
        .map(|&i| order::edge(order, i))
        .collect::<Result<Vec<_>>>()?;
    if body.is_poisoned() {
        return Err(Error::Internal(
            "SmBrep::RemoveTopologicalEdgesAndVertices: the body was left unfinished by an earlier panic"
                .into(),
        ));
    }
    run_taking(
        "SmBrep::RemoveTopologicalEdgesAndVertices",
        &[("body", &body)],
        |k| {
            let mut b = std::mem::replace(&mut *body.lock(), Brep::new());
            let expect = Expect::of(k, &mut b)?;
            let keep_edges = (!kept.is_empty()).then_some(kept.as_slice());
            if let Err(e) = remove_topological_edges_and_vertices(k, &mut b, keep_edges, None) {
                return Ok(Err(status(e)));
            }
            b.refresh_cached_topology();
            Ok(finish(k, b, expect)?.map(Body::new))
        },
    )?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boolean::{boolean, BooleanOp};
    use crate::measure::tests::kernel_order;
    use crate::{make_box, session};
    use solidcore::blend::fillet_executive::api::create_face_from_surface;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::primitive::api_primitives::create_box;
    use solidcore::surface::plane::Plane;
    use solidcore::surface::surface::Surface;
    use solidcore::{Point3, Vec2};

    fn all_fixes() {
        session::set_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    }

    /// The cap of the box [0, 10]^2 x [10, 15], its bottom open: five planar faces, each its own
    /// face with its own edges, added to `b`.
    fn add_cap(k: &mut Kernel, b: &mut Brep) {
        type Side = ([f64; 3], [f64; 3], [f64; 3], [f64; 2]);
        let sides: [Side; 5] = [
            (
                [0.0, 0.0, 10.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [10.0, 5.0],
            ),
            (
                [0.0, 10.0, 10.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [10.0, 5.0],
            ),
            (
                [0.0, 0.0, 10.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [10.0, 5.0],
            ),
            (
                [10.0, 0.0, 10.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [10.0, 5.0],
            ),
            (
                [0.0, 0.0, 15.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [10.0, 10.0],
            ),
        ];
        for (o, u, v, [du, dv]) in sides {
            let p = Plane::new(
                Point3::new(o[0], o[1], o[2]),
                solidcore::Vec3::new(u[0], u[1], u[2]),
                solidcore::Vec3::new(v[0], v[1], v[2]),
                Vec2::new(1.0, 1.0),
                &Extent2d::from_bounds(0.0, 0.0, du, dv),
            );
            create_face_from_surface(k, b, &Surface::from(p)).unwrap();
        }
        b.refresh_cached_topology();
    }

    /// What a 10 mm cube is merged with before the cap goes on.
    #[derive(Clone, Copy)]
    enum Beside {
        Nothing,
        /// The cube [3, 7]^3 inside it, a second material region.
        Inner,
        /// The cube [10, 20] x [0, 10]^2 beside it: their shared face is a partition, whose
        /// rim edges at x = 10 have three faces each.
        Next,
    }

    /// A 10 mm cube, merged with `beside`, and the cap of [0, 10]^2 x [10, 15] laid on the
    /// cube's top edges.
    fn capped(beside: Beside) -> Body {
        session::run("capped", |k| {
            let mut b = create_box(k, Point3::ZERO, 10.0, 10.0, 10.0)?;
            let other = match beside {
                Beside::Nothing => None,
                Beside::Inner => Some(create_box(k, Point3::new(3.0, 3.0, 3.0), 4.0, 4.0, 4.0)?),
                Beside::Next => Some(create_box(
                    k,
                    Point3::new(10.0, 0.0, 0.0),
                    10.0,
                    10.0,
                    10.0,
                )?),
            };
            if let Some(c) = other {
                b = solidcore::boolean::api_brep::boolean_merge(k, b, c)?;
            }
            add_cap(k, &mut b);
            Ok(b)
        })
        .map(Body::new)
        .unwrap()
    }

    fn partitions(b: &Body) -> usize {
        faces_between_material(&b.lock()).unwrap().len()
    }

    /// The cap's rim lies on the cube's top edges, which two faces already share: the second
    /// pass glues it there, closing the space under it, and the cube's top, now inside, goes: a
    /// 10 x 10 x 15 box.
    #[test]
    fn a_cap_on_a_cube_s_top_edges_closes_into_one_box() {
        all_fixes();
        let b = capped(Beside::Nothing);
        assert_eq!(b.counts().unwrap()[0], 11);
        let (s, closed) = stitch(b, 1e-6).unwrap();
        assert!(closed);
        assert_eq!(s.counts().unwrap(), [10, 20, 12]);
        assert!((s.volume(1e-8).unwrap() - 1500.0).abs() < 1e-6);
    }

    /// A partition the body came with (the inner cube's six faces, between it and the rest of
    /// the cube, both material) survives the second pass; only the face the gluing put inside
    /// (the cube's top) goes. Before: 6 + 6 + 5 faces, 6 of them partitions; after: 16, the same
    /// 6 partitions.
    #[test]
    fn a_partition_the_body_came_with_survives_the_stitch() {
        all_fixes();
        let b = capped(Beside::Inner);
        assert_eq!(b.counts().unwrap()[0], 17);
        assert_eq!(partitions(&b), 6);
        let (s, _) = stitch(b, 1e-6).unwrap();
        assert_eq!(s.counts().unwrap()[0], 16, "{:?}", s.counts());
        assert_eq!(partitions(&s), 6);
    }

    /// A lid laid on a blind pocket's rim (the 10 mm cube less [3, 7]^2 x [5, 10], 920 mm^3)
    /// closes the pocket off from the outside: it becomes material, its walls and floor go,
    /// and the lid is part of the top: one closed 1000 mm^3 box.
    #[test]
    fn a_lid_on_a_pocket_s_rim_fills_the_pocket() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        let t = make_box([3.0, 3.0, 5.0], [4.0, 4.0, 6.0]).unwrap();
        let b = boolean(a, t, BooleanOp::Subtract).unwrap().unwrap();
        assert!((b.volume(1e-8).unwrap() - 920.0).abs() < 1e-6);
        assert_eq!(b.counts().unwrap()[0], 11);
        session::run("lid", |k| {
            let mut g = b.lock();
            let p = Plane::new(
                Point3::new(3.0, 3.0, 10.0),
                solidcore::Vec3::new(1.0, 0.0, 0.0),
                solidcore::Vec3::new(0.0, 1.0, 0.0),
                Vec2::new(1.0, 1.0),
                &Extent2d::from_bounds(0.0, 0.0, 4.0, 4.0),
            );
            create_face_from_surface(k, &mut g, &Surface::from(p))?;
            g.refresh_cached_topology();
            Ok(())
        })
        .unwrap();
        let (s, closed) = stitch(b, 1e-6).unwrap();
        assert!(closed);
        assert_eq!(s.counts().unwrap(), [7, 16, 12]);
        assert!((s.volume(1e-8).unwrap() - 1000.0).abs() < 1e-6);
    }

    /// Edges of more than two faces the body came with (where the partition between two
    /// merged cubes meets their boundary) are not the gluing's: the cap over the first cube
    /// is glued, the first cube's top goes, the partition stays. Before: 11 + 5 faces, one
    /// partition, four edges of three faces; after: 15 faces, the same partition.
    #[test]
    fn edges_of_three_faces_the_body_came_with_do_not_refuse_the_stitch() {
        all_fixes();
        let b = capped(Beside::Next);
        assert_eq!(b.counts().unwrap()[0], 16);
        assert_eq!(partitions(&b), 1);
        assert_eq!(spine_edges(&b.lock()).unwrap().len(), 4);
        let (s, _) = stitch(b, 1e-6).unwrap();
        assert_eq!(s.counts().unwrap()[0], 15, "{:?}", s.counts());
        assert_eq!(partitions(&s), 1);
        assert!((s.volume(1e-8).unwrap() - 2500.0).abs() < 1e-6);
    }

    /// The gluing is judged side by side: a material side that is not material after it is
    /// an opened solid, whatever new material it closed (the region counts can still rise).
    /// Here the first cube's region is made void after a gluing that closed the cap's space:
    /// there are as many material regions as before, but the cube's sides were opened.
    #[test]
    fn a_material_side_lost_is_an_opened_solid_whatever_the_counts() {
        all_fixes();
        let b = capped(Beside::Inner);
        session::run("judge", |k| {
            let mut c = brep_copy(k, &b.lock())?;
            let before = material_sides(&c)?;
            stitch_faces(k, &mut c, 1e-6, None, true)?;
            assert_eq!(material_change(&before, &c)?, (false, true));
            // the outer cube's region (not the inner cube's, not the cap's) made void: the
            // region beside its face at x = 0
            let infinite = c.infinite_region();
            let side = bl::faces(&c, None)?
                .into_iter()
                .find(|&f| {
                    solidcore::query::api_queries::face_bounding_box(k, &c, f, true)
                        .is_ok_and(|x| x.max().x < 1e-9)
                })
                .unwrap();
            let outer = face_side_regions(&c, side)?
                .into_iter()
                .flatten()
                .find(|&r| Some(r) != infinite)
                .unwrap();
            c.region_mut(outer)?.set_is_void(1);
            assert_eq!(material_change(&before, &c)?, (true, true));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn two_boxes_side_by_side_merge_into_one_box() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let b = make_box([1.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
        let order = kernel_order(&u);
        let m = merge_faces(u, &order, &[]).unwrap();
        assert_eq!(m.counts().unwrap(), [6, 12, 8]);
        assert!((m.volume(1e-8).unwrap() - 2.0).abs() < 1e-9);
        let c = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let order = kernel_order(&c);
        assert_eq!(merge_faces(c, &order, &[99]).unwrap_err(), Error::Argument);
    }

    /// Kept edges stay: a 2 mm cube imprinted across x = 1 has its four side faces split
    /// there (10 faces); merged keeping the four edges at x = 1 it stays so, merged keeping
    /// none it is the cube again.
    #[test]
    fn merge_faces_keeps_the_record_edges_it_is_given() {
        all_fixes();
        let cut = || {
            let body = make_box([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap();
            let tool = make_box([1.0, -1.0, -1.0], [4.0, 4.0, 4.0]).unwrap();
            crate::boolean::imprint(body, tool).unwrap().0
        };
        let u = cut();
        assert_eq!(u.counts().unwrap()[0], 10);
        let order = kernel_order(&u);
        let across: Vec<usize> = {
            let k = u.lock();
            order
                .edges
                .iter()
                .enumerate()
                .filter(|&(_, &e)| {
                    let x = edge::vertices(&k, e, None).unwrap();
                    x.len() == 2
                        && x.iter().all(|v| {
                            v.is_some_and(|v| (k.vertex(v).unwrap().point.x - 1.0).abs() < 1e-12)
                        })
                })
                .map(|(i, _)| i)
                .collect()
        };
        assert_eq!(across.len(), 4, "the cut's four edges");
        let kept = merge_faces(u, &order, &across).unwrap();
        assert_eq!(kept.counts().unwrap(), [10, 20, 12]);
        let u = cut();
        let order = kernel_order(&u);
        assert_eq!(
            merge_faces(u, &order, &[]).unwrap().counts().unwrap(),
            [6, 12, 8]
        );
    }
}
