//! What an evaluation costs after an edit at the end of a large design: the features before
//! the edit are reused, so the edit's cost must not grow with the bodies and imported files
//! the design already holds. A sketch on a fixed plane once moved every body of every placed
//! component into its frame, and every evaluation wrote every imported STEP file out again to
//! fingerprint it, so each sketch command took longer the more parts were inserted.

use solidcraft_geom::prelude::*;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_kernel as kernel;
use solidcraft_sketch::Sketch;

use crate::eval::{PlacementReads, body_world, hash_placement, placement_digest};
use crate::*;

/// Work counted on this thread: bodies moved, STEP/mesh contents digested, joint faces and bodies
/// measured, bodies searched for by name, features searched for by id, bodies a cut (or
/// intersect) ran its booleans on, bodies searched for the cylinder a tap marking lies on,
/// bodies whose faces a tapped hole read for its wall, intersects run to find a pattern's copies
/// that cut nothing, the tools a cut of many tools is made with (one when they are cut in one
/// go), unions run to tell the tools of a cut apart, face surfaces classified for a hole's
/// direction, joins made of bodies that are all apart without running the kernel's union, the
/// holes whose tools were built (not moved from another copy's), the imported files read for an
/// import feature, the tight boxes the kernel was asked for, the flats of sheets made from
/// surfaces laid out (their pieces cut), the blends the kernel was asked for, the searches for
/// the radius that fits, the meshes a cam's profile was read from, and the kernel's point
/// classifications a pattern's copies were probed with, and the bodies a hole's points were
/// classified in.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Counts {
    pub moved: usize,
    pub digested: usize,
    pub imports_read: usize,
    pub cam_meshes: usize,
    pub mesh_digested: usize,
    pub joint_faces: usize,
    pub faces_at: usize,
    pub measures: usize,
    pub body_searches: usize,
    pub feature_searches: usize,
    pub cut: usize,
    pub cylinder_search: usize,
    pub tapped_wall: usize,
    pub miss_intersects: usize,
    pub cut_tools: usize,
    pub apart_unions: usize,
    pub surfaces: usize,
    pub flats: usize,
    pub joined_apart: usize,
    pub kept_checks: usize,
    pub detached_checks: usize,
    pub reused_checks: usize,
    pub thread_cuts: usize,
    pub grouped_thread_fallbacks: usize,
    pub hole_tools: usize,
    pub tight_boxes: usize,
    pub blends: usize,
    pub fit_searches: usize,
    pub classifications: usize,
    pub hole_points: usize,
    /// Contact tests a join made to find which bodies its tool meets.
    pub contact_tests: usize,
    pub joined_hole_tools: usize,
}

thread_local! {
    static COUNTS: Cell<Counts> = const {
        Cell::new(Counts {
            moved: 0,
            digested: 0,
            imports_read: 0,
            cam_meshes: 0,
            mesh_digested: 0,
            joint_faces: 0,
            faces_at: 0,
            measures: 0,
            body_searches: 0,
            feature_searches: 0,
            cut: 0,
            cylinder_search: 0,
            tapped_wall: 0,
            miss_intersects: 0,
            cut_tools: 0,
            apart_unions: 0,
            surfaces: 0,
            flats: 0,
            joined_apart: 0,
            kept_checks: 0,
            detached_checks: 0,
            reused_checks: 0,
            thread_cuts: 0,
            grouped_thread_fallbacks: 0,
            hole_tools: 0,
            tight_boxes: 0,
            blends: 0,
            fit_searches: 0,
            classifications: 0,
            hole_points: 0,
            contact_tests: 0,
            joined_hole_tools: 0,
        })
    };
}

pub(crate) fn count(f: impl FnOnce(&mut Counts)) {
    COUNTS.with(|c| {
        let mut n = c.get();
        f(&mut n);
        c.set(n);
    });
}

thread_local! {
    static WRITTEN: Cell<usize> = const { Cell::new(0) };
}

/// A feature written out on this thread to be fingerprinted.
pub(super) fn count_written() {
    WRITTEN.with(|c| c.set(c.get() + 1));
}

/// The features `f` writes out on this thread to fingerprint them.
fn written(f: impl FnOnce()) -> usize {
    WRITTEN.with(|c| c.set(0));
    f();
    WRITTEN.with(Cell::get)
}

/// A search on this thread through all of a design's features for one.
pub(crate) fn count_feature_search() {
    count(|c| c.feature_searches += 1);
}

/// The flat of a sheet made from surfaces laid out on this thread: its pieces cut.
pub(crate) fn count_flat() {
    count(|c| c.flats += 1);
}

/// The searches `f` makes on this thread through all features or all bodies.
fn searched(f: impl FnOnce()) -> usize {
    let n = counted(f);
    n.feature_searches + n.body_searches
}

/// The work `f` does on this thread.
pub(crate) fn counted(f: impl FnOnce()) -> Counts {
    COUNTS.with(|c| c.set(Counts::default()));
    f();
    COUNTS.with(Cell::get)
}

#[test]
fn joint_face_measurements_are_shared_across_motion_samples_and_snap_kinds() {
    use crate::joints::{Joint, JointKind, JointOrigin, Snap, XRef};
    let mut doc = Document::new("joint face work");
    let part = doc.add_component(Some("Crank"), 0).unwrap();
    let occurrence = doc.occurrences.iter_mut().find(|o| o.component == part).unwrap();
    occurrence.grounded = false;
    let occurrence = occurrence.id;
    let id = doc
        .add_feature(
            FeatureKind::Cylinder {
                base: point_expr(Vec3::ZERO),
                axis: crate::dir_expr(Vec3::Z),
                radius: "2".into(),
                height: "10".into(),
                operation: Operation::NewBody,
                references: Vec::new(),
            },
            None,
        )
        .unwrap();
    doc.feature_mut(id).unwrap().component = part;
    let mut model = Model::new();
    model.evaluate(&doc);
    let state = model.state();
    let pick = Vec3::new(0.0, 0.0, 10.0);
    let face = joints::face_name_at(&doc, &state, part, pick, false).unwrap();
    doc.assembly.joints.push(Joint {
        id: 100,
        name: "Pivot".into(),
        kind: JointKind::Revolute,
        a: JointOrigin::new(0, Snap::Frame { origin: pick, z: Vec3::Z, x: Some(Vec3::X) }),
        b: JointOrigin::new(occurrence, Snap::FaceCenter { pick, face: None, x: Some(XRef::Dir(Vec3::X)) }),
        values: vec![0.0],
        limits: Vec::new(),
        flip: true,
        offset: 0.0,
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
        offset_xy: [0.0; 2],
        captured: None,
        constraint: None,
        dynamics: Vec::new(),
    });
    // One body supplies both old nearest-face snaps and persistent named-face snaps.
    // A scratch model shares its measurements, even though each sample changes the pose.
    let snapshot = model.clone();
    let work = counted(|| {
        for name in [None, Some(face)] {
            doc.assembly.joints[0].b.snap = Snap::FaceCenter { pick, face: name, x: Some(XRef::Dir(Vec3::X)) };
            for k in 0..145 {
                let angle = (k as f64 * 2.5).to_radians();
                doc.assembly.joints[0].values[0] = angle;
                let solution = joints::solve(&doc, &snapshot.state());
                assert!(solution.errors.is_empty() && solution.conflicts.is_empty(), "{solution:?}");
                let point = apply_point(&solution.world[&occurrence], Vec3::new(5.0, 0.0, 10.0));
                assert!(point.dist(Vec3::new(5.0 * angle.cos(), 5.0 * angle.sin(), 10.0)) < 1e-9);
                doc.apply_joint_solution(&solution);
            }
        }
    });
    assert_eq!(work.joint_faces, 1, "pose-only samples must not remeasure unchanged faces: {work:?}");

    // Editing geometry creates a fresh measurement without changing a retained snapshot.
    let FeatureKind::Cylinder { height, .. } = &mut doc.feature_mut(id).unwrap().kind else { panic!("cylinder") };
    *height = "12".into();
    model.evaluate(&doc);
    let origin = &doc.assembly.joints[0].b;
    let work = counted(|| {
        let changed = joints::resolve_origin(&doc, &model.state(), origin).unwrap();
        let retained = joints::resolve_origin(&doc, &snapshot.state(), origin).unwrap();
        assert!((changed[3][2] - 12.0).abs() < 1e-9);
        assert!((retained[3][2] - 10.0).abs() < 1e-9);
    });
    assert_eq!(work.joint_faces, 1, "only the edited body must be measured: {work:?}");
}

#[test]
fn joint_face_measurements_never_follow_a_replaced_body_or_stale_offset() {
    use crate::joints::{JointOrigin, Snap};
    let mut doc = Document::new("replaced joint geometry");
    let mut state = ModelState::default();
    state.bodies.push(ModelBody::new("Pin".into(), kernel::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 10.0).unwrap(), 0));
    let origin = JointOrigin::new(0, Snap::FaceCenter { pick: Vec3::new(0.0, 0.0, 10.0), face: None, x: None });
    let work = counted(|| {
        joints::resolve_origin(&doc, &state, &origin).unwrap();
    });
    assert_eq!(work.joint_faces, 1);
    let retained = state.clone();
    // Body is public; a clone's old measurement must never mask a direct replacement.
    state.bodies[0].body = kernel::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 12.0).unwrap();
    let changed = joints::resolve_origin(&doc, &state, &origin).unwrap();
    assert!((changed[3][2] - 12.0).abs() < 1e-9);
    let work = counted(|| {
        let old = joints::resolve_origin(&doc, &retained, &origin).unwrap();
        assert!((old[3][2] - 10.0).abs() < 1e-9);
        doc.body_offsets.insert("Pin".into(), rigid(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, Vec3::Z, 0.0));
        let moved = joints::resolve_origin(&doc, &retained, &origin).unwrap();
        assert!((moved[3][2] - 11.0).abs() < 1e-9);
    });
    assert_eq!(work.joint_faces, 0, "offsets are applied after the shared face measurements");
}

/// Every solve of a drag resolves its edge origins again: they share the joint faces' one
/// measurement, as a face origin does, instead of tessellating the whole body each time.
#[test]
fn an_edge_origin_measures_its_faces_once() {
    use crate::joints::{JointOrigin, Snap};
    let doc = Document::new("edge origin");
    let mut state = ModelState::default();
    state.bodies.push(ModelBody::new("Block".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap(), 0));
    let pick = Vec3::new(5.0, 0.0, 10.0);
    let origin = JointOrigin::new(0, Snap::Edge { pick, edge: None, face: None, corner: None, axis: None });
    let work = counted(|| {
        for _ in 0..8 {
            let frame = joints::resolve_origin(&doc, &state.clone(), &origin).unwrap();
            assert!(apply_point(&frame, Vec3::ZERO).dist(pick) < 1e-9);
        }
    });
    assert_eq!(work.joint_faces, 1, "a solve must not measure the faces again: {work:?}");
}

/// The summed area of `body`'s faces as [`ModelBody::faces_at`] measures them at `tol`.
fn measured_area(body: &ModelBody, tol: f64) -> f64 {
    body.faces_at(tol).unwrap().iter().map(|f| f.area).sum()
}

/// The viewport asks which faces a feature changed at every rebuild of its highlight: a body's
/// faces are measured once per geometry and tolerance, snapshots share the measurement, and a
/// replaced body never answers with the old one.
#[test]
fn face_measurements_are_made_once_per_body_and_tolerance() {
    let mut state = ModelState::default();
    state.bodies.push(ModelBody::new("Block".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap(), 0));
    let snapshot = state.clone();
    let work = counted(|| {
        for _ in 0..8 {
            assert_eq!(state.bodies[0].faces_at(0.01).unwrap().len(), 6);
            assert!((measured_area(&state.bodies[0], 0.01) - 2200.0).abs() < 1e-6);
            assert!((measured_area(&snapshot.bodies[0], 0.01) - 2200.0).abs() < 1e-6);
        }
    });
    assert_eq!(work.faces_at, 1, "every rebuild and every snapshot shares one measurement: {work:?}");

    // Another tolerance is another measurement, kept beside the first.
    let work = counted(|| {
        for tol in [0.02, 0.01, 0.02, 0.01] {
            assert!((measured_area(&state.bodies[0], tol) - 2200.0).abs() < 1e-6);
        }
    });
    assert_eq!(work.faces_at, 1, "{work:?}");

    // Body is public; a clone's old measurement must never mask a direct replacement.
    state.bodies[0].body = kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let work = counted(|| {
        for _ in 0..3 {
            assert!((measured_area(&state.bodies[0], 0.01) - 600.0).abs() < 1e-6);
            assert!((measured_area(&snapshot.bodies[0], 0.01) - 2200.0).abs() < 1e-6);
        }
    });
    assert_eq!(work.faces_at, 1, "only the replaced body is measured, and the snapshot keeps its own: {work:?}");
}

/// Face measurements take any tolerance and stay bounded: a hostile tolerance answers as the
/// uncached measurement does, a body keeps [`super::FACES_KEPT`] measurements however many
/// tolerances it is asked at, and a dropped one is made again with the same answer.
#[test]
fn face_measurements_are_bounded_and_take_any_tolerance() {
    let body = ModelBody::new("Block".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap(), 0);
    for tol in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, 1e300] {
        let direct = body.body.faces(tol).map(|faces| faces.len()).map_err(|e| e.to_string());
        for _ in 0..2 {
            let cached = body.faces_at(tol).map(|faces| faces.len()).map_err(|e| e.to_string());
            assert_eq!(cached, direct, "tolerance {tol}");
        }
    }
    for k in 1..=3 * super::FACES_KEPT {
        assert_eq!(body.faces_at(k as f64 * 1e-3).unwrap().len(), 6);
    }
    assert_eq!(body.faces.lock().unwrap().len(), super::FACES_KEPT, "the measurements kept are bounded");
    let work = counted(|| {
        assert!((measured_area(&body, 1e-3) - 2200.0).abs() < 1e-6);
        assert!((measured_area(&body, 1e-3) - 2200.0).abs() < 1e-6);
    });
    assert_eq!(work.faces_at, 1, "a dropped measurement is made again, once: {work:?}");
}

/// `parts` imported files of two boxes each, every one in its own component placed away from
/// the origin (as inserted parts are).
fn placed_imports(parts: usize) -> Document {
    let mut doc = Document::new("placed imports");
    for i in 0..parts {
        let x = 30.0 * i as f64;
        let a = kernel::box_solid(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 10.0, 10.0, 10.0)).unwrap();
        let b = kernel::box_solid(Vec3::new(x, 20.0, 0.0), Vec3::new(x + 5.0, 25.0, 5.0)).unwrap();
        let step: Arc<str> = kernel::step_export(&[&a, &b], "test").unwrap().into();
        let comp = doc.add_component(Some(&format!("Part {i}")), 0).unwrap();
        let o = doc.occurrences.iter_mut().find(|o| o.component == comp).unwrap();
        o.transform = rigid(Vec3::new(0.0, 0.0, 5.0 * i as f64), Vec3::ZERO, Vec3::Z, 0.05 * i as f64);
        let kind = FeatureKind::Import { file: format!("part{i}.step"), step, components: Vec::new(), skip: Vec::new(), records: None };
        let id = doc.add_feature(kind, Some(&format!("Part {i}"))).unwrap();
        doc.feature_mut(id).unwrap().component = comp;
    }
    doc
}

fn line_sketch(plane: PlaneRef, to: Vec2) -> FeatureKind {
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(to, None).unwrap();
    sk.add_line_pts(a, b, None).unwrap();
    FeatureKind::Sketch { plane, sketch: sk }
}

#[test]
fn a_sketch_on_a_fixed_plane_costs_the_same_in_a_large_placed_design() {
    let parts = 12;
    let mut doc = placed_imports(parts);
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert_eq!(model.state().bodies.len(), parts * 2);

    // A construction plane, then sketches on the origin planes, on an offset of one and on the
    // construction plane, as commands make them one after another: no body is moved and no
    // file digested again.
    let plane = doc.add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Origin { name: "XZ".into() } }, Some("Datum")).unwrap();
    assert_eq!(counted(|| model.evaluate(&doc)), Counts::default(), "construction plane");
    assert!(model.result(plane).unwrap().error.is_none());
    let planes = [
        PlaneRef::Origin { name: "XY".into() },
        PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "YZ".into() }), distance: "5 mm".into() },
        PlaneRef::Construction { feature: plane.into(), placement: None },
    ];
    for (k, plane) in planes.into_iter().enumerate() {
        let id = doc.add_feature(line_sketch(plane, Vec2::new(10.0, 1.0 + k as f64)), None).unwrap();
        let work = counted(|| model.evaluate(&doc));
        assert_eq!(work, Counts::default(), "sketch {k}");
        assert_eq!(model.last_recomputed, 1);
        let r = model.result(id).unwrap();
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(model.state().bodies.len(), parts * 2);
        // Editing the sketch again recomputes it alone, for the same cost.
        if let Ok(sk) = doc.sketch_mut(id)
            && let Some(p) = sk.points.get_mut(1)
        {
            p.pos = Vec2::new(12.0, 2.0);
        }
        let work = counted(|| model.evaluate(&doc));
        assert_eq!(work, Counts::default(), "sketch {k} edited");
        assert_eq!(model.last_recomputed, 1);
    }
}

/// Construction geometry is an input of every feature: moving a construction plane moves the
/// sketch on it, though the sketch itself is unchanged.
#[test]
fn moving_a_construction_plane_moves_the_sketch_on_it() {
    let mut doc = Document::new("Datum");
    let offset = |distance: &str| FeatureKind::ConstructionPlane {
        plane: PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "XY".into() }), distance: distance.into() },
    };
    let plane = doc.add_feature(offset("5 mm"), Some("Datum")).unwrap();
    let sketch = doc.add_feature(line_sketch(PlaneRef::Construction { feature: plane.into(), placement: None }, Vec2::new(10.0, 1.0)), None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!((model.state().sketch(sketch).unwrap().plane.origin.z - 5.0).abs() < 1e-9);
    doc.feature_mut(plane).unwrap().kind = offset("12 mm");
    model.evaluate(&doc);
    assert!((model.state().sketch(sketch).unwrap().plane.origin.z - 12.0).abs() < 1e-9, "the sketch follows its plane");
}

/// A boundary surface reads its own sketch and nothing else: an edit of a sketch it does not use
/// leaves it as it was. A delete in a later sketch rebuilt a patch through four arcs of an earlier
/// one, whose edge tolerances take most of a second to prove, twice per check (#17714).
#[test]
fn a_boundary_surface_keeps_its_result_when_another_sketch_changes() {
    let mut doc = Document::new("Boundary");
    let mut square = Sketch::new();
    let corners = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)].map(|(x, y)| square.add_point(Vec2::new(x, y), None).unwrap());
    let mut curves = Vec::new();
    for k in 0..4 {
        let line = square.add_line_pts(corners[k], corners[(k + 1) % 4], None).unwrap();
        curves.push(square.curves[line].id.clone());
    }
    let loop_sketch = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: square }, None).unwrap();
    let other = doc.add_feature(line_sketch(PlaneRef::Origin { name: "XZ".into() }, Vec2::new(10.0, 1.0)), None).unwrap();
    let patch = doc
        .add_feature(FeatureKind::SurfaceBoundary { sketch: loop_sketch.into(), curves, orders: Vec::new(), crown: 0.0, samples: 25 }, None)
        .unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert_eq!(model.state().bodies.len(), 1);

    // The line sketch before the patch changes: it alone is evaluated again.
    if let Ok(sk) = doc.sketch_mut(other)
        && let Some(p) = sk.points.get_mut(1)
    {
        p.pos = Vec2::new(12.0, 2.0);
    }
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 1, "only the edited sketch");
    assert!(model.result(patch).unwrap().error.is_none());
    assert_eq!(model.state().bodies.len(), 1);

    // Its own sketch changes: the patch follows it.
    if let Ok(sk) = doc.sketch_mut(loop_sketch)
        && let Some(p) = sk.points.get_mut(corners[2])
    {
        p.pos = Vec2::new(30.0, 20.0);
    }
    let size = model.state().bodies[0].body.size();
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 2, "the square and the patch");
    assert!(model.result(patch).unwrap().error.is_none());
    assert!(model.state().bodies[0].body.size() > size, "the patch reaches the moved corner");
}

#[test]
fn a_sketch_on_a_face_still_sees_the_placed_bodies() {
    let parts = 3;
    let mut doc = placed_imports(parts);
    let mut model = Model::new();
    model.evaluate(&doc);
    // A face pick reads the model as the sketch's component sees it: the other components'
    // bodies are moved into its frame, as before.
    let top = Plane::new(Vec3::new(0.0, 0.0, 10.0), Vec3::X, Vec3::Y).unwrap();
    let face = PlaneRef::Face { plane: top, at: Vec3::new(5.0, 5.0, 10.0), name: None, anchor: None };
    doc.add_feature(line_sketch(face, Vec2::new(3.0, 3.0)), None).unwrap();
    let work = counted(|| model.evaluate(&doc));
    assert!(work.moved >= parts * 2 - 2, "{work:?}");
    assert_eq!(work.digested, 0);
}

/// A sketch on a face of a plate placed turned (its own frame, so the plate is moved into it),
/// evaluated once: the design, its model, the plate's component, the sketch, its anchor and the
/// face picked.
fn sketch_on_a_turned_occurrence() -> (Document, Model, u64, u64, FaceAnchor, Plane) {
    use solidcraft_sketch::{LinkKind, LinkSource};

    let mut doc = Document::new("turned face");
    let comp = doc.add_component(Some("Plate"), 0).unwrap();
    let occurrence = doc.occurrences.iter_mut().find(|o| o.component == comp).unwrap();
    occurrence.transform = rigid(Vec3::ZERO, Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_2);
    let path = vec![occurrence.id];
    let kind = serde_json::from_value(serde_json::json!({
        "type": "box", "corner": [0, 0, 0], "length": "90", "width": "120", "height": "8"
    }))
    .unwrap();
    let plate = doc.add_feature(kind, Some("Plate")).unwrap();
    doc.feature_mut(plate).unwrap().component = comp;
    let mut model = Model::new();
    model.evaluate(&doc);
    let state = model.state();
    let raw = state.bodies.first().unwrap();
    let index = raw.body.face_sources().unwrap().iter().position(|s| matches!(s, Some(kernel::FaceSource::Box { axis: 2, high: true, .. }))).unwrap();
    let world = world_state(&doc, &state).unwrap();
    let shown = world.bodies.first().unwrap();
    let picked = shown.body.face_plane(index).unwrap();
    let anchor = doc.capture_face_anchor(raw, index, &picked, &path, 0, &[]).unwrap();
    let source = LinkSource::Face { body: raw.name.clone(), at: apply_point(&doc.component_transform(comp), Vec3::new(30., 60., 8.)) };
    let resolved = crate::project::resolve(&doc, &BTreeMap::new(), &world, &picked, LinkKind::Project, &source).unwrap();
    let plane = PlaneRef::Face { plane: picked, at: picked.origin, name: None, anchor: Some(Box::new(anchor.clone())) };
    let mut sketch = Sketch::new();
    let a = sketch.add_point(Vec2::ZERO, None).unwrap();
    let b = sketch.add_point(Vec2::new(3., 3.), None).unwrap();
    sketch.add_line_pts(a, b, None).unwrap();
    sketch.add_link(LinkKind::Project, source, &resolved.geom).unwrap();
    let id = doc.add_feature(FeatureKind::Sketch { plane, sketch }, None).unwrap();
    doc.feature_mut(id).unwrap().component = 0;
    let first = counted(|| model.evaluate(&doc));
    assert!(first.moved > 0, "{first:?}");
    assert!(model.result(id).unwrap().error.is_none(), "{:?}", model.result(id).unwrap().error);
    (doc, model, comp, id, anchor, picked)
}

#[test]
fn editing_a_sketch_on_a_turned_occurrence_reuses_its_frame_bodies() {
    let (mut doc, model, comp, id, anchor, picked) = sketch_on_a_turned_occurrence();
    let reopened = Model::adopt(&doc, &model.saved(&doc).unwrap()).unwrap();
    for mut model in [model, reopened] {
        let before = state_in_face_anchor_frame(&doc, &model.state_before(id), &anchor, 0).unwrap().bodies.first().unwrap().mesh();

        for to in [Vec2::new(4., 5.), Vec2::new(6., 7.)] {
            doc.sketch_mut(id).unwrap().points.get_mut(1).unwrap().pos = to;
            let work = counted(|| model.evaluate(&doc));
            assert_eq!(work.moved, 0, "sketch edit must reuse the unchanged placement: {work:?}");
            assert_eq!(model.last_recomputed, 1);
            let result = model.result(id).unwrap();
            assert!(result.error.is_none(), "{:?}", result.error);
            let solved = model.state();
            let solved = solved.sketch(id).unwrap();
            assert!(solved.sketch.links.iter().all(|link| !link.lost));
            assert!((solved.plane.normal().dot(picked.normal()) - 1.).abs() < 1e-9);
            let after = state_in_face_anchor_frame(&doc, &model.state_before(id), &anchor, 0).unwrap().bodies.first().unwrap().mesh();
            assert!(Arc::ptr_eq(&before, &after), "the anchored body's mesh must survive sketch edits");
        }
        // A changed placement is a different frame: the body is moved again, not reused.
        let turned = doc.occurrences.iter().find(|o| o.component == comp).unwrap().transform;
        doc.occurrences.iter_mut().find(|o| o.component == comp).unwrap().transform =
            rigid(Vec3::ZERO, Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_3);
        let work = counted(|| model.evaluate(&doc));
        assert!(work.moved > 0, "a changed placement must move the body again: {work:?}");
        doc.occurrences.iter_mut().find(|o| o.component == comp).unwrap().transform = turned;
        model.evaluate(&doc);
    }
}

/// Evaluations between two edits of the sketch that move nothing (a delete refreshes the model
/// more than once, an undo can take its sketch from the cache) leave its frame bodies in place:
/// they once aged them out, and the next edit moved and meshed every placed body again, which
/// in a design of thousands of imported bodies took a minute per edit.
#[test]
fn evaluations_that_move_nothing_keep_the_frame_bodies() {
    let (mut doc, mut model, _, id, _, _) = sketch_on_a_turned_occurrence();
    let edit = |doc: &mut Document, to: Vec2| doc.sketch_mut(id).unwrap().points.get_mut(1).unwrap().pos = to;
    edit(&mut doc, Vec2::new(4., 5.));
    let work = counted(|| model.evaluate(&doc));
    assert_eq!(work.moved, 0, "{work:?}");
    for _ in 0..3 {
        let work = counted(|| model.evaluate(&doc));
        assert_eq!((work.moved, model.last_recomputed), (0, 0), "{work:?}");
    }
    edit(&mut doc, Vec2::new(6., 7.));
    let work = counted(|| model.evaluate(&doc));
    assert_eq!(model.last_recomputed, 1);
    assert_eq!(work.moved, 0, "the edit after idle evaluations must reuse the moved plate: {work:?}");
}

