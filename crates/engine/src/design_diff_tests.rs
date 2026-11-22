//! `file.diff` on designs built here, each expected result taken from the edit that made it.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use solidcraft_doc::text_params::TextParam;
use solidcraft_doc::{Document, Feature, FeatureKind, Param};

use super::diff;
use crate::Session;
use crate::schema::file::{DiffFeature, DiffFeatureChanged, DiffFeatures, DiffImport, DiffMarker, DiffParameter, DiffParameters, DiffResult, Pair};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A design of `n` boxes, one feature each.
fn boxes(n: usize) -> Document {
    let mut s = Session::default();
    for i in 0..n {
        run(&mut s, "solid.box", json!({"length": 10 + i, "width": 10, "height": 10}));
    }
    (*s.doc).clone()
}

fn named(d: &Document, i: usize) -> DiffFeature {
    super::named(&d.features[i])
}

fn ids(d: &Document) -> Vec<u64> {
    d.features.iter().map(|f| f.id).collect()
}

#[test]
fn identical_designs_compare_the_same() {
    let a = boxes(2);
    assert_eq!(diff(&a, &a.clone()), DiffResult { same: true, ..DiffResult::default() });
    // The format and the id counters are bookkeeping, so a design that differs only there is the same.
    let mut b = a.clone();
    b.format = "solidcraft/1".into();
    b.next_id += 5;
    b.next_component_color_index += 1;
    assert!(diff(&a, &b).same);
}

/// [1, 2, 3, 4] to [2, 3, 4, 1] is one move, of the first feature to the end, not three.
#[test]
fn a_reorder_reports_only_the_feature_that_moved() {
    let a = boxes(4);
    let mut b = a.clone();
    b.features.rotate_left(1);
    let r = diff(&a, &b);
    let moved: Vec<(u64, Option<u64>, Option<u64>)> =
        r.features.moved.iter().map(|m| (m.feature.id, m.after_a.as_ref().map(|f| f.id), m.after_b.as_ref().map(|f| f.id))).collect();
    assert_eq!(moved, vec![(ids(&a)[0], None, Some(ids(&a)[3]))], "{r:?}");
    assert!(r.features.added.is_empty() && r.features.removed.is_empty() && r.features.changed.is_empty() && r.features.marker.is_none(), "{r:?}");
    assert!(!r.same);
}

/// The marker is placed by the feature it follows, so deleting a feature before it is no marker
/// change, and moving it is.
#[test]
fn a_deletion_before_the_marker_is_not_a_marker_change() {
    let mut a = boxes(4);
    a.marker = Some(3);
    let mut b = a.clone();
    b.features.remove(0);
    b.marker = Some(2);
    let r = diff(&a, &b);
    assert_eq!(r.features.removed, vec![named(&a, 0)]);
    assert_eq!(r.features.marker, None);
    assert!(r.features.moved.is_empty(), "{r:?}");
    b.marker = None;
    assert_eq!(diff(&a, &b).features.marker, Some(Pair { a: DiffMarker::After(named(&a, 2)), b: DiffMarker::End }));
}

/// Two branches that hand out the same id to different features give a removal and an addition.
#[test]
fn an_id_reused_for_another_type_of_feature_is_a_removal_and_an_addition() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let both = (*s.doc).clone();
    let a = Document { features: vec![both.features[0].clone()], ..both.clone() };
    let mut sketch = both.features[1].clone();
    sketch.id = both.features[0].id;
    let b = Document { features: vec![sketch], ..both.clone() };
    let r = diff(&a, &b);
    assert_eq!(r.features.reused_ids, vec![a.features[0].id]);
    assert_eq!(r.features.removed, vec![named(&a, 0)]);
    assert_eq!(r.features.added.iter().map(|f| f.feature.clone()).collect::<Vec<_>>(), vec![named(&b, 0)]);
    assert!(r.features.changed.is_empty() && r.sketches.is_empty(), "{r:?}");
}

