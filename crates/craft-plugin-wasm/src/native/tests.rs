use super::*;

use craft_plugin::{API, FileGrant, FilePermission};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Default)]
struct App {
    user_data: BTreeMap<String, Value>,
    saved_user_data: Option<BTreeMap<String, Value>>,
    cancel_after_set: bool,
    value: usize,
    saved: Option<usize>,
    begins: usize,
    commits: usize,
    rollbacks: usize,
    grants: Vec<FileGrant>,
    input_grants: Vec<FileGrant>,
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
}
impl PluginHost for App {
    fn open_form(&mut self, entry: &str, params: &Value) -> Result<()> {
        if !self.interactive {
            return Err(HostError::InteractiveRequired { entry: entry.into() });
        }
        self.forms.push((entry.into(), params.clone()));
        Ok(())
    }
    fn user_storage_get(&mut self, key: &str) -> Result<Option<Value>> {
        Ok(self.user_data.get(key).cloned())
    }
    fn user_storage_set(&mut self, key: &str, value: &Value) -> Result<()> {
        let mut data = self.user_data.clone();
        data.insert(key.into(), value.clone());
        craft_plugin::check_user_storage(&data)?;
        self.user_data = data;
        self.cancelled |= self.cancel_after_set;
        Ok(())
    }
    fn user_storage_remove(&mut self, key: &str) -> Result<bool> {
        Ok(self.user_data.remove(key).is_some())
    }
    fn user_storage_list(&mut self) -> Result<Vec<String>> {
        Ok(self.user_data.keys().cloned().collect())
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
        self.saved_user_data = Some(self.user_data.clone());
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
        self.saved_user_data = None;
        self.commits += 1;
        Ok(())
    }
    fn rollback_transaction(&mut self) -> Result<()> {
        self.value = self.saved.take().unwrap();
        self.user_data = self.saved_user_data.take().unwrap();
        self.panel_data = self.saved_panel_data.take().unwrap();
        self.rollbacks += 1;
        self.forms.clear();
        Ok(())
    }
    fn granted_input_folders(&self) -> Vec<FileGrant> {
        self.input_grants.clone()
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

/// Add a guest that calls the query while instantiating and traps unless every returned byte
/// matches. It uses the fixture's memory and allocator within the one-memory sandbox limit.
fn with_capability_query(component: &str, expected: &[&str]) -> Vec<u8> {
    let query = r#"
  (import "craft:host/capabilities@0.1.0" (instance $host-query
    (type $capability-list (list string))
    (export "granted" (func (result $capability-list)))))
  (alias export $host-query "granted" (func $granted))
  (core func $granted-lowered (canon lower (func $granted)
    (memory $memory) (realloc __REALLOC__) string-encoding=utf8))
  (core module $query-probe
    (import "query" "granted" (func $granted (param i32)))
    (import "query" "memory" (memory 1))
    (func $check
      i32.const 128 call $granted
      __ASSERTIONS__)
    (start $check))
  (core instance $query-imports
    (export "granted" (func $granted-lowered))
    (export "memory" (memory $memory)))
  (core instance $query-probe (instantiate $query-probe
    (with "query" (instance $query-imports))))
"#;
    let mut assertions = format!("i32.const 132 i32.load i32.const {} i32.ne if unreachable end\n", expected.len());
    for (index, capability) in expected.iter().enumerate() {
        let offset = index * 8;
        assertions.push_str(&format!(
            "i32.const 128 i32.load i32.const {} i32.add i32.load i32.const {} i32.ne if unreachable end\n",
            offset + 4,
            capability.len()
        ));
        for (at, byte) in capability.bytes().enumerate() {
            assertions.push_str(&format!(
                "i32.const 128 i32.load i32.const {offset} i32.add i32.load \
                 i32.const {at} i32.add i32.load8_u i32.const {byte} i32.ne if unreachable end\n"
            ));
        }
    }
    let (package, version) = API.split_once('@').unwrap();
    // Keep the fixture's chosen API version, including 0.1's smaller error variant.
    let component = component.replace("__PACKAGE__", package).replace("__VERSION__", version);
    let end = component.rfind(')').unwrap();
    let realloc = if component.contains("(core func $cabi_realloc") { "$cabi_realloc" } else { "$realloc" };
    let query = query.replace("__ASSERTIONS__", &assertions).replace("__REALLOC__", realloc);
    let combined = format!("{}{}\n)", &component[..end], query);
    wat::parse_str(combined).unwrap()
}

#[test]
fn command_guests_query_only_requested_supported_capabilities_at_both_api_versions() {
    for version in ["0.1.0", "0.2.0"] {
        let mut manifest = manifest();
        manifest.capabilities.push("host@0.1".into());
        manifest.optional_capabilities = vec!["storage@0.1".into(), "forms@0.1".into(), "commands-new@0.1".into(), "dialog@0.1".into()];
        manifest.runtime.as_mut().unwrap().data_format = Some(1);
        let source = include_str!("../fixture.wat").replace("__VERSION__", version);
        let source = if version == "0.1.0" { source.replace(r#" (case "unsupported" string)"#, "") } else { source };
        let guest = with_capability_query(&source, &["commands@0.1", "host@0.1", "storage@0.1", "forms@0.1", "dialog@0.1"]);
        crate::check_component(&manifest, &guest).unwrap();
        let mut app = App::default();
        run(&mut app, &manifest, &guest, "make", &json!({})).unwrap();
        assert_eq!((app.value, app.commits), (2, 1));
        manifest.optional_capabilities.clear();
        // A previously supported optional capability disappearing changes the reply.
        assert!(run(&mut app, &manifest, &guest, "make", &json!({})).is_err());
        manifest.capabilities.pop();
        assert!(matches!(crate::check_component(&manifest, &guest), Err(HostError::InvalidInput { field, .. }) if field == "component"));
    }
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

fn form_component() -> Vec<u8> {
    form_component_with_dialog(false)
}

fn form_component_with_dialog(hook: bool) -> Vec<u8> {
    wat::parse_str(form_source_with_dialog(hook)).unwrap()
}

fn form_source_with_dialog(hook: bool) -> String {
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
    format!(
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
    )
}

#[test]
fn optional_forms_match_the_capability_query_and_remain_unavailable_to_hooks() {
    for (capability, supported) in [("forms@0.1", true), ("forms@0.1.3", true), ("forms@0.2", false)] {
        let mut manifest = hook_manifest();
        manifest.capabilities.push("host@0.1".into());
        manifest.optional_capabilities = vec![capability.into()];
        let mut expected = vec!["commands@0.1", "dialog@0.1", "host@0.1"];
        if supported {
            expected.push(capability);
        }
        let bytes = with_capability_query(&form_source_with_dialog(true), &expected);
        crate::check_component(&manifest, &bytes).unwrap();
        let app = App { interactive: true, ..Default::default() };
        let (app, result) = crate::run(app, &manifest, &bytes, "make", &json!({}));
        if supported {
            assert_eq!(result.unwrap(), json!({}));
            assert_eq!(app.forms, vec![("make".into(), json!({}))]);
            let (_, result) = crate::run(App::default(), &manifest, &bytes, "make", &json!({}));
            assert!(matches!(result, Err(HostError::InteractiveRequired { .. })));
        } else {
            assert!(matches!(result, Err(HostError::PermissionDenied { .. })));
            assert!(app.forms.is_empty());
        }
        let (app, result) = crate::dialog_fields(app, &manifest, &bytes, "make", &json!({}), craft_plugin::HostLimits::default());
        assert!(matches!(result, Err(HostError::PermissionDenied { .. })));
        assert!(app.forms.is_empty(), "a read-only hook cannot enqueue forms");
    }
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
    let mut m = manifest();
    m.capabilities.push("forms@0.1.0".into());
    let mut app = App { interactive: true, ..Default::default() };
    assert_eq!(run(&mut app, &m, &bytes, "make", &json!({})).unwrap(), json!({}));
    assert_eq!(app.forms, vec![("make".into(), json!({}))]);
    let _ = m.capabilities.pop();
    assert!(matches!(run(&mut app, &m, &bytes, "make", &json!({})), Err(HostError::PermissionDenied { .. })));
    m.capabilities.push("forms@0.2".into());
    assert!(matches!(craft_plugin::check_capabilities(&m.capabilities, "Test host"), Err(HostError::UnsupportedCapability { .. })));
    assert!(run(&mut app, &m, &bytes, "make", &json!({})).is_err());
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

fn storage_hook_component(importer: bool, code: &str) -> Vec<u8> {
    let template = if importer { include_str!("../importer_storage_fixture.wat") } else { include_str!("../user_storage_fixture.wat") };
    let hook = format!(
        r#"    (data (i32.const 4096) "{{}}")
    (func (export "update-fields") (param i32 i32 i32 i32) (result i32)
      {code}
      i32.const 0 i32.const 0 i32.store
      i32.const 4 i32.const 4096 i32.store
      i32.const 8 i32.const 2 i32.store
      i32.const 0)
    (func (export "execute")"#
    );
    let source = template.replace("__CODE__", "").replace("    (func (export \"execute\")", &hook);
    let (prefix, _) = source.rsplit_once(')').unwrap();
    wat::parse_str(format!(
        r#"{prefix}
  (alias core export $main "update-fields" (core func $update-core))
  (func $update (param "entry" string) (param "values" $json) (result (result $json (error $error)))
    (canon lift (core func $update-core) (memory $memory) (realloc $realloc)))
  (instance $dialog (export "json" (type $json)) (export "plugin-error" (type $error)) (export "update-fields" (func $update)))
  (export "craft:plugin/dialog-provider@0.2.0" (instance $dialog)))"#
    ))
    .unwrap()
}

#[test]
fn command_and_importer_dialog_hooks_read_user_settings_but_cannot_set_or_remove_them() {
    let read = "i32.const 512 i32.const 5 i32.const 128 call $get
        i32.const 128 i32.load if unreachable end
        i32.const 132 i32.load i32.const 1 i32.ne if unreachable end
        i32.const 140 i32.load i32.const 2 i32.ne if unreachable end
        i32.const 136 i32.load i32.load16_u i32.const 12852 i32.ne if unreachable end";
    for importer in [false, true] {
        let mut manifest = if importer { storage_importer_manifest() } else { user_storage_manifest() };
        manifest.capabilities.push("dialog@0.1".into());
        manifest.runtime.as_mut().unwrap().entries[0].dialog_hook = true;
        let entry = if importer { "convert" } else { "make" };
        for (code, writing) in [
            (read, None),
            (USER_SET, Some("user-storage.set")),
            ("i32.const 512 i32.const 5 i32.const 128 call $remove", Some("user-storage.remove")),
        ] {
            let bytes = storage_hook_component(importer, code);
            crate::check_component(&manifest, &bytes).unwrap();
            let saved = BTreeMap::from([("hinge".into(), json!(42))]);
            let app = App { user_data: saved.clone(), ..Default::default() };
            let (app, result) = crate::dialog_fields(app, &manifest, &bytes, entry, &json!({}), craft_plugin::HostLimits::default());
            if let Some(import) = writing {
                assert_eq!(result.unwrap_err(), craft_plugin::read_entry_refusal(import));
                assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 1));
            } else {
                assert_eq!(result.unwrap(), "{}");
                assert_eq!((app.begins, app.commits, app.rollbacks), (1, 1, 0));
            }
            assert_eq!(app.user_data, saved);
        }
    }
}

#[test]
fn dialog_hooks_query_capabilities_at_both_api_versions_without_gaining_edit_access() {
    for version in ["0.1.0", "0.2.0"] {
        let mut manifest = hook_manifest();
        manifest.capabilities.push("host@0.1".into());
        manifest.optional_capabilities = vec!["future@0.1".into()];
        for code in ["", "call $box"] {
            let source = include_str!("../dialog_fixture.wat")
                .replace("__VERSION__", version)
                .replace("__HOOK_CODE__", code)
                .replace("__REPLY_LEN__", "2")
                .replace("__REPLY__", "{}");
            let source = if version == "0.1.0" { source.replace(r#" (case "unsupported" string)"#, "") } else { source };
            let bytes = with_capability_query(&source, &["commands@0.1", "dialog@0.1", "host@0.1"]);
            crate::check_component(&manifest, &bytes).unwrap();
            let (host, result) = crate::dialog_fields(App::default(), &manifest, &bytes, "make", &json!({}), craft_plugin::HostLimits::default());
            if code.is_empty() {
                assert_eq!(result.unwrap(), "{}");
                assert_eq!((host.value, host.commits, host.rollbacks), (0, 1, 0));
            } else {
                assert!(matches!(result, Err(HostError::PermissionDenied { .. })));
                assert_eq!((host.value, host.commits, host.rollbacks), (0, 0, 1));
            }
            manifest.capabilities.pop();
            assert!(matches!(crate::check_component(&manifest, &bytes), Err(HostError::InvalidInput { field, .. }) if field == "component"));
            manifest.capabilities.push("host@0.1".into());
        }
    }
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
fn importer_guests_query_optional_capabilities_without_gaining_file_grants() {
    let root = import_folder(&[("bracket.smp", b"design bytes"), ("assembly.smp", b"S"), ("part.bin", b"a part")]);
    for version in ["0.1.0", "0.2.0"] {
        let mut manifest = importer_manifest(false);
        manifest.capabilities.push("host@0.1".into());
        manifest.optional_capabilities = vec!["evaluate@0.1".into(), "evaluate-next@0.1".into(), "forms@0.1".into()];
        let source = include_str!("../importer_fixture.wat").replace("__VERSION__", version);
        let source = if version == "0.1.0" { source.replace(r#" (case "unsupported" string)"#, "") } else { source };
        let guest = with_capability_query(&source, &["formats@0.1", "host@0.1", "evaluate@0.1", "forms@0.1"]);
        crate::check_component(&manifest, &guest).unwrap();
        let imported = import_with(&guest, &manifest, root.join("bracket.smp"), false).unwrap();
        assert_eq!(imported.design, b"design bytes");
        let failure = import_with(&guest, &manifest, root.join("assembly.smp"), true).unwrap_err();
        assert!(matches!(failure, HostError::PermissionDenied { permission } if permission.contains("input_folder")));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn capability_query_obeys_cancellation_and_host_call_limits() {
    let mut state = state(App::default());
    state.granted_capabilities = vec!["commands@0.1".into()];
    assert_eq!(capabilities::Host::granted(&mut state), ["commands@0.1"]);
    state.calls = MAX_CALLS;
    assert!(capabilities::Host::granted(&mut state).is_empty());
    assert_eq!(state.failed, Some(limit("host call count")));
    let mut state = self::state(App { cancelled: true, ..App::default() });
    assert!(capabilities::Host::granted(&mut state).is_empty());
    assert_eq!(state.failed, Some(HostError::Cancelled));
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

#[test]
fn capability_query_and_design_token_import_agree_on_optional_design_support() {
    for (capability, supported) in [("design@0.1", true), ("design@0.1.3", true), ("design@99.0", false)] {
        let mut manifest = manifest();
        manifest.capabilities.push("host@0.1".into());
        manifest.optional_capabilities = vec![capability.into(), "future@0.1".into()];
        manifest.runtime.as_mut().unwrap().entries[0].effect = Effect::Read;
        let mut expected = vec!["commands@0.1", "host@0.1"];
        if supported {
            expected.push(capability);
        }
        let component = with_capability_query(include_str!("../design_fixture.wat"), &expected);
        crate::check_component(&manifest, &component).unwrap();
        let (app, result) = crate::run(App::default(), &manifest, &component, "make", &json!({}));
        if supported {
            assert_eq!(result.unwrap(), json!({"document_id":"design-one","revision":"0123456789abcdef0123456789abcdef:7"}));
            assert_eq!((app.commits, app.rollbacks), (1, 0));
        } else {
            assert!(matches!(result, Err(HostError::PermissionDenied { permission }) if permission.contains("design@0.1")));
            assert_eq!((app.commits, app.rollbacks), (0, 1));
        }
    }
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
        input_folders: BTreeMap::new(),
        folder_bytes: 0,
        folder_files: BTreeSet::new(),
        host: app,
        commands: BTreeMap::new(),
        groups: BTreeSet::new(),
        granted_capabilities: Vec::new(),
        effect: Effect::Edit,
        user_storage: false,
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
    folder_or_files_component(writes, swallow_error, after, false)
}
fn files_component_source(writes: &[bool], swallow_error: bool, after: &str) -> String {
    folder_or_files_source(writes, swallow_error, after, false)
}
fn folder_or_files_component(writes: &[bool], swallow_error: bool, after: &str, folder: bool) -> Vec<u8> {
    wat::parse_str(folder_or_files_source(writes, swallow_error, after, folder)).unwrap()
}
fn folder_or_files_source(writes: &[bool], swallow_error: bool, after: &str, folder: bool) -> String {
    let (package, version) = API.split_once('@').unwrap();
    let interface = if folder { "folders" } else { "files" };
    let template = include_str!("../fixture.wat").replace("__PACKAGE__", package).replace("__VERSION__", version);
    // Keep exactly the shared WIT types, then exercise the real files interface.
    let (types, _) = template.split_once(&format!("  (type $ty-{package}/commands@{version}")).unwrap();
    let (mut exports, mut aliases, mut lowered, mut host, mut imports, mut calls) =
        (String::new(), String::new(), String::new(), String::new(), String::new(), String::new());
    for write in [false, true].into_iter().filter(|write| writes.contains(write)) {
        let (id, operation, result, parameters, core_parameters) = if write {
            ("write", "write-file", "(result (error $error))", "(param \"data\" (list u8))", "i32 i32 i32 i32 i32 i32 i32")
        } else if folder {
            ("read", "read", "(result (list u8) (error $error))", "(param \"offset\" u64) (param \"len\" u32)", "i32 i32 i32 i32 i64 i32 i32")
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
        let (id, arguments) =
            if *write { ("write", "i32.const 128 i32.const 7") } else { ("read", if folder { "i64.const 0 i32.const 1048576" } else { "" }) };
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
  (import "{package}/{interface}@{version}" (instance $files (type $files-type)))
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
fn dialog_hooks_link_folder_imports_and_only_read_host_approved_roots() {
    let source = folder_or_files_source(&[false], false, "", true);
    let (prefix, _) = source.rsplit_once(')').unwrap();
    let source = format!(
        r#"{prefix}
      (func $update (param "entry" string) (param "values" $json) (result (result $json (error $plugin-error)))
        (canon lift (core func $execute) (memory $memory) (realloc $realloc)))
      (instance $dialog (export "json" (type $json)) (export "plugin-error" (type $plugin-error)) (export "update-fields" (func $update)))
      (export "craft:plugin/dialog-provider@0.2.0" (instance $dialog)))"#
    );
    let component = wat::parse_str(source).unwrap();
    let root = import_folder(&[("secret.txt", b"{}")]);
    let mut manifest = hook_manifest();
    manifest.capabilities.push("folders@0.1".into());
    crate::check_component(&manifest, &component).unwrap();
    let (_, denied) = crate::dialog_fields(App::default(), &manifest, &component, "make", &json!({}), craft_plugin::HostLimits::default());
    assert!(matches!(denied, Err(HostError::PermissionDenied { .. })));
    let app = App { input_grants: vec![FileGrant { name: "project".into(), folder: root.clone(), read: true, write: false }], ..Default::default() };
    let (host, result) = crate::dialog_fields(app, &manifest, &component, "make", &json!({}), craft_plugin::HostLimits::default());
    assert_eq!(result.unwrap(), "{}");
    assert_eq!(host.value, 0);
    assert_eq!(std::fs::read(root.join("secret.txt")).unwrap(), b"{}");
    std::fs::remove_dir_all(root).unwrap();
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
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission { name: "project".into(), read: true, write: true });
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
            manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission { name: "project".into(), read: false, write: true });
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
    runtime.permissions.files.push(FilePermission { name: "project".into(), read: true, write: true });
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
fn reading_a_missing_file_is_an_answer_and_the_entry_can_then_write_it() {
    // #17899: the first run of a plugin that keeps a file of its own has no file yet.
    use files::Host;
    let root = std::env::temp_dir().join(format!("craft-plugin-missing-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(root.join("folder")).unwrap();
    let mut state = state(App::default());
    state.folders.insert(
        "data".into(),
        Folder { dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(), path: root.clone(), read: true, write: true },
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
    manifest.runtime.as_mut().unwrap().permissions.files.push(FilePermission { name: "project".into(), read: true, write: true });
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
            Folder { dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(), path: root.clone(), read: true, write: true },
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
fn storage_importer_manifest() -> Manifest {
    let mut manifest = importer_manifest(false);
    manifest.capabilities.push("user-storage@0.1".into());
    manifest
}
fn storage_importer_component(code: &str) -> Vec<u8> {
    wat::parse_str(include_str!("../importer_storage_fixture.wat").replace("__CODE__", code)).unwrap()
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
fn storage_importers_read_previous_successful_imports_and_leave_commit_to_acceptance() {
    let root = import_folder(&[("bracket.smp", b"design bytes")]);
    let request = crate::ImportRequest { path: root.join("bracket.smp"), input_folder: false };
    let manifest = storage_importer_manifest();
    let component = storage_importer_component(USER_SET);
    crate::check_component(&manifest, &component).unwrap();
    let (mut app, imported) = crate::run_import(App::default(), &manifest, &component, "convert", &json!({}), &request);
    assert_eq!(imported.unwrap().design, b"design bytes");
    assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 0));
    assert!(app.saved_user_data.as_ref().unwrap().is_empty(), "preferences are pending until acceptance");
    app.commit_transaction().unwrap();
    let read = "i32.const 512 i32.const 5 i32.const 128 call $get
        i32.const 128 i32.load if unreachable end
        i32.const 132 i32.load i32.const 1 i32.ne if unreachable end
        i32.const 140 i32.load i32.const 2 i32.ne if unreachable end
        i32.const 136 i32.load i32.load16_u i32.const 12852 i32.ne if unreachable end
        i32.const 128 call $list
        i32.const 128 i32.load if unreachable end
        i32.const 136 i32.load i32.const 1 i32.ne if unreachable end
        i32.const 132 i32.load i32.load offset=4 i32.const 5 i32.ne if unreachable end";
    let (mut app, imported) = crate::run_import(app, &manifest, &storage_importer_component(read), "convert", &json!({}), &request);
    imported.unwrap();
    app.commit_transaction().unwrap();
    let remove = "i32.const 512 i32.const 5 i32.const 128 call $remove
        i32.const 128 i32.load if unreachable end
        i32.const 132 i32.load i32.eqz if unreachable end";
    let (mut app, imported) = crate::run_import(app, &manifest, &storage_importer_component(remove), "convert", &json!({}), &request);
    imported.unwrap();
    assert!(app.user_data.is_empty());
    // A declined design restores the preferences even though guest execution succeeded.
    app.rollback_transaction().unwrap();
    assert_eq!(app.user_data["hinge"], json!(42));
    assert_eq!((app.begins, app.commits, app.rollbacks), (3, 2, 1));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn storage_importers_rollback_traps_guest_errors_invalid_reports_empty_designs_and_cancellation() {
    let root = import_folder(&[("bracket.smp", b"design bytes"), ("empty.smp", b"")]);
    let manifest = storage_importer_manifest();
    for (file, tail, cancel) in [
        ("bracket.smp", "unreachable", false),
        ("bracket.smp", "i32.const 0 i32.const 1 i32.store i32.const 4 i32.const 6 i32.store i32.const 0 return", false),
        ("bracket.smp", "i32.const 1024 i32.const 120 i32.store8", false),
        ("empty.smp", "", false),
        ("bracket.smp", "", true),
    ] {
        let request = crate::ImportRequest { path: root.join(file), input_folder: false };
        let app = App { user_data: BTreeMap::from([("previous".into(), json!(true))]), cancel_after_set: cancel, ..Default::default() };
        let component = storage_importer_component(&format!("{USER_SET} {tail}"));
        let (app, outcome) = crate::run_import(app, &manifest, &component, "convert", &json!({}), &request);
        assert!(outcome.is_err(), "{file}, {tail}, cancel={cancel}");
        assert_eq!(app.user_data, BTreeMap::from([("previous".into(), json!(true))]));
        assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 1));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn storage_importers_require_capability_for_every_storage_call() {
    let root = import_folder(&[("bracket.smp", b"design bytes")]);
    let request = crate::ImportRequest { path: root.join("bracket.smp"), input_folder: false };
    for code in [
        USER_SET,
        "i32.const 512 i32.const 5 i32.const 128 call $get",
        "i32.const 512 i32.const 5 i32.const 128 call $remove",
        "i32.const 128 call $list",
    ] {
        let (app, outcome) =
            crate::run_import(App::default(), &importer_manifest(false), &storage_importer_component(code), "convert", &json!({}), &request);
        assert_eq!(outcome.unwrap_err(), HostError::PermissionDenied { permission: "user-storage@0.1 is not requested".into() });
        assert!(app.user_data.is_empty());
        assert_eq!((app.begins, app.commits, app.rollbacks), (0, 0, 0));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn storage_importers_share_key_json_and_total_storage_limits() {
    let root = import_folder(&[("bracket.smp", b"design bytes")]);
    let request = crate::ImportRequest { path: root.join("bracket.smp"), input_folder: false };
    let manifest = storage_importer_manifest();
    for (code, field) in [
        ("i32.const 512 i32.const 0 i32.const 128 call $get", "user-storage.key"),
        ("i32.const 512 i32.const 129 i32.const 128 call $get", "user-storage.key"),
        ("i32.const 512 i32.const 5 i32.const 512 i32.const 5 i32.const 128 call $set", "user-storage.value"),
    ] {
        let (app, outcome) = crate::run_import(App::default(), &manifest, &storage_importer_component(code), "convert", &json!({}), &request);
        assert!(matches!(outcome, Err(HostError::InvalidInput { field: actual, .. }) if actual == field));
        assert!(app.user_data.is_empty());
        assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 1));
    }
    let oversized_input = "i32.const 20 memory.grow i32.const -1 i32.eq if unreachable end
        i32.const 2050 i32.const 32 i32.const 1048575 memory.fill
        i32.const 512 i32.const 5 i32.const 2048 i32.const 1048577 i32.const 128 call $set";
    let (app, outcome) = crate::run_import(App::default(), &manifest, &storage_importer_component(oversized_input), "convert", &json!({}), &request);
    assert_eq!(outcome.unwrap_err(), HostError::LimitExceeded { limit: "user-storage JSON input bytes (1048576)".into() });
    assert!(app.user_data.is_empty());
    assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 1));
    for (data, limit) in [
        ((0..craft_plugin::MAX_USER_STORAGE_KEYS).map(|i| (format!("k{i}"), json!(null))).collect(), "user-storage keys (128)"),
        (BTreeMap::from([("large".into(), json!("x".repeat(craft_plugin::MAX_USER_STORAGE_BYTES - 14)))]), "user-storage total bytes (65536)"),
    ] {
        let app = App { user_data: data, ..Default::default() };
        let original = app.user_data.clone();
        let (app, outcome) = crate::run_import(app, &manifest, &storage_importer_component(USER_SET), "convert", &json!({}), &request);
        assert_eq!(outcome.unwrap_err(), HostError::LimitExceeded { limit: limit.into() });
        assert_eq!(app.user_data, original);
        assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 1));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_importer_worlds_load_and_run_without_host_transactions() {
    let root = import_folder(&[("bracket.smp", b"design bytes")]);
    let request = crate::ImportRequest { path: root.join("bracket.smp"), input_folder: false };
    let manifest = importer_manifest(false);
    for version in craft_plugin::WIT_VERSIONS {
        let component = importer_component_for(version);
        crate::check_component(&manifest, &component).unwrap();
        let (app, outcome) = crate::run_import(App::default(), &manifest, &component, "convert", &json!({}), &request);
        assert_eq!(outcome.unwrap().design, b"design bytes");
        assert_eq!((app.begins, app.commits, app.rollbacks), (0, 0, 0));
    }
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

/// Extend the existing original importer fixture with the additive dialog export. The hook
/// echoes context by default; actions can read a JSON header instead, trap or attempt writes.
fn importer_dialog_component(action: &str) -> Vec<u8> {
    wat::parse_str(importer_dialog_source(action)).unwrap()
}

fn importer_dialog_source(action: &str) -> String {
    let (package, version) = API.split_once('@').unwrap();
    let source = include_str!("../importer_fixture.wat");
    let hook = format!(
        r#"
    (export "dialog" (func $dialog))
    (func $dialog (param i32 i32 i32 i32) (result i32)
      {action}
      i32.const 80 i32.const 0 i32.store
      i32.const 84 local.get 2 i32.store
      i32.const 88 local.get 3 i32.store
      i32.const 80)
    (func $import"#
    );
    let source = source.replacen("(func $import (;3;)", &format!("{hook} (;3;)"), 1);
    let (prefix, _) = source.rsplit_once(')').unwrap();
    format!(
        r##"{prefix}
      (alias core export $main "dialog" (core func $dialog-core))
      (type $dialog-result (result $json (error $"#type6 plugin-error")))
      (type $dialog-type (func (param "entry" string) (param "context" $json) (result $dialog-result)))
      (func $update (type $dialog-type) (canon lift (core func $dialog-core) (memory $memory) (realloc $cabi_realloc) string-encoding=utf8))
      (instance $dialog-export
        (export "json" (type $json))
        (export "plugin-error" (type $"#type6 plugin-error"))
        (export "update-fields" (func $update)))
      (export "__PACKAGE__/dialog-provider@__VERSION__" (instance $dialog-export))
    )"##
    )
    .replace("__PACKAGE__", package)
    .replace("__VERSION__", version)
}

fn dialog_manifest() -> Manifest {
    let mut manifest = importer_manifest(true);
    manifest.capabilities = vec!["formats@0.2".into(), "dialog@0.1".into()];
    manifest
}

#[test]
fn dialog_hooks_read_only_a_bounded_prefix_and_never_start_an_import_transaction() {
    let header = br#"{"fields":{"part":{"enum":["plate","cover"],"default":"plate"}}}"#;
    let root = import_folder(&[("header.smp", header), ("part.bin", b"private")]);
    let request = ImportRequest { path: root.join("header.smp"), input_folder: true };
    let read = r#"
      i32.const 0 i32.const 0 i64.const 0 i32.const 65536 i32.const 16 call $read
      i32.const 16 i32.load8_u if i32.const 16 return end
      i32.const 20 i32.load local.set 2
      i32.const 24 i32.load local.set 3
    "#;
    let component = importer_dialog_component(read);
    crate::check_component(&dialog_manifest(), &component).unwrap();
    let (host, fields) = crate::update_fields(App::default(), &dialog_manifest(), &component, "convert", &json!({}), &request);
    assert_eq!(fields.unwrap(), serde_json::from_slice::<Value>(header).unwrap());
    assert_eq!((host.begins, host.commits, host.rollbacks, host.value), (0, 0, 0, 0));
    for action in [
        // A read past the prefix, repeated reads over the total budget, a sibling despite the
        // install grant, a design write, a trap, and an infinite guest loop.
        "i32.const 0 i32.const 0 i64.const 65536 i32.const 1 i32.const 16 call $read",
        "i32.const 0 i32.const 0 i64.const 0 i32.const 65536 i32.const 16 call $read i32.const 0 i32.const 0 i64.const 0 i32.const 1 i32.const 16 call $read",
        "i32.const 1024 i32.const 8 i64.const 0 i32.const 1 i32.const 16 call $read",
        "i32.const 0 i32.const 0 i32.const 16 call $write",
        "unreachable",
        "(loop $forever br $forever)",
    ] {
        let result = crate::update_fields(App::default(), &dialog_manifest(), &importer_dialog_component(action), "convert", &json!({}), &request).1;
        assert!(result.is_err(), "{action}: {result:?}");
    }
    assert!(crate::update_fields(App::default(), &importer_manifest(false), &component, "convert", &json!({}), &request).1.is_err());
    let mut required = dialog_manifest();
    required.runtime.as_mut().unwrap().entries[0].dialog_hook = true;
    assert!(crate::check_component(&required, &importer_component()).is_err(), "an explicit hook requires its export");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dialog_context_is_data_and_legacy_importers_keep_loading() {
    let root = import_folder(&[("file.smp", b"header")]);
    let request = ImportRequest { path: root.join("file.smp"), input_folder: false };
    let context = json!({"values":{"mode":"parts"},"file":{"name":"file.smp","extension":"smp","size":6,"format":"sample.convert","generation":7}});
    let result = crate::update_fields(App::default(), &dialog_manifest(), &importer_dialog_component(""), "convert", &context, &request).1.unwrap();
    assert_eq!(result, context);
    for version in ["0.1.0", "0.2.0"] {
        crate::check_component(&importer_manifest(false), &importer_component_for(version)).unwrap();
        // A capability alone is optional in the general-hook manifest. An importer from main
        // without an opted-in hook must still load, with or without the advertised capability.
        crate::check_component(&dialog_manifest(), &importer_component_for(version)).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn file_aware_importer_hooks_query_supported_optional_capabilities() {
    let root = import_folder(&[("file.smp", b"header")]);
    let request = ImportRequest { path: root.join("file.smp"), input_folder: false };
    let context = json!({"values":{},"file":{"name":"file.smp","extension":"smp","size":6,"format":"sample.convert","generation":1}});
    let mut manifest = importer_manifest(false);
    manifest.capabilities.push("host@0.1".into());
    manifest.optional_capabilities = vec!["dialog@0.1".into(), "evaluate@0.1".into(), "future@0.1".into()];
    let guest = with_capability_query(&importer_dialog_source(""), &["formats@0.1", "host@0.1", "dialog@0.1", "evaluate@0.1"]);
    crate::check_component(&manifest, &guest).unwrap();
    let (host, result) = crate::update_fields(App::default(), &manifest, &guest, "convert", &context, &request);
    assert_eq!(result.unwrap(), context);
    assert_eq!((host.begins, host.commits, host.rollbacks), (0, 0, 0));
    manifest.optional_capabilities[0] = "dialog@0.2".into();
    assert!(crate::check_component(&manifest, &guest).is_err(), "an unsupported optional dialog capability cannot authorize its export");
    let failure = crate::update_fields(App::default(), &manifest, &guest, "convert", &context, &request).1.unwrap_err();
    assert!(matches!(failure, HostError::PermissionDenied { .. }));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_optional_folder_interface_links_and_reads_only_host_approved_roots() {
    let root = import_folder(&[("secret.txt", b"{}")]);
    let mut manifest = manifest();
    manifest.capabilities = vec!["commands@0.2".into(), "folders@0.1".into()];
    let component = folder_or_files_component(&[false], false, "", true);
    crate::check_component(&manifest, &component).unwrap();
    let mut app = App::default();
    assert!(matches!(run(&mut app, &manifest, &component, "make", &json!({})), Err(HostError::PermissionDenied { .. })));
    app.input_grants.push(FileGrant { name: "project".into(), folder: root.clone(), read: true, write: false });
    assert_eq!(run(&mut app, &manifest, &component, "make", &json!({})).unwrap(), json!({}));
    manifest.capabilities.retain(|c| !c.starts_with("folders@"));
    assert!(matches!(run(&mut app, &manifest, &component, "make", &json!({})), Err(HostError::PermissionDenied { .. })));
    manifest.optional_capabilities = vec!["folders@0.1".into()];
    assert_eq!(run(&mut app, &manifest, &component, "make", &json!({})).unwrap(), json!({}));
    manifest.optional_capabilities = vec!["folders@0.2".into()];
    assert!(matches!(run(&mut app, &manifest, &component, "make", &json!({})), Err(HostError::PermissionDenied { .. })));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn folder_reads_confine_paths_resolve_case_and_share_the_import_budgets() {
    use folders::Host as _;
    let root = import_folder(&[("Part.BIN", b"abcdef"), (".secret", b"hidden")]);
    std::fs::create_dir_all(root.join("parts")).unwrap();
    std::fs::write(root.join("parts/sub.bin"), b"sub").unwrap();
    let make = || {
        let mut state = state(App::default());
        state.input_folders.insert(
            "library".into(),
            Folder { dir: Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap(), path: root.clone(), read: true, write: false },
        );
        state
    };
    let mut allowed = make();
    assert_eq!(allowed.read("library".into(), "Part.BIN".into(), 2, 3).unwrap(), b"cde");
    assert_eq!(allowed.read("library".into(), "parts/sub.bin".into(), 0, 4).unwrap(), b"sub");
    assert_eq!(allowed.size("library".into(), "Part.BIN".into()).unwrap(), Some(6));
    assert_eq!(allowed.size("library".into(), "absent".into()).unwrap(), None);
    assert_eq!(allowed.resolve("library".into(), "part.bin".into()).unwrap(), Some("Part.BIN".into()));
    assert_eq!(allowed.resolve("library".into(), "".into()).unwrap(), Some("".into()));
    assert_eq!(allowed.resolve("library".into(), "absent".into()).unwrap(), None);
    for path in ["../outside", "/outside", ".secret", "parts/../../outside", "C:\\outside", "parts\\..\\outside"] {
        assert!(make().read("library".into(), path.into(), 0, 1).is_err(), "{path}");
    }
    assert!(make().read("other".into(), "Part.BIN".into(), 0, 1).is_err());
    assert!(files::Host::write_file(&mut make(), "library".into(), "Part.BIN".into(), b"changed".to_vec()).is_err());
    assert_eq!(std::fs::read(root.join("Part.BIN")).unwrap(), b"abcdef");
    assert!(make().read("library".into(), "Part.BIN".into(), 0, 0).is_err());
    assert!(make().read("library".into(), "Part.BIN".into(), 0, MAX_IMPORT_CHUNK_BYTES as u32 + 1).is_err());
    let mut exhausted = make();
    exhausted.folder_bytes = MAX_IMPORT_INPUT_BYTES;
    assert!(exhausted.read("library".into(), "Part.BIN".into(), 0, 1).is_err());
    let mut exhausted = make();
    exhausted.folder_files = (0..MAX_IMPORT_FILES).map(|i| ("library".into(), PathBuf::from(format!("file{i}")))).collect();
    assert!(exhausted.size("library".into(), "Part.BIN".into()).is_err());
    let mut cancelled = make();
    cancelled.host.cancelled = true;
    assert!(matches!(cancelled.size("library".into(), "Part.BIN".into()), Err(PluginError::Cancelled)));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("Part.BIN"), root.join("linked")).unwrap();
        assert!(make().read("library".into(), "linked".into(), 0, 1).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}

fn folder_import_component() -> Vec<u8> {
    let (package, version) = API.split_once('@').unwrap();
    let source = folder_or_files_source(&[false], false, "i32.const 4 i32.load i32.const 8 i32.load i32.const 32 call $write", true);
    let design = format!(
        r#"
(type $design-type (instance
    (alias outer 1 $plugin-error (type $error))
    (export "plugin-error" (type (eq $error)))
    (type $write (func (param "bytes" (list u8)) (result (result (error $error)))))
    (export "write" (func (type $write)))))
(import "{package}/design@{version}" (instance $design (type $design-type)))
(alias export $design "write" (func $write))
(type $file-info (record (field "name" string) (field "size" u64)))
"#
    );
    let source = source
        .replace("  (core module $allocation", &format!("{design}  (core module $allocation"))
        .replace(
            "  (core instance $imports",
            "  (core func $write-core (canon lower (func $write) (memory $memory) (realloc $realloc)))\n  (core instance $imports",
        )
        .replace("(export \"memory\" (memory $memory))", "(export \"write\" (func $write-core)) (export \"memory\" (memory $memory))")
        .replace("  (core module $main", "  (core module $main (import \"host\" \"write\" (func $write (param i32 i32 i32)))")
        .replace("(func (export \"execute\") (param i32 i32 i32 i32)", "(func (export \"execute\") (param i32 i32 i32 i32 i64 i32 i32)")
        .replace("(param \"params\" $json)", "(param \"file\" $file-info) (param \"options\" $json)")
        .replace("(export \"execute\" (func $execute))", "(export \"file-info\" (type $file-info)) (export \"import-file\" (func $execute))")
        .replace(&format!("{package}/entry@{version}"), &format!("{package}/importer@{version}"));
    wat::parse_str(source).unwrap()
}

#[test]
fn an_importer_can_read_an_approved_external_library_without_widening_source() {
    let root = import_folder(&[("assembly.smp", b"source")]);
    let library = root.join("library");
    std::fs::create_dir_all(&library).unwrap();
    std::fs::write(library.join("secret.txt"), b"{}").unwrap();
    let mut manifest = importer_manifest(false);
    manifest.capabilities = vec!["formats@0.2".into(), "folders@0.1".into()];
    let component = folder_import_component();
    crate::check_component(&manifest, &component).unwrap();
    let request = crate::ImportRequest { path: root.join("assembly.smp"), input_folder: false };
    let (_, denied) = crate::run_import(App::default(), &manifest, &component, "convert", &json!({}), &request);
    assert!(matches!(denied, Err(HostError::PermissionDenied { .. })));
    let app = App { input_grants: vec![FileGrant { name: "project".into(), folder: library, read: true, write: false }], ..Default::default() };
    let (_, imported) = crate::run_import(app, &manifest, &component, "convert", &json!({}), &request);
    assert_eq!(imported.unwrap().design, b"{}");
    std::fs::remove_dir_all(root).unwrap();
}

const USER_SET: &str = "i32.const 512 i32.const 5 i32.const 2048 i32.const 2 i32.const 128 call $set";

fn user_storage_component(code: &str) -> Vec<u8> {
    wat::parse_str(include_str!("../user_storage_fixture.wat").replace("__CODE__", code)).unwrap()
}

fn user_storage_manifest() -> Manifest {
    let mut m = manifest();
    m.capabilities.push("user-storage@0.1".into());
    m
}

#[test]
fn user_storage_wit_set_get_across_runs_list_and_remove() {
    let mut app = App::default();
    let mut m = user_storage_manifest();
    m.capabilities = vec!["commands@0.2".into(), "user-storage@0.1.0".into()];
    let padded = "i32.const 2050 i32.const 32 i32.const 70000 memory.fill
        i32.const 512 i32.const 5 i32.const 2048 i32.const 70002 i32.const 128 call $set";
    run(&mut app, &m, &user_storage_component(padded), "make", &json!({})).unwrap();
    assert_eq!(app.user_data["hinge"], json!(42));
    let read = "i32.const 512 i32.const 5 i32.const 128 call $get
        i32.const 128 i32.load if unreachable end
        i32.const 132 i32.load i32.const 1 i32.ne if unreachable end
        i32.const 140 i32.load i32.const 2 i32.ne if unreachable end
        i32.const 136 i32.load i32.load16_u i32.const 12852 i32.ne if unreachable end
        i32.const 128 call $list
        i32.const 128 i32.load if unreachable end
        i32.const 136 i32.load i32.const 1 i32.ne if unreachable end
        i32.const 132 i32.load i32.load offset=4 i32.const 5 i32.ne if unreachable end";
    let mut read_manifest = m.clone();
    read_manifest.runtime.as_mut().unwrap().entries[0].effect = Effect::Read;
    run(&mut app, &read_manifest, &user_storage_component(read), "make", &json!({})).unwrap();
    let remove = "i32.const 512 i32.const 5 i32.const 128 call $remove
        i32.const 128 i32.load if unreachable end
        i32.const 132 i32.load i32.eqz if unreachable end";
    run(&mut app, &m, &user_storage_component(remove), "make", &json!({})).unwrap();
    assert!(app.user_data.is_empty());
}

#[test]
fn user_storage_wit_refuses_missing_capability_read_writes_invalid_json_and_limits() {
    let mut app = App::default();
    assert!(matches!(run(&mut app, &manifest(), &user_storage_component(USER_SET), "make", &json!({})), Err(HostError::PermissionDenied { .. })));
    let mut m = user_storage_manifest();
    m.runtime.as_mut().unwrap().entries[0].effect = Effect::Read;
    for (code, import) in [(USER_SET, "user-storage.set"), ("i32.const 512 i32.const 5 i32.const 128 call $remove", "user-storage.remove")] {
        assert_eq!(run(&mut app, &m, &user_storage_component(code), "make", &json!({})).unwrap_err(), craft_plugin::read_entry_refusal(import));
    }
    assert!(app.user_data.is_empty());
    m.runtime.as_mut().unwrap().entries[0].effect = Effect::Edit;
    let invalid = "i32.const 512 i32.const 5 i32.const 512 i32.const 5 i32.const 128 call $set";
    assert!(matches!(run(&mut app, &m, &user_storage_component(invalid), "make", &json!({})), Err(HostError::InvalidInput { .. })));
    // The fixture sees limit-exceeded (variant 2), even if it swallows the error.
    for i in 0..craft_plugin::MAX_USER_STORAGE_KEYS {
        app.user_data.insert(format!("k{i}"), json!(null));
    }
    let code = format!(
        "{USER_SET} i32.const 128 i32.load i32.const 1 i32.ne if unreachable end i32.const 132 i32.load i32.const 2 i32.ne if unreachable end"
    );
    assert_eq!(
        run(&mut app, &m, &user_storage_component(&code), "make", &json!({})).unwrap_err(),
        HostError::LimitExceeded { limit: "user-storage keys (128)".into() }
    );
    app.user_data.clear();
    app.user_data.insert("large".into(), json!("x".repeat(craft_plugin::MAX_USER_STORAGE_BYTES - 14)));
    assert_eq!(
        run(&mut app, &m, &user_storage_component(USER_SET), "make", &json!({})).unwrap_err(),
        HostError::LimitExceeded { limit: "user-storage total bytes (65536)".into() }
    );
    m.runtime.as_mut().unwrap().limits.memory_mib = 2;
    m.runtime.as_mut().unwrap().limits.fuel = 10_000_000;
    let oversized_input = "i32.const 20 memory.grow i32.const -1 i32.eq if unreachable end
        i32.const 2050 i32.const 32 i32.const 1048575 memory.fill
        i32.const 512 i32.const 5 i32.const 2048 i32.const 1048577 i32.const 128 call $set";
    assert_eq!(
        run(&mut app, &m, &user_storage_component(oversized_input), "make", &json!({})).unwrap_err(),
        HostError::LimitExceeded { limit: "user-storage JSON input bytes (1048576)".into() }
    );
}

/// The storage importer also exports a read-only, file-aware field hook.
fn storage_importer_dialog_component(code: &str) -> Vec<u8> {
    let hook = format!(
        r#"    (data (i32.const 4096) "{{\22fields\22:{{}}}}")
    (func (export "dialog") (param i32 i32 i32 i32) (result i32)
      {code}
      i32.const 0 i32.const 0 i32.store
      i32.const 4 i32.const 4096 i32.store
      i32.const 8 i32.const 13 i32.store
      i32.const 0)
    (func (export "execute")"#
    );
    let source = include_str!("../importer_storage_fixture.wat").replace("__CODE__", "").replace("    (func (export \"execute\")", &hook);
    let (prefix, _) = source.rsplit_once(')').unwrap();
    wat::parse_str(format!(
        r#"{prefix}
  (alias core export $main "dialog" (core func $dialog-core))
  (func $update (param "entry" string) (param "values" $json) (result (result $json (error $error)))
    (canon lift (core func $dialog-core) (memory $memory) (realloc $realloc)))
  (instance $dialog (export "json" (type $json)) (export "plugin-error" (type $error)) (export "update-fields" (func $update)))
  (export "craft:plugin/dialog-provider@0.2.0" (instance $dialog)))"#
    ))
    .unwrap()
}

#[test]
fn file_aware_hooks_read_optional_user_settings_without_publishing_or_writing() {
    let root = import_folder(&[("part.smp", b"header")]);
    let request = ImportRequest { path: root.join("part.smp"), input_folder: false };
    let mut manifest = dialog_manifest();
    manifest.optional_capabilities = vec!["user-storage@0.1".into()];
    let read = "i32.const 512 i32.const 5 i32.const 128 call $get
        i32.const 128 i32.load if unreachable end
        i32.const 132 i32.load i32.eqz if unreachable end";
    let component = storage_importer_dialog_component(read);
    crate::check_component(&manifest, &component).unwrap();
    let original = BTreeMap::from([("hinge".into(), json!(42))]);
    let app = App { user_data: original.clone(), ..Default::default() };
    let (app, result) = crate::update_fields(app, &manifest, &component, "convert", &json!({"values":{}}), &request);
    assert_eq!(result.unwrap(), json!({"fields":{}}));
    assert_eq!((app.begins, app.commits, app.rollbacks), (1, 0, 1));
    assert_eq!(app.user_data, original);
    let (app, result) = crate::update_fields(app, &manifest, &storage_importer_dialog_component(USER_SET), "convert", &json!({}), &request);
    assert_eq!(result.unwrap_err(), craft_plugin::read_entry_refusal("user-storage.set"));
    assert_eq!(app.user_data, original);
    manifest.optional_capabilities = vec!["user-storage@0.2".into()];
    let (_, result) = crate::update_fields(app, &manifest, &component, "convert", &json!({}), &request);
    assert!(matches!(result, Err(HostError::PermissionDenied { .. })));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_blocked_prefix_read_expires_and_late_reads_keep_the_global_slots_until_finished() {
    let reads = Arc::new(AtomicUsize::new(0));
    let mut releases = Vec::new();
    let mut replies = Vec::new();
    for _ in 0..MAX_PREFIX_READS {
        let (release, blocked) = mpsc::channel();
        let (entered, started) = mpsc::channel();
        let reply = start_prefix_read(reads.clone(), move || {
            entered.send(()).unwrap();
            blocked.recv().unwrap();
            Ok(vec![42_u8])
        })
        .unwrap();
        started.recv().unwrap();
        // An already elapsed deadline makes this deterministic: the reader is still blocked.
        assert_eq!(wait_prefix(&reply, Instant::now()).unwrap_err(), deadline());
        releases.push(release);
        replies.push(reply);
    }
    assert_eq!(reads.load(Ordering::Acquire), MAX_PREFIX_READS);
    assert!(matches!(start_prefix_read(reads.clone(), || Ok(())), Err(HostError::LimitExceeded { .. })));
    for (release, reply) in releases.into_iter().zip(replies) {
        release.send(()).unwrap();
        assert_eq!(reply.recv().unwrap().unwrap(), vec![42]);
    }
    let (send, buffered) = mpsc::channel();
    send.send(Ok(vec![9_u8])).unwrap();
    // Even an already buffered late result cannot be returned as a successful hook update.
    assert_eq!(wait_prefix(&buffered, Instant::now()).unwrap_err(), deadline());
    assert_eq!(reads.load(Ordering::Acquire), 0);
    let reply = start_prefix_read(reads, || Ok(vec![7_u8])).unwrap();
    reply.recv().unwrap().unwrap();
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
    compile_with(bytes, 512 * 1024, Some(cache.clone()))
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
    compile_with(&importer_component(), 512 * 1024, None).unwrap();
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
