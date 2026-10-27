use std::collections::BTreeSet;

use solidcraft_geom::{Vec2, Vec3};
use solidcraft_sketch::{ConstraintKind, Sketch};

use super::*;
use crate::joints::{Joint, JointExprs, JointKind, JointOrigin, Snap};
use crate::{IDENTITY, Operation, PlaneRef, point_expr};

fn add_box(d: &mut Document, name: &str, length: &str) -> u64 {
    let kind = FeatureKind::Box {
        corner: point_expr(Vec3::ZERO),
        length: length.into(),
        width: "10".into(),
        height: "10".into(),
        operation: Operation::NewBody,
    };
    d.add_feature(kind, Some(name)).unwrap()
}

/// Three boxes, `A`, `B` and `C` (ids 1, 2, 3).
fn three_boxes() -> Document {
    let mut d = Document::new("parts");
    for name in ["A", "B", "C"] {
        add_box(&mut d, name, "10");
    }
    d
}

fn kinds(k: &[ChangeKind]) -> BTreeSet<ChangeKind> {
    k.iter().copied().collect()
}

fn names(c: &FeatureChange) -> (&str, &str) {
    (&c.names.0, &c.names.1)
}

/// Each target with how it was touched.
fn hows(t: &Touched) -> Vec<(Target, How)> {
    t.targets.iter().map(|(t, touch)| (t.clone(), touch.how)).collect()
}

fn edited(t: &Touched) -> Vec<Target> {
    t.targets.iter().filter(|(_, touch)| touch.how == How::Edited).map(|(t, _)| t.clone()).collect()
}

fn input(expression: &str, value: f64) -> Option<InputValue> {
    Some(InputValue { expression: expression.into(), value: Some(value) })
}

#[test]
fn the_same_design_has_no_changes() {
    let d = three_boxes();
    let diff = diff(&d, &d.clone());
    assert!(
        diff.features.is_empty() && diff.params.is_empty() && diff.components.is_empty() && diff.joints.is_empty() && diff.other.is_empty(),
        "{diff:?}"
    );
    assert_eq!(Touched::of(&diff, &d), Touched::default());
}

/// "B: Length 10 mm → 12 mm": the input, with its parameter name, values and unit, and not
/// again as a definition field.
#[test]
fn a_changed_input_is_reported_with_both_expressions() {
    let base = three_boxes();
    let mut head = base.clone();
    head.features[1].param_names = vec!["d1".into()];
    let FeatureKind::Box { length, .. } = &mut head.features[1].kind else { panic!("a box") };
    *length = "12".into();
    let diff = diff(&base, &head);
    let [b] = diff.features.as_slice() else { panic!("{diff:?}") };
    assert_eq!((b.id, b.type_name, names(b)), (2, "BoxFeature", ("B", "B")));
    assert_eq!(b.kinds, kinds(&[ChangeKind::Changed]));
    let length = InputChange { label: "Length", param: Some("d1".into()), before: input("10", 10.0), after: input("12", 12.0), unit: "mm" };
    assert_eq!(b.inputs, [length]);
    assert!(b.fields.is_empty(), "{:?}", b.fields);
    // An expression over a parameter: its value as evaluation works it out.
    let mut base = base;
    base.set_param("len", "2 in", Some("in"), None).unwrap();
    let FeatureKind::Box { length, .. } = &mut base.features[1].kind else { panic!("a box") };
    *length = "len".into();
    let mut head = base.clone();
    head.set_param("len", "4 in", Some("in"), None).unwrap();
    let FeatureKind::Box { length, .. } = &mut head.features[1].kind else { panic!("a box") };
    *length = "len / 2".into();
    let diff = super::diff(&base, &head);
    let length = &diff.features[0].inputs[0];
    // Each side in its own design: 2 in before, half of 4 in after.
    assert_eq!((&length.before, &length.after), (&input("len", 50.8), &input("len / 2", 50.8)));
}

/// An optional input that comes or goes (a revolve's second angle) is one input change, and
/// not again as a field.
#[test]
fn an_optional_input_is_reported_once() {
    let mut base = Document::new("turned");
    let sketch = base.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XZ".into() }, sketch: Sketch::new() }, None).unwrap();
    let revolve = FeatureKind::Revolve {
        sketch: crate::SketchId(sketch),
        profiles: crate::ProfileSel::All,
        axis: crate::AxisRef::World { axis: "Z".into() },
        angle: "90 deg".into(),
        operation: Operation::NewBody,
        targets: Vec::new(),
        angle2: None,
        symmetric: false,
        convention: Default::default(),
        to: None,
    };
    base.add_feature(revolve, Some("Turn")).unwrap();
    let mut head = base.clone();
    let FeatureKind::Revolve { angle2, .. } = &mut head.features[1].kind else { panic!("a revolve") };
    *angle2 = Some("30 deg".into());
    let diff = diff(&base, &head);
    let [turn] = diff.features.as_slice() else { panic!("{diff:?}") };
    assert!(turn.fields.is_empty(), "{:?}", turn.fields);
    let [angle2] = turn.inputs.as_slice() else { panic!("{turn:?}") };
    let after = angle2.after.as_ref().map(|a| (a.expression.as_str(), a.value.map(|v| (v - 30.0).abs() < 1e-9)));
    assert_eq!((angle2.label, &angle2.param, &angle2.before, after, angle2.unit), ("Angle 2", &None, &None, Some(("30 deg", Some(true))), "deg"));
    // And going away.
    let diff = super::diff(&head, &base);
    assert!(diff.features[0].fields.is_empty() && diff.features[0].inputs.len() == 1, "{diff:?}");
}

