use super::*;

use craft_plugin::{API, FileGrant, FilePermission};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Default)]
struct App {
    value: usize,
    saved: Option<usize>,
    begins: usize,
    commits: usize,
    rollbacks: usize,
    grants: Vec<FileGrant>,
    cancelled: bool,
    fail_commit: bool,
    unchanged_at_commit: Option<PathBuf>,
    /// The sensitive group `app.write` is in, and the groups the user granted.
    write_group: Option<String>,
    groups: BTreeSet<String>,
}
impl PluginHost for App {
    fn command_catalog(&self) -> Vec<CommandSchema> {
        vec![CommandSchema {
            name: "app.write".into(),
            input_schema: json!({"type":"object"}),
            output_schema: json!({"type":"object"}),
            effect: Effect::Edit,
            group: self.write_group.clone(),
        }]
    }
    fn run_command(&mut self, _: &str, _: &Value) -> Result<Value> {
        self.value += 1;
        Ok(json!({"value":self.value}))
    }
    fn begin_transaction(&mut self) -> Result<()> {
        self.saved = Some(self.value);
        self.begins += 1;
        Ok(())
    }
    fn commit_transaction(&mut self) -> Result<()> {
        if let Some(path) = &self.unchanged_at_commit {
            assert_eq!(std::fs::read(path).unwrap(), b"original");
        }
        if self.fail_commit {
            return Err(trap("commit refused"));
        }
        self.saved = None;
        self.commits += 1;
        Ok(())
    }
    fn rollback_transaction(&mut self) -> Result<()> {
        self.value = self.saved.take().unwrap();
        self.rollbacks += 1;
        Ok(())
    }
    fn granted_folders(&self) -> Vec<FileGrant> {
        self.grants.clone()
    }
    fn granted_command_groups(&self) -> BTreeSet<String> {
        self.groups.clone()
    }
    fn log(&mut self, _: &str) {}
    fn progress(&mut self, _: f32, _: &str) {}
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}
fn manifest() -> Manifest {
    craft_plugin::parse_manifest(
        json!({"id":"sample","name":"Sample","version":"1","capabilities":["commands@0.1"],"runtime":{
            "module":"plugin.wasm","entries":[{"name":"make","effect":"edit","input_schema":{"type":"object"}}],
            "commands":["app.write"],"limits":{"fuel":100000,"memory_mib":1,"deadline_ms":5000}
        }})
        .to_string()
        .as_bytes(),
        "sample",
    )
    .unwrap()
}
fn component(code: &str) -> Vec<u8> {
    let (package, version) = API.split_once('@').unwrap();
    let template = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version).replace("\r\n", "\n");
    let original = "      call $box\n      call $box\n";
    assert!(template.contains(original));
    wat::parse_str(template.replacen(original, &format!("      {code}\n"), 1)).unwrap()
}
#[test]
fn edit_commands_commit_as_one_transaction_and_instances_are_fresh() {
    let mut app = App::default();
    run(&mut app, &manifest(), &component("call $box call $box"), "make", &json!({})).unwrap();
    assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (2, 1, 1, 0));
    assert!(run(&mut app, &manifest(), &component("call $box unreachable"), "make", &json!({})).is_err());
    assert_eq!(app.value, 2);
    run(&mut app, &manifest(), &component("call $box"), "make", &json!({})).unwrap();
    assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (3, 3, 2, 1));
}
#[test]
fn traps_runaway_loops_memory_and_swallowed_failures_rollback() {
    for (code, diagnostic) in [
        ("call $box unreachable", "trap=UnreachableCodeReached"),
        ("call $box (loop $forever br $forever)", "trap=OutOfFuel"),
        ("call $box i32.const 10000 memory.grow drop", "memory"),
        ("call $box i32.const 8 i32.const 1 i32.store", "entry result is not JSON"),
        (
            "call $box i32.const 2048 i32.const 11 i32.const 1024 i32.const 33 i32.const 0 call $run i32.const 0 i32.const 0 i32.store i32.const 4 i32.const 1024 i32.store i32.const 8 i32.const 33 i32.store",
            "not granted",
        ),
    ] {
        let mut app = App { value: 7, ..Default::default() };
        let failure = run(&mut app, &manifest(), &component(code), "make", &json!({})).unwrap_err();
        assert!(failure.to_string().contains(diagnostic), "{code}: {failure}");
        assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (7, 1, 0, 1));
    }
}
#[test]
fn network_permission_is_refused_as_typed_error_before_any_code_runs() {
    let mut manifest = manifest();
    manifest.runtime.as_mut().unwrap().permissions.net = true;
    let mut app = App::default();
    assert_eq!(
        run(&mut app, &manifest, b"invalid component", "make", &json!({})).unwrap_err(),
        HostError::PermissionDenied { permission: "net".into() }
    );
    assert_eq!(app.begins, 0);
}
#[test]
fn linker_has_no_socket_or_http_imports() {
    let (package, version) = API.split_once('@').unwrap();
    for interface in [
        "wasi:sockets/network@0.2.0".to_string(),
        "wasi:sockets/instance-network@0.2.0".into(),
        "wasi:http/outgoing-handler@0.2.0".into(),
        "wasi:http/types@0.2.0".into(),
        format!("{package}/fetch@{version}"),
        "fetch".into(),
    ] {
        let source = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version);
        let (prefix, _) = source.rsplit_once(')').unwrap();
        let source = format!("{prefix}\n(import \"{interface}\" (instance))\n)");
        let mut app = App::default();
        let failure = run(&mut app, &manifest(), &wat::parse_str(source).unwrap(), "make", &json!({})).unwrap_err();
        assert_eq!(failure, HostError::PermissionDenied { permission: format!("network import is forbidden: {interface}") });
        assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (0, 1, 0, 1));
    }
}
#[test]
fn read_entries_cannot_edit_even_when_manifest_grants_the_command() {
    let mut manifest = manifest();
    manifest.runtime.as_mut().unwrap().entries[0].effect = Effect::Read;
    let mut app = App::default();
    let failure = run(&mut app, &manifest, &component("call $box"), "make", &json!({})).unwrap_err();
    assert!(matches!(failure, HostError::PermissionDenied { .. }));
    assert_eq!((app.value, app.commits, app.rollbacks), (0, 0, 1));
}
#[test]
fn a_grouped_command_runs_only_when_the_manifest_asks_and_the_app_grants() {
    let asks = |groups: &[&str]| {
        let mut manifest = manifest();
        manifest.runtime.as_mut().unwrap().permissions.command_groups = groups.iter().map(|g| g.to_string()).collect();
        manifest
    };
    let granted =
        |groups: &[&str]| App { write_group: Some("file".into()), groups: groups.iter().map(|g| g.to_string()).collect(), ..Default::default() };
    for (manifest, mut app) in [(asks(&[]), granted(&["file"])), (asks(&["file"]), granted(&[])), (asks(&["doc"]), granted(&["doc"]))] {
        let failure = run(&mut app, &manifest, &component("call $box"), "make", &json!({})).unwrap_err();
        assert!(matches!(&failure, HostError::PermissionDenied { permission } if permission.contains("needs the `file` permission")), "{failure}");
        assert_eq!((app.value, app.commits, app.rollbacks), (0, 0, 1));
    }
    let mut app = granted(&["file"]);
    run(&mut app, &asks(&["file"]), &component("call $box"), "make", &json!({})).unwrap();
    assert_eq!((app.value, app.commits), (1, 1));
}
#[test]
fn a_component_for_another_api_version_is_refused_in_plain_words() {
    let (package, version) = API.split_once('@').unwrap();
    let source = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version);
    let (prefix, _) = source.rsplit_once(')').unwrap();
    let source = format!("{prefix}\n(import \"{package}/storage@0.2.0\" (instance))\n)");
    let mut app = App::default();
    let failure = run(&mut app, &manifest(), &wat::parse_str(source).unwrap(), "make", &json!({})).unwrap_err();
    assert!(
        matches!(&failure, HostError::UnsupportedCapability { reason, .. } if reason.contains(&format!("built against {package}@0.2.0")) && reason.contains("update")),
        "{failure}"
    );
    assert_eq!((app.value, app.commits, app.rollbacks), (0, 0, 1));
}
#[test]
fn cancellation_prevents_running_plugin_code() {
    let mut app = App { cancelled: true, ..Default::default() };
    assert_eq!(run(&mut app, &manifest(), &component("(loop br 0)"), "make", &json!({})).unwrap_err(), HostError::Cancelled);
    assert_eq!(app.begins, 0);
}

