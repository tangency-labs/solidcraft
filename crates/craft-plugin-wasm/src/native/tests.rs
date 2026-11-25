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
    no_design: bool,
    fail_commit: bool,
    unchanged_at_commit: Option<PathBuf>,
    /// The sensitive group `app.write` is in, and the groups the user granted.
    write_group: Option<String>,
    groups: BTreeSet<String>,
    panel_data: craft_plugin::ui::PanelData,
    saved_panel_data: Option<craft_plugin::ui::PanelData>,
    panel_visible: bool,
    interactive: bool,
    forms: Vec<(String, Value)>,
    /// Granted folders where hidden names are reachable (their grant says `hidden`).
    hidden: Vec<PathBuf>,
    /// The home folder's files the plugin may read, by name and full path.
    home: Vec<(String, PathBuf)>,
}
impl PluginHost for App {
    fn shows_hidden(&self, folder: &Path) -> bool {
        self.hidden.iter().any(|shown| shown == folder)
    }
    fn home_files(&self) -> Vec<(String, PathBuf)> {
        self.home.clone()
    }
    fn open_form(&mut self, entry: &str, params: &Value) -> Result<()> {
        if !self.interactive {
            return Err(HostError::InteractiveRequired { entry: entry.into() });
        }
        self.forms.push((entry.into(), params.clone()));
        Ok(())
    }
    fn design_token(&self) -> Result<Option<craft_plugin::DesignToken>> {
        Ok((!self.no_design)
            .then(|| craft_plugin::DesignToken { document_id: "design-one".into(), revision: "0123456789abcdef0123456789abcdef:7".into() }))
    }
    fn command_catalog(&self) -> Vec<CommandSchema> {
        let schema = |name: &str, effect, group: Option<String>| CommandSchema {
            name: name.into(),
            input_schema: json!({"type":"object"}),
            output_schema: json!({"type":"object"}),
            effect,
            group,
        };
        vec![
            schema("app.write", Effect::Edit, self.write_group.clone()),
            schema("document.inspect", Effect::Read, None),
            schema("file.export", Effect::Read, Some("file".into())),
        ]
    }
    fn run_command(&mut self, name: &str, _: &Value) -> Result<Value> {
        if name == "document.inspect" {
            return Ok(json!({"name":"Bracket","bodies":1}));
        }
        self.value += 1;
        Ok(json!({"value":self.value}))
    }
    fn begin_transaction(&mut self) -> Result<()> {
        self.saved = Some(self.value);
        self.saved_panel_data = Some(self.panel_data.clone());
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
        self.panel_data = self.saved_panel_data.take().unwrap();
        self.rollbacks += 1;
        self.forms.clear();
        Ok(())
    }
    fn granted_folders(&self) -> Vec<FileGrant> {
        self.grants.clone()
    }
    fn granted_command_groups(&self) -> BTreeSet<String> {
        self.groups.clone()
    }
    fn ui_set_data(&mut self, panel: &str, updates: &[craft_plugin::ui::Datum]) -> Result<()> {
        assert_eq!(panel, "jobs");
        self.panel_data.set_data(updates)
    }
    fn ui_clear_data(&mut self, panel: &str) -> Result<()> {
        assert_eq!(panel, "jobs");
        self.panel_data.clear_data();
        Ok(())
    }
    fn ui_show_panel(&mut self, panel: &str, visible: bool) -> Result<()> {
        assert_eq!(panel, "jobs");
        self.panel_visible = visible;
        Ok(())
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
    let (_, version) = API.split_once('@').unwrap();
    component_for(version, code)
}
/// The command fixture built against WIT `version`: 0.1.0 has no `unsupported`.
fn component_for(version: &str, code: &str) -> Vec<u8> {
    let (package, _) = API.split_once('@').unwrap();
    let template = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version).replace("\r\n", "\n");
    let template = if version == "0.1.0" { template.replace(r#" (case "unsupported" string)"#, "") } else { template };
    let original = "      call $box\n      call $box\n";
    assert!(template.contains(original));
    wat::parse_str(template.replacen(original, &format!("      {code}\n"), 1)).unwrap()
}
fn panel_manifest() -> Manifest {
    let mut manifest = manifest();
    manifest.capabilities.push("ui@0.1".into());
    manifest.runtime.as_mut().unwrap().ui = Some(
        serde_json::from_value(json!({
            "ui_version":1,"panels":[{"id":"jobs","title":"Jobs","zones":["bottom"],"layout":{"root":0,"nodes":[{"widget":{"table":{
                "bind":"/jobs","row_id":"id","columns":[{"key":"state","label":"state","unit":null}],
                "select":"single","on_select":"make","on_edit":"make"
            }}}]}}],"workspaces":[]
        }))
        .unwrap(),
    );
    manifest
}
fn panel_component() -> Vec<u8> {
    let (package, version) = API.split_once('@').unwrap();
    wat::parse_str(
        include_str!("../ui_fixture.wat")
            .replace("__PACKAGE__", package)
            .replace("__VERSION__", version)
            .replace("__COMMAND__", "app.write")
            .replace("__COMMAND_LEN__", "9")
            .replace("__ARGS__", "{}")
            .replace("__ARGS_LEN__", "2"),
    )
    .unwrap()
}
#[test]
fn panel_table_updates_and_actions_share_the_ordinary_entry_transaction() {
    let manifest = panel_manifest();
    let component = panel_component();
    check_component(&manifest, &component, RuntimeOptions::default()).unwrap();
    let (app, declarations) = crate::describe_ui(App::default(), &manifest, &component);
    assert_eq!(declarations.unwrap(), manifest.runtime.as_ref().unwrap().ui.clone().unwrap());
    assert_eq!(app.begins, 0);
    let (mut app, result) = crate::run(app, &manifest, &component, "make", &json!({}));
    result.unwrap();
    assert_eq!(app.panel_data.get("/jobs/0/state"), None);
    app.panel_data.merge_frame();
    assert_eq!(app.panel_data.get("/jobs/0/state"), Some(&craft_plugin::ui::UiValue::Text("ready".into())));
    let event = craft_plugin::ui::UiEvent {
        panel: "jobs".into(),
        event: craft_plugin::ui::EventKind::Select,
        path: "/jobs".into(),
        row: Some("op1".into()),
        value: None,
    };
    let (mut app, result) = crate::run_ui_event(app, &manifest, &component, "make", &json!({}), &event);
    result.unwrap();
    app.panel_data.merge_frame();
    assert_eq!(app.panel_data.get("/jobs/0/state"), Some(&craft_plugin::ui::UiValue::Text("posted".into())));
    assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (2, 2, 2, 0));
}
#[test]
fn panel_imports_require_the_declared_ui_capability() {
    let (app, result) = crate::run(App::default(), &manifest(), &panel_component(), "make", &json!({}));
    assert!(matches!(result, Err(HostError::PermissionDenied { permission }) if permission.contains("ui")));
    assert_eq!((app.value, app.commits, app.rollbacks), (0, 0, 1));

    let mut undeclared = panel_manifest();
    undeclared.capabilities.retain(|capability| !capability.starts_with("ui@"));
    let (app, result) = crate::run(App::default(), &undeclared, &panel_component(), "make", &json!({}));
    assert!(matches!(result, Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));
    assert_eq!((app.value, app.begins), (0, 0));
    assert!(check_component(&undeclared, &panel_component(), RuntimeOptions::default()).is_err());
    let (_, result) = crate::describe_ui(App::default(), &undeclared, &panel_component());
    assert!(matches!(result, Err(HostError::InvalidManifest { field, .. }) if field == "capabilities"));

    let mut patch = panel_manifest();
    patch.capabilities.retain(|capability| !capability.starts_with("ui@"));
    patch.capabilities.push("ui@0.1.7".into());
    check_component(&patch, &panel_component(), RuntimeOptions::default()).unwrap();
}

#[test]
fn panel_provider_declarations_must_match_approved_manifest() {
    let (package, version) = API.split_once('@').unwrap();
    let component = wat::parse_str(
        include_str!("../ui_fixture.wat")
            .replace("__PACKAGE__", package)
            .replace("__VERSION__", version)
            .replace("__COMMAND__", "app.write")
            .replace("__COMMAND_LEN__", "9")
            .replace("__ARGS__", "{}")
            .replace("__ARGS_LEN__", "2")
            .replace("\"Jobs\"", "\"Evil\""),
    )
    .unwrap();
    let (app, error) = crate::run(App::default(), &panel_manifest(), &component, "make", &json!({}));
    assert!(matches!(error,Err(HostError::InvalidManifest {field,..}) if field=="runtime.ui"));
    assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (0, 1, 0, 1));
}
#[test]
fn reading_panel_declarations_has_no_command_authority_and_obeys_fuel() {
    let (package, version) = API.split_once('@').unwrap();
    let source = include_str!("../ui_fixture.wat")
        .replace("__PACKAGE__", package)
        .replace("__VERSION__", version)
        .replace("__COMMAND__", "app.write")
        .replace("__COMMAND_LEN__", "9")
        .replace("__ARGS__", "{}")
        .replace("__ARGS_LEN__", "2");
    let hostile = wat::parse_str(source.replace("(result i32) i32.const 3000)", "(result i32) call $box i32.const 3000)")).unwrap();
    let (app, result) = crate::describe_ui(App::default(), &panel_manifest(), &hostile);
    assert!(matches!(result, Err(HostError::PermissionDenied { .. })));
    assert_eq!((app.value, app.begins), (0, 0));
    let looping = wat::parse_str(source.replace("(result i32) i32.const 3000)", "(result i32) (loop $forever br $forever) i32.const 3000)")).unwrap();
    let (app, result) = crate::describe_ui(App::default(), &panel_manifest(), &looping);
    assert!(matches!(result, Err(HostError::Exhausted { limit, .. }) if limit == "fuel"));
    assert_eq!((app.value, app.begins), (0, 0));
}

#[test]
fn panel_invalid_updates_are_fatal_even_when_guest_ignores_the_host_error() {
    let (package, version) = API.split_once('@').unwrap();
    let component = wat::parse_str(
        include_str!("../ui_fixture.wat")
            .replace("__PACKAGE__", package)
            .replace("__VERSION__", version)
            .replace("__COMMAND__", "app.write")
            .replace("__COMMAND_LEN__", "9")
            .replace("__ARGS__", "{}")
            .replace("__ARGS_LEN__", "2")
            .replace("/jobs/0/state", "!jobs/0/state"),
    )
    .unwrap();
    let (app, result) = crate::run(App::default(), &panel_manifest(), &component, "make", &json!({}));
    assert!(matches!(result, Err(HostError::InvalidInput { .. })));
    assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (0, 1, 0, 1));
    assert!(app.panel_data.values().is_empty());
}

#[test]
fn ui_capability_requires_provider_exports() {
    assert!(check_component(&panel_manifest(), &component("call $box"), RuntimeOptions::default()).is_err());
}

