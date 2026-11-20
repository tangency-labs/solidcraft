//! `plugin check` and `plugin test` on packages made from the repository's original plugin
//! fixtures: a command that adds a 2 x 3 x 4 mm box, and an importer that copies the file it
//! opens into the design.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use solidcraft_engine::plugin::API;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("solidcraft-plugin-tools-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> String {
    let (package, version) = API.split_once('@').unwrap();
    let source = match name {
        "command" => include_str!("../../../crates/engine/src/plugin/fixture.wat"),
        "exporter" => include_str!("../../../crates/engine/src/plugin/exporter_fixture.wat"),
        _ => include_str!("../../../crates/engine/src/plugin/importer_fixture.wat"),
    };
    source.replace("\r\n", "\n").replace("__PACKAGE__", package).replace("__VERSION__", version)
}
/// The command fixture with its entry's body replaced: `call $box` adds one box, `unreachable` traps.
fn command_component(code: &str) -> Vec<u8> {
    let source = fixture("command");
    let original = "      call $box\n      call $box\n";
    assert!(source.contains(original));
    wat::parse_str(source.replacen(original, &format!("      {code}\n"), 1)).unwrap()
}
fn command_manifest() -> Value {
    json!({"id":"boxer","name":"Boxer","version":"1.0.0","capabilities":["commands@0.1"],"runtime":{
        "module":"boxer.wasm","entries":[{"name":"make","input_schema":{"type":"object","properties":{}}}],"commands":["solid.box"],
        "limits":{"fuel":100000,"memory_mib":1,"deadline_ms":5000}}})
}
fn importer_manifest() -> Value {
    json!({"id":"copier","name":"Copier","version":"1.0.0","capabilities":["formats@0.1"],"runtime":{
        "module":"copier.wasm","commands":[],"entries":[{"name":"convert","kind":"importer","extensions":["smp"]}],
        "limits":{"fuel":10_000_000,"memory_mib":2,"deadline_ms":5000,"stack_kib":16384}}})
}
/// A package folder `root/<id>` with its manifest, component and test cases.
fn package(root: &Path, manifest: &Value, component: &[u8], tests: &[(&str, Value)]) -> String {
    let id = manifest["id"].as_str().unwrap();
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(dir.join(manifest["runtime"]["module"].as_str().unwrap()), component).unwrap();
    for (name, case) in tests {
        std::fs::write(dir.join("tests").join(format!("{name}.json")), case.to_string()).unwrap();
    }
    dir.to_string_lossy().into_owned()
}
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli")).args(args).output().unwrap()
}
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// #19237: `plugin --help` says what each operation does, and an operation's own help says what
/// that operation does: `plugin check --help` does not print `plugin test`.
#[test]
fn plugin_help_lists_each_operation_and_each_operation_has_its_own_help() {
    let both = cli(&["plugin", "--help"]);
    assert!(both.status.success(), "{}", text(&both.stderr));
    let check = cli(&["plugin", "check", "--help"]);
    assert!(check.status.success(), "{}", text(&check.stderr));
    let test = cli(&["plugin", "test", "--help"]);
    assert!(test.status.success(), "{}", text(&test.stderr));
    let (both, check, test) = (text(&both.stdout), text(&check.stdout), text(&test.stdout));
    assert!(both.contains("solidcraft-cli plugin check") && both.contains("solidcraft-cli plugin test"), "{both}");
    assert!(check.contains("solidcraft-cli plugin check") && !check.contains("solidcraft-cli plugin test"), "{check}");
    assert!(test.contains("solidcraft-cli plugin test") && !test.contains("solidcraft-cli plugin check"), "{test}");
    // `plugin --help` is both operations' help, one after the other.
    let body = |usage: &str| usage.strip_prefix("usage:\n").unwrap_or_default().to_string();
    assert_eq!(both, format!("usage:\n{}{}", body(&check), body(&test)));
}

/// #19237: an argument that is not a plugin says what was expected (the plugin's package folder,
/// or its .zip), not the CLI's generic usage.
#[test]
fn an_argument_that_is_not_a_plugin_says_what_it_expected() {
    let scratch = Scratch::new();
    let nowhere = scratch.path("nowhere");
    for action in ["check", "test"] {
        let output = cli(&["plugin", action, &nowhere]);
        assert!(!output.status.success(), "`plugin {action}` succeeded");
        let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
        assert!(stdout.contains("fix: Give the plugin's package folder"), "plugin {action}: {stdout}");
        assert!(!stderr.contains("usage:"), "plugin {action}: {stderr}");
    }
}