#[test]
fn renames_suppressions_and_other_settings_are_named() {
    let a = boxes(2);
    let mut b = a.clone();
    b.features[0].name = "Base".into();
    b.features[0].suppressed = true;
    b.features[1].edge_names = vec!["E1".into()];
    let r = diff(&a, &b);
    assert_eq!(
        r.features.changed,
        vec![
            DiffFeatureChanged { feature: named(&b, 0), renamed_from: Some(a.features[0].name.clone()), suppressed: Some(true), keys: Vec::new() },
            DiffFeatureChanged { feature: named(&b, 1), renamed_from: None, suppressed: None, keys: vec!["edge_names".into()] },
        ]
    );
}

/// What the diff has no section for is still named, and view state is kept apart.
#[test]
fn other_sections_and_view_state_are_named() {
    let a = boxes(1);
    let mut b = a.clone();
    b.name = "Another".into();
    b.visibility.hidden_bodies.push("Body1".into());
    let r = diff(&a, &b);
    assert_eq!((r.other, r.view), (vec!["name".to_string()], vec!["visibility".to_string()]));
    assert!(!r.same);
}

/// A changed expression, unit or comment is a change. A value that follows another parameter is not.
#[test]
fn parameters_match_by_name_and_compare_their_expressions() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "width", "expression": "40 mm"}));
    run(&mut s, "parameters.add", json!({"name": "twice", "expression": "width * 2"}));
    let a = (*s.doc).clone();
    let mut b = a.clone();
    b.params.iter_mut().filter(|p| p.name == "width").for_each(|p| p.expr = "50 mm".into());
    b.params.retain(|p| p.name != "twice");
    let r = diff(&a, &b).parameters;
    let changed: Vec<(&str, &str, &str)> = r.changed.iter().map(|c| (c.a.name.as_str(), c.a.expression.as_str(), c.b.expression.as_str())).collect();
    assert_eq!(changed, vec![("width", "40 mm", "50 mm")]);
    assert_eq!(r.removed.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["twice"]);
    assert!(r.added.is_empty());
    let mut c = a.clone();
    c.params.iter_mut().filter(|p| p.name == "width").for_each(|p| p.expr = "50 mm".into());
    assert_eq!(diff(&a, &c).parameters.changed.len(), 1, "`twice` follows `width` and is not listed");
}

fn param(name: &str, expr: &str) -> Param {
    Param { name: name.into(), expr: expr.into(), unit: "mm".into(), comment: String::new(), model: false }
}

fn text(name: &str, text: &str) -> TextParam {
    TextParam { name: name.into(), text: text.into(), comment: String::new() }
}

/// A one-box design read from a file whose parameter sections are `params` and `text_params`.
fn read_with(base: &str, params: Value, text_params: Value) -> Document {
    let mut file: Value = serde_json::from_str(base).unwrap();
    file["params"] = params;
    file["text_params"] = text_params;
    solidcraft_io::read_design(file.to_string().as_bytes()).unwrap()
}

/// Reading keeps every row of a repeated parameter name, and evaluation lets the later one win, so
/// a later row counts as much as the first. The rows of one name match by their place among them.
#[test]
fn every_row_of_a_repeated_parameter_name_counts() {
    let base = boxes(1).to_json();
    let saved =
        |second: &str| read_with(&base, json!([{"name": "x", "expr": "1 mm", "unit": "mm"}, {"name": "x", "expr": second, "unit": "mm"}]), json!([]));
    let (a, b) = (saved("2 mm"), saved("3 mm"));
    assert_eq!(a.params.iter().map(|p| p.expr.as_str()).collect::<Vec<_>>(), ["1 mm", "2 mm"], "reading keeps both rows");
    let row = |expr: &str, row: usize| DiffParameter {
        name: "x".into(),
        source: "user".into(),
        expression: expr.into(),
        unit: "mm".into(),
        row: Some(row),
        ..DiffParameter::default()
    };
    let r = diff(&a, &b);
    assert_eq!(r.parameters, DiffParameters { changed: vec![Pair { a: row("2 mm", 2), b: row("3 mm", 2) }], ..DiffParameters::default() });
    assert!(r.other.is_empty() && !r.same, "{r:?}");
    // A third row is added as row 3 and leaves the other two alone, and taking it away removes it.
    let mut c = b.clone();
    c.params.push(param("x", "4 mm"));
    assert_eq!(diff(&b, &c).parameters, DiffParameters { added: vec![row("4 mm", 3)], ..DiffParameters::default() });
    assert_eq!(diff(&c, &b).parameters, DiffParameters { removed: vec![row("4 mm", 3)], ..DiffParameters::default() });
    assert!(!diff(&c, &b).same);
}

