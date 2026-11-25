//! `solidcraft-cli plugin check` and `plugin test`: a plugin author's tools.
//!
//! `check` runs the app's `plugin.check`: what Install and a call would refuse, each with its fix.
//! `test` installs the package into a scratch plugins folder with the app's own installer, stores
//! each case's approvals in that folder's grant store as an install does, then
//! runs the plugin's test cases (`tests/*.json` in the package) through `plugin.run`,
//! `file.import` and `file.export` on the real host, with its real limits, and compares what comes back.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use solidcraft_engine::plugin::check::{Source, TESTS_FOLDER};
use solidcraft_engine::plugin::{EntryKind, FileGrant, Grants, Manifest};
use solidcraft_engine::{EngineError, Session};

/// The most test cases one package runs.
const MAX_CASES: usize = 256;
/// The largest test case file read.
const MAX_CASE_BYTES: usize = 1 << 20;
/// The default tolerance for a number in an expectation: relative, or absolute below 1.
const TOLERANCE: f64 = 1e-6;

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("check") => check(&args[1..]),
        Some("test") => test(&args[1..]),
        Some(other) => Err(format!("plugin: unknown action `{other}`; use plugin check or plugin test\n{}", crate::plugin_usage(None))),
        None => Err(crate::plugin_usage(None)),
    }
}

/// The package path: the one word that is not an option or an option's value.
fn package_path<'a>(action: &str, args: &'a [String], valued: &[&str]) -> Result<&'a str, String> {
    let mut found = None;
    let mut i = 0;
    while let Some(a) = args.get(i) {
        if valued.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if !a.starts_with("--") && found.replace(a.as_str()).is_some() {
            return Err(format!("plugin {action} takes one package folder or .zip"));
        }
        i += 1;
    }
    found.ok_or_else(|| format!("plugin {action} needs a package folder or .zip\n{}", crate::plugin_usage(Some(action))))
}

fn check(args: &[String]) -> Result<(), String> {
    crate::known_args("plugin check", args, &[], &["--json"], true)?;
    let path = package_path("check", args, &[])?;
    let mut s = Session::default();
    let report = s.execute("plugin.check", &json!({"path": path})).map_err(|e| e.to_string())?;
    if args.iter().any(|a| a == "--json") {
        crate::print_json(&report, None)?;
    } else {
        print!("{}", describe_check(&report));
    }
    let errors = problems(&report, "error");
    if errors == 0 { Ok(()) } else { Err(format!("plugin check: {errors} error(s) in {path}")) }
}

fn problems(report: &Value, severity: &str) -> usize {
    report["problems"].as_array().map_or(0, |p| p.iter().filter(|p| p["severity"] == severity).count())
}

