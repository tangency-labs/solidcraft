//! Reference triangles belong to a definition, even while its occurrences move. Keep one lazy
//! source mesh and map it into each pose; retain the exact placed B-rep separately in ModelBody.
use std::sync::{Arc, OnceLock};

use solidcraft_geom::tess::TessPlan;
use solidcraft_geom::{Mesh, Vec3};
use solidcraft_kernel::{self as kernel, Body};

use super::ModelBody;

#[derive(Debug)]
pub(super) struct PlacedMesh {
    body: Body,
    mesh: Arc<OnceLock<Arc<Mesh>>>,
    /// The definition's display plans and their meshes by display quality (its own cells):
    /// each placement draws them moved, so a part is planned once however often it is placed.
    plans: Arc<[OnceLock<Option<Arc<TessPlan>>>; 4]>,
    plan_meshes: Arc<[OnceLock<Option<Arc<Mesh>>>; 4]>,
    transform: crate::Mat,
    pub(super) tolerance: f64,
}

impl PlacedMesh {
    /// Share the original definition, never a chain of previously placed meshes. A source's
    /// OnceLock lives with its immutable geometry, so edits get a new source automatically.
    pub(super) fn new(source: &ModelBody, transform: &crate::Mat) -> Option<Self> {
        if !proper_rigid(transform) {
            return None;
        }
        Some(match source.mesh_placement.as_deref() {
            Some(original) => Self {
                body: original.body.clone(),
                mesh: original.mesh.clone(),
                plans: original.plans.clone(),
                plan_meshes: original.plan_meshes.clone(),
                transform: crate::mat_mul(transform, &original.transform),
                tolerance: original.tolerance,
            },
            None => Self {
                body: source.body.clone(),
                mesh: source.mesh.clone(),
                plans: source.plan.clone(),
                plan_meshes: source.plan_mesh.clone(),
                transform: *transform,
                tolerance: source.display_tol(),
            },
        })
    }

    pub(super) fn mesh(&self) -> Arc<Mesh> {
        Arc::new(self.moved(&reference_mesh(&self.body, &self.mesh, self.tolerance)))
    }

    /// The definition's display plan at quality `q` (made once, in its own cell), moved here.
    pub(super) fn plan(&self, q: crate::display_quality::DisplayQuality, name: &str) -> Option<Arc<TessPlan>> {
        let source = definition_plan(&self.body, self.plans.get(q.index())?, q, name)?;
        Some(Arc::new(source.transformed(&self.transform)))
    }

    /// The definition's display plan mesh at quality `q`, moved here.
    pub(super) fn plan_mesh(&self, q: crate::display_quality::DisplayQuality, name: &str) -> Option<Arc<Mesh>> {
        let (source, _) = self.source_plan_mesh(q, name)?;
        Some(Arc::new(self.moved(&source)))
    }

    /// The definition's reference mesh (shared by every placement of it) and this placement.
    pub(super) fn source_mesh(&self) -> (Arc<Mesh>, crate::Mat) {
        (reference_mesh(&self.body, &self.mesh, self.tolerance), self.transform)
    }

    /// The definition's display plan mesh at quality `q` (shared by every placement of it) and
    /// this placement.
    pub(super) fn source_plan_mesh(&self, q: crate::display_quality::DisplayQuality, name: &str) -> Option<(Arc<Mesh>, crate::Mat)> {
        let cell = self.plan_meshes.get(q.index())?;
        let source = cell
            .get_or_init(|| {
                let plan = definition_plan(&self.body, self.plans.get(q.index())?, q, name)?;
                self.body.mesh_of(&plan).ok().map(Arc::new)
            })
            .clone()?;
        Some((source, self.transform))
    }