/// A text parameter is compared on its own, beside a number parameter of the same name (the text
/// is the one sketch texts read), and each row of a repeated text name counts.
#[test]
fn a_text_parameter_counts_beside_a_number_parameter_of_its_name() {
    let base = boxes(1).to_json();
    let number = json!([{"name": "t", "expr": "5 mm", "unit": "mm"}]);
    let saved = |texts: &[&str]| read_with(&base, number.clone(), texts.iter().map(|t| json!({"name": "t", "text": t})).collect());
    let (a, b) = (saved(&["AAA"]), saved(&["BBB"]));
    assert_eq!((a.params.len(), a.text_params.len()), (1, 1), "reading keeps both");
    let shown = |text: &str, row: Option<usize>| DiffParameter {
        name: "t".into(),
        source: "text".into(),
        expression: text.into(),
        unit: "text".into(),
        row,
        ..DiffParameter::default()
    };
    let r = diff(&a, &b);
    assert_eq!(r.parameters, DiffParameters { changed: vec![Pair { a: shown("AAA", None), b: shown("BBB", None) }], ..DiffParameters::default() });
    assert!(r.other.is_empty() && !r.same, "{r:?}");
    let r = diff(&saved(&["AAA", "CCC"]), &saved(&["AAA", "DDD"]));
    assert_eq!(r.parameters.changed, vec![Pair { a: shown("CCC", Some(2)), b: shown("DDD", Some(2)) }], "{r:?}");
    assert!(!r.same);
}

/// The `model` flag (a row a dimension or feature made) is part of the row, since pruning unused
/// model parameters reads it. It shows as the source.
#[test]
fn a_flipped_model_flag_is_a_change() {
    let mut a = boxes(1);
    a.params.push(param("d1", "5 mm"));
    let mut b = a.clone();
    b.params.iter_mut().filter(|p| p.name == "d1").for_each(|p| p.model = true);
    let source = |source: &str| DiffParameter {
        name: "d1".into(),
        source: source.into(),
        expression: "5 mm".into(),
        unit: "mm".into(),
        ..DiffParameter::default()
    };
    let r = diff(&a, &b);
    assert_eq!(r.parameters, DiffParameters { changed: vec![Pair { a: source("user"), b: source("sketch") }], ..DiffParameters::default() });
    assert!(r.other.is_empty() && !r.same, "{r:?}");
}

/// Rows that only change order show in no entry, so their section is named under `other`. Two
/// rows of one name that swap places are two changes, since evaluation takes the later one.
#[test]
fn reordered_parameter_rows_name_their_section() {
    let mut a = boxes(1);
    a.params = vec![param("w", "1 mm"), param("h", "2 mm")];
    a.text_params = vec![text("p", "P"), text("q", "Q")];
    let mut b = a.clone();
    b.params.reverse();
    let r = diff(&a, &b);
    assert_eq!((&r.parameters, &r.other), (&DiffParameters::default(), &vec!["params".to_string()]), "{r:?}");
    assert!(!r.same);
    b.text_params.reverse();
    assert_eq!(diff(&a, &b).other, ["params", "text_params"]);
    let mut c = a.clone();
    c.params = vec![param("x", "1 mm"), param("x", "2 mm")];
    let mut d = c.clone();
    d.params.reverse();
    let r = diff(&c, &d);
    let rows: Vec<(Option<usize>, &str, &str)> =
        r.parameters.changed.iter().map(|p| (p.a.row, p.a.expression.as_str(), p.b.expression.as_str())).collect();
    assert_eq!(rows, [(Some(1), "1 mm", "2 mm"), (Some(2), "2 mm", "1 mm")]);
    assert!(r.other.is_empty() && !r.same, "the entries show it: {r:?}");
}