/// `plugin.check`'s reply in words: the plugin, then each problem and its fix.
fn describe_check(report: &Value) -> String {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let mut out = match report["id"].as_str() {
        Some(id) => format!("{id} {} ({} plugin)", text(&report["version"]), text(&report["kind"])),
        None => text(&report["path"]),
    };
    let (errors, warnings) = (problems(report, "error"), problems(report, "warning"));
    out.push_str(&match (errors, warnings) {
        (0, 0) => ": no problems\n".to_string(),
        _ => format!(": {errors} error(s), {warnings} warning(s)\n"),
    });
    for p in report["problems"].as_array().into_iter().flatten() {
        out.push_str(&format!("{:<8}{}: {}\n        fix: {}\n", text(&p["severity"]), text(&p["field"]), text(&p["problem"]), text(&p["fix"])));
    }
    for e in report["entries"].as_array().into_iter().flatten() {
        let extensions = e["extensions"].as_array().map(|x| x.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let fields = e["input_schema"]["properties"].as_object().map(|p| p.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default();
        out.push_str(&format!("entry   {}.{} ({}", text(&report["id"]), text(&e["name"]), text(&e["kind"])));
        if !extensions.is_empty() {
            out.push_str(&format!(", opens {extensions}"));
        }
        if !fields.is_empty() {
            out.push_str(&format!("; fields: {fields}"));
        }
        out.push_str(")\n");
    }
    out
}

/// One test case, `tests/<name>.json`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    /// The entry it runs.
    entry: String,
    /// What it shows, for the reader.
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    /// A command's parameters, or an importer's or exporter's options. Default `{}`.
    #[serde(default)]
    params: Option<Value>,
    /// A command or an exporter: the design it runs on, in the tests folder. Default: an empty
    /// design.
    #[serde(default)]
    design: Option<String>,
    /// An importer: the file it opens, in the tests folder.
    #[serde(default)]
    file: Option<String>,
    /// What the user approved. Default: everything the manifest asks for, as Install approves it.
    #[serde(default)]
    grants: Option<CaseGrants>,
    #[serde(default)]
    expect: Expect,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseGrants {
    #[serde(default)]
    command_groups: BTreeSet<String>,
    #[serde(default)]
    folders: Vec<CaseFolder>,
    #[serde(default)]
    input_folder: bool,
    #[serde(default)]
    memory_mib: Option<usize>,
    #[serde(default)]
    fuel: Option<u64>,
    /// Connections to the endpoints the manifest names (`net@0.1`).
    #[serde(default)]
    net: bool,
}

/// A folder the plugin may read or write: a fresh, empty one, or a copy of `from` in the tests
/// folder, so a test never changes its own files.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseFolder {
    name: String,
    #[serde(default)]
    read: bool,
    #[serde(default)]
    write: bool,
    #[serde(default)]
    from: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expect {
    /// The call fails with this kind of error (`permission_denied`, `limit_exceeded`, `exhausted`,
    /// `over_ceiling`, `unsupported`, `trap`, `invalid_input`, `command_failed`, `cancelled`, ...),
    /// or `any`. Default: it succeeds.
    #[serde(default)]
    error: Option<String>,
    /// Text the error message contains.
    #[serde(default)]
    message: Option<String>,
    /// Values the reply contains: `plugin.run`'s (`{"result": ...}`), `file.import`'s or
    /// `file.export`'s.
    #[serde(default)]
    reply: Option<Value>,
    /// Values `inspect.measure` gives for the resulting design (`{"body_count": 1}`).
    #[serde(default)]
    model: Option<Value>,
    /// Files the plugin wrote, as `<folder name>/<path>`.
    #[serde(default)]
    files: Vec<String>,
    /// How far a number may be off: relative, or absolute below 1. Default 1e-6.
    #[serde(default)]
    tolerance: Option<f64>,
}

/// How one case went.
struct Outcome {
    name: String,
    entry: String,
    status: &'static str,
    message: String,
    ms: u128,
    design: Option<String>,
    journal: Option<String>,
}

fn test(args: &[String]) -> Result<(), String> {
    crate::known_args("plugin test", args, &["--out"], &["--json", "--require-cases", "--allow-net"], true)?;
    let path = package_path("test", args, &["--out"])?;
    let json_out = args.iter().any(|a| a == "--json");
    // For CI: an entry no case covers fails the run instead of being skipped.
    let require_cases = args.iter().any(|a| a == "--require-cases");
    // A package's own test cases never grant it the network: the person running the tests does.
    let allow_net = args.iter().any(|a| a == "--allow-net");
    let out = crate::flag(args, "--out").map(PathBuf::from);
    // The same checks as `plugin check`: a package that would not install is not tested.
    let report = Session::default().execute("plugin.check", &json!({"path": path})).map_err(|e| e.to_string())?;
    if problems(&report, "error") > 0 {
        if json_out {
            crate::print_json(&report, None)?;
        } else {
            print!("{}", describe_check(&report));
        }
        return Err(format!("plugin test: {path} does not pass plugin check"));
    }
    let (package, source) = solidcraft_engine::plugin::check::read(path).map_err(|p| format!("{}: {}", p.field, p.problem))?;
    let scratch = solidcraft_engine::io::vfs::temp_dir().join(format!(
        "solidcraft-plugin-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default()
    ));
    let result = (|| {
        let plugins = scratch.join("plugins");
        let installed = solidcraft_engine::plugin::install::install(&package, &plugins, false).map_err(|e| e.to_string())?;
        // A folder's tests stay where they are; a .zip's were installed with it.
        let tests = match source {
            Source::Folder => Path::new(path).join(TESTS_FOLDER),
            Source::Zip => installed.join(TESTS_FOLDER),
        };
        if let Some(out) = &out {
            std::fs::create_dir_all(out).map_err(|e| format!("--out {}: {e}", out.display()))?;
        }
        let mut cases = read_cases(&tests)?;
        let manifest = &package.manifest;
        let runtime = manifest.runtime.as_ref().ok_or("the plugin has no runtime: it has no entries to test")?;
        // An entry no case covers runs once with its defaults, when it needs nothing else.
        let mut outcomes = Vec::new();
        for entry in &runtime.entries {
            if cases.iter().any(|(_, c)| c.as_ref().is_ok_and(|c| c.entry == entry.name)) {
                continue;
            }
            match smoke_params(entry) {
                Some(params) => cases
                    .push((format!("{} (defaults)", entry.name), Ok(Case { entry: entry.name.clone(), params: Some(params), ..Default::default() }))),
                None => outcomes.push(Outcome {
                    name: entry.name.clone(),
                    entry: entry.name.clone(),
                    status: if require_cases { "failed" } else { "skipped" },
                    message: match entry.kind {
                        EntryKind::Importer => format!("no test case: add tests/{}.json with a \"file\" to open", entry.name),
                        EntryKind::Command | EntryKind::Exporter => {
                            format!("no test case, and its inputs need values: add tests/{}.json with \"params\"", entry.name)
                        }
                    },
                    ms: 0,
                    design: None,
                    journal: None,
                }),
            }
        }
        for (index, (name, case)) in cases.into_iter().enumerate() {
            let started = Instant::now();
            let (entry, outcome) = match case {
                Ok(case) => {
                    let entry = case.entry.clone();
                    let work = scratch.join(format!("case-{index}"));
                    (entry, run_case(manifest, &installed, &plugins, &tests, &work, &case, out.as_deref(), &name, allow_net))
                }
                Err(e) => (String::new(), CaseResult { failure: Some(e), design: None, journal: None }),
            };
            outcomes.push(Outcome {
                status: if outcome.failure.is_some() { "failed" } else { "passed" },
                message: outcome.failure.unwrap_or_default(),
                ms: started.elapsed().as_millis(),
                design: outcome.design,
                journal: outcome.journal,
                name,
                entry,
            });
        }
        Ok::<_, String>(outcomes)
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    let outcomes = result?;
    let count = |status: &str| outcomes.iter().filter(|o| o.status == status).count();
    let (passed, failed, skipped) = (count("passed"), count("failed"), count("skipped"));
    if json_out {
        let cases: Vec<Value> = outcomes
            .iter()
            .map(|o| {
                let mut c = json!({"name": o.name, "entry": o.entry, "status": o.status, "ms": o.ms as u64});
                if !o.message.is_empty() {
                    c["message"] = json!(o.message);
                }
                if let Some(d) = &o.design {
                    c["design"] = json!(d);
                }
                if let Some(j) = &o.journal {
                    c["journal"] = json!(j);
                }
                c
            })
            .collect();
        crate::print_json(
            &json!({"plugin": package.manifest.id, "version": package.manifest.version, "passed": passed, "failed": failed, "skipped": skipped, "cases": cases}),
            None,
        )?;
    } else {
        for o in &outcomes {
            let status = match o.status {
                "passed" => "pass",
                "failed" => "FAIL",
                _ => "skip",
            };
            let entry = if o.entry.is_empty() || o.name == o.entry || o.name == format!("{} (defaults)", o.entry) {
                String::new()
            } else {
                format!(" ({})", o.entry)
            };
            let message = if o.message.is_empty() { String::new() } else { format!(": {}", o.message) };
            println!("{status}  {}{entry}, {} ms{message}", o.name, o.ms);
        }
        println!("{} {}: {passed} passed, {failed} failed, {skipped} skipped", package.manifest.id, package.manifest.version);
    }
    if failed == 0 { Ok(()) } else { Err(format!("plugin test: {failed} case(s) failed")) }
}

/// The test cases in `tests`, by file name, each parsed or with why it could not be.
fn read_cases(tests: &Path) -> Result<Vec<(String, Result<Case, String>)>, String> {
    // No tests folder is no cases; one that can't be read, or an entry in it that can't, is an
    // error, so a run never passes on the cases it happened to see.
    let entries = match std::fs::read_dir(tests) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("could not read the tests folder {}: {e}", tests.display())),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| format!("could not read the tests folder {}: {e}", tests.display()))?.path();
        if path.extension().is_some_and(|e| e == "json")
            && std::fs::metadata(&path).map_err(|e| format!("could not check the test case {}: {e}", path.display()))?.is_file()
        {
            files.push(path);
            if files.len() > MAX_CASES {
                return Err(format!("{} holds more than {MAX_CASES} test cases", tests.display()));
            }
        }
    }
    files.sort();
    let read: Vec<(String, Result<Case, String>)> = files
        .into_iter()
        .map(|file| {
            let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let case = solidcraft_engine::io::vfs::read(&file.to_string_lossy(), MAX_CASE_BYTES)
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice::<Case>(&bytes).map_err(|e| format!("tests/{name}.json: {e}")));
            (name, case)
        })
        .collect();
    // A design or file a case runs on lives in the tests folder too: a JSON design there is that
    // case's input, not a case of its own.
    let inputs: Vec<String> = read
        .iter()
        .filter_map(|(_, case)| case.as_ref().ok())
        .flat_map(|case| [case.design.as_deref(), case.file.as_deref()])
        .flatten()
        .map(|input| input.trim_start_matches("./").to_owned())
        .collect();
    let is_input = |name: &str| {
        let file = format!("{name}.json");
        inputs.iter().any(|input| if cfg!(windows) { input.eq_ignore_ascii_case(&file) } else { *input == file })
    };
    Ok(read.into_iter().filter(|(name, _)| !is_input(name)).collect())
}