#[test]
fn panel_plugin_supports_dialog_hooks() {
    let (package, version) = API.split_once('@').unwrap();
    let dialog_export = format!(
        "(export \"{package}/ui-provider@{version}\" (instance $provider))\n\
         (instance $dialog (export \"json\" (type $json)) (export \"plugin-error\" (type $plugin-error)) (export \"update-fields\" (func $execute)))\n\
         (export \"{package}/dialog-provider@{version}\" (instance $dialog))"
    );
    let target = format!("(export \"{package}/ui-provider@{version}\" (instance $provider))");
    let component = wat::parse_str(
        include_str!("../ui_fixture.wat")
            .replace("__PACKAGE__", package)
            .replace("__VERSION__", version)
            .replace("__COMMAND__", "document.inspect")
            .replace("__COMMAND_LEN__", "16")
            .replace("__ARGS__", "{}")
            .replace("__ARGS_LEN__", "2")
            .replace(&target, &dialog_export),
    )
    .unwrap();
    let mut manifest = panel_manifest();
    manifest.capabilities.push("dialog@0.1".into());
    manifest.runtime.as_mut().unwrap().commands.insert("document.inspect".into());
    manifest.runtime.as_mut().unwrap().entries[0].dialog_hook = true;
    check_component(&manifest, &component, RuntimeOptions::default()).unwrap();
    let (mut app, result) = crate::dialog_fields(App::default(), &manifest, &component, "make", &json!({}), craft_plugin::HostLimits::default());
    assert_eq!(result.unwrap(), "{\"bodies\":1,\"name\":\"Bracket\"}");
    assert_eq!((app.begins, app.commits, app.rollbacks), (1, 1, 0));
    app.panel_data.merge_frame();
    assert_eq!(app.panel_data.get("/jobs/0/state"), Some(&craft_plugin::ui::UiValue::Text("ready".into())));
}

fn form_component() -> Vec<u8> {
    form_component_with_dialog(false)
}

fn form_component_with_dialog(hook: bool) -> Vec<u8> {
    let (package, version) = API.split_once('@').unwrap();
    let template = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version);
    let (types, _) = template.split_once(&format!("  (type $ty-{package}/commands@{version}")).unwrap();
    let dialog = if hook {
        format!(
            r#"(instance $dialog (export "json" (type $json)) (export "plugin-error" (type $plugin-error)) (export "update-fields" (func $execute)))
      (export "{package}/dialog-provider@{version}" (instance $dialog))"#
        )
    } else {
        String::new()
    };
    wat::parse_str(format!(
        r#"{types}
      (type $form-type (instance
        (alias outer 1 $json (type $json)) (export "json" (type (eq $json)))
        (alias outer 1 $plugin-error (type $error)) (export "plugin-error" (type (eq $error)))
        (type $result (result (error $error)))
        (export "open" (func (param "entry" string) (param "params" $json) (result $result)))))
      (import "{package}/forms@{version}" (instance $forms (type $form-type)))
      (alias export $forms "open" (func $open))
      (core module $allocation
        (memory (export "memory") 1)
        (global $heap (mut i32) i32.const 4096)
        (func (export "realloc") (param i32 i32 i32 i32) (result i32)
          (local $at i32) global.get $heap local.tee $at local.get 3 i32.add global.set $heap local.get $at))
      (core instance $allocation (instantiate $allocation))
      (alias core export $allocation "memory" (core memory $memory))
      (alias core export $allocation "realloc" (core func $realloc))
      (core func $open (canon lower (func $open) (memory $memory) (realloc $realloc)))
      (core instance $imports (export "open" (func $open)) (export "memory" (memory $memory)))
      (core module $main
        (import "host" "open" (func $open (param i32 i32 i32 i32 i32)))
        (import "host" "memory" (memory 1))
        (data (i32.const 64) "make") (data (i32.const 96) "{{}}")
        (func (export "execute") (param i32 i32 i32 i32) (result i32)
          i32.const 64 i32.const 4 i32.const 96 i32.const 2 i32.const 0 call $open
          i32.const 0 i32.const 0 i32.store
          i32.const 4 i32.const 96 i32.store i32.const 8 i32.const 2 i32.store i32.const 0))
      (core instance $main (instantiate $main (with "host" (instance $imports))))
      (alias core export $main "execute" (core func $execute))
      (func $execute (param "entry" string) (param "params" $json) (result (result $json (error $plugin-error)))
        (canon lift (core func $execute) (memory $memory) (realloc $realloc)))
      (instance $entry (export "json" (type $json)) (export "plugin-error" (type $plugin-error)) (export "execute" (func $execute)))
      (export "{package}/entry@{version}" (instance $entry))
      {dialog})"#
    ))
    .unwrap()
}

#[test]
fn dialog_hook_cannot_open_forms_even_when_the_command_can() {
    let bytes = form_component_with_dialog(true);
    let mut m = manifest();
    m.capabilities.extend(["forms@0.1".into(), "dialog@0.1".into()]);
    m.runtime.as_mut().unwrap().entries[0].dialog_hook = true;
    crate::check_component(&m, &bytes).unwrap();
    let mut app = App { interactive: true, ..Default::default() };
    assert_eq!(run(&mut app, &m, &bytes, "make", &json!({})).unwrap(), json!({}));
    assert_eq!(app.forms.len(), 1, "ordinary interactive execution retains form requests");
    let (app, result) = crate::dialog_fields(app, &m, &bytes, "make", &json!({}), craft_plugin::HostLimits::default());
    assert!(matches!(result, Err(HostError::PermissionDenied { .. })), "a hook is read-only");
    assert!(app.forms.is_empty(), "the hook cannot enqueue forms");
}

#[test]
fn form_import_accepts_patch_capability_versions() {
    let bytes = form_component();
    for capability in ["forms@0.1", "forms@0.1.0"] {
        let mut m = manifest();
        m.capabilities.push(capability.into());
        let mut app = App { interactive: true, ..Default::default() };
        assert_eq!(run(&mut app, &m, &bytes, "make", &json!({})).unwrap(), json!({}));
        assert_eq!(app.forms, vec![("make".into(), json!({}))]);
    }
}

#[test]
fn form_import_refuses_missing_or_unsupported_capabilities() {
    let bytes = form_component();
    let mut m = manifest();
    let mut app = App { interactive: true, ..Default::default() };
    assert!(matches!(run(&mut app, &m, &bytes, "make", &json!({})), Err(HostError::PermissionDenied { .. })));
    m.capabilities.push("forms@0.2".into());
    assert!(matches!(craft_plugin::check_capabilities(&m.capabilities, "Test host"), Err(HostError::UnsupportedCapability { .. })));
    assert!(
        matches!(run(&mut app, &m, &bytes, "make", &json!({})), Err(HostError::UnsupportedCapability { .. })),
        "an unsupported forms version is refused"
    );
    assert!(app.forms.is_empty(), "refused capabilities cannot open forms");
}

#[test]
fn additive_form_import_links_and_only_interactive_calls_can_open() {
    let bytes = form_component();
    let mut m = manifest();
    let mut app = App { interactive: true, ..Default::default() };
    assert!(run(&mut app, &m, &bytes, "make", &json!({})).is_err());
    assert!(app.forms.is_empty(), "undeclared capability cannot open forms");
    m.capabilities.push("forms@0.1".into());
    assert_eq!(run(&mut app, &m, &bytes, "make", &json!({})).unwrap(), json!({}));
    assert_eq!(app.forms, vec![("make".into(), json!({}))]);
    let mut program = App::default();
    assert!(matches!(run(&mut program, &m, &bytes, "make", &json!({})), Err(HostError::InteractiveRequired { .. })));
    assert!(program.forms.is_empty());
    let mut failed = App { interactive: true, fail_commit: true, ..Default::default() };
    assert!(run(&mut failed, &m, &bytes, "make", &json!({})).is_err());
    assert!(failed.forms.is_empty(), "rollback discards deferred forms");
}

