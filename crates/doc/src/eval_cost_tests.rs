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

use crate::*;

/// Work counted on this thread: bodies moved, STEP texts digested, joint faces and bodies
/// measured.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Counts {
    pub moved: usize,
    pub digested: usize,
    pub joint_faces: usize,
    pub measures: usize,
}

thread_local! {
    static COUNTS: Cell<Counts> = const { Cell::new(Counts { moved: 0, digested: 0, joint_faces: 0, measures: 0 }) };
}

pub(super) fn count(f: impl FnOnce(&mut Counts)) {
    COUNTS.with(|c| {
        let mut n = c.get();
        f(&mut n);
        c.set(n);
    });
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

#[test]
fn editing_a_sketch_on_a_turned_occurrence_reuses_its_frame_bodies() {
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