/// A Frame Member is built turned by the Rotate steps after it, so their turns (and the
/// parameters their angles use) are in its fingerprint and the key its result is cached under;
/// with none left, the keys are what they were before, and designs without turns keep theirs.
#[test]
fn a_frame_without_turns_keeps_its_fingerprint() {
    let kind = |v: serde_json::Value| serde_json::from_value::<FeatureKind>(v).unwrap();
    let mut doc = Document::new("frame");
    doc.set_param("tilt", "20 deg", None, None).unwrap();
    let frame = doc
        .add_feature(
            kind(serde_json::json!({"type": "frame_member", "path_sketch": 1,
                "profile": {"source": "library", "profile": {"kind": "rect_tube", "width": 40, "height": 40, "wall": 3}}})),
            None,
        )
        .unwrap();
    let key = |doc: &Document| {
        let vals = super::evaluation_values(doc).0;
        let f = doc.feature(frame).unwrap();
        let steps = super::ImportDigests::default();
        let fp = super::chain_fingerprint(doc, &vals, f, 0, false, &steps, None);
        // And the key its result is cached under, which leaves the chain out.
        (fp, super::EvalKey::new(doc, &vals, f, fp, 0, 0, &ModelState::default(), &steps, 0).whole)
    };
    let before = key(&doc);
    let turn = doc
        .add_feature(kind(serde_json::json!({"type": "frame_member_turn", "member": {"feature": frame, "line": "l1"}, "angle": "tilt"})), None)
        .unwrap();
    let turned = key(&doc);
    assert!(turned.0 != before.0 && turned.1 != before.1, "the turn is in the keys");
    doc.set_param("tilt", "40 deg", None, None).unwrap();
    let tilted = key(&doc);
    assert!(tilted.0 != turned.0 && tilted.1 != turned.1, "so is the value of the parameter its angle uses");
    doc.features.iter_mut().filter(|f| f.id == turn).for_each(|f| f.suppressed = true);
    assert_eq!(key(&doc), before, "a suppressed turn isn't");
    doc.features.iter_mut().filter(|f| f.id == turn).for_each(|f| f.suppressed = false);
    doc.delete_feature(turn).unwrap();
    assert_eq!(key(&doc), before);
}

#[test]
fn an_imported_file_is_fingerprinted_by_its_text() {
    let doc = placed_imports(2);
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert_eq!(work.digested, 2);
    let fingerprints = |m: &Model| m.results.iter().map(|r| r.fingerprint).collect::<Vec<_>>();
    let before = fingerprints(&model);

    // The same text held by another copy (an undo state, a design opened again): the same
    // fingerprints, and nothing evaluated again.
    let mut copy = doc.clone();
    for f in &mut copy.features {
        if let FeatureKind::Import { step, .. } = &mut f.kind {
            *step = Arc::from(step.to_string());
        }
    }
    let work = counted(|| model.evaluate(&copy));
    assert_eq!(work.digested, 2);
    assert_eq!(fingerprints(&model), before);
    assert_eq!(model.last_recomputed, 0);

    // Other text: that feature and the ones after it are evaluated again.
    let other = kernel::box_solid(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0)).unwrap();
    let text: Arc<str> = kernel::step_export(&[&other], "test").unwrap().into();
    if let Some(FeatureKind::Import { step, .. }) = copy.features.last_mut().map(|f| &mut f.kind) {
        *step = text;
    }
    let work = counted(|| model.evaluate(&copy));
    assert_eq!(work.digested, 1);
    let after = fingerprints(&model);
    assert_eq!(after.first(), before.first());
    assert_ne!(after.last(), before.last());
    assert_eq!(model.last_recomputed, 1);
    assert_eq!(model.state().bodies.len(), 3);
}

/// An import after a sketch that follows a moved part keeps its result: the file is not read
/// again. A joint moving a part re-read the imported STEP file, and a large file took seconds to
/// read, so every move of the joint was as slow as the file was big (#17310).
#[test]
fn an_import_after_a_moved_sketch_is_not_read_again() {
    let (mut doc, mut model, comp, _, _, _) = sketch_on_a_turned_occurrence();
    let part = kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let step: Arc<str> = kernel::step_export(&[&part], "test").unwrap().into();
    doc.add_feature(FeatureKind::Import { file: "part.step".into(), step, components: Vec::new(), skip: Vec::new(), records: None }, None).unwrap();
    let work = counted(|| model.evaluate(&doc));
    assert_eq!(work.imports_read, 1, "the file is read once: {work:?}");

    // The part turns: the sketch on its face follows it, and the import after it is not read
    // again: the import reads the file and nothing of the model.
    doc.occurrences.iter_mut().find(|o| o.component == comp).unwrap().transform = rigid(Vec3::ZERO, Vec3::ZERO, Vec3::X, std::f64::consts::FRAC_PI_3);
    let work = counted(|| model.evaluate(&doc));
    assert_eq!(work.imports_read, 0, "the import after the sketch must not be read again: {work:?}");
    assert_eq!(model.last_recomputed, 1, "only the sketch is evaluated again: {work:?}");
    assert_eq!(model.state().bodies.len(), 2, "the plate and the imported part: {work:?}");
}

/// Pinning body names fingerprints the timeline again only from the first renamed feature on
/// (`Model::pinned_prefix`), and gives the fingerprints the full pass gives: random pins
/// on a timeline of boxes, cuts, patterns of features (one placed before its source, as one made
/// with the timeline rolled back is), a suppressed feature and a history marker, after an
/// evaluation and after earlier pins.
#[test]
fn pinning_body_names_fingerprints_as_the_full_pass_does() {
    let kind = |v: serde_json::Value| serde_json::from_value::<FeatureKind>(v).unwrap();
    let mut doc = Document::new("pins");
    let mut ids = Vec::new();
    for i in 0..10 {
        let x = 20.0 * i as f64;
        let b =
            kind(serde_json::json!({"type": "box", "corner": [x, 0.0, 0.0], "length": "10", "width": "10", "height": "10", "operation": "new_body"}));
        ids.push(doc.add_feature(b, None).unwrap());
        if i % 3 == 1 {
            let c = kind(
                serde_json::json!({"type": "cylinder", "axis": [0.0, 0.0, 1.0], "base": [x + 5.0, 5.0, -1.0], "height": "12", "radius": "2", "operation": "cut"}),
            );
            ids.push(doc.add_feature(c, None).unwrap());
        }
    }
    let circular = |features: Vec<u64>| {
        kind(serde_json::json!({"type": "pattern", "features": features, "bodies": [],
            "pattern": {"type": "circular", "angle": "360 deg", "axis": [0.0, 0.0, 1.0], "count": "3", "origin": [0.0, -40.0, 0.0]}}))
    };
    ids.push(doc.add_feature(circular(vec![ids[2]]), None).unwrap());
    // A pattern of a later feature, placed early in the timeline.
    let early = doc.add_feature(circular(vec![ids[9]]), None).unwrap();
    let at = doc.features.iter().position(|f| f.id == early).unwrap();
    let f = doc.features.remove(at);
    doc.features.insert(3, f);
    ids.push(early);
    doc.feature_mut(ids[5]).unwrap().suppressed = true;

    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let fps = |m: &Model| m.results.iter().map(|r| (r.id, r.fingerprint)).collect::<Vec<_>>();
    let mut old = Arc::new(doc);
    let mut model = Model::new();
    model.evaluate_shared(&old);
    let (mut proven, mut kept) = (0, 0);
    for round in 0..60 {
        if round % 20 == 10 {
            // From an evaluation's fingerprints again, with the marker in some rounds.
            let mut d = (*old).clone();
            d.marker = (round == 30).then_some(d.features.len() - 2);
            old = Arc::new(d);
            model.evaluate_shared(&old);
        }
        let mut new = (*old).clone();
        let mut renamed = Vec::new();
        for _ in 0..1 + next(3) {
            let f = &mut new.features[next(old.features.len())];
            let slot = next(3);
            f.body_names.resize(f.body_names.len().max(slot + 1), String::new());
            f.body_names[slot] = format!("Pinned {round} {slot}");
            renamed.push(f.id);
        }
        new.reserved_body_names.insert(format!("Body{}", 100 + round));
        let new = Arc::new(new);
        let start = model.pinned_prefix(&old, &renamed);
        let first = new.features.iter().position(|f| renamed.contains(&f.id)).unwrap();
        assert!(start <= first, "round {round}: kept {start} past the first renamed feature {first}");
        proven += usize::from(start > 0);
        kept += start;
        let mut partial = model.clone();
        partial.refingerprint_shared(&new, start);
        let mut full = model.clone();
        full.refingerprint_shared(&new, 0);
        assert_eq!(fps(&partial), fps(&full), "round {round}");
        assert_ne!(fps(&partial), fps(&model), "round {round}: the names are fingerprinted");
        // The pinned fingerprints prove themselves for the next pin, as the full pass's do.
        assert_eq!(partial.fingerprinted.as_ref().map(|(_, d)| *d), full.fingerprinted.as_ref().map(|(_, d)| *d));
        model = partial;
        old = new;
    }
    assert!(proven > 40 && kept > 0, "the stored fingerprints were reused: {proven} rounds, {kept} results");

    // No proof, no reuse: results changed since, another design, or the design about to change
    // in place.
    let last = [*ids.last().unwrap()];
    assert!(model.pinned_prefix(&old, &last) > 0);
    let mut changed = model.clone();
    changed.results[0].fingerprint ^= 1;
    assert_eq!(changed.pinned_prefix(&old, &last), 0);
    assert_eq!(model.pinned_prefix(&Arc::new((*old).clone()), &last), 0);
    let mut forgot = model.clone();
    forgot.forget_fingerprints();
    assert_eq!(forgot.pinned_prefix(&old, &last), 0);
}

/// A design evaluated ahead, on a worker thread of its own, fingerprints its features from the
/// texts the window's evaluation wrote: none is written out again there (the store was per
/// thread, and every ahead run started it empty), and the fingerprints are the same.
#[test]
fn an_evaluation_on_another_thread_reuses_the_written_features() {
    let mut doc = Document::new("ahead");
    // Ids no other test's design uses: the store is shared by the tests running beside this one.
    doc.next_id = 0x0007_e57a_4ead_0000;
    for i in 0..40 {
        let x = 20.0 * i as f64;
        let b = serde_json::json!({"type": "box", "corner": [x, 0.0, 0.0], "length": "10", "width": "10", "height": "10", "operation": "new_body"});
        doc.add_feature(serde_json::from_value(b).unwrap(), None).unwrap();
    }
    let mut window = Model::new();
    let first = written(|| window.evaluate(&doc));
    assert!(first >= doc.features.len(), "{first} written");
    let fps = |m: &Model| m.results.iter().map(|r| r.fingerprint).collect::<Vec<_>>();
    let (doc2, mut ahead) = (doc.clone(), window.clone());
    ahead.no_cache = true;
    let (work, ahead_fps) = std::thread::spawn(move || {
        let work = written(|| ahead.evaluate(&doc2));
        (work, fps(&ahead))
    })
    .join()
    .unwrap();
    assert_eq!(work, 0, "features written out again");
    assert_eq!(ahead_fps, fps(&window));
}

/// A 10 mm square plate per hole along X, 4 mm thick, in component `comp`, with `holes` square
/// holes cut through it by boxes drawn in component `tools` (the plate's own, usually). The
/// plate's body name.
fn plate_with_holes(doc: &mut Document, comp: u64, tools: u64, holes: usize) -> String {
    let length = format!("{}", 10 * holes.max(1));
    let kind = FeatureKind::Box {
        corner: point_expr(Vec3::ZERO),
        length: length.into(),
        width: "10".into(),
        height: "4".into(),
        operation: Operation::NewBody,
        centred: false,
    };
    let plate = doc.add_feature(kind, None).unwrap();
    doc.feature_mut(plate).unwrap().component = comp;
    let mut model = Model::new();
    model.evaluate(doc);
    let body = model.state().bodies.iter().find(|b| b.feature == plate).unwrap().name.clone();
    for k in 0..holes {
        let corner = point_expr(Vec3::new(10.0 * k as f64 + 4.0, 4.0, -1.0));
        let kind = FeatureKind::Box { corner, length: "2".into(), width: "2".into(), height: "6".into(), operation: Operation::Cut, centred: false };
        let id = doc.add_feature(kind, None).unwrap();
        let f = doc.feature_mut(id).unwrap();
        f.component = tools;
        f.participants = vec![body.clone()];
    }
    body
}

fn place(doc: &mut Document, comp: u64, m: Mat) {
    doc.occurrences.iter_mut().find(|o| o.component == comp).unwrap().transform = m;
}

fn volume(model: &Model, body: &str) -> f64 {
    kernel::measure(&model.state().body(body).unwrap().body).unwrap().volume
}

/// A cam's profile is read from the cam's bodies, and their meshes are made once: a solve that
/// moves the parts does not mesh them again. Every turn of the cam worked the profile out again,
/// and the mesh of every body of the cam's component was the cost of it (#3389).
#[test]
fn a_cams_profile_is_meshed_once() {
    use crate::joints::cam::CamProfile;
    use crate::joints::{Joint, JointKind, JointOrigin, Snap};
    let mut doc = Document::new("cam");
    let comp = doc.add_component(Some("Cam"), 0).unwrap();
    // The cam's body: a cylinder in the cam's own component.
    let kind = FeatureKind::Cylinder {
        base: point_expr(Vec3::ZERO),
        axis: crate::dir_expr(Vec3::Z),
        radius: "5".into(),
        height: "10".into(),
        operation: Operation::NewBody,
        references: Vec::new(),
    };
    let id = doc.add_feature(kind, None).unwrap();
    doc.feature_mut(id).unwrap().component = comp;
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    let st = model.state();
    let occurrence = doc.occurrences.iter().find(|o| o.component == comp).unwrap().id;
    // The joint that turns the cam, and the profile picked on the cam's body.
    let origin = |occ: u64| JointOrigin::new(occ, Snap::Point { point: Vec3::ZERO, z: None, x: None });
    let joint = Joint {
        id: 1,
        name: "Cam".into(),
        kind: JointKind::Revolute,
        a: origin(0),
        b: origin(occurrence),
        values: Vec::new(),
        limits: Vec::new(),
        dynamics: Vec::new(),
        flip: false,
        offset: 0.0,
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
        offset_xy: [0.0; 2],
        captured: None,
        constraint: None,
    };
    let profile = CamProfile::Edges { occurrence, picks: vec![Vec3::new(5.0, 0.0, 10.0)], names: Vec::new() };
    let work = counted(|| {
        crate::joints::cam::profile_points(&doc, &st, &profile, &joint, (&crate::IDENTITY, &crate::IDENTITY)).unwrap();
    });
    assert_eq!(work.cam_meshes, 1, "the cam's body is meshed once: {work:?}");
    let work = counted(|| {
        crate::joints::cam::profile_points(&doc, &st, &profile, &joint, (&crate::IDENTITY, &crate::IDENTITY)).unwrap();
    });
    assert_eq!(work.cam_meshes, 0, "the mesh of the cam's body is kept: {work:?}");
}

#[test]
fn turning_a_joint_between_parts_cuts_no_hole_again() {
    let mut doc = Document::new("two parts");
    let base = doc.add_component(Some("Base"), 0).unwrap();
    let arm = doc.add_component(Some("Arm"), 0).unwrap();
    let base_plate = plate_with_holes(&mut doc, base, base, 3);
    let arm_plate = plate_with_holes(&mut doc, arm, arm, 3);
    place(&mut doc, arm, rigid(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Z, 0.0));
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert!((volume(&model, &base_plate) - (1200.0 - 3.0 * 16.0)).abs() < 1e-6);
    // Turning the arm (as a revolute joint does) moves it and nothing else: each part's holes
    // stay cut in its own frame, so no hole is cut again.
    for step in 1..=3 {
        place(&mut doc, arm, rigid(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Z, 0.3 * f64::from(step)));
        model.evaluate(&doc);
        assert_eq!(model.last_recomputed, 0, "step {step}");
        assert!((volume(&model, &arm_plate) - (1200.0 - 3.0 * 16.0)).abs() < 1e-6);
    }
    // Moving both parts together changes nothing between them either.
    for comp in [base, arm] {
        place(&mut doc, comp, rigid(Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    }
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0);
}

#[test]
fn a_tool_cutting_another_part_cuts_again_when_that_part_moves() {
    let mut doc = Document::new("cross part");
    let tools = doc.add_component(Some("Tools"), 0).unwrap();
    let plate = doc.add_component(Some("Plate"), 0).unwrap();
    let other = doc.add_component(Some("Other"), 0).unwrap();
    // The other part first, so the cut across parts is the last feature: it alone is evaluated
    // again when it changes.
    plate_with_holes(&mut doc, other, other, 1);
    let body = plate_with_holes(&mut doc, plate, tools, 1);
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert!((volume(&model, &body) - (400.0 - 16.0)).abs() < 1e-6);
    // An unrelated part moving: the cut keeps its result.
    place(&mut doc, other, rigid(Vec3::new(0.0, 40.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0);
    // The plate moving away from the tool by 5 mm: the hole is cut again where the tool now meets
    // it, half of it on the plate.
    place(&mut doc, plate, rigid(Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 1);
    assert!((volume(&model, &body) - (400.0 - 8.0)).abs() < 1e-6, "{}", volume(&model, &body));
    // The tool's part following it: the hole is where it was at first, the first result again.
    place(&mut doc, tools, rigid(Vec3::new(5.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0);
    assert!((volume(&model, &body) - (400.0 - 16.0)).abs() < 1e-6, "{}", volume(&model, &body));
}

/// A combine whose tool is a further placement's body (`Bit (Drill:2)`) reads that placement
/// alone. A component or another placement added elsewhere keeps the combine's result; moving
/// the tool's own placement cuts again. Reading every placement cut each such combine again
/// for every occurrence added anywhere in the design.
#[test]
fn a_combine_with_a_placed_copys_tool_keeps_its_result_when_other_placements_are_added() {
    let boxed = |corner: Vec3, length: &str, height: &str| FeatureKind::Box {
        corner: point_expr(corner),
        length: length.into(),
        width: length.into(),
        height: height.into(),
        operation: Operation::NewBody,
        centred: false,
    };
    let mut doc = Document::new("copy tool");
    let board = doc.add_feature(boxed(Vec3::ZERO, "100", "10"), None).unwrap();
    let drill = doc.add_component(Some("Drill"), 0).unwrap();
    let bit = doc.add_feature(boxed(Vec3::new(-50.0, -50.0, 5.0), "10", "5"), None).unwrap();
    doc.feature_mut(bit).unwrap().component = drill;
    let copy = doc.add_occurrence(drill, 0, rigid(Vec3::new(70.0, 70.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0)).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    let st = model.state();
    let name_of = |id: u64| st.bodies.iter().find(|b| b.feature == id).unwrap().name.clone();
    let (board, tool) = (name_of(board), crate::WorldNames::new(&doc, &st).name(&name_of(bit), drill, &[copy]));
    let kind = FeatureKind::Combine { target: board.clone().into(), tools: vec![tool.into()], operation: Operation::Cut, keep_tools: true };
    let combine = doc.add_feature(kind, None).unwrap();
    model.evaluate(&doc);
    assert!(model.result(combine).is_some_and(|r| r.error.is_none()), "{:?}", model.result(combine));
    assert!((volume(&model, &board) - (100_000.0 - 500.0)).abs() < 1e-6, "{}", volume(&model, &board));
    // A new component and another placement of the drill, away from the board: nothing to cut again.
    doc.add_component(Some("Other"), 0).unwrap();
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0, "component added");
    doc.add_occurrence(drill, 0, rigid(Vec3::new(500.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0)).unwrap();
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0, "placement added");
    // The tool's own placement moved off the board: the hole goes with it.
    doc.occurrences.iter_mut().find(|o| o.id == copy).unwrap().transform = rigid(Vec3::new(300.0, 300.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0);
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 1, "tool moved");
    assert!((volume(&model, &board) - 100_000.0).abs() < 1e-6, "{}", volume(&model, &board));
}

/// A root design with two 10 x 10 x 4 mm plates, "Plate" at the origin and "Other" 30 mm along Y,
/// and a sketch on origin plane `plane` of the rectangle `profile` spans (opposite corners, in mm):
/// (the plate part, the other part, the sketch).
fn two_plates_and_a_sketch(doc: &mut Document, plane: &str, profile: [Vec2; 2]) -> (u64, u64, u64) {
    let plate = doc.add_component(Some("Plate"), 0).unwrap();
    let other = doc.add_component(Some("Other"), 0).unwrap();
    place(doc, other, rigid(Vec3::new(0.0, 30.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0));
    plate_with_holes(doc, other, other, 0);
    plate_with_holes(doc, plate, plate, 0);
    let mut sk = Sketch::new();
    let [a, c] = profile;
    let p = [a, Vec2::new(c.x, a.y), c, Vec2::new(a.x, c.y)].map(|q| sk.add_point(q, None).unwrap());
    for k in 0..4 {
        sk.add_line_pts(p[k], p[(k + 1) % 4], None).unwrap();
    }
    let sketch = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: plane.into() }, sketch: sk }, None).unwrap();
    (plate, other, sketch)
}

/// `kind` added to the root with its face points named after the faces under them, as the
/// command that makes it names them.
fn add_named(doc: &mut Document, kind: FeatureKind) -> u64 {
    let mut model = Model::new();
    model.evaluate(doc);
    let names = face_point_names(doc, &model.state(), &kind, 0).unwrap();
    assert!(names.iter().all(|n| !n.is_empty()), "{names:?}");
    let id = doc.add_feature(kind, None).unwrap();
    doc.feature_mut(id).unwrap().face_names = names;
    id
}

/// An extrude in the root from the top of the plate (z 4) 6 mm up, over 2 to 8 mm on X and Y.
fn extrude_from_the_plate(doc: &mut Document) -> (u64, u64, u64) {
    let (plate, other, sketch) = two_plates_and_a_sketch(doc, "XY", [Vec2::new(2.0, 2.0), Vec2::new(8.0, 8.0)]);
    let kind = FeatureKind::Extrude {
        thin: None,
        sketch: SketchId(sketch),
        profiles: ProfileSel::All,
        extent: serde_json::from_value(serde_json::json!({"distance": "6", "from": [5.0, 5.0, 4.0]})).unwrap(),
        operation: Operation::NewBody,
        targets: vec![],
    };
    (plate, other, add_named(doc, kind))
}

/// Where the body feature `id` made has its centre.
fn centroid_of(model: &Model, id: u64) -> Vec3 {
    let st = model.state();
    kernel::measure(&st.bodies.iter().find(|b| b.feature == id).unwrap().body).unwrap().centroid
}

/// An extrude from a face of the plate reads where the plate is, and nothing else (#6325): a
/// joint moving another part leaves its result, and every result after it, as it was.
#[test]
fn an_extrude_from_its_own_part_keeps_its_result_when_another_part_moves() {
    let mut doc = Document::new("from object");
    let (plate, other, extrude) = extrude_from_the_plate(&mut doc);
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert!((centroid_of(&model, extrude).z - 7.0).abs() < 1e-6, "{:?}", centroid_of(&model, extrude));
    place(&mut doc, other, rigid(Vec3::new(0.0, 40.0, 3.0), Vec3::ZERO, Vec3::Z, 0.0));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0);
    // The plate lifted 5 mm: the extrude starts on its top where it is now (#928).
    place(&mut doc, plate, rigid(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, Vec3::Z, 0.0));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 1);
    assert!((centroid_of(&model, extrude).z - 12.0).abs() < 1e-6, "{:?}", centroid_of(&model, extrude));
}

/// A revolve To a face of the plate, as the extrude from it: it turns again only when the plate
/// moves.
#[test]
fn a_revolve_to_its_own_part_keeps_its_result_when_another_part_moves() {
    let mut doc = Document::new("revolve to object");
    let (plate, other, sketch) = two_plates_and_a_sketch(&mut doc, "XZ", [Vec2::new(12.0, 0.0), Vec2::new(15.0, 2.0)]);
    let kind = FeatureKind::Revolve {
        sketch: SketchId(sketch),
        profiles: ProfileSel::All,
        axis: AxisRef::SketchAxis { axis: "y".into() },
        angle: "90 deg".into(),
        operation: Operation::NewBody,
        targets: vec![],
        angle2: None,
        symmetric: false,
        convention: Default::default(),
        to: Some(serde_json::from_value(serde_json::json!([5.0, 5.0, 4.0])).unwrap()),
    };
    let revolve = add_named(&mut doc, kind);
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    let turned = volume(&model, &model.state().bodies.iter().find(|b| b.feature == revolve).unwrap().name.clone());
    place(&mut doc, other, rigid(Vec3::new(0.0, 40.0, 3.0), Vec3::ZERO, Vec3::Z, 0.0));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0);
    // The plate turned a quarter about Z: the revolve turns to its top where it is now.
    place(&mut doc, plate, rigid(Vec3::ZERO, Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 1);
    let now = volume(&model, &model.state().bodies.iter().find(|b| b.feature == revolve).unwrap().name.clone());
    assert!((now - turned).abs() > 1.0, "{turned} then {now}");
}

/// A From face without a name is found by its point, on whatever body lies there: moving any
/// part extrudes again.
#[test]
fn an_extrude_from_a_face_without_a_name_extrudes_again_when_any_part_moves() {
    let mut doc = Document::new("from a point");
    let (_, other, extrude) = extrude_from_the_plate(&mut doc);
    doc.feature_mut(extrude).unwrap().face_names.clear();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!((centroid_of(&model, extrude).z - 7.0).abs() < 1e-6, "{:?}", centroid_of(&model, extrude));
    place(&mut doc, other, rigid(Vec3::new(0.0, 40.0, 3.0), Vec3::ZERO, Vec3::Z, 0.0));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 1);
}

#[test]
fn a_body_is_measured_once_while_it_stays_the_same() {
    let mut doc = Document::new("measured once");
    let cylinder = |doc: &mut Document, x: f64| {
        let kind = FeatureKind::Cylinder {
            base: point_expr(Vec3::new(x, 0.0, 0.0)),
            axis: crate::dir_expr(Vec3::Z),
            radius: "2".into(),
            height: "10".into(),
            operation: Operation::NewBody,
            references: Vec::new(),
        };
        doc.add_feature(kind, None).unwrap();
    };
    cylinder(&mut doc, 0.0);
    cylinder(&mut doc, 10.0);
    let mut model = Model::new();
    model.evaluate(&doc);
    let measure_all = |st: &ModelState| {
        for b in &st.bodies {
            b.measure().unwrap();
        }
    };
    let first = model.state();
    assert_eq!(counted(|| measure_all(&first)).measures, 2);
    assert_eq!(counted(|| measure_all(&first)).measures, 0, "measured again");
    // A feature added: only its body is new.
    cylinder(&mut doc, 20.0);
    model.evaluate(&doc);
    let st = model.state();
    assert_eq!(counted(|| measure_all(&st)).measures, 1);
    // A body replaced in its holder is measured as it is now; the others keep theirs.
    let mut replaced = st.bodies[0].clone();
    replaced.body = kernel::box_solid(Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0)).unwrap();
    assert!((replaced.measure().unwrap().volume - 6.0).abs() < 1e-9);
    assert!((st.bodies[0].measure().unwrap().volume - first.bodies[0].measure().unwrap().volume).abs() < 1e-12);
}

#[test]
fn placed_bodies_taken_as_before_keep_their_measure() {
    let mut doc = Document::new("placed measure");
    let component = doc.add_component(Some("Part"), 0).unwrap();
    doc.body_components.insert("Pin".into(), component);
    doc.occurrences[0].transform = rigid(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO, Vec3::Y, 0.3);
    let mut state = ModelState::default();
    state.bodies.push(ModelBody::new("Pin".into(), kernel::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 8.0).unwrap(), 1));
    let mut cache = PlacedBodies::default();
    let before = world_state_reusing(&doc, &state, &mut cache).unwrap();
    let centroid = before.bodies[0].measure().unwrap().centroid;
    let again = world_state_reusing(&doc, &state, &mut cache).unwrap();
    assert_eq!(counted(|| assert_eq!(again.bodies[0].measure().unwrap().centroid, centroid)).measures, 0);
}