/// A definition field that isn't an input (the box's corner, its operation) is named.
#[test]
fn other_definition_fields_are_named() {
    let base = three_boxes();
    let mut head = base.clone();
    let FeatureKind::Box { corner, operation, .. } = &mut head.features[0].kind else { panic!("a box") };
    *corner = point_expr(Vec3::new(5.0, 0.0, 0.0));
    *operation = Operation::Join;
    let diff = diff(&base, &head);
    let [a] = diff.features.as_slice() else { panic!("{diff:?}") };
    assert_eq!((a.fields.as_slice(), a.inputs.len()), (["corner".to_string(), "operation".to_string()].as_slice(), 0));
}

#[test]
fn added_removed_renamed_and_suppressed_features() {
    let mut base = three_boxes();
    base.features[0].suppressed = true;
    let mut head = base.clone();
    head.features.retain(|f| f.name != "B");
    head.features[0].name = "Base".into();
    head.features[0].suppressed = false;
    head.features[1].suppressed = true;
    add_box(&mut head, "D", "5");
    let diff = diff(&base, &head);
    let got: Vec<(u64, (&str, &str), BTreeSet<ChangeKind>)> = diff.features.iter().map(|c| (c.id, names(c), c.kinds.clone())).collect();
    assert_eq!(
        got,
        [
            (1, ("A", "Base"), kinds(&[ChangeKind::Renamed, ChangeKind::Unsuppressed])),
            (3, ("C", "C"), kinds(&[ChangeKind::Suppressed])),
            (4, ("", "D"), kinds(&[ChangeKind::Added])),
            (2, ("B", ""), kinds(&[ChangeKind::Removed])),
        ]
    );
    // What the base had, edited; the new box is an addition inside the design.
    let touched = Touched::of(&diff, &base);
    assert_eq!(
        hows(&touched),
        [
            (Target::Component(0), How::AddedInside),
            (Target::Feature(1), How::Edited),
            (Target::Feature(2), How::Edited),
            (Target::Feature(3), How::Edited),
        ]
    );
    let names = |t: Target| touched.targets.get(&t).map(|touch| touch.names.clone());
    assert_eq!(names(Target::Feature(1)), Some(("A".into(), "Base".into())));
    assert_eq!(names(Target::Feature(2)), Some(("B".into(), String::new())));
}

/// #1017: ids come from each design's own counter, so a feature someone added can have an id
/// another branch used. Features pair by id with the base's: an id the base doesn't have is new,
/// even where the base's counter lagged behind its own ids.
#[test]
fn an_id_the_base_does_not_have_is_added() {
    let mut base = three_boxes();
    base.next_id = 3; // behind its own feature 3
    base.timeline_groups.push(crate::TimelineGroup { id: 9, name: "Folder".into(), parent: None, ..Default::default() });
    let mut head = base.clone();
    let FeatureKind::Box { length, .. } = &mut head.features[2].kind else { panic!("a box") };
    *length = "20".into();
    let mut d = head.features[2].clone();
    (d.id, d.name) = (10, "D".into());
    head.features.push(d);
    let diff = diff(&base, &head);
    let got: Vec<(u64, BTreeSet<ChangeKind>)> = diff.features.iter().map(|c| (c.id, c.kinds.clone())).collect();
    assert_eq!(got, [(3, kinds(&[ChangeKind::Changed])), (10, kinds(&[ChangeKind::Added]))]);
    // A feature of another type under the same id is not the same feature either.
    let mut other = three_boxes();
    other.features[1].kind = FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: Sketch::new() };
    let got: Vec<(u64, BTreeSet<ChangeKind>)> = super::diff(&three_boxes(), &other).features.iter().map(|c| (c.id, c.kinds.clone())).collect();
    assert_eq!(got, [(2, kinds(&[ChangeKind::Added])), (2, kinds(&[ChangeKind::Removed]))]);
    // A second feature with a base id is new: an addition, not an edit of the first.
    let base = three_boxes();
    let mut head = base.clone();
    head.features.push(head.features[0].clone());
    let diff = super::diff(&base, &head);
    let got: Vec<(u64, BTreeSet<ChangeKind>)> = diff.features.iter().map(|c| (c.id, c.kinds.clone())).collect();
    assert_eq!(got, [(1, kinds(&[ChangeKind::Added]))]);
    assert_eq!(hows(&Touched::of(&diff, &base)), [(Target::Component(0), How::AddedInside)]);
}