/// The parameters a command's or exporter's dialog would start with: each field's default. `None`
/// for an importer, which needs a file, and for an entry a required field without a default would
/// leave empty.
fn smoke_params(entry: &solidcraft_engine::plugin::Entry) -> Option<Value> {
    if entry.kind == EntryKind::Importer {
        return None;
    }
    let properties = entry.input_schema.get("properties").and_then(Value::as_object);
    let mut params = Map::new();
    for (key, property) in properties.into_iter().flatten() {
        if let Some(default) = property.get("default") {
            params.insert(key.clone(), default.clone());
        }
    }
    let required = entry.input_schema.get("required").and_then(Value::as_array);
    if required.into_iter().flatten().filter_map(Value::as_str).any(|key| !params.contains_key(key)) {
        return None;
    }
    Some(Value::Object(params))
}

struct CaseResult {
    failure: Option<String>,
    design: Option<String>,
    journal: Option<String>,
}

/// A path inside the tests folder: a plain relative path, never `..` or absolute.
fn in_tests(tests: &Path, path: &str, what: &str) -> Result<PathBuf, String> {
    let relative = Path::new(path);
    if path.is_empty() || !relative.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(format!("{what} `{path}` must be a plain path inside the tests folder"));
    }
    Ok(tests.join(relative))
}