    /// `source` (in the definition's place) moved by this placement.
    fn moved(&self, source: &Mesh) -> Mesh {
        Mesh {
            positions: source.positions.iter().map(|p| crate::apply_point(&self.transform, *p)).collect(),
            normals: source.normals.iter().map(|n| crate::apply_vector(&self.transform, *n)).collect(),
            edges: source.edges.iter().map(|edge| edge.iter().map(|p| crate::apply_point(&self.transform, *p)).collect()).collect(),
            display_only_edges: source.display_only_edges,
            triangles: source.triangles.clone(),
            tri_face: source.tri_face.clone(),
            edge_faces: source.edge_faces.clone(),
            seams: source.seams.clone(),
            smooth_edges: source.smooth_edges.clone(),
            sharp_edges: source.sharp_edges.clone(),
        }
    }
}

/// The display plan of a definition `body` at quality `q`, made once in `cell`.
pub(super) fn definition_plan(
    body: &Body,
    cell: &OnceLock<Option<Arc<TessPlan>>>,
    q: crate::display_quality::DisplayQuality,
    name: &str,
) -> Option<Arc<TessPlan>> {
    cell.get_or_init(|| match body.mesh_plan(&solidcraft_kernel::MeshTolerance { chord: q.chord(body.size()), angle: Some(q.angle()) }) {
        Ok(plan) => Some(Arc::new(plan)),
        Err(e) => {
            log::debug!("{name}: no display plan ({e})");
            None
        }
    })
    .clone()
}

pub(super) fn reference_mesh(body: &Body, mesh: &OnceLock<Arc<Mesh>>, tolerance: f64) -> Arc<Mesh> {
    mesh.get_or_init(|| {
        let mut mesh = body.mesh(&solidcraft_kernel::MeshTolerance { chord: tolerance, angle: Some(kernel::DISPLAY_ANGLE) }).unwrap_or_default();
        if let Some((positions, triangles)) = body.triangle_mesh() {
            mesh.edges = mesh_outlines(positions, triangles);
            mesh.display_only_edges = true;
        }
        Arc::new(mesh)
    })
    .clone()
}

/// Display lines for an imported triangle mesh: open boundaries, non-manifold sides and
/// creases of at least 30 degrees. The shared vertex indices come from the existing mesh body;
/// the geometry, winding and triangle topology are left to the importer.
fn mesh_outlines(positions: &[Vec3], triangles: &[[u32; 3]]) -> Vec<Vec<Vec3>> {
    let normal = |index: usize| {
        let t = triangles.get(index)?;
        let (a, b, c) = (positions.get(t[0] as usize)?, positions.get(t[1] as usize)?, positions.get(t[2] as usize)?);
        (*b - *a).cross(*c - *a).normalized()
    };
    // One compact record per side, sorted for deterministic output, instead of a map of
    // separately allocated adjacency lists. Mesh imports already cap the triangle count.
    let mut sides = Vec::with_capacity(triangles.len().saturating_mul(3));
    for (i, t) in triangles.iter().enumerate() {
        let Ok(triangle) = u32::try_from(i) else { continue };
        if normal(i).is_some() {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                sides.push((a.min(b), a.max(b), triangle));
            }
        }
    }
    sides.sort_unstable();
    let cos = 30f64.to_radians().cos();
    let mut edges = Vec::new();
    for side in sides.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)) {
        let Some(&(a, b, _)) = side.first() else { continue };
        let visible = match side {
            [(_, _, i), (_, _, j)] => normal(*i as usize).zip(normal(*j as usize)).is_some_and(|(n, m)| n.dot(m) <= cos),
            _ => true,
        };
        if visible && positions.get(a as usize).is_some() && positions.get(b as usize).is_some() {
            edges.push((a, b));
        }
    }
    straight_runs(positions, &edges)
}