#[test]
fn shared_evaluation_requires_its_actual_model_and_unmodified_geometry_inputs() {
    let mut doc = Document::new("shared evaluation");
    let feature = doc
        .add_feature(
            FeatureKind::Box {
                corner: point_expr(Vec3::ZERO),
                length: "2".into(),
                width: "3".into(),
                height: "4".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    let mut doc = Arc::new(doc);
    let mut model = Model::new();
    assert!(model.evaluate_shared(&doc));
    let original = model.state();
    assert!(!model.evaluate_shared(&doc));
    assert_eq!(model.last_recomputed, 0);
    assert!(Arc::ptr_eq(&original, &model.state()));

    // A geometry edit must be seen even when a caller did not evaluate it before moving.
    let FeatureKind::Box { length, .. } = &mut Arc::make_mut(&mut doc).feature_mut(feature).unwrap().kind else { panic!("box") };
    *length = "5".into();
    assert!(model.evaluate_shared(&doc));
    assert!((model.state().bodies[0].measure().unwrap().volume - 60.).abs() < 1e-8);
    assert!((original.bodies[0].measure().unwrap().volume - 24.).abs() < 1e-8);
    assert!(!model.evaluate_shared(&doc));

    let mut other_doc = (*doc).clone();
    let FeatureKind::Box { length, .. } = &mut other_doc.feature_mut(feature).unwrap().kind else { panic!("box") };
    *length = "7".into();
    let mut other = Model::new();
    other.evaluate(&other_doc);
    model.results = other.results;
    assert!(model.evaluate_shared(&doc), "replaced results cannot use the retained proof");
    assert_eq!(model.state().bodies.len(), 1);
    model.no_cache = true;
    assert!(model.evaluate_shared(&doc));
    model.no_cache = false;
    assert!(model.evaluate_shared(&doc));
    let ahead = model.clone();
    model.prime(&ahead);
    assert!(model.evaluate_shared(&doc), "primed results use normal evaluation");
    model.unprime();
    assert!(model.evaluate_shared(&doc));
    model.evaluate(&doc);
    assert!(model.evaluate_shared(&doc), "the borrowed-document API invalidates the shared-input proof");
    Arc::make_mut(&mut doc).set_param("Width", "6 mm", None, None).unwrap();
    let FeatureKind::Box { width, .. } = &mut Arc::make_mut(&mut doc).feature_mut(feature).unwrap().kind else { panic!("box") };
    *width = "Width".into();
    assert!(model.evaluate_shared(&doc));
    assert!((model.state().bodies[0].measure().unwrap().volume - 120.).abs() < 1e-8);
    Arc::make_mut(&mut doc).set_param("Width", "9 mm", None, None).unwrap();
    assert!(model.evaluate_shared(&doc));
    assert!((model.state().bodies[0].measure().unwrap().volume - 180.).abs() < 1e-8);
    // A primitive's label can change its result without replacing the geometry state.
    // Restoring the earlier document must restore that label and fingerprint too.
    let name = doc.feature(feature).unwrap().name.clone();
    let mut renamed = (*doc).clone();
    renamed.feature_mut(feature).unwrap().name = "Renamed box".into();
    assert!(model.rename_feature(&renamed, feature));
    assert!(model.evaluate_shared(&doc), "refingerprinted results must not keep the old shared-input proof");
    assert_eq!(model.result(feature).unwrap().name, name);
    assert!(model.is_current(&doc));
}

#[test]
fn shared_imported_mesh_inputs_keep_payload_identity_and_invalidate_replacements() {
    let mut doc = Document::new("shared mesh input");
    let component = doc.add_component(Some("Part"), 0).unwrap();
    let feature = doc
        .add_feature(
            FeatureKind::MeshImport {
                file: "triangle.stl".into(),
                meshes: vec![MeshData {
                    name: "Triangle".into(),
                    positions: Arc::from([0., 0., 0., 1., 0., 0., 0., 1., 0.]),
                    triangles: Arc::from([0, 1, 2]),
                    color: None,
                    corner_normals: Default::default(),
                }],
            },
            None,
        )
        .unwrap();
    doc.feature_mut(feature).unwrap().component = component;
    let mut doc = Arc::new(doc);
    let mut model = Model::new();
    assert!(model.evaluate_shared(&doc));
    let original = model.state();
    Arc::make_mut(&mut doc).occurrences[0].transform = rigid(Vec3::new(10., 0., 0.), Vec3::ZERO, Vec3::Z, 0.3);
    assert!(!model.evaluate_shared(&doc));
    assert!(Arc::ptr_eq(&original, &model.state()));
    for edit in 0..6 {
        let FeatureKind::MeshImport { meshes, .. } = &mut Arc::make_mut(&mut doc).feature_mut(feature).unwrap().kind else { panic!("mesh") };
        let mesh = &mut meshes[0];
        match edit {
            0 => Arc::make_mut(&mut mesh.positions)[3] = 2.,
            1 => Arc::make_mut(&mut mesh.triangles).swap(1, 2),
            2 => mesh.color = Some([0.2, 0.4, 0.6]),
            3 => mesh.name = "Renamed".into(),
            4 => mesh.corner_normals = Arc::from([0., 1., 1., 0., 1., 1., 0., 1., 1.]),
            _ => mesh.positions = Arc::from(mesh.positions.to_vec()),
        }
        assert!(model.evaluate_shared(&doc), "mesh edit {edit} must invalidate");
        assert!(model.results.iter().all(|r| r.error.is_none()));
        let mut fresh = Model::new();
        fresh.no_cache = true;
        fresh.evaluate(&doc);
        let (actual, expected) = (model.state(), fresh.state());
        assert_eq!(actual.bodies[0].name, expected.bodies[0].name);
        let (actual, expected) = (actual.bodies[0].mesh(), expected.bodies[0].mesh());
        assert_eq!(actual.positions, expected.positions);
        assert_eq!(actual.triangles, expected.triangles);
        assert_eq!(actual.normals, expected.normals);
        assert!(!model.evaluate_shared(&doc));
    }
}

#[test]
fn mesh_import_reuses_the_validated_body_only_for_its_input_arrays() {
    let mesh = crate::MeshData {
        name: "Triangle".into(),
        positions: vec![0., 0., 0., 1., 0., 0., 0., 1., 0.].into(),
        triangles: vec![0, 1, 2].into(),
        corner_normals: Default::default(),
        color: None,
    };
    let mut doc = Document::default();
    let id = doc.add_feature(FeatureKind::MeshImport { file: "triangle.stl".into(), meshes: vec![mesh.clone()] }, None).unwrap();
    let prepared = PreparedMeshImport::build(std::slice::from_ref(&mesh)).unwrap();
    let built = prepared.body(0, &mesh).unwrap().clone();

    let mut model = Model::default();
    model.prime_mesh_import(id, prepared);
    model.evaluate(&doc);
    assert!(model.state().bodies[0].body.same_shape(&built));
    assert!(model.prepared_meshes.is_none());

    // A prepared body for the same feature cannot replace new shading data.
    let prepared = PreparedMeshImport::build(&[mesh]).unwrap();
    let FeatureKind::MeshImport { meshes, .. } = &mut doc.feature_mut(id).unwrap().kind else { panic!("mesh import") };
    meshes[0].corner_normals = vec![0., 0., 1., 0., 0., 1., 0., 0., 1.].into();
    model.prime_mesh_import(id, prepared);
    model.evaluate(&doc);
    let state = model.state();
    let body = &state.bodies[0].body;
    assert!(!body.same_shape(&built));
    assert_eq!(body.triangle_corner_normals().unwrap().len(), 1);
}

#[test]
fn shared_evaluation_refreshes_live_sketches_and_keeps_unknown_features_conservative() {
    let mut doc = Document::new("live and unknown inputs");
    let feature = doc.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(2., 1.)), None).unwrap();
    let mut doc = Arc::new(doc);
    let mut model = Model::new();
    assert!(model.evaluate_shared(&doc));
    assert!(!model.evaluate_shared(&doc));
    let solved = model.state().sketch(feature).unwrap().clone();
    let mut live = solved.sketch.clone();
    live.points[1].pos.y += 2.;
    assert!(model.show_live_sketch(feature, live, solved.report.clone()));
    assert!(model.is_stale());
    assert!(model.evaluate_shared(&doc));
    assert!(!model.is_stale());
    assert_eq!(model.state().sketch(feature).unwrap().sketch.points, solved.sketch.points);
    Arc::make_mut(&mut doc).add_feature(FeatureKind::ConstructionPlane { plane: PlaneRef::Origin { name: "XY".into() } }, None).unwrap();
    assert!(model.evaluate_shared(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none()));
    assert!(model.evaluate_shared(&doc), "an unclassified feature must keep the ordinary evaluation path");
}

/// A saved model writes each set of sketch keys once, as the keys it sets and drops relative to
/// the set written before it (every state repeating its whole set made a 5,484-feature design's
/// model too large for its file, which then kept only some states and evaluated the rest on
/// opening). Every state reads back the keys it was evaluated with, also in a model thinned to
/// fewer states; a set that names no set before it is refused.
#[test]
fn saved_states_read_back_their_sketch_keys() {
    let mut doc = Document::new("Sketch keys");
    for i in 0..4 {
        doc.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(f64::from(i) + 1.0, 1.0)), None).unwrap();
        for j in 0..3 {
            let corner = point_expr(Vec3::new(f64::from(i) * 20.0, f64::from(j) * 20.0, 0.0));
            let kind =
                FeatureKind::Box { corner, length: "5".into(), width: "5".into(), height: "5".into(), operation: Operation::NewBody, centred: false };
            doc.add_feature(kind, None).unwrap();
        }
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    let keys = |m: &Model| m.results.iter().map(|r| r.state.sketch_keys.clone()).collect::<Vec<_>>();
    assert_eq!(model.state().sketch_keys.len(), 4);
    let saved = model.saved(&doc).unwrap();
    assert_eq!(saved.records["sketch_key_sets"].as_array().unwrap().len(), 4, "one set per sketch");
    assert_eq!(saved.records["states"].as_array().unwrap().len(), doc.features.len());
    assert_eq!(keys(&Model::adopt(&doc, &saved).unwrap()), keys(&model));
    // The final state alone (every body costs more than the budget): the sets are all kept.
    let pieces: Vec<Vec<(usize, usize)>> = (0..saved.bodies.len()).map(|i| vec![(i, 1 << 20)]).collect();
    let (thinned, _, _) = saved.thinned(&pieces, 0).unwrap();
    assert_eq!(thinned.records["states"].as_array().unwrap().len(), 1);
    assert_eq!(thinned.final_state().unwrap().sketch_keys, model.state().sketch_keys);
    assert_eq!(keys(&Model::adopt(&doc, &thinned).unwrap()), keys(&model));
    // Sets and drops read as written; a base that is not an earlier set is refused.
    let with_sets = |sets: serde_json::Value| SavedModel {
        records: {
            let mut r = saved.records.clone();
            r["sketch_key_sets"] = sets;
            r
        },
        bodies: saved.bodies.clone(),
    };
    let first = *model.state().sketch_keys.keys().next().unwrap();
    let dropped = with_sets(serde_json::json!([
        {"set": [[first, "0000000000000001"], [7, "0000000000000002"]], "drop": []},
        {"base": 0, "set": [], "drop": []},
        {"base": 1, "set": [[8, "0000000000000003"]], "drop": [first]},
        {"base": 0, "set": [[7, "00000000000000ff"]], "drop": []},
    ]));
    let want: BTreeMap<u64, u64> = [(7, 0xff), (first, 1)].into();
    assert_eq!(dropped.final_state().unwrap().sketch_keys, want);
    let dropped = with_sets(serde_json::json!([
        {"set": [[first, "0000000000000001"], [7, "0000000000000002"]], "drop": []},
        {"base": 0, "set": [], "drop": []},
        {"base": 0, "set": [], "drop": []},
        {"base": 1, "set": [[8, "0000000000000003"]], "drop": [first]},
    ]));
    assert_eq!(dropped.final_state().unwrap().sketch_keys, [(7, 2), (8, 3)].into());
    for base in [3, 4] {
        let sets =
            serde_json::json!([{"set": [], "drop": []}, {"set": [], "drop": []}, {"set": [], "drop": []}, {"base": base, "set": [], "drop": []}]);
        assert!(with_sets(sets).final_state().is_err(), "base {base}");
    }
}

/// Synthetic states isolate cache storage from geometry: ids run backwards as they can after
/// reordering a timeline, and a later state changes and then removes an earlier entry.
fn model_with_named_face_points() -> (Document, Model) {
    let mut doc = Document::new("Face points");
    for i in 0..12 {
        doc.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(f64::from(i) + 1.0, 1.0)), None).unwrap();
    }
    doc.features.reverse();
    let mut model = Model::new();
    model.evaluate(&doc);
    let first = doc.features[0].id;
    let mut points = BTreeMap::new();
    for (i, result) in model.results.iter_mut().enumerate() {
        points.insert(result.id, vec![Vec3::new(i as f64, -0.0, 2.0)]);
        if i == 6 {
            points.insert(first, vec![Vec3::new(20.0, 0.0, -0.0)]);
        }
        if i == 9 {
            points.remove(&first);
        }
        Arc::make_mut(&mut result.state).named_faces = points.clone();
    }
    (doc, model)
}

#[test]
fn saved_states_keep_named_face_points_once_and_restore_each_states_values() {
    let (doc, model) = model_with_named_face_points();
    let saved = model.saved(&doc).unwrap();
    assert_eq!(saved.records["named_faces"].as_array().unwrap().len(), 13, "each feature's points once, plus the changed entry");
    let states = saved.records["states"].as_array().unwrap();
    for state in states.iter().take(6) {
        assert_eq!(state["named_faces"].as_array().unwrap().len(), 1, "backwards feature ids still form one compact run");
    }
    let points = |m: &Model| {
        m.results
            .iter()
            .map(|r| {
                r.state
                    .named_faces
                    .iter()
                    .map(|(f, points)| (*f, points.iter().map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect::<Vec<_>>()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let reopened = Model::adopt(&doc, &saved).unwrap();
    assert_eq!(points(&reopened), points(&model), "each state keeps its own values, including signed zero");
    let saved_again = reopened.saved(&doc).unwrap();
    assert_eq!(points(&Model::adopt(&doc, &saved_again).unwrap()), points(&model));
    let (thinned, _, _) = saved.thinned(&[], 0).unwrap();
    assert_eq!(thinned.records["states"].as_array().unwrap().len(), 1);
    assert_eq!(thinned.final_state().unwrap().named_faces, model.state().named_faces);
}

#[test]
fn saved_named_face_points_refuse_missing_malformed_and_excessive_records() {
    use serde_json::json;
    let (doc, model) = model_with_named_face_points();
    let saved = model.saved(&doc).unwrap();
    let last = saved.records["states"].as_array().unwrap().len() - 1;
    let check = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut records = saved.records.clone();
        edit(&mut records);
        let hostile = SavedModel { records, bodies: saved.bodies.clone() };
        assert!(hostile.final_state().is_err());
        assert!(Model::adopt(&doc, &hostile).is_err());
    };
    check(&|r| {
        r.as_object_mut().unwrap().remove("named_faces");
    });
    check(&|r| {
        r["named_faces"][0][1][0][0] = json!(null);
    });
    check(&|r| {
        r["states"][last]["named_faces"] = json!([[0, u64::MAX]]);
    });
    check(&|r| {
        r["states"][last]["named_faces"] = json!([[13, 1]]);
    });
    check(&|r| {
        r["states"][last]["named_faces"] = json!([[0, 1], [0, 1]]);
    });
    // Different entries for one feature are also ambiguous within one state.
    check(&|r| {
        r["named_faces"][1][0] = r["named_faces"][0][0].clone();
        r["states"][last]["named_faces"] = json!([[0, 2]]);
    });
    // A small run list must not expand into millions of deeply cloned points.
    check(&|r| {
        r["named_faces"][0][1] = json!(vec![Vec3::ZERO; 1000]);
        r["states"][last]["named_faces"] = json!(vec![[0, 1]; 50_000]);
    });
}

/// The dirty check and autosave compare the whole document after every edit; a scanned
/// part's shared triangles must not be read for that. A coordinate that equals nothing (not a
/// number) shows whether the shared arrays were read: only reading them makes a mesh differ
/// from its own copy.
#[test]
fn comparing_a_copied_mesh_does_not_read_its_shared_arrays() {
    let mesh = MeshData {
        name: "Scan".into(),
        positions: Arc::from([0., 0., 0., 1., 0., 0., 0., 1., f64::NAN]),
        triangles: Arc::from([0, 1, 2]),
        color: None,
        corner_normals: Default::default(),
    };
    let copy = mesh.clone();
    assert!(copy == mesh, "a copy shares the arrays and is the same mesh without reading them");
    let mut renamed = mesh.clone();
    renamed.name = "Other".into();
    assert!(renamed != mesh);
    let mut coloured = mesh.clone();
    coloured.color = Some([0.2, 0.4, 0.6]);
    assert!(coloured != mesh);
    let unshared = MeshData { positions: Arc::from(mesh.positions.to_vec()), ..mesh.clone() };
    assert!(unshared != mesh, "arrays that are not shared still compare value by value");
    let finite = MeshData { positions: Arc::from([0., 0., 0., 1., 0., 0., 0., 1., 0.]), ..mesh.clone() };
    let equal = MeshData { positions: Arc::from(finite.positions.to_vec()), triangles: Arc::from(finite.triangles.to_vec()), ..finite.clone() };
    assert!(equal == finite, "equal values in other arrays are the same mesh");
    let flipped = MeshData { triangles: Arc::from([0, 2, 1]), ..finite.clone() };
    assert!(flipped != finite);
}

/// `blocks` boxes, each its own body, in component `comp`, then a cut box without targets
/// through each of the first `cuts` (it cuts every body of its component).
fn blocks_with_cuts(doc: &mut Document, comp: u64, blocks: usize, cuts: usize) {
    let y = 100.0 * comp as f64;
    for k in 0..blocks {
        let corner = point_expr(Vec3::new(20.0 * k as f64, y, 0.0));
        let kind =
            FeatureKind::Box { corner, length: "10".into(), width: "10".into(), height: "4".into(), operation: Operation::NewBody, centred: false };
        let id = doc.add_feature(kind, None).unwrap();
        doc.feature_mut(id).unwrap().component = comp;
    }
    for k in 0..cuts {
        let corner = point_expr(Vec3::new(20.0 * k as f64 + 4.0, y + 4.0, -1.0));
        let kind = FeatureKind::Box { corner, length: "2".into(), width: "2".into(), height: "6".into(), operation: Operation::Cut, centred: false };
        let id = doc.add_feature(kind, None).unwrap();
        doc.feature_mut(id).unwrap().component = comp;
    }
}

#[test]
fn an_evaluation_from_the_cache_does_not_search_the_bodies_for_each_cut() {
    // Each cut reads where every body of its component is placed; looking each one up by name
    // made an evaluation quadratic in the bodies, even with every feature taken from the cache.
    let searches = |blocks: usize| {
        let mut doc = Document::new("blocks");
        blocks_with_cuts(&mut doc, 0, blocks, 3);
        let mut model = Model::new();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
        let work = counted(|| model.evaluate(&doc));
        assert_eq!(model.last_recomputed, 0);
        work.body_searches
    };
    let (few, many) = (searches(4), searches(24));
    assert_eq!(few, many, "searches grew with the bodies: {few} for 4 blocks, {many} for 24");
}

/// The bodies part of the placement digest as it was before the bodies were looked up once:
/// each body searched by name, placed through [`body_world`].
fn digest_searching_each_body(doc: &Document, f: &Feature, st: &ModelState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut reads = PlacementReads::default();
    reads.drawn_targets(Some(st), f);
    let inverse = mat_inverse(&doc.component_transform(f.component)).unwrap();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for name in &reads.bodies {
        name.hash(&mut h);
        match st.body(name) {
            Some(b) => hash_placement(&mat_mul(&inverse, &body_world(doc, b)), &mut h),
            None => u8::MAX.hash(&mut h),
        }
    }
    h.finish() | 1
}

#[test]
fn looking_bodies_up_once_keeps_every_placement_digest() {
    // Two parts placed apart, a body moved into the other part and one moved within its part:
    // each cut's digest is the one searching each body gave, bit for bit, so saved caches stay
    // valid.
    let mut doc = Document::new("digests");
    let a = doc.add_component(Some("A"), 0).unwrap();
    let b = doc.add_component(Some("B"), 0).unwrap();
    blocks_with_cuts(&mut doc, a, 5, 2);
    blocks_with_cuts(&mut doc, b, 4, 2);
    blocks_with_cuts(&mut doc, 0, 3, 1);
    place(&mut doc, a, rigid(Vec3::new(3.0, -7.0, 1.5), Vec3::ZERO, Vec3::Z, 0.4));
    place(&mut doc, b, rigid(Vec3::new(-20.0, 5.0, 0.0), Vec3::ZERO, Vec3::X, 0.25));
    let mut model = Model::new();
    model.evaluate(&doc);
    let names: Vec<String> = model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names.len(), 12);
    doc.body_components.insert(names[1].clone(), b);
    doc.body_offsets.insert(names[2].clone(), rigid(Vec3::new(0.0, 0.0, 2.0), Vec3::ZERO, Vec3::Y, 0.1));
    doc.body_offsets.insert(names[6].clone(), rigid(Vec3::new(1.0, 1.0, 0.0), Vec3::ZERO, Vec3::Z, -0.3));
    model.evaluate(&doc);
    let st = model.state();
    let cuts: Vec<&Feature> = doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Box { operation: Operation::Cut, .. })).collect();
    assert_eq!(cuts.len(), 5);
    for f in cuts {
        let digest = placement_digest(&doc, f, Some(&st));
        assert_ne!(digest, 0);
        assert_eq!(digest, digest_searching_each_body(&doc, f, &st), "cut {}", f.id);
    }
}

/// #19451: a mirror of a cut that names a further placement's body (`Plate (Part:2)`, not in the
/// model) reads every placement, as the cut does (#20040): moving that placement evaluates both
/// again.
#[test]
fn a_mirror_of_a_cut_naming_a_further_placements_body_reads_every_placement() {
    let mut doc = Document::new("mirror");
    let part = doc.add_component(Some("Part"), 0).unwrap();
    let plate = plate_with_holes(&mut doc, part, 0, 1);
    let cut = doc.features.last().unwrap().id;
    doc.feature_mut(cut).unwrap().participants = vec![format!("{plate} (Part:2)")];
    let second = doc.add_occurrence(part, 0, rigid(Vec3::new(50.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0)).unwrap();
    let mirror = mirror_of(&mut doc, &[cut], PlaneRef::Origin { name: "YZ".into() });
    let digests = |doc: &Document| {
        let mut model = Model::new();
        model.evaluate(doc);
        [cut, mirror].map(|id| placement_digest(doc, doc.feature(id).unwrap(), Some(&model.state_before(id))))
    };
    let before = digests(&doc);
    doc.occurrences.iter_mut().find(|o| o.id == second).unwrap().transform = rigid(Vec3::new(80.0, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.0);
    let after = digests(&doc);
    assert_ne!(after[0], before[0], "the cut reads the moved placement");
    assert_ne!(after[1], before[1], "so does its mirror");
}

/// #19451: a mirror of a root cut into a placed part cuts the part where it is placed, so it is
/// evaluated again when that part moves, not when another does.
#[test]
fn a_mirror_of_a_cut_reads_where_the_cuts_target_is_placed() {
    let mut doc = Document::new("mirror");
    let part = doc.add_component(Some("Part"), 0).unwrap();
    let other = doc.add_component(Some("Other"), 0).unwrap();
    plate_with_holes(&mut doc, part, 0, 1);
    let cut = doc.features.last().unwrap().id;
    let mirror = mirror_of(&mut doc, &[cut], PlaneRef::Origin { name: "YZ".into() });
    let digest = |doc: &Document| {
        let mut model = Model::new();
        model.evaluate(doc);
        placement_digest(doc, doc.feature(mirror).unwrap(), Some(&model.state_before(mirror)))
    };
    let before = digest(&doc);
    assert_ne!(before, 0);
    place(&mut doc, other, rigid(Vec3::new(0.0, 0.0, 50.0), Vec3::ZERO, Vec3::Z, 0.0));
    assert_eq!(digest(&doc), before, "another part moved");
    place(&mut doc, part, rigid(Vec3::new(0.0, 0.0, 20.0), Vec3::ZERO, Vec3::Z, std::f64::consts::PI));
    assert_ne!(digest(&doc), before, "the cut's target moved");
}

/// A mirror of `features` across `plane`, in the root.
fn mirror_of(doc: &mut Document, features: &[u64], plane: PlaneRef) -> u64 {
    let kind = FeatureKind::Mirror {
        features: features.iter().map(|id| FeatureId(*id)).collect(),
        plane,
        bodies: Vec::new(),
        combine: false,
        placements: Vec::new(),
    };
    doc.add_feature(kind, None).unwrap()
}

/// #19451: a mirror of a mirror made in another part reads where that part is placed, though
/// what it copies is the root's: the inner mirror's copies are composed in the outer one's frame
/// through the part's placement. Another part moved changes nothing it reads.
#[test]
fn a_mirror_of_a_mirror_in_another_part_reads_where_that_part_is_placed() {
    let mut doc = Document::new("mirror");
    let part = doc.add_component(Some("Part"), 0).unwrap();
    let other = doc.add_component(Some("Other"), 0).unwrap();
    plate_with_holes(&mut doc, 0, 0, 1);
    let cut = doc.features.last().unwrap().id;
    let plane = PlaneRef::Offset { base: Box::new(PlaneRef::Origin { name: "YZ".into() }), distance: "3 mm".into() };
    let inner = mirror_of(&mut doc, &[cut], plane);
    doc.feature_mut(inner).unwrap().component = part;
    let outer = mirror_of(&mut doc, &[inner], PlaneRef::Origin { name: "XZ".into() });
    let digest = |doc: &Document| {
        let mut model = Model::new();
        model.evaluate(doc);
        placement_digest(doc, doc.feature(outer).unwrap(), Some(&model.state_before(outer)))
    };
    place(&mut doc, part, rigid(Vec3::new(0.0, 0.0, 20.0), Vec3::ZERO, Vec3::Z, std::f64::consts::PI));
    let before = digest(&doc);
    assert_ne!(before, 0);
    place(&mut doc, other, rigid(Vec3::new(0.0, 0.0, 50.0), Vec3::ZERO, Vec3::Z, 0.0));
    assert_eq!(digest(&doc), before, "another part moved");
    place(&mut doc, part, rigid(Vec3::new(0.0, 0.0, 20.0), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2));
    assert_ne!(digest(&doc), before, "the inner mirror's part turned");
}

/// #19451: a mirror of a round in a design with nothing placed copies it as before, without the
/// exact box of any body: those found the frame the round's edges are kept in, which is the
/// mirror's own when nothing is placed.
#[test]
fn a_mirror_of_a_round_with_nothing_placed_measures_no_box() {
    let mut doc = Document::new("blocks");
    block(&mut doc, "A", Vec3::new(-10.0, -5.0, 0.0), [20.0, 10.0, 10.0]);
    block(&mut doc, "B", Vec3::new(-10.0, 20.0, 0.0), [20.0, 10.0, 10.0]);
    let kind = FeatureKind::Fillet {
        edges: vec![Position(Vec3::new(10.0, 5.0, 5.0))],
        radius: "1".into(),
        body: None,
        style: Default::default(),
        fit: Default::default(),
        corner: Default::default(),
        setbacks: Vec::new(),
        turn: Default::default(),
        tangent_chain: false,
        edge_radii: Vec::new(),
    };
    let round = doc.add_feature(kind, None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    let rounded = volume(&model, "A");
    let mirror = mirror_of(&mut doc, &[round], PlaneRef::Origin { name: "YZ".into() });
    let work = counted(|| model.evaluate(&doc));
    assert_eq!(model.result(mirror).and_then(|r| r.error.clone()), None);
    assert!(volume(&model, "A") < rounded - 1e-3, "the copy rounds A's other edge");
    assert_eq!(work.tight_boxes, 0, "{work:?}");
}

/// The first edit after a design opens from its saved model evaluates only what the edit
/// changes, as every later edit does: a block changed leaves the other block and its round as
/// they were. The saved model gave only each feature's whole result, so the first edit after
/// opening evaluated every feature after it, the features of other bodies and parts included.
#[test]
fn the_first_edit_after_opening_keeps_the_features_of_other_bodies() {
    let mut doc = Document::new("blocks");
    block(&mut doc, "A", Vec3::new(-10.0, -5.0, 0.0), [20.0, 10.0, 10.0]);
    block(&mut doc, "B", Vec3::new(-10.0, 20.0, 0.0), [20.0, 10.0, 10.0]);
    let kind = FeatureKind::Fillet {
        edges: vec![Position(Vec3::new(10.0, 25.0, 10.0))],
        radius: "2".into(),
        body: Some("B".into()),
        style: Default::default(),
        fit: Default::default(),
        corner: Default::default(),
        setbacks: Vec::new(),
        turn: Default::default(),
        tangent_chain: false,
        edge_radii: Vec::new(),
    };
    doc.add_feature(kind, None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    let rounded = volume(&model, "B");
    assert!(rounded < 2000.0 - 1.0, "{rounded}");
    let saved = model.saved(&doc).unwrap();
    let reopened = Model::adopt(&doc, &saved).unwrap();
    // The same file with the round's body keyed otherwise (saved by a version that keyed it
    // another way): what the round changed is not taken back, the round is evaluated again.
    let mut other_key = model.saved(&doc).unwrap();
    // B as the round left it: the deepest of its records (a body keeps the feature that made it).
    let rounded_b = other_key.records["model_bodies"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|b| b["name"] == "B")
        .max_by_key(|b| b["depth"].as_u64())
        .unwrap();
    assert_eq!(rounded_b["depth"], 3, "{rounded_b}");
    rounded_b["key"] = serde_json::json!("deadbeef");
    let rekeyed = Model::adopt(&doc, &other_key).unwrap();
    let a = doc.features.first().unwrap().id;
    if let FeatureKind::Box { length, .. } = &mut doc.feature_mut(a).unwrap().kind {
        *length = "25".into();
    }
    let bodies = |model: &Model| {
        let st = model.state();
        st.bodies.iter().map(|b| (b.name.clone(), b.key, crate::naming::face_names(b))).collect::<Vec<_>>()
    };
    let mut fresh = Model::new();
    fresh.evaluate(&doc);
    let rebuilt = bodies(&fresh);
    for (what, mut model, evaluated) in [("in the session", model, 1), ("opened", reopened, 1), ("opened, keyed otherwise", rekeyed, 2)] {
        let work = counted(|| model.evaluate(&doc));
        assert_eq!(model.last_recomputed, evaluated, "{what}: {work:?}");
        assert_eq!(work.blends, evaluated - 1, "{what}: {work:?}");
        assert!((volume(&model, "A") - 2500.0).abs() < 1e-6, "{what}: {}", volume(&model, "A"));
        assert!((volume(&model, "B") - rounded).abs() < 1e-9, "{what}: {} then {rounded}", volume(&model, "B"));
        assert_eq!(bodies(&model), rebuilt, "{what}: the same bodies, keys and face names as the design evaluated afresh");
    }
}

/// A join into the only body there is takes it without asking whether the tool meets it: the
/// contact test meshes the body finely, and on a part of a few hundred faces it took longer than
/// the join. With two bodies the tool picks the one it meets, as before.
#[test]
fn a_join_into_the_only_body_makes_no_contact_test() {
    let boss = |corner: Vec3| FeatureKind::Box {
        corner: point_expr(corner),
        length: "4".into(),
        width: "4".into(),
        height: "4".into(),
        operation: Operation::Join,
        centred: false,
    };
    let mut doc = Document::new("one block");
    block(&mut doc, "A", Vec3::new(-10.0, -5.0, 0.0), [20.0, 10.0, 10.0]);
    doc.add_feature(boss(Vec3::new(-2.0, -2.0, 10.0)), None).unwrap();
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert_eq!(work.contact_tests, 0, "{work:?}");
    assert!((volume(&model, "A") - (2000.0 + 64.0)).abs() < 1e-6, "{}", volume(&model, "A"));
    // A second block: the boss joins the one it stands on.
    let mut doc = Document::new("two blocks");
    block(&mut doc, "A", Vec3::new(-10.0, -5.0, 0.0), [20.0, 10.0, 10.0]);
    block(&mut doc, "B", Vec3::new(-10.0, 20.0, 0.0), [20.0, 10.0, 10.0]);
    doc.add_feature(boss(Vec3::new(-2.0, 23.0, 10.0)), None).unwrap();
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().find(|r| r.error.is_some()));
    assert!(work.contact_tests > 0, "{work:?}");
    assert!((volume(&model, "A") - 2000.0).abs() < 1e-6, "{}", volume(&model, "A"));
    assert!((volume(&model, "B") - (2000.0 + 64.0)).abs() < 1e-6, "{}", volume(&model, "B"));
}

/// `comps` parts, each with `bodies` boxes, `sketches` sketches and `taps` taps, placed `copies`
/// times: a design and its state, made without evaluating it.
fn placed_parts(comps: usize, bodies: usize, sketches: usize, taps: usize, copies: usize) -> (Document, ModelState) {
    let mut doc = Document::new("placed parts");
    let mut one = Document::new("one sketch");
    one.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(3.0, 4.0)), None).unwrap();
    let mut m = Model::new();
    m.evaluate(&one);
    let solved = m.state().sketches.first().unwrap().clone();
    let mut st = ModelState::default();
    let solid = kernel::box_solid(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0)).unwrap();
    for c in 0..comps {
        let comp = doc.add_component(Some(&format!("C{c}")), 0).unwrap();
        let o = doc.occurrences.iter_mut().find(|o| o.component == comp).unwrap();
        o.transform = rigid(Vec3::new(10.0 * c as f64, 0.0, 0.0), Vec3::ZERO, Vec3::Z, 0.01 * c as f64);
        for k in 1..copies {
            doc.add_occurrence(comp, 0, rigid(Vec3::new(10.0 * c as f64, 20.0 * k as f64, 0.0), Vec3::ZERO, Vec3::Z, 0.02 * k as f64)).unwrap();
        }
        let mut names = Vec::new();
        for b in 0..bodies {
            let kind =
                serde_json::from_value(serde_json::json!({"type": "box", "corner": [0, 0, 0], "length": "2", "width": "3", "height": "4"})).unwrap();
            let name = format!("B{c}_{b}");
            let id = doc.add_feature(kind, Some(&name)).unwrap();
            doc.feature_mut(id).unwrap().component = comp;
            st.bodies.push(ModelBody::new(name.clone(), solid.clone(), id));
            names.push((name, id));
        }
        for s in 0..sketches {
            let id = doc.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(1.0 + s as f64, 2.0)), None).unwrap();
            doc.feature_mut(id).unwrap().component = comp;
            let mut s2 = SolvedSketch::clone(&solved);
            s2.feature = id;
            st.sketches.push(Arc::new(s2));
        }
        for t in 0..taps {
            let (body, feature) = names[t % names.len()].clone();
            st.threads.push(ThreadInfo {
                feature,
                body,
                designation: "M6".into(),
                major_diameter: 6.0,
                radius: 3.0,
                pitch: 1.0,
                internal: true,
                axis_point: Vec3::new(1.0, 1.0, 0.0),
                axis: solidcraft_geom::UnitVec3::Z,
                start: 0.0,
                end: 4.0,
                form: Default::default(),
                hand: Default::default(),
                starts: 1,
            });
        }
    }
    (doc, st)
}

