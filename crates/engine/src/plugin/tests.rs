use super::*;
use serde_json::{Value, json};

fn manifest() -> Value {
    json!({"id":"sample", "name":"Sample", "version":"0.1.0", "capabilities":["commands@0.1"],"runtime":{
        "module":"plugin.wasm","entries":["make"],"commands":["solid.box"],
        "limits":{"fuel":100000,"memory_mib":1,"deadline_ms":5000}}})
}
#[test]
fn manifest_is_inert_bounded_and_refuses_unknown_capabilities() {
    assert!(parse_manifest(br#"{"id":"sample","name":"Sample","version":"1"}"#, "sample").unwrap().runtime.is_none());
    assert!(parse_manifest(&serde_json::to_vec(&manifest()).unwrap(), "sample").is_ok());
    let mut explicit = manifest();
    explicit["runtime"]["limits"]["fuel"] = json!(100_000_000_000_u64);
    assert!(parse_manifest(&serde_json::to_vec(&explicit).unwrap(), "sample").is_ok());
    assert_eq!(Limits::default().fuel, 10_000_000);
    let old = parse_manifest(br#"{"id":"sample","name":" Sample ","version":" 1 ","description":" Old metadata "}"#, "sample").unwrap();
    assert_eq!((old.name.as_str(), old.version.as_str(), old.description.as_str()), ("Sample", "1", "Old metadata"));
    for mutate in [
        |m: &mut Value| m["runtime"]["module"] = json!("../other.wasm"),
        |m: &mut Value| m["runtime"]["commands"] = json!(["file.export"]),
        |m: &mut Value| m["runtime"]["commands"] = json!(["plugin.run"]),
        |m: &mut Value| m["runtime"]["entries"] = json!(["make", "make"]),
        |m: &mut Value| m["runtime"]["limits"]["memory_mib"] = json!(4097),
        |m: &mut Value| m["runtime"]["limits"]["fuel"] = json!(0),
        |m: &mut Value| m["runtime"]["limits"]["deadline_ms"] = json!(30_001),
        |m: &mut Value| m["runtime"]["api"] = json!("solidcraft:plugin@1.0.0"),
        |m: &mut Value| m["capabilities"] = json!(["commands@0.2"]),
        |m: &mut Value| m["capabilities"] = json!([]),
        |m: &mut Value| m["runtime"]["commands"] = json!(["solid.no_such_command"]),
        |m: &mut Value| m["runtime"]["permissions"] = json!({"command_groups":["network"]}),
        |m: &mut Value| m["network"] = json!(true),
        |m: &mut Value| m["permissions"] = json!(["net"]),
        |m: &mut Value| m["runtime"]["permissions"] = json!(["net"]),
        |m: &mut Value| m["runtime"]["permissions"] = json!({"net":true}),
    ] {
        let mut m = manifest();
        mutate(&mut m);
        assert!(parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").is_err(), "{m}");
    }
    assert!(parse_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1], "sample").is_err());
}
#[test]
fn capability_refusals_are_plain_and_name_solidcraft() {
    let refusal = |capabilities: Value| {
        let mut m = manifest();
        m["capabilities"] = capabilities;
        parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").unwrap_err().to_string()
    };
    assert_eq!(refusal(json!(["commands@0.2"])), "plugin: needs commands 0.2; update SolidCraft or the plugin");
    assert!(refusal(json!(["commands@0.1", "viewport@0.1"])).contains("needs viewport 0.1, which this version of SolidCraft does not have"));
    let mut old = manifest();
    old["runtime"]["api"] = json!("solidcraft:plugin@1.0.0");
    let old = parse_manifest(&serde_json::to_vec(&old).unwrap(), "sample").unwrap_err().to_string();
    assert!(old.contains("was built for solidcraft:plugin@1.0.0") && old.contains("update the plugin"), "{old}");
}
#[test]
fn every_built_command_is_reachable_and_sensitive_ones_need_their_group() {
    let host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
    };
    let catalog = host.command_catalog();
    let built = crate::command_specs().into_iter().filter(|c| c.planned.is_none()).count();
    assert_eq!(catalog.len(), built, "the plugin catalog is the whole MCP catalog");
    let group = |id: &str| catalog.iter().find(|c| c.name == id).unwrap_or_else(|| panic!("{id} is in the catalog")).group.clone();
    for (id, expected) in [
        ("solid.box", None),
        ("sketch.line", None),
        ("timeline.inspect", None),
        ("parameters.change", None),
        ("file.save", Some("file")),
        ("file.export_step", Some("file")),
        ("sketch.insert_dxf", Some("file")),
        ("canvas.insert", Some("file")),
        ("library.export", Some("file")),
        ("doc.open", Some("doc")),
        ("settings.get", Some("app")),
        ("plugin.run", Some("plugin")),
    ] {
        if crate::find_command(id).is_some() {
            assert_eq!(group(id).as_deref(), expected, "{id}");
        }
    }
    for c in &catalog {
        if let Some(g) = &c.group {
            assert!(COMMAND_GROUPS.contains(&g.as_str()), "{}: {g}", c.name);
        }
    }
    // A manifest names the group for each sensitive command it lists; then it loads.
    let mut m = manifest();
    m["runtime"]["commands"] = json!(["solid.box", "file.export_step"]);
    let refused = parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").unwrap_err().to_string();
    assert!(refused.contains("needs \"file\" in runtime.permissions.command_groups"), "{refused}");
    m["runtime"]["permissions"] = json!({"command_groups":["file"]});
    assert!(parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").is_ok());
}
#[test]
fn a_sensitive_command_is_refused_unless_granted_and_a_granted_file_command_sees_only_its_folders() {
    let root = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-plugin-groups-{}", std::process::id()));
    let granted = root.join("granted");
    std::fs::create_dir_all(&granted).unwrap();
    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
    };
    host.begin_transaction().unwrap();
    s_box(&mut host);
    let save = |host: &mut SolidCraftHost, path: &std::path::Path| host.run_command("file.save_as", &json!({"path": path.to_string_lossy()}));
    let refused = save(&mut host, &granted.join("a.sc")).unwrap_err();
    assert!(matches!(&refused, HostError::PermissionDenied { permission } if permission.contains("`file` permission")), "{refused}");
    for id in ["doc.open", "plugin.list"] {
        assert!(matches!(host.run_command(id, &json!({})), Err(HostError::PermissionDenied { .. })), "{id}");
    }
    host.groups.insert("file".into());
    host.file_folders = vec![granted.clone()];
    save(&mut host, &granted.join("a.sc")).unwrap();
    assert!(granted.join("a.sc").is_file());
    let outside = save(&mut host, &root.join("outside.sc")).unwrap_err();
    assert!(matches!(outside, HostError::CommandFailed { .. }), "{outside}");
    assert!(!root.join("outside.sc").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
fn s_box(host: &mut SolidCraftHost) {
    host.run_command("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
}
#[test]
fn disabled_ids_survive_scratch_and_new_design() {
    let mut s = Session::default();
    s.disabled_plugins.insert("sample".into());
    assert!(s.scratch().disabled_plugins.contains("sample"));
    s.execute("file.new", &json!({})).unwrap();
    assert!(s.disabled_plugins.contains("sample"));
}

#[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod native {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new(code: &str) -> Self {
            let root =
                solidcraft_io::vfs::temp_dir().join(format!("solidcraft-plugin-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            let dir = root.join("sample");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("plugin.json"), serde_json::to_vec(&manifest()).unwrap()).unwrap();
            std::fs::write(dir.join("plugin.wasm"), component(code)).unwrap();
            Self(root)
        }
        fn path(&self) -> String {
            self.0.join("sample").to_string_lossy().into_owned()
        }
        fn run(&self, s: &mut Session) -> crate::Result<Value> {
            let approval = s.execute("plugin.inspect", &json!({"path":self.path()}))?["approval"].clone();
            s.execute("plugin.run", &json!({"path":self.path(),"approval":approval,"entry":"make","params":{}}))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // An original component fixture. A real canonical lift/lower crosses the host boundary.
    // $box asks for a 2 x 3 x 4 mm solid; memory at 0 is the canonical result area.
    fn component_template() -> String {
        let (package, version) = API.split_once('@').unwrap();
        include_str!("fixture.wat").replace("\r\n", "\n").replace("__PACKAGE__", package).replace("__VERSION__", version)
    }
    fn component(code: &str) -> Vec<u8> {
        let template = component_template();
        let original = "      call $box\n      call $box\n";
        assert!(template.contains(original));
        wat::parse_str(template.replacen(original, &format!("      {code}\n"), 1)).unwrap()
    }
    fn file_component(write: bool) -> Vec<u8> {
        file_component_path(write, "secret.txt")
    }
    fn file_component_path(write: bool, path: &str) -> Vec<u8> {
        let path_len = path.len();
        let (package, version) = API.split_once('@').unwrap();
        let template = component_template();
        // Keep exactly the shared WIT types, then exercise the real files interface.
        let (types, _) = template.split_once(&format!("  (type $ty-{package}/commands@{version}")).unwrap();
        let (operation, result, parameters, arguments) = if write {
            ("write-file", "(result (error $error))", "(param \"data\" (list u8))", "i32.const 128 i32.const 7")
        } else {
            ("read-file", "(result (list u8) (error $error))", "", "")
        };
        let core_parameters = if write { "i32 i32 i32 i32 i32 i32 i32" } else { "i32 i32 i32 i32 i32" };
        let source = format!(
            r#"{types}
  (type $files-type (instance
    (alias outer 1 $plugin-error (type $error))
    (export "plugin-error" (type (eq $error)))
    (type $file-result {result})
    (type $operation (func (param "grant" string) (param "path" string) {parameters} (result $file-result)))
    (export "{operation}" (func (type $operation)))
  ))
  (import "{package}/files@{version}" (instance $files (type $files-type)))
  (alias export $files "{operation}" (func $file))
  (core module $allocation
    (memory (export "memory") 1)
    (global $heap (mut i32) i32.const 4096)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $at i32)
      global.get $heap local.tee $at local.get 3 i32.add global.set $heap local.get $at)
  )
  (core instance $allocation (instantiate $allocation))
  (alias core export $allocation "memory" (core memory $memory))
  (alias core export $allocation "realloc" (core func $realloc))
  (core func $file (canon lower (func $file) (memory $memory) (realloc $realloc)))
  (core instance $imports (export "file" (func $file)) (export "memory" (memory $memory)))
  (core module $main
    (import "host" "file" (func $file (param {core_parameters})))
    (import "host" "memory" (memory 1))
    (data (i32.const 64) "project")
    (data (i32.const 96) "{path}")
    (data (i32.const 128) "changed")
    (func (export "execute") (param i32 i32 i32 i32) (result i32)
      i32.const 64 i32.const 7 i32.const 96 i32.const {path_len} {arguments} i32.const 0 call $file
      i32.const 0)
  )
  (core instance $main (instantiate $main (with "host" (instance $imports))))
  (alias core export $main "execute" (core func $execute))
  (func $execute (param "entry" string) (param "params" $json) (result (result $json (error $plugin-error)))
    (canon lift (core func $execute) (memory $memory) (realloc $realloc)))
  (instance $command
    (export "json" (type $json))
    (export "plugin-error" (type $plugin-error))
    (export "execute" (func $execute)))
  (export "{package}/entry@{version}" (instance $command))
)"#
        );
        wat::parse_str(source).unwrap()
    }
    #[test]
    fn larger_declared_budgets_need_user_ceilings_and_exact_content_approval() {
        let f = Fixture::new("call $box i32.const 2048 memory.grow i32.const -1 i32.eq if unreachable end");
        let path = f.0.join("sample/plugin.json");
        let mut m = manifest();
        m["runtime"]["limits"] = json!({"fuel":200_000_000_000_u64,"memory_mib":256,"deadline_ms":5000});
        std::fs::write(&path, m.to_string()).unwrap();
        let mut s = Session::default();
        let inspection = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        assert_eq!(inspection["within_host_limits"], false);
        assert_eq!(inspection["host_limits"]["max_memory_mib"], 128);
        assert!(inspection["limits_error"].as_str().unwrap().contains("user host limits"));
        let before = s.doc.to_json();
        assert!(f.run(&mut s).unwrap_err().to_string().contains("user host limits"));
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
        s.plugin_limits = HostLimits { max_fuel: 200_000_000_000, max_memory_mib: 256 };
        let approved = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        assert_eq!(approved["within_host_limits"], true);
        assert_eq!(approved["approval"], inspection["approval"], "approval pins the exact request; a ceiling change never changes its budget");
        f.run(&mut s).unwrap();
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 24.);
        assert_eq!(s.undo.len(), 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        // Even the largest u64 budget reaches Wasmtime exactly; a new request needs new approval.
        m["runtime"]["limits"]["fuel"] = json!(u64::MAX);
        std::fs::write(&path, m.to_string()).unwrap();
        s.plugin_limits.max_fuel = u64::MAX;
        assert!(s.execute("plugin.run", &json!({"path":f.path(),"approval":approved["approval"],"entry":"make"})).is_err());
        assert_eq!(s.doc.to_json(), before);
        f.run(&mut s).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        // Reducing the host policy blocks a previously reviewed package.
        s.plugin_limits = HostLimits::default();
        assert!(f.run(&mut s).is_err());
        assert_eq!(s.doc.to_json(), before);
    }
    #[test]
    fn a_plugin_prepared_under_an_old_host_policy_is_not_adopted() {
        for command in ["plugin.run", "PLUGIN.RUN"] {
            let f = Fixture::new("call $box");
            let mut s = Session::default();
            let i = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
            let params = json!({"path":f.path(),"approval":i["approval"],"entry":"make"});
            let prepared = s.scratch().compute_commands(&[(command.into(), params)], None).unwrap();
            assert!(prepared.is_current(&s));
            s.plugin_limits.max_memory_mib = 512;
            assert!(!prepared.is_current(&s));
            let before = s.doc.to_json();
            assert!(s.accept_commands(prepared).is_err());
            assert_eq!(s.doc.to_json(), before);
            assert!(s.undo.is_empty());
        }
    }
    #[test]
    fn component_edits_are_one_undo_step_and_reopen_without_the_plugin() {
        let f = Fixture::new("call $box call $box");
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
        let before = s.doc.to_json();
        let undo = s.undo.len();
        f.run(&mut s).unwrap();
        assert_eq!(s.doc.features.len(), 3);
        assert_eq!(s.undo.len(), undo + 1);
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 72.);
        let saved = s.doc.to_json();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo, "one undo removes the entire plugin edit");
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.to_json(), saved);
        assert_eq!(s.undo.len(), undo + 1);
        drop(f);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert_eq!(reopened.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 72.);
    }
    #[test]
    fn component_file_reads_and_writes_need_a_host_grant_even_when_declared() {
        for write in [false, true] {
            for declared in [false, true] {
                let f = Fixture::new("call $box");
                let secret = f.0.join("sample/secret.txt");
                std::fs::write(&secret, b"private contents").unwrap();
                std::fs::write(f.0.join("sample/plugin.wasm"), file_component(write)).unwrap();
                if declared {
                    let mut m = manifest();
                    m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":true}]});
                    std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
                }
                let mut s = Session::default();
                s.execute("solid.box", &json!({"length":1,"width":2,"height":3})).unwrap();
                let before = s.doc.to_json();
                let history = (s.undo.len(), s.redo.len());
                assert!(s.plugin_state.file_grants.is_empty());
                let error = f.run(&mut s).unwrap_err();
                let expected = if write { "files.write:project" } else { "files.read:project" };
                assert!(
                    matches!(&error, EngineError::Plugin(craft_plugin::HostError::PermissionDenied { permission }) if permission == expected),
                    "write={write}, declared={declared}: {error}"
                );
                assert_eq!(std::fs::read(&secret).unwrap(), b"private contents");
                assert_eq!(s.doc.to_json(), before);
                assert_eq!((s.undo.len(), s.redo.len()), history);
                assert_eq!(solidcraft_io::vfs::current_scope(), None);
            }
        }
    }
    #[test]
    fn traps_fuel_memory_and_denied_calls_leave_design_and_history_untouched() {
        for (code, diagnostic) in [
            ("call $box unreachable", "trap=UnreachableCodeReached"),
            ("call $box i32.const 8 i32.const 1 i32.store", "entry result is not JSON"),
            ("call $box (loop $forever br $forever)", "trap=OutOfFuel"),
            ("call $box i32.const 10000 memory.grow drop", "forcing trap when growing memory"),
            ("call $box i32.const -1 i32.load drop", "trap=MemoryOutOfBounds"),
            ("call $box i32.const 1 i32.const 0 i32.div_u drop", "trap=IntegerDivisionByZero"),
            ("call $box i32.const 2048 i32.const 11 i32.const 1024 i32.const 33 i32.const 0 call $run", "not granted"),
            (
                "call $box i32.const 2048 i32.const 11 i32.const 1024 i32.const 33 i32.const 0 call $run i32.const 0 i32.const 0 i32.store i32.const 4 i32.const 1024 i32.store i32.const 8 i32.const 33 i32.store",
                "not granted",
            ),
        ] {
            let f = Fixture::new(code);
            let mut s = Session::default();
            s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            let before = s.doc.to_json();
            let history = (s.undo.len(), s.redo.len());
            let message = f.run(&mut s).unwrap_err().to_string();
            assert!(message.contains(diagnostic), "{code}: {message}");
            if diagnostic == "trap=OutOfFuel" {
                assert!(message.contains("fuel_consumed=100000"), "{message}");
                assert!(message.contains("fuel_remaining=0"), "{message}");
            }
            assert_eq!(s.doc.to_json(), before, "{code}");
            assert_eq!((s.undo.len(), s.redo.len()), history, "{code}");
            assert_eq!(s.active_sketch, None);
            assert_eq!(solidcraft_io::vfs::current_scope(), None);
        }
    }
    #[test]
    fn a_granted_component_reads_only_the_evaluated_mesh_and_preserves_history() {
        let f = Fixture::new("call $box");
        let template = component_template();
        let old_args = r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#;
        assert!(template.contains(old_args));
        assert_eq!(template.matches("i32.const 9\n").count(), 1);
        assert_eq!(template.matches("i32.const 33\n").count(), 1);
        let wat = template
            .replace("      call $box\n      call $box\n", "      call $box\n")
            .replace("solid.box", "model.mesh")
            .replace(old_args, r#"{\22body\22:\22Box\22}"#)
            .replace("i32.const 9\n", "i32.const 10\n")
            .replace("i32.const 33\n", "i32.const 14\n");
        std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(wat).unwrap()).unwrap();
        let mut m = manifest();
        m["runtime"]["commands"] = json!(["model.mesh"]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length":2,"width":3,"height":4,"body_name":"Box"})).unwrap();
        let before = s.doc.to_json();
        let history = (s.undo.len(), s.redo.len());
        let result = f.run(&mut s).unwrap();
        assert_eq!(result["result"]["frame"], "component-local-mm");
        assert!(!result["result"]["triangles"].as_array().unwrap().is_empty());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!((s.undo.len(), s.redo.len()), history);
    }

    #[test]
    fn approval_binds_exact_bytes_and_disabled_plugins_never_execute() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let inspected = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        let args = json!({"path":f.path(),"approval":inspected["approval"],"entry":"make"});
        let path = f.0.join("sample/plugin.json");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.push(b' ');
        std::fs::write(&path, bytes).unwrap();
        assert!(s.execute("plugin.run", &args).unwrap_err().to_string().contains("approval"));
        s.disabled_plugins.insert("sample".into());
        assert!(f.run(&mut s).unwrap_err().to_string().contains("disabled"));
        assert!(s.doc.features.is_empty());
        solidcraft_io::vfs::scoped(Some(Default::default()), || {
            assert!(s.execute("plugin.inspect", &json!({"path":f.path()})).is_err());
        });
    }

    #[test]
    fn net_manifest_permission_is_refused_before_execution() {
        let f = Fixture::new("call $box");
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"net":true});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let before = s.doc.to_json();
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(error, EngineError::Plugin(craft_plugin::HostError::PermissionDenied { ref permission }) if permission == "net"));
        assert_eq!(s.doc.to_json(), before);
        assert!(s.undo.is_empty());
    }

    #[test]
    fn sockets_and_http_imports_are_absent_from_the_linker() {
        let (package, version) = API.split_once('@').unwrap();
        for import in [
            "wasi:sockets/network@0.2.0".to_string(),
            "wasi:http/outgoing-handler@0.2.0".into(),
            format!("{package}/fetch@{version}"),
            "fetch".into(),
        ] {
            let f = Fixture::new("call $box");
            let template = component_template();
            let (prefix, _) = template.rsplit_once(')').unwrap();
            let wat = format!("{prefix}\n(type $network-type (instance))\n(import \"{import}\" (instance $network (type $network-type)))\n)");
            std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(wat).unwrap()).unwrap();
            let mut s = Session::default();
            let before = s.doc.to_json();
            let error = f.run(&mut s).unwrap_err();
            assert!(matches!(error, EngineError::Plugin(craft_plugin::HostError::PermissionDenied { .. })), "{import}: {error}");
            assert!(error.to_string().contains(&import), "{error}");
            assert_eq!(s.doc.to_json(), before);
            assert!(s.undo.is_empty());
        }
    }

    #[test]
    fn declared_entry_inputs_are_discoverable_and_namespaced_calls_share_approval() {
        let f = Fixture::new("call $box");
        let mut m = manifest();
        let schema = json!({"type":"object","properties":{"targets":{"type":"array","items":{"type":"string"}}}});
        m["runtime"]["entries"] = json!([{"name":"make","effect":"edit","input_schema":schema,"slots":[{"kind":"menu","name":"tools"}]}]);
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let inspection = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        let listed = s.execute("plugin.list", &json!({})).unwrap();
        let entry = listed["plugins"].as_array().unwrap().iter().find(|p| p["manifest"]["id"] == "sample").unwrap();
        assert_eq!(entry["commands"], json!(["sample.make"]));
        assert_eq!(entry["manifest"]["runtime"]["entries"][0]["input_schema"], schema);
        let description = s.execute("engine.describe", &json!({"command":"sample.make"})).unwrap();
        assert_eq!(description["input"]["properties"]["params"], schema);
        assert_eq!(description["effect"], "edit");
        assert!(s.doc.features.is_empty());
        assert!(s.execute("sample.make", &json!({"params":{}})).is_err());
        s.execute("sample.make", &json!({"approval":inspection["approval"],"params":{"targets":[]}})).unwrap();
        assert_eq!(s.undo.len(), 1);
        assert_eq!(s.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"], 24.);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.features.is_empty());
    }

    #[test]
    fn read_entries_cannot_edit_even_with_an_edit_command_grant() {
        for command in ["solid.box", "sketch.edit"] {
            let f = Fixture::new("call $box");
            if command == "sketch.edit" {
                let source = component_template()
                    .replace("      call $box\n      call $box\n", "      call $box\n")
                    .replace("solid.box", "sketch.edit")
                    .replace(r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#, r#"{\22sketch\22:\22Sketch1\22}"#)
                    .replace("i32.const 9\n", "i32.const 11\n")
                    .replace("i32.const 33\n", "i32.const 20\n");
                std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(source).unwrap()).unwrap();
            }
            let mut m = manifest();
            m["runtime"]["commands"] = json!([command]);
            m["runtime"]["entries"] = json!([{"name":"make","effect":"read","input_schema":{"type":"object"}}]);
            std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
            let mut s = Session::default();
            s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
            s.execute("sketch.finish", &json!({})).unwrap();
            s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
            let before = s.doc.to_json();
            let history = (s.undo.len(), s.redo.len());
            let error = f.run(&mut s).unwrap_err();
            assert!(
                matches!(error, EngineError::Plugin(craft_plugin::HostError::PermissionDenied { ref permission }) if permission.contains("read entry")),
                "{command}: {error}"
            );
            assert_eq!(s.doc.to_json(), before);
            assert_eq!(s.active_sketch, None);
            assert_eq!((s.undo.len(), s.redo.len()), history);
        }
    }

    #[test]
    fn accepted_plugin_replies_identify_inner_sketch_exits() {
        for command in ["sketch.finish", "sketch.cancel"] {
            for trap in [false, true] {
                let f = Fixture::new("call $box");
                let source = component_template()
                    .replace(
                        "      call $box\n      call $box\n",
                        if trap {
                            "      i32.const 2048 i32.const 11 i32.const 3072 i32.const 20 i32.const 0 call $run call $box unreachable\n"
                        } else {
                            "      i32.const 2048 i32.const 11 i32.const 3072 i32.const 20 i32.const 0 call $run call $box\n"
                        },
                    )
                    .replace("solid.box", command)
                    .replace(
                        "file.export",
                        r#"sketch.edit")
    (data (i32.const 3072) "{\22sketch\22:\22Sketch1\22}"#,
                    )
                    .replace(r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#, "{}")
                    .replace("i32.const 9\n", &format!("i32.const {}\n", command.len()))
                    .replace("i32.const 33\n", "i32.const 2\n");
                std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(source).unwrap()).unwrap();
                let mut m = manifest();
                m["runtime"]["commands"] = json!(["sketch.edit", command]);
                std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
                let mut s = Session::default();
                s.execute("sketch.create", &json!({"plane":"XY"})).unwrap();
                let open = s.active_sketch.unwrap();
                let before = s.doc.to_json();
                let result = f.run(&mut s);
                if trap {
                    assert!(result.is_err());
                    assert_eq!(s.active_sketch, Some(open));
                    assert_eq!(s.doc.to_json(), before);
                } else {
                    assert_eq!(result.unwrap()["sketch_exits"], json!([open]));
                    assert_eq!(s.active_sketch, None);
                }
            }
        }
    }

    #[test]
    fn folder_grants_apply_the_caller_scope_to_each_hidden_read_and_write() {
        for write in [false, true] {
            for name in [".env", ".hidden/secret.txt"] {
                let f = Fixture::new("call $box");
                std::fs::create_dir_all(f.0.join("sample/.hidden")).unwrap();
                let target = f.0.join("sample").join(name);
                std::fs::write(&target, b"private contents").unwrap();
                std::fs::write(f.0.join("sample/plugin.wasm"), file_component_path(write, name)).unwrap();
                let mut m = manifest();
                m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":true}]});
                std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
                let mut s = Session::default();
                s.plugin_state.file_grants.push(FileGrant { name: "project".into(), folder: f.0.join("sample"), read: true, write: true });
                let scope = solidcraft_io::vfs::PathScope::default().with_design_folder(&f.0);
                assert!(scope.check(&f.path()).is_ok());
                for scope in [None, Some(scope)] {
                    solidcraft_io::vfs::scoped(scope, || {
                        let error = f.run(&mut s).unwrap_err();
                        assert!(matches!(error, EngineError::Plugin(HostError::PermissionDenied { .. })), "{write} {name}: {error}");
                    });
                    assert_eq!(std::fs::read(&target).unwrap(), b"private contents");
                }
                let host =
                    SolidCraftHost { session: s, transaction: None, sketch_exits: Vec::new(), groups: BTreeSet::new(), file_folders: Vec::new() };
                solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([f.0.clone()])), || {
                    assert!(host.authorize_file_path(&target, write).is_ok(), "explicit scope grants hidden access");
                });
            }
        }
    }

    #[test]
    fn omitted_entry_params_are_objects_in_both_call_forms() {
        let f = Fixture::new("i32.const 0 i32.const 0 i32.store i32.const 4 local.get 2 i32.store i32.const 8 local.get 3 i32.store");
        let mut s = Session::default();
        let inspected = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        let direct = s.execute("plugin.run", &json!({"path":f.path(),"approval":inspected["approval"],"entry":"make"})).unwrap();
        let namespaced = s.execute("sample.make", &json!({"approval":inspected["approval"]})).unwrap();
        assert_eq!(direct["result"], json!({}));
        assert_eq!(direct["result"], namespaced["result"]);
    }

    #[test]
    fn metadata_only_discovery_preserves_the_explicitly_inspected_path() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("sample/plugin.json"), r#"{"id":"sample","name":"Metadata","version":"1"}"#).unwrap();
        std::fs::remove_file(f.0.join("sample/plugin.wasm")).unwrap();
        let mut s = Session::default();
        let inspected = s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap();
        assert_eq!(inspected["manifest"]["name"], "Metadata");
        assert!(read_package(&f.path()).unwrap().component.is_empty());
        s.plugin_state.register_discovered("sample".into(), f.0.join("installed/sample").to_string_lossy().into_owned());
        s.plugin_state.register_discovered("other".into(), "installed/other".into());
        assert_eq!(s.plugin_state.packages["sample"], f.path());
        assert_eq!(s.plugin_state.packages["other"], "installed/other");
        let listed = s.execute("plugin.list", &json!({})).unwrap();
        let entry = listed["plugins"].as_array().unwrap().iter().find(|p| p["manifest"]["id"] == "sample").unwrap();
        assert_eq!(entry["manifest"]["name"], "Metadata");
        assert_eq!(entry["commands"], json!([]));
        assert!(s.execute("engine.commands", &json!({})).is_ok());
        let error = s.execute("plugin.run", &json!({"path":f.path(),"approval":inspected["approval"],"entry":"make"})).unwrap_err();
        assert!(error.to_string().contains("no executable runtime"));
    }
}