/// The outline sides `sides` (vertex index pairs) as polylines: sides that run on straight
/// through a vertex no other outline side meets are one line. A mesh whose straight edges are
/// in pieces (a box exported at a grid's spacing) then has one outline per edge, as its CAD
/// model did, and a click on it measures the whole edge.
fn straight_runs(positions: &[Vec3], sides: &[(u32, u32)]) -> Vec<Vec<Vec3>> {
    let mut at: std::collections::BTreeMap<u32, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, &(a, b)) in sides.iter().enumerate() {
        at.entry(a).or_default().push(i);
        at.entry(b).or_default().push(i);
    }
    let point = |v: u32| positions.get(v as usize).copied().unwrap_or(Vec3::ZERO);
    let other = |i: usize, v: u32| sides.get(i).map_or(v, |&(a, b)| if a == v { b } else { a });
    // The side that continues `i` straight on through `v`, when `v` joins exactly two.
    let next = |i: usize, v: u32| -> Option<usize> {
        let [p, q] = at.get(&v)?.as_slice() else { return None };
        let j = if *p == i { *q } else { *p };
        let (into, out) = ((point(v) - point(other(i, v))).normalized()?, (point(other(j, v)) - point(v)).normalized()?);
        (into.dot(out) > 1.0 - 1e-9).then_some(j)
    };
    let mut used = vec![false; sides.len()];
    // Marks side `i` used; false when it already was.
    let mut take = |i: usize| used.get_mut(i).is_some_and(|u| !std::mem::replace(u, true));
    let mut runs = Vec::new();
    for (i, &(a, b)) in sides.iter().enumerate() {
        if !take(i) {
            continue;
        }
        let mut run = std::collections::VecDeque::from([a, b]);
        for (mut side, mut end, front) in [(i, b, false), (i, a, true)] {
            while let Some(j) = next(side, end).filter(|j| take(*j)) {
                end = other(j, end);
                side = j;
                if front {
                    run.push_front(end);
                } else {
                    run.push_back(end);
                }
            }
        }
        runs.push(run.into_iter().map(point).collect());
    }
    runs
}