/// Moving one feature to the front moves only it: the others keep their order among themselves.
#[test]
fn only_the_feature_that_moved_is_moved() {
    let mut base = three_boxes();
    add_box(&mut base, "D", "10");
    let mut head = base.clone();
    let d = head.features.remove(3);
    head.features.insert(0, d);
    let moved: Vec<u64> = diff(&base, &head).features.iter().map(|c| c.id).collect();
    assert_eq!(moved, [4]);
    // Two swapped neighbours: one of them moved.
    let mut head = base.clone();
    head.features.swap(1, 2);
    let diff = diff(&base, &head);
    assert_eq!(diff.features.len(), 1, "{diff:?}");
    assert_eq!(diff.features[0].kinds, kinds(&[ChangeKind::Moved]));
}

/// Evaluation pins the names it gave the bodies (`Body1`, `Kept (3)`) into empty slots, and
/// re-picks edge and face names: none of that is a change. A name that was set changing, or a
/// person's own name in an empty slot (a first `body.rename` on an older design), is.
#[test]
fn names_evaluation_writes_back_are_not_changes() {
    let mut base = three_boxes();
    base.features[0].body_names = vec![String::new(), "Kept".into()];
    let mut head = base.clone();
    head.features[0].body_names = vec!["Body1".into(), "Kept".into(), "Kept (3)".into()];
    head.features[0].edge_names = vec!["F1:top|F1:side".into()];
    head.features[0].face_names = vec!["F1:top".into()];
    assert!(diff(&base, &head).features.is_empty());
    let body_names = |head: &Document| super::diff(&base, head).features.first().map(|a| (a.fields.clone(), a.kinds.clone()));
    let changed = Some((vec!["body_names".to_string()], kinds(&[ChangeKind::Changed])));
    let mut renamed = head.clone();
    renamed.features[0].body_names[1] = "Renamed".into();
    assert_eq!(body_names(&renamed), changed);
    let mut first_rename = head.clone();
    first_rename.features[0].body_names[0] = "Lid".into();
    assert_eq!(body_names(&first_rename), changed);
}

/// An Import's STEP text is compared where it is; a linked design's face names come with a new
/// STEP text from Update Links, so alone they are a person's edit.
#[test]
fn heavy_fields_and_link_faces() {
    let mut base = Document::new("linked");
    let import = serde_json::json!({"id": 1, "name": "Link", "type": "import", "file": "part.step", "step": "ISO-10303-21; A"});
    base.features.push(serde_json::from_value(import).unwrap());
    base.next_id = 2;
    let fields = |head: &Document| super::diff(&base, head).features.first().map(|c| c.fields.clone()).unwrap_or_default();
    let mut relinked = base.clone();
    let FeatureKind::Import { step, .. } = &mut relinked.features[0].kind else { panic!("an import") };
    *step = "ISO-10303-21; B".into();
    relinked.features[0].link_faces = vec![vec!["top".into()]];
    assert_eq!(fields(&relinked), ["step"]);
    let mut edited = base.clone();
    edited.features[0].link_faces = vec![vec!["top".into()]];
    assert_eq!(fields(&edited), ["link_faces"]);
}

/// A sketch: a 10 x 10 square on XY with its side dimensioned, after box `A`.
fn with_sketch() -> (Document, u64) {
    let mut d = three_boxes();
    let mut sk = Sketch::new();
    let p: Vec<usize> =
        [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)].iter().map(|(x, y)| sk.add_point(Vec2::new(*x, *y), None).unwrap()).collect();
    let l: Vec<usize> = (0..4).map(|i| sk.add_line_pts(p[i], p[(i + 1) % 4], None).unwrap()).collect();
    let d1 = d.new_model_param("10 mm", "mm");
    sk.add_constraint(ConstraintKind::Length { l: l[0], value: 10.0 }, Some(d1)).unwrap();
    let id = d.add_feature(FeatureKind::Sketch { plane: PlaneRef::Origin { name: "XY".into() }, sketch: sk }, Some("Square")).unwrap();
    (d, id)
}

