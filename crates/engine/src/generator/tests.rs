//! Generator packages: loading, reading codes into a row and parameters, refusals, and hostile
//! package files. The fixture is a small metric T-nut: a parametric design with a four-row
//! configuration table, a free `length` with a declared range, and a row of pegs whose count
//! follows the length.

use std::path::Path;

use serde_json::{Value, json};

use super::{GeneratorError, Package, RefusalKind};
use crate::Session;

const MANIFEST: &str = r#"
format = "solidcraft-generator/2"
id = "metric-t-nut"
name = "Metric T-nut"
description = "A fixture generator"
license = "CC0-1.0"
model = "tnut.sc"
row_from = ["size"]

[parameters]
size = { type = "choice", values = ["M4", "M5", "M6", "M8"] }
length = { type = "int", min = 20, max = 100, step = 10, unit = "mm" }
grade = { type = "map", codes = { A = "zinc", S = "steel" }, default = "zinc" }

[[template]]
pattern = "TN-{size}-{length}[-{grade}]"
description = "T-nut {size} x {length} mm, {grade}"

[rows.M4]
source = "fixture table"
page = 1
status = "verified"

[rows.M5]
source = "fixture table"
page = 2
status = "needs_review"

[rows.M8]
source = "fixture table"
page = 3
status = "not_modelled"
note = "M8 T-nuts are not modelled yet"

[[example]]
code = "TN-M4-40"
volume_mm3 = 912.0
bbox_mm = [40.0, 7.0, 3.2]

[[example]]
code = "TN-M8-40"
invalid = true
message_contains = "not modelled"
"#;

pub(crate) fn run(s: &mut Session, id: &str, params: Value) -> Value {
    match s.execute(id, &params) {
        Ok(v) => v,
        Err(e) => panic!("{id} {params}: {e}"),
    }
}

/// The T-nut design: parameters `w`, `h` (by row) and `length` (free, 20 to 100 mm), a plate and
/// a row of 8 mm3 pegs, one per 20 mm of length.
fn write_model(path: &Path) {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "7 mm"}));
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "3.2 mm"}));
    run(&mut s, "parameters.add", json!({"name": "length", "expression": "40 mm"}));
    run(&mut s, "solid.box", json!({"length": "length", "width": "w", "height": "h", "body_name": "Nut"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "body_name": "Peg"}));
    run(&mut s, "solid.pattern.rectangular", json!({"bodies": ["Peg"], "dir1": [1, 0, 0], "count1": "floor(length / 20 mm)", "spacing1": 20}));
    run(&mut s, "config.configure", json!({"params": ["w", "h", "length"]}));
    for (row, w, h) in [("M4", "7 mm", "3.2 mm"), ("M5", "8 mm", "4 mm"), ("M6", "10 mm", "5 mm"), ("M8", "13 mm", "6.5 mm")] {
        run(&mut s, "config.row", json!({"name": row, "values": {"param:w": w, "param:h": h, "param:length": "40 mm"}}));
    }
    run(
        &mut s,
        "config.rule",
        json!({"rule": {"id": "length-range", "name": "Length range", "condition": {"type": "always"},
            "effect": {"type": "parameter_range", "parameter": "length", "minimum": "20 mm", "maximum": "100 mm"}}}),
    );
    run(&mut s, "file.save", json!({"path": path}));
}

/// A package folder with the fixture design and `manifest` (the fixture's own when `None`).
pub(crate) fn nut_package(manifest: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write_model(&dir.path().join("tnut.sc"));
    std::fs::write(dir.path().join("generator.toml"), manifest.unwrap_or(MANIFEST)).unwrap();
    dir
}