#[test]
fn placing_parts_looks_each_sketch_and_tap_up_once() {
    // A large assembly searched all features for every sketch and construction, and all bodies
    // twice for every tap and placement, each time its world state was made (every command).
    let searches = |sketches: usize, taps: usize| {
        let (doc, state) = placed_parts(4, 2, sketches, taps, 3);
        let mut world = ModelState::default();
        let n = searched(|| world = world_state(&doc, &state).unwrap());
        assert_eq!(world.bodies.len(), 4 * 2 * 3);
        assert_eq!(world.sketches.len(), 4 * sketches);
        assert_eq!(world.threads.len(), 4 * taps * 3);
        n
    };
    assert_eq!(searches(8, 6), searches(1, 1));
}

#[test]
fn a_placed_tap_follows_its_body_offset() {
    let (mut doc, state) = placed_parts(1, 1, 0, 1, 1);
    let comp = doc.components[0].id;
    doc.body_offsets.insert("B0_0".into(), rigid(Vec3::new(0.0, 0.0, 7.0), Vec3::ZERO, Vec3::Z, 0.0));
    let world = world_state(&doc, &state).unwrap();
    let tap = world.threads.first().unwrap();
    let expected =
        apply_point(&mat_mul(&doc.component_transform(comp), &rigid(Vec3::new(0.0, 0.0, 7.0), Vec3::ZERO, Vec3::Z, 0.0)), Vec3::new(1.0, 1.0, 0.0));
    assert!(tap.axis_point.dist(expected) < 1e-9, "{:?} {expected:?}", tap.axis_point);
}

/// What adding a feature of `kind` at the end of `doc` did to the bodies already there: how many
/// its cut ran booleans on, and which it left exactly as they were (the same geometry, its name,
/// key and face names carried over). A cut that cannot reach a body hands it back as it was, and
/// must not pay a whole boolean to find that out (#10225); the sheet metal hole (#10226) and the
/// cosmetic thread (#10227) check their own shortcuts with it too.
pub(super) struct CutWork {
    pub cut: usize,
    pub kept: Vec<String>,
    pub model: Model,
}

pub(super) fn cut_work(doc: &mut Document, kind: FeatureKind) -> CutWork {
    let mut model = Model::new();
    model.evaluate(doc);
    let id = doc.add_feature(kind, None).unwrap();
    let work = counted(|| model.evaluate(doc));
    let result = model.result(id).unwrap();
    assert_eq!(result.error, None, "the feature builds");
    let before = model.state_before(id);
    let kept = result
        .state
        .bodies
        .iter()
        .filter(|b| {
            before
                .body(&b.name)
                .is_some_and(|old| old.body.same(&b.body) && old.key == b.key && crate::naming::face_names(old) == crate::naming::face_names(b))
        })
        .map(|b| b.name.clone())
        .collect();
    CutWork { cut: work.cut, kept, model }
}

/// A box body named `name`, `size` long on each side from `corner`.
fn block(doc: &mut Document, name: &str, corner: Vec3, size: [f64; 3]) {
    let id = doc
        .add_feature(
            FeatureKind::Box {
                corner: point_expr(corner),
                length: size[0].to_string().into(),
                width: size[1].to_string().into(),
                height: size[2].to_string().into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    doc.feature_mut(id).unwrap().body_names = vec![name.into()];
}

/// A cylinder that cuts, standing on `base` along Z.
fn cutter(base: Vec3, radius: f64, height: f64) -> FeatureKind {
    FeatureKind::Cylinder {
        base: point_expr(base),
        axis: crate::dir_expr(Vec3::Z),
        radius: radius.to_string().into(),
        height: height.to_string().into(),
        operation: Operation::Cut,
        references: Vec::new(),
    }
}

/// A cut leaves the bodies its tool cannot reach as they were, without a boolean on each: in a
/// design of many bodies those booleans were most of a hole's time (#10225).
#[test]
fn a_cut_leaves_bodies_it_cannot_reach_as_they_were() {
    let mut doc = Document::new("far");
    block(&mut doc, "Near", Vec3::ZERO, [50.0, 40.0, 30.0]);
    // 0.5 mm clear of the tool, and far off.
    block(&mut doc, "Beside", Vec3::new(13.5, 0.0, 0.0), [20.0, 40.0, 30.0]);
    for i in 0..5 {
        block(&mut doc, &format!("Far{i}"), Vec3::new(200.0 * (i + 1) as f64, 0.0, 0.0), [50.0, 40.0, 30.0]);
    }
    // Near and Beside overlap; the hole goes through Near only, beside Beside.
    let work = cut_work(&mut doc, cutter(Vec3::new(10.0, 20.0, -5.0), 3.0, 40.0));
    assert_eq!(work.cut, 1, "only the body the tool reaches goes through the boolean");
    assert_eq!(work.kept, ["Beside", "Far0", "Far1", "Far2", "Far3", "Far4"]);
    assert!(volume(&work.model, "Near") < 60_000.0 - 800.0, "and it is cut");
}

/// A cut still goes through the boolean wherever its tool reaches a body: on a face, along a
/// line, into a dome above its edge circle. The boxes are exact, so a tool clear of a body by
/// more than their tolerances leaves it out. (Bodies its tool cannot reach are left out of it:
/// `a_cut_leaves_bodies_it_cannot_reach_as_they_were`.)
#[test]
fn a_cut_still_runs_where_its_tool_touches_grazes_or_meets_a_curved_face() {
    // A cylinder whose side is tangent to the face x = 50, one 0.5 µm clear of it (far past the
    // bodies' tolerances, so left out), one 0.1 mm in.
    for (x, cuts, boolean) in [(53.0, false, 1), (53.0005, false, 0), (52.9, true, 1)] {
        let mut doc = Document::new("graze");
        block(&mut doc, "A", Vec3::ZERO, [50.0, 40.0, 30.0]);
        let work = cut_work(&mut doc, cutter(Vec3::new(x, 20.0, 0.0), 3.0, 30.0));
        assert_eq!(work.cut, boolean, "the tool at x = {x} goes through the boolean {boolean} time(s)");
        let v = volume(&work.model, "A");
        assert_eq!(v < 60_000.0 - 1.0, cuts, "the tool at x = {x} cuts material: {v}");
        assert!(!cuts || work.kept.is_empty(), "a body cut is a new body");
    }
    // A box meeting the block only along its top edge.
    let mut doc = Document::new("edge");
    block(&mut doc, "A", Vec3::ZERO, [50.0, 40.0, 30.0]);
    let along = FeatureKind::Box {
        corner: point_expr(Vec3::new(50.0, 0.0, 30.0)),
        length: "10".into(),
        width: "40".into(),
        height: "10".into(),
        operation: Operation::Cut,
        centred: false,
    };
    assert_eq!(cut_work(&mut doc, along).cut, 1);
    // A dome: a sphere's cap rising 10 mm above the circle it stands on. A thin tool drilled into
    // its top near the apex, well above that circle, cuts it.
    let mut doc = Document::new("dome");
    doc.add_feature(FeatureKind::Sphere { center: point_expr(Vec3::new(0.0, 0.0, -10.0)), radius: "20".into(), operation: Operation::NewBody }, None)
        .unwrap();
    let below = FeatureKind::Box {
        corner: point_expr(Vec3::new(-30.0, -30.0, -40.0)),
        length: "60".into(),
        width: "60".into(),
        height: "40".into(),
        operation: Operation::Cut,
        centred: false,
    };
    doc.add_feature(below, None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    let dome = model.state().bodies[0].name.clone();
    let whole = volume(&model, &dome);
    let work = cut_work(&mut doc, cutter(Vec3::new(2.0, 1.0, 8.0), 1.0, 5.0));
    assert_eq!(work.cut, 1);
    assert!(volume(&work.model, &dome) < whole - 1.0, "the drill cuts into the dome");
    assert!(work.kept.is_empty());
}

/// A cylinder body named `name` standing on `base` along Z.
fn shaft(doc: &mut Document, name: &str, base: Vec3, radius: f64, height: f64) {
    let id = doc
        .add_feature(
            FeatureKind::Cylinder {
                base: point_expr(base),
                axis: crate::dir_expr(Vec3::Z),
                radius: radius.to_string().into(),
                height: height.to_string().into(),
                operation: Operation::NewBody,
                references: Vec::new(),
            },
            None,
        )
        .unwrap();
    doc.feature_mut(id).unwrap().body_names = vec![name.into()];
}

/// The work of adding `kind` at the end of `doc`, and the tap markings the design then has.
fn marking_work(doc: &mut Document, kind: FeatureKind) -> (Counts, Vec<ThreadInfo>) {
    let mut model = Model::new();
    model.evaluate(doc);
    let id = doc.add_feature(kind, None).unwrap();
    let work = counted(|| model.evaluate(doc));
    assert_eq!(model.result(id).unwrap().error, None, "the feature builds");
    (work, model.state().threads.clone())
}

/// A cosmetic tap marking at `p`.
fn marking(p: Vec3) -> FeatureKind {
    FeatureKind::Thread {
        face: FacePick(p),
        designation: Some("M6".into()),
        length: None,
        modeled: false,
        from: None,
        up_to: None,
        form: Default::default(),
        hand: crate::thread_forms::ThreadHand::Right,
        starts: 1,
        ease_out: None,
        tip: Default::default(),
        chamfer_angle: None,
        chamfer_depth: None,
        steep_flank: Default::default(),
        continuous_flanks: false,
        old_auto_size: false,
        additional_faces: Vec::new(),
        target_bodies: Vec::new(),
        single_face_selection: false,
    }
}

/// Four selected cylindrical faces on one plate for a grouped Thread feature.
fn grouped_thread_document(modeled: bool) -> (Document, u64) {
    let mut doc = Document::new("grouped threads");
    block(&mut doc, "Part", Vec3::new(-15.0, -15.0, 0.0), [30.0, 30.0, 2.0]);
    let mut faces = Vec::new();
    for (x, y) in [(-8.0, -8.0), (8.0, -8.0), (-8.0, 8.0), (8.0, 8.0)] {
        doc.add_feature(
            FeatureKind::Cylinder {
                base: point_expr(Vec3::new(x, y, 2.0)),
                axis: crate::dir_expr(Vec3::Z),
                radius: "3".into(),
                height: "4".into(),
                operation: Operation::Join,
                references: Vec::new(),
            },
            None,
        )
        .unwrap();
        faces.push(FacePick(Vec3::new(x + 3.0, y, 5.0)));
    }
    let mut kind = marking(faces.remove(0).0);
    if let FeatureKind::Thread { additional_faces, modeled: value, designation, .. } = &mut kind {
        *additional_faces = faces;
        *value = modeled;
        *designation = Some("M6x0.75".into());
    }
    let id = doc.add_feature(kind, None).unwrap();
    (doc, id)
}

/// A cosmetic group keeps one feature and one marking at each selected cylindrical face.
#[test]
fn a_grouped_thread_marks_every_selected_face_with_one_feature() {
    let (doc, id) = grouped_thread_document(false);
    let mut model = Model::new();
    model.evaluate(&doc);
    assert_eq!(model.result(id).unwrap().error, None);
    assert_eq!(doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Thread { .. })).count(), 1);
    let state = model.state();
    let threads = &state.threads;
    assert_eq!(threads.len(), 4);
    assert!(threads.iter().all(|t| t.feature == id && t.designation == "M6x0.75"));
    let origins: Vec<_> = threads.iter().map(|t| (t.axis_point.x.round() as i64, t.axis_point.y.round() as i64)).collect();
    assert_eq!(origins, vec![(-8, -8), (8, -8), (-8, 8), (8, 8)]);
}

#[test]
fn a_pattern_replays_every_grouped_thread_pick_on_its_named_body() {
    let (mut doc, id) = grouped_thread_document(false);
    if let FeatureKind::Thread { additional_faces, target_bodies, .. } = &mut doc.feature_mut(id).unwrap().kind {
        additional_faces.truncate(1);
        *target_bodies = vec!["Part".into(); 2];
    }
    let pattern = doc
        .add_feature(
            FeatureKind::Pattern {
                features: vec![FeatureId(id)],
                pattern: PatternKind::Rectangular {
                    dir1: Dir(Vec3::Y),
                    count1: "2".into(),
                    spacing1: "16".into(),
                    dir2: None,
                    count2: None,
                    spacing2: None,
                    alignment: solidcraft_sketch::PatternAlignment::Right,
                    alignment2: Default::default(),
                    reversed: false,
                    reversed2: false,
                    distance1: Default::default(),
                    distance2: Default::default(),
                    extent1: None,
                    extent2: None,
                    copy_at_end1: false,
                    copy_at_end2: false,
                },
                bodies: Vec::new(),
                placements: Vec::new(),
                components: Vec::new(),
                occurrences: Vec::new(),
            },
            None,
        )
        .unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert_eq!(model.result(id).unwrap().error, None);
    assert_eq!(model.result(pattern).unwrap().error, None);
    let state = model.state();
    let copied: Vec<_> = state
        .threads
        .iter()
        .filter(|t| t.feature == pattern)
        .map(|t| (t.body.as_str(), t.axis_point.x.round() as i64, t.axis_point.y.round() as i64))
        .collect();
    assert_eq!(copied, vec![("Part", -8, 8), ("Part", 8, 8)]);
}

/// A pattern replays the continuous-flank group and reuses only its final exact output.
#[test]
fn a_patterned_modelled_thread_reuses_its_final_check() {
    let (mut doc, id) = grouped_thread_document(true);
    if let FeatureKind::Thread { additional_faces, target_bodies, continuous_flanks, .. } = &mut doc.feature_mut(id).unwrap().kind {
        additional_faces.truncate(1);
        *target_bodies = vec!["Part".into(); 2];
        *continuous_flanks = true;
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|result| result.error.is_none()));
    let pattern = doc
        .add_feature(
            FeatureKind::Pattern {
                features: vec![FeatureId(id)],
                pattern: PatternKind::Rectangular {
                    dir1: Dir(Vec3::Y),
                    count1: "2".into(),
                    spacing1: "16".into(),
                    dir2: None,
                    count2: None,
                    spacing2: None,
                    alignment: solidcraft_sketch::PatternAlignment::Right,
                    alignment2: Default::default(),
                    reversed: false,
                    reversed2: false,
                    distance1: Default::default(),
                    distance2: Default::default(),
                    extent1: None,
                    extent2: None,
                    copy_at_end1: false,
                    copy_at_end2: false,
                },
                bodies: Vec::new(),
                placements: Vec::new(),
                components: Vec::new(),
                occurrences: Vec::new(),
            },
            None,
        )
        .unwrap();
    let work = counted(|| model.evaluate(&doc));
    let result = model.result(pattern).unwrap();
    assert_eq!(result.error, None);
    assert_eq!(work.thread_cuts, 1);
    assert_eq!(work.kept_checks, 1);
    assert_eq!(work.detached_checks, 0);
    assert_eq!(work.reused_checks, 1);
    let body = &result.state.body("Part").unwrap().body;
    assert!(!super::checked_thread_output(body), "the pattern's receipt ends with its scope");
    kernel::check_kept(body).unwrap();
}

/// Different points on the same cylindrical interval create just one marking.
#[test]
fn duplicate_grouped_thread_picks_resolve_to_one_marking() {
    let (mut doc, id) = grouped_thread_document(false);
    if let FeatureKind::Thread { additional_faces, .. } = &mut doc.feature_mut(id).unwrap().kind {
        additional_faces.push(FacePick(Vec3::new(-8.0, -5.0, 4.0)));
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    assert_eq!(model.result(id).unwrap().error, None);
    assert_eq!(model.state().threads.len(), 4);
}

/// A malformed saved group is bounded before expensive geometry is visited.
#[test]
fn an_oversized_grouped_thread_is_refused() {
    let (mut doc, id) = grouped_thread_document(false);
    if let FeatureKind::Thread { additional_faces, .. } = &mut doc.feature_mut(id).unwrap().kind {
        *additional_faces = vec![FacePick(Vec3::new(-5.0, -8.0, 5.0)); 1024];
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.result(id).unwrap().error.as_ref().is_some_and(|e| e.contains("at most 1024")));
    assert!(model.state().threads.is_empty());
}

/// A body-qualified modelled group cuts every selected shaft within one timeline feature.
#[test]
fn a_grouped_modelled_thread_cuts_every_selected_shaft() {
    let (mut doc, id) = grouped_thread_document(true);
    if let FeatureKind::Thread { target_bodies, .. } = &mut doc.feature_mut(id).unwrap().kind {
        *target_bodies = vec!["Part".into(); 4];
    }
    assert_eq!(doc.feature(id).unwrap().kind.input_bodies(), ["Part"; 4]);
    assert_eq!(doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Thread { .. })).count(), 1);
    let mut model = Model::new();
    model.evaluate(&doc);
    assert_eq!(model.result(id).unwrap().error, None);
    let before = model.state_before(id);
    let after = model.state();
    assert_eq!(after.bodies.len(), 1);
    assert!(!before.bodies[0].body.same_shape(&after.bodies[0].body));
    assert!(after.threads.is_empty(), "modelled threads are geometry, not tap markings");
    let base_volume = kernel::measure(&before.bodies[0].body).unwrap().volume;
    let threaded_volume = kernel::measure(&after.bodies[0].body).unwrap().volume;
    assert!(base_volume - threaded_volume > 8.0, "all four grooves remove material");
    for (x, y) in [(-8.0, -8.0), (8.0, -8.0), (-8.0, 8.0), (8.0, 8.0)] {
        // Four phases at radius 2.9 mm, inside the original 3 mm shaft. A 0.75 mm
        // ISO groove spans more than a quarter turn there, so at least one is cut.
        let probes = [Vec3::new(x + 2.9, y, 4.0), Vec3::new(x, y + 2.9, 4.0), Vec3::new(x - 2.9, y, 4.0), Vec3::new(x, y - 2.9, 4.0)];
        assert!(probes.iter().all(|p| before.bodies[0].body.classify_point(*p).unwrap() == kernel::PointClass::Inside));
        assert!(
            probes.iter().any(|p| after.bodies[0].body.classify_point(*p).unwrap() == kernel::PointClass::Outside),
            "shaft at ({x}, {y}) is threaded"
        );
        assert_eq!(after.bodies[0].body.classify_point(Vec3::new(x, y, 4.0)).unwrap(), kernel::PointClass::Inside, "the shaft core stays intact");
        assert_eq!(
            after.bodies[0].body.classify_point(Vec3::new(x, y, 1.0)).unwrap(),
            kernel::PointClass::Inside,
            "the supporting plate stays intact"
        );
    }
}

/// A bad later pick refuses the complete group rather than leaving earlier markings behind.
#[test]
fn an_invalid_grouped_thread_face_keeps_the_starting_model() {
    let (mut doc, id) = grouped_thread_document(false);
    if let FeatureKind::Thread { additional_faces, .. } = &mut doc.feature_mut(id).unwrap().kind {
        additional_faces.push(FacePick(Vec3::new(1000.0, 0.0, 0.0)));
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.result(id).unwrap().error.is_some());
    assert!(model.state().threads.is_empty());
    let before = model.state_before(id);
    assert!(model.state().bodies[0].body.same_shape(&before.bodies[0].body));
}

/// Evaluate just the grouped thread, after its unchanged support geometry is already cached.
fn grouped_thread_work(mode: kernel::Validation, refuse_trim: bool, continuous_flanks: bool) -> (Counts, Model, u64) {
    let (mut doc, id) = grouped_thread_document(true);
    if let FeatureKind::Thread { continuous_flanks: value, .. } = &mut doc.feature_mut(id).unwrap().kind {
        *value = continuous_flanks;
    }
    doc.feature_mut(id).unwrap().suppressed = true;
    let mut model = Model::new();
    model.set_validation(mode);
    model.evaluate(&doc);
    assert!(model.results.iter().all(|result| result.error.is_none()));
    doc.feature_mut(id).unwrap().suppressed = false;
    let prior = super::thread::replace_empty_trim_for_test(refuse_trim);
    let work = counted(|| model.evaluate(&doc));
    super::thread::replace_empty_trim_for_test(prior);
    assert_eq!(model.result(id).unwrap().error, None, "the grouped thread builds");
    kernel::check_kept(&model.state().bodies[0].body).unwrap();
    (work, model, id)
}

/// Separate tools share one target cut and one full native-result validity check.
#[test]
fn a_grouped_modelled_thread_cuts_and_checks_each_target_once() {
    use solidcore_record::brep::Surface;
    for continuous_flanks in [false, true] {
        let (work, model, _) = grouped_thread_work(kernel::Validation::EachFeature, false, continuous_flanks);
        assert_eq!(work.thread_cuts, 1);
        assert_eq!(work.kept_checks, 1);
        assert_eq!(work.detached_checks, 0, "reuse the check before copying a native body for another check");
        assert_eq!(work.reused_checks, 1);
        assert_eq!(work.grouped_thread_fallbacks, 0);
        let state = model.state();
        assert_eq!(state.bodies.len(), 1);
        let flanks = state.bodies[0]
            .body
            .record()
            .faces
            .iter()
            .filter(|f| f.nurbs.is_none() && matches!(f.surface, Surface::Spline { .. } | Surface::RationalSpline { .. }))
            .count();
        if continuous_flanks {
            assert_eq!(flanks, 8, "two whole flank faces on each of the four shafts");
        } else {
            assert!(flanks > 8, "historical half-turn faces remain segmented");
        }
    }
}