/// A sketch solved again for a new dimension has new positions and values: that is `Resolved`,
/// and the dimension's change touches the sketch. Points moved with nothing the sketch uses
/// changed are a person's drag, and new curves are a change.
#[test]
fn a_sketch_solved_again_is_resolved() {
    let (base, id) = with_sketch();
    let mut head = base.clone();
    head.params[0].expr = "12 mm".into();
    let sk = head.sketch_mut(id).unwrap();
    sk.points[2].pos = Vec2::new(12.0, 10.0);
    sk.points[3].pos = Vec2::new(12.0, 0.0);
    sk.constraints[0].kind.set_value(12.0);
    sk.view.hide_constraints = true;
    let diff = diff(&base, &head);
    let [sketch] = diff.features.as_slice() else { panic!("{diff:?}") };
    assert_eq!((sketch.id, &sketch.kinds, sketch.fields.len()), (id, &kinds(&[ChangeKind::Resolved]), 0));
    let [d1] = diff.params.as_slice() else { panic!("{diff:?}") };
    assert_eq!(
        (d1.kind, d1.before.as_ref().and_then(|r| r.value), d1.after.as_ref().and_then(|r| r.value)),
        (ChangeKind::Changed, Some(10.0), Some(12.0))
    );
    assert_eq!(edited(&Touched::of(&diff, &base)), [Target::Feature(id), Target::Parameter(d1.name.clone())]);

    let mut dragged = head.clone();
    dragged.params[0].expr = "10 mm".into();
    dragged.sketch_mut(id).unwrap().constraints[0].kind.set_value(10.0);
    let diff = super::diff(&base, &dragged);
    let [sketch] = diff.features.as_slice() else { panic!("{diff:?}") };
    assert_eq!((&sketch.kinds, sketch.fields.as_slice()), (&kinds(&[ChangeKind::Changed]), ["sketch".to_string()].as_slice()));
    // The same drag after box `A` changed is still a drag: the sketch projects nothing, so `A`
    // can't have moved it.
    let FeatureKind::Box { length, .. } = &mut dragged.features[0].kind else { panic!("a box") };
    *length = "12".into();
    let diff = super::diff(&base, &dragged);
    let [_, sketch] = diff.features.as_slice() else { panic!("{diff:?}") };
    assert_eq!((&sketch.kinds, sketch.fields.as_slice()), (&kinds(&[ChangeKind::Changed]), ["sketch".to_string()].as_slice()));

    let sk = head.sketch_mut(id).unwrap();
    let (a, b) = (sk.add_point(Vec2::new(0.0, 0.0), None).unwrap(), sk.add_point(Vec2::new(5.0, 5.0), None).unwrap());
    sk.add_line_pts(a, b, None).unwrap();
    let diff = super::diff(&base, &head);
    assert_eq!(diff.features[0].fields, ["sketch"]);
    assert_eq!(diff.features[0].kinds, kinds(&[ChangeKind::Changed]));
}

/// User parameters pair by name: a new expression is Changed with both values; a rename is one
/// removed and one added. The base's parameters are edited; a new one is an addition to the
/// design.
#[test]
fn parameters_pair_by_name() {
    let mut base = three_boxes();
    base.set_param("width", "40 mm", Some("mm"), None).unwrap();
    base.set_param("gap", "2 mm", Some("mm"), None).unwrap();
    let mut head = base.clone();
    head.set_param("width", "45 mm", Some("mm"), None).unwrap();
    head.params.iter_mut().filter(|p| p.name == "gap").for_each(|p| p.name = "clearance".into());
    let diff = diff(&base, &head);
    let got: Vec<(&str, ChangeKind, Option<f64>, Option<f64>)> = diff
        .params
        .iter()
        .map(|p| (p.name.as_str(), p.kind, p.before.as_ref().and_then(|r| r.value), p.after.as_ref().and_then(|r| r.value)))
        .collect();
    assert_eq!(
        got,
        [
            ("width", ChangeKind::Changed, Some(40.0), Some(45.0)),
            ("clearance", ChangeKind::Added, None, Some(2.0)),
            ("gap", ChangeKind::Removed, Some(2.0), None)
        ]
    );
    let touched = Touched::of(&diff, &base);
    assert_eq!(
        hows(&touched),
        [(Target::Component(0), How::AddedInside), (Target::Parameter("gap".into()), How::Edited), (Target::Parameter("width".into()), How::Edited)]
    );
    assert_eq!(touched.targets.get(&Target::Parameter("gap".into())).map(|t| t.names.clone()), Some(("gap".into(), String::new())));
}

/// A parameter whose expression stayed but whose value follows one that changed is reported
/// `Recomputed`, and touches what uses it; the parameter itself is not edited.
#[test]
fn values_that_follow_a_change_are_recomputed() {
    let mut base = three_boxes();
    base.set_param("thick", "2 mm", Some("mm"), None).unwrap();
    base.set_param("width", "thick * 5", Some("mm"), None).unwrap();
    let FeatureKind::Box { length, .. } = &mut base.features[1].kind else { panic!("a box") };
    *length = "width".into();
    let mut head = base.clone();
    head.set_param("thick", "3 mm", Some("mm"), None).unwrap();
    let diff = diff(&base, &head);
    let got: Vec<(&str, ChangeKind, Option<f64>, Option<f64>)> = diff
        .params
        .iter()
        .map(|p| (p.name.as_str(), p.kind, p.before.as_ref().and_then(|r| r.value), p.after.as_ref().and_then(|r| r.value)))
        .collect();
    assert_eq!(got, [("thick", ChangeKind::Changed, Some(2.0), Some(3.0)), ("width", ChangeKind::Recomputed, Some(10.0), Some(15.0))]);
    assert!(diff.features.is_empty(), "B's expression is the same: {diff:?}");
    assert_eq!(edited(&Touched::of(&diff, &base)), [Target::Feature(2), Target::Parameter("thick".into())]);
}