fn dialog_component(version: &str, code: &str, reply: &str) -> Vec<u8> {
    let source = include_str!("../dialog_fixture.wat")
        .replace("__PACKAGE__", "craft:plugin")
        .replace("__VERSION__", version)
        .replace("__HOOK_CODE__", code)
        .replace("__REPLY_LEN__", &reply.len().to_string())
        .replace("__REPLY__", &reply.replace('"', "\\22"));
    let source = if version == "0.1.0" { source.replace(r#" (case "unsupported" string)"#, "") } else { source };
    wat::parse_str(source).unwrap()
}

fn hook_manifest() -> Manifest {
    let mut m = manifest();
    m.capabilities.push("dialog@0.1".into());
    m.runtime.as_mut().unwrap().entries[0].dialog_hook = true;
    m
}

#[test]
fn dialog_hook_computes_defaults_looks_up_choices_and_old_worlds_still_link() {
    let reply = r#"{"width":{"default":0},"size":{"choices":["large"]}}"#;
    let digit = reply.find('0').unwrap() + 8192;
    let choice = reply.find("large").unwrap() + 8192;
    let code = format!(
        "i32.const {digit} local.get 2 i32.const 9 i32.add i32.load8_u i32.const 48 i32.sub i32.const 2 i32.mul i32.const 48 i32.add i32.store8
        local.get 2 i32.const 9 i32.add i32.load8_u i32.const 51 i32.lt_u
        if i32.const {choice} i32.const 1818324339 i32.store i32.const {} i32.const 108 i32.store8 end",
        choice + 4
    );
    for version in ["0.1.0", "0.2.0"] {
        let bytes = dialog_component(version, &code, reply);
        crate::check_component(&hook_manifest(), &bytes).unwrap();
        let (host, result) =
            crate::dialog_fields(App::default(), &hook_manifest(), &bytes, "make", &json!({"count":3}), craft_plugin::HostLimits::default());
        let patch: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(patch["width"]["default"], 6);
        assert_eq!(patch["size"]["choices"], json!(["large"]));
        assert_eq!(host.value, 0);
        let (_, result) =
            crate::dialog_fields(App::default(), &hook_manifest(), &bytes, "make", &json!({"count":2}), craft_plugin::HostLimits::default());
        let patch: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(patch["width"]["default"], 4);
        assert_eq!(patch["size"]["choices"], json!(["small"]));
        let mut app = App::default();
        let old = component_for(version, "call $box");
        crate::check_component(&manifest(), &old).unwrap();
        run(&mut app, &manifest(), &old, "make", &json!({})).unwrap();
        assert_eq!(app.value, 1);
    }
}

#[test]
fn dialog_hook_loops_traps_and_edit_attempts_stop_without_changing_the_host() {
    for (code, expected) in [("(loop $forever br $forever)", "exhausted"), ("unreachable", "trap"), ("call $box", "permission_denied")] {
        let bytes = dialog_component("0.2.0", code, "{}");
        let (host, result) = crate::dialog_fields(App::default(), &hook_manifest(), &bytes, "make", &json!({}), craft_plugin::HostLimits::default());
        let error = result.unwrap_err();
        assert_eq!(serde_json::to_value(&error).unwrap()["kind"], expected, "{error}");
        assert_eq!(host.value, 0);
    }
}

#[test]
fn dialog_hook_uses_its_own_fuel_ceiling_and_stops_memory_growth() {
    let bytes = dialog_component("0.2.0", "(loop $forever br $forever)", "{}");
    let (_, result) = crate::dialog_fields(
        App::default(),
        &hook_manifest(),
        &bytes,
        "make",
        &json!({}),
        craft_plugin::HostLimits { max_fuel: 77, max_memory_mib: 1 },
    );
    assert_eq!(result.unwrap_err(), HostError::Exhausted { limit: "fuel".into(), allowed: 77 });
    let bytes = dialog_component("0.2.0", "i32.const 10000 memory.grow drop", "{}");
    let (_, result) = crate::dialog_fields(App::default(), &hook_manifest(), &bytes, "make", &json!({}), craft_plugin::HostLimits::default());
    assert!(matches!(result, Err(HostError::Exhausted { limit, allowed:1 }) if limit == "memory_mib"));
}

#[test]
fn dialog_hook_export_requires_opt_in_and_a_declared_hook_requires_the_export() {
    assert!(crate::check_component(&manifest(), &dialog_component("0.2.0", "", "{}")).is_err());
    assert!(run(&mut App::default(), &manifest(), &dialog_component("0.2.0", "", "{}"), "make", &json!({})).is_err());
    assert!(crate::check_component(&hook_manifest(), &component("")).is_err());
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
        ("call $box (loop $forever br $forever)", "used all 100000 fuel it asks for and was stopped"),
        ("call $box i32.const 10000 memory.grow drop", "needed more than the 1 MiB of memory it asks for and was stopped"),
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
fn design_token_import_requires_its_capability_and_returns_identity_and_revision() {
    let mut manifest = manifest();
    manifest.runtime.as_mut().unwrap().entries[0].effect = Effect::Read;
    let component = design_token_component();
    let (_, result) = crate::run(App::default(), &manifest, &component, "make", &json!({}));
    assert!(matches!(result, Err(HostError::PermissionDenied { permission }) if permission.contains("design@0.1")));
    for capability in ["design@99.0", "design@bogus"] {
        manifest.capabilities = vec!["commands@0.1".into(), capability.into()];
        let (app, result) = crate::run(App::default(), &manifest, &component, "make", &json!({}));
        assert!(matches!(
            (capability, result),
            ("design@99.0", Err(HostError::UnsupportedCapability { .. })) | ("design@bogus", Err(HostError::InvalidManifest { .. }))
        ));
        assert_eq!(app.begins, 0);
    }
    for capability in ["design@0.1", "design@0.1.3"] {
        manifest.capabilities = vec!["commands@0.1".into(), capability.into()];
        let (_, result) = crate::run(App::default(), &manifest, &component, "make", &json!({}));
        assert_eq!(result.unwrap(), json!({"document_id":"design-one","revision":"0123456789abcdef0123456789abcdef:7"}));
    }
    let (_, result) = crate::run(App { no_design: true, ..Default::default() }, &manifest, &component, "make", &json!({}));
    assert_eq!(result.unwrap(), Value::Null);
}

fn design_token_component() -> Vec<u8> {
    wat::parse_str(design_token_source()).unwrap()
}

fn design_token_source() -> String {
    let (package, version) = API.split_once('@').unwrap();
    include_str!("../design_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)
}

#[test]
fn dialog_hooks_and_commands_share_the_design_token_import_without_changing_old_worlds() {
    let source = design_token_source();
    let (prefix, _) = source.rsplit_once(')').unwrap();
    let (package, version) = API.split_once('@').unwrap();
    let source = format!(
        r#"{prefix}
      (func $update (param "entry" string) (param "values" $json) (result (result $json (error $plugin-error)))
        (canon lift (core func ${package}/entry@{version}#execute) (memory $memory) (realloc $realloc) string-encoding=utf8))
      (instance $dialog (export "json" (type $json)) (export "plugin-error" (type $plugin-error)) (export "update-fields" (func $update)))
      (export "{package}/dialog-provider@{version}" (instance $dialog)))"#
    );
    let component = wat::parse_str(source).unwrap();
    for declared in [false, true] {
        let mut manifest = hook_manifest();
        if declared {
            manifest.capabilities.push("design@0.1".into());
        }
        crate::check_component(&manifest, &component).unwrap();
        for no_design in [false, true] {
            let (host, hook) = crate::dialog_fields(
                App { no_design, ..Default::default() },
                &manifest,
                &component,
                "make",
                &json!({}),
                craft_plugin::HostLimits::default(),
            );
            let (_, command) = crate::run(App { no_design, ..Default::default() }, &manifest, &component, "make", &json!({}));
            if declared {
                let expected =
                    if no_design { Value::Null } else { json!({"document_id":"design-one","revision":"0123456789abcdef0123456789abcdef:7"}) };
                assert_eq!(serde_json::from_str::<Value>(&hook.unwrap()).unwrap(), expected);
                assert_eq!(command.unwrap(), expected);
                assert_eq!((host.value, host.begins, host.commits, host.rollbacks), (0, 1, 1, 0));
            } else {
                for result in [hook.map(Value::String), command] {
                    assert!(matches!(result, Err(HostError::PermissionDenied { permission }) if permission.contains("design@0.1")));
                }
                assert_eq!((host.value, host.begins, host.commits, host.rollbacks), (0, 1, 0, 1));
            }
        }
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
    let source = format!("{prefix}\n(import \"{package}/storage@0.9.0\" (instance))\n)");
    let mut app = App::default();
    let failure = run(&mut app, &manifest(), &wat::parse_str(source).unwrap(), "make", &json!({})).unwrap_err();
    assert!(
        matches!(&failure, HostError::UnsupportedCapability { reason, .. } if reason.contains(&format!("built against {package}@0.9.0")) && reason.contains("update")),
        "{failure}"
    );
    // Refused while the component is checked, before any transaction starts: nothing to roll back.
    assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (0, 0, 0, 0));
    // A component naming two versions the host runs is refused too: it was built against neither.
    let source = format!("{prefix}\n(import \"{package}/storage@0.1.0\" (instance))\n)");
    let failure = run(&mut App::default(), &manifest(), &wat::parse_str(source).unwrap(), "make", &json!({})).unwrap_err();
    assert!(matches!(&failure, HostError::UnsupportedCapability { reason, .. } if reason.contains("mixes")), "{failure}");
    // A version named inside an exported instance counts like one at the top.
    let source = format!(
        "{prefix}\n(instance $inner)\n(instance $compat (export \"{package}/storage@0.9.0\" (instance $inner)))\n(export \"compat\" (instance $compat))\n)"
    );
    let failure = crate::check_component(&manifest(), &wat::parse_str(source).unwrap()).unwrap_err();
    assert!(
        matches!(&failure, HostError::UnsupportedCapability { reason, .. } if reason.contains(&format!("built against {package}@0.9.0"))),
        "{failure}"
    );
    // Past 4096 exports the scan stops with an error rather than leave the rest unread.
    let exports: String = (0..4097).map(|i| format!("(export \"e{i}\" (instance $inner))\n")).collect();
    let source = format!("{prefix}\n(instance $inner)\n{exports})");
    let failure = crate::check_component(&manifest(), &wat::parse_str(source).unwrap()).unwrap_err();
    assert_eq!(failure, HostError::LimitExceeded { limit: "plugin export interface count or depth".into() });
    // Below our scan budget, Wasmtime's own instance-index limit still becomes a host limit.
    let exports: String = (0..1001).map(|i| format!("(export \"e{i}\" (instance $inner))\n")).collect();
    let source = format!("{prefix}\n(instance $inner)\n{exports})");
    let failure = crate::check_component(&manifest(), &wat::parse_str(source).unwrap()).unwrap_err();
    assert!(matches!(&failure, HostError::LimitExceeded { limit } if limit.contains("instances count exceeds limit")), "{failure}");
    // An unsupported interface after those aliases is refused before validation hits that limit.
    let source = format!("{prefix}\n(instance $inner)\n{exports}(export \"{package}/storage@0.9.0\" (instance $inner)))");
    let failure = crate::check_component(&manifest(), &wat::parse_str(source).unwrap()).unwrap_err();
    assert!(
        matches!(&failure, HostError::UnsupportedCapability { reason, .. } if reason.contains(&format!("built against {package}@0.9.0")) && reason.contains("update")),
        "{failure}"
    );
}
#[test]
fn a_command_plugin_built_against_wit_0_1_still_runs() {
    let mut app = App::default();
    run(&mut app, &manifest(), &component_for("0.1.0", "call $box call $box"), "make", &json!({})).unwrap();
    assert_eq!((app.value, app.commits), (2, 1));
    let failure = run(&mut app, &manifest(), &component_for("0.1.0", "call $box (loop $forever br $forever)"), "make", &json!({})).unwrap_err();
    assert_eq!(failure, HostError::Exhausted { limit: "fuel".into(), allowed: 100_000 });
    assert_eq!((app.value, app.rollbacks), (2, 1));
}
#[test]
fn a_component_is_checked_against_its_world_without_running_it() {
    let check = |manifest: &Manifest, bytes: &[u8]| crate::check_component(manifest, bytes);
    // `unreachable` would trap if any code ran.
    check(&manifest(), &component("unreachable")).unwrap();
    check(&importer_manifest(false), &importer_component()).unwrap();
    // Each world needs its own export: a command component is no importer, and back.
    for (manifest, bytes) in [(importer_manifest(false), component("call $box")), (manifest(), importer_component())] {
        let failure = check(&manifest, &bytes).unwrap_err();
        assert!(matches!(&failure, HostError::InvalidInput { field, .. } if field == "component"), "{failure}");
    }
    let core = wat::parse_str("(module)").unwrap();
    assert!(matches!(check(&manifest(), &core), Err(HostError::InvalidInput { reason, .. }) if reason.contains("core WebAssembly module")));
    let (package, version) = API.split_once('@').unwrap();
    let source = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version);
    let (prefix, _) = source.rsplit_once(')').unwrap();
    let with_import = |interface: &str| wat::parse_str(format!("{prefix}\n(import \"{interface}\" (instance (export \"get\" (func))))\n)")).unwrap();
    let wasi = check(&manifest(), &with_import("wasi:cli/environment@0.2.0")).unwrap_err();
    assert!(matches!(&wasi, HostError::InvalidInput { reason, .. } if reason.contains("wasi:cli/environment")), "{wasi}");
    assert_eq!(
        check(&manifest(), &with_import("wasi:http/types@0.2.0")).unwrap_err(),
        HostError::PermissionDenied { permission: "network import is forbidden: wasi:http/types@0.2.0".into() }
    );
    assert!(matches!(check(&manifest(), &with_import(&format!("{package}/storage@0.9.0"))), Err(HostError::UnsupportedCapability { .. })));
    // A component built against 0.1 checks against the 0.1 world it runs in.
    check(&manifest(), &component_for("0.1.0", "call $box")).unwrap();
    check(&importer_manifest(false), &importer_component_for("0.1.0")).unwrap();
    let mut net = manifest();
    net.runtime.as_mut().unwrap().permissions.net = true;
    assert_eq!(check(&net, &component("call $box")).unwrap_err(), HostError::PermissionDenied { permission: "net".into() });
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
        design_tokens: false,
        folders: BTreeMap::new(),
        staged: BTreeMap::new(),
        created_dirs: Vec::new(),
        clock: false,
        forms: false,
        limits: StoreLimitsBuilder::new().build(),
        failed: None,
        calls: 0,
        max_calls: MAX_CALLS,
        log_bytes: 0,
        deadline: Instant::now() + Duration::from_secs(5),
        import: None,
        panels: None,
    }
}
#[test]
fn panel_imports_validate_typed_data_and_allow_read_entries_to_update_their_panels() {
    use panel_ui::Host;
    let mut state = state(App::default());
    state.effect = Effect::Read;
    state.panels = panel_manifest().runtime.unwrap().ui;
    state.set_data("jobs".into(), vec![types::Datum { path: "/jobs/0/state".into(), value: types::Value::Text("ready".into()) }]).unwrap();
    state.show_panel("jobs".into(), true).unwrap();
    assert!(state.host.panel_visible);
    state.host.panel_data.merge_frame();
    assert_eq!(state.host.panel_data.get("/jobs/0/state"), Some(&craft_plugin::ui::UiValue::Text("ready".into())));
    state.clear_data("jobs".into()).unwrap();
    state.host.panel_data.merge_frame();
    assert!(state.host.panel_data.values().is_empty());
    assert!(state.show_panel("unknown".into(), true).is_err());
    assert!(state.failed.is_some());

    let mut state = super::tests::state(App::default());
    state.panels = panel_manifest().runtime.unwrap().ui;
    assert!(state.set_data("jobs".into(), vec![types::Datum { path: "/number".into(), value: types::Value::Number(f64::NAN) }]).is_err());
    assert!(state.failed.is_some());
    assert!(state.host.panel_data.values().is_empty());
}

#[test]
fn form_import_requires_capability_and_refuses_programmatic_hosts() {
    let mut denied = state(App::default());
    assert!(matches!(forms::Host::open(&mut denied, "make".into(), "{}".into()), Err(PluginError::PermissionDenied(_))));
    let mut program = state(App::default());
    program.forms = true;
    assert!(forms::Host::open(&mut program, "make".into(), "{}".into()).is_err());
    assert!(matches!(program.failed, Some(HostError::InteractiveRequired { .. })));
    let mut hostile = state(App::default());
    hostile.forms = true;
    assert!(forms::Host::open(&mut hostile, "../entry".into(), "{}".into()).is_err());
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
        Folder {
            dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
            path: root.clone(),
            read: true,
            write: true,
            hidden: false,
            picked: None,
        },
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

fn temp_folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("craft-plugin-{name}-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    root
}
fn folder(root: &Path, hidden: bool, picked: Option<&str>) -> Folder {
    let dir = Dir::open_ambient_dir(root, cap_std::ambient_authority()).unwrap();
    Folder { dir, path: root.to_path_buf(), read: true, write: true, hidden, picked: picked.map(str::to_owned) }
}

#[test]
fn a_picked_folder_is_named_by_its_full_path_and_only_inside_one_of_the_grants_folders() {
    use files::Host;
    let (first, second, elsewhere) = (temp_folder("picked-a"), temp_folder("picked-b"), temp_folder("picked-c"));
    std::fs::write(second.join("lid.sc"), b"lid").unwrap();
    let mut state = state(App::default());
    state.folders.insert("repositories/0".into(), folder(&first, false, Some("repositories")));
    state.folders.insert("repositories/1".into(), folder(&second, false, Some("repositories")));
    let full = |root: &Path, name: &str| root.join(name).to_string_lossy().into_owned();
    assert_eq!(state.read_file("repositories".into(), full(&second, "lid.sc")).unwrap(), b"lid");
    state.write_file("repositories".into(), full(&first, "Robot/base.sc"), b"base".to_vec()).unwrap();
    state.publish_writes().unwrap();
    assert_eq!(std::fs::read(first.join("Robot/base.sc")).unwrap(), b"base");
    // A relative path, a folder not picked, or a way out of one names nothing.
    for path in ["lid.sc".to_string(), full(&elsewhere, "x.sc"), full(&first, "../escape.sc")] {
        assert!(state.read_file("repositories".into(), path.clone()).is_err(), "{path}");
    }
    drop(state);
    for root in [first, second, elsewhere] {
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn hidden_names_are_reachable_only_in_a_folder_whose_grant_says_hidden() {
    use files::Host;
    let root = temp_folder("hidden");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
    for shown in [false, true] {
        let grants = vec![FileGrant { name: "repo".into(), folder: root.clone(), read: true, write: true }];
        let app = App { grants, hidden: if shown { vec![root.clone()] } else { Vec::new() }, ..Default::default() };
        let mut state = state(app);
        state.folders.insert("repo".into(), folder(&root, shown, None));
        // (A refused call fails the run, so the listing comes first.)
        let listed = state.list_folder("repo".into(), String::new()).unwrap();
        assert_eq!(listed.contains(&".git/".to_string()), shown, "{listed:?}");
        let read = state.read_file("repo".into(), ".git/HEAD".into());
        assert_eq!(read.is_ok(), shown, "{read:?}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn read_part_list_folder_and_remove_file_see_the_runs_own_changes_and_publish_with_it() {
    use files::Host;
    let root = temp_folder("parts");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("pack"), b"0123456789").unwrap();
    std::fs::write(root.join("old.sc"), b"old").unwrap();
    let mut state = state(App::default());
    state.folders.insert("data".into(), folder(&root, false, None));
    assert_eq!(state.read_part("data".into(), "pack".into(), 3, 4).unwrap(), b"3456");
    assert_eq!(state.read_part("data".into(), "pack".into(), 8, 100).unwrap(), b"89", "fewer at the end");
    state.write_file("data".into(), "new.sc".into(), b"new".to_vec()).unwrap();
    state.remove_file("data".into(), "old.sc".into()).unwrap();
    assert_eq!(state.list_folder("data".into(), String::new()).unwrap(), ["new.sc", "pack", "sub/"]);
    assert!(state.read_file("data".into(), "old.sc".into()).is_err(), "removed for this run");
    assert!(root.join("old.sc").exists(), "nothing changes before the run is accepted");
    state.publish_writes().unwrap();
    assert!(!root.join("old.sc").exists());
    assert_eq!(std::fs::read(root.join("new.sc")).unwrap(), b"new");
    for refused in [true, false] {
        let mut state = self::state(App::default());
        state.folders.insert("data".into(), folder(&root, false, None));
        let failed = if refused {
            state.read_part("data".into(), "pack".into(), 0, u32::MAX)
        } else {
            state.remove_file("data".into(), "sub".into()).map(|()| Vec::new())
        };
        assert!(failed.is_err(), "more than 1 MiB at once, or a folder removed");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_home_file_is_read_only_when_the_manifest_names_it_and_never_written() {
    use files::Host;
    let home = temp_folder("home");
    std::fs::write(home.join(".gitconfig"), b"[user]\n\tname = Ada\n").unwrap();
    std::fs::write(home.join(".netrc"), b"secret").unwrap();
    let app = App { home: vec![(".gitconfig".into(), home.join(".gitconfig"))], ..Default::default() };
    let mut state = state(app);
    assert_eq!(state.read_file("~".into(), ".gitconfig".into()).unwrap(), b"[user]\n\tname = Ada\n");
    assert_eq!(state.read_part("~".into(), ".gitconfig".into(), 1, 4).unwrap(), b"user");
    let refused = state.read_file("~".into(), ".netrc".into()).unwrap_err();
    assert!(matches!(refused, PluginError::PermissionDenied(_)), "{refused:?}");
    let mut state = self::state(App { home: vec![(".gitconfig".into(), home.join(".gitconfig"))], ..Default::default() });
    assert!(state.write_file("~".into(), ".gitconfig".into(), b"x".to_vec()).is_err());
    assert!(state.list_folder("~".into(), String::new()).is_err());
    drop(state);
    // A named pipe where the file should be is refused at once, never waited on for a writer.
    #[cfg(unix)]
    {
        let pipe = home.join(".gitconfig-pipe");
        assert!(std::process::Command::new("mkfifo").arg(&pipe).status().unwrap().success());
        let mut state = self::state(App { home: vec![(".gitconfig".into(), pipe)], ..Default::default() });
        let started = std::time::Instant::now();
        assert!(state.read_file("~".into(), ".gitconfig".into()).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
    std::fs::remove_dir_all(home).unwrap();
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
            Folder {
                dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
                path: root.clone(),
                read: true,
                write: true,
                hidden: false,
                picked: None,
            },
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
    let limits = Limits { fuel: 100_000, memory_mib: 1, deadline_ms: 5000, stack_kib: None };
    store.set_fuel(limits.fuel).unwrap();
    store.set_epoch_deadline(1);
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance.get_typed_func::<(), ()>(&mut store, "run").unwrap();
    engine.increment_epoch();
    let failure = run.call(&mut store, ()).unwrap_err();
    let remaining = store.get_fuel().unwrap();
    assert!(remaining > 0);
    let message = execution_error(&failure, Some(remaining), &limits, 512 * 1024).to_string();
    assert!(message.contains("trap=Interrupt"), "{message}");
    assert!(!message.contains("OutOfFuel"), "{message}");
    assert!(message.contains(&format!("fuel_remaining={remaining}")), "{message}");
}

fn files_component(write: bool, swallow_error: bool) -> Vec<u8> {
    files_component_after(write, swallow_error, "")
}

fn files_component_after(write: bool, swallow_error: bool, after: &str) -> Vec<u8> {
    files_component_calls(&[write], swallow_error, after)
}

/// An entry that makes these file calls in order (`true`: write "changed" to `secret.txt` of the
/// grant `project`; `false`: read it), ignoring what each returns unless it is the last.
fn files_component_calls(writes: &[bool], swallow_error: bool, after: &str) -> Vec<u8> {
    wat::parse_str(files_component_source(writes, swallow_error, after)).unwrap()
}
fn files_component_source(writes: &[bool], swallow_error: bool, after: &str) -> String {
    let (package, version) = API.split_once('@').unwrap();
    let template = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version);
    // Keep exactly the shared WIT types, then exercise the real files interface.
    let (types, _) = template.split_once(&format!("  (type $ty-{package}/commands@{version}")).unwrap();
    let (mut exports, mut aliases, mut lowered, mut host, mut imports, mut calls) =
        (String::new(), String::new(), String::new(), String::new(), String::new(), String::new());
    for write in [false, true].into_iter().filter(|write| writes.contains(write)) {
        let (id, operation, result, parameters, core_parameters) = if write {
            ("write", "write-file", "(result (error $error))", "(param \"data\" (list u8))", "i32 i32 i32 i32 i32 i32 i32")
        } else {
            ("read", "read-file", "(result (list u8) (error $error))", "", "i32 i32 i32 i32 i32")
        };
        exports += &format!(
            "(type ${id}-result {result})\n(type ${id}-type (func (param \"grant\" string) (param \"path\" string) {parameters} (result ${id}-result)))\n(export \"{operation}\" (func (type ${id}-type)))\n"
        );
        aliases += &format!("  (alias export $files \"{operation}\" (func ${id}))\n");
        lowered += &format!("  (core func ${id}-core (canon lower (func ${id}) (memory $memory) (realloc $realloc)))\n");
        host += &format!("(export \"{id}\" (func ${id}-core)) ");
        imports += &format!("(import \"host\" \"{id}\" (func ${id} (param {core_parameters})))\n");
    }
    for write in writes {
        let (id, arguments) = if *write { ("write", "i32.const 128 i32.const 7") } else { ("read", "") };
        calls += &format!("  i32.const 64 i32.const 7 i32.const 96 i32.const 10 {arguments} i32.const 0 call ${id}\n");
    }
    let reply =
        if swallow_error { "i32.const 0 i32.const 0 i32.store i32.const 4 i32.const 160 i32.store i32.const 8 i32.const 2 i32.store" } else { "" };
    let source = format!(
        r#"{types}
  (type $files-type (instance
(alias outer 1 $plugin-error (type $error))
(export "plugin-error" (type (eq $error)))
{exports}  ))
  (import "{package}/files@{version}" (instance $files (type $files-type)))
{aliases}  (core module $allocation
(memory (export "memory") 1)
(global $heap (mut i32) i32.const 4096)
(func (export "realloc") (param i32 i32 i32 i32) (result i32)
  (local $at i32)
  global.get $heap local.tee $at local.get 3 i32.add global.set $heap local.get $at)
  )
  (core instance $allocation (instantiate $allocation))
  (alias core export $allocation "memory" (core memory $memory))
  (alias core export $allocation "realloc" (core func $realloc))
{lowered}  (core instance $imports {host}(export "memory" (memory $memory)))
  (core module $main
{imports}(import "host" "memory" (memory 1))
(data (i32.const 64) "project")
(data (i32.const 96) "secret.txt")
(data (i32.const 128) "changed")
(data (i32.const 160) "{{}}")
(func (export "execute") (param i32 i32 i32 i32) (result i32)
{calls}  {reply}
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
    source
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
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission {
        name: "project".into(),
        read: true,
        write: false,
        picked: false,
        hidden: false,
    });
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
fn dialog_hook_cannot_write_files_even_with_the_command_entries_approved_grants() {
    let source = files_component_source(&[true], true, "");
    let (prefix, _) = source.rsplit_once(')').unwrap();
    let source = format!(
        r#"{prefix}
      (func $update (param "entry" string) (param "values" $json) (result (result $json (error $plugin-error)))
        (canon lift (core func $execute) (memory $memory) (realloc $realloc)))
      (instance $dialog (export "json" (type $json)) (export "plugin-error" (type $plugin-error)) (export "update-fields" (func $update)))
      (export "craft:plugin/dialog-provider@0.2.0" (instance $dialog)))"#
    );
    let root = import_folder(&[("secret.txt", b"original")]);
    let mut manifest = hook_manifest();
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission {
        name: "project".into(),
        read: true,
        write: true,
        picked: false,
        hidden: false,
    });
    let app = App { grants: vec![FileGrant { name: "project".into(), folder: root.clone(), read: true, write: true }], ..Default::default() };
    let (_, result) = crate::dialog_fields(app, &manifest, &wat::parse_str(source).unwrap(), "make", &json!({}), craft_plugin::HostLimits::default());
    assert_eq!(result.unwrap_err(), craft_plugin::read_entry_refusal("write-file"));
    assert_eq!(std::fs::read(root.join("secret.txt")).unwrap(), b"original");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn component_file_writes_are_denied_without_a_host_grant() {
    for declared in [false, true] {
        let mut manifest = manifest();
        if declared {
            manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission {
                name: "project".into(),
                read: false,
                write: true,
                picked: false,
                hidden: false,
            });
        }
        let mut app = App { value: 7, ..Default::default() };
        let failure = run(&mut app, &manifest, &files_component(true, true), "make", &json!({})).unwrap_err();
        assert_eq!(failure, HostError::PermissionDenied { permission: "files.write:project".into() });
        assert_eq!((app.value, app.begins, app.commits, app.rollbacks), (7, 1, 0, 1));
    }
}

#[test]
fn a_read_entry_is_refused_write_file_even_with_a_write_grant() {
    let root = std::env::temp_dir().join(format!("craft-plugin-read-entry-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("secret.txt");
    std::fs::write(&target, b"original").unwrap();
    let mut manifest = manifest();
    let runtime = manifest.runtime.as_mut().unwrap();
    runtime.entries[0].effect = Effect::Read;
    runtime.permissions.files.push(FilePermission { name: "project".into(), read: true, write: true, picked: false, hidden: false });
    let mut app = App { grants: vec![FileGrant { name: "project".into(), folder: root.clone(), read: true, write: true }], ..Default::default() };
    // The guest ignores the refusal and returns normally: the call still fails, naming the import.
    let failure = run(&mut app, &manifest, &files_component(true, true), "make", &json!({})).unwrap_err();
    assert_eq!(failure, craft_plugin::read_entry_refusal("write-file"));
    assert!(failure.to_string().contains("write-file"), "{failure}");
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
    assert_eq!((app.commits, app.rollbacks), (0, 1));
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1, "nothing is staged");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn staged_file_writes_publish_after_commit_and_disappear_after_traps_or_commit_failure() {
    let root = std::env::temp_dir().join(format!("craft-plugin-staged-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("secret.txt");
    let mut manifest = manifest();
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission {
        name: "project".into(),
        read: false,
        write: true,
        picked: false,
        hidden: false,
    });
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
fn reading_a_missing_file_is_an_answer_and_the_entry_can_then_write_it() {
    // #17899: the first run of a plugin that keeps a file of its own has no file yet.
    use files::Host;
    let root = std::env::temp_dir().join(format!("craft-plugin-missing-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(root.join("folder")).unwrap();
    let mut state = state(App::default());
    state.folders.insert(
        "data".into(),
        Folder {
            dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
            path: root.clone(),
            read: true,
            write: true,
            hidden: false,
            picked: None,
        },
    );
    for path in ["jobs.json", "none/jobs.json"] {
        let missing = state.read_file("data".into(), path.into()).unwrap_err();
        assert!(
            matches!(&missing, PluginError::InvalidInput(types::FieldFailure { field, reason }) if field == "path" && reason == "no such file"),
            "{missing:?}"
        );
        assert_eq!(state.failed, None, "{path}");
    }
    state.write_file("data".into(), "jobs.json".into(), b"[]".to_vec()).unwrap();
    assert_eq!(state.read_file("data".into(), "jobs.json".into()).unwrap(), b"[]");
    assert_eq!(state.failed, None);
    // What was refused before still ends the run: a folder is not a file, and a path outside the grant.
    assert!(matches!(state.read_file("data".into(), "folder".into()), Err(PluginError::PermissionDenied(_))));
    assert!(matches!(state.failed.take(), Some(HostError::PermissionDenied { .. })));
    assert!(matches!(state.read_file("data".into(), "../jobs.json".into()), Err(PluginError::PermissionDenied(_))));
    assert_eq!(state.failed, Some(HostError::PermissionDenied { permission: "files.read:data".into() }));
    state.discard_writes();
    drop(state);

    // A whole entry: it reads the file that is not there, writes it, and the run publishes it.
    let target = root.join("secret.txt");
    let mut manifest = manifest();
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission {
        name: "project".into(),
        read: true,
        write: true,
        picked: false,
        hidden: false,
    });
    let component = files_component_calls(&[false, true], true, "");
    let mut app = App { grants: vec![FileGrant { name: "project".into(), folder: root.clone(), read: true, write: true }], ..Default::default() };
    assert_eq!(run(&mut app, &manifest, &component, "make", &json!({})).unwrap(), json!({}));
    assert_eq!(std::fs::read(&target).unwrap(), b"changed");
    assert_eq!((app.commits, app.rollbacks), (1, 0));
    // The same entry with no read access still fails its run, and writes nothing.
    std::fs::remove_file(&target).unwrap();
    let mut app = App { grants: vec![FileGrant { name: "project".into(), folder: root.clone(), read: false, write: true }], ..Default::default() };
    let failure = run(&mut app, &manifest, &component, "make", &json!({})).unwrap_err();
    assert_eq!(failure, HostError::PermissionDenied { permission: "files.read:project".into() });
    assert!(!target.exists());
    assert_eq!((app.commits, app.rollbacks), (0, 1));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_write_into_a_missing_sub_folder_makes_it_and_a_discarded_run_removes_it_again() {
    // #20493: `NewProject/network.json` in a granted folder with no `NewProject` yet.
    use files::Host;
    let root = std::env::temp_dir().join(format!("craft-plugin-subfolder-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("plain"), b"file").unwrap();
    let granted = |host: &mut State<App>| {
        host.folders.insert(
            "projects".into(),
            Folder {
                dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
                path: root.clone(),
                read: true,
                write: true,
                hidden: false,
                picked: None,
            },
        );
    };

    // Published: the folders stay with the file in them.
    let mut host = state(App::default());
    granted(&mut host);
    host.write_file("projects".into(), "NewProject/pipes/network.json".into(), b"{}".to_vec()).unwrap();
    assert_eq!(host.read_file("projects".into(), "NewProject/pipes/network.json".into()).unwrap(), b"{}");
    assert_eq!(host.failed, None);
    host.publish_writes().unwrap();
    host.discard_writes();
    assert_eq!(std::fs::read(root.join("NewProject/pipes/network.json")).unwrap(), b"{}");
    drop(host);

    // Not published (a trap, a refusal, a failed commit): nothing is left behind, and a folder
    // that was already there stays.
    let mut host = state(App::default());
    granted(&mut host);
    host.write_file("projects".into(), "NewProject/other/a/b.json".into(), b"{}".to_vec()).unwrap();
    assert!(root.join("NewProject/other/a").is_dir());
    host.discard_writes();
    assert!(!root.join("NewProject/other").exists());
    assert!(root.join("NewProject/pipes/network.json").exists());

    // A file where a folder should be is a wrong path, not a denied permission.
    let refused = host.write_file("projects".into(), "plain/network.json".into(), b"{}".to_vec()).unwrap_err();
    assert!(matches!(&refused, PluginError::InvalidInput(types::FieldFailure { field, .. }) if field == "path"), "{refused:?}");
    assert_eq!(std::fs::read(root.join("plain")).unwrap(), b"file");
    drop(host);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(unix)]
fn a_link_in_the_middle_of_a_new_folder_path_is_refused_before_anything_is_made() {
    // #20493: `link/New/x.json` where `link` points outside the grant makes no folder, inside or outside.
    use files::Host;
    let base = std::env::temp_dir().join(format!("craft-plugin-midlink-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    let root = base.join("grant");
    let outside = base.join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    for path in ["link/New/x.json", "link/New/Deeper/x.json"] {
        let mut state = state(App::default());
        state.folders.insert(
            "data".into(),
            Folder {
                dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
                path: root.clone(),
                read: true,
                write: true,
                hidden: false,
                picked: None,
            },
        );
        let refused = state.write_file("data".into(), path.into(), b"{}".to_vec()).unwrap_err();
        assert!(matches!(refused, PluginError::PermissionDenied(_)), "{path}: {refused:?}");
        assert!(state.created_dirs.is_empty() && state.staged.is_empty(), "{path}");
    }
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0, "nothing made outside the grant");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1, "nothing made inside it but the link");
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_path_of_more_than_64_components_is_refused_before_any_folder_is_made() {
    use files::Host;
    let root = std::env::temp_dir().join(format!("craft-plugin-deep-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    let mut state = state(App::default());
    state.folders.insert(
        "data".into(),
        Folder {
            dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
            path: root.clone(),
            read: true,
            write: true,
            hidden: false,
            picked: None,
        },
    );
    let deep = |folders: usize| format!("{}x.json", "d/".repeat(folders));
    // 63 folders and the file are 64 components: allowed.
    state.write_file("data".into(), deep(63), b"{}".to_vec()).unwrap();
    // 64 folders and the file are 65: refused as a wrong path, naming the limit.
    let refused = state.write_file("data".into(), deep(64).replacen("d/", "e/", 1), b"{}".to_vec()).unwrap_err();
    assert!(
        matches!(&refused, PluginError::InvalidInput(types::FieldFailure { field, reason }) if field == "path" && reason.contains("at most 64")),
        "{refused:?}"
    );
    assert!(!root.join("e").exists(), "no folder made for a refused path");
    state.discard_writes();
    drop(state);
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
                Folder {
                    dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
                    path: root.clone(),
                    read: true,
                    write: true,
                    hidden: false,
                    picked: None,
                },
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
    let folder = || Folder {
        dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
        path: root.clone(),
        read: true,
        write: true,
        hidden: false,
        picked: None,
    };
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
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission {
        name: "project".into(),
        read: false,
        write: true,
        picked: false,
        hidden: false,
    });
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
            Folder {
                dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(),
                path: root.clone(),
                read: true,
                write: true,
                hidden: false,
                picked: None,
            },
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

fn importer_manifest(input_folder: bool) -> Manifest {
    craft_plugin::parse_manifest(
        json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.1"],"runtime":{
            "module":"sample.wasm","commands":[],"entries":[{"name":"convert","kind":"importer","extensions":["smp"]}],
            "limits":{"fuel":10000000,"memory_mib":2,"deadline_ms":2000},"permissions":{"input_folder":input_folder}
        }})
        .to_string()
        .as_bytes(),
        "sample",
    )
    .unwrap()
}
fn importer_component() -> Vec<u8> {
    importer_component_for(API.split_once('@').unwrap().1)
}
/// The importer fixture built against WIT `version`: 0.1.0 has no `unsupported`.
fn importer_component_for(version: &str) -> Vec<u8> {
    let (package, _) = API.split_once('@').unwrap();
    let source = include_str!("../importer_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version);
    let source = if version == "0.1.0" { source.replace(r#" (case "unsupported" string)"#, "") } else { source };
    wat::parse_str(source).unwrap()
}
fn import(manifest: &Manifest, path: PathBuf, input_folder: bool) -> Result<crate::Imported> {
    import_with(&importer_component(), manifest, path, input_folder)
}
fn import_with(component: &[u8], manifest: &Manifest, path: PathBuf, input_folder: bool) -> Result<crate::Imported> {
    let request = crate::ImportRequest { path, input_folder };
    crate::run_import(App::default(), manifest, component, "convert", &json!({}), &request).1
}
fn import_folder(files: &[(&str, &[u8])]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("craft-plugin-import-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    for (name, bytes) in files {
        std::fs::write(root.join(name), bytes).unwrap();
    }
    root
}
#[test]
fn dialog_hook_on_an_importer_links_but_cannot_write_a_design() {
    let mut manifest = importer_manifest(false);
    manifest.capabilities.push("dialog@0.1".into());
    manifest.runtime.as_mut().unwrap().entries[0].dialog_hook = true;
    let source = include_str!("../importer_fixture.wat").replace("__PACKAGE__", "craft:plugin").replace("__VERSION__", "0.2.0");
    let source = source.replace(
        "    (func $import (;3;)",
        r#"
    (export "update-fields" (func $update))
    (func $update (param i32 i32 i32 i32) (result i32)
        i32.const 0 i32.const 0 i32.const 0 call $write
        i32.const 0 i32.const 0 i32.store
        i32.const 4 i32.const 1536 i32.store
        i32.const 8 i32.const 2 i32.store i32.const 0)
    (data (i32.const 1536) "{}")
    (func $import (;3;)"#,
    );
    let (prefix, _) = source.rsplit_once(')').unwrap();
    let source = format!(
        r##"{prefix}
      (alias core export $main "update-fields" (core func $update))
      (func $update (param "entry" string) (param "values" $json) (result (result $json (error $"#type6 plugin-error")))
        (canon lift (core func $update) (memory $memory) (realloc $cabi_realloc)))
      (instance $dialog (export "json" (type $json)) (export "plugin-error" (type $"#type6 plugin-error")) (export "update-fields" (func $update)))
      (export "craft:plugin/dialog-provider@0.2.0" (instance $dialog)))"##
    );
    let component = wat::parse_str(source).unwrap();
    crate::check_component(&manifest, &component).unwrap();
    let (_, result) = crate::dialog_fields(App::default(), &manifest, &component, "convert", &json!({}), craft_plugin::HostLimits::default());
    assert!(matches!(result, Err(HostError::PermissionDenied { permission }) if permission.contains("design")));
}

#[test]
fn an_importer_streams_its_file_into_a_design_and_returns_its_loss_report() {
    let root = import_folder(&[("bracket.smp", b"design bytes")]);
    let imported = import(&importer_manifest(false), root.join("bracket.smp"), false).unwrap();
    assert_eq!(imported.design, b"design bytes");
    assert_eq!(imported.report.summary, "Copied the file");
    assert_eq!(imported.report.items.len(), 1);
    assert_eq!(imported.report.items[0].severity, craft_plugin::LossSeverity::Dropped);
    // A command runtime cannot be run as an importer, nor an importer as a command.
    assert!(matches!(import(&manifest(), root.join("bracket.smp"), false), Err(HostError::InvalidInput { .. })));
    let mut app = App::default();
    assert!(matches!(run(&mut app, &importer_manifest(false), &importer_component(), "convert", &json!({})), Err(HostError::InvalidInput { .. })));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn reading_beside_the_file_needs_the_input_folder_permission_both_asked_and_granted() {
    let root = import_folder(&[("assembly.smp", b"S"), ("part.bin", b"part bytes"), (".hidden.smp", b"x")]);
    for (asked, granted) in [(false, true), (true, false)] {
        let failure = import(&importer_manifest(asked), root.join("assembly.smp"), granted).unwrap_err();
        assert!(matches!(&failure, HostError::PermissionDenied { permission } if permission.contains("input_folder")), "{failure}");
    }
    assert_eq!(import(&importer_manifest(true), root.join("assembly.smp"), true).unwrap().design, b"part bytes");
    // A hidden file is never imported.
    assert!(matches!(import(&importer_manifest(true), root.join(".hidden.smp"), true), Err(HostError::InvalidInput { .. })));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn an_importer_trap_or_runaway_loop_fails_that_one_call_and_the_next_call_works() {
    let root = import_folder(&[("trap.smp", b"T"), ("loop.smp", b"L"), ("fine.smp", b"fine")]);
    let manifest = importer_manifest(false);
    let failure = import(&manifest, root.join("trap.smp"), false).unwrap_err();
    assert!(matches!(&failure, HostError::Trap { reason } if reason.contains("UnreachableCodeReached")), "{failure}");
    assert_eq!(import(&manifest, root.join("loop.smp"), false).unwrap_err(), HostError::Exhausted { limit: "fuel".into(), allowed: 10_000_000 });
    assert_eq!(import(&manifest, root.join("fine.smp"), false).unwrap().design, b"fine");
    // With fuel to spare, the time limit stops it, and says how long that was.
    let mut slow = importer_manifest(false);
    let limits = &mut slow.runtime.as_mut().unwrap().limits;
    (limits.fuel, limits.deadline_ms) = (1 << 40, 50);
    assert_eq!(import(&slow, root.join("loop.smp"), false).unwrap_err(), HostError::Exhausted { limit: "deadline_ms".into(), allowed: 50 });
    std::fs::remove_dir_all(root).unwrap();
}
/// Runs `check` with every compile on this thread made by `tier`'s compiler, bypassing the cache.
fn on_tier(tier: Tier, check: impl FnOnce()) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FORCED_TIER.with(|forced| forced.set(None));
        }
    }
    let _reset = Reset;
    FORCED_TIER.with(|forced| forced.set(Some(tier)));
    check();
}
#[test]
fn fuel_memory_the_deadline_and_cancel_stop_plugins_under_both_compilers() {
    for tier in [Tier::Baseline, Tier::Optimized] {
        on_tier(tier, || {
            for (code, diagnostic) in [
                ("call $box (loop $forever br $forever)", "used all 100000 fuel it asks for and was stopped"),
                ("call $box i32.const 10000 memory.grow drop", "needed more than the 1 MiB of memory it asks for and was stopped"),
            ] {
                let failure = run(&mut App::default(), &manifest(), &component(code), "make", &json!({})).unwrap_err();
                assert!(failure.to_string().contains(diagnostic), "{tier:?} {code}: {failure}");
            }
            let root = import_folder(&[("loop.smp", b"L"), ("fine.smp", b"fine")]);
            let manifest = importer_manifest(false);
            let fuel = import(&manifest, root.join("loop.smp"), false).unwrap_err();
            assert_eq!(fuel, HostError::Exhausted { limit: "fuel".into(), allowed: 10_000_000 }, "{tier:?}");
            assert_eq!(import(&manifest, root.join("fine.smp"), false).unwrap().design, b"fine", "{tier:?}");
            let mut slow = importer_manifest(false);
            let limits = &mut slow.runtime.as_mut().unwrap().limits;
            (limits.fuel, limits.deadline_ms) = (1 << 40, 50);
            let deadline = import(&slow, root.join("loop.smp"), false).unwrap_err();
            assert_eq!(deadline, HostError::Exhausted { limit: "deadline_ms".into(), allowed: 50 }, "{tier:?}");
            std::fs::remove_dir_all(root).unwrap();
            let mut cancelled = App { cancelled: true, ..Default::default() };
            assert_eq!(
                run(&mut cancelled, &super::tests::manifest(), &component("(loop br 0)"), "make", &json!({})).unwrap_err(),
                HostError::Cancelled
            );
        });
    }
}
/// The cached code's compiler, if the cache holds `bytes`.
fn cached_tier(bytes: &[u8]) -> Option<Tier> {
    cache().compiled.lock().unwrap().as_ref().filter(|cached| cached.bytes.as_ref() == bytes).map(|cached| cached.tier)
}
#[test]
fn imports_never_wait_for_the_cranelift_swap_and_give_the_same_design_before_and_after() {
    let root = import_folder(&[("bracket.smp", b"design bytes")]);
    let manifest = importer_manifest(false);
    let component = importer_component();
    // Hold the background compile back: every import below runs while it has not finished.
    let gate = cache().gate.lock().unwrap();
    let first = import_with(&component, &manifest, root.join("bracket.smp"), false).unwrap();
    assert_eq!(cached_tier(&component), Some(Tier::Baseline));
    // The first import started the Cranelift compile; a second one now finds it running.
    assert!(cache().optimizing.load(Ordering::Acquire));
    assert!(optimize_in_background().is_none());
    let during = import_with(&component, &manifest, root.join("bracket.smp"), false).unwrap();
    assert_eq!(cached_tier(&component), Some(Tier::Baseline));
    drop(gate);
    while cache().optimizing.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    assert_eq!(cached_tier(&component), Some(Tier::Optimized));
    let after = import_with(&component, &manifest, root.join("bracket.smp"), false).unwrap();
    for imported in [&first, &during, &after] {
        assert_eq!((imported.design.as_slice(), imported.report.summary.as_str()), (&b"design bytes"[..], "Copied the file"));
    }
    // Already Cranelift's: nothing more to compile.
    assert!(optimize_in_background().is_none());
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn a_late_cranelift_compile_never_replaces_another_components_code() {
    let stack_bytes = 512 * 1024;
    let first: Arc<[u8]> = Arc::from(importer_component());
    let (engine, component) = compile_with(&first, stack_bytes, Tier::Baseline).unwrap();
    *cache().compiled.lock().unwrap() =
        Some(Compiled { bytes: first, stack_bytes, engine: engine.clone(), component: component.clone(), tier: Tier::Baseline });
    let gate = cache().gate.lock().unwrap();
    let worker = optimize_in_background().unwrap();
    // Another component (the same bytes in another allocation stand in for one) takes the cache
    // while the first one's Cranelift compile waits.
    let second: Arc<[u8]> = Arc::from(importer_component());
    *cache().compiled.lock().unwrap() = Some(Compiled { bytes: second.clone(), stack_bytes, engine, component, tier: Tier::Baseline });
    drop(gate);
    worker.join().unwrap();
    let cached = cache().compiled.lock().unwrap();
    let cached = cached.as_ref().unwrap();
    assert_eq!((cached.tier, Arc::ptr_eq(&cached.bytes, &second)), (Tier::Baseline, true));
}
#[test]
fn a_component_winch_refuses_is_compiled_with_cranelift_and_runs() {
    // Relaxed SIMD is one of the proposals Wasmtime's Winch does not compile.
    let code = "call $box v128.const i64x2 0 0 v128.const i64x2 0 0 v128.const i64x2 0 0 f32x4.relaxed_madd drop";
    let bytes = component(code);
    assert!(compile_with(&bytes, 512 * 1024, Tier::Baseline).is_err());
    let (_, _, tier) = compile_fresh(&bytes, 512 * 1024, None).unwrap();
    assert_eq!(tier, Tier::Optimized);
    run(&mut App::default(), &manifest(), &bytes, "make", &json!({})).unwrap();
    assert_eq!(cached_tier(&bytes), Some(Tier::Optimized));
    crate::check_component(&manifest(), &bytes).unwrap();
}
#[test]
fn an_importer_says_a_valid_file_is_not_supported() {
    let root = import_folder(&[("sketch.smp", b"U"), ("broken.smp", b"I")]);
    let failure = import(&importer_manifest(false), root.join("sketch.smp"), false).unwrap_err();
    assert_eq!(failure, HostError::Unsupported { reason: "the model has no solids".into() });
    assert_eq!(failure.to_string(), "not supported: the model has no solids");
    let failure = import(&importer_manifest(false), root.join("broken.smp"), false).unwrap_err();
    assert_eq!(failure, HostError::InvalidInput { field: "file".into(), reason: "the model has no solids".into() });
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn an_importer_built_against_wit_0_1_still_runs() {
    let root = import_folder(&[("bracket.smp", b"design bytes"), ("broken.smp", b"I"), ("loop.smp", b"L")]);
    let old = importer_component_for("0.1.0");
    let imported = import_with(&old, &importer_manifest(false), root.join("bracket.smp"), false).unwrap();
    assert_eq!((imported.design.as_slice(), imported.report.summary.as_str()), (&b"design bytes"[..], "Copied the file"));
    // Its errors read as they did: 0.1 had only invalid-input for a file it could not convert.
    let failure = import_with(&old, &importer_manifest(false), root.join("broken.smp"), false).unwrap_err();
    assert_eq!(failure, HostError::InvalidInput { field: "file".into(), reason: "the model has no solids".into() });
    let failure = import_with(&old, &importer_manifest(false), root.join("loop.smp"), false).unwrap_err();
    assert_eq!(failure, HostError::Exhausted { limit: "fuel".into(), allowed: 10_000_000 });
    std::fs::remove_dir_all(root).unwrap();
}

/// A long job runs past the command limits: it may make more than 256 host calls (progress
/// reports among them), and its deadline is the fixed job deadline, not the runtime's.
#[test]
fn a_long_job_runs_under_the_job_deadline_instead_of_the_command_limits() {
    let job = |job: bool| {
        craft_plugin::parse_manifest(
            json!({"id":"sample","name":"Sample","version":"1","capabilities":["commands@0.1"],"runtime":{
                "module":"plugin.wasm","entries":[{"name":"make","job":job}],
                "commands":["app.write"],"limits":{"fuel":u64::MAX / 2,"memory_mib":1,"deadline_ms":5000}
            }})
            .to_string()
            .as_bytes(),
            "sample",
        )
        .unwrap()
    };
    // 300 commands: one past the other at address 4000.
    let many = component(
        "(loop $more call $box i32.const 4000 i32.const 4000 i32.load i32.const 1 i32.add i32.store \
         i32.const 4000 i32.load i32.const 300 i32.lt_u br_if $more)",
    );
    let mut app = App::default();
    assert!(run(&mut app, &job(false), &many, "make", &json!({})).unwrap_err().to_string().contains("host call count"));
    assert_eq!(app.value, 0, "the command's calls are rolled back");
    run(&mut app, &job(true), &many, "make", &json!({})).unwrap();
    assert_eq!(app.value, 300);
    // A job that never ends stops at the job deadline and changes nothing.
    let options = RuntimeOptions { job_deadline_ms: 200, ..Default::default() };
    let (returned, result) =
        crate::run_with_options(std::mem::take(&mut app), &job(true), &component("call $box (loop br 0)"), "make", &json!({}), options);
    app = returned;
    assert_eq!(result.unwrap_err(), HostError::Exhausted { limit: "deadline_ms".into(), allowed: 200 });
    assert_eq!(app.value, 300);
}

fn exporter_manifest() -> Manifest {
    let slot = |name: &str| json!({"name":name,"kind":"exporter","extensions":["smp"]});
    craft_plugin::parse_manifest(
        json!({"id":"sample","name":"Sample","version":"1","capabilities":["formats@0.1"],"runtime":{
            "module":"sample.wasm","commands":["app.write","document.inspect","file.export"],
            "entries":[slot("copy"),slot("error"),slot("trap"),slot("loop"),slot("write")],
            "limits":{"fuel":10000000,"memory_mib":2,"deadline_ms":2000}
        }})
        .to_string()
        .as_bytes(),
        "sample",
    )
    .unwrap()
}
fn exporter_component() -> Vec<u8> {
    let (package, version) = API.split_once('@').unwrap();
    wat::parse_str(include_str!("../exporter_fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version)).unwrap()
}
fn export(app: App, slot: &str) -> (App, Result<Vec<u8>>) {
    crate::run_export(app, &exporter_manifest(), &exporter_component(), slot, &json!({}))
}
#[test]
fn an_exporter_reads_the_design_through_read_commands_and_returns_the_file() {
    let mut exporter = exporter_manifest();
    crate::check_component(&exporter, &exporter_component()).unwrap();
    // The compatibility linker must not accept a legacy command world as an export slot.
    assert!(crate::check_component(&exporter, &component_for("0.1.0", "call $box")).is_err());
    // Format slots may request a larger stack than commands, including during installation.
    exporter.runtime.as_mut().unwrap().limits.stack_kib = Some(2048);
    crate::check_component(&exporter, &exporter_component()).unwrap();
    let (app, file) = export(App::default(), "copy");
    let file: Value = serde_json::from_slice(&file.unwrap()).unwrap();
    assert_eq!(file, json!({"name":"Bracket","bodies":1}));
    // The export ran in a transaction that was thrown away, whatever its commands did.
    assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 1));
    // An exporter is not a command, and a command runtime is not an exporter.
    let mut app = App::default();
    assert!(matches!(run(&mut app, &exporter_manifest(), &exporter_component(), "copy", &json!({})), Err(HostError::InvalidInput { .. })));
    let (_, wrong) = crate::run_export(App::default(), &manifest(), &component("call $box"), "make", &json!({}));
    assert!(matches!(wrong, Err(HostError::InvalidInput { .. })));
}
#[test]
fn an_exporter_that_fails_traps_or_runs_away_returns_no_file_and_the_next_export_works() {
    let (_, failure) = export(App::default(), "error");
    assert!(matches!(&failure, Err(HostError::Trap { reason }) if reason.contains("refused this design")), "{failure:?}");
    let (_, failure) = export(App::default(), "trap");
    assert!(matches!(&failure, Err(HostError::Trap { reason }) if reason.contains("UnreachableCodeReached")), "{failure:?}");
    let (app, failure) = export(App::default(), "loop");
    assert!(matches!(&failure, Err(HostError::Exhausted { limit, .. }) if limit == "fuel"), "{failure:?}");
    assert_eq!(app.rollbacks, 1);
    assert!(export(App::default(), "copy").1.is_ok());
}
#[test]
fn an_exporter_reaches_no_file_even_through_a_command_its_manifest_lists() {
    // `file.export` is in the manifest's list, but it is in the `file` group: an exporter never
    // gets it, nor any command that edits.
    let (app, failure) = export(App { groups: ["file".to_string()].into(), ..App::default() }, "write");
    assert!(matches!(&failure, Err(HostError::PermissionDenied { permission }) if permission.contains("file.export")), "{failure:?}");
    assert_eq!(app.value, 0);
    // A component built for the command world (it imports `files`) does not link as an exporter.
    let (_, failure) = crate::run_export(App::default(), &exporter_manifest(), &files_component(true, false), "copy", &json!({}));
    assert!(failure.is_err());
}

/// An empty folder for one compile-cache test. Wasmtime's cache wants an absolute path.
fn cache_folder() -> PathBuf {
    let root = std::env::temp_dir().join(format!("craft-plugin-cache-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::canonicalize(root).unwrap()
}
/// The compiled entries Wasmtime's cache wrote under `root`, leaving out its usage-statistics files.
fn cache_entries(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(versions) = std::fs::read_dir(root.join("modules")) else { return found };
    for version in versions.flatten() {
        for entry in std::fs::read_dir(version.path()).unwrap().flatten() {
            if !entry.file_name().to_string_lossy().contains('.') {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}
fn compile_cached(bytes: &[u8], cache: &wasmtime::Cache) -> Result<(Engine, Component)> {
    compile_on(bytes, 512 * 1024, Tier::Optimized, true, Some(cache.clone()))
}
#[test]
fn a_compiled_component_is_read_back_from_the_disk_cache_by_a_new_engine() {
    let root = cache_folder();
    let cache = open_disk_cache(&root).unwrap();
    let bytes = importer_component();
    let (engine, component) = compile_cached(&bytes, &cache).unwrap();
    let first = check_imports(&engine, &component).unwrap();
    assert_eq!((cache.cache_hits(), cache.cache_misses(), cache_entries(&root).len()), (0, 1, 1));
    // Every compile makes a new engine, as a new session does: this one reads the entry back.
    let (engine, component) = compile_cached(&bytes, &cache).unwrap();
    assert_eq!(check_imports(&engine, &component).unwrap(), first);
    assert_eq!((cache.cache_hits(), cache.cache_misses(), cache_entries(&root).len()), (1, 1, 1));
    // Entries live under a folder named for the compiler and its version, so another Wasmtime never reads them.
    let versions: Vec<String> =
        std::fs::read_dir(root.join("modules")).unwrap().flatten().map(|v| v.file_name().to_string_lossy().into_owned()).collect();
    assert!(versions.len() == 1 && versions[0].starts_with("wasmtime-"), "{versions:?}");
    let _ = std::fs::remove_dir_all(root); // best effort: Wasmtime's cache worker may still be writing here
}
#[test]
fn a_changed_component_or_compile_setting_gets_its_own_cache_entry() {
    let root = cache_folder();
    let cache = open_disk_cache(&root).unwrap();
    compile_cached(&importer_component(), &cache).unwrap();
    // A plugin update: other bytes, another entry, never the old code.
    compile_cached(&component("call $box"), &cache).unwrap();
    assert_eq!((cache.cache_hits(), cache.cache_misses(), cache_entries(&root).len()), (0, 2, 2));
    // The same module under another setting is another entry too: the key covers the
    // configuration, fuel metering included.
    let module = wat::parse_str(r#"(module (func (export "run") (loop br 0)))"#).unwrap();
    for fuel in [true, false, true] {
        let mut config = Config::new();
        config.cache(Some(cache.clone())).consume_fuel(fuel).epoch_interruption(true);
        wasmtime::Module::new(&Engine::new(&config).unwrap(), &module).unwrap();
    }
    assert_eq!((cache.cache_hits(), cache.cache_misses(), cache_entries(&root).len()), (1, 4, 4));
    let _ = std::fs::remove_dir_all(root); // best effort: Wasmtime's cache worker may still be writing here
}
#[test]
fn fuel_still_stops_code_read_back_from_the_disk_cache() {
    let root = cache_folder();
    let cache = open_disk_cache(&root).unwrap();
    let module = wat::parse_str(r#"(module (func (export "run") (loop br 0)))"#).unwrap();
    for _ in 0..2 {
        let mut config = Config::new();
        config.cache(Some(cache.clone())).consume_fuel(true).epoch_interruption(true);
        let engine = Engine::new(&config).unwrap();
        let module = wasmtime::Module::new(&engine, &module).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        store.set_fuel(10_000).unwrap();
        store.set_epoch_deadline(u64::MAX);
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let run = instance.get_typed_func::<(), ()>(&mut store, "run").unwrap();
        let failure = run.call(&mut store, ()).unwrap_err();
        assert_eq!(failure.downcast_ref::<wasmtime::Trap>(), Some(&wasmtime::Trap::OutOfFuel));
    }
    assert_eq!((cache.cache_hits(), cache.cache_misses()), (1, 1));
    let _ = std::fs::remove_dir_all(root); // best effort: Wasmtime's cache worker may still be writing here
}
#[test]
fn a_damaged_or_mismatched_cache_entry_is_compiled_again() {
    let root = cache_folder();
    let cache = open_disk_cache(&root).unwrap();
    let bytes = importer_component();
    compile_cached(&bytes, &cache).unwrap();
    let [entry] = cache_entries(&root).try_into().unwrap();
    let good = std::fs::read(&entry).unwrap();
    // An entry compiled under another configuration (no fuel metering), put where this one's
    // key points: Wasmtime checks the settings recorded in it and refuses it.
    let other_root = cache_folder();
    let other = open_disk_cache(&other_root).unwrap();
    let mut config = Config::new();
    config.cache(Some(other)).consume_fuel(false).epoch_interruption(true).wasm_memory64(false).wasm_multi_memory(false).max_wasm_stack(512 * 1024);
    config.async_stack_size(512 * 1024 + (1 << 20));
    Component::from_binary(&Engine::new(&config).unwrap(), &bytes).unwrap();
    let [mismatched] = cache_entries(&other_root).try_into().unwrap();
    let mismatched = std::fs::read(mismatched).unwrap();
    for (damage, contents) in [
        ("not compressed data", b"not a cache entry".to_vec()),
        ("cut short", good[..good.len() / 2].to_vec()),
        ("empty", Vec::new()),
        ("another configuration's code", mismatched),
    ] {
        std::fs::write(&entry, &contents).unwrap();
        let misses = cache.cache_misses();
        let (engine, component) = compile_cached(&bytes, &cache).unwrap_or_else(|e| panic!("{damage}: {e}"));
        check_imports(&engine, &component).unwrap_or_else(|e| panic!("{damage}: {e}"));
        assert_eq!(cache.cache_misses(), misses + 1, "{damage}");
        // The compile wrote a good entry back in its place, which the next compile reads.
        let hits = cache.cache_hits();
        compile_cached(&bytes, &cache).unwrap();
        assert_eq!(cache.cache_hits(), hits + 1, "{damage}");
    }
    let _ = std::fs::remove_dir_all(root); // best effort: Wasmtime's cache worker may still be writing here
    let _ = std::fs::remove_dir_all(other_root); // best effort: Wasmtime's cache worker may still be writing here
}
#[test]
fn a_cache_folder_that_cannot_be_made_or_written_still_compiles() {
    let root = cache_folder();
    // A file where the folder should be: the cache can't be opened, and compiling works as before.
    let blocked = root.join("blocked");
    std::fs::write(&blocked, b"a file").unwrap();
    assert!(open_disk_cache(&blocked.join("plugin-compile")).is_none());
    compile_with(&importer_component(), 512 * 1024, Tier::Optimized).unwrap();
    // A folder that opens but can't be written: every compile still succeeds.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let read_only = root.join("read-only");
        std::fs::create_dir_all(&read_only).unwrap();
        let cache = open_disk_cache(&read_only).unwrap();
        std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o555)).unwrap();
        for _ in 0..2 {
            let (engine, component) = compile_cached(&importer_component(), &cache).unwrap();
            check_imports(&engine, &component).unwrap();
        }
        std::fs::set_permissions(&read_only, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _ = std::fs::remove_dir_all(root); // best effort: Wasmtime's cache worker may still be writing here
}

/// Gives this test thread's runtime a disk cache in a new folder, as the app's startup does.
/// Returns the folder Wasmtime's cache writes in, inside the app's folder, as the app lays it out.
fn with_disk_cache() -> (PathBuf, wasmtime::Cache) {
    let root = cache_folder().join("wasmtime");
    let disk = open_disk_cache(&root).unwrap();
    assert!(cache().disk.set(Some(disk.clone())).is_ok());
    (root, disk)
}
#[test]
fn a_cache_miss_starts_on_winch_and_caches_the_cranelift_code_for_the_next_session() {
    let (root, disk) = with_disk_cache();
    let folder = import_folder(&[("bracket.smp", b"design bytes")]);
    let component = importer_component();
    let gate = cache().gate.lock().unwrap();
    let first = import_with(&component, &importer_manifest(false), folder.join("bracket.smp"), false).unwrap();
    // Nothing cached yet: Winch's code, and Winch's code is never written to the disk cache.
    assert_eq!(cached_tier(&component), Some(Tier::Baseline));
    assert!(cache_entries(&root).is_empty());
    drop(gate);
    while cache().optimizing.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    // The background compile swapped in Cranelift's code and wrote it to the disk cache.
    assert_eq!(cached_tier(&component), Some(Tier::Optimized));
    assert_eq!((disk.cache_misses(), cache_entries(&root).len()), (1, 1));
    // A new session, which starts with nothing in memory, loads that entry: no Winch.
    *cache().compiled.lock().unwrap() = None;
    let next = import_with(&component, &importer_manifest(false), folder.join("bracket.smp"), false).unwrap();
    assert_eq!(cached_tier(&component), Some(Tier::Optimized));
    assert_eq!((disk.cache_hits(), disk.cache_misses()), (1, 1));
    assert_eq!((first.design, next.design), (b"design bytes".to_vec(), b"design bytes".to_vec()));
    std::fs::remove_dir_all(folder).unwrap();
    // Best effort: Wasmtime's cache worker may still be writing here.
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
#[test]
fn a_cache_hit_loads_cranelift_code_without_compiling_with_winch() {
    let (root, disk) = with_disk_cache();
    let component = importer_component();
    // An earlier session cached Cranelift's code for this component.
    optimize_and_cache(&component, 512 * 1024, true, Some(&disk)).unwrap();
    assert_eq!((disk.cache_hits(), disk.cache_misses()), (0, 1));
    let folder = import_folder(&[("bracket.smp", b"design bytes")]);
    let imported = import_with(&component, &importer_manifest(false), folder.join("bracket.smp"), false).unwrap();
    assert_eq!(imported.design, b"design bytes");
    // Read from the disk cache, Cranelift's from the start: nothing left to optimise.
    assert_eq!(cached_tier(&component), Some(Tier::Optimized));
    assert_eq!((disk.cache_hits(), disk.cache_misses()), (1, 1));
    assert!(optimize_in_background().is_none());
    // A deleted entry is only a slower start: Cranelift compiles it again and caches it.
    for entry in cache_entries(&root) {
        std::fs::remove_file(entry).unwrap();
    }
    *cache().compiled.lock().unwrap() = None;
    import_with(&component, &importer_manifest(false), folder.join("bracket.smp"), false).unwrap();
    assert_eq!((cached_tier(&component), disk.cache_misses(), cache_entries(&root).len()), (Some(Tier::Optimized), 2, 1));
    std::fs::remove_dir_all(folder).unwrap();
    // Best effort: Wasmtime's cache worker may still be writing here.
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