/// Only proper rotations and translations preserve triangle winding, normal lengths and the
/// reference chord tolerance. Scale, shear, reflection and malformed matrices use the mesher.
fn proper_rigid(m: &crate::Mat) -> bool {
    if m.iter().flatten().any(|v| !v.is_finite()) || m[0][3] != 0. || m[1][3] != 0. || m[2][3] != 0. || m[3][3] != 1. {
        return false;
    }
    let [x, y, z] = [Vec3::new(m[0][0], m[0][1], m[0][2]), Vec3::new(m[1][0], m[1][1], m[1][2]), Vec3::new(m[2][0], m[2][1], m[2][2])];
    [x, y, z].iter().all(|v| (v.dot(*v) - 1.).abs() < 1e-9)
        && x.dot(y).abs() < 1e-9
        && x.dot(z).abs() < 1e-9
        && y.dot(z).abs() < 1e-9
        && (x.cross(y).dot(z) - 1.).abs() < 1e-9
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_outlines_keep_boundaries_and_creases_without_smooth_facet_lines() {
        // Flat (0 degrees), the sides from Y through the origin to -Y run straight on: one line.
        for (degrees, expected) in [(0.0f64, 3), (15.0, 4), (30.0, 5), (45.0, 5), (90.0, 5)] {
            let angle = degrees.to_radians();
            let p = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(0., -angle.cos(), angle.sin())];
            let triangles = [[0, 1, 2], [1, 0, 3]];
            let edges = mesh_outlines(&p, &triangles);
            assert_eq!(edges.len(), expected, "hinge angle {degrees}");
            assert_eq!(edges.iter().any(|e| e == &[Vec3::ZERO, Vec3::X]), degrees >= 30.);
            assert_eq!(edges, mesh_outlines(&p, &triangles), "deterministic order");
        }
        // A third triangle makes the common side non-manifold: keep it once, even if coplanar.
        let p = [Vec3::ZERO, Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Y * 2.];
        let edges = mesh_outlines(&p, &[[0, 1, 2], [1, 0, 3], [0, 1, 4]]);
        assert_eq!(edges.len(), 7);
        assert_eq!(edges.iter().filter(|e| **e == [Vec3::ZERO, Vec3::X]).count(), 1);
        // The rings of an open faceted cylinder stay visible, but its smooth wall has no
        // diagonals or longitudinal facet lines.
        let n = 36u32;
        let mut p = Vec::new();
        for z in [0., 10.] {
            p.extend((0..n).map(|i| {
                let a = f64::from(i) * std::f64::consts::TAU / f64::from(n);
                Vec3::new(a.cos(), a.sin(), z)
            }));
        }
        let triangles: Vec<_> = (0..n)
            .flat_map(|i| {
                let j = (i + 1) % n;
                [[i, j, j + n], [i, j + n, i + n]]
            })
            .collect();
        let edges = mesh_outlines(&p, &triangles);
        assert_eq!(edges.len(), 2 * n as usize);
        assert!(edges.iter().all(|e| e[0].z == e[1].z));
        // A straight edge in pieces (a node at its middle, where the facets of the face meet)
        // is one outline; its corners stay corners.
        let p = [Vec3::ZERO, Vec3::X * 2., Vec3::new(2., 1., 0.), Vec3::Y, Vec3::X];
        let edges = mesh_outlines(&p, &[[0, 4, 3], [4, 1, 2], [4, 2, 3]]);
        assert_eq!(edges.len(), 4, "{edges:?}");
        assert!(edges.iter().any(|e| e == &[Vec3::ZERO, Vec3::X, Vec3::X * 2.] || e == &[Vec3::X * 2., Vec3::X, Vec3::ZERO]), "{edges:?}");
        // Bad indices and zero-area facets do not invent outlines or panic.
        assert!(mesh_outlines(&[Vec3::ZERO, Vec3::X, Vec3::X * 2.], &[[0, 1, 99], [0, 1, 2]]).is_empty());
    }

    #[test]
    fn placed_mesh_is_lazy_and_composes_from_one_immutable_source() {
        let source = ModelBody::new("Part".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.)).unwrap(), 1);
        let a = crate::rigid(Vec3::new(10., 20., 30.), Vec3::ZERO, Vec3::Z, 0.7);
        let b = crate::rigid(Vec3::new(3., -5., 2.), Vec3::ZERO, Vec3::X, 0.4);
        let mut first = ModelBody::new("First".into(), kernel::transform_matrix(&source.body, a).unwrap(), 1);
        first.mesh_placement = Some(Arc::new(PlacedMesh::new(&source, &a).unwrap()));
        let mut nested = ModelBody::new("Nested".into(), kernel::transform_matrix(&first.body, b).unwrap(), 1);
        nested.mesh_placement = Some(Arc::new(PlacedMesh::new(&first, &b).unwrap()));
        assert_eq!(first.display_tol(), source.display_tol());
        assert_eq!(nested.display_tol(), source.display_tol());
        assert!(source.cached_mesh().is_none());
        assert!(first.cached_mesh().is_none());
        let mesh = nested.mesh();
        assert!(source.cached_mesh().is_some(), "the canonical source is warmed on demand");
        assert!(first.cached_mesh().is_none(), "composition does not retain or evaluate intermediate poses");
        let local = source.mesh();
        let matrix = crate::mat_mul(&b, &a);
        for (p, q) in local.positions.iter().zip(&mesh.positions) {
            assert!(crate::apply_point(&matrix, *p).dist(*q) < 1e-8);
        }
        let (other, work) = kernel::profile::capture(|| first.mesh());
        assert_eq!(work.tessellate_ms, 0.);
        assert_eq!(other.tri_face, local.tri_face);
        assert!(Arc::ptr_eq(&mesh, &nested.mesh()));
    }

    /// A placement hands out its definition's own meshes and its move without making the moved
    /// copy: every placement of a part gives the same mesh, and that mesh moved is the
    /// placement's own.
    #[test]
    fn placements_share_their_definitions_meshes_in_place() {
        use crate::display_quality::DisplayQuality;
        let source = ModelBody::new("Part".into(), kernel::cylinder(Vec3::ZERO, Vec3::Z, 2., 8.).unwrap(), 1);
        let placed: Vec<ModelBody> =
            [crate::rigid(Vec3::new(10., 20., 30.), Vec3::ZERO, Vec3::Z, 0.7), crate::rigid(Vec3::new(-4., 5., 1.), Vec3::ZERO, Vec3::X, 1.1)]
                .iter()
                .map(|m| {
                    let mut p = ModelBody::new("Copy".into(), kernel::transform_matrix(&source.body, *m).unwrap(), 1);
                    p.mesh_placement = Some(Arc::new(PlacedMesh::new(&source, m).unwrap()));
                    p
                })
                .collect();
        let (own, identity) = source.mesh_in_place();
        assert_eq!(identity, crate::IDENTITY);
        let q = DisplayQuality::parse("normal").unwrap();
        for p in &placed {
            let (mesh, m) = p.mesh_in_place();
            assert!(Arc::ptr_eq(&mesh, &own), "the definition's mesh, not a copy");
            let bounds = p.mesh_bounds();
            assert!(p.cached_mesh().is_none(), "no moved copy is made");
            let (plan_mesh, pm) = p.plan_mesh_in_place_at(q).unwrap();
            assert!(Arc::ptr_eq(&plan_mesh, &source.plan_mesh_at(q).unwrap()));
            assert_eq!(pm, m);
            let moved = p.mesh();
            let exact = moved.bounds();
            assert!(bounds.min.dist(exact.min) < 1e-9 && bounds.max.dist(exact.max) < 1e-9, "{bounds:?} vs {exact:?}");
            for (a, b) in mesh.positions.iter().zip(&moved.positions) {
                assert!(crate::apply_point(&m, *a).dist(*b) < 1e-9);
            }
            assert!(!Arc::ptr_eq(&p.display_identity(), &source.display_identity()), "each placement is its own body");
            assert!(Arc::ptr_eq(&p.display_identity(), &p.clone().display_identity()), "clones are the same body");
        }
    }

    /// Placed parts share their definition's display plan (#3044): the definition is planned
    /// once, every placement moves that plan, and a moved plan evaluates to the definition's
    /// points moved, on every kind of surface (planes, revolved and spline surfaces).
    #[test]
    fn placed_parts_are_planned_once_and_draw_the_moved_plan() {
        use crate::display_quality::DisplayQuality;
        let q = DisplayQuality::parse("normal").unwrap();
        for body in [
            kernel::box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.)).unwrap(),
            kernel::cylinder(Vec3::new(1., 0., 0.), Vec3::Z, 2., 8.).unwrap(),
            kernel::torus(Vec3::ZERO, 6., 1.5).unwrap(),
            kernel::sphere(Vec3::new(0., 1., 2.), 3.).unwrap(),
        ] {
            let source = ModelBody::new("Part".into(), body, 1);
            let a = crate::rigid(Vec3::new(10., 20., 30.), Vec3::ZERO, Vec3::Z, 0.7);
            let b = crate::rigid(Vec3::new(-4., 5., 1.), Vec3::new(1., 1., 0.), Vec3::X, 1.1);
            let placed: Vec<ModelBody> = [a, b]
                .iter()
                .map(|m| {
                    let mut p = ModelBody::new("Copy".into(), kernel::transform_matrix(&source.body, *m).unwrap(), 1);
                    p.mesh_placement = Some(Arc::new(PlacedMesh::new(&source, m).unwrap()));
                    p
                })
                .collect();
            assert!(source.plan.get(q.index()).unwrap().get().is_none());
            let plans: Vec<_> = placed.iter().map(|p| p.display_plan_at(q).unwrap()).collect();
            assert!(source.plan.get(q.index()).unwrap().get().is_some(), "placing plans the definition, in its own cell");
            let own = source.display_plan_at(q).unwrap();
            let local = own.evaluate();
            for (m, plan) in [a, b].iter().zip(&plans) {
                assert!(!Arc::ptr_eq(plan, &own));
                let moved = plan.evaluate();
                assert_eq!(moved.triangles, local.triangles);
                for (p, q) in local.positions.iter().zip(&moved.positions) {
                    assert!(crate::apply_point(m, *p).dist(*q) < 1e-9, "{p:?} -> {q:?}");
                }
                for (n, k) in local.normals.iter().zip(&moved.normals) {
                    assert!(crate::apply_vector(m, *n).dist(*k) < 1e-9);
                }
            }
            for (m, p) in [a, b].iter().zip(&placed) {
                let mesh = p.plan_mesh_at(q).unwrap();
                let base = source.plan_mesh_at(q).unwrap();
                assert_eq!(mesh.triangles, base.triangles);
                for (x, y) in base.positions.iter().zip(&mesh.positions) {
                    assert!(crate::apply_point(m, *x).dist(*y) < 1e-9);
                }
            }
        }
    }

    /// #3745: a highlight never waits for a body's plan mesh: the first ask starts it on a
    /// worker and answers `None`, and once made it is the plan mesh, in the body's own cell.
    #[test]
    fn plan_mesh_soon_makes_it_on_a_worker() {
        let body = ModelBody::new("Pin".into(), kernel::cylinder(Vec3::ZERO, Vec3::Z, 2., 8.).unwrap(), 1);
        assert!(body.plan_mesh_soon().is_none(), "not made in the asking frame");
        let began = std::time::Instant::now();
        let soon = loop {
            if let Some(mesh) = body.plan_mesh_soon() {
                break mesh;
            }
            assert!(began.elapsed() < std::time::Duration::from_secs(30), "the worker made it");
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        assert!(Arc::ptr_eq(&soon, &body.plan_mesh().unwrap()), "the same mesh, in the body's cell");
    }

    #[test]
    fn world_instances_warm_one_definition_and_an_edit_replaces_it() {
        let mut doc = crate::Document::new("instances");
        let component = doc.add_component(Some("Part"), 0).unwrap();
        doc.body_components.insert("Pin".into(), component);
        let first = doc.occurrences[0].id;
        doc.occurrences[0].transform = crate::rigid(Vec3::new(10., 0., 0.), Vec3::ZERO, Vec3::Y, 0.3);
        doc.add_occurrence(component, 0, crate::rigid(Vec3::new(0., 20., 0.), Vec3::ZERO, Vec3::X, 0.6)).unwrap();
        let mut state = crate::ModelState::default();
        state.bodies.push(ModelBody::new("Pin".into(), kernel::cylinder(Vec3::ZERO, Vec3::Z, 2., 8.).unwrap(), 1));
        let mut cache = super::super::PlacedBodies::default();
        let before = super::super::world_state_reusing(&doc, &state, &mut cache).unwrap();
        assert!(state.bodies[0].cached_mesh().is_none(), "placing bodies must remain lazy");
        let old = before.bodies[0].mesh();
        assert!(state.bodies[0].cached_mesh().is_some());
        let (_, work) = kernel::profile::capture(|| before.bodies[1].mesh());
        assert_eq!(work.tessellate_ms, 0., "the second instance shares the first one's triangulation");
        doc.occurrences.iter_mut().find(|o| o.id == first).unwrap().transform = crate::rigid(Vec3::new(30., 0., 0.), Vec3::ZERO, Vec3::Y, 0.9);
        let moved = super::super::world_state_reusing(&doc, &state, &mut cache).unwrap();
        let (_, work) = kernel::profile::capture(|| moved.bodies[0].mesh());
        assert_eq!(work.tessellate_ms, 0., "a new pose shares the same definition");
        state.bodies[0] = ModelBody::new("Pin".into(), kernel::cylinder(Vec3::ZERO, Vec3::Z, 4., 8.).unwrap(), 1);
        let edited = super::super::world_state_reusing(&doc, &state, &mut cache).unwrap();
        assert!(state.bodies[0].cached_mesh().is_none());
        let new = edited.bodies[0].mesh();
        assert!(state.bodies[0].cached_mesh().is_some(), "changed geometry warms its new definition cache");
        assert_ne!(new.bounds(), moved.bodies[0].mesh().bounds());
        assert!(Arc::ptr_eq(&old, &before.bodies[0].mesh()), "editing does not mutate older snapshots");
    }

    #[test]
    fn placements_taken_as_before_and_placed_again_keep_their_order() {
        let mut doc = crate::Document::new("instances");
        let component = doc.add_component(Some("Part"), 0).unwrap();
        doc.body_components.insert("Pin".into(), component);
        doc.occurrences[0].transform = crate::rigid(Vec3::new(10., 0., 0.), Vec3::ZERO, Vec3::Y, 0.3);
        let second = doc.add_occurrence(component, 0, crate::rigid(Vec3::new(0., 20., 0.), Vec3::ZERO, Vec3::X, 0.6)).unwrap();
        doc.add_occurrence(component, 0, crate::rigid(Vec3::new(0., -20., 5.), Vec3::ZERO, Vec3::Z, 1.1)).unwrap();
        let mut state = crate::ModelState::default();
        state.bodies.push(ModelBody::new("Pin".into(), kernel::cylinder(Vec3::ZERO, Vec3::Z, 2., 8.).unwrap(), 1));
        let mut cache = super::super::PlacedBodies::default();
        let before = super::super::world_state_reusing(&doc, &state, &mut cache).unwrap();
        let meshes: Vec<_> = before.bodies.iter().map(|b| b.mesh()).collect();
        // The middle placement moves: the other two are taken as they were, it is placed again.
        doc.occurrences.iter_mut().find(|o| o.id == second).unwrap().transform = crate::rigid(Vec3::new(0., 40., 0.), Vec3::ZERO, Vec3::X, 0.2);
        let after = super::super::world_state_reusing(&doc, &state, &mut cache).unwrap();
        let fresh = super::super::world_state_reusing(&doc, &state, &mut super::super::PlacedBodies::default()).unwrap();
        let names = |st: &crate::ModelState| st.bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&after), names(&fresh));
        assert_eq!(names(&after), names(&before));
        for (i, (a, f)) in after.bodies.iter().zip(&fresh.bodies).enumerate() {
            assert_eq!(a.mesh().bounds(), f.mesh().bounds(), "body {i} is where a fresh placement puts it");
        }
        assert!(Arc::ptr_eq(&after.bodies[0].mesh(), &meshes[0]) && Arc::ptr_eq(&after.bodies[2].mesh(), &meshes[2]));
        assert!(!Arc::ptr_eq(&after.bodies[1].mesh(), &meshes[1]), "the moved placement is placed again");
    }
    #[test]
    fn only_proper_rigid_affine_matrices_reuse_reference_meshes() {
        let source = ModelBody::new("Part".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.)).unwrap(), 1);
        assert!(PlacedMesh::new(&source, &crate::IDENTITY).is_some());
        for (col, row, value) in [(0, 0, -1.), (0, 0, 2.), (0, 1, 0.5), (0, 3, 0.1), (3, 3, 2.), (3, 0, f64::NAN), (3, 2, f64::INFINITY)] {
            let mut matrix = crate::IDENTITY;
            matrix[col][row] = value;
            assert!(PlacedMesh::new(&source, &matrix).is_none(), "{matrix:?}");
        }
        // A positive unit determinant alone is insufficient: this scale changes normals and tolerance.
        let mut scale = crate::IDENTITY;
        scale[0][0] = 2.;
        scale[1][1] = 0.5;
        assert!(PlacedMesh::new(&source, &scale).is_none());
    }
}