/// A refused grouped trim returns to the existing per-face square-stop fallback.
#[test]
fn a_grouped_thread_keeps_the_established_shoulder_fallback() {
    let (work, model, id) = grouped_thread_work(kernel::Validation::EachFeature, true, false);
    assert_eq!(work.grouped_thread_fallbacks, 1);
    assert_eq!(work.thread_cuts, 4, "the failed preparation never cut a target");
    assert_eq!(work.kept_checks + work.detached_checks, 1, "the fallback still validates its final body");
    assert_eq!(work.reused_checks, 0);
    assert!(model.result(id).unwrap().warning.as_ref().is_some_and(|w| w.contains("left uncut")));
}

/// A later replay step must still see the target in its component before feature-end stamping.
#[test]
fn modelled_thread_batching_and_fallback_keep_the_targets_component() {
    for refuse_trim in [false, true] {
        let (mut doc, id) = grouped_thread_document(true);
        let component = doc.add_component(Some("Threaded part"), 0).unwrap();
        assert_ne!(component, 0);
        for feature in &mut doc.features {
            feature.component = component;
        }
        doc.feature_mut(id).unwrap().suppressed = true;
        let mut model = Model::new();
        model.evaluate(&doc);
        assert!(model.results.iter().all(|result| result.error.is_none()));
        doc.feature_mut(id).unwrap().suppressed = false;
        let before = model.state();
        let original = before.body("Part").unwrap();
        assert_eq!(original.component, component);
        let mut after = (*before).clone();
        let _scope = super::EvalValidationGuard::new(kernel::Validation::EachFeature);
        let prior = super::thread::replace_empty_trim_for_test(refuse_trim);
        let mut result = Ok(());
        let mut warning = None;
        let work = counted(|| {
            result = kernel::validating(kernel::Validation::EachFeature, || {
                super::thread::eval(&doc, &BTreeMap::new(), doc.feature(id).unwrap(), &mut after, &mut warning)
            })
            .0;
        });
        super::thread::replace_empty_trim_for_test(prior);
        result.unwrap();
        assert_eq!(work.thread_cuts, if refuse_trim { 4 } else { 1 });
        assert_eq!(work.grouped_thread_fallbacks, usize::from(refuse_trim));
        let remade = after.body("Part").unwrap();
        assert_eq!(remade.name, original.name);
        assert_eq!(remade.feature, original.feature);
        assert_eq!(remade.component, component, "thread cuts keep their owner's identity before the next replay step");
        assert!(!remade.body.same_shape(&original.body), "the operation actually changed the target");
        kernel::check_kept(&remade.body).unwrap();
    }
}

/// Explicit Off mode keeps the original sequential orchestration and shoulder checks.
#[test]
fn grouped_thread_batching_does_not_change_explicit_off_validation() {
    let (work, _, _) = grouped_thread_work(kernel::Validation::Off, false, false);
    assert_eq!(work.thread_cuts, 4);
    assert_eq!(work.kept_checks, 4);
    assert_eq!(work.reused_checks, 0);
    assert_eq!(work.grouped_thread_fallbacks, 0);
}

/// A grouped thread over custom support bodies, with only its work counted.
fn custom_grouped_thread_work(mut doc: Document, faces: &[Vec3]) -> (Counts, Model, u64) {
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|result| result.error.is_none()));
    let mut kind = marking(faces[0]);
    if let FeatureKind::Thread { additional_faces, modeled, designation, .. } = &mut kind {
        *additional_faces = faces.iter().skip(1).copied().map(FacePick).collect();
        *modeled = true;
        *designation = Some("M6x0.75".into());
    }
    let id = doc.add_feature(kind, None).unwrap();
    let work = counted(|| model.evaluate(&doc));
    assert_eq!(model.result(id).unwrap().error, None, "the custom grouped thread builds");
    for body in &model.state().bodies {
        kernel::check_kept(&body.body).unwrap();
    }
    (work, model, id)
}

/// Distinct target bodies each keep one final cut and one complete native-result check.
#[test]
fn a_grouped_modelled_thread_cuts_and_checks_each_of_two_bodies_once() {
    let mut doc = Document::new("two threaded bodies");
    let mut faces = Vec::new();
    for (name, x) in [("First", 0.0), ("Second", 40.0)] {
        block(&mut doc, name, Vec3::new(x - 10.0, -10.0, 0.0), [20.0, 20.0, 2.0]);
        let boss = doc
            .add_feature(
                FeatureKind::Cylinder {
                    base: point_expr(Vec3::new(x, 0.0, 2.0)),
                    axis: crate::dir_expr(Vec3::Z),
                    radius: "3".into(),
                    height: "4".into(),
                    operation: Operation::Join,
                    references: Vec::new(),
                },
                None,
            )
            .unwrap();
        doc.feature_mut(boss).unwrap().participants = vec![name.into()];
        faces.push(Vec3::new(x + 3.0, 0.0, 5.0));
    }
    let (work, model, id) = custom_grouped_thread_work(doc, &faces);
    assert_eq!(work.thread_cuts, 2);
    assert_eq!(work.kept_checks, 2);
    assert_eq!(work.reused_checks, 2);
    assert_eq!(work.grouped_thread_fallbacks, 0);
    let before = model.state_before(id);
    let after = model.state();
    assert_eq!(after.bodies.len(), 2);
    for body in &after.bodies {
        assert!(!body.body.same_shape(&before.body(&body.name).unwrap().body));
    }
}

/// Overlapping exact boxes are not proof that two diagonally separated grooves can be joined.
#[test]
fn overlapping_grouped_thread_tool_boxes_keep_the_sequential_cutter() {
    let mut doc = Document::new("diagonal threads");
    block(&mut doc, "Part", Vec3::new(-10.0, -10.0, 0.0), [25.0, 25.0, 2.0]);
    let mut faces = Vec::new();
    for (x, y) in [(0.0, 0.0), (5.0, 5.0)] {
        doc.add_feature(
            FeatureKind::Cylinder {
                base: point_expr(Vec3::new(x, y, 2.0)),
                axis: crate::dir_expr(Vec3::Z),
                radius: "3".into(),
                height: "4".into(),
                operation: Operation::Join,
                references: Vec::new(),
            },
            None,
        )
        .unwrap();
        faces.push(Vec3::new(x + 3.0, y, 5.0));
    }
    let (work, model, _) = custom_grouped_thread_work(doc, &faces);
    assert_eq!(work.thread_cuts, 2, "no grouped target cut runs when the tool boxes overlap");
    assert_eq!(work.kept_checks, 2, "each ordinary shoulder cut still checks its own result");
    assert_eq!(work.reused_checks, 1, "only the last unchanged output satisfies the feature-end check");
    assert_eq!(work.grouped_thread_fallbacks, 0, "an unproven grouping is not a kernel failure");
    assert_eq!(model.state().bodies.len(), 1);
}

/// EachStep explicitly retains the existing per-face sequence and its shoulder checks.
#[test]
fn grouped_thread_batching_does_not_change_explicit_each_step_validation() {
    let (work, _, _) = grouped_thread_work(kernel::Validation::EachStep, false, false);
    assert_eq!(work.thread_cuts, 4);
    assert_eq!(work.kept_checks, 4);
    assert_eq!(work.reused_checks, 0);
    assert_eq!(work.grouped_thread_fallbacks, 0);
}

/// The validation work of a short modelled thread on a shaft joined to a plate.
fn shoulder_thread_checks(tip: crate::thread_forms::ThreadTip, continuous_flanks: bool) -> Counts {
    let mut doc = Document::new("shoulder checks");
    block(&mut doc, "Part", Vec3::new(-10.0, -10.0, 0.0), [20.0, 20.0, 2.0]);
    doc.add_feature(
        FeatureKind::Cylinder {
            base: point_expr(Vec3::new(0.0, 0.0, 2.0)),
            axis: crate::dir_expr(Vec3::Z),
            radius: "3".into(),
            height: "4".into(),
            operation: Operation::Join,
            references: Vec::new(),
        },
        None,
    )
    .unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|result| result.error.is_none()));
    let mut kind = marking(Vec3::new(3.0, 0.0, 5.0));
    if let FeatureKind::Thread { modeled, designation, tip: value, continuous_flanks: flanks, .. } = &mut kind {
        *modeled = true;
        *designation = Some("M6x0.75".into());
        *value = tip;
        *flanks = continuous_flanks;
    }
    let id = doc.add_feature(kind, None).unwrap();
    let work = counted(|| model.evaluate(&doc));
    let result = model.result(id).unwrap();
    assert_eq!(result.error, None, "the thread builds");
    assert!(!super::checked_thread_output(&result.state.bodies[0].body), "the receipt ends with the feature");
    // The retained native result passed inside the feature. Its reconstructed record must also pass.
    kernel::check_kept(&result.state.bodies[0].body).unwrap();
    work
}

/// A successful shoulder check already satisfies EachFeature's native-result check contract.
#[test]
fn a_modelled_thread_reuses_its_successful_shoulder_check() {
    for continuous_flanks in [false, true] {
        let work = shoulder_thread_checks(crate::thread_forms::ThreadTip::None, continuous_flanks);
        assert_eq!(work.kept_checks, 1, "check the native shoulder result once");
        assert_eq!(work.detached_checks, 0);
        assert_eq!(work.reused_checks, 1, "the unchanged final output reuses that success");
    }
}

/// A tip boolean creates another result; the shoulder's success cannot validate that result.
#[test]
fn a_chamfered_thread_still_checks_its_changed_output() {
    let work = shoulder_thread_checks(crate::thread_forms::ThreadTip::Both, false);
    assert_eq!(work.kept_checks + work.detached_checks, 2, "check the shoulder and the later chamfered result");
    assert_eq!(work.reused_checks, 0);
}

/// Continuous flanks cut the chamfer first, so the checked shoulder result includes the tip.
#[test]
fn a_continuous_chamfered_thread_reuses_its_final_shoulder_check() {
    let work = shoulder_thread_checks(crate::thread_forms::ThreadTip::Both, true);
    assert_eq!(work.kept_checks, 1);
    assert_eq!(work.detached_checks, 0);
    assert_eq!(work.reused_checks, 1);
}

/// Replaying a thread on one body retains one latest receipt, while other bodies keep theirs.
#[test]
fn thread_check_receipts_retain_only_the_latest_shape_per_body() {
    use super::{EvalValidationGuard, checked_thread_output, remember_checked_thread_output};
    let original = kernel::box_solid(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)).unwrap();
    let replacement = kernel::box_solid(Vec3::ZERO, Vec3::new(3.0, 2.0, 2.0)).unwrap();
    let other = kernel::box_solid(Vec3::new(10.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0)).unwrap();
    let _scope = EvalValidationGuard::new(kernel::Validation::EachFeature);
    let retained = || {
        super::CHECKED_THREAD_OUTPUTS.with(|cell| {
            let checked = cell.take();
            let count = checked.as_ref().map_or(0, BTreeMap::len);
            cell.set(checked);
            count
        })
    };
    remember_checked_thread_output("First", &original);
    remember_checked_thread_output("Second", &other);
    assert_eq!(retained(), 2);
    for _ in 0..16 {
        remember_checked_thread_output("First", &replacement);
        assert_eq!(retained(), 2, "updates replace the previous receipt for this body");
    }
    assert!(!checked_thread_output(&original), "the earlier shape is no longer retained");
    assert!(checked_thread_output(&replacement.clone()));
    assert!(checked_thread_output(&other), "another body keeps its own receipt");
    let equal_geometry = kernel::box_solid(Vec3::ZERO, Vec3::new(3.0, 2.0, 2.0)).unwrap();
    assert!(!checked_thread_output(&equal_geometry), "equal geometry is not the checked record");
}

/// Successful checks belong to one exact shape and one feature, including nested and failed work.
#[test]
fn thread_check_receipts_are_scoped_to_one_feature_and_validation_mode() {
    use super::{EvalValidationGuard, checked_thread_output, remember_checked_thread_output};
    use kernel::Validation;
    let body = kernel::box_solid(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)).unwrap();
    let other = kernel::box_solid(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)).unwrap();
    let original_mode = super::EVAL_VALIDATION.get();
    remember_checked_thread_output("Part", &body);
    assert!(!checked_thread_output(&body), "there is no receipt outside a feature");
    {
        let _outer = EvalValidationGuard::new(Validation::EachFeature);
        remember_checked_thread_output("Part", &body);
        assert!(checked_thread_output(&body.clone()));
        assert!(!checked_thread_output(&other), "equal geometry is not the same result");
        for mode in [Validation::EachStep, Validation::Off, Validation::EachFeature] {
            let _inner = EvalValidationGuard::new(mode);
            assert!(!checked_thread_output(&body), "the inner feature has no outer receipts");
            remember_checked_thread_output("Other", &other);
            assert_eq!(checked_thread_output(&other), mode == Validation::EachFeature);
        }
        assert!(checked_thread_output(&body));
        assert!(!checked_thread_output(&other));
        let failed: std::result::Result<(), &str> = {
            let _inner = EvalValidationGuard::new(Validation::EachFeature);
            remember_checked_thread_output("Other", &other);
            Err("feature failed")
        };
        assert!(failed.is_err());
        assert!(checked_thread_output(&body));
        assert!(!checked_thread_output(&other), "failed feature receipts do not escape");
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _inner = EvalValidationGuard::new(Validation::EachFeature);
            remember_checked_thread_output("Other", &other);
            panic!("feature unwound");
        }));
        assert!(unwound.is_err());
        assert!(checked_thread_output(&body));
        assert!(!checked_thread_output(&other), "unwound feature receipts do not escape");
        let on_worker = body.clone();
        std::thread::spawn(move || assert!(!checked_thread_output(&on_worker), "worker scopes stay separate")).join().unwrap();
    }
    assert_eq!(super::EVAL_VALIDATION.get(), original_mode);
    assert!(!checked_thread_output(&body));
}

/// A tap marking looks for its cylinder in the bodies near it only: each body searched was
/// tessellated again, so many markings in a design of many bodies took quadratic time (#10227).
#[test]
fn a_tap_marking_searches_only_the_bodies_near_it() {
    let mut doc = Document::new("far");
    shaft(&mut doc, "Shaft", Vec3::ZERO, 3.0, 6.0);
    for i in 0..5 {
        block(&mut doc, &format!("Far{i}"), Vec3::new(200.0 * (i + 1) as f64, 0.0, 0.0), [50.0, 40.0, 30.0]);
    }
    let (work, threads) = marking_work(&mut doc, marking(Vec3::new(3.0, 0.0, 3.0)));
    assert_eq!(work.cylinder_search, 1, "only the shaft is searched");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].body, "Shaft");
    assert_eq!(threads[0].designation, "M6");
    assert!((threads[0].radius - 3.0).abs() < 1e-6 && !threads[0].internal);

    // A tapped hole in a plate among far bodies: only the plate is searched for its wall.
    let mut doc = Document::new("tapped");
    block(&mut doc, "Plate", Vec3::ZERO, [60.0, 60.0, 12.0]);
    for i in 0..5 {
        block(&mut doc, &format!("Far{i}"), Vec3::new(200.0 * (i + 1) as f64, 0.0, 0.0), [50.0, 40.0, 30.0]);
    }
    let hole = FeatureKind::Hole {
        position: FacePick(Vec3::new(30.0, 30.0, 12.0)),
        at: None,
        direction: Dir(-Vec3::Z),
        diameter: "5".into(),
        depth: Some("10".into()),
        hole: HoleKind::default(),
        points: None,
        thread: Some("M6".into()),
        thread_depth: None,
        hand: crate::thread_forms::ThreadHand::Right,
        thread_class: None,
        modeled: false,
        ease_out: None,
        to: None,
        references: Vec::new(),
        old_surface_start: false,
    };
    let (work, threads) = marking_work(&mut doc, hole);
    assert_eq!(work.cylinder_search, 1, "only the plate is searched");
    assert_eq!(work.tapped_wall, 1, "only the plate's faces are read for the wall");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].body, "Plate");
    assert!(threads[0].internal && (threads[0].radius - 2.5).abs() < 1e-6);
}

/// Bodies within reach of a tap marking are still searched: the shaft it lies on, a block 3 mm
/// off (within a twentieth of the block's size, where the cylinder search itself looks), and one
/// 3.6 mm off (just past it, but within the margin the boxes keep); a block 4.5 mm off is not.
/// The marking stays on the shaft.
#[test]
fn a_tap_marking_still_searches_the_bodies_within_its_reach() {
    let mut doc = Document::new("near");
    shaft(&mut doc, "Shaft", Vec3::ZERO, 3.0, 6.0);
    // Each block is 50 x 40 x 30 (a twentieth of its size is 3.54 mm), its face x = 3 + gap.
    for (name, gap) in [("Close", 3.0), ("Edge", 3.6), ("Clear", 4.5)] {
        block(&mut doc, name, Vec3::new(3.0 + gap, -20.0, 0.0), [50.0, 40.0, 30.0]);
    }
    let (work, threads) = marking_work(&mut doc, marking(Vec3::new(3.0, 0.0, 3.0)));
    assert_eq!(work.cylinder_search, 3, "the shaft, Close and Edge are searched");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].body, "Shaft");

    // A pin in a hole a hair wider: a marking on the pin, and one on the hole's wall.
    for (p, body, internal) in [(Vec3::new(2.0, 0.0, 5.0), "Pin", false), (Vec3::new(2.5, 0.0, 5.0), "Plate", true)] {
        let mut doc = Document::new("pin");
        block(&mut doc, "Plate", Vec3::new(-20.0, -20.0, 0.0), [40.0, 40.0, 10.0]);
        let bore = FeatureKind::Cylinder {
            base: point_expr(Vec3::new(0.0, 0.0, -1.0)),
            axis: crate::dir_expr(Vec3::Z),
            radius: "2.5".into(),
            height: "12".into(),
            operation: Operation::Cut,
            references: Vec::new(),
        };
        doc.add_feature(bore, None).unwrap();
        shaft(&mut doc, "Pin", Vec3::ZERO, 2.0, 10.0);
        let mut kind = marking(p);
        if let FeatureKind::Thread { designation, .. } = &mut kind {
            *designation = None;
        }
        let (work, threads) = marking_work(&mut doc, kind);
        assert_eq!(work.cylinder_search, 2, "both bodies are searched for {body}");
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].body, body);
        assert_eq!(threads[0].internal, internal);
    }
}

/// A tap marking in a part looks in that part's own bodies first: the bodies of other parts are
/// in their own coordinates, so a part built alike has a face at the same point, and searching
/// it too tessellated every such body for nothing.
#[test]
fn a_tap_marking_in_a_part_searches_that_parts_bodies_first() {
    let mut doc = Document::new("parts");
    let mut parts = Vec::new();
    for name in ["A", "B"] {
        let comp = doc.add_component(Some(name), 0).unwrap();
        shaft(&mut doc, &format!("Shaft {name}"), Vec3::ZERO, 3.0, 6.0);
        doc.features.last_mut().unwrap().component = comp;
        parts.push(comp);
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    let id = doc.add_feature(marking(Vec3::new(3.0, 0.0, 3.0)), None).unwrap();
    doc.feature_mut(id).unwrap().component = parts[1];
    let work = counted(|| model.evaluate(&doc));
    assert_eq!(model.result(id).unwrap().error, None, "the marking builds");
    assert_eq!(work.cylinder_search, 1, "only part B's shaft is searched");
    let threads = &model.state().threads;
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].body, "Shaft B");
}

/// A flat sheet of the default rule (2.5 mm steel) on a `w` x `h` rectangle from `corner` on XY.
fn flat_sheet(doc: &mut Document, corner: Vec2, w: f64, h: f64) {
    let mut sk = Sketch::new();
    let p = [corner, corner + Vec2::new(w, 0.0), corner + Vec2::new(w, h), corner + Vec2::new(0.0, h)].map(|q| sk.add_point(q, None).unwrap());
    for k in 0..4 {
        sk.add_line_pts(p[k], p[(k + 1) % 4], None).unwrap();
    }
    let sketch = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    doc.add_feature(FeatureKind::SheetBase { sketch: SketchId(sketch), profiles: ProfileSel::All, rule: None, flip: false }, None).unwrap();
}

/// A hole of `diameter` at `position`, drilling along `direction` through all.
fn hole(position: Vec3, direction: Vec3, diameter: f64) -> FeatureKind {
    serde_json::from_value(serde_json::json!({
        "type": "hole", "position": [position.x, position.y, position.z], "direction": [direction.x, direction.y, direction.z],
        "diameter": diameter.to_string(),
    }))
    .unwrap()
}

/// The sheet metal bodies of a model.
fn sheet_bodies(model: &Model) -> Vec<String> {
    model.state().surface_sheets.iter().map(|s| s.body.clone()).collect()
}

/// A cut leaves the sheets it cannot reach as they were, without checking that it would leave
/// each one unchanged (#10226).
#[test]
fn a_cut_leaves_sheets_it_cannot_reach_as_they_were() {
    let mut doc = Document::new("far sheets");
    for i in 0..5 {
        flat_sheet(&mut doc, Vec2::new(200.0 * i as f64, 0.0), 150.0, 100.0);
    }
    block(&mut doc, "Plate", Vec3::new(0.0, -200.0, 0.0), [60.0, 60.0, 12.0]);
    let work = cut_work(&mut doc, cutter(Vec3::new(30.0, -170.0, -1.0), 3.0, 20.0));
    assert_eq!(work.cut, 1, "only the plate goes through a boolean");
    assert_eq!(work.kept, sheet_bodies(&work.model), "every sheet is left as it was");
    assert!(volume(&work.model, "Plate") < 43_200.0 - 300.0, "and the plate is cut");
}

/// A tapped hole reads for its wall the faces of the bodies its drill line comes near: the plate
/// it is drilled in, a block far below on the same line, and one whose side lies a radius off the
/// line; not a block beside it. The marking stays in the plate's hole.
#[test]
fn a_tapped_hole_reads_the_bodies_along_its_drill_line() {
    let mut doc = Document::new("line");
    block(&mut doc, "Plate", Vec3::ZERO, [60.0, 60.0, 12.0]);
    block(&mut doc, "Below", Vec3::new(20.0, 20.0, -150.0), [20.0, 20.0, 20.0]);
    block(&mut doc, "Grazing", Vec3::new(32.5, 20.0, -60.0), [20.0, 20.0, 20.0]);
    block(&mut doc, "Beside", Vec3::new(50.0, 20.0, -60.0), [20.0, 20.0, 20.0]);
    let hole = FeatureKind::Hole {
        position: FacePick(Vec3::new(30.0, 30.0, 12.0)),
        at: None,
        direction: Dir(-Vec3::Z),
        diameter: "5".into(),
        depth: Some("10".into()),
        hole: HoleKind::default(),
        points: None,
        thread: Some("M6".into()),
        thread_depth: None,
        hand: crate::thread_forms::ThreadHand::Right,
        thread_class: None,
        modeled: false,
        ease_out: None,
        to: None,
        references: Vec::new(),
        old_surface_start: false,
    };
    let (work, threads) = marking_work(&mut doc, hole);
    assert_eq!(work.tapped_wall, 3, "Plate, Below and Grazing are read");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].body, "Plate");
    assert!(threads[0].internal && (threads[0].radius - 2.5).abs() < 1e-6);
}

/// A hole leaves the sheets it cannot reach as they were, without drilling, checking or cutting
/// each one: in a design of many sheet parts that was most of a hole's time (#10226).
#[test]
fn a_hole_leaves_sheets_it_cannot_reach_as_they_were() {
    let mut doc = Document::new("far sheets");
    for i in 0..5 {
        flat_sheet(&mut doc, Vec2::new(200.0 * i as f64, 0.0), 150.0, 100.0);
    }
    block(&mut doc, "Plate", Vec3::new(0.0, -200.0, 0.0), [60.0, 60.0, 12.0]);
    let work = cut_work(&mut doc, hole(Vec3::new(30.0, -170.0, 12.0), Vec3::new(0.0, 0.0, -1.0), 6.0));
    let sheets = sheet_bodies(&work.model);
    assert_eq!(sheets.len(), 5);
    assert_eq!(work.cut, 1, "only the plate goes through a boolean");
    assert_eq!(work.kept, sheets, "every sheet is left as it was");
    assert!(volume(&work.model, "Plate") < 43_200.0 - 300.0, "and the plate is drilled");
}

/// A hole drilled into a sheet's rim still bores that sheet, beside sheets it cannot reach. The
/// sheet takes the drill as a bore of the panel it runs along and builds its body again (see
/// `surface_hole_cut`), so no body goes through a boolean: on main the flat sheet was cut and read
/// back, one boolean. The far sheets are left as they were.
#[test]
fn a_hole_into_a_rim_still_bores_its_sheet_beside_far_ones() {
    let mut doc = Document::new("rim");
    for i in 0..4 {
        flat_sheet(&mut doc, Vec2::new(200.0 * i as f64, 0.0), 60.0, 40.0);
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    // The sheet the drill runs along: the one at the origin, which holds the middle of its run, 30
    // along it and halfway through the 2.5 mm.
    let mid = Vec3::new(30.0, 20.0, 1.25);
    let holds = |m: &Model, name: &str| m.state().body(name).is_some_and(|b| matches!(b.body.classify_point(mid), Ok(kernel::PointClass::Inside)));
    let near = sheet_bodies(&model).into_iter().find(|b| holds(&model, b.as_str())).expect("the sheet at the origin");
    let whole = volume(&model, &near);
    // Into its rim at x = 60, along it (-x) through all, 1.5 across, halfway through its thickness.
    let d = 1.5;
    let work = cut_work(&mut doc, hole(Vec3::new(60.0, 20.0, 1.25), Vec3::new(-1.0, 0.0, 0.0), d));
    assert_eq!(work.cut, 0, "the sheet bores itself: no body goes through a boolean");
    let mut far: Vec<String> = sheet_bodies(&work.model).into_iter().filter(|b| *b != near).collect();
    let mut kept = work.kept.clone();
    far.sort();
    kept.sort();
    assert!(far.len() == 3 && kept == far, "the far sheets are left as they were: {kept:?} of {far:?}");
    let state = work.model.state();
    let sheet = state.surface_sheets.iter().find(|s| s.body == near).expect("the drilled sheet");
    assert!(sheet.bores.len() == 1 && (sheet.bores[0].radius - d / 2.0).abs() < 1e-12, "kept as a bore of the sheet: {:?}", sheet.bores);
    // It takes the bore's cylinder through the plate's 60: π (d/2)² 60 = 106.03 mm³ (to 2 %, as on
    // main).
    let removed = whole - volume(&work.model, &near);
    let bore = std::f64::consts::PI * (d / 2.0).powi(2) * 60.0;
    assert!((removed - bore).abs() < 0.02 * bore, "removed {removed} of {bore}");
}

/// A flange on a sheet made from surfaces lays the sheet's flat out once, however many flanges
/// the sheet already has: naming its pick (the model's name for the edge), finding the sheet and
/// the edge again by that name, and keying the panels it makes lay nothing out. Each of them laid
/// the whole sheet out again, five flats for every flange built (#6748).
#[test]
fn a_flange_lays_its_sheet_out_once() {
    let mut doc = Document::new("flanges");
    flat_sheet(&mut doc, Vec2::new(0.0, 0.0), 150.0, 100.0);
    let mut model = Model::new();
    model.evaluate(&doc);
    let flange = |p: Vec3| FeatureKind::SheetFlange {
        edges: vec![FlangeEdge::at(p)],
        height: "20 mm".into(),
        angle: "90 deg".into(),
        radius: None,
        position: "inside".into(),
        flip: false,
        body: None,
        corner_relief_shape: None,
        corner_relief: None,
        height_from: None,
    };
    for p in [Vec3::new(75.0, 0.0, 2.5), Vec3::new(75.0, 100.0, 2.5), Vec3::new(0.0, 50.0, 2.5), Vec3::new(150.0, 50.0, 2.5)] {
        // The pick is named as the engine names it, on the model as it is.
        let mut names = Vec::new();
        let naming = counted(|| names = crate::sheet::sheet_edge_names(&model.state(), &[p])).flats;
        assert!(names.first().is_some_and(|n| n.contains('|')), "the model's name for the edge: {names:?}");
        let id = doc.add_feature(flange(p), None).unwrap();
        doc.feature_mut(id).unwrap().edge_names = names;
        let built = counted(|| model.evaluate(&doc)).flats;
        assert_eq!(model.result(id).unwrap().error, None, "the flange at {p:?} builds");
        assert_eq!((naming, built), (0, 1), "the flange at {p:?}: flats laid out naming its pick, and building it");
    }
}

/// A 1.5 mm disc of 50 mm radius with a 3.5 mm through hole at `radius` on X, patterned `count`
/// times around Z (#9007). The pattern's id.
fn disc_with_hole_pattern(doc: &mut Document, radius: f64, count: usize) -> u64 {
    let disc = FeatureKind::Cylinder {
        base: point_expr(Vec3::ZERO),
        axis: crate::dir_expr(Vec3::Z),
        radius: "50".into(),
        height: "1.5".into(),
        operation: Operation::NewBody,
        references: Vec::new(),
    };
    doc.add_feature(disc, Some("Disc")).unwrap();
    let hole: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "hole", "position": [radius, 0, 1.5], "direction": [0, 0, -1], "diameter": "3.5",
    }))
    .unwrap();
    let hole = doc.add_feature(hole, Some("Hole")).unwrap();
    hole_pattern(doc, hole, count)
}

