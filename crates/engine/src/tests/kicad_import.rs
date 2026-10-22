//! A KiCad board opens through `doc.open`: the substrate as a solid, the copper and silkscreen
//! chosen by the options, the board's own thickness unless overridden, and every option checked
//! before the board is read.

use serde_json::json;

use super::{run, volume};
use crate::*;

fn board_file(tag: &str, thickness: f64) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-kicad-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.kicad_pcb");
    std::fs::write(
        &path,
        format!(
            r#"(kicad_pcb (general (thickness {thickness}))
  (gr_rect (start 0 0) (end 20 10) (layer "Edge.Cuts"))
  (segment (start 1 2) (end 10 2) (width 0.3) (layer "F.Cu") (net 1)))"#
        ),
    )
    .unwrap();
    path
}

#[test]
fn a_board_opens_as_a_solid_substrate() {
    let path = board_file("solid", 2.0);
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["board_solid"], json!(true), "{r}");
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert!(names.iter().any(|name| name == "board"), "{names:?}");
    // Copper is off by default: 20 × 10 mm at the board's own 2 mm.
    let got = volume(&mut s);
    assert!((got - 400.0).abs() < 1e-6, "{got}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_options_choose_the_parts_and_override_the_thickness() {
    let path = board_file("options", 2.0);
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path.to_string_lossy(), "copper": "mesh", "silkscreen": false, "thickness": 1.0}));
    assert_eq!(r["board_solid"], json!(true), "{r}");
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert!(names.iter().any(|name| name == "Copper F.Cu"), "{names:?}");
    // The solid substrate is 200 mm³ with the overriding 1 mm thickness, plus the 9 mm × 0.3 mm
    // × 0.035 mm copper track.
    let got = volume(&mut s);
    assert!((got - (200.0 + 9.0 * 0.3 * 0.035)).abs() < 1e-3, "{got}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_bad_option_is_refused_before_the_board_is_read() {
    let path = board_file("bad", 2.0);
    let mut s = Session::default();
    let error = s.execute("doc.open", &json!({"path": path.to_string_lossy(), "copper": "brep"})).unwrap_err();
    assert!(format!("{error}").contains("copper"), "{error}");
    assert!(s.doc.features.is_empty(), "a refused option left a feature behind");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
