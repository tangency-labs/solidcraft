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
    assert_eq!(MAX_MANIFEST_BYTES, 64 * 1024);
    assert!(parse_manifest(&vec![b' '; MAX_MANIFEST_BYTES + 1], "sample").is_err());
}
#[test]
fn plugin_entries_cannot_shadow_host_commands() {
    let mut m = manifest();
    m["id"] = json!("solid");
    m["runtime"]["entries"] = json!(["box"]);
    let error = parse_manifest(&serde_json::to_vec(&m).unwrap(), "solid").unwrap_err();
    assert!(matches!(&error, EngineError::Plugin(HostError::InvalidManifest { field, .. }) if field == "runtime.entries"), "{error}");
    assert!(error.to_string().contains("solid.box"));
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
fn an_importer_checks_its_own_design_apart_from_the_open_one() {
    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
    };
    let mut made = Session::new(solidcraft_doc::Document::new("Bracket"));
    made.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    host.evaluate_open(&solidcraft_io::write_design(&made.doc)).unwrap();
    let measured = host.evaluate_run("inspect.measure", &json!({})).unwrap();
    assert_eq!(measured["bodies"].as_array().map(Vec::len), Some(1), "{measured}");
    assert!(host.evaluate_run("document.inspect", &json!({})).unwrap()["timeline"].is_array());
    // A command that fails is an answer; a command outside the list is refused.
    let missing = host.evaluate_run("model.edges", &json!({"body":"No such body"})).unwrap();
    assert!(missing["error"].is_string(), "{missing}");
    for refused in ["solid.box", "file.export_stl", "doc.open", "plugin.install"] {
        assert!(matches!(host.evaluate_run(refused, &json!({})), Err(HostError::PermissionDenied { .. })), "{refused}");
    }
    assert!(host.session.doc.features.is_empty(), "the open design is untouched");
    host.evaluate_close();
    assert!(host.evaluate_run("inspect.measure", &json!({})).is_err(), "nothing stays open");
    assert!(matches!(host.evaluate_open(b"not a design"), Err(HostError::InvalidInput { .. })));
}