#[test]
fn evaluated_mesh_transfer_is_bounded_and_does_not_edit_the_design() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":2,"width":3,"height":4,"body_name":"Box"})).unwrap();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    let result = s.execute("model.mesh", &json!({"body":"Box"})).unwrap();
    assert_eq!(result["frame"], "component-local-mm");
    let positions: Vec<[f64; 3]> = serde_json::from_value(result["positions"].clone()).unwrap();
    let triangles: Vec<[u32; 3]> = serde_json::from_value(result["triangles"].clone()).unwrap();
    assert!(!positions.is_empty() && positions.len() <= 8192);
    assert_eq!(triangles.len(), 12);
    assert!(triangles.iter().flatten().all(|i| (*i as usize) < positions.len()));
    for (axis, max) in [2., 3., 4.].into_iter().enumerate() {
        assert_eq!(positions.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min), 0.);
        assert_eq!(positions.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max), max);
        assert!(positions.iter().all(|p| p[axis].is_finite() && (0. ..=max).contains(&p[axis])));
    }
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), undo);
    assert!(s.execute("model.mesh", &json!({"body":"Box","tolerance":0})).is_err());
    assert!(s.execute("model.mesh", &json!({"body":"Missing"})).is_err());
}

#[test]
fn evaluated_mesh_transfer_refuses_an_oversized_input_without_truncation() {
    let mut doc = solidcraft_doc::Document::default();
    doc.add_feature(
        solidcraft_doc::FeatureKind::MeshImport {
            file: "budget.stl".into(),
            meshes: vec![solidcraft_doc::MeshData {
                name: "Many".into(),
                positions: (0..8193).flat_map(|i| [i as f64, (i % 2) as f64, 0.]).collect(),
                triangles: (0..2731).flat_map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect(),
                color: None,
                corner_normals: Default::default(),
            }],
        },
        Some("Budget mesh"),
    )
    .unwrap();
    doc.features.last_mut().unwrap().body_names = vec!["Many".into()];
    let mut s = Session::new(doc);
    let before = s.doc.to_json();
    assert!(s.execute("model.mesh", &json!({"body":"Many"})).unwrap_err().to_string().contains("transfer budget"));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn host_resource_policy_is_exact_user_state_and_never_design_data() {
    let defaults = HostLimits::default();
    assert_eq!((defaults.max_fuel, defaults.max_memory_mib), (100_000_000_000, 128));
    let request = Limits::default();
    assert_eq!((request.fuel, request.memory_mib, request.deadline_ms), (10_000_000, 64, 5000));
    assert!(defaults.check(&request).is_ok());
    for limits in
        [HostLimits { max_fuel: 0, ..defaults }, HostLimits { max_memory_mib: 0, ..defaults }, HostLimits { max_memory_mib: 4097, ..defaults }]
    {
        assert!(limits.validate().is_err());
    }
    let mut m = manifest();
    m["runtime"]["limits"] = json!({"fuel":u64::MAX,"memory_mib":4096,"deadline_ms":30000});
    assert!(parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").is_ok());
    let policy = HostLimits { max_fuel: u64::MAX, max_memory_mib: 4096 };
    assert_eq!(serde_json::from_str::<HostLimits>(&serde_json::to_string(&policy).unwrap()).unwrap(), policy);
    let mut s = Session { plugin_limits: policy, ..Default::default() };
    assert_eq!(s.scratch().plugin_limits, policy);
    s.execute("file.new", &json!({})).unwrap();
    assert_eq!(s.plugin_limits, policy);
    assert!(!s.doc.to_json().contains("max_fuel"));
    assert!(command_group("plugin.configure") == Some("plugin") && catalog_command("plugin.configure").is_none());
}

#[test]
fn command_effects_are_explicit_and_independent_of_undo() {
    let host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
    };
    let catalog = host.command_catalog();
    for command in ["sketch.edit", "sketch.finish", "sketch.cancel", "solid.box", "sketch.export_dxf"] {
        assert_eq!(catalog.iter().find(|c| c.name == command).unwrap().effect, Effect::Edit, "{command}");
    }
    for command in
        ["model.mesh", "model.faces", "model.edges", "document.inspect", "timeline.inspect", "inspect.measure", "sketch.inspect", "sketch.solve"]
    {
        assert_eq!(catalog.iter().find(|c| c.name == command).unwrap().effect, Effect::Read, "{command}");
    }
    // A test-only command: its id goes through a variable so the icon audit, which reads command ids
    // registered under crates/engine/src, doesn't take this fixture for a real command without an icon.
    let id = "sample.new";
    let new = crate::cmd::CommandSpec::new(id, "New", |_, _| Ok(json!({}))).noundo();
    assert_eq!(new.effect, Effect::Edit);
}

#[test]
fn granted_roots_allow_hidden_ancestors_but_exclude_hidden_descendants() {
    let root = solidcraft_io::vfs::temp_dir().join(".settings/AppData/granted");
    let mut session = Session::default();
    session.plugin_state.file_grants = vec![
        FileGrant { name: "project".into(), folder: root.clone(), read: true, write: true },
        FileGrant { name: "hidden".into(), folder: root.join(".hidden"), read: true, write: false },
    ];
    let host = SolidCraftHost { session, transaction: None, sketch_exits: Vec::new(), groups: BTreeSet::new(), file_folders: Vec::new() };
    solidcraft_io::vfs::scoped(None, || {
        for write in [false, true] {
            assert!(host.authorize_file_path(&root.join("public.txt"), write).is_ok());
            assert!(host.authorize_file_path(&root.join(".env"), write).is_err());
            assert!(host.authorize_file_path(&root.join(".hidden/.env"), write).is_err());
            assert!(host.authorize_file_path(&root.parent().unwrap().join("sibling/public.txt"), write).is_err());
        }
        assert!(host.authorize_file_path(&root.join(".hidden/public.txt"), false).is_ok());
        assert!(host.authorize_file_path(&root.join(".hidden/public.txt"), true).is_err());
    });
}

mod install_zip {
    use super::*;
    use crate::plugin::install::{InstallError, unpack};
    use solidcraft_io::safe_zip;

    fn manifest_bytes(id: &str, capabilities: Value) -> Vec<u8> {
        let mut m = manifest();
        m["id"] = json!(id);
        m["name"] = json!("Gears");
        m["capabilities"] = capabilities;
        serde_json::to_vec(&m).unwrap()
    }
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        safe_zip::write(files).unwrap()
    }
    fn gears(version: &str) -> Vec<u8> {
        let mut m: Value = serde_json::from_slice(&manifest_bytes("gears", json!(["commands@0.1"]))).unwrap();
        m["version"] = json!(version);
        zip(&[("gears-0.1.0/plugin.json", &serde_json::to_vec(&m).unwrap()[..]), ("gears-0.1.0/plugin.wasm", b"\0asm"), ("__MACOSX/._x", b"")])
    }
    /// A session whose plugins folder is a fresh temporary one, and the folder of the zip files.
    fn session(name: &str) -> (Session, std::path::PathBuf) {
        let root = solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-install-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("zips")).unwrap();
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(root.join("plugins"));
        (s, root)
    }
    fn put(root: &Path, name: &str, bytes: &[u8]) -> String {
        let path = root.join("zips").join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_valid_zip_installs_and_its_entries_resolve() {
        let (mut s, root) = session("valid");
        let path = put(&root, "gears.zip", &gears("0.1.0"));
        let dry = s.execute("plugin.install", &json!({"path": path, "dry_run": true})).unwrap();
        assert_eq!((dry["installed"].clone(), root.join("plugins/gears").exists()), (json!(false), false));
        let reply = s.execute("plugin.install", &json!({"path": path})).unwrap();
        assert_eq!(reply["id"], "gears");
        assert_eq!(reply["name"], "Gears");
        assert_eq!(reply["version"], "0.1.0");
        assert_eq!(reply["capabilities"], json!(["commands@0.1"]));
        assert_eq!(reply["entries"], json!(["gears.make"]));
        assert_eq!(reply["commands"], json!(["solid.box"]));
        assert!(reply["permissions"].is_object() && reply.get("replaces").is_none());
        assert_eq!(std::fs::read(root.join("plugins/gears/plugin.wasm")).unwrap(), b"\0asm");
        assert!(!root.join("plugins/gears/__MACOSX").exists());
        // Installed means loaded: the package reads back and plugin.list shows it.
        assert!(read_package(&root.join("plugins/gears").to_string_lossy()).is_ok());
        let list = s.execute("plugin.list", &json!({})).unwrap();
        assert_eq!(list["plugins"][0]["commands"], json!(["gears.make"]));
        let only = std::fs::read_dir(root.join("plugins")).unwrap().count();
        assert_eq!(only, 1, "no staging folders are left behind");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn replacing_an_installed_plugin_needs_replace() {
        let (mut s, root) = session("replace");
        s.execute("plugin.install", &json!({"path": put(&root, "a.zip", &gears("0.1.0"))})).unwrap();
        let newer = put(&root, "b.zip", &gears("0.2.0"));
        let dry = s.execute("plugin.install", &json!({"path": newer, "dry_run": true})).unwrap();
        assert_eq!(dry["replaces"], "Gears 0.1.0");
        let refused = s.execute("plugin.install", &json!({"path": newer})).unwrap_err().to_string();
        assert!(refused.contains("already installed"), "{refused}");
        let dir = root.join("plugins");
        let error = crate::plugin::install::install(&unpack(&gears("0.2.0")).unwrap(), &dir, false).unwrap_err();
        assert_eq!(error, InstallError::AlreadyInstalled { id: "gears".into(), name: "Gears".into(), version: "0.1.0".into() });
        assert_eq!(read_package(&dir.join("gears").to_string_lossy()).unwrap().manifest.version, "0.1.0", "kept as it was");
        s.execute("plugin.install", &json!({"path": newer, "replace": true})).unwrap();
        assert_eq!(read_package(&dir.join("gears").to_string_lossy()).unwrap().manifest.version, "0.2.0");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "the old folder is gone");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unsafe_oversized_and_incomplete_zips_are_refused_before_anything_is_written() {
        let good_bytes = manifest_bytes("gears", json!(["commands@0.1"]));
        let good: &[u8] = &good_bytes;
        let slip = unpack(&zip(&[("plugin.json", good), ("plugin.wasm", b"x"), ("../evil", b"x")])).unwrap_err();
        assert!(matches!(slip, InstallError::Archive(safe_zip::Error::UnsafeEntry { .. })), "{slip}");
        let absolute = unpack(&zip(&[("/plugin.json", good)])).unwrap_err();
        assert!(matches!(absolute, InstallError::Archive(safe_zip::Error::UnsafeEntry { .. })), "{absolute}");
        let huge = vec![0u8; MAX_COMPONENT_BYTES + 1];
        let bomb = unpack(&zip(&[("plugin.json", good), ("plugin.wasm", &huge[..])])).unwrap_err();
        assert!(matches!(bomb, InstallError::Archive(safe_zip::Error::EntryTooLarge { .. })), "{bomb}");
        let chunk = vec![0u8; 15 << 20];
        let many: Vec<String> = (0..5).map(|i| format!("data/{i}.bin")).collect();
        let mut files: Vec<(&str, &[u8])> = vec![("plugin.json", good), ("plugin.wasm", b"x")];
        files.extend(many.iter().map(|n| (n.as_str(), chunk.as_slice())));
        assert_eq!(unpack(&zip(&files)).unwrap_err(), InstallError::Archive(safe_zip::Error::TooLarge { limit: 64 << 20 }));
        assert_eq!(unpack(&zip(&[("plugin.wasm", b"x")])).unwrap_err(), InstallError::MissingManifest);
        assert_eq!(unpack(&zip(&[("plugin.json", good)])).unwrap_err(), InstallError::MissingModule("plugin.wasm".into()));
        let two = zip(&[("a/plugin.json", good), ("a/plugin.wasm", b"x"), ("b/plugin.json", good), ("b/plugin.wasm", b"x")]);
        assert_eq!(unpack(&two).unwrap_err(), InstallError::NotOnePlugin);
        let stray = zip(&[("a/plugin.json", good), ("a/plugin.wasm", b"x"), ("other.txt", b"x")]);
        assert_eq!(unpack(&stray).unwrap_err(), InstallError::NotOnePlugin);
        let deep = zip(&[("a/b/plugin.json", good), ("a/b/plugin.wasm", b"x")]);
        assert_eq!(unpack(&deep).unwrap_err(), InstallError::NotOnePlugin);
        let newer = unpack(&zip(&[("plugin.json", &manifest_bytes("gears", json!(["commands@0.2"]))[..]), ("plugin.wasm", b"x")])).unwrap_err();
        assert!(matches!(&newer, InstallError::Manifest(HostError::UnsupportedCapability { .. })), "{newer}");
        assert_eq!(newer.to_string(), "needs commands 0.2; update SolidCraft or the plugin");
        // Through the command: refused, and the plugins folder is never created.
        let (mut s, root) = session("refused");
        let path = put(&root, "slip.zip", &zip(&[("plugin.json", good), ("plugin.wasm", b"x"), ("../evil", b"x")]));
        assert!(s.execute("plugin.install", &json!({"path": path})).is_err());
        let path = put(&root, "newer.zip", &zip(&[("plugin.json", &manifest_bytes("gears", json!(["commands@0.2"]))[..]), ("plugin.wasm", b"x")]));
        let error = s.execute("plugin.install", &json!({"path": path})).unwrap_err();
        assert!(matches!(error, crate::EngineError::Plugin(HostError::UnsupportedCapability { .. })), "{error}");
        assert!(!root.join("plugins").exists() && !root.join("evil").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