/// The grants a case runs with, checked with the app's own rules, and the folders made for it.
/// Connections only with `allow_net` (`--allow-net`): a case can't grant itself the network.
fn case_grants(manifest: &Manifest, approval: &str, tests: &Path, work: &Path, case: &Case, allow_net: bool) -> Result<Grants, String> {
    let runtime = manifest.runtime.as_ref().ok_or("the plugin has no runtime")?;
    let asked = CaseGrants {
        command_groups: runtime.permissions.command_groups.clone(),
        folders: runtime.permissions.files.iter().map(|f| CaseFolder { name: f.name.clone(), read: f.read, write: f.write, from: None }).collect(),
        input_folder: runtime.permissions.input_folder,
        fuel: Some(runtime.limits.fuel),
        memory_mib: Some(runtime.limits.memory_mib),
        net: allow_net && !runtime.permissions.connect.is_empty(),
    };
    let wanted = case.grants.as_ref().unwrap_or(&asked);
    if wanted.net && !allow_net {
        return Err("grants: this case connects to servers; run plugin test with --allow-net to let it".into());
    }
    // Every name is one the manifest asks for (a plain lowercase id) before any folder is made, so
    // a case's grants never reach outside its own folder.
    for folder in &wanted.folders {
        if !runtime.permissions.files.iter().any(|f| f.name == folder.name) {
            let asked: Vec<&str> = runtime.permissions.files.iter().map(|f| f.name.as_str()).collect();
            return Err(format!("grants: the plugin asks for no folder `{}`; it asks for [{}]", folder.name, asked.join(", ")));
        }
    }
    let mut folders = Vec::new();
    for folder in &wanted.folders {
        let path = work.join("folders").join(&folder.name);
        std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        folders.push(FileGrant { name: folder.name.clone(), folder: path, read: folder.read, write: folder.write });
    }
    let grants = Grants {
        package_hash: Some(approval.to_string()),
        command_groups: wanted.command_groups.clone(),
        folders,
        fuel: wanted.fuel,
        memory_mib: wanted.memory_mib,
        input_folder: wanted.input_folder,
        net: wanted.net,
    };
    solidcraft_engine::plugin::validate_grants(manifest, &grants).map_err(|e| format!("grants: {e}"))?;
    // Only a grant the app accepts is seeded from its fixture.
    for (folder, grant) in wanted.folders.iter().zip(&grants.folders) {
        if let Some(from) = &folder.from {
            copy_tree(&unlinked_in_tests(tests, from)?, &grant.folder)?;
        }
    }
    Ok(grants)
}