fn hole_pattern(doc: &mut Document, hole: u64, count: usize) -> u64 {
    let pattern: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "pattern", "features": [hole],
        "pattern": {"type": "circular", "origin": [0, 0, 0], "axis": [0, 0, 1], "count": count.to_string(), "angle": "360 deg"},
    }))
    .unwrap();
    doc.add_feature(pattern, None).unwrap()
}

fn disc_volume(model: &Model) -> f64 {
    let st = model.state();
    assert_eq!(st.bodies.len(), 1);
    kernel::measure(&st.bodies[0].body).unwrap().volume
}

#[test]
fn a_circular_pattern_of_a_through_hole_cuts_its_copies_at_once() {
    // The copies' tools run twice the disc's size along Z, so the kernel's box test, which
    // grows each box by a share of the tool's size, finds them meeting (#9007); they are still
    // 4.3 mm apart, centre to centre, and are cut in one go. No copy is intersected with the
    // disc to see whether it takes material: each passes through the disc's material.
    let mut doc = Document::new("sieve");
    let pattern = disc_with_hole_pattern(&mut doc, 20.0, 29);
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    assert_eq!((work.cut_tools, work.miss_intersects), (1, 0), "{work:?}");
    let hole = std::f64::consts::PI * 1.75 * 1.75 * 1.5;
    let disc = std::f64::consts::PI * 2500.0 * 1.5;
    let v = disc_volume(&model);
    assert!((v - (disc - 29.0 * hole)).abs() < 1e-3 * hole, "{v}");
    // The same copies again, landing in the holes cut already: they are intersected together, in
    // one boolean, and each still counts as a copy that removes no material.
    let hole_id = doc.features.iter().find(|f| f.name == "Hole").unwrap().id;
    hole_pattern(&mut doc, hole_id, 29);
    let work = counted(|| model.evaluate(&doc));
    let again = model.results.last().unwrap();
    assert!(again.error.is_none(), "{again:?}");
    // (The kernel may add a note on a retry of a boolean on faces that lie on each other.)
    assert!(again.warning.as_deref().is_some_and(|w| w.starts_with("28 of 28 cut copies remove no material")), "{again:?}");
    assert_eq!(work.miss_intersects, 1, "{work:?}");
    assert!((disc_volume(&model) - v).abs() < 1e-3 * hole);
    assert!(model.results.iter().any(|r| r.id == pattern && r.warning.is_none()));
}

/// The copies of a pattern that remove no material are tested against the target in one boolean,
/// not one a copy (#18396): a body with many faces makes every boolean cost seconds, and a
/// design of many patterns pays that a copy at a time.
#[test]
fn the_copies_that_remove_no_material_are_tested_in_one_boolean() {
    // The same copies again, landing in the holes cut already: they remove no material, and
    // the kernel is asked about all of them at once. The count of the copies that remove no
    // material does not change: all 28 still count.
    let mut doc = Document::new("cut nothing");
    disc_with_hole_pattern(&mut doc, 20.0, 29);
    let mut model = Model::new();
    counted(|| model.evaluate(&doc));
    let hole_id = doc.features.iter().find(|f| f.name == "Hole").unwrap().id;
    hole_pattern(&mut doc, hole_id, 29);
    let work = counted(|| model.evaluate(&doc));
    let again = model.results.last().unwrap();
    assert!(again.error.is_none(), "{again:?}");
    assert!(again.warning.as_deref().is_some_and(|w| w.starts_with("28 of 28 cut copies remove no material")), "{again:?}");
    assert_eq!(work.miss_intersects, 1, "{work:?}");
}

#[test]
fn copies_of_a_hole_that_come_close_are_cut_one_by_one() {
    // At 8.5 mm radius, 16 copies of a 3.5 mm hole overlap their neighbours: they are not apart,
    // and each is cut on its own, as before.
    let mut doc = Document::new("crowded");
    disc_with_hole_pattern(&mut doc, 8.5, 16);
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    assert_eq!((work.cut_tools, work.miss_intersects), (15, 0), "{work:?}");
    // At 9 mm they come within 0.012 mm of each other without touching: their exact shapes are
    // apart, so they are cut in one go, and all 16 holes are cut.
    let mut doc = Document::new("close");
    disc_with_hole_pattern(&mut doc, 9.0, 16);
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    assert_eq!((work.cut_tools, work.miss_intersects), (1, 0), "{work:?}");
    let hole = std::f64::consts::PI * 1.75 * 1.75 * 1.5;
    let disc = std::f64::consts::PI * 2500.0 * 1.5;
    let v = disc_volume(&model);
    assert!((v - (disc - 16.0 * hole)).abs() < 1e-3 * hole, "{v}");
}

#[test]
fn many_copies_cut_in_groups_leave_what_one_cut_leaves() {
    // 48 holes on a 40 mm circle, 5.2 mm apart: the 47 copies are more than one joined tool
    // takes, so they are cut as two (#8765). The disc comes out as cutting all of them in one go
    // leaves it: the same volume, the same faces, named the same.
    let disc = |limit: usize| {
        super::JOINED_TOOLS_LIMIT.with(|l| l.set(limit));
        let mut doc = Document::new("perforated");
        disc_with_hole_pattern(&mut doc, 40.0, 48);
        let mut model = Model::new();
        let work = counted(|| model.evaluate(&doc));
        super::JOINED_TOOLS_LIMIT.with(|l| l.set(super::MAX_JOINED_TOOLS));
        assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
        let st = model.state();
        assert_eq!(st.bodies.len(), 1);
        let mut names = crate::naming::face_names(&st.bodies[0]).to_vec();
        names.sort();
        (disc_volume(&model), st.bodies[0].body.face_count(), names, work.cut_tools)
    };
    let (volume, faces, names, tools) = disc(super::MAX_JOINED_TOOLS);
    let (one_volume, one_faces, one_names, one_tools) = disc(usize::MAX);
    assert_eq!((tools, one_tools), (2, 1));
    let hole = std::f64::consts::PI * 1.75 * 1.75 * 1.5;
    assert!((volume - one_volume).abs() < 1e-6 * hole, "{volume} vs {one_volume}");
    assert!((one_volume - (std::f64::consts::PI * 2500.0 * 1.5 - 48.0 * hole)).abs() < 1e-3 * hole, "{one_volume}");
    assert_eq!(faces, one_faces);
    assert_eq!(names, one_names);
}

/// A 60 x 60 x 3 plate cut through by one sketch of `n` x `n` separate 2 mm squares, 4 mm apart,
/// with at most `shells` shells in one joined tool: the plate's volume, its face count and the
/// extrude's error.
fn plate_cut_by_squares(n: usize, shells: usize) -> (f64, usize, Option<String>) {
    super::JOINED_SHELLS_LIMIT.with(|l| l.set(shells));
    let mut doc = Document::new("squares");
    let kind = FeatureKind::Box {
        corner: point_expr(Vec3::ZERO),
        length: "60".into(),
        width: "60".into(),
        height: "3".into(),
        operation: Operation::NewBody,
        centred: false,
    };
    doc.add_feature(kind, Some("Plate")).unwrap();
    let mut sk = Sketch::new();
    for (i, j) in (0..n).flat_map(|i| (0..n).map(move |j| (i, j))) {
        let (x, y) = (2.0 + 4.0 * i as f64, 2.0 + 4.0 * j as f64);
        let p = [(x, y), (x + 2.0, y), (x + 2.0, y + 2.0), (x, y + 2.0)].map(|(x, y)| sk.add_point(Vec2::new(x, y), None).unwrap());
        for k in 0..4 {
            sk.add_line_pts(p[k], p[(k + 1) % 4], None).unwrap();
        }
    }
    let sketch = doc.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, None).unwrap();
    let kind = FeatureKind::Extrude {
        thin: None,
        sketch: SketchId(sketch),
        profiles: ProfileSel::All,
        extent: serde_json::from_value(serde_json::json!({"distance": "3"})).unwrap(),
        operation: Operation::Cut,
        targets: vec![],
    };
    doc.add_feature(kind, Some("Squares")).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    super::JOINED_SHELLS_LIMIT.with(|l| l.set(super::MAX_JOINED_SHELLS));
    let error = model.results.iter().find_map(|r| r.error.clone());
    let st = model.state();
    let faces = st.bodies.first().map_or(0, |b| b.body.face_count());
    (disc_volume(&model), faces, error)
}

#[test]
fn profiles_apart_past_one_tools_shells_cut_in_groups() {
    // 25 squares apart, joined into tools of at most 8 shells: four tools, each cut in turn, as
    // more than 1024 squares are cut with the kernel's own limit (#20398: 1369 squares ended in
    // "pieces: portable B-rep: shell count"). The plate comes out as one cut of all of them
    // leaves it.
    let (volume, faces, error) = plate_cut_by_squares(5, 8);
    let (one_volume, one_faces, one_error) = plate_cut_by_squares(5, usize::MAX);
    assert_eq!((error, one_error), (None, None));
    let plate = 60.0 * 60.0 * 3.0 - 25.0 * 4.0 * 3.0;
    assert!((volume - plate).abs() < 1e-6 * plate && (one_volume - plate).abs() < 1e-6 * plate, "{volume} {one_volume} vs {plate}");
    assert_eq!((faces, one_faces), (6 + 25 * 4, 6 + 25 * 4));
}

#[test]
fn more_profiles_apart_than_one_record_holds_join_into_several_tools() {
    // 1100 boxes apart, each one shell: more than one body's record takes, so they come out as
    // two tools, each of at most that many shells, together all the boxes' faces.
    let tools: Vec<crate::lineage::Traced> = (0..1100)
        .map(|i| {
            let (x, y) = (3.0 * (i % 40) as f64, 3.0 * (i / 40) as f64);
            crate::lineage::Traced::unnamed(kernel::box_solid(Vec3::new(x, y, 0.0), Vec3::new(x + 1.0, y + 1.0, 1.0)).unwrap())
        })
        .collect();
    let bodies: Vec<kernel::Body> = tools.iter().map(|t| t.body.clone()).collect();
    let one = kernel::join_apart(&bodies).map(|_| ()).map_err(|e| e.to_string());
    assert!(one.as_ref().is_err_and(|e| e.contains("shell count")), "{one:?}");
    let joined = super::join_apart_tools(tools).unwrap();
    let shells: Vec<usize> = joined.iter().map(|t| t.body.record().shells.len()).collect();
    assert_eq!(shells, vec![550, 550]);
    assert_eq!(joined.iter().map(|t| t.body.face_count()).sum::<usize>(), 6 * 1100);
}

/// A ring (25 mm outside, 16 mm bore, 55 mm high) with a blind groove, 8 mm wide and 30 mm deep,
/// cut into its bore from below, in a circular pattern of ten about Z: their boxes, grown as the
/// kernel grows them, meet, their exact shapes are 1.9 mm apart. The ring's faces counted as its
/// surfaces are, and the work.
fn grooved_ring_faces() -> (usize, Counts) {
    let mut doc = Document::new("grooves");
    let cylinder = |base: Vec3, radius: &str, height: &str, operation: Operation| FeatureKind::Cylinder {
        base: point_expr(base),
        axis: crate::dir_expr(Vec3::Z),
        radius: radius.into(),
        height: height.into(),
        operation,
        references: Vec::new(),
    };
    doc.add_feature(cylinder(Vec3::ZERO, "25", "55", Operation::NewBody), Some("Ring")).unwrap();
    doc.add_feature(cylinder(Vec3::new(0.0, 0.0, -1.0), "16", "57", Operation::Cut), Some("Bore")).unwrap();
    let groove = doc.add_feature(cylinder(Vec3::new(16.0, 0.0, -1.0), "4", "31", Operation::Cut), Some("Groove")).unwrap();
    hole_pattern(&mut doc, groove, 10);
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    let st = model.state();
    assert_eq!(st.bodies.len(), 1);
    (kernel::measure(&st.bodies[0].body).unwrap().merged.faces, work)
}

#[test]
fn copies_that_notch_the_same_round_face_cut_at_once_leave_the_faces_of_one_by_one() {
    // The ten grooves notch the same round bore. Cut all in one go, they leave it in as many
    // faces as cutting them one after another does (#9007).
    let (patterned, work) = grooved_ring_faces();
    let cut = |a: kernel::Body, b: &kernel::Body| kernel::boolean(&a, b, kernel::BoolOp::Cut).unwrap().unwrap();
    let ring =
        cut(kernel::cylinder(Vec3::ZERO, Vec3::Z, 25.0, 55.0).unwrap(), &kernel::cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 16.0, 57.0).unwrap());
    let groove = kernel::cylinder(Vec3::new(16.0, 0.0, -1.0), Vec3::Z, 4.0, 31.0).unwrap();
    let one_by_one = (0..10).fold(ring, |ring, k| {
        let copy = kernel::transform(&groove, Vec3::ZERO, Vec3::ZERO, Vec3::Z, (36.0 * k as f64).to_radians()).unwrap();
        cut(ring, &copy)
    });
    assert_eq!(patterned, kernel::measure(&one_by_one).unwrap().merged.faces);
    assert_eq!(work.cut_tools, 1, "{work:?}");
}

/// A shaft (10 mm radius, 40 mm high) with a helical groove cut into its wall, and the groove
/// in a circular pattern of `count` about Z: the shaft's volume and the work the pattern cost.
fn shaft_with_coil_pattern(doc: &mut Document, count: Option<usize>) -> (f64, Counts) {
    let shaft = FeatureKind::Cylinder {
        base: point_expr(Vec3::ZERO),
        axis: crate::dir_expr(Vec3::Z),
        radius: "10".into(),
        height: "40".into(),
        operation: Operation::NewBody,
        references: Vec::new(),
    };
    doc.add_feature(shaft, Some("Shaft")).unwrap();
    // A groove that climbs 120 mm a turn around the shaft's wall: over the shaft's height it
    // runs a third of a turn, and the copies of it round the shaft are a twelfth of a turn
    // apart.
    let groove: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "coil", "base": [0, 0, 0], "axis": [0, 0, 1], "diameter": "20", "pitch": "120",
        "turns": "1", "section_size": "0.5", "section": "circular", "operation": "cut",
    }))
    .unwrap();
    let groove = doc.add_feature(groove, Some("Groove")).unwrap();
    if let Some(count) = count {
        let pattern: FeatureKind = serde_json::from_value(serde_json::json!({
            "type": "pattern", "features": [groove],
            "pattern": {"type": "circular", "origin": [0, 0, 0], "axis": [0, 0, 1], "count": count.to_string(), "angle": "360 deg"},
        }))
        .unwrap();
        doc.add_feature(pattern, Some("Grooves")).unwrap();
    }
    let mut model = Model::new();
    let work = counted(|| model.evaluate(doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    (disc_volume(&model), work)
}

/// A circular pattern of a coil cut (#17632): the copies of a helical groove are told apart
/// with one union of all of them, not with a union for every pair of them, and they are cut in
/// one go.
#[test]
fn a_circular_pattern_of_a_coil_cut_cuts_its_copies_at_once() {
    // The groove wraps round the shaft, so every copy's box is the whole cylinder and the
    // copies' boxes meet: their exact shapes decide whether they are apart. Telling the
    // forty-nine copies of a pattern of fifty apart took a union for each of their 1176 pairs,
    // and minutes on the design of #17632. The copies are cut in two groups, and each group is
    // told apart with one union.
    let mut doc = Document::new("grooved shaft");
    let (volume, work) = shaft_with_coil_pattern(&mut doc, Some(50));
    assert_eq!((work.cut_tools, work.apart_unions), (2, 2), "{work:?}");
    // The shaft less fifty grooves: fifty times what one groove takes away.
    let mut doc = Document::new("one groove");
    let (one, _) = shaft_with_coil_pattern(&mut doc, None);
    let shaft = std::f64::consts::PI * 100.0 * 40.0;
    let taken = shaft - volume;
    assert!((taken - 50.0 * (shaft - one)).abs() < 0.02 * taken, "{taken} against {}", 50.0 * (shaft - one));
}

/// A 100 x 100 x 2 plate with a 3 mm hole at (5, 5) drilled from its top, in a 10 x 9 grid on a
/// 10 mm pitch: the plate's volume, the holes' and the work.
fn plate_with_hole_grid(hole: serde_json::Value) -> (f64, f64, Counts) {
    let (model, work) = plate_with_hole_grid_model(hole);
    (disc_volume(&model), std::f64::consts::PI * 1.5 * 1.5 * 2.0, work)
}

/// [`plate_with_hole_grid`]'s model, and the work.
fn plate_with_hole_grid_model(hole: serde_json::Value) -> (Model, Counts) {
    let mut doc = Document::new("perforated");
    let plate: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "box", "corner": [0, 0, 0], "length": "100", "width": "100", "height": "2",
    }))
    .unwrap();
    doc.add_feature(plate, Some("Plate")).unwrap();
    let mut spec = serde_json::json!({"type": "hole", "position": [5, 5, 2], "direction": [0, 0, -1], "diameter": "3"});
    if let (Some(spec), Some(extra)) = (spec.as_object_mut(), hole.as_object()) {
        spec.extend(extra.clone());
    }
    let hole = doc.add_feature(serde_json::from_value(spec).unwrap(), Some("Hole")).unwrap();
    let pattern: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "pattern", "features": [hole],
        "pattern": {"type": "rectangular", "dir1": [1, 0, 0], "count1": "10", "spacing1": "10", "dir2": [0, 1, 0], "count2": "9", "spacing2": "10"},
    }))
    .unwrap();
    doc.add_feature(pattern, Some("Grid")).unwrap();
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    (model, work)
}

/// A hole as deep as its plate, after `others` boxes 300 mm and more away and, with `under`, a
/// slab touching the plate's bottom face: the plate's and slab's volumes and the work.
fn plate_hole_among_bodies(others: usize, under: bool) -> (f64, Option<f64>, Counts) {
    let mut doc = Document::new("bodies");
    let solid = |corner: [f64; 3], length: f64, height: f64| -> FeatureKind {
        serde_json::from_value(serde_json::json!({
            "type": "box", "corner": corner, "length": length.to_string(), "width": length.to_string(), "height": height.to_string(),
        }))
        .unwrap()
    };
    for i in 0..others {
        doc.add_feature(solid([300.0 + 30.0 * i as f64, 0.0, 0.0], 20.0, 20.0), Some(&format!("Far{i}"))).unwrap();
    }
    if under {
        doc.add_feature(solid([0.0, 0.0, -5.0], 100.0, 5.0), Some("Under")).unwrap();
    }
    doc.add_feature(solid([0.0, 0.0, 0.0], 100.0, 2.0), Some("Plate")).unwrap();
    let hole: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "hole", "position": [50, 50, 2], "direction": [0, 0, -1], "diameter": "3", "depth": "2",
    }))
    .unwrap();
    doc.add_feature(hole, Some("Hole")).unwrap();
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    let st = model.state();
    // The plate is the body from z = 0 to 2 at the origin, the slab the one below it.
    let volume = |top: f64| {
        st.bodies
            .iter()
            .find(|b| b.body.exact_bounds().is_ok_and(|bb| bb.min.x.abs() < 1e-6 && (bb.max.z - top).abs() < 1e-6))
            .map(|b| kernel::measure(&b.body).unwrap().volume)
    };
    (volume(2.0).unwrap(), volume(0.0), work)
}

/// A hole with a depth looks for material and air either side of its bottom, to know whether it
/// comes out there (#17873), in the bodies whose boxes hold those points only (#19946). It
/// classified both points in every body of the design: 2 x 21 classifications with 20 boxes far
/// away, which in a design of hundreds of imported parts cost minutes for each evaluation of its
/// holes. A body touching the plate where the hole ends is still asked.
#[test]
fn a_hole_classifies_its_bottom_in_the_bodies_there_only() {
    let through = 20_000.0 - std::f64::consts::PI * 1.5 * 1.5 * 2.0;
    for others in [0, 20] {
        let (plate, under, work) = plate_hole_among_bodies(others, false);
        assert!((plate - through).abs() < 1e-6 * through, "{others} boxes: {plate}");
        assert_eq!(under, None);
        assert_eq!(work.hole_points, 2, "{others} boxes: {work:?}");
    }
    let (plate, under, work) = plate_hole_among_bodies(20, true);
    assert!((plate - through).abs() < 1e-6 * through, "{plate}");
    assert!((under.unwrap() - 50_000.0).abs() < 1e-6 * 50_000.0, "the slab is not drilled: {under:?}");
    assert_eq!(work.hole_points, 4, "{work:?}");
}

#[test]
fn targeted_holes_and_their_copies_classify_only_the_plate() {
    let mut doc = Document::new("plate and reference");
    let reference = MeshData {
        name: "Reference".into(),
        // A tetrahedron whose box contains both holes. At the plate's bottom (z = 0)
        // the mesh ends at x + y = 45, and the plate starts at x + y = 48.
        positions: [0., 0., -5., 60., 0., -5., 0., 60., -5., 0., 0., 15.].into(),
        triangles: [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3].into(),
        color: None,
        corner_normals: Default::default(),
    };
    doc.add_feature(FeatureKind::MeshImport { file: "reference.stl".into(), meshes: vec![reference] }, Some("Reference")).unwrap();
    let plate = doc
        .add_feature(
            serde_json::from_value(serde_json::json!({"type": "box", "corner": [24, 24, 0], "length": "10", "width": "10", "height": "10"})).unwrap(),
            Some("Plate"),
        )
        .unwrap();
    doc.feature_mut(plate).unwrap().body_names = vec!["Plate".into()];
    let hole = doc
        .add_feature(
            serde_json::from_value(serde_json::json!({
                "type": "hole", "position": [27, 27, 10], "direction": [0, 0, -1], "diameter": "3", "depth": "5",
            }))
            .unwrap(),
            Some("Hole"),
        )
        .unwrap();
    doc.feature_mut(hole).unwrap().participants = vec!["Plate".into()];
    doc.add_feature(
        serde_json::from_value(serde_json::json!({
            "type": "pattern", "features": [hole],
            "pattern": {"type": "rectangular", "dir1": [1, 0, 0], "count1": "2", "spacing1": "4"},
        }))
        .unwrap(),
        Some("Copy"),
    )
    .unwrap();

    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    // Two samples either side of the depth of each of the two holes.
    assert_eq!(work.hole_points, 4, "{work:?}");
    let volume = kernel::measure(&model.state().body("Plate").unwrap().body).unwrap().volume;
    let expected = 1000.0 - 2.0 * std::f64::consts::PI * 1.5 * 1.5 * 5.0;
    assert!((volume - expected).abs() < 1e-6 * expected, "{volume} against {expected}");
}

/// A pattern of a hole builds its tools once when every copy is drilled the same way, and for
/// each copy when the tools look at the material there (#7942).
#[test]
fn copies_of_a_plain_hole_move_its_tools_and_a_drill_point_drills_each() {
    // A plain hole's tools are the same at every copy, moved: they are built for the hole and
    // for the pattern's first copy, and the 90 holes are all cut (#7942).
    let (v, hole, work) = plate_with_hole_grid(serde_json::json!({}));
    assert!((v - (20_000.0 - 90.0 * hole)).abs() < 1e-6 * hole, "{v}");
    assert_eq!(work.hole_tools, 2, "{work:?}");
    // A counterbore's are too.
    let cb = serde_json::json!({"hole": {"type": "counterbore", "cb_diameter": "5", "cb_depth": "1"}});
    let (v, hole, work) = plate_with_hole_grid(cb);
    let pocket = std::f64::consts::PI * (2.5 * 2.5 - 1.5 * 1.5) * 1.0;
    assert!((v - (20_000.0 - 90.0 * (hole + pocket))).abs() < 1e-6 * hole, "{v}");
    assert_eq!(work.hole_tools, 2, "{work:?}");
    // A blind hole with a drill point looks at the material at each copy, so each copy is
    // drilled on its own, as before.
    let drilled = serde_json::json!({"depth": "1", "hole": {"type": "drilled", "tip_angle": "118 deg"}});
    let (_, _, work) = plate_with_hole_grid(drilled);
    assert_eq!(work.hole_tools, 90, "{work:?}");
}

/// [`plate_with_hole_grid`] with `hole`, cut as a pattern cuts it and tool by tool, one boolean a
/// tool, as before #2358: the work of each, after checking that both leave the same plate (the
/// same volume, the same faces, named the same).
fn hole_grid_both_ways(hole: serde_json::Value) -> (Counts, Counts) {
    let plate = |limit: usize| {
        super::JOINED_TOOLS_LIMIT.with(|l| l.set(limit));
        let (model, work) = plate_with_hole_grid_model(hole.clone());
        super::JOINED_TOOLS_LIMIT.with(|l| l.set(super::MAX_JOINED_TOOLS));
        let st = model.state();
        assert_eq!(st.bodies.len(), 1);
        let mut names = crate::naming::face_names(&st.bodies[0]).to_vec();
        names.sort();
        (disc_volume(&model), st.bodies[0].body.face_count(), names, work)
    };
    let (v, faces, names, work) = plate(super::MAX_JOINED_TOOLS);
    let (one_v, one_faces, one_names, one_work) = plate(1);
    let hole = std::f64::consts::PI * 1.5 * 1.5 * 2.0;
    assert!((v - one_v).abs() < 1e-6 * hole, "{v} vs {one_v}");
    assert_eq!(faces, one_faces);
    assert_eq!(names, one_names);
    // Each of the 89 copies' two tools was cut on its own.
    assert_eq!(one_work.cut_tools, 178, "{one_work:?}");
    (work, one_work)
}

/// A pattern of a countersunk hole cuts its copies 32 at a time, as a plain hole's, not one tool
/// at a time (#2358). Each hole is cut with two tools, its cone and its bore, which overlap, so
/// the copies' tools together were never apart and each was cut on its own: two booleans a copy,
/// each dearer as the plate gained holes. The cone and the bore are joined once, and moved to each
/// copy as one tool. A counterbore's pocket and bore are too.
#[test]
fn a_pattern_of_a_countersunk_or_counterbored_hole_cuts_its_copies_at_once() {
    let csink = serde_json::json!({"hole": {"type": "countersink", "cs_diameter": "6", "cs_angle": "90 deg"}});
    let (work, _) = hole_grid_both_ways(csink.clone());
    // 89 copies of one tool, cut in 3 groups; no union run to tell the copies apart.
    assert_eq!((work.cut_tools, work.joined_hole_tools, work.apart_unions), (3, 1, 0), "{work:?}");
    // The plate less 90 bores and 90 cones over them (a 90 deg cone from 6 mm on the face down to
    // the 3 mm bore: 1.5 mm deep).
    let (model, _) = plate_with_hole_grid_model(csink);
    let hole = std::f64::consts::PI * 1.5 * 1.5 * 2.0;
    let cone = std::f64::consts::PI * 1.5 / 3.0 * (3.0 * 3.0 + 3.0 * 1.5 + 1.5 * 1.5) - std::f64::consts::PI * 1.5 * 1.5 * 1.5;
    let v = disc_volume(&model);
    assert!((v - (20_000.0 - 90.0 * (hole + cone))).abs() < 1e-3 * hole, "{v}");
    let cbore = serde_json::json!({"hole": {"type": "counterbore", "cb_diameter": "5", "cb_depth": "1"}});
    let (work, _) = hole_grid_both_ways(cbore);
    assert_eq!((work.cut_tools, work.joined_hole_tools, work.apart_unions), (3, 1, 0), "{work:?}");
}