#[test]
fn every_built_command_is_reachable_and_sensitive_ones_need_their_group() {
    let host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
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
/// A read-only folder must pass the importer's scope check without granting write access.
/// Both the user's grant and the package's manifest must allow the intended operation.
#[test]
fn file_scopes_require_separate_read_and_write_approvals() {
    let folder = solidcraft_io::vfs::temp_dir().join("sc-readonly-grant-18793");
    let approved = vec![FileGrant { name: "data".into(), folder: folder.clone(), read: true, write: false }];
    let manifest = vec![craft_plugin::FilePermission { name: "data".into(), read: true, write: false }];
    assert_eq!(approved_file_folders(&approved, &manifest, false), vec![folder.clone()]);
    assert!(approved_file_folders(&approved, &manifest, true).is_empty());

    let mut more_grant = approved.clone();
    more_grant[0].write = true;
    assert!(approved_file_folders(&more_grant, &manifest, true).is_empty(), "a grant cannot override a read-only manifest");
    let mut more_manifest = manifest.clone();
    more_manifest[0].write = true;
    assert!(approved_file_folders(&approved, &more_manifest, true).is_empty(), "the manifest cannot override read-only approval");
    assert_eq!(approved_file_folders(&more_grant, &more_manifest, true), vec![folder.clone()]);
    assert!(approved_file_folders(&more_grant, &[], false).is_empty(), "a missing manifest declaration confers no access");
    assert!(approved_file_folders(&[], &more_manifest, false).is_empty(), "a missing user grant confers no access");

    for command in ["file.insert_step", "file.insert_mesh", "file.insert_gerber", "file.gerber_files", "sketch.insert_dxf", "sketch.insert_svg"] {
        assert!(input_only_command(command), "{command} reads its supplied file");
    }
    for command in ["file.save_as", "file.copy", "file.move", "file.trash", "file.export_step", "doc.open", "doc.recovery_discard", "library.export"] {
        assert!(!input_only_command(command), "{command} needs the stricter write-capable scope");
    }
}

/// Importing a STEP may change the design without modifying its source file. Even after this
/// read-only command succeeds, write commands must not gain access to the same folder.
#[test]
fn read_only_folder_allows_step_insert_but_refuses_file_writes() {
    let root = solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-read-scope-{}", std::process::id()));
    let read = root.join("read");
    std::fs::create_dir_all(&read).unwrap();
    let source_path = read.join("source.step");
    let mut source = Session::default();
    source.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    source.execute("file.export_step", &json!({"path":source_path})).unwrap();
    assert!(source_path.is_file());

    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::from(["file".into()]),
        file_folders: Vec::new(),
        file_read_folders: vec![read.clone()],
        evaluation: None,
    };
    host.begin_transaction().unwrap();
    let inserted = host.run_command("file.insert_step", &json!({"path":source_path}));
    assert!(inserted.is_ok(), "read-only insert should work: {inserted:?}");
    let output = read.join("forbidden.sc");
    let write = host.run_command("file.save_as", &json!({"path":output}));
    assert!(matches!(&write, Err(HostError::CommandFailed { .. })), "the same grant cannot save files: {write:?}");
    assert!(!output.exists(), "no file was written under a read-only grant");
    let output = read.join("forbidden.stl");
    assert!(host.run_command("file.export_stl", &json!({"path":output})).is_err(), "export also requires write authority");
    assert!(!output.exists());
    assert!(source_path.exists(), "the imported STEP is unchanged");
    host.rollback_transaction().unwrap();
    std::fs::remove_dir_all(root).unwrap();
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
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
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
/// A plugin reaches a command only by its exact registered id: another
/// case, whitespace around it or a look-alike letter is not a granted command, so it can't slip
/// past a command group, nor a manifest list a command under another spelling.
#[test]
fn a_command_in_another_spelling_is_never_granted() {
    let mut host = SolidCraftHost {
        session: Session::default(),
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
    };
    host.begin_transaction().unwrap();
    for id in ["file.save_as", "file.export_step", "doc.open", "plugin.run", "plugin.list", "settings.get", "diagnostics.crash_reports", "solid.box"]
    {
        let mixed: String = id.chars().enumerate().map(|(i, c)| if i % 2 == 0 { c.to_ascii_uppercase() } else { c }).collect();
        for spelled in [
            id.to_ascii_uppercase(),
            mixed,
            format!(" {id}"),
            format!("{id}\n"),
            format!("\u{a0}{id}"),
            id.replacen('o', "\u{043e}", 1),
            id.replacen('.', "\u{ff0e}", 1),
            format!("\u{200b}{id}"),
        ]
        .into_iter()
        .filter(|spelled| spelled != id)
        {
            let refused = host.run_command(&spelled, &json!({"path": "x.sc", "length": 1, "width": 1, "height": 1}));
            assert!(
                matches!(&refused, Err(HostError::PermissionDenied { permission }) if permission.contains("is not granted")),
                "{spelled:?}: {refused:?}"
            );
            let mut m = manifest();
            m["runtime"]["commands"] = json!([spelled.clone()]);
            m["runtime"]["permissions"] = json!({"command_groups": ["app", "doc", "file", "plugin"]});
            assert!(parse_manifest(&serde_json::to_vec(&m).unwrap(), "sample").is_err(), "manifest lists {spelled:?}");
        }
    }
    let catalog = host.command_catalog();
    assert!(catalog.iter().all(|c| crate::find_command(&c.name).is_some_and(|spec| spec.id == c.name)), "the catalog lists registered ids");
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
        fn install(&self, s: &mut Session, approve: Value) -> String {
            let bytes = solidcraft_io::safe_zip::write(&[
                ("plugin.json", &std::fs::read(self.0.join("sample/plugin.json")).unwrap()),
                ("plugin.wasm", &std::fs::read(self.0.join("sample/plugin.wasm")).unwrap()),
            ])
            .unwrap();
            let zip = self.0.join("package.zip");
            std::fs::write(&zip, bytes).unwrap();
            s.plugin_state.plugins_dir = Some(self.0.join("installed"));
            s.execute("plugin.install", &json!({"path":zip,"replace":true,"approve":approve})).unwrap()["folder"].as_str().unwrap().into()
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
                s.plugin_state.grants.insert(
                    "sample".into(),
                    Grants {
                        package_hash: Some(read_package(&f.path()).unwrap().approval),
                        command_groups: BTreeSet::new(),
                        folders: s.plugin_state.file_grants.clone(),
                        memory_mib: None,
                        input_folder: false,
                    },
                );
                let scope = solidcraft_io::vfs::PathScope::default().with_design_folder(&f.0);
                assert!(scope.check(&f.path()).is_ok());
                for scope in [None, Some(scope)] {
                    solidcraft_io::vfs::scoped(scope, || {
                        let error = f.run(&mut s).unwrap_err();
                        assert!(matches!(error, EngineError::Plugin(HostError::PermissionDenied { .. })), "{write} {name}: {error}");
                    });
                    assert_eq!(std::fs::read(&target).unwrap(), b"private contents");
                }
                let host = SolidCraftHost {
                    session: s,
                    transaction: None,
                    sketch_exits: Vec::new(),
                    view: None,
                    groups: BTreeSet::new(),
                    file_folders: Vec::new(),
                    file_read_folders: Vec::new(),
                    evaluation: None,
                };
                solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([f.0.clone()])), || {
                    assert!(host.authorize_file_path(&target, write).is_ok(), "explicit scope grants hidden access");
                });
            }
        }
    }

    #[test]
    fn hand_installed_plugins_get_no_legacy_or_other_plugins_folder_grants() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("sample/plugin.wasm"), file_component(false)).unwrap();
        std::fs::write(f.0.join("sample/secret.txt"), "test data").unwrap();
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":false}]});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let grant = FileGrant { name: "project".into(), folder: f.0.join("sample"), read: true, write: false };
        s.plugin_state.file_grants.push(grant.clone());
        s.plugin_state.grants.insert("another-plugin".into(), Grants { folders: vec![grant], ..Default::default() });
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(error, EngineError::Plugin(HostError::PermissionDenied { .. })), "{error}");
        assert!(s.doc.features.is_empty());
    }

    #[test]
    fn only_the_running_plugins_stored_command_groups_authorize_sensitive_calls() {
        let f = Fixture::new("call $box");
        let command = "settings.profile.list";
        let source = component_template();
        let arguments = r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#;
        assert!(source.contains(arguments));
        let source = source
            .replace("\"solid.box\"", &format!("\"{command}\""))
            .replace(
                "i32.const 9\n      i32.const 1024\n      i32.const 33",
                &format!("i32.const {}\n      i32.const 1024\n      i32.const 2", command.len()),
            )
            .replace(arguments, "{}");
        std::fs::write(f.0.join("sample/plugin.wasm"), wat::parse_str(&source).unwrap()).unwrap();
        let mut m = manifest();
        m["runtime"]["commands"] = json!([command]);
        m["runtime"]["permissions"] = json!({"command_groups":["app"]});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        s.plugin_state.command_groups.insert("app".into());
        s.plugin_state.grants.insert("another-plugin".into(), Grants { command_groups: BTreeSet::from(["app".into()]), ..Default::default() });
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(&error, EngineError::Plugin(HostError::PermissionDenied { permission }) if permission.contains("app")), "{error}");
        let bytes =
            solidcraft_io::safe_zip::write(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", &wat::parse_str(&source).unwrap())])
                .unwrap();
        let zip = f.0.join("package.zip");
        std::fs::write(&zip, bytes).unwrap();
        s.plugin_state.plugins_dir = Some(f.0.join("installed"));
        s.execute("plugin.install", &json!({"path":zip,"approve":{"command_groups":["app"]}})).unwrap();
        assert_eq!(f.run(&mut s).unwrap()["result"], json!({}));
        s.plugin_state.grants.remove("sample");
        assert!(matches!(f.run(&mut s), Err(EngineError::Plugin(HostError::PermissionDenied { .. }))));
        assert!(s.doc.features.is_empty());
    }

    /// `plugin.check` finds what Install and a call would refuse, with a fix for each, and runs
    /// no plugin code: the fixture's component would trap if it ran.
    #[test]
    fn check_reports_each_refusal_with_its_fix_and_runs_nothing() {
        let f = Fixture::new("unreachable");
        let mut s = Session::default();
        let check = |s: &mut Session, path: &str| s.execute("plugin.check", &json!({"path":path})).unwrap();
        let good = check(&mut s, &f.path());
        assert_eq!((good["ok"].as_bool(), good["kind"].as_str()), (Some(true), Some("command")), "{good}");
        assert_eq!(good["problems"], json!([]));
        assert_eq!(good["approval"], s.execute("plugin.inspect", &json!({"path":f.path()})).unwrap()["approval"], "one hash for one package");
        // The package as it ships: a .zip, checked by the installer's own reader.
        let component = std::fs::read(f.0.join("sample/plugin.wasm")).unwrap();
        let zip = |manifest: &Value, component: &[u8]| {
            let path = f.0.join("package.zip");
            let manifest = serde_json::to_vec(manifest).unwrap();
            std::fs::write(&path, solidcraft_io::safe_zip::write(&[("plugin.json", &manifest), ("plugin.wasm", component)]).unwrap()).unwrap();
            path.to_string_lossy().into_owned()
        };
        let zipped = check(&mut s, &zip(&manifest(), &component));
        assert_eq!((zipped["ok"].as_bool(), &zipped["approval"]), (Some(true), &good["approval"]));
        // Hidden files and the tests folder stay out of a package folder's check.
        std::fs::create_dir_all(f.0.join("sample/tests")).unwrap();
        std::fs::write(f.0.join("sample/tests/big.bin"), vec![0u8; 17 << 20]).unwrap();
        std::fs::write(f.0.join("sample/.notes"), b"x").unwrap();
        assert_eq!(check(&mut s, &f.path())["problems"], json!([]));
        let fields = |reply: &Value| -> Vec<(String, String)> {
            reply["problems"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p["severity"].as_str().unwrap().into(), p["field"].as_str().unwrap().into()))
                .collect()
        };
        for (mutate, field) in [
            ((|m: &mut Value| m["runtime"]["permissions"] = json!({"net": true})) as fn(&mut Value), "runtime.permissions.net"),
            (|m: &mut Value| m["id"] = json!("Sample"), "id"),
            (|m: &mut Value| m["capabilities"] = json!(["commands@0.2"]), "capabilities"),
            (|m: &mut Value| m["runtime"]["commands"] = json!(["file.export"]), "runtime.commands"),
            (|m: &mut Value| m["runtime"]["limits"]["deadline_ms"] = json!(60_000), "runtime.limits.deadline_ms"),
            (|m: &mut Value| m["runtime"]["limits"]["fuel"] = json!(200_000_000_000_u64), "runtime.limits.fuel"),
            (|m: &mut Value| m["runtime"]["module"] = json!("missing.wasm"), "runtime.module"),
            (|m: &mut Value| m["surprise"] = json!(true), "manifest"),
        ] {
            let mut m = manifest();
            mutate(&mut m);
            let reply = check(&mut s, &zip(&m, &component));
            assert_eq!(reply["ok"], false, "{m}");
            assert_eq!(fields(&reply).first(), Some(&("error".to_string(), field.to_string())), "{reply}");
            assert!(reply["problems"].as_array().unwrap().iter().all(|p| !p["fix"].as_str().unwrap().is_empty()), "{reply}");
        }
        // Memory over the ceiling installs once the user approves it: a warning.
        let mut m = manifest();
        m["runtime"]["limits"]["memory_mib"] = json!(512);
        let reply = check(&mut s, &zip(&m, &component));
        assert_eq!((reply["ok"].as_bool(), fields(&reply)), (Some(true), vec![("warning".to_string(), "runtime.limits.memory_mib".to_string())]));
        // A core module, and a command component declared as an importer, do not link.
        let core = wat::parse_str("(module)").unwrap();
        let reply = check(&mut s, &zip(&manifest(), &core));
        assert!(reply["problems"][0]["fix"].as_str().unwrap().contains("wasm-tools component new"), "{reply}");
        let importer = json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.1"],"runtime":{"module":"plugin.wasm",
            "commands":[],"entries":[{"name":"convert","kind":"importer","extensions":["smp"]}]}});
        assert_eq!(fields(&check(&mut s, &zip(&importer, &component))), vec![("error".to_string(), "component".to_string())]);
        // A folder named apart from its id still checks, with a note.
        std::fs::rename(f.0.join("sample"), f.0.join("sample-src")).unwrap();
        let renamed = check(&mut s, &f.0.join("sample-src").to_string_lossy());
        assert_eq!((renamed["ok"].as_bool(), fields(&renamed)), (Some(true), vec![("warning".to_string(), "id".to_string())]));
        std::fs::rename(f.0.join("sample-src"), f.0.join("sample")).unwrap();
        assert!(s.doc.features.is_empty() && s.undo.is_empty(), "checking changes nothing");
        assert!(!f.0.join("installed").exists(), "and installs nothing");
    }

    #[test]
    fn installed_plugins_run_with_only_the_approved_folders() {
        let f = Fixture::new("call $box");
        std::fs::write(f.0.join("secret.txt"), "{}").unwrap();
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":false}]});
        let bytes =
            solidcraft_io::safe_zip::write(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", &file_component(false))]).unwrap();
        let zip = f.0.join("package.zip");
        std::fs::write(&zip, bytes).unwrap();
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(f.0.join("installed"));
        let installed = s.execute("plugin.install", &json!({"path":zip})).unwrap();
        let path = installed["folder"].clone();
        let approval = s.execute("plugin.inspect", &json!({"path":path})).unwrap()["approval"].clone();
        let params = json!({"path":path,"approval":approval,"entry":"make"});
        assert!(matches!(s.execute("plugin.run", &params), Err(EngineError::Plugin(HostError::PermissionDenied { .. }))));
        s.execute(
            "plugin.install",
            &json!({"path":zip,"replace":true,"approve":{"folders":[{"name":"project","path":f.0,"read":true,"write":false}]}}),
        )
        .unwrap();
        assert_eq!(s.plugin_state.grants["sample"].folders[0].folder, f.0);
        assert!(s.execute("plugin.run", &params).is_ok());
        s.execute("plugin.remove", &json!({"id":"sample"})).unwrap();
        assert!(!s.plugin_state.grants.contains_key("sample"));
        assert!(s.execute("plugin.run", &params).is_err());
    }

    #[test]
    fn stored_grants_refuse_same_id_packages_with_different_manifest_or_component() {
        let f = Fixture::new("call $box");
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"command_groups":["app"],"files":[{"name":"project","read":true,"write":true}]});
        std::fs::write(f.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let mut s = Session::default();
        let installed = f.install(&mut s, json!({"command_groups":["app"],"folders":[{"name":"project","path":f.0,"read":true,"write":true}]}));
        let trusted = read_package(&installed).unwrap();
        assert_eq!(s.plugin_state.grants["sample"].package_hash.as_deref(), Some(trusted.approval.as_str()));
        let other = Fixture::new("call $box");
        m["version"] = json!("0.2.0");
        std::fs::write(other.0.join("sample/plugin.json"), m.to_string()).unwrap();
        let grants = s.plugin_state.grants.clone();
        assert!(grants["sample"].command_groups.contains("app"));
        assert_eq!(grants["sample"].folders.len(), 1);
        for change_component in [false, true] {
            if change_component {
                std::fs::copy(f.0.join("sample/plugin.json"), other.0.join("sample/plugin.json")).unwrap();
                std::fs::write(other.0.join("sample/plugin.wasm"), component("call $box call $box")).unwrap();
            }
            let error = other.run(&mut s).unwrap_err();
            assert!(
                matches!(&error, EngineError::Plugin(HostError::PermissionDenied { permission }) if permission.contains("re-approval")),
                "{error}"
            );
            assert!(s.doc.features.is_empty(), "refuse before even non-sensitive plugin edits");
            assert!(s.plugin_state.file_grants.is_empty() && s.plugin_state.command_groups.is_empty());
            assert_eq!(s.plugin_state.grants, grants, "inspection cannot change approved grants");
        }
        s.execute("plugin.run", &json!({"path":installed,"approval":trusted.approval,"entry":"make"})).unwrap();
        assert_eq!(s.doc.features.len(), 1, "the installed package still runs");
    }

    #[test]
    fn stored_grants_require_reapproval_after_installed_package_changes() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let installed = f.install(&mut s, json!({}));
        let old_hash = s.plugin_state.grants["sample"].package_hash.clone();
        let mut m = manifest();
        m["version"] = json!("0.2.0");
        for path in [f.0.join("sample/plugin.json"), Path::new(&installed).join("plugin.json")] {
            std::fs::write(path, m.to_string()).unwrap();
        }
        let inspected = s.execute("plugin.inspect", &json!({"path":installed})).unwrap();
        let args = json!({"path":installed,"approval":inspected["approval"],"entry":"make"});
        let error = s.execute("plugin.run", &args).unwrap_err();
        assert!(matches!(&error, EngineError::Plugin(HostError::PermissionDenied { permission }) if permission.contains("re-approval")), "{error}");
        assert!(s.doc.features.is_empty());
        assert_eq!(s.plugin_state.grants["sample"].package_hash, old_hash);
        f.install(&mut s, json!({}));
        assert_ne!(s.plugin_state.grants["sample"].package_hash, old_hash);
        s.execute("plugin.run", &args).unwrap();
        assert_eq!(s.doc.features.len(), 1);
    }

    #[test]
    fn legacy_stored_grants_require_reapproval_instead_of_trusting_an_inspection() {
        let f = Fixture::new("call $box");
        let mut s = Session::default();
        let legacy: Grants = serde_json::from_value(json!({"command_groups":["app"],"folders":[]})).unwrap();
        assert_eq!(legacy.package_hash, None);
        s.plugin_state.grants.insert("sample".into(), legacy.clone());
        let error = f.run(&mut s).unwrap_err();
        assert!(matches!(&error, EngineError::Plugin(HostError::PermissionDenied { permission }) if permission.contains("re-approval")), "{error}");
        assert_eq!(s.plugin_state.grants["sample"], legacy);
        assert!(s.doc.features.is_empty() && s.plugin_state.file_grants.is_empty());
        f.install(&mut s, json!({}));
        f.run(&mut s).unwrap();
        assert!(s.plugin_state.grants["sample"].command_groups.is_empty());
        assert_eq!(s.doc.features.len(), 1);
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
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
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
    let host = SolidCraftHost {
        session,
        transaction: None,
        sketch_exits: Vec::new(),
        view: None,
        groups: BTreeSet::new(),
        file_folders: Vec::new(),
        file_read_folders: Vec::new(),
        evaluation: None,
    };
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

/// Importer plugins: an installed importer opens the files it claims as new, unsaved designs.
#[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod importers {
    use super::*;

    fn importer_manifest(id: &str, extensions: &[&str], input_folder: bool) -> Value {
        json!({"id":id,"name":"Sample Importer","version":"1","capabilities":["formats@0.1"],"runtime":{
            "module":"importer.wasm","commands":[],"entries":[{"name":"convert","kind":"importer","extensions":extensions}],
            "limits":{"fuel":10_000_000,"memory_mib":2,"deadline_ms":5000,"stack_kib":16384},"permissions":{"input_folder":input_folder}}})
    }
    /// A session whose plugins folder holds the original importer fixture as `id`, and a folder of
    /// files to open. The fixture copies the start of the file it imports into the design.
    fn installed(name: &str, plugins: &[(&str, &[&str], bool)]) -> (Session, std::path::PathBuf) {
        let root = solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-importer-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (package, version) = API.split_once('@').unwrap();
        let component = wat::parse_str(include_str!("importer_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap();
        for (id, extensions, input_folder) in plugins {
            let dir = root.join("plugins").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("plugin.json"), importer_manifest(id, extensions, *input_folder).to_string()).unwrap();
            std::fs::write(dir.join("importer.wasm"), &component).unwrap();
        }
        std::fs::create_dir_all(root.join("files")).unwrap();
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(root.join("plugins"));
        (s, root)
    }
    /// A small design's file bytes: what the fixture hands back as the imported design.
    fn design() -> Vec<u8> {
        let mut made = Session::new(solidcraft_doc::Document::new("Bracket"));
        made.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
        solidcraft_io::write_design(&made.doc)
    }
    fn put(root: &Path, name: &str, bytes: &[u8]) -> String {
        let path = root.join("files").join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_claimed_file_opens_as_a_new_unsaved_design_with_its_loss_report() {
        let (mut s, root) = installed("open", &[("smp", &["smp", "stl"], false)]);
        let listed = s.execute("plugin.list", &json!({})).unwrap();
        assert_eq!(listed["importers"][0]["id"], "smp.convert");
        assert_eq!(listed["plugins"][0]["commands"], json!([]), "an importer is not a command");
        assert_eq!(listed["plugins"][0]["manifest"]["runtime"]["entries"][0]["effect"], "open", "it opens a new design, it does not edit");
        let opened = s.execute("doc.open", &json!({"path": put(&root, "bracket.smp", &design())})).unwrap();
        assert_eq!(
            (opened["format"].as_str(), opened["plugin"].as_str(), opened["bodies"].as_u64()),
            (Some("smp.convert"), Some("Sample Importer"), Some(1))
        );
        assert_eq!(opened["loss_report"]["summary"], "Copied the file");
        assert_eq!(opened["loss_report"]["items"][0]["severity"], "dropped");
        assert_eq!(s.doc.name, "Bracket");
        assert!(s.path.is_none() && s.is_dirty() && s.undo.is_empty(), "a new design, not saved anywhere yet");
        // SolidCraft reads .stl itself: doc.open keeps its own reader, file.import uses the plugin.
        let stl = put(&root, "part.stl", &design());
        assert!(s.execute("doc.open", &json!({"path": stl})).map_or(true, |r| r.get("loss_report").is_none()));
        assert_eq!(s.execute("file.import", &json!({"path": stl})).unwrap()["format"], "smp.convert");
        assert_eq!(s.execute("doc.open", &json!({"path": stl, "format": "smp.convert"})).unwrap()["bodies"], 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_trap_fails_that_one_import_and_a_disabled_or_ambiguous_importer_opens_nothing() {
        let (mut s, root) = installed("trap", &[("smp", &["smp"], false)]);
        s.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
        let before = s.doc.to_json();
        let failure = s.execute("doc.open", &json!({"path": put(&root, "broken.smp", b"T")})).unwrap_err().to_string();
        assert!(failure.contains("Sample Importer failed on broken.smp") && failure.contains("UnreachableCodeReached"), "{failure}");
        assert_eq!(s.doc.to_json(), before, "the open design is untouched");
        assert_eq!(s.execute("doc.open", &json!({"path": put(&root, "good.smp", &design())})).unwrap()["bodies"], 1);
        s.disabled_plugins.insert("smp".into());
        assert!(s.execute("file.import", &json!({"path": put(&root, "off.smp", &design())})).is_err());
        let (mut two, root2) = installed("two", &[("smp", &["smp"], false), ("other", &["smp"], false)]);
        let ambiguous = two.execute("doc.open", &json!({"path": put(&root2, "x.smp", &design())})).unwrap_err().to_string();
        assert!(ambiguous.contains("other.convert, smp.convert"), "{ambiguous}");
        assert!(two.execute("doc.open", &json!({"path": put(&root2, "x.smp", &design()), "format": "other.convert"})).is_ok());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(root2).unwrap();
    }

    #[test]
    fn an_assembly_reads_the_files_beside_it_only_when_asked_and_granted_at_install() {
        for (asked, granted) in [(false, false), (true, false), (true, true)] {
            let (mut s, root) = installed(&format!("folder-{asked}-{granted}"), &[("smp", &["smp"], asked)]);
            let package = read_package(&root.join("plugins/smp").to_string_lossy()).unwrap();
            if granted {
                s.plugin_state
                    .grants
                    .insert("smp".into(), Grants { package_hash: Some(package.approval.clone()), input_folder: true, ..Default::default() });
            }
            put(&root, "part.bin", &design());
            let reply = s.execute("doc.open", &json!({"path": put(&root, "assembly.smp", b"S")}));
            match granted {
                false => assert!(reply.unwrap_err().to_string().contains("input_folder"), "asked {asked}"),
                true => assert_eq!(reply.unwrap()["bodies"], 1),
            }
            // Approving the folder for an importer that doesn't ask for it is refused.
            let approval = Grants { input_folder: true, ..Default::default() };
            assert_eq!(validate_grants(&package.manifest, &approval).is_ok(), asked);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    /// A session that keeps its grants in the store of `plugins`, as the command line's do.
    fn stored_session(plugins: &Path) -> Session {
        let mut s = Session::default();
        s.plugin_state.plugins_dir = Some(plugins.to_path_buf());
        s.plugin_state.stored = Some(Default::default());
        s
    }

    #[test]
    fn an_import_uses_approvals_from_an_earlier_session_until_the_component_changes_or_is_revoked() {
        let (_, root) = installed("stored", &[]);
        let plugins = root.join("plugins");
        let (package, version) = API.split_once('@').unwrap();
        let component = wat::parse_str(include_str!("importer_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap();
        let manifest = importer_manifest("smp", &["smp"], true).to_string();
        let zip = |component: &[u8]| solidcraft_io::safe_zip::write(&[("plugin.json", manifest.as_bytes()), ("importer.wasm", component)]).unwrap();
        let original = put(&root, "smp.zip", &zip(&component));
        put(&root, "part.bin", &design());
        let assembly = put(&root, "assembly.smp", b"S");
        let open = |s: &mut Session| s.execute("doc.open", &json!({"path": assembly}));

        // Installed and approved in one session, used by the next.
        stored_session(&plugins).execute("plugin.install", &json!({"path": original, "approve": {"input_folder": true}})).unwrap();
        assert!(plugins.join(".grants/smp.json").is_file());
        let mut later = stored_session(&plugins);
        assert_eq!(open(&mut later).unwrap()["bodies"], 1);
        let listed = later.execute("plugin.grants", &json!({})).unwrap();
        assert_eq!((listed["grants"][0]["id"].as_str(), listed["grants"][0]["status"].as_str()), (Some("smp"), Some("current")));
        assert_eq!(listed["grants"][0]["input_folder"], true);
        // A session that does not keep grants between runs gets nothing from the store.
        let mut session_only = Session::default();
        session_only.plugin_state.plugins_dir = Some(plugins.clone());
        assert!(open(&mut session_only).unwrap_err().to_string().contains("input_folder"));

        // The installed component changes: refused until it is installed and approved again.
        let mut changed = component.clone();
        changed.extend([0, 4, 3, b'n', b'e', b'w']);
        std::fs::write(plugins.join("smp/importer.wasm"), &changed).unwrap();
        let mut after_change = stored_session(&plugins);
        assert!(open(&mut after_change).unwrap_err().to_string().contains("re-approval"));
        assert_eq!(after_change.execute("plugin.grants", &json!({})).unwrap()["grants"][0]["status"], "changed");
        let reviewed = put(&root, "changed.zip", &zip(&changed));
        after_change.execute("plugin.install", &json!({"path": reviewed, "replace": true, "approve": {"input_folder": true}})).unwrap();
        assert_eq!(open(&mut stored_session(&plugins)).unwrap()["bodies"], 1);

        // Revoked in one session: the next runs it with nothing granted.
        let revoked = stored_session(&plugins).execute("plugin.revoke", &json!({"id": "smp"})).unwrap();
        assert_eq!(revoked["revoked"], true);
        assert!(!plugins.join(".grants/smp.json").exists());
        let mut after_revoke = stored_session(&plugins);
        assert_eq!(after_revoke.execute("plugin.grants", &json!({})).unwrap()["grants"], json!([]));
        assert!(open(&mut after_revoke).unwrap_err().to_string().contains("input_folder"));
        assert_eq!(after_revoke.execute("doc.open", &json!({"path": put(&root, "one.smp", &design())})).unwrap()["bodies"], 1);
        assert_eq!(after_revoke.execute("plugin.revoke", &json!({"id": "smp"})).unwrap()["revoked"], false);
        assert!(after_revoke.execute("plugin.revoke", &json!({"id": "../smp"})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_removal_whose_approvals_cannot_be_taken_back_keeps_the_package_installed() {
        let (_, root) = installed("stored-remove", &[]);
        let plugins = root.join("plugins");
        let (package, version) = API.split_once('@').unwrap();
        let component = wat::parse_str(include_str!("importer_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap();
        let manifest = importer_manifest("smp", &["smp"], true).to_string();
        let zip = solidcraft_io::safe_zip::write(&[("plugin.json", manifest.as_bytes()), ("importer.wasm", &component)]).unwrap();
        let original = put(&root, "smp.zip", &zip);
        stored_session(&plugins).execute("plugin.install", &json!({"path": original, "approve": {"input_folder": true}})).unwrap();
        let record = plugins.join(".grants/smp.json");
        // A file scope that reaches the package but not the grant store: the record cannot be
        // deleted, so nothing is removed and the error says why.
        let mut s = stored_session(&plugins);
        let scope = solidcraft_io::vfs::PathScope::new([plugins.join("smp")]);
        let refused = solidcraft_io::vfs::scoped(Some(scope), || s.execute("plugin.remove", &json!({"id": "smp"})));
        assert!(refused.unwrap_err().to_string().contains("approvals"));
        assert!(record.is_file() && plugins.join("smp/plugin.json").is_file());
        s.execute("plugin.remove", &json!({"id": "smp"})).unwrap();
        assert!(!record.exists() && !plugins.join("smp").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
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
        // A changed auxiliary file keeps the manifest-and-component hash but not the archive's:
        // installing with the reviewed archive hash refuses it and publishes nothing.
        let (m, w) = (
            serde_json::to_vec(&{
                let mut m: Value = serde_json::from_slice(&manifest_bytes("gears", json!(["commands@0.1"]))).unwrap();
                m["version"] = json!("0.1.0");
                m
            })
            .unwrap(),
            b"\0asm",
        );
        let reviewed = zip(&[("gears-0.1.0/plugin.json", &m[..]), ("gears-0.1.0/plugin.wasm", w), ("gears-0.1.0/data.txt", b"reviewed")]);
        let changed = zip(&[("gears-0.1.0/plugin.json", &m[..]), ("gears-0.1.0/plugin.wasm", w), ("gears-0.1.0/data.txt", b"changed")]);
        let aux = put(&root, "aux.zip", &reviewed);
        let aux_dry = s.execute("plugin.install", &json!({"path": aux, "dry_run": true})).unwrap();
        std::fs::write(&aux, &changed).unwrap();
        let error = s.execute("plugin.install", &json!({"path": aux, "approval": aux_dry["approval"], "archive": aux_dry["archive"]})).unwrap_err();
        assert!(error.to_string().contains("archive"), "{error}");
        assert!(!root.join("plugins/gears").exists() && s.plugin_state.grants.is_empty());
        let reply = s.execute("plugin.install", &json!({"path": path, "approval": dry["approval"], "archive": dry["archive"]})).unwrap();
        assert_eq!(reply["approval"], dry["approval"]);
        assert_eq!(reply["archive"], dry["archive"]);
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
    fn a_zip_replaced_after_review_is_refused_before_files_or_grants_are_written() {
        let (mut s, root) = session("swapped-after-review");
        let path = requested_package(&root);
        let dry = s.execute("plugin.install", &json!({"path":path,"dry_run":true})).unwrap();
        assert!(dry["approval"].as_str().is_some_and(|hash| !hash.is_empty()));
        // The same manifest with different component bytes must also invalidate the review.
        let mut replacement = unpack(&std::fs::read(&path).unwrap()).unwrap();
        replacement.files.iter_mut().find(|file| file.path == "plugin.wasm").unwrap().bytes = b"replaced component".to_vec();
        let files: Vec<_> = replacement.files.iter().map(|file| (file.path.as_str(), file.bytes.as_slice())).collect();
        std::fs::write(&path, zip(&files)).unwrap();
        let approve = json!({"command_groups":["file"],"folders":[{"name":"project","path":root,"read":true,"write":false}]});
        let error = s.execute("plugin.install", &json!({"path":path,"approval":dry["approval"],"approve":approve})).unwrap_err();
        assert!(matches!(error, crate::EngineError::Plugin(HostError::InvalidInput { field, .. }) if field == "approval"));
        assert!(!root.join("plugins").exists(), "even the plugins root and staging folders stay unwritten");
        assert!(s.plugin_state.grants.is_empty());
        assert!(s.plugin_state.packages.is_empty());
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

    fn requested_package(root: &Path) -> String {
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"command_groups":["app","file"],"files":[{"name":"project","read":true,"write":true}]});
        m["runtime"]["entries"] = json!([{"name":"make","slots":[{"kind":"menu","name":"File"},{"kind":"ribbon","name":"Model"}]}]);
        put(root, "requested.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"inert bytes")]))
    }

    #[test]
    fn install_approvals_are_validated_before_writing_and_dry_run_never_stores_them() {
        let (mut s, root) = session("approval");
        let path = requested_package(&root);
        let approve = json!({"command_groups":["file"],"folders":[{"name":"project","path":root,"read":true,"write":false}]});
        let dry = s.execute("plugin.install", &json!({"path":path,"dry_run":true,"approve":approve})).unwrap();
        assert_eq!(dry["entry_points"][0]["slots"].as_array().unwrap().len(), 2);
        assert!(s.plugin_state.grants.is_empty() && !root.join("plugins").exists());
        for unasked in [
            json!({"command_groups":["plugin"]}),
            json!({"folders":[{"name":"unasked","path":root,"read":true,"write":false}]}),
            json!({"folders":[{"name":"project","path":"relative","read":true,"write":false}]}),
            json!({"folders":[{"name":"project","path":root,"read":false,"write":false}]}),
            json!({"folders":[{"name":"project","path":root,"read":true,"write":false},{"name":"project","path":root,"read":true,"write":false}]}),
        ] {
            for dry_run in [false, true] {
                let error = s.execute("plugin.install", &json!({"path":path,"dry_run":dry_run,"approve":unasked})).unwrap_err();
                assert!(matches!(error, EngineError::Plugin(HostError::PermissionDenied { .. })), "{error}");
                assert!(s.plugin_state.grants.is_empty() && !root.join("plugins").exists());
            }
        }
        // Reject a write escalation even when the folder's name is requested.
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"files":[{"name":"project","read":true,"write":false}]});
        let read_only = put(&root, "read-only.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"x")]));
        let error = s
            .execute("plugin.install", &json!({"path":read_only,"approve":{"folders":[{"name":"project","path":root,"read":true,"write":true}]}}))
            .unwrap_err();
        assert!(matches!(error, EngineError::Plugin(HostError::PermissionDenied { .. })));
        s.execute("plugin.install", &json!({"path":path,"approve":approve})).unwrap();
        let granted = s.plugin_state.grants["sample"].clone();
        assert_eq!(granted.package_hash.as_deref(), Some(read_package(&root.join("plugins/sample").to_string_lossy()).unwrap().approval.as_str()));
        assert_eq!(granted.command_groups, BTreeSet::from(["file".into()]));
        assert_eq!(granted.folders, vec![FileGrant { name: "project".into(), folder: root.clone(), read: true, write: false }]);
        // A refused replacement cannot discard the original grants or package.
        assert!(s.execute("plugin.install", &json!({"path":path})).is_err());
        assert_eq!(s.plugin_state.grants["sample"], granted);
        assert!(s.execute("plugin.remove", &json!({"id":"../outside"})).is_err());
        s.disabled_plugins.insert("sample".into());
        let removed = s.execute("plugin.remove", &json!({"id":"sample"})).unwrap();
        assert_eq!(removed["removed"], true);
        assert!(!root.join("plugins/sample").exists());
        assert!(
            !s.plugin_state.grants.contains_key("sample")
                && !s.plugin_state.packages.contains_key("sample")
                && !s.disabled_plugins.contains("sample")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn approved_memory_raises_the_ceiling_for_that_exact_package_only() {
        let (mut s, root) = session("memory");
        let mut m = manifest();
        m["runtime"]["limits"]["memory_mib"] = json!(1024);
        let path = put(&root, "memory.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"inert bytes")]));
        let dry = s.execute("plugin.install", &json!({"path":path,"dry_run":true})).unwrap();
        assert_eq!((dry["limits"]["memory_mib"].as_u64(), dry["host_limits"]["max_memory_mib"].as_u64()), (Some(1024), Some(128)));
        for wrong in [json!(512), json!(2048), json!(0), json!(-1), json!("1024")] {
            assert!(s.execute("plugin.install", &json!({"path":path,"approve":{"memory_mib":wrong}})).is_err(), "{wrong}");
            assert!(s.plugin_state.grants.is_empty() && !root.join("plugins").exists());
        }
        s.execute("plugin.install", &json!({"path":path,"approve":{"memory_mib":1024}})).unwrap();
        let approval = read_package(&root.join("plugins/sample").to_string_lossy()).unwrap().approval;
        assert_eq!(s.plugin_state.grants["sample"].memory_mib, Some(1024));
        assert_eq!(host_limits_for(&s, "sample", &approval).max_memory_mib, 1024);
        assert_eq!(host_limits_for(&s, "sample", "another package").max_memory_mib, 128);
        assert_eq!(s.plugin_limits.max_memory_mib, 128, "the user's ceiling for other plugins is unchanged");
        let inspected = s.execute("plugin.inspect", &json!({"path":root.join("plugins/sample")})).unwrap();
        assert_eq!(inspected["within_host_limits"], true);
        // Reinstalling without approving memory takes the grant away.
        s.execute("plugin.install", &json!({"path":path,"replace":true})).unwrap();
        assert_eq!(host_limits_for(&s, "sample", &approval).max_memory_mib, 128);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installs_without_approval_grant_nothing_and_replacement_drops_stale_grants() {
        let (mut s, root) = session("stale");
        let path = requested_package(&root);
        s.execute("plugin.install", &json!({"path":path})).unwrap();
        assert_eq!(s.plugin_state.grants["sample"].command_groups, BTreeSet::new());
        assert!(s.plugin_state.grants["sample"].folders.is_empty());
        assert!(s.plugin_state.grants["sample"].package_hash.is_some());
        s.execute("plugin.install", &json!({"path":path,"replace":true,"approve":{"command_groups":["app","file"],"folders":[{"name":"project","path":root,"read":true,"write":true}]}})).unwrap();
        let mut m = manifest();
        m["runtime"]["permissions"] = json!({"command_groups":["app"]});
        let newer = put(&root, "newer.zip", &zip(&[("plugin.json", &serde_json::to_vec(&m).unwrap()), ("plugin.wasm", b"new bytes")]));
        s.execute("plugin.install", &json!({"path":newer,"replace":true,"approve":{"command_groups":["app"]}})).unwrap();
        let package_hash = Some(read_package(&root.join("plugins/sample").to_string_lossy()).unwrap().approval);
        assert_eq!(
            s.plugin_state.grants["sample"],
            Grants { package_hash: package_hash.clone(), command_groups: BTreeSet::from(["app".into()]), ..Default::default() }
        );
        s.execute("plugin.install", &json!({"path":newer,"replace":true})).unwrap();
        assert_eq!(s.plugin_state.grants["sample"], Grants { package_hash, ..Default::default() });
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hostile_install_approvals_and_remove_scopes_leave_the_package_unchanged() {
        let (mut s, root) = session("hostile");
        let path = requested_package(&root);
        for approve in [
            json!(false),
            json!([]),
            json!([["file"], []]),
            json!({"folders":[["project",root,true,true]]}),
            json!({"extra":true}),
            json!({"folders":[{"name":"project","folder":root,"read":true,"write":true}]}),
            json!({"folders":[{"name":"project","path":root,"read":"yes","write":true}]}),
        ] {
            assert!(
                s.execute("plugin.install", &json!({"path":path,"approve":approve})).is_err(),
                "refuse non-object or invalid approval: {approve}"
            );
            assert!(!root.join("plugins").exists());
        }
        let approve = json!({"folders":[{"name":"project","path":root,"read":true,"write":true}]});
        assert!(s.execute("plugin.install", &json!([path, false, false, approve])).is_err(), "install parameters must be an object");
        assert!(!root.join("plugins").exists());
        solidcraft_io::vfs::scoped(Some(Default::default()), || {
            assert!(s.execute("plugin.install", &json!({"path":path,"approve":approve})).is_err());
        });
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.join("zips")])), || {
            assert!(s.execute("plugin.install", &json!({"path":path,"approve":approve})).is_err(), "folder approval retains caller scope");
        });
        assert!(!root.join("plugins").exists());
        s.execute("plugin.install", &json!({"path":path,"approve":approve})).unwrap();
        let hidden = root.join("plugins/sample/.private");
        std::fs::write(&hidden, "private").unwrap();
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::default().with_design_folder(&root)), || {
            assert!(s.execute("plugin.remove", &json!({"id":"sample"})).is_err());
        });
        // Replacement must not erase hidden descendants that this caller cannot remove.
        let prior_grants = s.plugin_state.grants["sample"].clone();
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::default().with_design_folder(&root)), || {
            assert!(s.execute("plugin.install", &json!({"path":path,"replace":true})).is_err());
        });
        assert_eq!(s.plugin_state.grants["sample"], prior_grants);
        assert!(hidden.exists() && s.plugin_state.grants.contains_key("sample"));
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([root.join("plugins")])), || {
            s.execute("plugin.remove", &json!({"id":"sample"})).unwrap();
        });
        let hidden_zip = put(
            &root,
            "hidden.zip",
            &zip(&[("plugin.json", &serde_json::to_vec(&manifest()).unwrap()), ("plugin.wasm", b"x"), (".private", b"new private data")]),
        );
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::default().with_design_folder(&root)), || {
            assert!(s.execute("plugin.install", &json!({"path":hidden_zip})).is_err());
        });
        assert!(!root.join("plugins/sample").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_oversized_and_incomplete_zips_are_refused_before_anything_is_written() {
        let good_bytes = manifest_bytes("gears", json!(["commands@0.1"]));
        let good: &[u8] = &good_bytes;
        let slip = unpack(&zip(&[("plugin.json", good), ("plugin.wasm", b"x"), ("../evil", b"x")])).unwrap_err();
        assert!(matches!(slip, InstallError::Archive(safe_zip::Error::UnsafeEntry { .. })), "{slip}");
        let absolute = unpack(&zip(&[("/plugin.json", good)])).unwrap_err();
        assert!(matches!(absolute, InstallError::Archive(safe_zip::Error::UnsafeEntry { .. })), "{absolute}");
        let huge = vec![0u8; craft_plugin::MAX_IMPORTER_COMPONENT_BYTES + 1];
        let bomb = unpack(&zip(&[("plugin.json", good), ("plugin.wasm", &huge[..])])).unwrap_err();
        assert!(matches!(bomb, InstallError::Archive(safe_zip::Error::EntryTooLarge { .. })), "{bomb}");
        // A command plugin's component stays within 16 MiB; only an importer's may be larger.
        let big = vec![0u8; MAX_COMPONENT_BYTES + 1];
        let command = unpack(&zip(&[("plugin.json", good), ("plugin.wasm", &big[..])])).unwrap_err();
        assert!(matches!(command, InstallError::Manifest(HostError::LimitExceeded { .. })), "{command}");
        let chunk = vec![0u8; 15 << 20];
        let many: Vec<String> = (0..6).map(|i| format!("data/{i}.bin")).collect();
        let mut files: Vec<(&str, &[u8])> = vec![("plugin.json", good), ("plugin.wasm", b"x")];
        files.extend(many.iter().map(|n| (n.as_str(), chunk.as_slice())));
        assert_eq!(unpack(&zip(&files)).unwrap_err(), InstallError::Archive(safe_zip::Error::TooLarge { limit: 80 << 20 }));
        assert_eq!(unpack(&zip(&[("plugin.wasm", b"x")])).unwrap_err(), InstallError::MissingManifest);
        assert_eq!(unpack(&zip(&[("plugin.json", good)])).unwrap_err(), InstallError::MissingModule("plugin.wasm".into()));
        // GitHub's "Source code" archive of a release: one folder with the manifest and the
        // sources, but no built module. The refusal points to the plugin's own .zip (#16736).
        let source = zip(&[("gears-0.1.1/plugin.json", good), ("gears-0.1.1/Cargo.toml", b"[package]"), ("gears-0.1.1/src/lib.rs", b"")]);
        let error = unpack(&source).unwrap_err();
        assert_eq!(error, InstallError::SourceOnly("plugin.wasm".into()));
        assert!(error.to_string().contains("the plugin's own .zip from its release"), "{error}");
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

/// The grant store the desktop app, the command line and headless MCP share.
#[cfg(not(target_arch = "wasm32"))]
mod stored_grants {
    use super::*;
    use crate::plugin::grant_store::{self, Synced};

    fn root(name: &str) -> PathBuf {
        let root = solidcraft_io::vfs::temp_dir().join(format!("sc-plugin-grant-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
    fn approved(groups: &[&str]) -> Grants {
        Grants { package_hash: Some("a".repeat(64)), command_groups: groups.iter().map(|g| g.to_string()).collect(), ..Default::default() }
    }

    #[test]
    fn hostile_records_are_ignored_and_grant_nothing() {
        let plugins = root("hostile");
        grant_store::write(&plugins, "good", &approved(&["file"])).unwrap();
        let dir = grant_store::folder(&plugins);
        let record = |grants: Value| serde_json::to_vec(&grants).unwrap();
        for (name, bytes) in [
            ("broken.json", b"{".to_vec()),
            ("unknown.json", record(json!({"command_groups":["file"],"net":true}))),
            ("group.json", record(json!({"command_groups":["everything"]}))),
            ("memory.json", record(json!({"memory_mib":1_000_000}))),
            ("hash.json", record(json!({"package_hash":"not a hash"}))),
            ("relative.json", record(json!({"folders":[{"name":"out","folder":"out","read":true,"write":true}]}))),
            ("Bad_Id.json", record(json!({}))),
            ("large.json", vec![b' '; 70 << 10]),
            ("notes.txt", record(json!({}))),
        ] {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        std::fs::create_dir(dir.join("folder.json")).unwrap();
        assert_eq!(grant_store::read_all(&plugins), BTreeMap::from([("good".to_string(), approved(&["file"]))]));
        assert!(grant_store::write(&plugins, "../evil", &approved(&[])).is_err() && grant_store::delete(&plugins, "../evil").is_err());
        assert!(grant_store::write(&plugins, "bad", &approved(&["everything"])).is_err());
        assert!(!plugins.join("evil.json").exists() && !dir.join("bad.json").exists());
        grant_store::delete(&plugins, "absent").unwrap();
        std::fs::remove_dir_all(plugins).unwrap();
    }

    #[test]
    fn a_store_outside_the_file_scope_is_neither_read_nor_written() {
        let (plugins, elsewhere) = (root("scope"), root("scope-elsewhere"));
        grant_store::write(&plugins, "gears", &approved(&["file"])).unwrap();
        let (mut synced, mut working) = (Synced::default(), BTreeMap::new());
        synced.refresh(&plugins, &mut working);
        solidcraft_io::vfs::scoped(Some(solidcraft_io::vfs::PathScope::new([elsewhere.clone()])), || {
            assert!(grant_store::read_all(&plugins).is_empty());
            assert!(grant_store::write(&plugins, "bolts", &approved(&[])).is_err());
            assert!(grant_store::delete(&plugins, "gears").is_err());
            synced.refresh(&plugins, &mut working);
        });
        assert_eq!(working.keys().collect::<Vec<_>>(), ["gears"], "a store out of reach is no revocation");
        assert_eq!(grant_store::read_all(&plugins).keys().collect::<Vec<_>>(), ["gears"]);
        std::fs::remove_dir_all(plugins).unwrap();
        std::fs::remove_dir_all(elsewhere).unwrap();
    }

    #[test]
    fn each_host_writes_its_own_changes_and_takes_in_the_others() {
        let plugins = root("sync");
        let (mut desktop, mut cli) = (Synced::default(), Synced::default());
        let mut desktop_grants = BTreeMap::from([("gears".to_string(), approved(&["file"]))]);
        // No store yet: the desktop's grants move to it.
        desktop.adopt(&plugins, &mut desktop_grants).unwrap();
        assert_eq!(grant_store::read_all(&plugins), desktop_grants);
        let mut cli_grants = BTreeMap::new();
        cli.refresh(&plugins, &mut cli_grants);
        assert_eq!(cli_grants, desktop_grants);
        // The command line approves another plugin and revokes the desktop's.
        cli_grants.insert("bolts".into(), approved(&["doc"]));
        cli_grants.remove("gears");
        cli.persist(&plugins, &cli_grants).unwrap();
        // The desktop changed nothing of either meanwhile, so it takes both in.
        desktop_grants.insert("nuts".into(), approved(&[]));
        desktop.persist(&plugins, &desktop_grants).unwrap();
        desktop.refresh(&plugins, &mut desktop_grants);
        assert_eq!(desktop_grants.keys().collect::<Vec<_>>(), ["bolts", "nuts"]);
        assert_eq!(grant_store::read_all(&plugins), desktop_grants);
        // Once the store exists it is the whole truth: an old desktop record is not brought back.
        let mut restarted = BTreeMap::from([("gears".to_string(), approved(&["file"]))]);
        Synced::default().adopt(&plugins, &mut restarted).unwrap();
        assert_eq!(restarted, desktop_grants);
        std::fs::remove_dir_all(plugins).unwrap();
    }
}
