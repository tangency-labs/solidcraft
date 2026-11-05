use serde_json::json;

use super::{Export3mfParams, ExportMeshParams, ExportStepParams, ExportUrdfParams};
use crate::Session;
use crate::schema::tests::assert_keys;
use crate::schema::{ParamCheck, input_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ExportStepParams>(), &["bodies", "path"]);
    assert_keys(input_schema_for::<ExportMeshParams>(), &["bodies", "path", "quality"]);
    assert_keys(input_schema_for::<Export3mfParams>(), &["bodies", "objects", "path", "quality"]);
    assert_keys(input_schema_for::<ExportUrdfParams>(), &["effort", "mesh_prefix", "path", "velocity"]);
}

#[test]
fn bad_calls_say_what_is_wrong_and_write_nothing() {
    let dir = std::env::temp_dir().join(format!("solidcraft-export-schemas-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    for cmd in ["file.export_step", "file.export_stl", "file.export_3mf", "file.export_obj", "file.export_urdf"] {
        let e = s.execute(cmd, &json!({})).unwrap_err().to_string();
        assert!(e.contains(cmd) && e.contains("path"), "{cmd}: {e}");
    }
    let at = dir.join("a.stl").to_string_lossy().to_string();
    let e = s.execute("file.export_stl", &json!({"path": at, "quality": "rough"})).unwrap_err().to_string();
    assert!(e.contains("quality"), "{e}");
    s.param_check = ParamCheck::Strict;
    let e = s.execute("file.export_stl", &json!({"path": at, "objects": "component"})).unwrap_err().to_string();
    assert!(e.contains("objects"), "objects is for 3MF only: {e}");
    s.param_check = ParamCheck::Off;
    let e = s.execute("file.export_urdf", &json!({"path": at, "effort": -1})).unwrap_err().to_string();
    assert!(e.contains("effort"), "{e}");
    assert_eq!(std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0), 0, "nothing written");
    let _ = std::fs::remove_dir_all(&dir);
}
