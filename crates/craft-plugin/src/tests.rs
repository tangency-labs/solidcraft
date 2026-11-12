use super::*;
use serde_json::json;

fn manifest() -> Value {
    json!({"id":"sample","name":" Sample ","version":" 1 ","runtime":{
        "api":API,"module":"plugin.wasm","entries":["make"],"commands":["app.make"]
    }})
}
fn parse(value: &Value) -> Result<Manifest> {
    parse_manifest(&serde_json::to_vec(value).unwrap(), "sample")
}

#[test]
fn metadata_normalization_and_legacy_entries() {
    let parsed = parse(&manifest()).unwrap();
    assert_eq!(parsed.name, "Sample");
    assert_eq!(parsed.version, "1");
    let runtime = parsed.runtime.unwrap();
    assert_eq!(runtime.entries[0].name, "make");
    assert_eq!(runtime.entries[0].effect, Effect::Edit);
    let serialized = serde_json::to_value(&runtime.entries[0]).unwrap();
    assert_eq!(serialized["input_schema"]["type"], "object");
    assert_eq!(serde_json::from_value::<Entry>(serialized).unwrap().name, "make");
    assert!(parse_manifest(br#"{"id":"sample","name":"Sample","version":"1"}"#, "sample").unwrap().runtime.is_none());
}

#[test]
fn declared_inputs_effects_and_slots_are_readable() {
    let mut value = manifest();
    value["runtime"]["entries"] = json!([{"name":"inspect","effect":"read",
        "input_schema":{"type":"object","properties":{"selection":{"type":"array","items":{"type":"string"}}}},
        "slots":[{"kind":"menu","name":"tools"},{"kind":"ribbon","name":"utilities"}]}]);
    let entry = parse(&value).unwrap().runtime.unwrap().entries.remove(0);
    assert_eq!(entry.effect, Effect::Read);
    assert_eq!(entry.slots.len(), 2);
    assert_eq!(entry.input_schema["properties"]["selection"]["type"], "array");
    value["runtime"]["entries"][0]["input_schema"]["properties"]["selection"]["type"] = json!("string");
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field.ends_with("selection")));
}

#[test]
fn schema_properties_must_be_objects_even_in_nested_inputs() {
    for schema in [
        json!({"type":"object","properties":[]}),
        json!({"type":"object","properties":null}),
        json!({"type":"object","properties":{"settings":{"type":"object","properties":false}}}),
    ] {
        let mut value = manifest();
        value["runtime"]["entries"] = json!([{"name":"make","input_schema":schema}]);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field, .. }) if field.ends_with(".properties")));
    }
}

#[test]
fn network_permission_is_a_typed_refusal() {
    let mut value = manifest();
    value["runtime"]["permissions"] = json!({"net":true});
    assert_eq!(parse(&value).unwrap_err(), HostError::PermissionDenied { permission: "net".into() });
}

#[test]
fn named_files_permissions_require_access_and_unique_names() {
    let mut value = manifest();
    value["runtime"]["permissions"] = json!({"files":[{"name":"inputs","read":true}],"clock":true});
    let permissions = parse(&value).unwrap().runtime.unwrap().permissions;
    assert!(permissions.clock);
    assert!(permissions.files[0].read);
    value["runtime"]["permissions"]["files"] = json!([{"name":"inputs"}]);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
    value["runtime"]["permissions"]["files"] = json!([{"name":"inputs","read":true},{"name":"inputs","write":true}]);
    assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
}

