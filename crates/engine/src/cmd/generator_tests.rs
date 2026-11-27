//! The generator commands: an ordering code inserts a parametric part through the normal command
//! path, the part keeps its parameters and rebuilds within the design's declared limits, and what
//! cannot be built changes nothing.

use serde_json::json;

use crate::Session;
use crate::generator::tests::{nut_package, run};

fn body_count(s: &mut Session) -> usize {
    run(s, "inspect.measure", json!({"names_only": true}))["body_count"].as_u64().unwrap() as usize
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

fn path(dir: &tempfile::TempDir) -> String {
    dir.path().to_string_lossy().into_owned()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

#[test]
fn a_code_inserts_a_part_with_the_figures_its_row_and_length_give() {
    let dir = nut_package(None);
    let mut s = Session::default();
    let r = run(&mut s, "generator.insert", json!({"generator": path(&dir), "code": "tn m4 40"}));
    assert_eq!(r["code"], "TN-M4-40");
    assert_eq!(r["configuration"], "M4");
    assert_eq!(r["parameters"], json!({"length": "40 mm"}));
    assert_eq!(r["provenance"]["status"], "verified");
    // 7 x 3.2 x 40 plate, and 2 pegs of 8 mm3 (the count follows the length).
    assert!(close(r["volume_mm3"].as_f64().unwrap(), 912.0), "{r}");
    let size: Vec<f64> = r["bbox_mm"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert!(close(size[0], 40.0 + 0.0) || size[0] >= 40.0, "{size:?}");
    // One undo step takes the whole part away.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(body_count(&mut s), 0);
}

#[test]
fn the_inserted_part_stays_parametric_and_stays_within_its_limits() {
    let dir = nut_package(None);
    let mut s = Session::default();
    let r = run(&mut s, "generator.insert", json!({"generator": path(&dir), "code": "TN-M4-40"}));
    let component = r["component"].as_u64().unwrap();
    assert!(close(volume(&mut s), 912.0));
    // A longer part: the plate grows and the row of pegs gets a third peg.
    run(&mut s, "component.set_link_parameters", json!({"component": component, "parameters": {"length": "60 mm"}}));
    assert!(close(volume(&mut s), 7.0 * 3.2 * 60.0 + 3.0 * 8.0), "{}", volume(&mut s));
    // Out of the declared range: refused, naming the rule, and nothing changes.
    let e = s.execute("component.set_link_parameters", &json!({"component": component, "parameters": {"length": "120 mm"}})).unwrap_err().to_string();
    assert!(e.contains("Length range"), "{e}");
    assert!(close(volume(&mut s), 7.0 * 3.2 * 60.0 + 3.0 * 8.0));
    // A name the design does not have is refused too.
    assert!(s.execute("component.set_link_parameters", &json!({"component": component, "parameters": {"nope": "1 mm"}})).is_err());
    // Undo goes back to 40 mm; clearing the parameters goes back to the row's own length.
    run(&mut s, "edit.undo", json!({}));
    assert!(close(volume(&mut s), 912.0));
    run(&mut s, "component.set_link_parameters", json!({"component": component, "parameters": null}));
    assert!(close(volume(&mut s), 912.0));
}

#[test]
fn parameters_are_kept_by_the_link_and_saved_with_the_design() {
    let dir = nut_package(None);
    let out = tempfile::tempdir().unwrap();
    let file = out.path().join("assembly.sc");
    let mut s = Session::default();
    run(&mut s, "generator.insert", json!({"generator": path(&dir), "parameters": {"size": "M6", "length": 80}}));
    let want = 10.0 * 5.0 * 80.0 + 4.0 * 8.0;
    assert!(close(volume(&mut s), want), "{}", volume(&mut s));
    run(&mut s, "file.save", json!({"path": file}));
    let mut again = Session::default();
    run(&mut again, "file.open", json!({"path": file}));
    assert!(close(volume(&mut again), want), "{}", volume(&mut again));
    let component = run(&mut again, "component.links", json!({}))["links"][0]["component"].as_u64().unwrap();
    // Update Links builds it again with the saved values.
    run(&mut again, "component.update_links", json!({"component": component}));
    assert!(close(volume(&mut again), want));
}

#[test]
fn parse_code_and_template_answer_without_changing_the_design() {
    let dir = nut_package(None);
    let mut s = Session::default();
    let r = run(&mut s, "generator.parse", json!({"generator": path(&dir), "code": "TN-M5-20-S"}));
    assert_eq!(r["ok"], true);
    assert_eq!(r["fields"], json!({"size": "M5", "length": 20, "grade": "steel"}));
    assert_eq!(r["configuration"], "M5");
    assert_eq!(r["warnings"].as_array().unwrap().len(), 1);
    let refused = run(&mut s, "generator.parse", json!({"generator": path(&dir), "code": "TN-M8-40"}));
    assert_eq!((refused["ok"].clone(), refused["reason"].clone()), (json!(false), json!("not_modelled")));
    assert!(refused["message"].as_str().unwrap().contains("not modelled"));
    let c = run(&mut s, "generator.code", json!({"generator": path(&dir), "parameters": r["fields"]}));
    assert_eq!(c["code"], "TN-M5-20-S");
    assert!(s.execute("generator.code", &json!({"generator": path(&dir), "parameters": {"size": "M9", "length": 20}})).is_err());
    let t = run(&mut s, "generator.template", json!({"template": "MGN{size: 7|9|12|15}R{length: int 40..2000}", "code": "mgn15r600"}));
    assert_eq!(t["parameters"], json!({"size": "15", "length": 600}));
    assert_eq!(t["prefixes"], json!(["MGN7", "MGN9", "MGN12", "MGN15"]));
    assert_eq!(body_count(&mut s), 0);
}

#[test]
fn a_refused_code_is_an_error_that_changes_nothing() {
    let dir = nut_package(None);
    let mut s = Session::default();
    for (code, words) in [("TN-M8-40", "not modelled"), ("TN-M4-45", "ordering code"), ("", "ordering code")] {
        let e = s.execute("generator.insert", &json!({"generator": path(&dir), "code": code})).unwrap_err().to_string();
        assert!(e.contains(words), "{code}: {e}");
    }
    assert!(s.execute("generator.insert", &json!({"generator": path(&dir)})).is_err());
    assert!(s.execute("generator.insert", &json!({"generator": path(&dir), "code": "TN-M4-40", "parameters": {}})).is_err());
    assert!(s.execute("generator.insert", &json!({"generator": path(&dir), "code": "TN-M4-40", "placement": {"colour": 1}})).is_err());
    assert!(s.execute("generator.insert", &json!({"generator": "no-such-generator", "code": "TN-M4-40"})).is_err());
    assert_eq!(body_count(&mut s), 0);
}

#[test]
fn the_part_is_placed_and_named_from_the_code() {
    let dir = nut_package(None);
    let mut s = Session::default();
    let r = run(&mut s, "generator.insert", json!({"generator": path(&dir), "code": "TN-M6-30", "placement": {"translate": [100, 0, 0]}}));
    assert_eq!(r["description"], "T-nut M6 x 30 mm, zinc");
    let links = run(&mut s, "component.links", json!({}));
    assert_eq!(links["links"].as_array().unwrap().len(), 1);
    let comps = run(&mut s, "component.list", json!({}));
    assert!(comps.to_string().contains("TN-M6-30"), "{comps}");
}

#[test]
fn check_runs_the_declared_examples_and_reports_a_wrong_figure() {
    let dir = nut_package(None);
    let mut s = Session::default();
    let r = run(&mut s, "generator.check", json!({"generator": path(&dir)}));
    assert_eq!(r["passed"], true, "{r}");
    assert_eq!(r["results"].as_array().unwrap().len(), 2);
    let wrong = std::fs::read_to_string(dir.path().join("generator.toml")).unwrap().replace("volume_mm3 = 912.0", "volume_mm3 = 900.0");
    std::fs::write(dir.path().join("generator.toml"), wrong).unwrap();
    let r = run(&mut s, "generator.check", json!({"generator": path(&dir)}));
    assert_eq!(r["passed"], false);
    assert!(r["results"][0]["problems"][0].as_str().unwrap().contains("volume"), "{r}");
}

#[test]
fn list_answers_with_the_installed_generators_and_what_it_skipped() {
    let mut s = Session::default();
    let r = run(&mut s, "generator.list", json!({}));
    assert!(r["generators"].is_array() && r["skipped"].is_array(), "{r}");
}