/// The design's other fields are named, and touch the design; view state (the marker, what is
/// hidden, named views, browser order and folders) is not a change.
#[test]
fn other_fields_are_named_and_view_state_is_ignored() {
    let base = three_boxes();
    let mut head = base.clone();
    head.marker = Some(1);
    head.visibility.hidden_bodies = vec!["Body1".into()];
    head.named_views.push(crate::NamedView::default());
    head.browser_order.insert("0/bodies".into(), vec!["Body2".into()]);
    head.timeline_groups.push(crate::TimelineGroup { id: 9, name: "Folder".into(), parent: None, features: vec![1], ..Default::default() });
    assert!(diff(&base, &head).other.is_empty());
    head.materials.insert("Body1".into(), "Steel".into());
    head.units = "in".into();
    let diff = diff(&base, &head);
    assert_eq!(diff.other, ["units", "materials"]);
    assert_eq!(hows(&Touched::of(&diff, &base)), [(Target::Field("materials"), How::Edited), (Target::UNITS, How::Edited)]);
    // Each field under its own name.
    let cases: [(&str, fn(&mut Document)); 14] = [
        ("name", |d| d.name = "renamed".into()),
        ("metadata", |d| {
            d.metadata.0.insert("project".into(), "X".into());
        }),
        ("parts", |d| {
            d.parts.insert(7, crate::StandardPart { family: "bolt".into(), size: "M3".into(), length: None });
        }),
        ("links", |d| {
            d.links.insert(7, crate::DesignLink { path: "a.solidcraft".into(), feature: 1, ..Default::default() });
        }),
        ("favorites", |d| {
            d.favorites.insert("width".into());
        }),
        ("param_comments", |d| {
            d.param_comments.insert("d1".into(), "why".into());
        }),
        ("materials", |d| {
            d.materials.insert("Body1".into(), "Steel".into());
        }),
        ("user_materials", |d| {
            d.user_materials.push(crate::UserMaterial { name: "Foam".into(), density: 0.1, appearance: None, physical_card: None })
        }),
        ("appearances", |d| {
            d.appearances.components.insert(7, crate::appearance::Look::custom([1, 2, 3], 1.0));
        }),
        ("body_components", |d| {
            d.body_components.insert("Body1".into(), 7);
        }),
        ("body_offsets", |d| {
            d.body_offsets.insert("Body1".into(), IDENTITY);
        }),
        ("print_faces", |d| {
            let face = crate::config::ConfigFaceTarget {
                body: "Body1".into(),
                origin_feature: 1,
                name: "F1:+z".into(),
                point: Vec3::new(0.0, 0.0, 1.0),
                certificate: None,
            };
            d.print_faces.insert("Body1".into(), face);
        }),
        ("sheet", |d| d.sheet.active = Some("Steel 1 mm".into())),
        ("plastic", |d| d.plastic.active = Some("ABS".into())),
    ];
    for (name, change) in cases {
        let mut head = base.clone();
        change(&mut head);
        assert_eq!(super::diff(&base, &head).other, [name]);
    }
}

/// An arm placed twice (the first placement grounded, the second on a hinge joint whose angle
/// is `hinge_angle`) and a loose bracket.
fn assembly() -> Document {
    assembly_on(three_boxes())
}

/// [`assembly`] added to `d`.
fn assembly_on(mut d: Document) -> Document {
    d.set_param("hinge_angle", "10 deg", Some("deg"), None).unwrap();
    let arm = d.add_component(Some("Arm"), 0).unwrap();
    d.add_component(Some("Bracket"), 0).unwrap();
    let a = d.add_occurrence(arm, 0, IDENTITY).unwrap();
    let origin = |occurrence| JointOrigin { occurrence, snap: Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: None } };
    d.assembly.joints.push(Joint {
        id: 99,
        name: "Hinge".into(),
        kind: JointKind::Revolute,
        a: origin(0),
        b: origin(a),
        values: vec![10f64.to_radians()],
        limits: Vec::new(),
        flip: false,
        offset: 0.0,
        angle: 0.0,
        suppressed: false,
        exprs: JointExprs { values: vec![Some("hinge_angle".into())], ..Default::default() },
    });
    d
}

/// Move placement `id` 5 mm along x.
fn shift(d: &mut Document, id: u64) {
    d.occurrences.iter_mut().filter(|o| o.id == id).for_each(|o| o.transform[3][0] += 5.0);
}

/// A placement the joints moved is `Resolved` while the joints are defined as they were (the
/// values their expressions drive are worked out again); one a person moved (no joint places
/// it, or a joint's own input changed) is a change, and a target.
#[test]
fn placements_the_joints_moved_are_resolved() {
    let base = assembly();
    let (arm, bracket) = (base.components[0].id, base.components[1].id);
    let hinged = base.assembly.joints[0].b.occurrence;
    let mut head = base.clone();
    head.set_param("hinge_angle", "20 deg", Some("deg"), None).unwrap();
    head.assembly.joints[0].values = vec![20f64.to_radians()];
    shift(&mut head, hinged);
    let diff = diff(&base, &head);
    let [c] = diff.components.as_slice() else { panic!("{diff:?}") };
    assert_eq!((c.id, &c.kinds, c.fields.len()), (arm, &kinds(&[ChangeKind::Resolved]), 0));
    assert!(diff.joints.is_empty() && diff.other.is_empty(), "{diff:?}");
    // The angle's change touches the joint it drives.
    assert_eq!(edited(&Touched::of(&diff, &base)), [Target::Joint(99), Target::Parameter("hinge_angle".into())]);

    let loose = head.occurrences.iter().find(|o| o.component == bracket).unwrap().id;
    shift(&mut head, loose);
    head.assembly.joints[0].flip = true;
    let diff = super::diff(&base, &head);
    let got: Vec<(u64, BTreeSet<ChangeKind>, Vec<&str>)> = diff.components.iter().map(|c| (c.id, c.kinds.clone(), c.fields.clone())).collect();
    assert_eq!(got, [(arm, kinds(&[ChangeKind::Changed]), vec!["placement"]), (bracket, kinds(&[ChangeKind::Changed]), vec!["placement"])]);
    assert!(diff.other.is_empty(), "joints are reported as joints: {diff:?}");
    let [hinge] = diff.joints.as_slice() else { panic!("{diff:?}") };
    assert_eq!((hinge.id, &hinge.kinds), (99, &kinds(&[ChangeKind::Changed])));
    assert_eq!(
        edited(&Touched::of(&diff, &base)),
        [Target::Component(arm), Target::Component(bracket), Target::Joint(99), Target::Parameter("hinge_angle".into())]
    );
}