fn load(dir: &tempfile::TempDir) -> Package {
    Package::load(dir.path()).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn a_code_reads_into_a_row_and_parameters() {
    let dir = nut_package(None);
    let g = load(&dir);
    let r = g.parse("TN-M4-40").unwrap();
    assert_eq!(r.code, "TN-M4-40");
    assert_eq!(r.configuration.as_deref(), Some("M4"));
    assert_eq!(r.parameters.get("length").map(String::as_str), Some("40 mm"));
    assert_eq!(r.description.as_deref(), Some("T-nut M4 x 40 mm, zinc"));
    assert_eq!(r.row.as_ref().map(|x| (x.status.as_str(), x.page.as_deref())), Some(("verified", Some("1"))));
    // Case and separators are ignored; the code prints back in the template's spelling.
    let r = g.parse("tn m6 70 s").unwrap();
    assert_eq!(r.code, "TN-M6-70-S");
    assert_eq!(r.fields["grade"], json!("steel"));
    let r = g.parse("TN-M5-20").unwrap();
    assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    let printed = g.code_for(&r.fields, None).unwrap();
    assert_eq!(printed.1, "TN-M5-20");
}

#[test]
fn refusals_say_why() {
    let dir = nut_package(None);
    let g = load(&dir);
    let long = "9".repeat(200);
    for (code, kind, words) in [
        ("TN-M8-40", RefusalKind::NotModelled, "not modelled yet"),
        ("TN-M4-110", RefusalKind::Invalid, "not a Metric T-nut"),
        ("TN-M4-45", RefusalKind::Invalid, "ordering code"),
        ("XX-M4-40", RefusalKind::Invalid, "ordering code"),
        ("", RefusalKind::Invalid, "ordering code"),
        (long.as_str(), RefusalKind::Limit, "more than 128"),
    ] {
        let e = g.parse(code).unwrap_err();
        assert_eq!(e.kind, kind, "{code}: {e:?}");
        assert!(e.message.contains(words), "{code}: {}", e.message);
    }
}

#[test]
fn a_package_with_a_script_resolves_through_rhai_and_tables() {
    let dir = nut_package(Some(MANIFEST.replace("row_from = [\"size\"]\n", "").as_str()));
    std::fs::create_dir(dir.path().join("tables")).unwrap();
    std::fs::write(dir.path().join("tables/sizes.csv"), "key,row,note\nM4,M4,\nM6,M6,\nM5,,no such row\n").unwrap();
    std::fs::write(
        dir.path().join("generator.rhai"),
        r#"
        fn resolve(f) {
            let t = table("sizes", f.size);
            if t == () { refuse("invalid", "unknown size " + f.size); }
            if t.row == () { refuse("not_modelled", t.note); }
            #{ configuration: t.row, parameters: #{ length: f.length.to_string() + " mm" }, description: "nut " + code_template(f) }
        }
        "#,
    )
    .unwrap();
    let g = load(&dir);
    let r = g.parse("TN-M6-60").unwrap();
    assert_eq!(r.configuration.as_deref(), Some("M6"));
    assert_eq!(r.parameters.get("length").map(String::as_str), Some("60 mm"));
    assert_eq!(r.description.as_deref(), Some("nut TN-M6-60"));
    let e = g.parse("TN-M5-60").unwrap_err();
    assert_eq!((e.kind, e.message.as_str()), (RefusalKind::NotModelled, "no such row"));
    assert_eq!(g.parse("TN-M8-60").unwrap_err().kind, RefusalKind::Invalid);
}

#[test]
fn hostile_packages_are_refused_in_words() {
    let bad_manifests = [
        MANIFEST.replace("license = \"CC0-1.0\"", "licence = \"CC0-1.0\""),
        MANIFEST.replace("model = \"tnut.sc\"", "model = \"../tnut.sc\""),
        MANIFEST.replace("model = \"tnut.sc\"", "model = \"/etc/passwd\""),
        MANIFEST.replace("model = \"tnut.sc\"", "model = \"missing.sc\""),
        MANIFEST.replace("solidcraft-generator/2", "solidcraft-generator/1"),
        MANIFEST.replace("solidcraft-generator/2", "solidcraft-generator/9"),
        MANIFEST.replace("min = 20", "min = 200"),
        MANIFEST.replace("TN-{size}", "TN-{nope}"),
        MANIFEST.replace("row_from = [\"size\"]", "row_from = [\"nope\"]"),
        MANIFEST.replace("status = \"verified\"", "status = \"great\""),
        MANIFEST.replace("id = \"metric-t-nut\"", "id = \"X\""),
        format!("{MANIFEST}\n{}", "x = 1\n".repeat(3)),
        "not toml [[[".to_string(),
        String::new(),
    ];
    for m in &bad_manifests {
        let dir = nut_package(Some(m.as_str()));
        assert!(Package::load(dir.path()).is_err(), "{m}");
    }
    let dir = nut_package(Some(MANIFEST.replace("solidcraft-generator/2", "solidcraft-generator/9").as_str()));
    assert!(matches!(Package::load(dir.path()), Err(GeneratorError::NeedsNewer { .. })));
    // x- keys are notes and are ignored.
    let dir = nut_package(Some(format!("x-note = \"hi\"\n{MANIFEST}").as_str()));
    assert!(Package::load(dir.path()).is_ok());
    // A runaway script cannot be loaded into a hang.
    let dir = nut_package(None);
    std::fs::write(dir.path().join("generator.rhai"), "fn resolve(f) { loop { } }").unwrap();
    let g = load(&dir);
    assert_eq!(g.parse("TN-M4-40").unwrap_err().kind, RefusalKind::Limit);
}

#[cfg(unix)]
#[test]
fn a_model_that_links_out_of_the_folder_is_refused() {
    let dir = nut_package(None);
    let outside = tempfile::tempdir().unwrap();
    std::fs::copy(dir.path().join("tnut.sc"), outside.path().join("other.sc")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("other.sc"), dir.path().join("linked.sc")).unwrap();
    let m = MANIFEST.replace("model = \"tnut.sc\"", "model = \"linked.sc\"");
    std::fs::write(dir.path().join("generator.toml"), m).unwrap();
    assert!(Package::load(dir.path()).is_err());
}

#[test]
fn installed_generators_are_listed_by_folder_and_a_broken_one_is_skipped() {
    let base = tempfile::tempdir().unwrap();
    let dir = nut_package(None);
    std::fs::create_dir(base.path().join("metric-t-nut")).unwrap();
    for f in ["tnut.sc", "generator.toml"] {
        std::fs::copy(dir.path().join(f), base.path().join("metric-t-nut").join(f)).unwrap();
    }
    std::fs::create_dir(base.path().join("broken")).unwrap();
    std::fs::write(base.path().join("broken/generator.toml"), "nope").unwrap();
    let (found, skipped) = super::installed::list_in(base.path());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].package.id, "metric-t-nut");
    assert_eq!(found[0].package.templates[0].template.prefixes(), ["TNM4", "TNM5", "TNM6", "TNM8"].map(String::from).to_vec());
    assert_eq!(skipped.len(), 1, "{skipped:?}");
}