/// A circular pattern of a countersunk hole turns its one tool to each copy (no box moves with a
/// turn: each copy's is measured) and leaves the disc that cutting each tool on its own leaves
/// (#2358).
#[test]
fn a_circular_pattern_of_a_countersunk_hole_cuts_its_copies_at_once() {
    let disc = |limit: usize| {
        super::JOINED_TOOLS_LIMIT.with(|l| l.set(limit));
        let mut doc = Document::new("countersunk ring");
        let disc = FeatureKind::Cylinder {
            base: point_expr(Vec3::ZERO),
            axis: crate::dir_expr(Vec3::Z),
            radius: "50".into(),
            height: "3".into(),
            operation: Operation::NewBody,
            references: Vec::new(),
        };
        doc.add_feature(disc, Some("Disc")).unwrap();
        let hole: FeatureKind = serde_json::from_value(serde_json::json!({
            "type": "hole", "position": [30, 0, 3], "direction": [0, 0, -1], "diameter": "3.5",
            "hole": {"type": "countersink", "cs_diameter": "6", "cs_angle": "90 deg"},
        }))
        .unwrap();
        let hole = doc.add_feature(hole, Some("Hole")).unwrap();
        hole_pattern(&mut doc, hole, 24);
        let mut model = Model::new();
        let work = counted(|| model.evaluate(&doc));
        super::JOINED_TOOLS_LIMIT.with(|l| l.set(super::MAX_JOINED_TOOLS));
        assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
        let st = model.state();
        assert_eq!(st.bodies.len(), 1);
        let mut names = crate::naming::face_names(&st.bodies[0]).to_vec();
        names.sort();
        (disc_volume(&model), st.bodies[0].body.face_count(), names, work)
    };
    let (v, faces, names, work) = disc(super::MAX_JOINED_TOOLS);
    let (one_v, one_faces, one_names, one_work) = disc(1);
    assert_eq!((work.cut_tools, work.joined_hole_tools), (1, 1), "{work:?}");
    assert_eq!(one_work.cut_tools, 46, "{one_work:?}");
    assert!((v - one_v).abs() < 1e-6 * v, "{v} vs {one_v}");
    assert_eq!(faces, one_faces);
    assert_eq!(names, one_names);
}

/// Copies whose tools are built at each copy (a blind countersink with a drill point, which looks
/// at the material there) are not joined: their cones are cut together, then their bores, each
/// in groups of 32, as each copy cuts its own (#2358). A cone and its bore are known to meet (a
/// point on their axis lies inside both), so no union is run to find out that the copies' tools
/// cannot all be joined as they are.
#[test]
fn copies_of_a_hole_built_at_each_copy_cut_their_tools_in_layers() {
    let drilled =
        serde_json::json!({"depth": "1.5", "hole": {"type": "countersink", "cs_diameter": "6", "cs_angle": "90 deg", "tip_angle": "118 deg"}});
    let (work, _) = hole_grid_both_ways(drilled);
    assert_eq!(work.hole_tools, 90, "{work:?}");
    assert_eq!((work.cut_tools, work.joined_hole_tools, work.apart_unions), (6, 0, 0), "{work:?}");
}

/// The box a hole's run-on column is tested by, without building it (#2358), holds the exact box
/// of the column built, and is no more than a hair larger than the cylinder: along axes square
/// to the coordinate axes and slanted ones, small and large.
#[test]
fn a_run_on_columns_box_holds_the_column_built() {
    let axes = [Vec3::Z, Vec3::X * -1.0, Vec3::new(1.0, 2.0, 3.0), Vec3::new(-0.3, 0.05, -1.0), Vec3::new(1.0, 1.0, 0.0)];
    for axis in axes {
        let axis = axis.normalized().unwrap();
        for (radius, height) in [(4.5, 0.12), (0.25, 0.1), (60.0, 3.0)] {
            let base = Vec3::new(9.0, -3.5, 12.0);
            let plane = Plane::from_normal(base, axis).unwrap();
            let disc = solidcore_record::Region2 { outer: solidcore_record::Loop2::circle_from(Vec2::ZERO, radius, 0.0), holes: vec![] };
            let column = kernel::extrude(&plane, &[disc], 0.0, height).unwrap().pop().unwrap();
            let built = super::more::exact_box(&column).unwrap();
            let computed = super::cylinder_box(base, axis, radius, height).unwrap();
            assert!(computed.contains(built.min) && computed.contains(built.max), "{axis:?} r {radius} h {height}: {computed:?} against {built:?}");
            // The cylinder's own box, tight: each face of it touched by the disc at one end.
            let slack = 2e-4 + 1e-6 * (radius + height);
            for (lo, hi) in [(computed.min.x, built.min.x), (computed.min.y, built.min.y), (computed.min.z, built.min.z)] {
                assert!(hi - lo <= slack + 1e-9 * radius.max(1.0), "{axis:?} r {radius}: {computed:?} against {built:?}");
            }
        }
    }
    assert!(super::cylinder_box(Vec3::ZERO, Vec3::Z, f64::NAN, 1.0).is_none());
}

/// A copy of a hole with material just above its spot, which is cut out of its tools there, has
/// its own tools built: the moved tools of a copy in the clear would cut that material (#7942).
#[test]
fn a_hole_copy_under_material_builds_its_own_tools() {
    let mut doc = Document::new("perforated under a block");
    for (corner, length, height) in [([0.0, 0.0, 0.0], "100", "2"), ([0.0, 0.0, 2.1], "20", "1")] {
        let block: FeatureKind = serde_json::from_value(serde_json::json!({
            "type": "box", "corner": corner, "length": length, "width": length, "height": height,
        }))
        .unwrap();
        doc.add_feature(block, None).unwrap();
    }
    let spec = serde_json::json!({"type": "hole", "position": [5, 5, 2], "direction": [0, 0, -1], "diameter": "3"});
    let hole = doc.add_feature(serde_json::from_value(spec).unwrap(), Some("Hole")).unwrap();
    let pattern: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "pattern", "features": [hole],
        "pattern": {"type": "rectangular", "dir1": [1, 0, 0], "count1": "10", "spacing1": "10", "dir2": [0, 1, 0], "count2": "9", "spacing2": "10"},
    }))
    .unwrap();
    doc.add_feature(pattern, Some("Grid")).unwrap();
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.results.iter().all(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.results);
    let mut volumes: Vec<f64> = model.state().bodies.iter().map(|b| kernel::measure(&b.body).unwrap().volume).collect();
    volumes.sort_by(f64::total_cmp);
    let hole = std::f64::consts::PI * 1.5 * 1.5 * 2.0;
    // The block 0.1 mm over the four holes under it is left whole, and the plate has all 90.
    assert_eq!(volumes.len(), 2, "{volumes:?}");
    assert!((volumes[0] - 400.0).abs() < 1e-6, "{volumes:?}");
    assert!((volumes[1] - (20_000.0 - 90.0 * hole)).abs() < 1e-6 * hole, "{volumes:?}");
    // The three copies under the block build theirs, and so does a copy in the clear after one.
    assert!(work.hole_tools > 4 && work.hole_tools < 90, "{work:?}");
}

/// A pattern of a pattern is held to the copies one pattern makes, counted before they are made
/// (#7942).
#[test]
fn a_pattern_of_a_pattern_is_refused_past_the_copies_one_pattern_makes() {
    // Four holes in a row, patterned 5000 times: 14997 copies, more than one pattern makes. The
    // two counts are multiplied before any copy is placed, so a pattern of two large patterns
    // is refused without placing them all (#7942).
    let mut doc = Document::new("nested");
    let plate: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "box", "corner": [0, 0, 0], "length": "20", "width": "10", "height": "2",
    }))
    .unwrap();
    doc.add_feature(plate, Some("Plate")).unwrap();
    let hole: FeatureKind =
        serde_json::from_value(serde_json::json!({"type": "hole", "position": [5, 5, 2], "direction": [0, 0, -1], "diameter": "1"})).unwrap();
    let hole = doc.add_feature(hole, Some("Hole")).unwrap();
    let row = |features: u64, count: &str, spacing: &str| -> FeatureKind {
        serde_json::from_value(serde_json::json!({
            "type": "pattern", "features": [features],
            "pattern": {"type": "rectangular", "dir1": [1, 0, 0], "count1": count, "spacing1": spacing},
        }))
        .unwrap()
    };
    let inner = doc.add_feature(row(hole, "4", "2"), Some("Inner")).unwrap();
    let outer = doc.add_feature(row(inner, "5000", "0.001"), Some("Outer")).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().find(|r| r.id == inner).is_some_and(|r| r.error.is_none()), "{:?}", model.results);
    let error = model.results.iter().find(|r| r.id == outer).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(error.contains("too many pattern instances"), "{error}");
}

#[test]
fn tools_are_apart_only_when_they_neither_meet_nor_nest() {
    let rod = |x: f64, y: f64, r: f64| kernel::cylinder(Vec3::new(x, y, -100.0), Vec3::Z, r, 200.0).unwrap();
    // Long rods whose boxes, grown as the kernel grows them, meet: 0.5 mm apart.
    assert!(super::tools_clearly_apart(&[rod(0.0, 0.0, 1.0), rod(2.5, 0.0, 1.0)]));
    // Touching, crossing and nested rods are not apart.
    assert!(!super::tools_clearly_apart(&[rod(0.0, 0.0, 1.0), rod(2.0, 0.0, 1.0)]));
    assert!(!super::tools_clearly_apart(&[rod(0.0, 0.0, 1.0), rod(1.5, 0.0, 1.0)]));
    assert!(!super::tools_clearly_apart(&[rod(0.0, 0.0, 3.0), rod(0.5, 0.0, 1.0)]));
    let tube = kernel::boolean(&rod(0.0, 0.0, 5.0), &rod(0.0, 0.0, 4.0), kernel::BoolOp::Cut).unwrap().unwrap();
    assert!(!super::tools_clearly_apart(&[tube.clone(), rod(0.0, 0.0, 4.5)]));
    // A rod in the tube's bore, clear of its wall, is apart from it.
    assert!(super::tools_clearly_apart(&[tube, rod(0.0, 0.0, 3.0)]));
}

fn imported_meshes() -> Document {
    let mut doc = Document::new("reference meshes");
    let comp = doc.add_component(Some("Part"), 0).unwrap();
    let meshes = (0..2)
        .map(|i| MeshData {
            name: format!("Mesh{i}"),
            positions: [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0].into(),
            triangles: [0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3].into(),
            color: Some([0.2, 0.4, 0.6]),
            corner_normals: Default::default(),
        })
        .collect();
    let id = doc.add_feature(FeatureKind::MeshImport { file: "tetrahedra.stl".into(), meshes }, Some("Meshes")).unwrap();
    doc.feature_mut(id).unwrap().component = comp;
    doc
}

fn meshes_mut(doc: &mut Document) -> &mut Vec<MeshData> {
    let FeatureKind::MeshImport { meshes, .. } = &mut doc.features[0].kind else { panic!("mesh fixture") };
    meshes
}

fn same_as_fresh(doc: &Document, model: &Model) {
    let mut fresh = Model::new();
    fresh.no_cache = true;
    fresh.evaluate(doc);
    assert_eq!(model.results.iter().map(|r| r.fingerprint).collect::<Vec<_>>(), fresh.results.iter().map(|r| r.fingerprint).collect::<Vec<_>>());
    let (actual, expected) = (model.state(), fresh.state());
    assert_eq!(actual.bodies.len(), expected.bodies.len());
    for (a, b) in actual.bodies.iter().zip(&expected.bodies) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.mesh().positions, b.mesh().positions);
        assert_eq!(a.mesh().triangles, b.mesh().triangles);
        assert_eq!(a.body.color(), b.body.color());
    }
    assert!(model.results.iter().all(|r| r.error.is_none()));
}

#[test]
fn unchanged_mesh_payloads_are_not_read_again_for_rigid_motion() {
    let mut doc = imported_meshes();
    for mesh in meshes_mut(&mut doc) {
        mesh.generate_crease_normals(10.0).unwrap();
        assert!(!mesh.corner_normals.is_empty());
    }
    let compact: serde_json::Value = serde_json::from_str(&super::feature_json(&doc.features[0], &super::ImportDigests::default())).unwrap();
    for mesh in compact[0]["meshes"].as_array().unwrap() {
        assert_eq!(mesh["positions"], serde_json::json!([]));
        assert_eq!(mesh["triangles"], serde_json::json!([]));
        assert!(mesh.get("corner_normals").is_none(), "the fingerprint must hash the retained digest without serializing shading payloads");
    }
    let mut model = Model::new();
    assert_eq!(counted(|| model.evaluate(&doc)).mesh_digested, 2);
    for k in 1..5 {
        doc = doc.clone(); // Undo/preview copies share the arrays, not the document itself.
        doc.occurrences[0].transform = rigid(Vec3::new(f64::from(k), 2.0, 3.0), Vec3::ZERO, Vec3::Z, 0.1 * f64::from(k));
        assert_eq!(
            counted(|| {
                model.evaluate(&doc);
                assert!(model.is_current(&doc));
            })
            .mesh_digested,
            0
        );
        assert_eq!(model.last_recomputed, 0);
        same_as_fresh(&doc, &model);
    }
    let saved = serde_json::to_string(&doc).unwrap();
    let reopened: Document = serde_json::from_str(&saved).unwrap();
    assert_eq!(counted(|| model.evaluate(&reopened)).mesh_digested, 2, "new array identities must be checked");
    assert_eq!(model.last_recomputed, 0, "equal content has equal fingerprints");
    same_as_fresh(&reopened, &model);
}

#[test]
fn mesh_payload_edits_invalidate_the_cache_and_undo_restores_the_geometry() {
    let mut doc = imported_meshes();
    let original = doc.clone();
    let mut model = Model::new();
    model.evaluate(&doc);
    let fingerprint = model.results[0].fingerprint;
    Arc::make_mut(&mut meshes_mut(&mut doc)[0].positions)[3] = 5.0;
    assert!(!model.is_current(&doc));
    assert_eq!(counted(|| model.evaluate(&doc)).mesh_digested, 1);
    assert_ne!(model.results[0].fingerprint, fingerprint);
    same_as_fresh(&doc, &model);
    let edited = doc.clone();
    Arc::make_mut(&mut meshes_mut(&mut doc)[0].triangles).swap(0, 1);
    assert_eq!(counted(|| model.evaluate(&doc)).mesh_digested, 1);
    same_as_fresh(&doc, &model);
    for restored in [&original, &edited, &original] {
        model.evaluate(restored);
        same_as_fresh(restored, &model);
    }
    assert_eq!(model.results[0].fingerprint, fingerprint);
}

#[test]
fn mesh_metadata_and_order_remain_part_of_the_fingerprint() {
    let mut doc = imported_meshes();
    let mut model = Model::new();
    model.evaluate(&doc);
    for edit in 0..7 {
        let before = model.results[0].fingerprint;
        match edit {
            0 => meshes_mut(&mut doc)[0].name = "Renamed".into(),
            1 => meshes_mut(&mut doc)[0].color = Some([0.8, 0.1, 0.3]),
            2 => doc.features[0].name = "Renamed feature".into(),
            3 => meshes_mut(&mut doc).swap(0, 1),
            4 => {
                meshes_mut(&mut doc).pop();
            }
            5 => {
                let mesh = meshes_mut(&mut doc)[0].clone();
                meshes_mut(&mut doc).push(MeshData { name: "Added".into(), ..mesh });
            }
            _ => {
                let FeatureKind::MeshImport { file, .. } = &mut doc.features[0].kind else { panic!("mesh fixture") };
                *file = "renamed.stl".into();
            }
        }
        assert!(!model.is_current(&doc));
        let work = counted(|| model.evaluate(&doc));
        if edit < 3 || edit == 6 {
            assert_eq!(work.mesh_digested, 0, "metadata alone: {edit}");
        }
        assert_ne!(model.results[0].fingerprint, before, "edit {edit}");
        same_as_fresh(&doc, &model);
    }
}

#[test]
fn an_added_and_deleted_sketch_entity_keeps_its_fingerprint() {
    let mut doc = Document::new("sketch fingerprint");
    let id = doc.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(10.0, 0.0)), None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    let key = model.state().sketch_keys.get(&id).copied();
    let json = super::feature_json(doc.feature(id).unwrap(), &super::ImportDigests::default());

    // Make and delete an id: the counters advance, but the geometry is the same.
    {
        let sk = doc.sketch_mut(id).unwrap();
        let a = sk.add_point(Vec2::new(2.0, 2.0), None).unwrap();
        let b = sk.add_point(Vec2::new(3.0, 2.0), None).unwrap();
        let line = sk.add_line_pts(a, b, None).unwrap();
        sk.remove_curves(&[line]);
        sk.remove_points(&[b]);
        sk.remove_points(&[a]);
    }

    let json_after = super::feature_json(doc.feature(id).unwrap(), &super::ImportDigests::default());
    assert_eq!(json, json_after, "deleted ids are not geometry");
    model.evaluate(&doc);
    assert_eq!(model.state().sketch_keys.get(&id).copied(), key);
    assert_eq!((model.last_recomputed, model.last_restored), (0, 0));
}

#[test]
fn a_hole_skips_the_parts_its_line_cannot_reach() {
    use solidcraft_geom::Aabb3;
    let unit = Aabb3 { min: Vec3::ZERO, max: Vec3::new(1.0, 1.0, 1.0) };
    // Down through the box, from above and from below (the line runs both ways).
    assert!(super::line_meets_box(Vec3::new(0.5, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0), &unit));
    assert!(super::line_meets_box(Vec3::new(0.5, 0.5, -5.0), Vec3::new(0.0, 0.0, -1.0), &unit));
    // Beside it, parallel to a side: a zero direction component keeps the line out of the slab.
    assert!(!super::line_meets_box(Vec3::new(2.0, 0.5, 5.0), Vec3::new(0.0, 0.0, -1.0), &unit));
    // Slanted past a corner, and slanted through it.
    assert!(!super::line_meets_box(Vec3::new(3.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), &unit));
    assert!(super::line_meets_box(Vec3::new(-1.0, -1.0, 0.5), Vec3::new(1.0, 1.0, 0.0), &unit));
    // Touching a face counts; an empty box never meets.
    assert!(super::line_meets_box(Vec3::new(1.0, 0.5, 9.0), Vec3::new(0.0, 0.0, 1.0), &unit));
    assert!(!super::line_meets_box(Vec3::new(0.5, 0.5, 0.5), Vec3::new(0.0, 0.0, 1.0), &Aabb3::EMPTY));
}

#[test]
fn a_hole_classifies_the_faces_it_reads_not_every_face_of_the_design() {
    // Forty separate cylinders, three faces each. A hole placed on the top of one drills down,
    // and finding that out reads the nearest face only: it used to classify every face of every
    // body first, which in an assembly of hundreds of imported parts took seconds per hole.
    let mut doc = Document::new("hole direction work");
    for k in 0..40 {
        let kind = FeatureKind::Cylinder {
            base: point_expr(Vec3::new(10.0 * f64::from(k), 0.0, 0.0)),
            axis: crate::dir_expr(Vec3::Z),
            radius: "2".into(),
            height: "10".into(),
            operation: Operation::NewBody,
            references: Vec::new(),
        };
        doc.add_feature(kind, None).unwrap();
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    let st = model.state();
    assert_eq!(st.bodies.len(), 40);
    let mut dir = None;
    let work = counted(|| dir = super::hole_direction_at(&st, Vec3::new(250.5, 0.5, 10.0), &[]));
    let dir = dir.unwrap();
    assert!((dir - Vec3::new(0.0, 0.0, -1.0)).len() < 1e-9, "drills down into the top: {dir:?}");
    assert!(work.surfaces <= 2, "classified {} face surfaces for one hole", work.surfaces);
    // A point on a side wall drills into that wall.
    let side = super::hole_direction_at(&st, Vec3::new(252.0, 0.0, 5.0), &[]).unwrap();
    assert!((side - Vec3::new(-1.0, 0.0, 0.0)).len() < 1e-6, "into the side wall: {side:?}");
}

/// A cut whose target name is taken by another component's sheet, while a body of the cut's own
/// component was asked for that name, is refused before the sheet is machined (#12141).
#[test]
fn an_ambiguous_target_is_refused_before_a_sheet_is_machined() {
    let mut doc = Document::new("names");
    let linked = doc.add_component(Some("Encoder"), 0).unwrap();
    let local = doc.add_component(Some("Assembly"), 0).unwrap();
    flat_sheet(&mut doc, Vec2::new(0.0, 0.0), 40.0, 40.0);
    for f in doc.features.iter_mut() {
        f.component = linked;
    }
    let base = doc.features.last().unwrap().id;
    doc.feature_mut(base).unwrap().body_names = vec!["Plate".into()];
    let own = doc
        .add_feature(
            FeatureKind::Box {
                corner: point_expr(Vec3::new(100.0, 0.0, 0.0)),
                length: "10".into(),
                width: "10".into(),
                height: "2".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
    doc.feature_mut(own).unwrap().component = local;
    doc.feature_mut(own).unwrap().body_names = vec!["Plate".into()];
    let cut = doc
        .add_feature(
            FeatureKind::Box {
                corner: point_expr(Vec3::new(10.0, 10.0, -5.0)),
                length: "5".into(),
                width: "5".into(),
                height: "20".into(),
                operation: Operation::Cut,
                centred: false,
            },
            None,
        )
        .unwrap();
    doc.feature_mut(cut).unwrap().component = local;
    doc.feature_mut(cut).unwrap().participants = vec!["Plate".into()];
    let mut model = Model::new();
    model.evaluate(&doc);
    let err = model.result(cut).and_then(|r| r.error.clone()).expect("the cut is refused");
    assert!(err.contains("ambiguous") && err.contains("`Encoder`"), "{err}");
    let st = model.state();
    let sheet = st.surface_sheets.iter().find(|s| s.body == "Plate").expect("the linked sheet");
    assert!(sheet.sheet.panels.iter().all(|p| p.region.holes.is_empty()), "the linked sheet is not machined");
}

/// A hole's look at the material over its entry face (#10695) is best effort: when the kernel
/// cannot build the run-on's column (here a column of no width), the hole keeps its full run-on
/// and cuts nothing out of its tools, as before the check, instead of failing.
#[test]
fn a_hole_whose_run_on_column_cannot_be_built_keeps_its_run_on() {
    let mut state = ModelState::default();
    state.bodies.push(ModelBody::new("Over".into(), kernel::box_solid(Vec3::new(-5.0, -5.0, 0.5), Vec3::new(5.0, 5.0, 2.0)).unwrap(), 0));
    for reach in [0.0, f64::NAN] {
        let (top, near) = super::hole_run_on(&state, &Plane::XY, reach, 1.0);
        assert_eq!(top, 1.0, "reach {reach}");
        assert!(near.is_empty(), "reach {reach}");
    }
    // With a column, the body 0.5 over the face halves the run-on to its gap.
    let (top, near) = super::hole_run_on(&state, &Plane::XY, 3.0, 1.0);
    assert!((top - 0.25).abs() < 1e-6, "top {top}");
    assert!(near.is_empty());
}

/// `n` boxes of 2 x 2 x 2 in a row, `pitch` from one to the next, joined into the first by one
/// Combine: the model, and the Combine.
fn boxes_joined(n: usize, pitch: f64) -> (Model, u64, Counts) {
    let mut doc = Document::new("joined boxes");
    let mut made = Vec::new();
    for i in 0..n {
        let kind = FeatureKind::Box {
            corner: point_expr(Vec3::new(pitch * i as f64, 0.0, 0.0)),
            length: "2".into(),
            width: "2".into(),
            height: "2".into(),
            operation: Operation::NewBody,
            centred: false,
        };
        made.push(doc.add_feature(kind, None).unwrap());
    }
    let mut model = Model::new();
    model.evaluate(&doc);
    let names: Vec<String> = made.iter().map(|id| model.state().bodies.iter().find(|b| b.feature == *id).unwrap().name.clone()).collect();
    let (target, tools) = names.split_first().unwrap();
    let combine = doc
        .add_feature(
            FeatureKind::Combine {
                target: target.clone().into(),
                tools: tools.iter().map(|t| t.clone().into()).collect(),
                operation: Operation::Join,
                keep_tools: false,
            },
            None,
        )
        .unwrap();
    let work = counted(|| model.evaluate(&doc));
    (model, combine, work)
}

/// Joining bodies that are all apart runs no kernel union (#3455: 64 separate pegs took the
/// union 4.5 s, growing with the square of their number): they are one body of several pieces
/// as they stand. Bodies that meet still go through the union.
#[test]
fn a_join_of_bodies_that_are_all_apart_runs_no_union() {
    let (model, combine, work) = boxes_joined(12, 5.0);
    assert!(model.result(combine).is_some_and(|r| r.error.is_none() && r.warning.is_none()), "{:?}", model.result(combine));
    assert_eq!(work.joined_apart, 1, "{work:?}");
    let st = model.state();
    assert_eq!(st.bodies.len(), 1);
    let body = &st.bodies[0].body;
    assert_eq!((body.lumps().unwrap().len(), body.face_count()), (12, 72));
    assert!((kernel::measure(body).unwrap().volume - 96.0).abs() < 1e-9);

    // Each box overlaps the next by half: the kernel's union makes them one piece.
    let (model, combine, work) = boxes_joined(4, 1.0);
    assert!(model.result(combine).is_some_and(|r| r.error.is_none()), "{:?}", model.result(combine));
    assert_eq!(work.joined_apart, 0, "{work:?}");
    let st = model.state();
    assert_eq!(st.bodies.len(), 1);
    assert_eq!(st.bodies[0].body.lumps().unwrap().len(), 1);
    assert!((kernel::measure(&st.bodies[0].body).unwrap().volume - 20.0).abs() < 1e-9);
}

/// When an import's background checks end having healed a part, the model takes the healed
/// body in the import's place: settling marks the import and what follows it stale, the next
/// evaluation reads the healed body under another fingerprint, and settling the same heal again
/// changes nothing. Another model of the same design, whose checks healed nothing, keeps the
/// file's bodies.
#[test]
fn an_imports_healed_parts_replace_its_bodies_once_its_checks_end() {
    // the heal preference held on for the whole test, on its thread
    let _on = kernel::heal_preference_for_tests(true);
    let mut doc = placed_imports(1);
    let later = doc.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(10.0, 1.0)), None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()));
    let volumes = |m: &Model| {
        let mut v = m.state().bodies.iter().map(|b| b.body.volume_at(1e-6).unwrap().round()).collect::<Vec<_>>();
        v.sort_by(f64::total_cmp);
        v
    };
    assert_eq!(volumes(&model), [125.0, 1000.0]);
    let fingerprints = |m: &Model| m.results.iter().map(|r| r.fingerprint).collect::<Vec<_>>();
    let before = fingerprints(&model);
    let import = doc.features[0].id;
    let issue = kernel::ImportIssue {
        kind: kernel::IssueKind::Repaired,
        item: Some(1),
        part: String::new(),
        paths: Vec::new(),
        faces: 0,
        times: 1,
        detail: "healed SMLib's check failures".into(),
    };
    let healed = kernel::box_solid(Vec3::ZERO, Vec3::new(3.0, 3.0, 3.0)).unwrap();
    let face_origin = vec![0; healed.face_count()];
    // Each healed face says which face of the body as read it was: here in reverse order.
    let face_source: Vec<usize> = (0..healed.face_count()).rev().collect();
    let body_of = |m: &Model, volume: f64| {
        let st = m.state();
        let b = st.bodies.iter().find(|b| b.body.volume_at(1e-6).unwrap().round() == volume).unwrap().clone();
        crate::naming::face_names(&b)
    };
    let names_before = body_of(&model, 1000.0);
    let checks = kernel::ImportChecks::ended_for_tests(
        vec![issue],
        vec![kernel::HealedBody { index: 0, body: healed, face_origin, face_source: face_source.clone() }],
    );
    let r = model.results.iter_mut().find(|r| r.id == import).unwrap();
    r.import_report =
        Some(Arc::new(kernel::ImportReport { parts: 2, solids: 2, issues: Vec::new(), single_root: false, split_edges: 0, checks: checks.clone() }));

    assert!(model.settle_import_checks(), "a heal changes the import");
    assert!(model.settle_import_checks(), "until an evaluation brings it in, it is still owed");
    assert!(model.results.iter().all(|r| r.stale), "the import and what follows it are stale");
    model.evaluate(&doc);
    assert_eq!(volumes(&model), [27.0, 125.0], "the healed part replaces the file's");
    // A reference to a face of the body as read finds the face it became.
    let names_after = body_of(&model, 27.0);
    assert_eq!(names_after.len(), face_source.len());
    for (k, &from) in face_source.iter().enumerate() {
        assert_eq!(names_after[k], names_before[from], "healed face {k} is read face {from}");
    }
    let after = fingerprints(&model);
    assert!(after.iter().zip(&before).all(|(a, b)| a != b), "no result keeps an unhealed fingerprint");
    assert!(model.result(later).unwrap().error.is_none());

    // The same heal settled again: nothing to do.
    let r = model.results.iter_mut().find(|r| r.id == import).unwrap();
    r.import_report = Some(Arc::new(kernel::ImportReport { parts: 2, solids: 2, issues: Vec::new(), single_root: false, split_edges: 0, checks }));
    assert!(!model.settle_import_checks());
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0);
    assert_eq!(fingerprints(&model), after);

    let mut other = Model::new();
    other.evaluate(&doc);
    assert_eq!(volumes(&other), [125.0, 1000.0], "another model keeps the file's bodies");
    assert_eq!(fingerprints(&other), before);
}

