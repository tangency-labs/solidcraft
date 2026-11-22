//! `plugin.check`: everything the app checks before it installs or runs a plugin, on a package
//! that is not installed, without running any of its code. It goes through the same code as
//! Install ([`install::from_files`]) and a call ([`craft_plugin_wasm::check_component`]), so the
//! check and the app cannot disagree, and adds to each problem how to fix it.
use super::install::{self, InstallError, Unpacked};
use super::*;
use crate::schema::plugin::{CheckProblem, CheckResult, Severity};
use solidcraft_io::safe_zip;

/// Where a checked package came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A package folder, read as its `.zip` would hold it, without its `tests` folder.
    Folder,
    /// A plugin `.zip`, as Install reads it.
    Zip,
}

/// The folder in a package folder that holds the plugin's own tests (`plugin test`). It is not
/// part of what ships, so the check leaves it out.
pub const TESTS_FOLDER: &str = "tests";

fn problem(severity: Severity, field: impl Into<String>, problem: impl Into<String>, fix: impl Into<String>) -> CheckProblem {
    CheckProblem { severity, field: field.into(), problem: problem.into(), fix: fix.into() }
}
fn error_problem(field: impl Into<String>, text: impl Into<String>, fix: impl Into<String>) -> CheckProblem {
    problem(Severity::Error, field, text, fix)
}

/// Reads and checks the package at `path` as Install would: a `.zip`, or a folder's files with
/// the same limits. The error is the problem that stopped it.
pub fn read(path: &str) -> std::result::Result<(Unpacked, Source), CheckProblem> {
    let is_zip = Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if is_zip {
        let zip = solidcraft_io::vfs::read(path, install::MAX_ZIP_BYTES)
            .map_err(|e| error_problem("package", format!("could not read {path}: {e}"), "Give the path of the plugin's .zip, at most 80 MiB."))?;
        return install::unpack(&zip).map(|u| (u, Source::Zip)).map_err(|e| install_problem(&e));
    }
    let meta = solidcraft_io::vfs::check(path).and_then(|()| std::fs::metadata(path)).map_err(|e| {
        error_problem("package", format!("could not read {path}: {e}"), "Give the plugin's package folder (the one with plugin.json) or its .zip.")
    })?;
    if !meta.is_dir() {
        return Err(error_problem(
            "package",
            format!("{path} is neither a folder nor a .zip"),
            "Give the plugin's package folder (the one with plugin.json) or its .zip.",
        ));
    }
    let files = read_folder(Path::new(path))?;
    install::from_files(files, String::new()).map(|u| (u, Source::Folder)).map_err(|e| install_problem(&e))
}

/// A package folder's files, by `/`-separated relative path, held to the limits of a plugin
/// `.zip`. Hidden names (`.git`) and the `tests` folder are left out; a link is refused, as a
/// `.zip` entry that is one is.
fn read_folder(root: &Path) -> std::result::Result<Vec<safe_zip::File>, CheckProblem> {
    let limits = install::LIMITS;
    let too_big = |what: String| {
        error_problem(
            "package",
            what,
            "Keep only plugin.json, the component and the files it ships (icons, a README) in the package folder; build the component elsewhere and copy it in.",
        )
    };
    let unreadable = |path: &Path, e: &dyn std::fmt::Display| {
        error_problem("package", format!("could not read {}: {e}", path.display()), "Check the folder's permissions.")
    };
    let mut files = Vec::new();
    let mut total = 0u64;
    // Files and folders alike, so a deep tree of empty folders ends too.
    let mut seen = 0usize;
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        solidcraft_io::vfs::check(&dir.to_string_lossy()).map_err(|e| unreadable(&dir, &e))?;
        // Counted while listing, so one huge folder is not listed in full before the limit
        // applies; an entry the listing cannot read is an error, not a file left out.
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| unreadable(&dir, &e))? {
            let entry = entry.map_err(|e| unreadable(&dir, &e))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || (prefix.is_empty() && name == TESTS_FOLDER) {
                continue;
            }
            seen += 1;
            if seen > super::MAX_FILES {
                return Err(too_big(format!("the package holds more than {} files and folders", super::MAX_FILES)));
            }
            entries.push((name, entry));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, entry) in entries {
            let relative = safe_zip::checked_path(&format!("{prefix}{name}")).map_err(|reason| {
                error_problem(
                    "package",
                    format!("{prefix}{name} has {reason}"),
                    "Rename the file to a plain name: Install refuses a .zip entry with this name.",
                )
            })?;
            let kind = entry.file_type().map_err(|e| unreadable(&entry.path(), &e))?;
            if kind.is_symlink() {
                return Err(error_problem(
                    "package",
                    format!("{relative} is a link"),
                    "Copy the file itself into the package: Install refuses links.",
                ));
            }
            if kind.is_dir() {
                pending.push((entry.path(), format!("{relative}/")));
                continue;
            }
            if files.len() >= limits.max_entries {
                return Err(too_big(format!("the package has more than {} files", limits.max_entries)));
            }
            let path = entry.path();
            let max = usize::try_from(limits.max_entry_bytes).unwrap_or(usize::MAX);
            let bytes = solidcraft_io::vfs::read(&path.to_string_lossy(), max)
                .map_err(|e| too_big(format!("{relative} could not be read within {} MiB: {e}", limits.max_entry_bytes >> 20)))?;
            total += bytes.len() as u64;
            if total > limits.max_total_bytes {
                return Err(too_big(format!("the package is larger than {} MiB", limits.max_total_bytes >> 20)));
            }
            files.push(safe_zip::File { path: relative, bytes });
        }
    }
    Ok(files)
}