/// A folder in the tests folder to copy from, refused when it or a folder on the way to it is a
/// link: a link could reach outside the tests folder, so its files would be copied into a grant.
fn unlinked_in_tests(tests: &Path, from: &str) -> Result<PathBuf, String> {
    let path = in_tests(tests, from, "from")?;
    let mut at = tests.to_path_buf();
    let meta = std::fs::symlink_metadata(&at).map_err(|e| format!("from `{from}`: {}: {e}", at.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("from `{from}`: {} is a link; copy the files into the tests folder instead", at.display()));
    }
    for part in Path::new(from).components() {
        at.push(part);
        let meta = std::fs::symlink_metadata(&at).map_err(|e| format!("from `{from}`: {}: {e}", at.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!("from `{from}`: {} is a link; copy the files into the tests folder instead", at.display()));
        }
    }
    Ok(path)
}

/// Copies the files of `from` into `to`, folders and all; links are left out.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let mut pending = vec![(from.to_path_buf(), to.to_path_buf())];
    let mut copied = 0;
    while let Some((from, to)) = pending.pop() {
        for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
            copied += 1;
            if copied > 4096 {
                return Err(format!("{} holds more than 4096 files", from.display()));
            }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let target = to.join(entry.file_name());
            if kind.is_dir() {
                std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
                pending.push((entry.path(), target));
            } else if kind.is_file() {
                std::fs::copy(entry.path(), &target).map_err(|e| format!("{}: {e}", target.display()))?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_case(
    manifest: &Manifest,
    installed: &Path,
    plugins: &Path,
    tests: &Path,
    work: &Path,
    case: &Case,
    out: Option<&Path>,
    name: &str,
    allow_net: bool,
) -> CaseResult {
    let mut journal = None;
    let mut design = None;
    let failure = (|| -> Result<(), String> {
        let runtime = manifest.runtime.as_ref().ok_or("the plugin has no runtime")?;
        let entry = runtime.entries.iter().find(|e| e.name == case.entry).ok_or_else(|| {
            let names: Vec<&str> = runtime.entries.iter().map(|e| e.name.as_str()).collect();
            format!("no entry `{}`; the plugin has {}", case.entry, names.join(", "))
        })?;
        let mut s = match (&case.design, entry.kind) {
            (Some(_), EntryKind::Importer) => return Err("an importer makes a new design: give it a \"file\", not a \"design\"".into()),
            (Some(d), EntryKind::Command | EntryKind::Exporter) => crate::load(&in_tests(tests, d, "design")?.to_string_lossy())?,
            (None, _) => Session::default(),
        };
        s.plugin_state.plugins_dir = Some(plugins.to_path_buf());
        let installed_path = installed.to_string_lossy().into_owned();
        let approval = s.execute("plugin.inspect", &json!({"path": installed_path})).map_err(|e| e.to_string())?["approval"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let mut grants = case_grants(manifest, &approval, tests, work, case, allow_net)?;
        grants.folders.sort_by(|a, b| a.name.cmp(&b.name));
        let folders = grants.folders.clone();
        // Approved as an install would store it: in the scratch plugins folder's grant store, which
        // the session reads back before the call, as a later run of the command line does.
        solidcraft_engine::plugin::grant_store::write(plugins, &manifest.id, &grants).map_err(|e| format!("grants: {e}"))?;
        s.plugin_state.stored = Some(Default::default());
        s.plugin_state.refresh_stored();
        if s.plugin_state.grants.get(&manifest.id) != Some(&grants) {
            return Err("grants: the stored approval did not read back".into());
        }
        let params = case.params.clone().unwrap_or_else(|| json!({}));
        let reply = match entry.kind {
            EntryKind::Command => {
                if case.file.is_some() {
                    return Err("a command runs on a design: give it a \"design\", not a \"file\"".into());
                }
                s.execute("plugin.run", &json!({"path": installed_path, "approval": approval, "entry": entry.name, "params": params}))
            }
            EntryKind::Importer => {
                let file = case.file.as_deref().ok_or("an importer test needs a \"file\" to open, in the tests folder")?;
                let file = in_tests(tests, file, "file")?;
                s.execute(
                    "file.import",
                    &json!({"path": file.to_string_lossy(), "format": format!("{}.{}", manifest.id, entry.name), "options": params}),
                )
            }
            EntryKind::Exporter => {
                if case.file.is_some() {
                    return Err("an exporter writes the design: give it a \"design\", not a \"file\"".into());
                }
                // Into the scratch folder, under the case's name and the slot's first extension.
                let extension = entry.extensions.first().map(String::as_str).unwrap_or("out");
                std::fs::create_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))?;
                let path = work.join(format!("{name}.{extension}"));
                s.execute(
                    "file.export",
                    &json!({"path": path.to_string_lossy(), "format": format!("{}.{}", manifest.id, entry.name), "options": params}),
                )
            }
        };
        if let Some(out) = out {
            let log = out.join(format!("{name}.log"));
            std::fs::write(&log, s.log.join("\n") + "\n").map_err(|e| format!("{}: {e}", log.display()))?;
            journal = Some(log.to_string_lossy().into_owned());
            if reply.is_ok() {
                let file = out.join(format!("{name}.sc"));
                s.execute("file.save_as", &json!({"path": file.to_string_lossy()})).map_err(|e| e.to_string())?;
                design = Some(file.to_string_lossy().into_owned());
            }
        }
        let tolerance = case.expect.tolerance.unwrap_or(TOLERANCE);
        match (reply, &case.expect.error) {
            (Ok(reply), None) => {
                if let Some(expected) = &case.expect.reply {
                    contains(&reply, expected, tolerance, "reply")?;
                }
            }
            (Ok(_), Some(kind)) => return Err(format!("succeeded, and the test expects a {kind} error")),
            (Err(e), None) => return Err(format!("failed: {e}")),
            (Err(e), Some(kind)) => {
                let got = error_kind(&e);
                if !kind_matches(kind, &got) {
                    return Err(format!("failed with a {got} error, and the test expects {kind}: {e}"));
                }
                if let Some(text) = &case.expect.message
                    && !e.to_string().contains(text.as_str())
                {
                    return Err(format!("the error does not say \"{text}\": {e}"));
                }
            }
        }
        if let Some(expected) = &case.expect.model {
            let measured = s.execute("inspect.measure", &json!({})).map_err(|e| e.to_string())?;
            contains(&measured, expected, tolerance, "model")?;
        }
        for file in &case.expect.files {
            let (grant, path) = file.split_once('/').ok_or_else(|| format!("expected file `{file}` is not <folder name>/<path>"))?;
            let folder = folders.iter().find(|f| f.name == grant).ok_or_else(|| format!("expected file `{file}`: no folder `{grant}` is granted"))?;
            if !folder.folder.join(path).is_file() {
                return Err(format!("the plugin did not write {file}"));
            }
        }
        Ok(())
    })()
    .err();
    CaseResult { failure, design, journal }
}

/// Whether an error of kind `got` is the `expected` one. `limit_exceeded` also covers the two
/// limit errors that carry numbers, `exhausted` (a budget ran out) and `over_ceiling` (refused
/// before it ran), so a test written before they existed still passes.
fn kind_matches(expected: &str, got: &str) -> bool {
    expected == "any" || expected == got || (expected == "limit_exceeded" && matches!(got, "exhausted" | "over_ceiling"))
}

/// The kind of an engine error, as a plugin error names it (`permission_denied`, ...), or `other`.
fn error_kind(e: &EngineError) -> String {
    if let Some(e) = e.host_error() {
        return serde_json::to_value(e).ok().and_then(|v| v["kind"].as_str().map(String::from)).unwrap_or_else(|| "other".into());
    }
    match e {
        EngineError::BadParams { .. } => "invalid_input".into(),
        EngineError::Cancelled(_) => "cancelled".into(),
        _ => "other".into(),
    }
}

/// Whether `actual` holds `expected`: every key of an expected object with a matching value, an
/// array of the same length whose items match, numbers within `tolerance`.
fn contains(actual: &Value, expected: &Value, tolerance: f64, at: &str) -> Result<(), String> {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            for (key, value) in expected {
                let at = format!("{at}.{key}");
                let found = actual.get(key).ok_or_else(|| format!("{at} is missing"))?;
                contains(found, value, tolerance, &at)?;
            }
            Ok(())
        }
        (Value::Array(a), Value::Array(e)) if a.len() == e.len() => {
            a.iter().zip(e).enumerate().try_for_each(|(i, (a, e))| contains(a, e, tolerance, &format!("{at}[{i}]")))
        }
        (Value::Number(a), Value::Number(e)) => {
            let (a, e) = (a.as_f64().unwrap_or(f64::NAN), e.as_f64().unwrap_or(f64::NAN));
            if (a - e).abs() <= tolerance * e.abs().max(1.0) { Ok(()) } else { Err(format!("{at} is {a}, and the test expects {e}")) }
        }
        _ if actual == expected => Ok(()),
        _ => Err(format!("{at} is {actual}, and the test expects {expected}")),
    }
}