/// #17708 review: the evaluation that brings an import's healed parts in reads the file again,
/// and that read's checks heal it again. When they end with the heals the model already has,
/// the import is not news: the window is told of the heal once, and nothing goes stale.
#[test]
fn a_heal_the_next_read_finds_again_is_told_once() {
    // the heal preference held on for the whole test, on its thread
    let _on = kernel::heal_preference_for_tests(true);
    let doc = placed_imports(1);
    let mut model = Model::new();
    model.evaluate(&doc);
    let import = doc.features[0].id;
    let issue = kernel::ImportIssue {
        kind: kernel::IssueKind::Repaired,
        item: Some(1),
        part: String::new(),
        paths: Vec::new(),
        faces: 0,
        times: 1,
        detail: "healed SMLib's check failures".into(),
    };
    // A run of the import's checks that healed its first part (each run its own, as each read
    // starts one).
    let run = || {
        let healed = kernel::box_solid(Vec3::ZERO, Vec3::new(3.0, 3.0, 3.0)).unwrap();
        let n = healed.face_count();
        kernel::ImportChecks::ended_for_tests(
            vec![issue.clone()],
            vec![kernel::HealedBody { index: 0, body: healed, face_origin: vec![0; n], face_source: (0..n).collect() }],
        )
    };
    let ended = |model: &mut Model, checks: kernel::ImportChecks| {
        let r = model.results.iter_mut().find(|r| r.id == import).unwrap();
        r.import_report =
            Some(Arc::new(kernel::ImportReport { parts: 2, solids: 2, issues: Vec::new(), single_root: false, split_edges: 0, checks }));
    };
    ended(&mut model, run());
    assert!(model.settle_import_checks());
    assert_eq!(model.take_import_notices(), [import], "the heal is news once");
    model.evaluate(&doc);
    let fingerprints: Vec<u64> = model.results.iter().map(|r| r.fingerprint).collect();
    // The checks of the read that brought the heal in end with the same heals.
    ended(&mut model, run());
    assert!(!model.settle_import_checks(), "the same heals owe no evaluation");
    assert!(model.take_import_notices().is_empty(), "the same heal found again is not news");
    assert!(model.results.iter().all(|r| !r.stale));
    model.evaluate(&doc);
    assert_eq!(model.last_recomputed, 0);
    assert_eq!(model.results.iter().map(|r| r.fingerprint).collect::<Vec<_>>(), fingerprints);
}

/// #20133: a save while an import's SMLib check of its parts runs does not wait for it. The
/// model is left out of the save (the design is rebuilt when it opens), since what the check
/// finds is part of the model; once the check ends, the model is kept with what it found.
#[test]
fn a_save_while_an_import_is_checked_keeps_no_model_and_does_not_wait() {
    let doc = placed_imports(1);
    let mut model = Model::new();
    model.evaluate(&doc);
    let import = doc.features[0].id;
    let (checks, end) = kernel::ImportChecks::running_for_tests();
    let r = model.results.iter_mut().find(|r| r.id == import).unwrap();
    r.import_report = Some(Arc::new(kernel::ImportReport { parts: 2, solids: 2, issues: Vec::new(), single_root: false, split_edges: 0, checks }));
    assert!(model.checking_imports());
    // (were it to wait, this would never return: nothing else ends the check)
    assert!(model.saved_if_checked(&doc).is_none(), "no model while the check runs");
    let issue = kernel::ImportIssue {
        kind: kernel::IssueKind::NeedsRepair,
        item: Some(1),
        part: String::new(),
        paths: Vec::new(),
        faces: 0,
        times: 1,
        detail: "fails SMLib's check".into(),
    };
    end(vec![issue]);
    assert!(!model.checking_imports());
    let saved = model.saved_if_checked(&doc).expect("the model once the check has ended");
    let feature = &saved.records["features"][0];
    assert_eq!(feature["warning"], serde_json::json!(crate::eval::import_warning(&["fails SMLib's check"])));
    assert_eq!(feature["import_report"]["issues"][0]["detail"], "fails SMLib's check", "kept with what the check found");
    assert_eq!(saved.records, model.saved(&doc).unwrap().records, "as a save that waited keeps it");
}

/// #20633: a window is woken once when an import's check of its parts ends, and not at all while
/// it runs, so an untouched window draws nothing for the minutes a large import's check takes.
#[test]
fn a_window_is_woken_once_when_an_imports_check_ends() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let doc = placed_imports(1);
    let mut model = Model::new();
    model.evaluate(&doc);
    let import = doc.features[0].id;
    let end = model.running_import_check_for_tests(import);
    assert!(model.checking_imports());
    let woke = Arc::new(AtomicUsize::new(0));
    let w = woke.clone();
    model.on_import_checks_end(Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(woke.load(Ordering::SeqCst), 0, "nothing while the check runs");
    end(Vec::new());
    assert_eq!(woke.load(Ordering::SeqCst), 1, "the end of the check wakes the window once");
    assert!(!model.checking_imports());
    // A model whose checks have all ended wakes nothing: the result is already in.
    let w = woke.clone();
    model.on_import_checks_end(Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(woke.load(Ordering::SeqCst), 1, "no check is running, so nothing is woken");
}

/// The tolerance heal preference is part of an import's and a Repair's fingerprint, and of no
/// other feature's: turned off, the model is not current and evaluates both again (the import
/// read as it is, the Repair without its heal); turned back on, it comes back to the first
/// fingerprints.
#[test]
fn turning_the_heal_preference_off_evaluates_imports_and_repair_again() {
    let mut doc = placed_imports(1);
    let mut model = Model::new();
    {
        let _on = kernel::heal_preference_for_tests(true);
        model.evaluate(&doc);
    }
    let body = model.state().bodies[0].name.clone();
    let repair = doc.add_feature(FeatureKind::Repair { body: body.into(), gap: "0.2 mm".into(), patch_holes: false }, None).unwrap();
    let import = doc.features[0].id;
    let fingerprint_of = |m: &Model, id: u64| m.result(id).unwrap().fingerprint;
    let (on_import, on_repair) = {
        let _on = kernel::heal_preference_for_tests(true);
        model.evaluate(&doc);
        assert!(model.heals() && model.is_current(&doc));
        (fingerprint_of(&model, import), fingerprint_of(&model, repair))
    };
    {
        let _off = kernel::heal_preference_for_tests(false);
        assert!(!model.is_current(&doc), "the preference turned off: the model is owed an evaluation");
        model.evaluate(&doc);
        assert!(!model.heals());
        assert!(model.last_recomputed >= 2, "the import and the Repair evaluated again: {}", model.last_recomputed);
        assert_ne!(fingerprint_of(&model, import), on_import);
        assert_ne!(fingerprint_of(&model, repair), on_repair);
        assert!(model.is_current(&doc));
    }
    let _on = kernel::heal_preference_for_tests(true);
    assert!(!model.is_current(&doc), "turned back on: owed again");
    model.evaluate(&doc);
    assert_eq!((fingerprint_of(&model, import), fingerprint_of(&model, repair)), (on_import, on_repair));

    // A feature that is neither keeps its fingerprint either way.
    let sketch = doc.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(10.0, 1.0)), None).unwrap();
    let f = doc.feature(sketch).unwrap();
    let off = super::ImportDigests { heal_off: true, ..super::ImportDigests::default() };
    let print = |steps: &super::ImportDigests| super::fingerprint(&doc, 7, f, &BTreeMap::new(), false, steps);
    assert_eq!(print(&off), print(&super::ImportDigests::default()));
    let r = doc.feature(repair).unwrap();
    assert_ne!(
        super::fingerprint(&doc, 7, r, &BTreeMap::new(), false, &off),
        super::fingerprint(&doc, 7, r, &BTreeMap::new(), false, &super::ImportDigests::default())
    );
}

/// A background heal that ends after the preference was turned off is not taken, nor its note:
/// the import keeps no "healed" warning or report over the part as read, nothing is told, and
/// the import and what follows it are stale, owed an evaluation that reads it again.
#[test]
fn a_heal_that_ends_after_the_preference_turned_off_leaves_no_healed_note() {
    let mut doc = placed_imports(1);
    doc.add_feature(line_sketch(PlaneRef::Origin { name: "XY".into() }, Vec2::new(10.0, 1.0)), None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    let import = doc.features[0].id;
    let issue = kernel::ImportIssue {
        kind: kernel::IssueKind::Repaired,
        item: Some(1),
        part: String::new(),
        paths: Vec::new(),
        faces: 0,
        times: 1,
        detail: "healed SMLib's check failures".into(),
    };
    let healed = kernel::box_solid(Vec3::ZERO, Vec3::new(3.0, 3.0, 3.0)).unwrap();
    let n = healed.face_count();
    let checks = kernel::ImportChecks::ended_for_tests(
        vec![issue],
        vec![kernel::HealedBody { index: 0, body: healed, face_origin: vec![0; n], face_source: (0..n).collect() }],
    );
    let r = model.results.iter_mut().find(|r| r.id == import).unwrap();
    r.import_report = Some(Arc::new(kernel::ImportReport { parts: 2, solids: 2, issues: Vec::new(), single_root: false, split_edges: 0, checks }));

    assert!(model.settle_import_checks_under(false), "an evaluation is owed: the import is read again");
    assert!(!model.has_import_heals(), "the heal is not taken");
    assert!(model.take_import_notices().is_empty(), "nothing is told of a heal not taken");
    let r = model.result(import).unwrap();
    assert!(!r.warning.as_deref().unwrap_or("").contains("healed"), "{:?}", r.warning);
    assert!(r.import_report.as_ref().is_none_or(|i| i.current().issues.iter().all(|i| !i.detail.contains("healed"))), "{:?}", r.import_report);
    assert!(model.settle_import_checks_under(false), "settled again: still owed");
    assert!(!model.has_import_heals() && model.take_import_notices().is_empty());
    assert!(model.results.iter().all(|r| r.stale), "the import and what follows it are stale");
}

/// A cut measures the tight box of its tool, and the body's only when the box can show
/// the tool apart of it (#18821): the kernel takes minutes to work out the tight box of
/// a body with many faces (347 s for the cup of the report), against 30 s for the cut
/// the box was measured for. A tool inside the body is not apart from it.
#[test]
fn a_cut_measures_the_tight_box_of_its_tool_only() {
    // A tool inside the body: the body's tight box cannot show it apart of the body,
    // so it is not measured, and the cut is made all the same.
    let mut doc = Document::new("a cut inside");
    block(&mut doc, "A", Vec3::ZERO, [20.0, 20.0, 20.0]);
    let kind = FeatureKind::Box {
        corner: point_expr(Vec3::new(5.0, 5.0, 5.0)),
        length: "10".into(),
        width: "10".into(),
        height: "10".into(),
        operation: Operation::Cut,
        centred: false,
    };
    let cut = doc.add_feature(kind, None).unwrap();
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.result(cut).is_some_and(|r| r.error.is_none()), "{:?}", model.result(cut));
    // The tool's tight box, and no other.
    assert_eq!(work.tight_boxes, 1, "{work:?}");
    // The cut is made all the same.
    assert_eq!(work.cut, 1, "{work:?}");
    assert_eq!(model.state().bodies.len(), 1);

    // A tool that reaches out of the body but meets the box of its vertices: that box lies
    // inside the body's exact box, so the exact box cannot show the tool apart of the body
    // and is not measured either (a hole through a wall of a rounded spline body measured
    // it for over a minute).
    let mut doc = Document::new("a cut through");
    block(&mut doc, "A", Vec3::ZERO, [20.0, 20.0, 20.0]);
    let kind = FeatureKind::Box {
        corner: point_expr(Vec3::new(5.0, 5.0, 15.0)),
        length: "10".into(),
        width: "10".into(),
        height: "10".into(),
        operation: Operation::Cut,
        centred: false,
    };
    let cut = doc.add_feature(kind, None).unwrap();
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.result(cut).is_some_and(|r| r.error.is_none()), "{:?}", model.result(cut));
    // The tool's tight box, and no other.
    assert_eq!(work.tight_boxes, 1, "{work:?}");
    assert_eq!(work.cut, 1, "{work:?}");
    let st = model.state();
    assert_eq!(st.bodies.len(), 1);
    // The tool crosses the top face: it takes 500 mm3 of the block with it.
    assert!((kernel::measure(&st.bodies[0].body).unwrap().volume - (8000.0 - 500.0)).abs() < 1e-6);

    // A tool clear of the box of the body's vertices: only the exact box can show it apart of
    // the body, so it is measured, and the body is left as it was without a boolean.
    let mut doc = Document::new("a cut beside");
    block(&mut doc, "A", Vec3::ZERO, [20.0, 20.0, 20.0]);
    let kind = FeatureKind::Box {
        corner: point_expr(Vec3::new(30.0, 5.0, 5.0)),
        length: "10".into(),
        width: "10".into(),
        height: "10".into(),
        operation: Operation::Cut,
        centred: false,
    };
    let cut = doc.add_feature(kind, None).unwrap();
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    assert!(model.result(cut).is_some(), "{:?}", model.result(cut));
    // The tool's tight box and the body's.
    assert_eq!(work.tight_boxes, 2, "{work:?}");
    assert_eq!(work.cut, 0, "{work:?}");
    let st = model.state();
    assert_eq!(st.bodies.len(), 1);
    assert!((kernel::measure(&st.bodies[0].body).unwrap().volume - 8000.0).abs() < 1e-6);
}

/// The design of #4282: a cube with a hole through it along each of the three axes, all of
/// them through the cube's centre, and a round on every edge of the result.
fn holed_cube(doc: &mut Document, size: f64, hole: f64) {
    block(doc, "Cube", Vec3::new(-size / 2.0, -size / 2.0, 0.0), [size, size, size]);
    // The three holes: a hole along each axis, its axis through the cube's centre.
    for (base, axis) in
        [(Vec3::new(-size / 2.0, 0.0, size / 2.0), Vec3::X), (Vec3::new(0.0, -size / 2.0, size / 2.0), Vec3::Y), (Vec3::new(0.0, 0.0, 0.0), Vec3::Z)]
    {
        let kind = FeatureKind::Cylinder {
            base: point_expr(base),
            axis: crate::dir_expr(axis),
            radius: (hole / 2.0).to_string().into(),
            height: size.to_string().into(),
            operation: Operation::Cut,
            references: Vec::new(),
        };
        doc.add_feature(kind, None).unwrap();
    }
}

/// The middle of every edge of the body, the points a round of all of them is picked with.
fn every_edge(body: &kernel::Body) -> Vec<Vec3> {
    let tol = (body.size() * 1e-3).max(1e-3);
    body.edges(tol).unwrap().iter().map(|e| e.mid).collect()
}

/// A round of every edge of the body, with `radius`, and the feature it makes.
fn round_of_every_edge(doc: &mut Document, body: &kernel::Body, radius: &str) -> u64 {
    let edges = every_edge(body);
    let kind = FeatureKind::Fillet {
        edges: edges.into_iter().map(Position).collect(),
        radius: radius.into(),
        body: None,
        style: Default::default(),
        fit: Default::default(),
        corner: Default::default(),
        setbacks: Vec::new(),
        turn: Default::default(),
        tangent_chain: true,
        edge_radii: Vec::new(),
    };
    doc.add_feature(kind, None).unwrap()
}

/// A round the kernel refuses without naming a radius that fits does not make the document
/// search for the largest radius that fits (#4282): the search cannot be guided then, and it
/// blends every edge again for each of the sizes it tries. On the design of #4282 (a cube with
/// a hole through each axis, a 2 mm round of every edge) those ten blends of the forty-one edges
/// took 77 s, fifteen times the blend that failed, and they found nothing: the round fails at
/// any radius there.
#[test]
fn a_refused_round_that_names_no_radius_to_try_does_not_search_for_one() {
    // The design of #4282 at a third of its size (the round and the holes keep their
    // proportions): a cube with a hole through each axis, a round of every one of its edges.
    // The kernel refuses the radius and cannot size the refusal.
    let mut doc = Document::new("holed cube");
    holed_cube(&mut doc, 10.0, 10.0 / 3.0);
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    let body = model.state().bodies[0].body.clone();
    let fillet = round_of_every_edge(&mut doc, &body, &format!("{} mm", 2.0 / 3.0));
    let work = counted(|| model.evaluate(&doc));
    let r = model.result(fillet);
    let error = r.and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(error.contains("larger than the faces beside its edges allow"), "{error}");
    // The blend the kernel refused, and no search for a radius that fits.
    assert_eq!(work.blends, 1, "{work:?}");
    assert_eq!(work.fit_searches, 0, "{work:?}");
}

/// A round the kernel refuses naming the radius that fits still searches for it: the search is
/// guided by that radius and settles in two or three blends, and the refusal names the largest
/// radius that fits.
#[test]
fn a_refused_round_that_names_a_radius_to_try_searches_for_it() {
    // A 10 mm box with a 6 mm round of all its edges: the radius is too large and the kernel
    // names the largest that fits (4.99 mm).
    let mut doc = Document::new("a box");
    block(&mut doc, "A", Vec3::ZERO, [10.0, 10.0, 10.0]);
    let mut model = Model::new();
    model.evaluate(&doc);
    let body = model.state().bodies[0].body.clone();
    let fillet = round_of_every_edge(&mut doc, &body, "6");
    let work = counted(|| model.evaluate(&doc));
    let r = model.result(fillet);
    let error = r.and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(error.contains("larger than the faces beside its edges allow"), "{error}");
    // The blend that was refused, and the search for the radius that fits.
    assert_eq!(work.blends, 1, "{work:?}");
    assert_eq!(work.fit_searches, 1, "{work:?}");
}

/// A round refused because it would cut through a thin wall into the hollow behind it names
/// the radius at which it opens, so the document still searches for the largest radius that
/// fits and names it (the kernel rig's 1668 case). A 104 x 64 x 42 box hollowed 2 mm from the
/// inside, open at the bottom: an R7 round of the outer top edge at (32, 42) has its centre at
/// (32 - r, 42 - r), and its arc uncovers the hollow's corner (30, 40) where sqrt2 (r - 2) = r,
/// r = 4 + 2 sqrt2 = 6.8284; the search builds 6.828.
#[test]
fn a_round_refused_into_a_hollow_searches_for_the_largest_radius_that_fits() {
    let mut doc = Document::new("thin wall");
    block(&mut doc, "Case", Vec3::new(-52.0, -32.0, 0.0), [104.0, 64.0, 42.0]);
    let hollow = FeatureKind::Box {
        corner: point_expr(Vec3::new(-50.0, -30.0, -1.0)),
        length: "100".into(),
        width: "60".into(),
        height: "41".into(),
        operation: Operation::Cut,
        centred: false,
    };
    doc.add_feature(hollow, None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    assert!(model.results.iter().all(|r| r.error.is_none()), "{:?}", model.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    let kind = FeatureKind::Fillet {
        edges: vec![Position(Vec3::new(0.0, 32.0, 42.0))],
        radius: "7".into(),
        body: None,
        style: Default::default(),
        fit: Default::default(),
        corner: Default::default(),
        setbacks: Vec::new(),
        turn: Default::default(),
        tangent_chain: true,
        edge_radii: Vec::new(),
    };
    let fillet = doc.add_feature(kind, None).unwrap();
    let work = counted(|| model.evaluate(&doc));
    let error = model.result(fillet).and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(error.contains("into the hollow at (0.000, 30.000, 40.000) and open the part"), "{error}");
    assert!(error.contains("the largest radius that fits is 6.828 mm"), "{error}");
    assert_eq!(work.blends, 1, "{work:?}");
    assert_eq!(work.fit_searches, 1, "{work:?}");
}

/// Whether a pattern's copies take material is read from the cut's own history first (#18396):
/// a copy whose tool leaves a wall in the body, on a surface the body did not have, took
/// material, and is neither probed with the kernel's point classifications (#11257) nor
/// intersected with the body.
#[test]
fn a_pattern_knows_its_copies_took_material_from_the_cut() {
    // The ten grooves of the pattern notch the same round bore. Each leaves its own round wall
    // (4 mm about its own axis, a surface the ring did not have) in the ring: none is probed.
    // The probe asked the kernel for 36 classifications here, batched a line to a call.
    let (faces, work) = grooved_ring_faces();
    assert_eq!(faces, 24, "the ring comes out as it did");
    assert_eq!((work.classifications, work.miss_intersects), (0, 0), "{work:?}");
}

/// A pattern's copies the cut's history cannot place are probed with the kernel's point
/// classifications (#11257), the points of a line in one call, not one call a point: the kernel
/// makes the body's copy and its search structures once for all the points of a call.
#[test]
fn a_pattern_probes_the_copies_the_cut_does_not_place_with_one_classification_a_line() {
    let mut doc = Document::new("in the bore");
    let cylinder = |base: Vec3, radius: &str, height: &str, operation: Operation| FeatureKind::Cylinder {
        base: point_expr(base),
        axis: crate::dir_expr(Vec3::Z),
        radius: radius.into(),
        height: height.into(),
        operation,
        references: Vec::new(),
    };
    doc.add_feature(cylinder(Vec3::ZERO, "25", "55", Operation::NewBody), Some("Ring")).unwrap();
    doc.add_feature(cylinder(Vec3::new(0.0, 0.0, -1.0), "16", "57", Operation::Cut), Some("Bore")).unwrap();
    // Sixteen pegs 2 mm round on a 10 mm circle, all in the empty bore (out to 12 mm of its 16):
    // each 3.9 mm from the next, so their 4 mm boxes meet and the cut can't place them.
    let peg = doc.add_feature(cylinder(Vec3::new(10.0, 0.0, 10.0), "2", "20", Operation::Cut), Some("Peg")).unwrap();
    let pattern = hole_pattern(&mut doc, peg, 16);
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    let result = model.result(pattern).unwrap();
    assert!(result.error.is_none(), "{result:?}");
    assert!(result.warning.as_deref().is_some_and(|w| w.starts_with("15 of 15 cut copies remove no material")), "{result:?}");
    // Each of the 15 copies is probed along 4 lines (its round side's axis and the three lines
    // through the middle of its box), each line with one call for the peg's points and one for
    // the ring's points among them (none in the ring): 15 x 4 x 2. The first line's points of
    // all 15 copies are asked of the peg they were turned from in one call, and of the ring in
    // one, not two calls a copy (#2358): 120 - 2 x (15 - 1).
    assert_eq!(work.classifications, 92, "{work:?}");
}

/// A body of two pieces can lose a whole piece to a copy that leaves none of its faces: that copy
/// took material, and the cut's history does not count it a miss.
#[test]
fn a_copy_that_takes_a_whole_piece_of_a_body_is_not_a_miss() {
    let block = |corner: Vec3, size: f64, operation: Operation| FeatureKind::Box {
        corner: point_expr(corner),
        length: size.to_string().into(),
        width: size.to_string().into(),
        height: size.to_string().into(),
        operation,
        centred: false,
    };
    let mut doc = Document::new("two pieces");
    doc.add_feature(block(Vec3::ZERO, 10.0, Operation::NewBody), Some("A")).unwrap();
    // A second piece of A, 4 mm across at x 30: the join meets nothing and keeps it apart.
    doc.add_feature(block(Vec3::new(30.0, 0.0, 0.0), 4.0, Operation::Join), Some("Piece")).unwrap();
    // A 6 mm cutter at x 18 to 24, clear of both pieces; its copy 11 mm on, x 29 to 35, holds the
    // whole second piece.
    let cutter = doc.add_feature(block(Vec3::new(18.0, -1.0, -1.0), 6.0, Operation::Cut), Some("Cutter")).unwrap();
    let pattern: FeatureKind = serde_json::from_value(serde_json::json!({
        "type": "pattern", "features": [cutter],
        "pattern": {"type": "rectangular", "dir1": [1, 0, 0], "count1": "2", "spacing1": "11"},
    }))
    .unwrap();
    let pattern = doc.add_feature(pattern, None).unwrap();
    let mut model = Model::new();
    model.evaluate(&doc);
    let result = model.result(pattern).unwrap();
    assert!(result.error.is_none(), "{result:?}");
    assert!(!result.warning.as_deref().is_some_and(|w| w.contains("cut copies remove no material")), "{:?}", result.warning);
    let st = model.state();
    let v: f64 = st.bodies.iter().map(|b| kernel::measure(&b.body).unwrap().volume).sum();
    assert!((v - 1000.0).abs() < 1e-6, "{v}");
}

/// A pattern's copies that leave no face in the body, each clear of the others, took no
/// material: the cut's history says so, and the count of the copies that remove no material is
/// the one the kernel's tests gave, without asking it (#18396).
#[test]
fn a_pattern_knows_its_copies_beside_the_body_took_no_material() {
    let mut doc = Document::new("beside the ring");
    let cylinder = |base: Vec3, radius: &str, height: &str, operation: Operation| FeatureKind::Cylinder {
        base: point_expr(base),
        axis: crate::dir_expr(Vec3::Z),
        radius: radius.into(),
        height: height.into(),
        operation,
        references: Vec::new(),
    };
    doc.add_feature(cylinder(Vec3::ZERO, "25", "55", Operation::NewBody), Some("Ring")).unwrap();
    // Every copy stands 60 mm from the axis: 4 mm round, 31 mm or more from the ring, and about
    // 38 mm from the next copy.
    let peg = doc.add_feature(cylinder(Vec3::new(60.0, 0.0, -1.0), "4", "57", Operation::Cut), Some("Peg")).unwrap();
    let pattern = hole_pattern(&mut doc, peg, 10);
    let mut model = Model::new();
    let work = counted(|| model.evaluate(&doc));
    let result = model.result(pattern).unwrap();
    assert!(result.error.is_none(), "{result:?}");
    assert!(result.warning.as_deref().is_some_and(|w| w.starts_with("9 of 9 cut copies remove no material")), "{result:?}");
    assert_eq!((work.classifications, work.miss_intersects), (0, 0), "{work:?}");
}