/// The capabilities this SolidCraft serves, as a manifest lists them.
fn served() -> String {
    craft_plugin::CAPABILITIES.iter().map(|(name, (major, minor))| format!("\"{name}@{major}.{minor}\"")).collect::<Vec<_>>().join(", ")
}

/// What to change for a refusal of `field`.
fn fix_for(field: &str) -> String {
    let fix = match field {
        "id" => "Set \"id\" to 1 to 64 lowercase letters, digits and dashes, not starting or ending with a dash, such as \"gear-maker\".",
        "name" => "Give \"name\" 1 to 80 printable characters on one line.",
        "version" => "Use a version such as \"1.0.0\": at most 32 letters, digits, dots, dashes and plus signs.",
        "description" => "Shorten \"description\" to at most 240 characters on one line; longer help goes in a README in the package.",
        "manifest" => "plugin.json must be one JSON object with only the fields the plugin guide lists; the message names the field and where it is.",
        "plugin.json" => "Put plugin.json at the top of the package (in a .zip, at its top or in its one folder), at most 16 KiB.",
        "runtime" => "Add a \"runtime\" with the component's \"module\", its \"entries\" and the \"commands\" it calls.",
        "runtime.module" => {
            "Name the component in \"runtime.module\": one lowercase file name ending in .wasm, such as \"gear-maker.wasm\", beside plugin.json."
        }
        "runtime.commands" => {
            "List exact command ids from `solidcraft-cli commands`; a command that reaches past the open design also needs its group in runtime.permissions.command_groups. An importer lists none."
        }
        "runtime.entries" => "Declare 1 to 32 entries with distinct lowercase names, all commands or all importers.",
        "runtime.entries.slots" => {
            "Give each entry at most 16 slots, each {\"kind\": \"menu\" or \"ribbon\", \"name\": \"...\"} with a name of at most 80 characters."
        }
        "runtime.permissions" => {
            "An importer reads only its file (and, with input_folder, the files beside it): remove \"files\" and \"command_groups\"."
        }
        "runtime.permissions.command_groups" => "Ask only for the groups app, doc, file and plugin, and only for those the listed commands need.",
        "runtime.permissions.files" => {
            "Give each folder a distinct lowercase name and ask for read, write or both: {\"name\": \"exports\", \"write\": true}."
        }
        "runtime.permissions.input_folder" => "Remove \"input_folder\": only an importer reads the folder of the file it opens.",
        "runtime.permissions.net" => {
            "Remove \"net\": SolidCraft gives plugins no network access. Read and write files through named folder grants instead."
        }
        "runtime.limits.fuel" => "Set \"fuel\" to a positive number that fits the app's ceiling (100000000000 unless the user raised it).",
        "runtime.limits.memory_mib" => {
            "Set \"memory_mib\" to 1..4096. Above the user's ceiling (128 unless raised), Install asks the user to approve it."
        }
        "runtime.limits.deadline_ms" => "Set \"deadline_ms\" to at most 30000 for a command, 600000 for an importer.",
        "runtime.limits.stack_kib" => "Set \"stack_kib\" to 64..65536; a command may ask for at most 1024.",
        "capabilities" => "",
        "component" => "Build the component for this app's WIT (crates/craft-plugin/wit/plugin.wit) and wrap it with `wasm-tools component new`.",
        "package" => {
            "Ship one plugin: plugin.json and its component, at the top of the .zip or in its one folder, with plain relative paths and no links."
        }
        _ => "",
    };
    if field == "capabilities" {
        return format!(
            "List the capabilities the plugin uses: \"commands@0.2\" for a command plugin, \"formats@0.2\" for an importer (with \"evaluate@0.2\" to check its result). This SolidCraft serves {}, and still runs plugins built for 0.1.",
            served()
        );
    }
    if !fix.is_empty() {
        return fix.into();
    }
    if field.starts_with("runtime.entries.") && field.ends_with(".extensions") {
        return "List 1 to 32 distinct lowercase extensions without the dot, such as [\"f3d\"], on an importer entry only.".into();
    }
    if field.starts_with("runtime.entries.") && field.ends_with(".effect") {
        return "An importer makes a design: give its entry \"effect\": \"edit\" or leave it out.".into();
    }
    if field.starts_with("runtime.entries.") && field.contains(".input_schema") {
        return "Make the entry's inputs an object schema, {\"type\": \"object\", \"properties\": {...}}; selection, target, targets, bodies and body_ids are arrays.".into();
    }
    "See the plugin guide for this field.".into()
}