/// Joints pair by id: added, removed and renamed ones; the rest of the assembly is `other`.
#[test]
fn joints_added_removed_and_renamed() {
    let base = assembly();
    let mut head = base.clone();
    head.assembly.joints[0].name = "Pivot".into();
    let mut second = head.assembly.joints[0].clone();
    (second.id, second.name) = (100, "Stop".into());
    head.assembly.joints.push(second);
    let diff = diff(&base, &head);
    let got: Vec<(u64, BTreeSet<ChangeKind>)> = diff.joints.iter().map(|j| (j.id, j.kinds.clone())).collect();
    assert_eq!(got, [(99, kinds(&[ChangeKind::Renamed])), (100, kinds(&[ChangeKind::Added]))]);
    assert_eq!(hows(&Touched::of(&diff, &base)), [(Target::Component(0), How::AddedInside), (Target::Joint(99), How::Edited)]);
    let got: Vec<(u64, BTreeSet<ChangeKind>)> = super::diff(&head, &base).joints.iter().map(|j| (j.id, j.kinds.clone())).collect();
    assert_eq!(got, [(99, kinds(&[ChangeKind::Renamed])), (100, kinds(&[ChangeKind::Removed]))]);
    let mut head = base.clone();
    head.assembly.contact_enabled = true;
    assert_eq!(super::diff(&base, &head).other, ["assembly"]);
}

#[test]
fn components_added_removed_renamed_moved_and_their_placements() {
    let base = assembly();
    let (arm, bracket) = (base.components[0].id, base.components[1].id);
    let mut head = base.clone();
    head.components[0].name = "Lever".into();
    head.components[0].properties.insert("finish".into(), "anodised".into());
    head.components[0].history_hidden = true;
    head.components.retain(|c| c.id != bracket);
    let hinged = head.assembly.joints[0].b.occurrence;
    head.occurrences.retain(|o| o.component != bracket && o.id != hinged);
    let new = head.add_component(Some("Pin"), arm).unwrap();
    let diff = diff(&base, &head);
    let got: Vec<(u64, (&str, &str), BTreeSet<ChangeKind>, Vec<&str>)> =
        diff.components.iter().map(|c| (c.id, (c.names.0.as_str(), c.names.1.as_str()), c.kinds.clone(), c.fields.clone())).collect();
    assert_eq!(
        got,
        [
            (arm, ("Arm", "Lever"), kinds(&[ChangeKind::Changed, ChangeKind::Renamed]), vec!["history_hidden", "occurrences", "properties"]),
            (new, ("", "Pin"), kinds(&[ChangeKind::Added]), vec!["occurrences"]),
            (bracket, ("Bracket", ""), kinds(&[ChangeKind::Removed]), vec!["occurrences"]),
        ]
    );
    // The new component is an addition inside the arm, which is edited anyway.
    assert_eq!(hows(&Touched::of(&diff, &base)), [(Target::Component(arm), How::Edited), (Target::Component(bracket), How::Edited)]);
    // A component put into another is moved.
    let mut moved = base.clone();
    moved.components.iter_mut().filter(|c| c.id == arm).for_each(|c| c.parent = bracket);
    let diff = super::diff(&base, &moved);
    let [c] = diff.components.as_slice() else { panic!("{diff:?}") };
    assert_eq!((c.id, &c.kinds), (arm, &kinds(&[ChangeKind::Moved])));
}

/// A design with a frame holding a hinge, with feature `Pin` made inside the hinge.
fn nested() -> (Document, u64, u64, u64) {
    let mut d = three_boxes();
    let frame = d.add_component(Some("Frame"), 0).unwrap();
    let hinge = d.add_component(Some("Hinge"), frame).unwrap();
    let pin = add_box(&mut d, "Pin", "3");
    d.feature_mut(pin).unwrap().component = hinge;
    (d, frame, hinge, pin)
}

fn touched(targets: &[Target]) -> Touched {
    Touched { targets: targets.iter().map(|t| (t.clone(), Touch { names: Default::default(), how: How::Edited })).collect() }
}

