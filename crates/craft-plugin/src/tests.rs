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
        Err(HostError::InvalidManifest { field, reason }) if field == "id" && reason.contains("does not match") && reason.contains("rename the folder to")
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
fn larger_manifests_fit_without_removing_input_descriptions() {
    let serialized = serde_json::to_vec(&manifest()).unwrap();
    assert!(serialized.len() < 16 * 1024);
    assert_eq!(MAX_MANIFEST_BYTES, 64 * 1024);

    // Whitespace pads a real manifest to exact byte boundaries without changing its
    // meaning. The old 16 KiB cap no longer rejects a valid manifest.
    for len in [16 * 1024 + 1, 64 * 1024] {
        let mut bytes = serialized.clone();
        bytes.resize(len, b' ');
        let parsed = parse_manifest(&bytes, "sample").unwrap();
        assert_eq!(parsed.id, "sample");
    }

    let mut too_large = serialized;
    too_large.resize(MAX_MANIFEST_BYTES + 1, b' ');
    assert!(matches!(parse_manifest(&too_large, "sample"), Err(HostError::LimitExceeded { limit }) if limit.contains("64 KiB")));
}

#[test]
fn host_limits_have_typed_failures() {
    let limits = HostLimits::default();
    assert!(limits.check(&Limits::default()).is_ok());
    let fuel = limits.check(&Limits { fuel: limits.max_fuel + 1, ..Limits::default() }).unwrap_err();
    assert_eq!(fuel, HostError::OverCeiling { limit: "fuel".into(), requested: limits.max_fuel + 1, allowed: limits.max_fuel });
    // A refusal gives both numbers and what to change.
    let memory = limits.check(&Limits { memory_mib: 2048, ..Limits::default() }).unwrap_err();
    assert_eq!(memory, HostError::OverCeiling { limit: "memory_mib".into(), requested: 2048, allowed: 128 });
    assert_eq!(
        memory.to_string(),
        "asks for 2048 MiB of memory, more than the 128 MiB plugins may use; allow its memory request when you install it, or raise the memory ceiling for plugins"
    );
    assert_eq!(serde_json::to_value(&memory).unwrap(), json!({"kind":"over_ceiling","limit":"memory_mib","requested":2048,"allowed":128}));
    assert!(HostLimits { max_fuel: 0, ..limits }.validate().is_err());
    // A budget the plugin ran into names it, with the number.
    let stopped = |limit: &str, allowed| HostError::Exhausted { limit: limit.into(), allowed }.to_string();
    assert_eq!(stopped("fuel", 1000), "used all 1000 fuel it asks for and was stopped");
    assert_eq!(stopped("memory_mib", 64), "needed more than the 64 MiB of memory it asks for and was stopped");
    assert_eq!(stopped("deadline_ms", 5000), "ran longer than its time limit of 5000 ms and was stopped");
    assert_eq!(stopped("design_mib", 256), "made a design larger than 256 MiB, the most an importer may hand over");
    assert_eq!(HostError::Unsupported { reason: "no solids".into() }.to_string(), "not supported: no solids");
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
    assert!(WIT.starts_with("package craft:plugin@0.2.0;") && API == "craft:plugin@0.2.0");
    // The frozen 0.1 WIT is 0.1.0 without `unsupported`, and the host still serves its capabilities.
    assert!(WIT_0_1.contains("\npackage craft:plugin@0.1.0;") && !WIT_0_1.contains("unsupported") && WIT.contains("unsupported(string)"));
    assert_eq!(WIT_VERSIONS.first().copied(), API.split_once('@').map(|(_, v)| v));
    // The commands capability is the WIT package's version.
    let (_, version) = API.split_once('@').unwrap();
    let (major, minor) = CAPABILITIES.iter().find(|(name, _)| *name == "commands").unwrap().1;
    assert!(version.starts_with(&format!("{major}.{minor}.")));
    let caps = |list: &[&str]| list.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    assert!(check_capabilities(&caps(&["commands@0.1"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["commands@0.1.3"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["commands@0.2"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["formats@0.1", "evaluate@0.1"]), "Craft").is_ok());
    assert!(check_capabilities(&caps(&["formats@0.2", "evaluate@0.2"]), "Craft").is_ok());
    let refused = |list: &[&str]| match check_capabilities(&caps(list), "Craft") {
        Err(HostError::UnsupportedCapability { reason, .. }) => reason,
        other => panic!("{list:?}: {other:?}"),
    };
    assert_eq!(refused(&["commands@0.3"]), "needs commands 0.3; update Craft or the plugin");
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
    assert!(refusal.contains("was built for solidcraft:plugin@1.0.0") && refusal.contains("craft:plugin@0.2.0"), "{refusal}");
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
    // An importer opens a file as a new design: that is its effect, whether it says so, says
    // nothing, or says `edit` as manifests written before `open` did; a command never opens.
    assert_eq!(runtime.entries[0].effect, Effect::Open);
    assert_eq!(serde_json::to_value(&runtime.entries[0]).unwrap()["effect"], "open");
    for declared in ["open", "edit"] {
        let mut value = importer();
        value["runtime"]["entries"][0]["effect"] = json!(declared);
        assert_eq!(parse(&value).unwrap().runtime.unwrap().entries[0].effect, Effect::Open, "{declared}");
    }
    refused(&|v| v["runtime"]["entries"].as_array_mut().unwrap().push(json!({"name":"make"})), "runtime.entries");
    refused(&|v| v["runtime"]["commands"] = json!(["app.make"]), "runtime.commands");
    refused(&|v| v["runtime"]["limits"]["stack_kib"] = json!(1 << 20), "stack_kib");
    // An importer may also check its result; a command may not ask to.
    let mut evaluating = importer();
    evaluating["capabilities"] = json!(["evaluate@0.1", "formats@0.1"]);
    assert!(parse(&evaluating).is_ok());
    // A command claims no extension, reads no input folder and still finishes within 30 s.
    let command = |change: &dyn Fn(&mut Value), field: &str| {
        let mut value = manifest();
        change(&mut value);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field: f, .. }) if f.contains(field)), "{field}");
    };
    command(&|v| v["runtime"]["entries"] = json!([{"name":"make","extensions":["f3d"]}]), "extensions");
    command(&|v| v["runtime"]["entries"] = json!([{"name":"make","effect":"open"}]), "effect");
    command(&|v| v["runtime"]["permissions"] = json!({"input_folder":true}), "input_folder");
    command(&|v| v["capabilities"] = json!(["commands@0.1", "evaluate@0.1"]), "capabilities");
    command(&|v| v["runtime"]["limits"] = json!({"fuel":1,"memory_mib":1,"deadline_ms":60000}), "deadline_ms");
}