/// The problem a [`HostError`] from the app's own checks stands for, with how to fix it.
fn host_problem(e: &HostError) -> CheckProblem {
    let text = e.to_string();
    match e {
        HostError::InvalidManifest { field, reason } => error_problem(field.clone(), reason.clone(), fix_for(field)),
        HostError::UnsupportedCapability { reason, .. } => error_problem(
            "capabilities",
            reason.clone(),
            format!(
                "Rebuild the plugin against this SolidCraft's {API} WIT (crates/craft-plugin/wit/plugin.wit) and list the capabilities it serves: {}.",
                served()
            ),
        ),
        HostError::PermissionDenied { permission } if permission == "net" => {
            error_problem("runtime.permissions.net", "asks for network access (net)", fix_for("runtime.permissions.net"))
        }
        HostError::PermissionDenied { permission } if permission.starts_with("network import") => error_problem(
            "component",
            permission.clone(),
            "Remove the network import: SolidCraft gives plugins no network access, so build without HTTP or socket crates.",
        ),
        HostError::LimitExceeded { limit } if limit.contains("manifest") => {
            error_problem("plugin.json", limit.clone(), "Keep plugin.json under 16 KiB: move long text into a README in the package.")
        }
        HostError::LimitExceeded { limit } if limit.contains("component") => error_problem(
            "component",
            limit.clone(),
            "A command's component may be 16 MiB and an importer's 64 MiB: build in release mode with LTO and strip debug sections (`wasm-tools strip`).",
        ),
        HostError::LimitExceeded { limit } if limit.contains("stack") => {
            error_problem("runtime.limits.stack_kib", limit.clone(), fix_for("runtime.limits.stack_kib"))
        }
        HostError::InvalidInput { field, reason } if field == "component" || field == "stack_bytes" => {
            error_problem("component", reason.clone(), component_fix(reason))
        }
        HostError::InvalidInput { field, reason } => error_problem(field.clone(), reason.clone(), fix_for(field)),
        HostError::Trap { reason } if reason.contains("compil") => error_problem(
            "component",
            reason.clone(),
            "The component does not compile: build it again for wasm32-unknown-unknown and wrap it with `wasm-tools component new`.",
        ),
        _ => error_problem("package", text, fix_for("package")),
    }
}

/// How to fix a component that does not link to the app's world.
fn component_fix(reason: &str) -> String {
    if reason.contains("core WebAssembly module") {
        return "Wrap the module as a component: `wasm-tools component new plugin.wasm -o plugin.wasm` (after `cargo build --target wasm32-unknown-unknown`).".into();
    }
    if reason.contains("wasi:") {
        return "The app links no WASI: build for wasm32-unknown-unknown (not wasm32-wasip1 or wasip2), and keep crates that read the clock, environment or files through std out of the plugin.".into();
    }
    if reason.contains("import") {
        return format!(
            "Import only the interfaces of the plugin's world, from {API}: world command-plugin for a command plugin, world importer-plugin for an importer. Generate the bindings from this SolidCraft's crates/craft-plugin/wit/plugin.wit."
        );
    }
    "Export the world's interface: `entry` for a command plugin (world command-plugin), `importer` for an importer (world importer-plugin), generated from crates/craft-plugin/wit/plugin.wit.".into()
}