/// The comment of a feature input is compared by name and named with its feature's id and its
/// input, when one input has that name. The input's expression is a key of its feature.
#[test]
fn a_feature_inputs_comment_is_named_with_its_input() {
    let mut a = boxes(1);
    let label = a.features[0].kind.inputs()[0].0;
    a.features[0].param_names = vec!["len".into()];
    let mut b = a.clone();
    b.param_comments.insert("len".into(), "the long side".into());
    let comment = |text: &str, feature: Option<u64>| DiffParameter {
        name: "len".into(),
        source: "feature".into(),
        feature,
        input: feature.map(|_| label.to_owned()),
        comment: text.into(),
        ..DiffParameter::default()
    };
    let id = a.features[0].id;
    let r = diff(&a, &b);
    assert_eq!(r.parameters, DiffParameters { added: vec![comment("the long side", Some(id))], ..DiffParameters::default() });
    assert!(r.other.is_empty() && !r.same, "{r:?}");
    // Two inputs of that name name no feature.
    let mut c = b.clone();
    c.features[0].param_names = vec!["len".into(), "len".into()];
    c.param_comments.insert("len".into(), "either side".into());
    let r = diff(&b, &c);
    assert_eq!(r.parameters.changed, vec![Pair { a: comment("the long side", Some(id)), b: comment("either side", None) }], "{r:?}");
}

/// One feature may have any number of inputs (a Fillet reads each of its `edge_radii` as one), and
/// nothing limits the length of its name. Each input's comment entry names the feature by id, so a
/// long name with many commented inputs gives a reply about the size of the files. Copying the
/// name into each entry made the reply grow with the name's length times the inputs (40 GB for a
/// 1 MB name and 40,000 radii, and 200 MB here).
#[test]
fn a_long_feature_name_is_not_copied_into_each_inputs_comment() {
    let (n, id) = (2_000, 1_000);
    let name = "F".repeat(50_000);
    let fillet = json!({
        "id": id,
        "name": name,
        "type": "fillet",
        "edges": (0..n).map(|i| [10.0, 0.0, f64::from(i) / f64::from(n)]).collect::<Vec<_>>(),
        "radius": "1 mm",
        "edge_radii": (0..n).map(|i| format!("{} mm", 1 + i % 3)).collect::<Vec<_>>(),
        "param_names": (0..=n).map(|i| format!("r{i}")).collect::<Vec<_>>(),
    });
    let mut base = boxes(1);
    base.features.push(serde_json::from_value(fillet).unwrap());
    // The two files differ in the comment of every edge radius, `r1` to `r2000` (`r0` is `radius`).
    let file = |comment: &str| {
        let mut d = base.clone();
        d.param_comments = (1..=n).map(|i| (format!("r{i}"), comment.to_owned())).collect();
        d.to_json()
    };
    let files = [file("before"), file("after")];
    let [a, b] = files.each_ref().map(|f| solidcraft_io::read_design(f.as_bytes()).unwrap());
    assert_eq!((a.features[1].name.len(), a.param_comments.len()), (name.len(), 2_000), "reading keeps the name and every comment");
    let r = diff(&a, &b);
    assert_eq!(r.parameters.changed.len(), 2_000, "{:?}", r.parameters.changed.first());
    let by_id = |p: &DiffParameter| p.feature == Some(id) && p.input.as_deref() == Some("Edge radius");
    assert!(r.parameters.changed.iter().all(|c| by_id(&c.a) && by_id(&c.b)), "{:?}", r.parameters.changed.first());
    assert!(r.features == DiffFeatures::default() && r.other.is_empty() && !r.same, "{:?}", r.other);
    let reply = serde_json::to_string(&r).unwrap();
    let size: usize = files.iter().map(String::len).sum();
    assert!(!reply.contains(&name), "the feature's name is in the reply");
    assert!(reply.len() < 2 * size, "a reply of {} bytes for files of {size} bytes", reply.len());
}