fn state<H: PluginHost>(app: H) -> State<H> {
    State {
        host: app,
        commands: BTreeMap::new(),
        groups: BTreeSet::new(),
        effect: Effect::Edit,
        folders: BTreeMap::new(),
        staged: BTreeMap::new(),
        clock: false,
        limits: StoreLimitsBuilder::new().build(),
        failed: None,
        calls: 0,
        log_bytes: 0,
        deadline: Instant::now() + Duration::from_secs(5),
    }
}
#[test]
fn files_are_denied_without_an_explicit_folder_grant() {
    use files::Host;
    let mut state = state(App::default());
    assert!(matches!(state.read_file("input".into(), "secret.txt".into()), Err(PluginError::PermissionDenied(_))));
    assert!(matches!(state.failed, Some(HostError::PermissionDenied { .. })));
    let mut state = self::state(App::default());
    assert!(matches!(state.write_file("output".into(), "file.txt".into(), vec![]), Err(PluginError::PermissionDenied(_))));
}
#[test]
fn files_inside_grant_work_and_escape_is_refused() {
    use files::Host;
    let root = std::env::temp_dir().join(format!("craft-plugin-test-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    let mut state = state(App::default());
    state.folders.insert(
        "data".into(),
        Folder { dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(), path: root.clone(), read: true, write: true },
    );
    state.write_file("data".into(), "hello.txt".into(), b"hello".to_vec()).unwrap();
    assert_eq!(state.read_file("data".into(), "hello.txt".into()).unwrap(), b"hello");
    assert!(!root.join("hello.txt").exists());
    state.publish_writes().unwrap();
    state.discard_writes();
    assert_eq!(std::fs::read(root.join("hello.txt")).unwrap(), b"hello");
    assert!(state.read_file("data".into(), "../hello.txt".into()).is_err());
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn staged_writes_publish_in_guest_order_across_grant_aliases_and_normalize_separators() {
    use files::Host;
    let root = std::env::temp_dir().join(format!("craft-plugin-write-order-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(root.join("nested")).unwrap();
    let mut state = state(App::default());
    for name in ["a", "z"] {
        state.folders.insert(
            name.into(),
            Folder { dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(), path: root.clone(), read: true, write: true },
        );
    }
    state.write_file("z".into(), "nested//target.txt".into(), b"first".to_vec()).unwrap();
    assert_eq!(state.read_file("z".into(), "nested/target.txt".into()).unwrap(), b"first");
    state.write_file("a".into(), "nested/target.txt".into(), b"last".to_vec()).unwrap();
    assert_eq!(state.staged.len(), 2);
    state.publish_writes().unwrap();
    state.discard_writes();
    assert_eq!(std::fs::read(root.join("nested/target.txt")).unwrap(), b"last");
    assert_eq!(std::fs::read_dir(root.join("nested")).unwrap().count(), 1);

    state.write_file("z".into(), "nested//target.txt".into(), b"replaced".to_vec()).unwrap();
    state.write_file("z".into(), "nested/target.txt".into(), b"latest".to_vec()).unwrap();
    assert_eq!(state.staged.len(), 1);
    assert_eq!(state.read_file("z".into(), "nested//target.txt".into()).unwrap(), b"latest");
    state.publish_writes().unwrap();
    state.discard_writes();
    assert_eq!(std::fs::read(root.join("nested/target.txt")).unwrap(), b"latest");
    assert_eq!(std::fs::read_dir(root.join("nested")).unwrap().count(), 1);
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn epoch_interrupt_is_distinct_from_fuel_exhaustion_in_diagnostics() {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true).epoch_interruption(true);
    let engine = wasmtime::Engine::new(&config).unwrap();
    let module = wasmtime::Module::from_binary(&engine, &wat::parse_str(r#"(module (func (export "run") (loop br 0)))"#).unwrap()).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let limits = Limits { fuel: 100_000, memory_mib: 1, deadline_ms: 5000 };
    store.set_fuel(limits.fuel).unwrap();
    store.set_epoch_deadline(1);
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance.get_typed_func::<(), ()>(&mut store, "run").unwrap();
    engine.increment_epoch();
    let failure = run.call(&mut store, ()).unwrap_err();
    let remaining = store.get_fuel().unwrap();
    assert!(remaining > 0);
    let message = execution_error(&failure, Some(remaining), &limits).to_string();
    assert!(message.contains("trap=Interrupt"), "{message}");
    assert!(!message.contains("OutOfFuel"), "{message}");
    assert!(message.contains(&format!("fuel_remaining={remaining}")), "{message}");
}

fn files_component(write: bool, swallow_error: bool) -> Vec<u8> {
    files_component_after(write, swallow_error, "")
}

fn files_component_after(write: bool, swallow_error: bool, after: &str) -> Vec<u8> {
    let (package, version) = API.split_once('@').unwrap();
    let template = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version);
    // Keep exactly the shared WIT types, then exercise the real files interface.
    let (types, _) = template.split_once(&format!("  (type $ty-{package}/commands@{version}")).unwrap();
    let (operation, result, parameters, arguments) = if write {
        ("write-file", "(result (error $error))", "(param \"data\" (list u8))", "i32.const 128 i32.const 7")
    } else {
        ("read-file", "(result (list u8) (error $error))", "", "")
    };
    let core_parameters = if write { "i32 i32 i32 i32 i32 i32 i32" } else { "i32 i32 i32 i32 i32" };
    let reply =
        if swallow_error { "i32.const 0 i32.const 0 i32.store i32.const 4 i32.const 160 i32.store i32.const 8 i32.const 2 i32.store" } else { "" };
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
(data (i32.const 96) "secret.txt")
(data (i32.const 128) "changed")
(data (i32.const 160) "{{}}")
(func (export "execute") (param i32 i32 i32 i32) (result i32)
  i32.const 64 i32.const 7 i32.const 96 i32.const 10 {arguments} i32.const 0 call $file
  {reply}
  {after}
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
fn file_import_denial_rolls_back_even_when_guest_swallows_it_and_grants_enable_reads() {
    for swallow_error in [false, true] {
        let mut app = App::default();
        let failure = run(&mut app, &manifest(), &files_component(false, swallow_error), "make", &json!({})).unwrap_err();
        assert!(matches!(failure, HostError::PermissionDenied { .. }), "{failure}");
        assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 1));
    }
    let mut app = App::default();
    let root = std::env::temp_dir().join(format!("craft-plugin-grant-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("secret.txt"), b"{}").unwrap();
    let mut manifest = manifest();
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission { name: "project".into(), read: true, write: false });
    let failure = run(&mut app, &manifest, &files_component(false, false), "make", &json!({})).unwrap_err();
    assert_eq!(failure, HostError::PermissionDenied { permission: "files.read:project".into() });
    app.grants.push(FileGrant { name: "project".into(), folder: root.clone(), read: true, write: false });
    assert_eq!(run(&mut app, &manifest, &files_component(false, false), "make", &json!({})).unwrap(), json!({}));
    std::fs::remove_dir_all(root).unwrap();
}

fn run(app: &mut App, manifest: &Manifest, component: &[u8], entry: &str, params: &Value) -> Result<Value> {
    let (returned, result) = crate::run(std::mem::take(app), manifest, component, entry, params);
    *app = returned;
    result
}

#[test]
fn component_file_writes_are_denied_without_a_host_grant() {
    for declared in [false, true] {
        let mut manifest = manifest();
        if declared {
            manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission { name: "project".into(), read: false, write: true });
        }
        let mut app = App { value: 7, ..Default::default() };
        let failure = run(&mut app, &manifest, &files_component(true, true), "make", &json!({})).unwrap_err();
        assert_eq!(failure, HostError::PermissionDenied { permission: "files.write:project".into() });
        assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (7, 1, 0, 1));
    }
}

#[test]
fn staged_file_writes_publish_after_commit_and_disappear_after_traps_or_commit_failure() {
    let root = std::env::temp_dir().join(format!("craft-plugin-staged-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("secret.txt");
    let mut manifest = manifest();
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission { name: "project".into(), read: false, write: true });
    for (after, fail_commit) in [("unreachable", false), ("", true), ("", false)] {
        std::fs::write(&target, b"original").unwrap();
        let mut app = App {
            grants: vec![FileGrant { name: "project".into(), folder: root.clone(), read: false, write: true }],
            fail_commit,
            unchanged_at_commit: Some(target.clone()),
            ..Default::default()
        };
        let result = run(&mut app, &manifest, &files_component_after(true, true, after), "make", &json!({}));
        let succeeds = after.is_empty() && !fail_commit;
        assert_eq!(result.is_ok(), succeeds, "{result:?}");
        assert_eq!(std::fs::read(&target).unwrap(), if succeeds { b"changed".as_slice() } else { b"original".as_slice() });
        assert_eq!((app.commits, app.rollbacks), if succeeds { (1, 0) } else { (0, 1) });
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1, "staged files must be removed");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_file_operation_refuses_hidden_names_inside_a_granted_folder() {
    use files::Host;
    let root = std::env::temp_dir().join(format!("craft-plugin-hidden-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(root.join(".private")).unwrap();
    std::fs::write(root.join(".env"), b"secret").unwrap();
    std::fs::write(root.join(".private/config"), b"secret").unwrap();
    for path in [".env", ".private/config"] {
        for write in [false, true] {
            let mut state = state(App::default());
            state.folders.insert(
                "data".into(),
                Folder { dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(), path: root.clone(), read: true, write: true },
            );
            let result = if write {
                state.write_file("data".into(), path.into(), b"changed".to_vec())
            } else {
                state.read_file("data".into(), path.into()).map(|_| ())
            };
            assert!(matches!(result, Err(PluginError::PermissionDenied(_))));
            assert_eq!(std::fs::read(root.join(path)).unwrap(), b"secret");
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn a_visible_link_to_a_hidden_folder_is_refused_for_reads_and_writes() {
    use files::Host;
    let root = std::env::temp_dir().join(format!("craft-plugin-link-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(root.join(".hidden")).unwrap();
    std::fs::write(root.join(".hidden/secret.txt"), b"secret").unwrap();
    std::fs::write(root.join("plain.txt"), b"plain").unwrap();
    std::os::unix::fs::symlink(".hidden", root.join("public")).unwrap();
    std::os::unix::fs::symlink(".hidden/secret.txt", root.join("note.txt")).unwrap();
    let folder = || Folder { dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(), path: root.clone(), read: true, write: true };
    for path in ["public/secret.txt", "public/new.txt", "note.txt"] {
        for write in [false, true] {
            let mut state = state(App::default());
            state.folders.insert("data".into(), folder());
            let result = if write {
                state.write_file("data".into(), path.into(), b"changed".to_vec())
            } else {
                state.read_file("data".into(), path.into()).map(|_| ())
            };
            assert!(matches!(&result, Err(PluginError::PermissionDenied(_))), "{path} write={write}: {result:?}");
        }
    }
    assert_eq!(std::fs::read(root.join(".hidden/secret.txt")).unwrap(), b"secret");
    assert_eq!(std::fs::read_dir(root.join(".hidden")).unwrap().count(), 1, "nothing staged through the link");
    // A plain file beside the links is still readable.
    let mut state = state(App::default());
    state.folders.insert("data".into(), folder());
    assert_eq!(state.read_file("data".into(), "plain.txt".into()).unwrap(), b"plain");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn file_publication_failure_reports_possible_changes_and_cleans_staged_files() {
    let root = std::env::temp_dir().join(format!("craft-plugin-publication-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    // The guest's target is a directory, so staging succeeds but publication fails.
    std::fs::create_dir_all(root.join("secret.txt")).unwrap();
    let mut app = App { grants: vec![FileGrant { name: "project".into(), folder: root.clone(), read: false, write: true }], ..Default::default() };
    let mut manifest = manifest();
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission { name: "project".into(), read: false, write: true });
    let failure = run(&mut app, &manifest, &files_component(true, true), "make", &json!({})).unwrap_err();
    assert!(failure.to_string().contains("publication failed after app commit; files may have changed"), "{failure}");
    assert_eq!((app.commits, app.rollbacks), (1, 0));
    assert!(root.join("secret.txt").is_dir());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn file_access_authorizes_the_complete_path_for_reads_and_writes() {
    use files::Host;
    struct ScopedHost {
        checks: std::cell::RefCell<Vec<(PathBuf, bool)>>,
        allowed: PathBuf,
    }
    impl PluginHost for ScopedHost {
        fn command_catalog(&self) -> Vec<CommandSchema> {
            vec![]
        }
        fn run_command(&mut self, _: &str, _: &Value) -> Result<Value> {
            Err(trap("no commands"))
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
            vec![]
        }
        fn log(&mut self, _: &str) {}
        fn progress(&mut self, _: f32, _: &str) {}
        fn authorize_file_path(&self, path: &Path, write: bool) -> Result<()> {
            self.checks.borrow_mut().push((path.to_owned(), write));
            if path == self.allowed { Ok(()) } else { Err(HostError::PermissionDenied { permission: "caller file scope".into() }) }
        }
    }
    let root = std::env::temp_dir().join(format!("craft-plugin-scope-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("allowed.txt"), b"original").unwrap();
    std::fs::write(root.join("denied.txt"), b"original").unwrap();
    for write in [false, true] {
        let mut state = state(ScopedHost { allowed: root.join("allowed.txt"), checks: Default::default() });
        state.folders.insert(
            "data".into(),
            Folder { dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(), path: root.clone(), read: true, write: true },
        );
        for path in ["allowed.txt", "denied.txt"] {
            let result = if write {
                state.write_file("data".into(), path.into(), b"changed".to_vec())
            } else {
                state.read_file("data".into(), path.into()).map(|_| ())
            };
            assert_eq!(result.is_ok(), path == "allowed.txt");
        }
        assert_eq!(*state.host.checks.borrow(), vec![(root.join("allowed.txt"), write), (root.join("denied.txt"), write)]);
        state.discard_writes();
    }
    assert_eq!(std::fs::read(root.join("denied.txt")).unwrap(), b"original");
    std::fs::remove_dir_all(root).unwrap();
}
