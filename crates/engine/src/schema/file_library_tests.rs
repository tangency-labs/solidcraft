use serde_json::{Value, json};

use super::{FileTransferParams, FileTransferResult, FileTrashParams, FileTrashResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<FileTransferParams>(), &["destination", "path"]);
    assert_keys(input_schema_for::<FileTrashParams>(), &["path"]);
    // Both commands need their paths, so the schema says so even though the type leaves them to
    // the handler, whose message a caller then hears.
    assert_eq!(input_schema_for::<FileTransferParams>().as_value()["required"], json!(["path", "destination"]));
    assert_eq!(input_schema_for::<FileTrashParams>().as_value()["required"], json!(["path"]));
}

/// A path the contract takes is one `cmd::file_library::arg` takes: text with something in it,
/// under 4096 bytes, no NUL character. Which files keep a design extension stays the command's
/// to refuse, so the contract takes any path that looks like one.
#[test]
fn paths_are_read_as_the_handler_reads_them() {
    let transfer = |p: Value| serde_json::from_value::<FileTransferParams>(p);
    assert!(transfer(json!({"path": "Part.sc", "destination": "Folder/Copy.solidcraft"})).is_ok());
    assert!(transfer(json!({"path": "x".repeat(4095), "destination": "Copy.sc"})).is_ok(), "4095 bytes is under the limit");
    for bad in [json!(""), json!("   "), json!(5), json!(null), json!([]), json!("Part\0.sc"), json!("x".repeat(4096))] {
        assert!(transfer(json!({"path": bad.clone(), "destination": "Copy.sc"})).is_err(), "{bad} as `path`");
        assert!(transfer(json!({"path": "Part.sc", "destination": bad.clone()})).is_err(), "{bad} as `destination`");
        assert!(serde_json::from_value::<FileTrashParams>(json!({"path": bad})).is_err(), "{bad} as `path`");
    }
    // Left out, the type has no value and the handler refuses the call in its own words.
    let half = transfer(json!({"path": "Part.sc"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((half.path.as_deref(), half.destination.as_deref()), (Some("Part.sc"), None));
    let schema = input_schema_for::<FileTransferParams>();
    let path = &schema.as_value()["properties"]["path"];
    assert_eq!((&path["type"], &path["minLength"], &path["maxLength"]), (&json!("string"), &json!(1), &json!(4095)), "{path}");
}

/// A key the command does not take is the check's to name, with the closest key it does take;
/// the handler never sees it and goes on to what it checks itself.
#[test]
fn a_misspelt_key_is_named_with_the_key_it_meant() {
    let call = |id: &str, p: Value| {
        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        s.execute(id, &p).err().unwrap_or_else(|| panic!("{id} {p} should be refused")).to_string()
    };
    assert_eq!(
        call("file.copy", json!({"path": "Part.sc", "destination": "Copy.sc", "destinaton": "Copy.sc"})),
        "invalid parameters for `file.copy`: unknown key `destinaton`; did you mean `destination`?"
    );
    assert_eq!(call("file.trash", json!({"path": "Part.sc", "trase": true})), "invalid parameters for `file.trash`: unknown key `trase`");
    // With the check off the extra key is simply not one the handler reads.
    let mut s = Session::default();
    let e = s.execute("file.copy", &json!({"path": "Part.txt", "destination": "Copy.sc", "destinaton": 1})).unwrap_err().to_string();
    assert!(e.contains("Keep a supported design extension"), "{e}");
}

/// A copy, a move and a Trash reply match their output schemas; a warning about a key the command
/// does not take matches it too.
#[test]
fn replies_match_the_output_schema() {
    let dir = folder("replies");
    let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let source = saved(&dir, "Part.sc");
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let transfer = output_schema_for::<FileTransferResult>();
    let transfer = transfer.as_value();
    let matches = |reply: &Value| {
        assert!(conforms(transfer, transfer, reply), "{reply} doesn't match {transfer}");
        assert!(reply["path"].is_string() && reply["destination"].is_string(), "{reply}");
    };

    let copied = run(&mut s, "file.copy", json!({"path": source, "destination": at("Copy.sc")}));
    matches(&copied);
    assert_eq!((copied["path"].as_str(), copied["destination"].as_str()), (Some(source.as_str()), Some(at("Copy.sc").as_str())), "{copied}");
    assert!(copied.get("param_warnings").is_none(), "a call with nothing wrong warns about nothing: {copied}");
    assert!(std::path::Path::new(&at("Copy.sc")).exists(), "the copy is there");

    let moved = run(&mut s, "file.move", json!({"path": at("Copy.sc"), "destination": at("Renamed.sc")}));
    matches(&moved);
    assert!(!std::path::Path::new(&at("Copy.sc")).exists() && std::path::Path::new(&at("Renamed.sc")).exists(), "{moved}");

    // The same command with a key it does not take: the file is still copied, and the warning is
    // part of the reply the schema describes.
    let warned = run(&mut s, "file.copy", json!({"path": at("Renamed.sc"), "destination": at("Second.sc"), "destinaton": at("Second.sc")}));
    matches(&warned);
    assert!(warned["param_warnings"].as_array().is_some_and(|w| w.iter().any(|w| w.as_str().is_some_and(|w| w.contains("destinaton")))), "{warned}");

    // The Trash reply names the file and says it can be put back. A real Trash call needs the
    // desktop's trash backend, which no test run can count on; `cmd::file_library`'s own tests
    // cover one where it is available.
    let trashed = FileTrashResult { path: at("Gone.sc"), trashed: true, param_warnings: Vec::new() };
    let reply = serde_json::to_value(trashed).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<FileTrashResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &reply), "{reply} doesn't match {schema}");
    assert_eq!(reply, json!({"path": at("Gone.sc"), "trashed": true}));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Checked against the contract, each of these calls is refused in the same words as today: the
/// paths are read as `arg` reads them, and which files are design files, and which are in use,
/// stays the handler's to say.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("file.copy", json!({})),
        ("file.copy", json!({"path": "Part.sc"})),
        ("file.copy", json!({"path": null, "destination": "Copy.sc"})),
        ("file.copy", json!({"path": " ", "destination": "Copy.sc"})),
        ("file.copy", json!({"path": 5, "destination": "Copy.sc"})),
        ("file.copy", json!({"path": "Part.sc", "destination": ""})),
        ("file.copy", json!({"path": "Part.sc", "destination": "x".repeat(4096)})),
        ("file.copy", json!({"path": "Part\0.sc", "destination": "Copy.sc"})),
        ("file.copy", json!({"path": "Part.txt", "destination": "Copy.sc"})),
        ("file.copy", json!({"path": "Part.sc", "destination": "Copy.txt"})),
        ("file.move", json!({"path": "Part.sc", "destination": 1})),
        ("file.trash", json!({})),
        ("file.trash", json!({"path": ""})),
        ("file.trash", json!({"path": []})),
        ("file.trash", json!({"path": "Part.txt"})),
    ]);
}

/// A fresh folder for one test's files.
fn folder(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-library-schemas-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

/// A saved design file in `dir`, as small as one can be, by its path.
fn saved(dir: &std::path::Path, name: &str) -> String {
    let path = dir.join(name);
    let bytes = solidcraft_io::write_design(&solidcraft_doc::Document::new(name)).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(&path, bytes).unwrap_or_else(|e| panic!("{e}"));
    path.to_string_lossy().into_owned()
}