/// The problem an [`InstallError`] stands for.
fn install_problem(e: &InstallError) -> CheckProblem {
    match e {
        InstallError::Manifest(e) => host_problem(e),
        InstallError::MissingManifest => error_problem("plugin.json", e.to_string(), fix_for("plugin.json")),
        InstallError::MissingModule(module) | InstallError::SourceOnly(module) => error_problem(
            "runtime.module",
            e.to_string(),
            format!("Build the component and put it beside plugin.json as {module}, the file runtime.module names."),
        ),
        _ => error_problem("package", e.to_string(), fix_for("package")),
    }
}

/// Checks the package at `path` (a folder or a `.zip`) against this session's plugin ceilings.
pub fn check(session: &Session, path: &str) -> CheckResult {
    let mut result = CheckResult {
        ok: false,
        path: path.into(),
        id: None,
        name: None,
        version: None,
        kind: None,
        approval: None,
        entries: Vec::new(),
        problems: Vec::new(),
        param_warnings: Vec::new(),
    };
    let (package, source) = match read(path) {
        Ok(read) => read,
        Err(problem) => {
            result.problems.push(problem);
            return result;
        }
    };
    for warning in package.swatch_warnings() {
        result.problems.push(problem(
            Severity::Warning,
            craft_plugin::swatches::KEY,
            warning,
            "Use a package PNG or JPEG of at most 64 KiB and 256 × 256 pixels.",
        ));
    }
    let manifest = &package.manifest;
    result.id = Some(manifest.id.clone());
    result.name = Some(manifest.name.clone());
    result.version = Some(manifest.version.clone());
    result.approval = Some(package.approval.clone());
    if source == Source::Folder {
        let folder = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if folder != manifest.id {
            result.problems.push(problem(
                Severity::Warning,
                "id",
                format!("the folder is `{folder}`, and the plugin's id is `{}`", manifest.id),
                "Fine for a .zip, which installs into a folder named after the id; name the folder after the id to run it in place with plugin.inspect.",
            ));
        }
    }
    let Some(runtime) = manifest.runtime.as_ref() else {
        result.kind = Some("metadata".into());
        result.problems.push(problem(
            Severity::Warning,
            "runtime",
            "the plugin has no runtime, so it adds no commands or importers",
            fix_for("runtime"),
        ));
        result.ok = true;
        return result;
    };
    result.kind = Some(if runtime.is_importer() { "importer" } else { "command" }.into());
    result.entries = runtime.entries.clone();
    let ceilings = session.plugin_limits;
    if runtime.limits.fuel > ceilings.max_fuel {
        result.problems.push(error_problem(
            "runtime.limits.fuel",
            format!("asks for {} fuel, over the app's ceiling of {}", runtime.limits.fuel, ceilings.max_fuel),
            format!("Lower \"fuel\" to at most {}: the app refuses to run a plugin that asks for more.", ceilings.max_fuel),
        ));
    }
    if runtime.limits.memory_mib > ceilings.max_memory_mib {
        result.problems.push(problem(
            Severity::Warning,
            "runtime.limits.memory_mib",
            format!("asks for {} MiB of memory, over the app's ceiling of {} MiB", runtime.limits.memory_mib, ceilings.max_memory_mib),
            "Install asks the user to allow the extra memory; a plugin copied in by hand runs only once the user raises Max plugin memory (MiB) in Preferences › Plugins (in solidcraft-cli, max_memory_mib in the --plugin-limits file).",
        ));
    }
    #[cfg(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64")))]
    if let Some(module) = package.files.iter().find(|f| f.path == runtime.module)
        && let Err(e) = craft_plugin_wasm::check_component(manifest, &module.bytes)
    {
        result.problems.push(host_problem(&e));
    }
    #[cfg(not(all(feature = "plugins", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    result.problems.push(problem(
        Severity::Warning,
        "component",
        "this build runs no plugins, so the component was not checked",
        "Run the check with the desktop command line, solidcraft-cli.",
    ));
    result.problems.sort_by_key(|p| p.severity != Severity::Error);
    result.ok = !result.problems.iter().any(|p| p.severity == Severity::Error);
    result
}