#[test]
fn a_plugin_that_keeps_design_data_names_storage_and_its_data_format() {
    let mut keeps = manifest();
    keeps["capabilities"] = json!(["commands@0.1", "storage@0.1"]);
    keeps["runtime"]["data_format"] = json!(3);
    assert_eq!(parse(&keeps).unwrap().runtime.unwrap().data_format, Some(3));
    let refused = |change: &dyn Fn(&mut Value), field: &str| {
        let mut value = keeps.clone();
        change(&mut value);
        assert!(matches!(parse(&value), Err(HostError::InvalidManifest { field: f, .. }) if f == field), "{field}");
    };
    refused(
        &|v| {
            v["runtime"].as_object_mut().unwrap().remove("data_format");
        },
        "runtime.data_format",
    );
    refused(&|v| v["runtime"]["data_format"] = json!(0), "runtime.data_format");
    refused(&|v| v["runtime"]["data_format"] = json!(MAX_DATA_FORMAT + 1), "runtime.data_format");
    refused(&|v| v["capabilities"] = json!(["commands@0.1"]), "runtime.data_format");
    // An importer keeps nothing in the open design.
    let mut importing = importer();
    importing["capabilities"] = json!(["formats@0.1", "storage@0.1"]);
    importing["runtime"]["data_format"] = json!(1);
    assert!(matches!(parse(&importing), Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
}

fn empty_item() -> LossItem {
    LossItem { severity: LossSeverity::Info, title: String::new(), subject: String::new(), message: String::new(), code: String::new() }
}

#[test]
fn a_loss_report_is_bounded_printable_text() {
    let report: LossReport = serde_json::from_value(json!({"summary":"12 of 14 features","items":[
        {"severity":"degraded","subject":"Fillet3","message":"kept as geometry"},
        {"severity":"info","title":"Parts of the file kept as they are","code":"f3d/source.unindexed-records-retained","message":"14 records"}],
        "details":{"any":"thing"}}))
    .unwrap();
    report.validate().unwrap();
    assert_eq!(report.items[1].title, "Parts of the file kept as they are");
    let long_title = json!({"items":[{"severity":"info","title":"t".repeat(121),"message":"x"}]});
    assert!(serde_json::from_value::<LossReport>(long_title).unwrap().validate().is_err());
    assert!(serde_json::from_value::<LossReport>(json!({"items":[{"severity":"lost","message":"x"}]})).is_err());
    let control = LossReport { summary: "bad\nline".into(), ..LossReport::default() };
    assert!(matches!(control.validate(), Err(HostError::InvalidInput { field, .. }) if field == "loss_report.summary"));
    let empty = LossReport {
        items: vec![LossItem { severity: LossSeverity::Info, subject: String::new(), message: " ".into(), ..empty_item() }],
        ..LossReport::default()
    };
    assert!(empty.validate().is_err());
}

#[test]
fn entry_arguments_are_checked_as_strictly_as_app_commands() {
    let schema = json!({"type":"object","required":["teeth"],"properties":{
        "teeth":{"type":"integer","minimum":6,"maximum":200},
        "module":{"type":"number","exclusiveMinimum":0},
        "name":{"type":"string","maxLength":8},
        "up_axis":{"type":"string","oneOf":[{"const":"as_is","title":"As in the file"},{"const":"stand_up","title":"Stand up"}]},
        "bodies":{"type":"array","items":{"type":"string"},"maxItems":2},
        "free":{"type":"object","additionalProperties":true},
        "fixed":{"type":"object"}
    }});
    assert_eq!(check_input(&schema, &json!({"teeth": 20})), Ok(()));
    assert_eq!(check_input(&schema, &json!({"teeth": 20, "up_axis": "stand_up", "bodies": ["b1"], "free": {"any": 1}})), Ok(()));
    let field = |value: Value| match check_input(&schema, &value) {
        Err(HostError::InvalidInput { field, .. }) => field,
        other => panic!("{value} gave {other:?}"),
    };
    assert_eq!(field(json!({})), "params.teeth");
    assert_eq!(field(json!({"teeth": 2.5})), "params.teeth");
    assert_eq!(field(json!({"teeth": 4})), "params.teeth");
    assert_eq!(field(json!({"teeth": 20, "module": 0})), "params.module");
    assert_eq!(field(json!({"teeth": 20, "name": "much too long"})), "params.name");
    assert_eq!(field(json!({"teeth": 20, "up_axis": "sideways"})), "params.up_axis");
    assert_eq!(field(json!({"teeth": 20, "bodies": [1]})), "params.bodies[0]");
    assert_eq!(field(json!({"teeth": 20, "bodies": ["a", "b", "c"]})), "params.bodies");
    assert_eq!(field(json!({"teeth": 20, "extra": true})), "params.extra", "an undeclared field is refused");
    assert_eq!(field(json!([20])), "params");
    assert_eq!(field(json!({"teeth": 20, "fixed": {"any": 1}})), "params.fixed.any", "an object schema takes only what it declares");
    // The listed schema says what the check does.
    assert_eq!(strict_input_schema(&schema)["additionalProperties"], false);
    assert_eq!(strict_input_schema(&schema)["properties"]["free"]["additionalProperties"], true);
    assert_eq!(strict_input_schema(&schema)["properties"]["fixed"]["additionalProperties"], false);
    // A hostile schema is bounded, never a crash.
    let mut deep = json!({"type":"object"});
    for _ in 0..100 {
        deep = json!({"type":"object","properties":{"a":deep}});
    }
    let mut value = json!({});
    for _ in 0..100 {
        value = json!({"a": value});
    }
    assert!(check_input(&deep, &value).is_err());
}

#[test]
fn additional_property_schemas_are_listed_as_strictly_as_they_are_checked() {
    let schema = json!({"type":"object","additionalProperties":{"type":"object","properties":{"x":{}}}});
    let listed = strict_input_schema(&schema);
    assert_eq!(listed["additionalProperties"]["additionalProperties"], false);
    for candidate in [&schema, &listed] {
        assert_eq!(check_input(candidate, &json!({"k":{"x":1}})), Ok(()));
        assert!(matches!(check_input(candidate, &json!({"k":{"x":1,"y":2}})),
            Err(HostError::InvalidInput { field, .. }) if field == "params.k.y"));
    }
}

#[test]
fn schema_composition_does_not_implicitly_close_object_constraints() {
    let schema = json!({"type":"object","allOf":[{"properties":{"a":{"type":"integer"}}}]});
    let listed = strict_input_schema(&schema);
    assert!(listed.get("additionalProperties").is_none());
    assert_eq!(check_input(&schema, &json!({"a":1})), Ok(()));
    assert_eq!(check_input(&listed, &json!({"a":1})), Ok(()));
    for keyword in ["oneOf", "anyOf"] {
        let mut schema = json!({"type":"object","properties":{"a":{},"b":{}}});
        schema[keyword] = json!([{"type":"object","required":["a"]},{"type":"object","required":["b"]}]);
        let listed = strict_input_schema(&schema);
        assert_eq!(listed["additionalProperties"], false);
        assert!(listed[keyword][0].get("additionalProperties").is_none());
        for candidate in [&schema, &listed] {
            assert_eq!(check_input(candidate, &json!({"a":1})), Ok(()));
            assert_eq!(check_input(candidate, &json!({"b":2})), Ok(()));
            assert!(check_input(candidate, &json!({})).is_err());
            assert!(check_input(candidate, &json!({"a":1,"extra":2})).is_err());
        }
        // An explicitly closed branch still refuses undeclared fields.
        schema[keyword][0]["additionalProperties"] = json!(false);
        assert!(check_input(&schema, &json!({"a":1})).is_err());
    }
}
