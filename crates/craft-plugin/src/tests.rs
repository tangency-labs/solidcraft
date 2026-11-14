use super::*;
use serde_json::json;

fn manifest() -> Value {
    json!({"id":"sample","name":" Sample ","version":" 1 ","capabilities":["commands@0.1"],"runtime":{
        "module":"plugin.wasm","entries":["make"],"commands":["app.make"]
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

#[test]
fn capabilities_are_checked_at_load_with_plain_refusals() {
    assert!(WIT.starts_with("package craft:plugin@0.1.0;") && API == "craft:plugin@0.1.0");
    // The commands capability is the WIT package's version.
    let (_, version) = API.split_once('@').unwrap();
    let (major, minor) = CAPABILITIES.iter().find(|(name, _)| *name == "commands").unwrap().1;
    assert!(version.starts_with(&format!("{major}.{minor}.")));
    let caps = |list: &[&str]| list.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    assert!(check_capabilities(&caps(&["commands@0.1"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["commands@0.1.3"]), "Craft").is_ok());
    let refused = |list: &[&str]| match check_capabilities(&caps(list), "Craft") {
        Err(HostError::UnsupportedCapability { reason, .. }) => reason,
        other => panic!("{list:?}: {other:?}"),
    };
    assert_eq!(refused(&["commands@0.2"]), "needs commands 0.2; update Craft or the plugin");
    assert_eq!(refused(&["commands@1.0"]), "needs commands 1.0; update Craft or the plugin");
    assert!(refused(&["commands@0.0"]).contains("was built for commands 0.0") && refused(&["commands@0.0"]).ends_with("update the plugin"));
    assert!(refused(&["ui@0.1"]).starts_with("needs ui 0.1, which this version of Craft does not have"));
    for bad in [&["commands"][..], &["commands@x"], &["commands@0.1", "commands@0.1"], &["Commands@0.1"], &["commands@0.1.2.3"]] {
        assert!(matches!(check_capabilities(&caps(bad), "Craft"), Err(HostError::InvalidManifest { .. })), "{bad:?}");
    }
    // A runtime needs the commands capability; inert metadata needs none.
    let mut m = manifest();
    m.as_object_mut().unwrap().remove("capabilities");
    assert!(matches!(parse_manifest(m.to_string().as_bytes(), "sample"), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    assert!(parse_manifest(br#"{"id":"sample","name":"Sample","version":"1"}"#, "sample").is_ok());
    // A plugin from before capabilities is told what it was built for, not a parse error.
    let mut old = manifest();
    old["runtime"]["api"] = json!("solidcraft:plugin@1.0.0");
    let refusal = parse_manifest(old.to_string().as_bytes(), "sample").unwrap_err().to_string();
    assert!(refusal.contains("was built for solidcraft:plugin@1.0.0") && refusal.contains("craft:plugin@0.1.0"), "{refusal}");
    // Sensitive command groups are named, bounded requests.
    let mut groups = manifest();
    groups["runtime"]["permissions"] = json!({"command_groups":["file","doc"]});
    assert_eq!(parse_manifest(groups.to_string().as_bytes(), "sample").unwrap().runtime.unwrap().permissions.command_groups.len(), 2);
    groups["runtime"]["permissions"] = json!({"command_groups":["File"]});
    assert!(parse_manifest(groups.to_string().as_bytes(), "sample").is_err());
}

fn importer() -> Value {
    json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.1"],"runtime":{
        "module":"sample.wasm","commands":[],"entries":[{"name":"convert","kind":"importer","extensions":["f3d","ipt"]}],
        "limits":{"fuel":1000,"memory_mib":512,"deadline_ms":300000,"stack_kib":16384},"permissions":{"input_folder":true}
    }})
}

#[test]
fn an_importer_claims_extensions_needs_formats_and_never_mixes_with_commands() {
    let runtime = parse(&importer()).unwrap().runtime.unwrap();
    assert!(runtime.is_importer());
    assert_eq!(runtime.max_component_bytes(), MAX_IMPORTER_COMPONENT_BYTES);
    assert_eq!(runtime.importer_for(Path::new("/x/Bracket.F3D")).map(|e| e.name.as_str()), Some("convert"));
    assert!(runtime.importer_for(Path::new("bracket.step")).is_none());
    let refused = |change: &dyn Fn(&mut Value), field: &str| {
        let mut value = importer();
        change(&mut value);
        let failure = parse(&value).unwrap_err();
        assert!(matches!(&failure, HostError::InvalidManifest { field: f, .. } if f.contains(field)), "{field}: {failure}");
    };
    refused(&|v| v["capabilities"] = json!(["commands@0.1"]), "capabilities");
    refused(&|v| v["runtime"]["entries"][0]["extensions"] = json!([]), "extensions");
    refused(&|v| v["runtime"]["entries"][0]["extensions"] = json!([".f3d"]), "extensions");
    refused(&|v| v["runtime"]["entries"][0]["extensions"] = json!(["f3d", "f3d"]), "extensions");
    refused(&|v| v["runtime"]["entries"][0]["effect"] = json!("read"), "effect");
    refused(&|v| v["runtime"]["entries"].as_array_mut().unwrap().push(json!({"name":"make"})), "runtime.entries");
    refused(&|v| v["runtime"]["commands"] = json!(["app.make"]), "runtime.commands");
    refused(&|v| v["runtime"]["limits"]["stack_kib"] = json!(1 << 20), "stack_kib");
    // A command claims no extension, reads no input folder and still finishes within 30 s.
    let command = |change: &dyn Fn(&mut Value), field: &str| {
        let mut value = manifest();
        change(&mut value);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field: f, .. }) if f.contains(field)), "{field}");
    };
    command(&|v| v["runtime"]["entries"] = json!([{"name":"make","extensions":["f3d"]}]), "extensions");
    command(&|v| v["runtime"]["permissions"] = json!({"input_folder":true}), "input_folder");
    command(&|v| v["runtime"]["limits"] = json!({"fuel":1,"memory_mib":1,"deadline_ms":60000}), "deadline_ms");
}

#[test]
fn a_loss_report_is_bounded_printable_text() {
    let report: LossReport = serde_json::from_value(json!({"summary":"12 of 14 features","items":[
        {"severity":"degraded","subject":"Fillet3","message":"kept as geometry"}],"details":{"any":"thing"}}))
    .unwrap();
    report.validate().unwrap();
    assert!(serde_json::from_value::<LossReport>(json!({"items":[{"severity":"lost","message":"x"}]})).is_err());
    let control = LossReport { summary: "bad\nline".into(), ..LossReport::default() };
    assert!(matches!(control.validate(), Err(HostError::InvalidInput { field, .. }) if field == "loss_report.summary"));
    let empty =
        LossReport { items: vec![LossItem { severity: LossSeverity::Info, subject: String::new(), message: " ".into() }], ..LossReport::default() };
    assert!(empty.validate().is_err());
}