#[test]
fn options_outside_the_code_reach_the_script_and_a_tables_columns_are_the_provenance() {
    let dir = nut_package(None);
    std::fs::create_dir(dir.path().join("tables")).unwrap();
    std::fs::write(
        dir.path().join("tables/sizes.csv"),
        "key,row,source,page,status\n6,M6,Fixture catalogue,7,needs_review\nM6,M6,Fixture catalogue,7,needs_review\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("generator.rhai"),
        r#"
        fn resolve(f) {
            let t = table("sizes", 6);
            let caps = if f.options == () { 0 } else { f.options.caps };
            #{ configuration: t.row, parameters: #{ length: f.length.to_string() + " mm" }, description: "caps " + caps }
        }
        "#,
    )
    .unwrap();
    let g = load(&dir);
    let options = json!({"caps": 2});
    let r = g.parse_with("TN-M6-30", options.as_object()).unwrap();
    assert_eq!(r.description.as_deref(), Some("caps 2"));
    assert_eq!(r.fields["options"], options);
    let row = r.row.unwrap();
    assert_eq!((row.source.as_str(), row.page.as_deref(), row.status.as_str()), ("Fixture catalogue", Some("7"), "needs_review"));
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(g.parse("TN-M6-30").unwrap().description.as_deref(), Some("caps 0"));
    // The letters of a letter map are kept next to its value.
    assert_eq!(g.parse("TN-M6-30-S").unwrap().fields["grade_code"], json!("S"));
}