/// Many rows of one name, many text rows and many feature inputs that read the name compare in
/// one pass over each section. Listing the rows as the Parameters window does clones every reader
/// of `x` once per row of `x` (300 million names or more here), and looks through every feature
/// once per text parameter.
#[test]
fn many_repeated_names_and_inputs_compare_in_one_pass() {
    let mut a = boxes(1);
    let one = a.features[0].clone();
    let inputs = one.kind.inputs().len();
    assert!(inputs >= 3, "a box has {inputs} inputs");
    a.features = (0..10_000u64)
        .map(|i| {
            let mut f = Feature { id: i + 1, name: format!("Box{i}"), ..one.clone() };
            f.param_names = (0..inputs).map(|k| format!("i{i}_{k}")).collect();
            for (_, e, _) in f.kind.inputs_mut() {
                *e = "x".into();
            }
            f
        })
        .collect();
    a.params = (0..10_000).map(|_| param("x", "1 mm")).collect();
    a.text_params = (0..100_000).map(|_| text("t", "T")).collect();
    a.param_comments = (0..10_000).map(|i| (format!("i{i}_0"), "c".to_owned())).collect();
    let mut b = a.clone();
    b.params.last_mut().unwrap().expr = "2 mm".into();
    b.text_params.last_mut().unwrap().text = "U".into();
    b.param_comments.insert("i9999_0".into(), "d".into());
    let r = diff(&a, &b);
    let changed: Vec<(&str, Option<usize>, &str, Option<u64>)> =
        r.parameters.changed.iter().map(|c| (c.b.name.as_str(), c.b.row, c.b.expression.as_str(), c.b.feature)).collect();
    // Box9999 has the id 10,000.
    assert_eq!(changed, [("x", Some(10_000), "2 mm", None), ("t", Some(100_000), "U", None), ("i9999_0", None, "", Some(10_000))]);
    assert!(r.parameters.added.is_empty() && r.parameters.removed.is_empty() && r.features.changed.is_empty() && r.other.is_empty(), "{:?}", r.other);
}

/// A sketch is named with what changed in it and its entity counts. A point that moved well under
/// the evaluation's quantum has not moved.
#[test]
fn a_changed_sketch_is_named_with_its_counts() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tests/fixtures/revolve-360-cut.sc");
    let a = solidcraft_io::read_design(&std::fs::read(path).unwrap()).unwrap();
    let i = a.features.iter().position(|f| matches!(f.kind, FeatureKind::Sketch { .. })).unwrap();
    let FeatureKind::Sketch { sketch, .. } = &a.features[i].kind else { panic!("feature {i} is a sketch") };
    let counts = (sketch.points.len(), sketch.curves.len(), sketch.constraints.len());
    let mut b = a.clone();
    let FeatureKind::Sketch { sketch, .. } = &mut b.features[i].kind else { panic!("feature {i} is a sketch") };
    sketch.points[1].pos.y += 1e-9;
    assert!(diff(&a, &b).same);
    let FeatureKind::Sketch { sketch, .. } = &mut b.features[i].kind else { panic!("feature {i} is a sketch") };
    sketch.points[1].pos.x += 1.0;
    sketch.constraints.pop();
    let r = diff(&a, &b);
    assert_eq!(r.sketches.len(), 1, "{r:?}");
    let s = &r.sketches[0];
    assert_eq!((&s.feature, &s.keys), (&named(&b, i), &vec!["constraints".to_string(), "points".to_string()]));
    let pair = |a: usize, b: usize| Pair { a, b };
    assert_eq!(
        (s.points.clone(), s.curves.clone(), s.constraints.clone()),
        (pair(counts.0, counts.0), pair(counts.1, counts.1), pair(counts.2, counts.2 - 1))
    );
    assert!(r.features.changed.is_empty(), "the sketch is reported once, under sketches: {r:?}");
}