/// Overlaps: the same target, a component and what is inside it at any depth, a feature and the
/// bodies it made. Nothing else, and never an addition.
#[test]
fn overlaps_are_shared_targets_or_one_inside_the_other() {
    let (d, frame, hinge, pin) = nested();
    let body = Target::Body(BodyKey { name: "Body4".into(), feature: pin });
    let mine = touched(&[Target::Feature(pin), Target::Feature(1), Target::Parameter("width".into())]);
    assert_eq!(mine.overlaps(&touched(&[Target::Feature(1)]), &d), [Target::Feature(1)]);
    assert_eq!(mine.overlaps(&touched(&[Target::Component(frame)]), &d), [Target::Feature(pin)]);
    assert_eq!(mine.overlaps(&touched(&[Target::Component(hinge)]), &d), [Target::Feature(pin)]);
    assert_eq!(mine.overlaps(&touched(std::slice::from_ref(&body)), &d), [Target::Feature(pin)]);
    assert_eq!(mine.overlaps(&touched(&[Target::Parameter("width".into())]), &d), [Target::Parameter("width".into())]);
    assert!(mine.overlaps(&touched(&[Target::Feature(2), Target::Parameter("gap".into()), Target::Component(99)]), &d).is_empty());
    // And the other way round.
    assert_eq!(touched(&[Target::Component(frame)]).overlaps(&mine, &d), [Target::Component(frame)]);
    assert_eq!(touched(&[Target::Component(frame)]).overlaps(&touched(&[Target::Component(hinge)]), &d), [Target::Component(frame)]);
    assert!(touched(&[Target::Component(hinge)]).overlaps(&touched(&[Target::Feature(1)]), &d).is_empty());
    assert_eq!(touched(std::slice::from_ref(&body)).overlaps(&touched(&[Target::Feature(pin)]), &d), [body]);
    assert_eq!(touched(&[Target::Joint(5)]).overlaps(&touched(&[Target::Joint(5), Target::Joint(6)]), &d), [Target::Joint(5)]);
    // Additions inside a component don't overlap: two people adding is #1017's question.
    let mut adding = touched(&[Target::Component(hinge)]);
    adding.targets.values_mut().for_each(|t| t.how = How::AddedInside);
    assert!(adding.overlaps(&touched(&[Target::Component(hinge)]), &d).is_empty());
    assert!(touched(&[Target::Component(hinge)]).overlaps(&adding, &d).is_empty());
}

/// An edit of the design's own fields overlaps only an edit of the same field (materials with
/// materials, render with render), never a part's; a units change overlaps everything touched,
/// joints, parameters and additions included.
#[test]
fn a_design_field_overlaps_the_same_field_and_units_overlap_everything() {
    let base = assembly();
    let touched_by = |change: &dyn Fn(&mut Document)| {
        let mut head = base.clone();
        change(&mut head);
        Touched::of(&diff(&base, &head), &base)
    };
    let render = touched_by(&|d| {
        std::sync::Arc::make_mut(&mut d.render).setups.insert("Studio".into(), Default::default());
    });
    let steel = touched_by(&|d| {
        d.materials.insert("Body1".into(), "Steel".into());
    });
    let brass = touched_by(&|d| {
        d.materials.insert("Body2".into(), "Brass".into());
    });
    let units = touched_by(&|d| d.units = "in".into());
    let lengthen = touched_by(&|d| {
        if let Some(FeatureKind::Box { length, .. }) = d.feature_mut(2).map(|f| &mut f.kind) {
            *length = "12".into();
        }
    });
    let angle = touched_by(&|d| {
        d.set_param("hinge_angle", "20 deg", Some("deg"), None).unwrap();
    });
    assert_eq!(edited(&lengthen), [Target::Feature(2)]);
    assert_eq!(edited(&angle), [Target::Joint(99), Target::Parameter("hinge_angle".into())]);
    // Caught: two material edits, two render edits.
    assert_eq!(steel.overlaps(&brass, &base), [Target::Field("materials")]);
    assert_eq!(render.overlaps(&render.clone(), &base), [Target::Field("render")]);
    // No warning: a render or material change beside a part's edit, or beside each other.
    for (a, b) in [(&render, &lengthen), (&steel, &lengthen), (&render, &steel), (&steel, &angle)] {
        assert!(a.overlaps(b, &base).is_empty() && b.overlaps(a, &base).is_empty(), "{a:?} {b:?}");
    }
    // Units overlap everything, both ways round, additions too (a box added 10 long is read in
    // them), and nothing when the other side touched nothing.
    let added = touched_by(&|d| {
        add_box(d, "Spacer", "10");
    });
    assert_eq!(hows(&added), [(Target::DESIGN, How::AddedInside)]);
    for other in [&render, &steel, &lengthen, &angle, &added] {
        assert_eq!(units.overlaps(other, &base), [Target::UNITS]);
        assert_eq!(other.overlaps(&units, &base), other.targets.keys().cloned().collect::<Vec<_>>());
    }
    assert!(units.overlaps(&Touched::default(), &base).is_empty());
}