#[test]
fn check_passes_good_packages_and_names_each_broken_one_with_its_fix() {
    let scratch = Scratch::new();
    // `unreachable`: checking would fail if it ran any of the plugin's code.
    let good = package(&scratch.0, &command_manifest(), &command_component("unreachable"), &[]);
    let output = cli(&["plugin", "check", &good]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(stdout.starts_with("boxer 1.0.0 (command plugin): no problems"), "{stdout}");
    assert!(stdout.contains("entry   boxer.make (command)"), "{stdout}");
    let importer = package(&scratch.0, &importer_manifest(), &wat::parse_str(fixture("importer")).unwrap(), &[]);
    let output = cli(&["plugin", "check", &importer, "--json"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!((report["ok"].as_bool(), report["kind"].as_str()), (Some(true), Some("importer")), "{report}");

    let broken = Scratch::new();
    let mut net = command_manifest();
    net["runtime"]["permissions"] = json!({"net": true});
    let mut wasi = fixture("command");
    let end = wasi.rfind(')').unwrap();
    wasi.replace_range(end.., "(import \"wasi:cli/environment@0.2.0\" (instance (export \"get\" (func)))))");
    for (manifest, component, field, fix) in [
        (net, command_component("call $box"), "runtime.permissions.net", "no network access"),
        (command_manifest(), wat::parse_str("(module)").unwrap(), "component", "wasm-tools component new"),
        (command_manifest(), wat::parse_str(&wasi).unwrap(), "component", "wasm32-unknown-unknown"),
        (importer_manifest(), command_component("call $box"), "component", "importer-plugin"),
    ] {
        let path = package(&broken.0, &manifest, &component, &[]);
        let output = cli(&["plugin", "check", &path]);
        let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
        assert!(!output.status.success(), "{stdout}");
        assert!(stdout.contains(&format!("error   {field}: ")) && stdout.contains(fix), "{field}: {stdout}");
        assert!(stderr.contains("plugin check: 1 error(s)"), "{stderr}");
        std::fs::remove_dir_all(&path).unwrap();
    }
    let output = cli(&["plugin", "check", &scratch.path("nowhere")]);
    assert!(!output.status.success() && text(&output.stdout).contains("error   package: could not read"), "{}", text(&output.stdout));
}

#[test]
fn check_reports_invalid_conditional_fields_without_running_wasm() {
    let scratch = Scratch::new();
    for condition in [json!({"field":"missing","eq":true}), json!({"field":"size","eq":1})] {
        let mut manifest = command_manifest();
        manifest["runtime"]["entries"][0]["input_schema"] = json!({"type":"object","properties":{
            "size":{"type":"number","x-solidcraft-visible-when":condition}}});
        let path = package(&scratch.0, &manifest, &command_component("unreachable"), &[]);
        let output = cli(&["plugin", "check", &path, "--json"]);
        assert!(!output.status.success(), "{}", text(&output.stdout));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["ok"], false);
        assert!(text(&output.stdout).contains("input_schema"));
    }
}

#[test]
fn test_runs_the_packages_cases_on_the_real_host() {
    let scratch = Scratch::new();
    let one_box = json!({"entry":"make","expect":{"reply":{"result":{}},"model":{"body_count":1,"total":{"volume_mm3":24}}}});
    let path = package(&scratch.0, &command_manifest(), &command_component("call $box"), &[("one-box", one_box.clone())]);
    let out = scratch.0.join("out");
    let output = cli(&["plugin", "test", &path, "--json", "--out", &out.to_string_lossy()]);
    assert!(output.status.success(), "{}{}", text(&output.stdout), text(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!((report["passed"].as_u64(), report["failed"].as_u64(), report["skipped"].as_u64()), (Some(1), Some(0), Some(0)), "{report}");
    assert_eq!(report["cases"][0]["name"], "one-box");
    // The resulting design and its log are kept: the design reopens with the box.
    let saved = report["cases"][0]["design"].as_str().unwrap();
    let measured = cli(&["eval", saved]);
    assert_eq!(serde_json::from_slice::<Value>(&measured.stdout).unwrap()["total"]["volume_mm3"], 24.0);
    assert!(Path::new(report["cases"][0]["journal"].as_str().unwrap()).is_file());

    // A wrong expectation fails the run and says what differs; a trap is a result a case can expect.
    let wrong = json!({"entry":"make","expect":{"model":{"total":{"volume_mm3":25}}}});
    std::fs::write(Path::new(&path).join("tests/wrong.json"), wrong.to_string()).unwrap();
    let output = cli(&["plugin", "test", &path]);
    let stdout = text(&output.stdout);
    assert!(!output.status.success(), "{stdout}");
    assert!(stdout.contains("FAIL  wrong (make)") && stdout.contains("model.total.volume_mm3 is 24, and the test expects 25"), "{stdout}");
    assert!(stdout.contains("boxer 1.0.0: 1 passed, 1 failed, 0 skipped"), "{stdout}");
    let traps = Scratch::new();
    let trap = json!({"entry":"make","expect":{"error":"trap"}});
    let path = package(&traps.0, &command_manifest(), &command_component("unreachable"), &[("traps", trap)]);
    let output = cli(&["plugin", "test", &path]);
    assert!(output.status.success(), "{}", text(&output.stdout));
    let output = cli(&["plugin", "test", &path, "--json"]);
    assert_eq!(serde_json::from_slice::<Value>(&output.stdout).unwrap()["cases"][0]["status"], "passed");

    // With no case, an entry that needs no values runs once with its defaults.
    let bare = Scratch::new();
    let path = package(&bare.0, &command_manifest(), &command_component("call $box"), &[]);
    let output = cli(&["plugin", "test", &path]);
    assert!(output.status.success() && text(&output.stdout).contains("pass  make (defaults)"), "{}", text(&output.stdout));

    // The shipped .zip carries its tests with it.
    let manifest = command_manifest().to_string();
    let component = command_component("call $box");
    let case = one_box.to_string();
    let zip = solidcraft_engine::io::safe_zip::write(&[
        ("boxer/plugin.json", manifest.as_bytes()),
        ("boxer/boxer.wasm", &component),
        ("boxer/tests/one-box.json", case.as_bytes()),
    ])
    .unwrap();
    std::fs::write(bare.0.join("boxer.zip"), zip).unwrap();
    let output = cli(&["plugin", "test", &bare.path("boxer.zip")]);
    assert!(output.status.success() && text(&output.stdout).contains("pass  one-box (make)"), "{}", text(&output.stdout));

    // A package that does not pass plugin check is not tested.
    let mut net = command_manifest();
    net["runtime"]["permissions"] = json!({"net": true});
    let refused = Scratch::new();
    let path = package(&refused.0, &net, &command_component("call $box"), &[("one-box", one_box)]);
    let output = cli(&["plugin", "test", &path]);
    assert!(!output.status.success() && text(&output.stderr).contains("does not pass plugin check"), "{}", text(&output.stderr));
}

#[test]
fn an_exporter_without_folder_grants_runs_default_and_explicit_cases() {
    let scratch = Scratch::new();
    let manifest = json!({"id":"writer","name":"Writer","version":"1.0.0","capabilities":["formats@0.1"],"runtime":{
        "module":"exporter.wasm","commands":["document.inspect"],
        "entries":[{"name":"copy","kind":"exporter","extensions":["smp"]}],
        "limits":{"fuel":10_000_000,"memory_mib":2,"deadline_ms":5000}}});
    let path = package(&scratch.0, &manifest, &wat::parse_str(fixture("exporter")).unwrap(), &[]);
    for name in ["copy (defaults)", "write"] {
        if name == "write" {
            let case = json!({"entry":"copy","expect":{"reply":{"format":"writer.copy","plugin":"Writer"}}});
            std::fs::write(Path::new(&path).join("tests/write.json"), case.to_string()).unwrap();
        }
        let output = cli(&["plugin", "test", &path, "--json"]);
        assert!(output.status.success(), "{}{}", text(&output.stdout), text(&output.stderr));
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!((report["passed"].as_u64(), report["failed"].as_u64(), report["skipped"].as_u64()), (Some(1), Some(0), Some(0)), "{report}");
        assert_eq!(report["cases"][0]["name"], name);
        assert_eq!(report["cases"][0]["status"], "passed");
    }
}

#[test]
fn an_importers_cases_open_their_files_and_check_the_loss_report() {
    let scratch = Scratch::new();
    let open = json!({"entry":"convert","file":"bracket.smp",
        "expect":{"reply":{"bodies":1,"loss_report":{"summary":"Copied the file","items":[{"severity":"dropped"}]}},"model":{"body_count":1}}});
    let traps = json!({"entry":"convert","file":"trap.smp","expect":{"error":"trap","message":"Copier could not open trap.smp"}});
    // A valid file the importer does not convert is `unsupported`, named with the plugin and the file.
    let refuses = json!({"entry":"convert","file":"sketch.smp","expect":{"error":"unsupported","message":"Copier does not support sketch.smp: the model has no solids"}});
    let cases = [("open", open), ("traps", traps), ("refuses", refuses)];
    let path = package(&scratch.0, &importer_manifest(), &wat::parse_str(fixture("importer")).unwrap(), &cases);
    // The fixture copies the file into the design: give it a design's bytes. A file starting with T traps.
    let mut made = solidcraft_engine::Session::new(solidcraft_engine::doc::Document::new("Bracket"));
    made.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
    std::fs::write(Path::new(&path).join("tests/bracket.smp"), solidcraft_engine::io::write_design(&made.doc).unwrap()).unwrap();
    std::fs::write(Path::new(&path).join("tests/trap.smp"), b"T").unwrap();
    std::fs::write(Path::new(&path).join("tests/sketch.smp"), b"U").unwrap();
    let output = cli(&["plugin", "test", &path]);
    let stdout = text(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", text(&output.stderr));
    assert!(stdout.contains("pass  open (convert)") && stdout.contains("pass  traps (convert)"), "{stdout}");
    assert!(stdout.contains("pass  refuses (convert)"), "{stdout}");
    // A case for a file that is not there fails with the reason.
    std::fs::write(Path::new(&path).join("tests/missing.json"), json!({"entry":"convert","file":"missing.smp"}).to_string()).unwrap();
    let output = cli(&["plugin", "test", &path]);
    assert!(!output.status.success() && text(&output.stdout).contains("FAIL  missing (convert)"), "{}", text(&output.stdout));
}

#[test]
fn require_cases_fails_an_untested_entry_in_a_folder_and_a_zip() {
    let scratch = Scratch::new();
    let path = package(&scratch.0, &importer_manifest(), &wat::parse_str(fixture("importer")).unwrap(), &[]);
    let output = cli(&["plugin", "test", &path]);
    let stdout = text(&output.stdout);
    assert!(output.status.success() && stdout.contains("copier 1.0.0: 0 passed, 0 failed, 1 skipped"), "{stdout}");
    let manifest = importer_manifest().to_string();
    let component = wat::parse_str(fixture("importer")).unwrap();
    let zip = solidcraft_engine::io::safe_zip::write(&[("copier/plugin.json", manifest.as_bytes()), ("copier/copier.wasm", &component)]).unwrap();
    std::fs::write(scratch.0.join("copier.zip"), zip).unwrap();
    for package in [path, scratch.path("copier.zip")] {
        let output = cli(&["plugin", "test", &package, "--require-cases"]);
        let stdout = text(&output.stdout);
        assert!(!output.status.success(), "{package}: {stdout}");
        assert!(stdout.contains("FAIL  convert") && stdout.contains("0 passed, 1 failed, 0 skipped"), "{package}: {stdout}");
    }
}

#[test]
fn a_cases_grants_and_fixtures_stay_inside_its_tests_and_scratch_folders() {
    let scratch = Scratch::new();
    let mut manifest = command_manifest();
    manifest["runtime"]["permissions"] = json!({"files":[{"name":"seed","read":true}]});
    let path = package(&scratch.0, &manifest, &command_component("call $box"), &[]);
    let tests = Path::new(&path).join("tests");
    std::fs::create_dir_all(tests.join("seed")).unwrap();
    std::fs::write(tests.join("seed/part.txt"), b"x").unwrap();
    // A grant the manifest does not ask for is refused before any folder is made or seeded.
    let outside = scratch.0.join("outside");
    let escape = json!({"entry":"make","grants":{"folders":[{"name":outside.to_string_lossy(),"read":true,"from":"seed"}]}});
    std::fs::write(tests.join("escape.json"), escape.to_string()).unwrap();
    let output = cli(&["plugin", "test", &path]);
    let stdout = text(&output.stdout);
    assert!(!output.status.success() && stdout.contains("FAIL  escape (make)") && stdout.contains("asks for no folder"), "{stdout}");
    assert!(!outside.exists(), "nothing was made outside the case's folder");
    std::fs::remove_file(tests.join("escape.json")).unwrap();
    let seeded = json!({"entry":"make","grants":{"folders":[{"name":"seed","read":true,"from":"seed"}]}});
    std::fs::write(tests.join("seeded.json"), seeded.to_string()).unwrap();
    let output = cli(&["plugin", "test", &path]);
    assert!(output.status.success() && text(&output.stdout).contains("pass  seeded (make)"), "{}", text(&output.stdout));
    #[cfg(unix)]
    {
        // A fixture folder that is a link could reach anywhere: refused, as links inside it are.
        std::os::unix::fs::symlink(tests.join("seed"), tests.join("linked")).unwrap();
        let linked = json!({"entry":"make","grants":{"folders":[{"name":"seed","read":true,"from":"linked"}]}});
        std::fs::write(tests.join("seeded.json"), linked.to_string()).unwrap();
        let output = cli(&["plugin", "test", &path]);
        let stdout = text(&output.stdout);
        assert!(!output.status.success() && stdout.contains("FAIL  seeded (make)") && stdout.contains("is a link"), "{stdout}");
        std::fs::remove_file(tests.join("seeded.json")).unwrap();
        std::fs::remove_file(tests.join("linked")).unwrap();
        // The tests root itself must not redirect the fixture copy outside the package.
        std::fs::write(tests.join("seeded.json"), seeded.to_string()).unwrap();
        let external = scratch.0.join("external-tests");
        std::fs::rename(&tests, &external).unwrap();
        std::os::unix::fs::symlink(&external, &tests).unwrap();
        let output = cli(&["plugin", "test", &path]);
        std::fs::remove_file(&tests).unwrap();
        std::fs::rename(&external, &tests).unwrap();
        let stdout = text(&output.stdout);
        assert!(!output.status.success() && stdout.contains("FAIL  seeded (make)") && stdout.contains("is a link"), "{stdout}");
        // Listing a folder without search permission must not silently drop its cases.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tests, std::fs::Permissions::from_mode(0o400)).unwrap();
        let metadata_denied = std::fs::metadata(tests.join("seeded.json")).is_err();
        let output = cli(&["plugin", "test", &path, "--require-cases"]);
        std::fs::set_permissions(&tests, std::fs::Permissions::from_mode(0o755)).unwrap();
        if metadata_denied {
            assert!(!output.status.success() && text(&output.stderr).contains("could not check the test case"), "{}", text(&output.stderr));
        }
        std::fs::remove_file(tests.join("seeded.json")).unwrap();
        // A dangling case link also reports its metadata error, even with elevated rights.
        std::os::unix::fs::symlink(tests.join("missing.json"), tests.join("broken.json")).unwrap();
        let output = cli(&["plugin", "test", &path, "--require-cases"]);
        std::fs::remove_file(tests.join("broken.json")).unwrap();
        let stderr = text(&output.stderr);
        assert!(!output.status.success() && stderr.contains("could not check the test case") && stderr.contains("broken.json"), "{stderr}");
        // A tests folder that can't be read is an error, not an empty suite (unless this runs
        // with the rights to read it anyway).
        std::fs::set_permissions(&tests, std::fs::Permissions::from_mode(0o000)).unwrap();
        let unreadable = std::fs::read_dir(&tests).is_err();
        let output = cli(&["plugin", "test", &path]);
        std::fs::set_permissions(&tests, std::fs::Permissions::from_mode(0o755)).unwrap();
        if unreadable {
            assert!(!output.status.success() && text(&output.stderr).contains("could not read the tests folder"), "{}", text(&output.stderr));
        }
    }
}