/// An imported STEP file is named by the SHA-256 of the text the design keeps, which for a UTF-8
/// file is the file's own.
#[test]
fn an_imported_step_file_is_named_by_its_sha256() {
    let dir = std::env::temp_dir().join(format!("solidcraft-design-diff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let step = dir.join("part.step").to_string_lossy().to_string();
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "file.export_step", json!({"path": step}));
    let mut s = Session::default();
    let a = (*s.doc).clone();
    run(&mut s, "file.insert_step", json!({"path": step}));
    let b = (*s.doc).clone();
    let bytes = std::fs::read(&step).unwrap();
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let r = diff(&a, &b);
    assert_eq!(r.imports.len(), 1, "{r:?}");
    assert!(r.imports[0].a.is_empty());
    assert_eq!(r.imports[0].file, "part.step");
    assert_eq!(
        (r.imports[0].b[0].kind.as_str(), r.imports[0].b[0].sha256.as_str(), r.imports[0].b[0].bytes),
        ("step", digest.as_str(), bytes.len() as u64)
    );
    // An edited STEP text gives the digest of the new text.
    let mut c = b.clone();
    for f in &mut c.features {
        if let FeatureKind::Import { step, .. } = &mut f.kind {
            *step = format!("{step}\n").into();
        }
    }
    let r = diff(&b, &c);
    let edited = format!("{:x}", Sha256::digest([bytes.as_slice(), b"\n"].concat()));
    assert_eq!(r.imports.iter().map(|i| (i.a[0].sha256.clone(), i.b[0].sha256.clone())).collect::<Vec<_>>(), vec![(digest, edited)]);
    assert!(r.features.changed.is_empty(), "the file is reported once, under imports: {r:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Imports match as features do, by id and type. An import whose id names a box on the other side
/// is removed under its own name and file, and an Import and a mesh import under one id are a
/// removal and an addition, not a change.
#[test]
fn an_import_whose_id_names_another_type_of_feature_is_a_removal_and_an_addition() {
    let dir = std::env::temp_dir().join(format!("solidcraft-design-diff-ids-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (step, stl) = (dir.join("part.step").to_string_lossy().to_string(), dir.join("part.stl").to_string_lossy().to_string());
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "file.export_step", json!({"path": step}));
    run(&mut s, "file.export_stl", json!({"path": stl}));
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 10}));
    run(&mut s, "file.insert_step", json!({"path": step}));
    run(&mut s, "file.insert_mesh", json!({"path": stl}));
    let all = (*s.doc).clone();
    let of = |key: &str| all.features.iter().find(|f| f.kind.kind_key() == key).cloned().unwrap_or_else(|| panic!("a {key} feature"));
    let (import, mesh) = (of("import"), of("mesh_import"));
    let boxed = Feature { id: import.id, ..all.features[0].clone() };
    let mesh = Feature { id: import.id, ..mesh };
    assert_ne!(boxed.kind.kind_key(), "import");
    let stored = |f: &Feature| super::payloads(f).unwrap_or_else(|| panic!("{} imports a file", f.name));
    let removed = DiffImport { feature: super::named(&import), file: "part.step".into(), a: stored(&import), b: Vec::new() };
    assert_eq!(removed.a.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(), ["step"]);
    let a = Document { features: vec![import.clone()], ..all.clone() };
    let r = diff(&a, &Document { features: vec![boxed], ..all.clone() });
    assert_eq!(r.imports, vec![removed.clone()]);
    let r = diff(&a, &Document { features: vec![mesh.clone()], ..all.clone() });
    let added = DiffImport { feature: super::named(&mesh), file: "part.stl".into(), a: Vec::new(), b: stored(&mesh) };
    assert_eq!(added.b.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(), ["mesh"]);
    assert_eq!(r.imports, vec![added, removed]);
    assert_eq!((r.features.reused_ids, r.features.changed), (vec![import.id], Vec::new()), "the features agree");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a newer SolidCraft saved that this build kept aside is compared too, without the format
/// it names. Two files that differ only there are not the same.
#[test]
fn what_a_newer_version_saved_and_this_build_kept_aside_is_compared() {
    use solidcraft_doc::format::{FORMAT, format_string};
    let a = Document::from_json(&boxes(1).to_json()).unwrap();
    assert!(a.newer_file.is_none());
    let saved_by = |format: u32, kept: Value| {
        let mut file: Value = serde_json::from_str(&a.to_json()).unwrap();
        file["format"] = json!(format_string(format));
        file["lattice"] = kept;
        let doc = Document::from_json(&file.to_string()).unwrap();
        assert!(doc.newer_file.as_ref().is_some_and(|f| f.not_understood.iter().any(|n| n.contains("lattice"))), "kept aside");
        doc
    };
    let (b, c) = (saved_by(FORMAT + 1, json!({"cell": 2.5})), saved_by(FORMAT + 1, json!({"cell": 3.0})));
    assert_eq!(diff(&b, &c).other, vec!["newer_file".to_string()]);
    assert!(!diff(&b, &c).same);
    // The same thing kept aside by two newer formats is the same design.
    assert!(diff(&b, &saved_by(FORMAT + 2, json!({"cell": 2.5}))).same);
    // A newer format that keeps nothing aside is the same as this build's.
    let mut file: Value = serde_json::from_str(&a.to_json()).unwrap();
    file["format"] = json!(format_string(FORMAT + 1));
    let d = Document::from_json(&file.to_string()).unwrap();
    assert!(d.newer_file.is_some());
    assert!(diff(&a, &d).same, "{:?}", diff(&a, &d));
    // And one that keeps something aside is not.
    assert_eq!(diff(&a, &b).other, vec!["newer_file".to_string()]);
}

/// A newer SolidCraft keeps its signature chain under `provenance/`, and each signed save adds to
/// it with no edit, so the chain is no change. Nor is the container format its note names, or the
/// order of its parts. Any other part it keeps is compared, and so is the rest of its note.
#[test]
fn a_newer_versions_signatures_and_container_format_are_no_change() {
    use solidcraft_doc::format::{FORMAT, format_string};
    use std::sync::Arc;
    let mut file: Value = serde_json::from_str(&boxes(1).to_json()).unwrap();
    file["format"] = json!(format_string(FORMAT + 1));
    file["lattice"] = json!({"cell": 2.5});
    let read = Document::from_json(&file.to_string()).unwrap();
    let kept = |parts: &[(&str, &str)], container: Option<Value>| {
        let mut newer = read.newer_file.as_deref().cloned().unwrap();
        newer.parts = parts.iter().map(|&(name, bytes)| (name.to_owned(), Arc::from(bytes.as_bytes()))).collect();
        newer.container = container;
        Document { newer_file: Some(Arc::new(newer)), ..read.clone() }
    };
    let signed_once = kept(&[("lattice/cells.bin", "cells"), ("provenance/log.cbor", "a record")], Some(json!({"format": "solidcraft-container/8"})));
    let signed_twice =
        kept(&[("provenance/log.cbor", "a record, then another"), ("lattice/cells.bin", "cells")], Some(json!({"format": "solidcraft-container/9"})));
    assert!(diff(&signed_once, &signed_twice).same, "{:?}", diff(&signed_once, &signed_twice));
    // A note that names only the container's format is the same as none, and so is no chain.
    let unsigned = kept(&[("lattice/cells.bin", "cells")], None);
    assert!(diff(&signed_once, &unsigned).same, "{:?}", diff(&signed_once, &unsigned));
    // Another part that differs is a change, and so is a note that says more than the format.
    let other_cells = kept(&[("lattice/cells.bin", "other cells")], None);
    assert_eq!(diff(&unsigned, &other_cells).other, vec!["newer_file".to_string()]);
    let noted = kept(&[("lattice/cells.bin", "cells")], Some(json!({"format": "solidcraft-container/9", "units": "inch"})));
    assert_eq!(diff(&unsigned, &noted).other, vec!["newer_file".to_string()]);
}