#[test]
fn hostile_manifest_fields_return_errors() {
    assert!(matches!(
        parse_manifest(&serde_json::to_vec(&manifest()).unwrap(), "another-folder"),
        Err(HostError::InvalidManifest { field, reason }) if field == "id" && reason.contains("does not match")
    ));
    for change in [
        |value: &mut Value| value["runtime"]["module"] = json!("../plugin.wasm"),
        |value: &mut Value| value["runtime"]["entries"] = json!(["make", "make"]),
        |value: &mut Value| value["runtime"]["limits"] = json!({"fuel":0,"memory_mib":64,"deadline_ms":1}),
        |value: &mut Value| value["runtime"]["limits"] = json!({"fuel":1,"memory_mib":4097,"deadline_ms":1}),
        |value: &mut Value| value["runtime"]["limits"] = json!({"fuel":1,"memory_mib":1,"deadline_ms":30001}),
        |value: &mut Value| value["runtime"]["entries"] = json!([{"name":"make","input_schema":true}]),
    ] {
        let mut value = manifest();
        change(&mut value);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { .. })));
    }
    assert!(matches!(parse_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1], "sample"), Err(HostError::LimitExceeded { .. })));
    assert!(parse_manifest(br#"{}"#, "sample").is_err());
}

#[test]
fn host_limits_have_typed_failures() {
    let limits = HostLimits::default();
    assert!(limits.check(&Limits::default()).is_ok());
    assert!(
        matches!(limits.check(&Limits { fuel: limits.max_fuel + 1, ..Limits::default() }), Err(HostError::LimitExceeded { limit }) if limit.contains("fuel"))
    );
    assert!(
        matches!(limits.check(&Limits { memory_mib: limits.max_memory_mib + 1, ..Limits::default() }), Err(HostError::LimitExceeded { limit }) if limit.contains("memory"))
    );
    assert!(HostLimits { max_fuel: 0, ..limits }.validate().is_err());
}

#[test]
fn host_errors_are_machine_readable() {
    let error = HostError::CommandFailed { command: "app.make".into(), reason: "missing part".into() };
    let value = serde_json::to_value(&error).unwrap();
    assert_eq!(value["kind"], "command_failed");
    assert_eq!(value["command"], "app.make");
    assert_eq!(serde_json::from_value::<HostError>(value).unwrap(), error);
    assert!(WIT.contains(&format!("package {API};")));
}

#[test]
fn explicit_folder_grants_allow_hidden_ancestors_but_refuse_hidden_children() {
    struct Host(Vec<FileGrant>);
    impl PluginHost for Host {
        fn command_catalog(&self) -> Vec<CommandSchema> {
            vec![]
        }
        fn run_command(&mut self, _: &str, _: &Value) -> Result<Value> {
            Ok(Value::Null)
        }
        fn begin_transaction(&mut self) -> Result<()> {
            Ok(())
        }
        fn commit_transaction(&mut self) -> Result<()> {
            Ok(())
        }
        fn rollback_transaction(&mut self) -> Result<()> {
            Ok(())
        }
        fn granted_folders(&self) -> Vec<FileGrant> {
            self.0.clone()
        }
        fn log(&mut self, _: &str) {}
        fn progress(&mut self, _: f32, _: &str) {}
    }
    let folder = Path::new("parent").join(".private").join("approved");
    for (read, write) in [(true, true), (true, false), (false, true)] {
        let host = Host(vec![FileGrant { name: "project".into(), folder: folder.clone(), read, write }]);
        assert_eq!(host.authorize_file_path(&folder.join("ordinary.txt"), false).is_ok(), read);
        assert_eq!(host.authorize_file_path(&folder.join("ordinary.txt"), true).is_ok(), write);
        for operation in [false, true] {
            for child in [Path::new(".env").to_owned(), Path::new(".nested").join("ordinary.txt")] {
                assert!(matches!(host.authorize_file_path(&folder.join(child), operation), Err(HostError::PermissionDenied { .. })));
            }
        }
    }
    let mut host = Host(vec![FileGrant { name: "project".into(), folder: folder.clone(), read: true, write: true }]);
    host.0.push(FileGrant { name: "explicit-hidden".into(), folder: folder.join(".nested"), read: true, write: false });
    assert!(host.authorize_file_path(&folder.join(".nested/ordinary.txt"), false).is_ok());
    assert!(host.authorize_file_path(&folder.join(".nested/ordinary.txt"), true).is_err());
    assert!(Host(vec![]).authorize_file_path(&folder.join("ordinary.txt"), false).is_err());
}