/// What is added touches what it is added inside: a feature its component, a component its
/// parent, a parameter the design.
#[test]
fn additions_touch_their_parent() {
    let (base, frame, hinge, _) = nested();
    let mut head = base.clone();
    let bolt = add_box(&mut head, "Bolt", "4");
    head.feature_mut(bolt).unwrap().component = hinge;
    head.set_param("bolt_d", "4 mm", Some("mm"), None).unwrap();
    head.add_component(Some("Washer"), frame).unwrap();
    let diff = diff(&base, &head);
    assert_eq!(
        hows(&Touched::of(&diff, &base)),
        [(Target::Component(0), How::AddedInside), (Target::Component(frame), How::AddedInside), (Target::Component(hinge), How::AddedInside)]
    );
    // A component renamed as well as added to is edited, with its names.
    head.components.iter_mut().filter(|c| c.id == hinge).for_each(|c| c.name = "Pivot".into());
    let touched = Touched::of(&super::diff(&base, &head), &base);
    let hinge = touched.targets.get(&Target::Component(hinge)).cloned();
    assert_eq!(hinge, Some(Touch { names: ("Hinge".into(), "Pivot".into()), how: How::Edited }));
}

/// Hostile designs: a component tree with a loop, repeated ids, a counter at zero. Nothing
/// panics, and each repeated id pairs once.
#[test]
fn damaged_designs_never_panic() {
    let (mut d, frame, hinge, pin) = nested();
    d.components.iter_mut().filter(|c| c.id == frame).for_each(|c| c.parent = hinge);
    let dup = d.features[0].clone();
    d.features.push(dup);
    d.occurrences.push(crate::Occurrence { id: 50, component: 777, parent: 0, name: "Ghost".into(), transform: IDENTITY, grounded: false });
    let mut head = d.clone();
    head.next_id = 0;
    head.features.reverse();
    head.occurrences.clear();
    let diff = diff(&d, &head);
    let _ = Touched::of(&diff, &d).overlaps(&touched(&[Target::Component(frame), Target::Component(777)]), &d);
    let _ = touched(&[Target::Feature(pin), Target::Component(hinge)]).overlaps(&touched(&[Target::Component(frame)]), &d);
    let diff = super::diff(&head, &d);
    let added = diff.features.iter().filter(|c| c.kinds.contains(&ChangeKind::Added)).count();
    assert_eq!(added, 1, "the counter is floored at the ids in use: only the repeated id is new");
}

/// A changeset comes from someone else, so whatever its design file says, the compare, what it
/// touched and the overlap test answer in both directions: never a panic, within the testkit's
/// time budget. The base has a sketch, a parameter, nested components, a placement and a joint,
/// so the mutations reach every part of the compare.
#[test]
fn changesets_from_anyone_never_panic() {
    let mut base = assembly_on(with_sketch().0);
    let frame = base.add_component(Some("Frame"), 0).unwrap();
    let hinge = base.add_component(Some("Hinge"), frame).unwrap();
    let pin = add_box(&mut base, "Pin", "3");
    base.feature_mut(pin).unwrap().component = hinge;
    // The design's own fields too, so mutations reach them.
    base.materials.insert("Body1".into(), "Steel".into());
    base.param_comments.insert("hinge_angle".into(), "the open hinge".into());
    base.metadata.0.insert("project".into(), "Gate".into());
    let json = base.to_json();
    // Compare designs as the reader leaves them, as a changeset's are.
    let base = crate::format::read(&json).unwrap();
    let everything = touched(&[
        Target::DESIGN,
        Target::Component(frame),
        Target::Component(hinge),
        Target::Feature(pin),
        Target::Joint(99),
        Target::Parameter("hinge_angle".into()),
        Target::Field("materials"),
    ]);
    let units = touched(&[Target::UNITS]);
    let cases = solidcraft_testkit::mutations(json.as_bytes(), true, 0xd1ff);
    let (mut changed, mut fields) = (0, 0);
    solidcraft_testkit::survives("design compare", &cases, usize::MAX, |bytes| {
        let head = crate::format::read(std::str::from_utf8(bytes).ok()?).ok()?;
        let (forward, back) = (diff(&base, &head), diff(&head, &base));
        let (theirs, ours) = (Touched::of(&forward, &base), Touched::of(&back, &head));
        changed += usize::from(!theirs.targets.is_empty());
        fields += usize::from(theirs.targets.keys().any(|t| matches!(t, Target::Field(_))));
        // Units overlap everything touched, both ways round, whatever the changeset.
        assert_eq!(theirs.overlaps(&units, &base), theirs.targets.keys().cloned().collect::<Vec<_>>());
        assert_eq!(units.overlaps(&theirs, &base).is_empty(), theirs.targets.is_empty());
        let overlaps = theirs.overlaps(&everything, &base).len() + everything.overlaps(&theirs, &head).len() + ours.overlaps(&theirs, &head).len();
        Some(forward.features.len() + back.components.len() + overlaps)
    });
    // 129 and 28 with this seed: the fuzz must keep reaching the compare and the design's own
    // fields, not stop at the reader.
    assert!(changed >= 50, "only {changed} mutated designs changed anything");
    assert!(fields >= 10, "only {fields} mutated designs changed the design's own fields");
}
