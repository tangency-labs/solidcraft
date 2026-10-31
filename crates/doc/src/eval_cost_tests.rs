//! What an evaluation costs after an edit at the end of a large design: the features before
//! the edit are reused, so the edit's cost must not grow with the bodies and imported files
//! the design already holds. A sketch on a fixed plane once moved every body of every placed
//! component into its frame, and every evaluation wrote every imported STEP file out again to
//! fingerprint it, so each sketch command took longer the more parts were inserted.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;

use solidcraft_geom::{Plane, Vec2, Vec3};
use solidcraft_kernel as kernel;
use solidcraft_sketch::Sketch;

use crate::eval::{PlacementReads, body_world, hash_placement, placement_digest};
use crate::*;

/// Work counted on this thread: bodies moved, STEP texts digested, joint faces and bodies
/// measured, bodies searched for by name, features searched for by id, and bodies a cut (or
/// intersect) ran its booleans on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Counts {
    pub moved: usize,
    pub digested: usize,
    pub joint_faces: usize,
    pub measures: usize,
    pub body_searches: usize,
    pub feature_searches: usize,
    pub cut: usize,
}

thread_local! {
    static COUNTS: Cell<Counts> = const {
        Cell::new(Counts { moved: 0, digested: 0, joint_faces: 0, measures: 0, body_searches: 0, feature_searches: 0, cut: 0 })
    };
}

pub(super) fn count(f: impl FnOnce(&mut Counts)) {
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

/// The searches `f` makes on this thread through all features or all bodies.
fn searched(f: impl FnOnce()) -> usize {
    let n = counted(f);
    n.feature_searches + n.body_searches
}

/// The work `f` does on this thread.
fn counted(f: impl FnOnce()) -> Counts {
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
                axis: Dir(Vec3::Z),
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
        a: JointOrigin { occurrence: 0, snap: Snap::Frame { origin: pick, z: Vec3::Z, x: Some(Vec3::X) } },
        b: JointOrigin { occurrence, snap: Snap::FaceCenter { pick, face: None, x: Some(XRef::Dir(Vec3::X)) } },
        values: vec![0.0],
        limits: Vec::new(),
        flip: true,
        offset: 0.0,
        angle: 0.0,
        suppressed: false,
        exprs: Default::default(),
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
    let origin = JointOrigin { occurrence: 0, snap: Snap::FaceCenter { pick: Vec3::new(0.0, 0.0, 10.0), face: None, x: None } };
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
        let kind = FeatureKind::Import { file: format!("part{i}.step"), step, components: Vec::new(), skip: Vec::new() };
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
        PlaneRef::Construction { feature: plane.into() },
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
    let world = world_state(&doc, &state);
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
    };
    let plate = doc.add_feature(kind, None).unwrap();
    doc.feature_mut(plate).unwrap().component = comp;
    let mut model = Model::new();
    model.evaluate(doc);
    let body = model.state().bodies.iter().find(|b| b.feature == plate).unwrap().name.clone();
    for k in 0..holes {
        let corner = point_expr(Vec3::new(10.0 * k as f64 + 4.0, 4.0, -1.0));
        let kind = FeatureKind::Box { corner, length: "2".into(), width: "2".into(), height: "6".into(), operation: Operation::Cut };
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

#[test]
fn a_body_is_measured_once_while_it_stays_the_same() {
    let mut doc = Document::new("measured once");
    let cylinder = |doc: &mut Document, x: f64| {
        let kind = FeatureKind::Cylinder {
            base: point_expr(Vec3::new(x, 0.0, 0.0)),
            axis: Dir(Vec3::Z),
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
    let before = world_state_reusing(&doc, &state, &mut cache);
    let centroid = before.bodies[0].measure().unwrap().centroid;
    let again = world_state_reusing(&doc, &state, &mut cache);
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
    for edit in 0..5 {
        let FeatureKind::MeshImport { meshes, .. } = &mut Arc::make_mut(&mut doc).feature_mut(feature).unwrap().kind else { panic!("mesh") };
        let mesh = &mut meshes[0];
        match edit {
            0 => Arc::make_mut(&mut mesh.positions)[3] = 2.,
            1 => Arc::make_mut(&mut mesh.triangles).swap(1, 2),
            2 => mesh.color = Some([0.2, 0.4, 0.6]),
            3 => mesh.name = "Renamed".into(),
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
            let kind = FeatureKind::Box { corner, length: "5".into(), width: "5".into(), height: "5".into(), operation: Operation::NewBody };
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
        let kind = FeatureKind::Box { corner, length: "10".into(), width: "10".into(), height: "4".into(), operation: Operation::NewBody };
        let id = doc.add_feature(kind, None).unwrap();
        doc.feature_mut(id).unwrap().component = comp;
    }
    for k in 0..cuts {
        let corner = point_expr(Vec3::new(20.0 * k as f64 + 4.0, y + 4.0, -1.0));
        let kind = FeatureKind::Box { corner, length: "2".into(), width: "2".into(), height: "6".into(), operation: Operation::Cut };
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
                handedness: Default::default(),
                axis_point: Vec3::new(1.0, 1.0, 0.0),
                axis: solidcraft_geom::UnitVec3::Z,
                start: 0.0,
                end: 4.0,
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
        let n = searched(|| world = world_state(&doc, &state));
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
    let world = world_state(&doc, &state);
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
        axis: Dir(Vec3::Z),
        radius: radius.to_string().into(),
        height: height.to_string().into(),
        operation: Operation::Cut,
        references: Vec::new(),
    }
}

/// A cut still goes through the boolean wherever its tool reaches a body or comes close: on a
/// face, along a line, a hair beside it, into a dome above its edge circle. (Bodies its tool
/// cannot reach are left out of it: `a_cut_leaves_bodies_it_cannot_reach_as_they_were`.)
#[test]
fn a_cut_still_runs_where_its_tool_touches_grazes_or_meets_a_curved_face() {
    // A cylinder whose side is tangent to the face x = 50, one a hair clear of it, one 0.1 mm in.
    for (x, cuts) in [(53.0, false), (53.0005, false), (52.9, true)] {
        let mut doc = Document::new("graze");
        block(&mut doc, "A", Vec3::ZERO, [50.0, 40.0, 30.0]);
        let work = cut_work(&mut doc, cutter(Vec3::new(x, 20.0, 0.0), 3.0, 30.0));
        assert_eq!(work.cut, 1, "the tool at x = {x} goes through the boolean");
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
