//! Slicers a mesh can be sent to: find the installed ones on Linux, macOS and Windows, start one
//! with a file, and read its application icon (never shipped: it comes from the installed
//! application).
//!
//! Detection reads desktop entries and Flatpak exports (Linux), `.app` bundles (macOS) and the
//! program folders (Windows), plus `PATH` and the usual AppImage folders. On Linux a desktop entry
//! that claims 3MF or STL files is listed too, even when it is not a slicer we know by name. Nothing here runs a
//! program except [`Slicer::open`].

use std::path::{Path, PathBuf};

use crate::slicer_icon;

/// Most directory entries looked at in one folder.
const MAX_ENTRIES: usize = 5_000;
/// Largest desktop entry read.
const MAX_DESKTOP_BYTES: u64 = 64 * 1024;
/// The file types (`MimeType=`) that make a desktop entry a program to send a mesh to: 3MF (and
/// the name Windows software gave it) and STL.
const MESH_TYPES: &[&str] = &["model/3mf", "application/vnd.ms-3mfdocument", "model/stl", "application/sla"];

/// A slicer the user can send a mesh to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slicer {
    /// `prusa`, `bambu`, … for the known ones; `custom` for a program the user picked.
    pub id: String,
    pub name: String,
    /// The program and its leading arguments; the file to open is appended.
    pub command: Vec<String>,
    pub icon: Option<IconSource>,
    /// For PrusaSlicer, the major version of this install (`3` for `3.0.0-alpha10`), read from its
    /// appstream metadata when it was detected (see [`prusa_major`]); `None` when that can't be read,
    /// and for every other program. Never worked out by running the program.
    pub prusa_major: Option<u32>,
}

/// Where an application's icon is read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconSource {
    /// A PNG file (Linux icon themes).
    Image(PathBuf),
    /// A macOS `.icns` file.
    Icns(PathBuf),
    /// A Windows program with icon resources.
    Exe(PathBuf),
}

impl IconSource {
    /// The icon as PNG or ICO bytes, ready for an image decoder; `None` when it can't be read.
    pub fn load(&self) -> Option<Vec<u8>> {
        match self {
            IconSource::Image(p) => slicer_icon::read_capped(p, 4 << 20),
            IconSource::Icns(p) => slicer_icon::icns_png(&slicer_icon::read_capped(p, 32 << 20)?),
            IconSource::Exe(p) => slicer_icon::exe_icon(p),
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            IconSource::Image(p) | IconSource::Icns(p) | IconSource::Exe(p) => p,
        }
    }
}

/// What a mesh can be sent to. `Slicer` is the only kind there is. It is the extension point for a
/// later Send to App: a new kind would name programs that are not slicers (a viewer, a CAM tool, a
/// converter), each with its own accepted formats and way of taking a file, and would send through
/// the same pipeline as `file.send_to_slicer` (write the file, find the target, start it and wait if
/// it needs that, hand the file over, report).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Slicer,
}

/// How a target takes the file it is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileArg {
    /// As an argument at the end of its command, or inside the `@@ … @@` pair of a Flatpak command.
    Argument,
}

/// A program a mesh can be sent to, and what is known about how to do it. One row per known program;
/// a program that is not in the table (a desktop entry that claims 3MF or STL, one the user picked) is
/// a slicer with [`Target::SLICER`]'s defaults.
pub struct Target {
    pub id: &'static str,
    /// The label shown to the user.
    pub name: &'static str,
    pub kind: TargetKind,
    /// Names the application goes by, letters and digits only, lower case; an entry matches when
    /// its name starts with one of them.
    pub keys: &'static [&'static str],
    /// Program file names, how it is found and started when it has no desktop entry.
    pub exes: &'static [&'static str],
    /// The mesh formats it takes, as `file.send_to_slicer` names them, and the one used when none is asked for.
    pub formats: &'static [&'static str],
    pub default_format: &'static str,
    pub file: FileArg,
    /// The flag that makes a running instance take the file instead of a new one starting, if it has one.
    pub single_instance: Option<&'static str>,
    /// The first major version whose `--single-instance-on-url` hands a file over; in an older one the
    /// flag is swapped for `single_instance`.
    pub on_url_swap_below: Option<u32>,
    /// When it has to be started alone and waited for before it can be sent a file.
    pub ready: Option<WaitRule>,
}

impl Target {
    /// The defaults of a slicer: every mesh format, a file as an argument, no flag, no wait.
    pub const SLICER: Target = Target {
        id: "",
        name: "",
        kind: TargetKind::Slicer,
        keys: &[],
        exes: &[],
        formats: &["3mf", "stl", "3mf-tagged"],
        default_format: "3mf",
        file: FileArg::Argument,
        single_instance: None,
        on_url_swap_below: None,
        ready: None,
    };

    pub fn accepts(&self, format: &str) -> bool {
        self.formats.contains(&format)
    }

    /// Whether the version of an install matters to how it is sent to, and so is read when it is found.
    fn reads_version(&self) -> bool {
        self.on_url_swap_below.is_some() || self.ready.is_some()
    }
}

/// The known programs, in the order they are listed.
const TARGETS: &[Target] = &[
    Target {
        id: "prusa",
        name: "PrusaSlicer",
        keys: &["prusaslicer"],
        exes: &["prusa-slicer", "PrusaSlicer", "prusa-slicer.exe"],
        single_instance: Some("--single-instance"),
        on_url_swap_below: Some(3),
        ready: Some(PRUSA_READY),
        ..Target::SLICER
    },
    Target {
        id: "bambu",
        name: "Bambu Studio",
        keys: &["bambustudio"],
        exes: &["bambu-studio", "BambuStudio", "bambu-studio.exe"],
        ..Target::SLICER
    },
    Target { id: "orca", name: "OrcaSlicer", keys: &["orcaslicer"], exes: &["orca-slicer", "OrcaSlicer", "orca-slicer.exe"], ..Target::SLICER },
    Target {
        id: "cura",
        name: "UltiMaker Cura",
        keys: &["ultimakercura", "curaultimaker", "cura"],
        exes: &["cura", "UltiMaker-Cura", "UltiMaker-Cura.exe", "Cura.exe"],
        single_instance: Some("--single-instance"),
        ..Target::SLICER
    },
    Target {
        id: "superslicer",
        name: "SuperSlicer",
        keys: &["superslicer"],
        exes: &["superslicer", "SuperSlicer", "superslicer.exe"],
        single_instance: Some("--single-instance"),
        ..Target::SLICER
    },
    Target {
        id: "creality",
        name: "Creality Print",
        keys: &["crealityprint"],
        exes: &["CrealityPrint", "creality-print", "CrealityPrint.exe"],
        ..Target::SLICER
    },
    Target {
        id: "elegoo",
        name: "ElegooSlicer",
        keys: &["elegooslicer"],
        exes: &["elegoo-slicer", "ElegooSlicer", "ElegooSlicer.exe"],
        ..Target::SLICER
    },
    Target {
        id: "anycubic",
        name: "AnycubicSlicerNext",
        keys: &["anycubicslicer"],
        exes: &["AnycubicSlicerNext", "AnycubicSlicerNext.exe"],
        ..Target::SLICER
    },
    Target { id: "ideamaker", name: "ideaMaker", keys: &["ideamaker"], exes: &["ideamaker", "ideaMaker.exe"], ..Target::SLICER },
    Target { id: "lychee", name: "Lychee Slicer", keys: &["lycheeslicer"], exes: &["lychee-slicer", "Lychee Slicer.exe"], ..Target::SLICER },
    Target { id: "slic3r", name: "Slic3r", keys: &["slic3r"], exes: &["slic3r", "Slic3r", "slic3r.exe"], ..Target::SLICER },
];

/// Lower-case letters and digits only: how names are compared.
fn normalize(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// The known slicer an application name (or file name) belongs to.
fn known_for(name: &str) -> Option<&'static Target> {
    let n = normalize(name);
    // Companion tools (G-code viewers, command-line builds) are not slicers to send a mesh to.
    if n.contains("viewer") || n.contains("console") || n.contains("gcode") || n.contains("uninstall") {
        return None;
    }
    TARGETS.iter().find(|k| k.keys.iter().any(|key| n.starts_with(key)))
}

/// Folders and variables detection looks at, so it can be tested without the real machine.
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// The user's home folder.
    pub home: Option<PathBuf>,
    /// `PATH`.
    pub path_dirs: Vec<PathBuf>,
    /// XDG data folders (each holds `applications/` and `icons/`).
    pub data_dirs: Vec<PathBuf>,
    /// Folders holding AppImages, loose programs and per-slicer folders (Linux).
    pub program_dirs: Vec<PathBuf>,
    /// Folders holding `.app` bundles (macOS).
    pub app_dirs: Vec<PathBuf>,
    /// Folders holding one folder per installed program (Windows).
    pub install_dirs: Vec<PathBuf>,
    /// Flatpak installations (`~/.local/share/flatpak`, `/var/lib/flatpak`), where an app's
    /// metadata says its version.
    pub flatpak_roots: Vec<PathBuf>,
}

impl Env {
    /// The running machine.
    pub fn current() -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = var("HOME").or_else(|| var("USERPROFILE"));
        let mut env = Env {
            home: home.clone(),
            path_dirs: std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default(),
            ..Env::default()
        };
        if cfg!(target_os = "macos") {
            env.app_dirs.push("/Applications".into());
            env.app_dirs.extend(home.iter().map(|h| h.join("Applications")));
        } else if cfg!(target_os = "windows") {
            for k in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
                env.install_dirs.extend(var(k));
            }
            if let Some(local) = var("LOCALAPPDATA") {
                env.install_dirs.push(local.join("Programs"));
                env.install_dirs.push(local);
            }
        } else {
            let data_home = var("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local/share")));
            env.data_dirs.extend(data_home.clone());
            env.data_dirs.extend(home.iter().map(|h| h.join(".local/share/flatpak/exports/share")));
            env.data_dirs.push("/var/lib/flatpak/exports/share".into());
            env.data_dirs.push("/var/lib/snapd/desktop".into());
            match std::env::var_os("XDG_DATA_DIRS").filter(|v| !v.is_empty()) {
                Some(dirs) => env.data_dirs.extend(std::env::split_paths(&dirs)),
                None => env.data_dirs.extend(["/usr/local/share".into(), "/usr/share".into()]),
            }
            for dir in ["Applications", "AppImages", "Apps", ".local/bin", "bin"] {
                env.program_dirs.extend(home.iter().map(|h| h.join(dir)));
            }
            env.program_dirs.extend(["/opt", "/usr/local/bin"].map(PathBuf::from));
            env.flatpak_roots.extend(home.iter().map(|h| h.join(".local/share/flatpak")));
            env.flatpak_roots.push("/var/lib/flatpak".into());
        }
        env
    }
}

/// The slicers installed on this machine, known ones in a fixed order. Empty in the browser.
pub fn detect() -> Vec<Slicer> {
    #[cfg(test)]
    if let Some(made_up) = TEST_DETECTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
        return made_up;
    }
    if cfg!(target_arch = "wasm32") { Vec::new() } else { detect_in(&Env::current()) }
}

/// Tests of the commands set the slicers `detect` finds, and the `busctl` that is asked, to ones they made up.
#[cfg(test)]
pub(crate) static TEST_DETECTED: std::sync::Mutex<Option<Vec<Slicer>>> = std::sync::Mutex::new(None);
#[cfg(test)]
pub(crate) static TEST_BUSCTL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The program that is asked what is on the session bus: `busctl`.
pub(crate) fn busctl_program() -> String {
    #[cfg(test)]
    if let Some(made_up) = TEST_BUSCTL.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
        return made_up;
    }
    "busctl".to_string()
}

/// [`detect`] over the folders in `env`. Every install of a slicer is listed: when there are
/// several of one, each gets its own id (`prusa#<where it was found>`, the same on every run) and a
/// name that says which it is (`PrusaSlicer (system)`, `PrusaSlicer (Flatpak, beta)`).
pub fn detect_in(env: &Env) -> Vec<Slicer> {
    // (slicer, which program it starts, where it was found)
    let mut found: Vec<(Slicer, String, PathBuf)> = Vec::new();
    let mut add = |s: Slicer, origin: &Path| {
        let key = install_key(env, &s.command);
        // The same program found twice (a desktop entry and PATH) is one install, and which of the
        // places it was found in speaks for it must not depend on which was read first: the one
        // that sorts first does, so its file name, and so the id, is the same on every run.
        if let Some(at) = found.iter().position(|(_, k, _)| *k == key) {
            if origin < found[at].2.as_path() {
                found[at] = (s, key, origin.to_path_buf());
            }
            return;
        }
        // An unknown program that a second desktop entry names the same is the one too.
        if !found.iter().any(|(f, ..)| f.id == s.id && known_by_id(&s.id).is_none()) {
            found.push((s, key, origin.to_path_buf()));
        }
    };
    desktop_entries(env, &mut add);
    path_programs(env, &mut add);
    program_folders(env, &mut add);
    mac_apps(env, &mut add);
    windows_programs(env, &mut add);
    // Told apart by their program, so the result does not depend on the order they were found in.
    found.sort_by(|a, b| a.1.cmp(&b.1));
    let mut out: Vec<Slicer> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for (s, _, origin) in &found {
        let several = found.iter().filter(|(o, ..)| o.id == s.id).count() > 1;
        let mut slicer = s.clone();
        if slicer.target().reads_version() {
            slicer.prusa_major = prusa_major(env, &slicer.command, origin);
        }
        if several {
            let mut slug = install_slug(origin);
            // Two versions of one AppImage side by side in a folder would have the same id: their file
            // names, which differ, tell them apart, and only while there is more than one.
            let side_by_side = |(o, _, og): &&(Slicer, String, PathBuf)| o.id == s.id && is_appimage(og) && og.parent() == origin.parent();
            if is_appimage(origin) && found.iter().filter(side_by_side).count() > 1 {
                slug = format!("{slug}-{}", normalize(&origin.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            }
            let mut id = format!("{}#{slug}", s.id);
            let mut n = 2;
            while out.iter().any(|o| o.id == id) {
                id = format!("{}#{slug}-{n}", s.id);
                n += 1;
            }
            let mut name = format!("{} ({})", s.name, install_label(env, &s.command, origin));
            let mut n = 2;
            while labels.contains(&name) {
                name = format!("{} ({} {n})", s.name, install_label(env, &s.command, origin));
                n += 1;
            }
            (slicer.id, slicer.name) = (id, name);
        }
        labels.push(slicer.name.clone());
        out.push(slicer);
    }
    let order = |s: &Slicer| (TARGETS.iter().position(|k| k.id == s.base_id()).unwrap_or(usize::MAX), s.name.to_lowercase(), s.id.clone());
    out.sort_by_key(order);
    out
}

/// The major version of an install of PrusaSlicer, from the release at the top of its appstream
/// metadata: for a Flatpak, in the app's own files (the branch the command names, else the current
/// one); for any other install found through a desktop entry, in the `metainfo` or `appdata` folder
/// beside that entry's `applications`. Read from files, never by running the program. `None` when
/// there is no such file or it has no release, in which case the entry's own command decides (see
/// `Slicer::waits_for_ready` and `Slicer::base_args`).
fn prusa_major(env: &Env, command: &[String], origin: &Path) -> Option<u32> {
    let xml = if command.first().is_some_and(|c| file_name(Path::new(c)) == "flatpak") {
        flatpak_metainfo(env, command)?
    } else {
        let stem = origin.file_stem()?.to_string_lossy().into_owned();
        let data = origin.parent()?.parent()?;
        [data.join(format!("metainfo/{stem}.metainfo.xml")), data.join(format!("appdata/{stem}.appdata.xml"))].into_iter().find(|p| p.is_file())?
    };
    if std::fs::metadata(&xml).ok()?.len() > 1 << 20 {
        return None;
    }
    release_major(&std::fs::read_to_string(xml).ok()?)
}

/// The metadata file of the Flatpak a `flatpak run …` command starts.
fn flatpak_metainfo(env: &Env, command: &[String]) -> Option<PathBuf> {
    let mut rest = command.iter().skip(1).skip_while(|a| *a != "run").skip(1);
    let app = rest.find(|a| !a.starts_with('-'))?;
    let branch = command.iter().find_map(|a| a.strip_prefix("--branch="));
    if app.is_empty() || app.contains('/') || branch.is_some_and(|b| b.contains('/')) {
        return None;
    }
    for root in &env.flatpak_roots {
        let app_dir = root.join("app").join(app);
        // The branch the command names, under each architecture; else the one `current` points at.
        let deployments: Vec<PathBuf> = match branch {
            Some(b) => entries(&app_dir).into_iter().filter(|p| p.is_dir() && file_name(p) != "current").map(|arch| arch.join(b)).collect(),
            None => vec![app_dir.join("current")],
        };
        for d in deployments {
            let files = d.join("active/files/share");
            for rel in [format!("metainfo/{app}.metainfo.xml"), format!("appdata/{app}.appdata.xml")] {
                if files.join(&rel).is_file() {
                    return Some(files.join(rel));
                }
            }
        }
    }
    None
}

/// The leading number of the first `<release version="…">`: `3` for `3.0.0-alpha10`.
fn release_major(xml: &str) -> Option<u32> {
    let release = &xml[xml.find("<release ")?..];
    let version = &release[release.find("version=\"")? + "version=\"".len()..];
    let digits: String = version.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn is_appimage(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("appimage"))
}

/// What tells this install's id apart from another install of the same slicer: the name of the file
/// it was found through (the desktop entry, the program), or for an AppImage the folder it is in. An
/// AppImage's file name carries its version, which changes with every upgrade and would make the
/// remembered choice miss; its folder, and the fact that it is an AppImage, do not.
fn install_slug(origin: &Path) -> String {
    match origin.parent().filter(|_| is_appimage(origin)) {
        Some(folder) => {
            // FNV-1a: the same on every release, unlike `DefaultHasher`.
            let hash = folder.to_string_lossy().bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
            format!("appimage-{:08x}", hash as u32)
        }
        None => normalize(&origin.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
    }
}

fn known_by_id(id: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|k| k.id == id.split('#').next().unwrap_or(id))
}

/// The program that `command` starts, as one string: a Flatpak or `open` command by itself, any
/// other by where its program really is (found through `PATH`, symlinks followed).
fn install_key(env: &Env, command: &[String]) -> String {
    let Some(first) = command.first() else { return String::new() };
    let program = Path::new(first);
    // `flatpak run …` and `/usr/bin/flatpak run …` are one invocation when they start the same
    // launcher: it is found as any program is, and its name stands in where it can't be.
    if file_name(program) == "flatpak" {
        let launcher = which(env, program).map(|at| s(&std::fs::canonicalize(&at).unwrap_or(at))).unwrap_or_else(|| "flatpak".into());
        return std::iter::once(launcher.as_str()).chain(command[1..].iter().map(String::as_str)).collect::<Vec<_>>().join("\u{0}");
    }
    if first == "open" {
        return command.join("\u{0}");
    }
    let at = which(env, program).unwrap_or_else(|| program.to_path_buf());
    s(&std::fs::canonicalize(&at).unwrap_or(at))
}

/// A program name or path as an absolute path, looking through `PATH`.
fn which(env: &Env, program: &Path) -> Option<PathBuf> {
    if program.is_absolute() {
        return Some(program.to_path_buf());
    }
    (program.components().count() == 1).then(|| env.path_dirs.iter().map(|d| d.join(program)).find(|p| is_executable(p))).flatten()
}

/// What tells this install from another of the same slicer, worked out from its command and where
/// it was found, without running anything.
fn install_label(env: &Env, command: &[String], origin: &Path) -> String {
    if command.first().is_some_and(|c| file_name(Path::new(c)) == "flatpak") {
        let branch = command.iter().find_map(|a| a.strip_prefix("--branch=")).filter(|b| *b != "stable");
        return branch.map_or_else(|| "Flatpak".to_string(), |b| format!("Flatpak, {b}"));
    }
    // `open -a <app>` starts `open`, wherever that is; the application is what tells installs apart.
    let program = command.first().filter(|c| *c != "open").and_then(|c| which(env, Path::new(c)));
    let at = program.as_deref().unwrap_or(origin);
    if at.extension().is_some_and(|e| e.eq_ignore_ascii_case("appimage")) {
        return "AppImage".into();
    }
    let Some(rel) = env.home.as_deref().and_then(|h| at.strip_prefix(h).ok()) else { return "system".into() };
    if rel.starts_with(".local") {
        return "user".into();
    }
    match rel.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(dir) => format!("~/{}", dir.display()),
        None => "~".into(),
    }
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).take(MAX_ENTRIES).collect()).unwrap_or_default()
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn known_slicer(k: &Target, command: Vec<String>, icon: Option<IconSource>) -> Slicer {
    Slicer { id: k.id.into(), name: k.name.into(), command, icon, prusa_major: None }
}

// --- Linux ---

/// A desktop entry's `[Desktop Entry]` keys.
fn parse_desktop(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut inside = false;
    for line in text.lines().take(2_000) {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
        } else if inside && let Some((k, v)) = line.split_once('=') {
            // Localised keys (`Name[de]`) never replace the plain one.
            if !k.contains('[') {
                out.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
            }
        }
    }
    out
}

/// An `Exec=` line as a command: quotes honoured, field codes (`%F`, …) dropped.
pub fn parse_exec(exec: &str) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    args.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        args.push(cur);
    }
    args.retain(|a| !(a.len() == 2 && a.starts_with('%')));
    args
}

fn desktop_entries(env: &Env, add: &mut impl FnMut(Slicer, &Path)) {
    for data in &env.data_dirs {
        for file in entries(&data.join("applications")) {
            if file.extension().is_none_or(|e| e != "desktop") || std::fs::metadata(&file).map(|m| m.len() > MAX_DESKTOP_BYTES).unwrap_or(true) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let entry = parse_desktop(&text);
            let Some(name) = entry.get("Name") else { continue };
            let flag = |key: &str| entry.get(key).is_some_and(|v| v.eq_ignore_ascii_case("true"));
            if entry.get("Type").is_some_and(|t| t != "Application") || flag("NoDisplay") || flag("Hidden") {
                continue;
            }
            let known = known_for(name);
            // A program we don't know by name counts when it says it opens 3MF or STL files.
            let claims_meshes =
                entry.get("MimeType").is_some_and(|m| m.split(';').any(|t| MESH_TYPES.iter().any(|w| t.trim().eq_ignore_ascii_case(w))));
            let ours = normalize(name).starts_with("solidcraft");
            if known.is_none() && (!claims_meshes || ours || flag("Terminal")) {
                continue;
            }
            let command = entry.get("Exec").map(|e| parse_exec(e)).unwrap_or_default();
            let Some(program) = command.first() else { continue };
            // A stale entry points at a program that is gone.
            if Path::new(program).is_absolute() && !Path::new(program).is_file() {
                continue;
            }
            let icon = entry.get("Icon").and_then(|i| theme_icon(env, i));
            match known {
                Some(k) => add(known_slicer(k, command, icon), &file),
                None => add(Slicer { id: format!("desktop-{}", normalize(name)), name: name.clone(), command, icon, prusa_major: None }, &file),
            }
        }
    }
}

/// An icon by theme name (or absolute PNG path) from the icon themes.
fn theme_icon(env: &Env, name: &str) -> Option<IconSource> {
    const SIZES: [u32; 7] = [256, 128, 192, 64, 48, 512, 32];
    let direct = Path::new(name);
    if direct.is_absolute() {
        return (direct.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) && direct.is_file())
            .then(|| IconSource::Image(direct.to_path_buf()));
    }
    if name.is_empty() || name.contains('/') || name.contains("..") {
        return None;
    }
    for data in &env.data_dirs {
        for size in SIZES {
            let p = data.join(format!("icons/hicolor/{size}x{size}/apps/{name}.png"));
            if p.is_file() {
                return Some(IconSource::Image(p));
            }
        }
        let p = data.join(format!("pixmaps/{name}.png"));
        if p.is_file() {
            return Some(IconSource::Image(p));
        }
    }
    None
}

fn path_programs(env: &Env, add: &mut impl FnMut(Slicer, &Path)) {
    for k in TARGETS {
        for dir in &env.path_dirs {
            for exe in k.exes {
                let p = dir.join(exe);
                if is_executable(&p) {
                    let icon = k.exes.iter().chain([&k.id]).find_map(|n| theme_icon(env, n.trim_end_matches(".exe")));
                    add(known_slicer(k, vec![s(&p)], icon), &p);
                }
            }
        }
    }
}

/// AppImages and per-slicer folders in the usual places.
fn program_folders(env: &Env, add: &mut impl FnMut(Slicer, &Path)) {
    for dir in &env.program_dirs {
        for p in entries(dir) {
            let name = file_name(&p);
            let Some(known) = known_for(&name) else { continue };
            if p.is_dir() {
                let inside = known.exes.iter().flat_map(|e| [p.join(e), p.join("bin").join(e)]).find(|c| is_executable(c));
                if let Some(exe) = inside {
                    let icon = theme_icon(env, known.id);
                    add(known_slicer(known, vec![s(&exe)], icon), &exe);
                }
            } else if name.to_ascii_lowercase().ends_with(".appimage") && is_executable(&p) {
                add(known_slicer(known, vec![s(&p)], None), &p);
            }
        }
    }
}

// --- macOS ---

fn mac_apps(env: &Env, add: &mut impl FnMut(Slicer, &Path)) {
    for dir in &env.app_dirs {
        for app in entries(dir) {
            let name = file_name(&app);
            let Some(stem) = name.strip_suffix(".app") else { continue };
            let Some(known) = known_for(stem) else { continue };
            add(known_slicer(known, vec!["open".into(), "-a".into(), s(&app)], mac_icon(&app)), &app);
        }
    }
}

/// The icon of a `.app` bundle: the `.icns` named like the app, else the first one.
fn mac_icon(app: &Path) -> Option<IconSource> {
    let icons: Vec<PathBuf> = entries(&app.join("Contents/Resources")).into_iter().filter(|p| p.extension().is_some_and(|e| e == "icns")).collect();
    let stem = normalize(&file_name(app).replace(".app", ""));
    let pick = icons.iter().find(|p| p.file_stem().is_some_and(|n| normalize(&n.to_string_lossy()) == stem)).or_else(|| icons.iter().min());
    pick.cloned().map(IconSource::Icns)
}

// --- Windows ---

fn windows_programs(env: &Env, add: &mut impl FnMut(Slicer, &Path)) {
    for root in &env.install_dirs {
        let mut dirs: Vec<PathBuf> = entries(root).into_iter().filter(|p| p.is_dir()).collect();
        // PrusaSlicer installs one level down, in `Prusa3D\PrusaSlicer`.
        let nested: Vec<PathBuf> =
            dirs.iter().filter(|d| normalize(&file_name(d)) == "prusa3d").flat_map(|d| entries(d)).filter(|p| p.is_dir()).collect();
        dirs.extend(nested);
        for dir in dirs {
            let Some(known) = known_for(&file_name(&dir)) else { continue };
            let exe = known.exes.iter().filter(|e| e.ends_with(".exe")).flat_map(|e| [dir.join(e), dir.join("bin").join(e)]).find(|c| c.is_file());
            if let Some(exe) = exe {
                add(known_slicer(known, vec![s(&exe)], Some(IconSource::Exe(exe.clone()))), &exe);
            }
        }
    }
}

// --- custom programs and launching ---

/// `name` with its first letter in capitals.
fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// A program the user picked: an application bundle, a program file or an AppImage.
pub fn custom(path: &str) -> Option<Slicer> {
    let p = Path::new(path);
    let name = file_name(p);
    let lower = name.to_ascii_lowercase();
    let mac_app = lower.ends_with(".app") && p.is_dir();
    if !(mac_app || (p.is_file() && is_executable(p))) {
        return None;
    }
    let stem = [".app", ".exe", ".appimage"]
        .iter()
        .find(|ext| lower.ends_with(**ext))
        .and_then(|ext| name.get(..name.len() - ext.len()))
        .map_or_else(|| name.clone(), str::to_string);
    let name = known_for(&stem).map_or_else(|| capitalised(&stem), |k| k.name.to_string());
    let (command, icon) = if mac_app {
        (vec!["open".into(), "-a".into(), path.to_string()], mac_icon(p))
    } else if lower.ends_with(".exe") {
        (vec![path.to_string()], Some(IconSource::Exe(p.to_path_buf())))
    } else {
        let stem = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        (vec![path.to_string()], theme_icon(&Env::current(), &stem))
    };
    Some(Slicer { id: "custom".into(), name, command, icon, prusa_major: None })
}

/// The programs the user picked under Custom…, set by the application. Commands may start these
/// and the detected slicers, and nothing else.
static CHOSEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Tell the commands which programs the user has chosen (the application keeps them in its settings).
pub fn set_chosen_programs(paths: &[String]) {
    *CHOSEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = paths.to_vec();
}

/// A slicer from a command parameter: the id or name of a detected one, the path of a detected
/// one's program, or a program the user chose (by its path or name). Any other program path is
/// refused: scripts and agents reach this through `file.save_mesh` and `file.send_to_slicer`, and
/// must not run arbitrary programs.
pub fn resolve(spec: &str) -> Option<Slicer> {
    resolve_for(TargetKind::Slicer, spec)
}

/// [`resolve`] among the targets of one kind.
pub fn resolve_for(kind: TargetKind, spec: &str) -> Option<Slicer> {
    let chosen = CHOSEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    resolve_in(detect_for(kind), &chosen, spec)
}

/// The installed targets of one kind, in the order they are listed. Empty in the browser.
pub fn detect_for(kind: TargetKind) -> Vec<Slicer> {
    detect().into_iter().filter(|s| s.target().kind == kind).collect()
}

/// Whether `spec` names the program that `command` starts: the same text, or the same file.
fn same_program(command: &str, spec: &str) -> bool {
    command == spec
        || (Path::new(spec).is_absolute()
            && std::fs::canonicalize(command).ok().is_some_and(|c| std::fs::canonicalize(spec).ok().is_some_and(|s| c == s)))
}

fn resolve_in(detected: Vec<Slicer>, chosen: &[String], spec: &str) -> Option<Slicer> {
    let wanted = normalize(spec);
    let named = |s: &Slicer| normalize(&s.name) == wanted;
    // An exact id or name first; else the slicer without saying which install: the first one.
    let plain = |d: &Slicer| normalize(d.base_id()) == wanted || known_by_id(d.base_id()).is_some_and(|k| normalize(k.name) == wanted);
    if !wanted.is_empty()
        && let Some(i) = detected.iter().position(|d| normalize(&d.id) == wanted || named(d)).or_else(|| detected.iter().position(plain))
    {
        return detected.into_iter().nth(i);
    }
    // The path of a detected slicer's program, as `file.slicers` lists it (an AppImage, a program
    // on `PATH`): that slicer, with its own command and arguments. Only when it is the program of
    // exactly one: a launcher several entries share (`/usr/bin/flatpak`) says nothing about which of
    // them is meant, and is not taken for the first. It goes on to the user's own programs instead.
    let by_path: Vec<usize> = (0..detected.len()).filter(|&i| detected[i].command.first().is_some_and(|c| same_program(c, spec))).collect();
    if let [only] = by_path.as_slice() {
        return detected.into_iter().nth(*only);
    }
    let picked = chosen.iter().filter_map(|p| custom(p).map(|s| (p, s)));
    picked.into_iter().find(|(p, s)| p.as_str() == spec || (!wanted.is_empty() && named(s))).map(|(_, s)| s)
}

/// `args` with the file added: at the end, or inside the `@@ … @@` pair of a Flatpak command, which
/// is how Flatpak lets the application see a file (`@@u` would hand it over as a URI).
#[cfg(not(target_arch = "wasm32"))]
fn with_file(args: &[String], path: &str) -> Vec<String> {
    let mut out: Vec<String> = args.iter().map(|a| if a == "@@u" { "@@".to_string() } else { a.clone() }).collect();
    let marks: Vec<usize> = out.iter().enumerate().filter(|(_, a)| *a == "@@").map(|(i, _)| i).collect();
    match marks.as_slice() {
        [.., _, last] => out.insert(*last, path.to_string()),
        _ => out.push(path.to_string()),
    }
    out
}

impl Slicer {
    /// The id without the `#<where>` that tells installs of one slicer apart: `prusa` for
    /// `prusa#comprusa3dprusaslicer`.
    pub fn base_id(&self) -> &str {
        self.id.split('#').next().unwrap_or(&self.id)
    }

    /// The arguments after the program: the command's own, the single-instance flag when asked for
    /// and this slicer has one, and the file. A desktop entry that already carries a
    /// `--single-instance…` flag is never given a second one, and loses its own when
    /// `single_instance` is off. PrusaSlicer 3.0's launcher has `--single-instance-on-url`, which
    /// hands the file to a running instance, and keeps it. PrusaSlicer 2.9's Flatpak entry has the
    /// same flag, but there it does not hand a file over (each send opened a new window); only
    /// `--single-instance` does, so on a slicer that is not on the wait table it is swapped.
    ///
    /// A slicer on the wait table (the 3.0 launcher) gets the flag whatever `single_instance` says:
    /// it can't hand a file to a running instance without it (a send with no flag left a launcher
    /// process that never forwarded, and a start with a file and no flag loaded nothing).
    #[cfg(not(target_arch = "wasm32"))]
    fn args(&self, path: &str, single_instance: bool) -> Vec<String> {
        let single_instance = single_instance || self.waits_for_ready().is_some();
        with_file(&self.base_args(single_instance), path)
    }

    /// [`Self::args`] without a file, for starting the slicer on its own: a Flatpak command's file
    /// markers go too. A slicer that has to be started first (`Target::ready`) is always started
    /// with the single-instance flag, whatever the preference says: a start that finds one already
    /// running (a slow first look at the bus can start it twice) hands over to it and exits, where
    /// without the flag it would open a second window.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_args(&self, single_instance: bool) -> Vec<String> {
        let single_instance = single_instance || self.waits_for_ready().is_some();
        self.base_args(single_instance).into_iter().filter(|a| a != "@@" && a != "@@u").collect()
    }

    /// The row of the target table this is, or the slicer defaults for a program that has none.
    pub fn target(&self) -> &'static Target {
        TARGETS.iter().find(|t| t.id == self.kind()).unwrap_or(&Target::SLICER)
    }

    /// Which known slicer this is, by identity: its id without the install part, or for a program
    /// the user picked, the one its name says (as for its icon). Empty for any other program.
    fn kind(&self) -> &str {
        if self.id == "custom" { known_for(&self.name).map_or("", |k| k.id) } else { self.base_id() }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn base_args(&self, single_instance: bool) -> Vec<String> {
        let mut args: Vec<String> = self.command.iter().skip(1).cloned().collect();
        let has = |args: &[String]| args.iter().any(|a| a.starts_with("--single-instance"));
        if !single_instance {
            args.retain(|a| !a.starts_with("--single-instance"));
        } else if self.url_flag_swaps() && args.iter().any(|a| a == "--single-instance-on-url") {
            // PrusaSlicer 2 only (by its version, when that could be read): its `-on-url` does not hand
            // a file over, and `--single-instance` does. Another program that happens to have the flag
            // keeps its own meaning for it, and so does an entry whose version is unknown.
            for a in args.iter_mut().filter(|a| *a == "--single-instance-on-url") {
                *a = "--single-instance".into();
            }
        } else if let Some(flag) = self.target().single_instance.filter(|_| self.command.first().is_some_and(|p| p != "open") && !has(&args)) {
            // Before a Flatpak command's file markers, which close the application's own arguments.
            let at = args.iter().position(|a| a == "@@" || a == "@@u").unwrap_or(args.len());
            args.insert(at, flag.into());
        }
        args
    }

    /// Start the slicer with `path` loaded; does not wait for it to exit. With `single_instance`,
    /// a slicer that supports it loads the file in the window that is already open.
    pub fn open(&self, path: &str, single_instance: bool) -> std::result::Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, single_instance);
            Err(format!("{} can't be started from the browser", self.name))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.spawn(self.args(path, single_instance)).map_err(|e| format!("couldn't start {} ({e}); the file is saved at {path}", self.name))
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn spawn(&self, args: Vec<String>) -> std::io::Result<()> {
        use std::process::{Command, Stdio};
        let program = self.command.first().ok_or_else(|| std::io::Error::other("no program to start"))?;
        Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(drop)
    }

    /// Start the slicer on its own, with no file. Does not wait for it to exit or to be ready.
    pub fn start(&self, single_instance: bool) -> std::result::Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = single_instance;
            Err(format!("{} can't be started from the browser", self.name))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.spawn(self.start_args(single_instance)).map_err(|e| format!("couldn't start {} ({e})", self.name))
        }
    }

    /// Whether this slicer drops a file given at a cold start and so has to be started first and
    /// sent the file once it can take one (see `Target::ready`). That is PrusaSlicer 3 or newer
    /// (`prusa_major`), which starts through its launcher; PrusaSlicer 2 loads the file at launch,
    /// whatever its command is. Where the version could not be read, and for a program that is not
    /// in the table with such a rule, the launcher's name in the command decides.
    pub fn waits_for_ready(&self) -> Option<&'static WaitRule> {
        let by_name = || TARGETS.iter().filter_map(|t| t.ready.as_ref()).find(|r| self.command.iter().any(|a| a.contains(r.command_has)));
        match (self.target().ready.as_ref(), self.prusa_major) {
            (Some(rule), Some(m)) if m < rule.min_major => None,
            (Some(rule), Some(_)) => Some(rule),
            _ => by_name(),
        }
    }

    /// Whether this is a PrusaSlicer old enough that its `--single-instance-on-url` does not hand a
    /// file over, by the version read when it was detected.
    #[cfg(not(target_arch = "wasm32"))]
    fn url_flag_swaps(&self) -> bool {
        self.target().on_url_swap_below.is_some_and(|below| self.prusa_major.is_some_and(|v| v < below))
    }

    /// Whether the slicer can take a file now: `Some(true)` or `Some(false)`, or `None` when that
    /// can't be told (no `busctl`, or it failed), in which case nobody waits. A `busctl` that is
    /// only slow is `Some(false)`: nothing says it is up yet.
    pub fn is_ready(&self) -> Option<bool> {
        self.is_ready_within(PROBE_LIMIT)
    }

    /// [`Self::is_ready`], giving `busctl` at most `limit` to answer.
    pub fn is_ready_within(&self, limit: std::time::Duration) -> Option<bool> {
        self.ready_via(&busctl_program(), limit)
    }

    pub(crate) fn ready_via(&self, busctl: &str, limit: std::time::Duration) -> Option<bool> {
        match (self.waits_for_ready()?, bus_list(busctl, limit)) {
            #[cfg(all(unix, not(target_arch = "wasm32")))]
            (rule, Bus::Names(list)) => Some(has_bus_name(&list, rule.bus_prefix)),
            #[cfg(all(unix, not(target_arch = "wasm32")))]
            (_, Bus::Slow) => Some(false),
            (_, Bus::Unknown) => None,
        }
    }
}

/// A rule for a program that ignores a file given at a cold start: how it is found by its command,
/// and the sign that it is up and can take one.
pub struct WaitRule {
    /// Part of an argument of the command.
    pub command_has: &'static str,
    /// A name on the session bus that exists while the slicer is ready. Its end changes with the
    /// install, so only its start is matched.
    pub bus_prefix: &'static str,
    /// The first major version that needs this (older ones load the file at launch).
    pub min_major: u32,
}

/// PrusaSlicer 3.0's launcher ignores the file on a cold start whatever the flag, and loads it
/// over D-Bus when an instance is running. PrusaSlicer 2.9 and OrcaSlicer load it at launch, so
/// they have no such rule.
pub const PRUSA_READY: WaitRule =
    WaitRule { command_has: "slic3r-app-launcher", bus_prefix: "com.prusa3d.prusaslicer.BroadcastListener.", min_major: 3 };

/// Longest wait for a started slicer to be ready, and how often it is looked at.
pub const READY_LIMIT: std::time::Duration = std::time::Duration::from_secs(10);
pub const READY_POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// The longest one look at the session bus may take.
pub const PROBE_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);

/// What one look at the session bus came to.
#[derive(Debug, PartialEq, Eq)]
enum Bus {
    /// The names, one line each.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    Names(String),
    /// `busctl` did not answer in time: nothing is known yet, which is not the same as "not up".
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    Slow,
    /// `busctl` is missing, failed, or this is not a system that has it: it can't be told.
    Unknown,
}

/// The session bus's names, as `busctl --user --no-legend --no-pager list` (`program` is
/// `busctl`) prints them, or why they could not be had.
fn bus_list(program: &str, limit: std::time::Duration) -> Bus {
    #[cfg(any(target_arch = "wasm32", not(unix)))]
    {
        let _ = (program, limit);
        Bus::Unknown
    }
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        use std::io::Read;
        use std::process::Stdio;
        let Ok(mut child) = std::process::Command::new(program)
            .args(["--user", "--no-legend", "--no-pager", "list"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return Bus::Unknown;
        };
        let end = std::time::Instant::now() + limit;
        let left = || end.saturating_duration_since(std::time::Instant::now());
        // Read on another thread so a long list can't fill the pipe while this one waits, and hand
        // the text back over a channel so that waiting for it has the same deadline: a process
        // the program left running may keep the pipe open long after the program is gone.
        // The reader is left to end when the pipe closes, not waited for. A busctl that answered but
        // left a process holding its stdout reads as Slow, and each poll of such a bus can leave one
        // blocked reader behind until that process lets go.
        let Some(mut stdout) = child.stdout.take() else { return Bus::Unknown };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = tx.send(stdout.read_to_string(&mut text).ok().map(|_| text));
        });
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if !left().is_zero() => std::thread::sleep(std::time::Duration::from_millis(10).min(left())),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Bus::Slow;
                }
                Err(_) => return Bus::Unknown,
            }
        };
        // A reader that is still waiting when this gives up ends when the pipe closes.
        match rx.recv_timeout(left()) {
            Ok(Some(text)) if status.success() => Bus::Names(text),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Bus::Slow,
            _ => Bus::Unknown,
        }
    }
}

/// The first column of each line: the bus names.
#[cfg(any(test, all(unix, not(target_arch = "wasm32"))))]
fn bus_names(list: &str) -> impl Iterator<Item = &str> {
    list.lines().filter_map(|l| l.split_whitespace().next())
}

/// Whether the list has a name that starts with `prefix`.
#[cfg(any(test, all(unix, not(target_arch = "wasm32"))))]
fn has_bus_name(list: &str, prefix: &str) -> bool {
    bus_names(list).any(|n| n.starts_with(prefix))
}

/// Look every `poll` until `ready` says so; false when `limit` runs out first. `ready` is asked at
/// once, so a slicer that is already up costs no sleep, and is given the time that is left, so a
/// probe that can block (`busctl`) can be cut short.
///
/// No probe is started after the limit has passed. A probe that is already running when it passes
/// may finish, and a yes from it is accepted on purpose: the file should still go. So the wait takes
/// `limit` plus at most that last probe's own time. The production probe is given the time left,
/// which is a polling and read budget, not a wall-clock promise: its clock starts after the process
/// is spawned, and killing and reaping it, and scheduling, take time of their own.
pub fn wait_until(mut ready: impl FnMut(std::time::Duration) -> bool, limit: std::time::Duration, poll: std::time::Duration) -> bool {
    let end = std::time::Instant::now() + limit;
    let mut first = true;
    loop {
        let left = end.saturating_duration_since(std::time::Instant::now());
        if !first && left.is_zero() {
            return false;
        }
        first = false;
        if ready(left) {
            return true;
        }
        std::thread::sleep(poll.min(end.saturating_duration_since(std::time::Instant::now())));
    }
}

/// Wait for `slicer` to be ready to take a file. True when it is, or when that can't be told; false
/// when the limit passes first, a slow `busctl` included.
pub fn wait_for_ready(slicer: &Slicer) -> bool {
    wait_for_ready_via(slicer, &busctl_program(), READY_LIMIT, READY_POLL)
}

pub(crate) fn wait_for_ready_via(slicer: &Slicer, busctl: &str, limit: std::time::Duration, poll: std::time::Duration) -> bool {
    // On purpose: if busctl is missing, or fails partway through, stop waiting and send; a file sent
    // too early is dropped quietly and costs one re-send. A busctl that is only slow is not that:
    // it keeps the wait going to the limit.
    wait_until(|left| slicer.ready_via(busctl, left.min(PROBE_LIMIT)) != Some(false), limit, poll)
}

/// What tests of this module and of the send pipeline share.
#[cfg(test)]
#[cfg(unix)]
pub(crate) mod test_support {
    use std::path::Path;

    /// A small shell script that can be run. One that was only just written can fail to start (text
    /// file busy) while another test forks, which would read as a `busctl` that is missing; so it is
    /// started once here, patiently, with a flag the script answers at once, before a test relies
    /// on it.
    pub(crate) fn fake_program(root: &Path, name: &str, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let p = root.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n[ \"$1\" = --warm-up ] && exit 0\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        for _ in 0..200 {
            use std::process::Stdio;
            match std::process::Command::new(&p).arg("--warm-up").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
                Err(e) if e.raw_os_error() == Some(26) => std::thread::sleep(std::time::Duration::from_millis(20)),
                _ => break,
            }
        }
        p.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::test_support::fake_program;
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-slicers-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names_match_known_slicers_but_not_their_companions() {
        assert_eq!(known_for("PrusaSlicer").map(|k| k.id), Some("prusa"));
        assert_eq!(known_for("Bambu Studio").map(|k| k.id), Some("bambu"));
        assert_eq!(known_for("UltiMaker Cura 5.9").map(|k| k.id), Some("cura"));
        assert!(known_for("PrusaSlicer G-code Viewer").is_none());
        assert!(known_for("Firefox").is_none());
    }

    #[test]
    fn exec_lines_become_commands() {
        assert_eq!(parse_exec("prusa-slicer %F"), ["prusa-slicer"]);
        assert_eq!(parse_exec(r#""/opt/My Apps/bambu" --x %U"#), ["/opt/My Apps/bambu", "--x"]);
        assert_eq!(
            parse_exec("flatpak run --branch=stable com.prusa3d.PrusaSlicer @@u %U @@"),
            ["flatpak", "run", "--branch=stable", "com.prusa3d.PrusaSlicer", "@@u", "@@"]
        );
        assert!(parse_exec("").is_empty());
    }

    #[test]
    fn the_file_goes_where_the_command_expects_it() {
        let v = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(with_file(&v("run --file-forwarding app --x @@u @@"), "/t/a.3mf"), v("run --file-forwarding app --x @@ /t/a.3mf @@"));
        assert_eq!(with_file(&v("-a /Applications/X.app"), "/t/a.stl"), v("-a /Applications/X.app /t/a.stl"));
        assert_eq!(with_file(&[], "/t/a.stl"), v("/t/a.stl"));
    }

    #[test]
    fn finds_linux_desktop_entries_and_their_icons() {
        let root = scratch("linux");
        let data = root.join("share");
        std::fs::create_dir_all(data.join("applications")).unwrap();
        std::fs::create_dir_all(data.join("icons/hicolor/128x128/apps")).unwrap();
        std::fs::write(data.join("icons/hicolor/128x128/apps/PrusaSlicer.png"), b"png").unwrap();
        std::fs::write(
            data.join("applications/a.desktop"),
            "[Desktop Entry]\nType=Application\nName=PrusaSlicer\nName[de]=Prusa\nExec=flatpak run com.prusa3d.PrusaSlicer %U\nIcon=PrusaSlicer\n",
        )
        .unwrap();
        std::fs::write(data.join("applications/b.desktop"), "[Desktop Entry]\nName=Firefox\nExec=firefox\n").unwrap();
        std::fs::write(data.join("applications/c.desktop"), "[Desktop Entry]\nName=PrusaSlicer G-code Viewer\nExec=prusa-slicer --gcodeviewer\n")
            .unwrap();
        let found = detect_in(&Env { data_dirs: vec![data.clone()], ..Env::default() });
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "prusa");
        assert_eq!(found[0].command, ["flatpak", "run", "com.prusa3d.PrusaSlicer"]);
        assert_eq!(found[0].icon, Some(IconSource::Image(data.join("icons/hicolor/128x128/apps/PrusaSlicer.png"))));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn programs_that_claim_3mf_or_stl_are_candidates_too() {
        let root = scratch("mime");
        let data = root.join("share");
        std::fs::create_dir_all(data.join("applications")).unwrap();
        let entry = |file: &str, body: &str| {
            std::fs::write(data.join("applications").join(file), format!("[Desktop Entry]\nType=Application\n{body}\n")).unwrap()
        };
        entry("fancy.desktop", "Name=Fancy Slicer\nExec=fancy %f\nMimeType=text/plain;MODEL/3MF;");
        entry("viewer.desktop", "Name=Mesh Viewer\nExec=meshview --open %F\nMimeType=image/png;application/sla;");
        // Known by name: the table still gives the name and id, and the file type is not needed.
        entry("prusa.desktop", "Name=PrusaSlicer\nExec=prusa-slicer %F");
        entry("hidden.desktop", "Name=Hidden Slicer\nExec=h\nMimeType=model/stl;\nNoDisplay=true");
        entry("gone.desktop", "Name=Gone Slicer\nExec=g\nMimeType=model/stl;\nHidden=true");
        entry("terminal.desktop", "Name=Console Mesher\nExec=cm\nMimeType=model/stl;\nTerminal=true");
        entry("noclaim.desktop", "Name=Text Editor\nExec=edit %f\nMimeType=text/plain;application/vnd.ms-3mfdocumentation;");
        entry("hiddenprusa.desktop", "Name=PrusaSlicer Beta\nExec=prusa-beta\nNoDisplay=true");
        entry("ours.desktop", "Name=SolidCraft\nExec=solidcraft %f\nMimeType=model/3mf;model/stl;");
        entry("alias.desktop", "Name=Windows Mesher\nExec=wm\nMimeType=application/vnd.ms-3mfdocument;");
        let found = detect_in(&Env { data_dirs: vec![data], ..Env::default() });
        let listed: Vec<(&str, &str)> = found.iter().map(|f| (f.id.as_str(), f.name.as_str())).collect();
        assert_eq!(
            listed,
            [
                ("prusa", "PrusaSlicer"),
                ("desktop-fancyslicer", "Fancy Slicer"),
                ("desktop-meshviewer", "Mesh Viewer"),
                ("desktop-windowsmesher", "Windows Mesher")
            ]
        );
        assert_eq!(found[1].command, ["fancy"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_program_path_that_several_entries_share_is_refused_and_a_single_one_runs_its_whole_command() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("shared-path");
        let flatpak = root.join("bin/flatpak");
        std::fs::create_dir_all(flatpak.parent().unwrap()).unwrap();
        std::fs::write(&flatpak, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&flatpak, std::fs::Permissions::from_mode(0o755)).unwrap();
        let share = root.join("share");
        std::fs::create_dir_all(share.join("applications")).unwrap();
        for (file, name, branch) in [("a.desktop", "PrusaSlicer", "stable"), ("b.desktop", "OrcaSlicer", "master")] {
            let exec = format!("{} run --branch={branch} com.example.{name} @@u %U @@", flatpak.display());
            std::fs::write(share.join("applications").join(file), format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\n")).unwrap();
        }
        // And an AppImage that is the program of one entry only.
        let apps = root.join("Apps");
        let image = apps.join("SuperSlicer.AppImage");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(&image, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = detect_in(&Env { data_dirs: vec![share], program_dirs: vec![apps], ..Env::default() });
        assert_eq!(found.len(), 3, "{found:?}");
        // Two detected Flatpak entries start the same launcher: the path alone does not say which.
        assert_eq!(resolve_in(found.clone(), &[], flatpak.to_str().unwrap()), None);
        // Each is still reachable by its own id or name, with its full command.
        let by_id = resolve_in(found.clone(), &[], "orca").unwrap();
        assert_eq!(by_id.command.first().map(String::as_str), flatpak.to_str());
        assert!(by_id.command.contains(&"--branch=master".to_string()));
        // The one entry whose program the path is runs its whole command.
        let by_path = resolve_in(found.clone(), &[], image.to_str().unwrap()).unwrap();
        assert_eq!(by_path.command, found.iter().find(|s| s.base_id() == "superslicer").unwrap().command);
        // A path the user chose under Custom… is the user's own decision, whatever else shares it.
        let chosen = [flatpak.to_str().unwrap().to_string()];
        assert_eq!(resolve_in(found, &chosen, flatpak.to_str().unwrap()).map(|s| s.id), Some("custom".into()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finds_mac_bundles_and_windows_folders() {
        let root = scratch("mac-win");
        let apps = root.join("Applications");
        std::fs::create_dir_all(apps.join("BambuStudio.app/Contents/Resources")).unwrap();
        std::fs::write(apps.join("BambuStudio.app/Contents/Resources/BambuStudio.icns"), b"x").unwrap();
        std::fs::create_dir_all(apps.join("Safari.app")).unwrap();
        let prog = root.join("Program Files");
        std::fs::create_dir_all(prog.join("Prusa3D/PrusaSlicer")).unwrap();
        std::fs::write(prog.join("Prusa3D/PrusaSlicer/prusa-slicer.exe"), b"x").unwrap();
        std::fs::create_dir_all(prog.join("OrcaSlicer")).unwrap();
        std::fs::write(prog.join("OrcaSlicer/orca-slicer.exe"), b"x").unwrap();
        let found = detect_in(&Env { app_dirs: vec![apps.clone()], install_dirs: vec![prog.clone()], ..Env::default() });
        let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["prusa", "bambu", "orca"]);
        assert_eq!(found[0].icon, Some(IconSource::Exe(prog.join("Prusa3D/PrusaSlicer/prusa-slicer.exe"))));
        assert_eq!(found[1].command, ["open", "-a", apps.join("BambuStudio.app").to_str().unwrap()]);
        assert!(matches!(found[1].icon, Some(IconSource::Icns(_))));
        assert_eq!(found[2].icon, Some(IconSource::Exe(prog.join("OrcaSlicer/orca-slicer.exe"))));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn finds_programs_on_the_path_and_appimages() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("path");
        let bin = root.join("bin");
        let apps = root.join("Applications");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&apps).unwrap();
        let exe = bin.join("orca-slicer");
        let image = apps.join("PrusaSlicer-2.8.AppImage");
        for f in [&exe, &image] {
            std::fs::write(f, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(bin.join("prusa-slicer"), b"not executable").unwrap();
        let found = detect_in(&Env { path_dirs: vec![bin.clone()], program_dirs: vec![apps.clone()], ..Env::default() });
        let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["prusa", "orca"]);
        assert_eq!(found[0].command, [image.to_str().unwrap()]);
        // A picked program becomes a custom entry named after it; a missing path is refused.
        let c = custom(exe.to_str().unwrap()).unwrap();
        assert_eq!((c.id.as_str(), c.name.as_str()), ("custom", "OrcaSlicer"));
        assert_eq!(capitalised("mycura"), "Mycura");
        assert!(custom(root.join("nope").to_str().unwrap()).is_none());
        assert!(custom(root.to_str().unwrap()).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn single_instance_is_asked_of_the_slicers_that_have_it() {
        let slicer = |id: &str, name: &str, command: &[&str]| Slicer {
            id: id.into(),
            name: name.into(),
            command: command.iter().map(|s| s.to_string()).collect(),
            icon: None,
            prusa_major: None,
        };
        let args = |s: &Slicer, single| s.args("/t/a.3mf", single);
        for (id, name) in [("prusa", "PrusaSlicer"), ("superslicer", "SuperSlicer"), ("cura", "UltiMaker Cura")] {
            let s = slicer(id, name, &["/x/prog"]);
            assert_eq!(args(&s, true), ["--single-instance", "/t/a.3mf"], "{id}");
            assert_eq!(args(&s, false), ["/t/a.3mf"], "{id}");
        }
        // Bambu Studio is not given the flag, also when the user picked it as a custom program.
        assert_eq!(args(&slicer("bambu", "Bambu Studio", &["/x/bambu-studio"]), true), ["/t/a.3mf"]);
        assert_eq!(args(&slicer("custom", "Bambu Studio", &["/x/bambu-studio"]), true), ["/t/a.3mf"]);
        // OrcaSlicer and the rest have no such flag; a macOS bundle is single-instance already.
        assert_eq!(args(&slicer("orca", "OrcaSlicer", &["/x/orca"]), true), ["/t/a.3mf"]);
        assert_eq!(
            args(&slicer("prusa", "PrusaSlicer", &["open", "-a", "/Applications/PrusaSlicer.app"]), true),
            ["-a", "/Applications/PrusaSlicer.app", "/t/a.3mf"]
        );
        // A program the user picked is known by its name.
        assert_eq!(args(&slicer("custom", "PrusaSlicer", &["/x/prusa-slicer"]), true), ["--single-instance", "/t/a.3mf"]);
        assert_eq!(args(&slicer("custom", "Mycura", &["/x/mycura"]), true), ["/t/a.3mf"]);
        // PrusaSlicer 3.0's launcher entry already has the flag (`%u` is dropped when the entry is read).
        let launcher = slicer("prusa", "PrusaSlicer", &["slic3r-app-launcher", "--single-instance-on-url"]);
        assert_eq!(args(&launcher, true), ["--single-instance-on-url", "/t/a.3mf"]);
        // Its launcher can't hand a file over without the flag, so the preference does not remove it.
        assert_eq!(args(&launcher, false), ["--single-instance-on-url", "/t/a.3mf"]);
        // Flatpak: the flag goes among the application's arguments, before the file markers.
        let flatpak = slicer("prusa", "PrusaSlicer", &["flatpak", "run", "com.prusa3d.PrusaSlicer", "@@u", "@@"]);
        assert_eq!(args(&flatpak, true), ["run", "com.prusa3d.PrusaSlicer", "--single-instance", "@@", "/t/a.3mf", "@@"]);
    }

    #[cfg(unix)]
    #[test]
    fn only_detected_slicers_and_chosen_programs_can_be_resolved() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("resolve");
        let program = root.join("mycura");
        std::fs::write(&program, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = program.to_str().unwrap();
        let prusa = Slicer { id: "prusa".into(), name: "PrusaSlicer".into(), command: vec!["prusa-slicer".into()], icon: None, prusa_major: None };
        let found = |spec: &str, chosen: &[String]| resolve_in(vec![prusa.clone()], chosen, spec).map(|s| s.id);
        assert_eq!(found("prusa", &[]).as_deref(), Some("prusa"));
        assert_eq!(found("Prusa Slicer", &[]).as_deref(), Some("prusa"));
        // A real program that nobody chose is not a slicer, whatever path it is given by.
        assert_eq!(found(path, &[]), None);
        assert_eq!(found("/bin/sh", &[]), None);
        assert_eq!(found("", &[]), None);
        // One the user chose works by path and by name; a path that was not chosen still does not.
        let chosen = [path.to_string()];
        assert_eq!(found(path, &chosen).as_deref(), Some("custom"));
        assert_eq!(found("Mycura", &chosen).as_deref(), Some("custom"));
        assert_eq!(found("/bin/sh", &chosen), None);
        // Chosen, then uninstalled: gone.
        std::fs::remove_file(&program).unwrap();
        assert_eq!(found(path, &chosen), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn each_install_of_a_slicer_is_listed_on_its_own() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("installs");
        let home = root.join("home");
        let (system, user) = (root.join("usr/share"), home.join(".local/share"));
        for dir in [&system, &user] {
            std::fs::create_dir_all(dir.join("applications")).unwrap();
        }
        // The two entries of the real desktop: PrusaSlicer 2.9 from the system, 3.0 beta as a Flatpak.
        std::fs::write(
            system.join("applications/PrusaSlicer.desktop"),
            "[Desktop Entry]\nType=Application\nName=PrusaSlicer\nExec=prusa-slicer %F\n",
        )
        .unwrap();
        std::fs::write(
            user.join("applications/com.prusa3d.PrusaSlicer.desktop"),
            "[Desktop Entry]\nType=Application\nName=PrusaSlicer\nExec=flatpak run --branch=beta --arch=x86_64 --command=slic3r-app-launcher --file-forwarding com.prusa3d.PrusaSlicer --single-instance-on-url @@u %U @@\n",
        )
        .unwrap();
        let bin = root.join("usr/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("prusa-slicer"), b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join("prusa-slicer"), std::fs::Permissions::from_mode(0o755)).unwrap();
        // The same program is also on PATH under its own name: still the one install.
        let env = Env { home: Some(home), data_dirs: vec![user, system], path_dirs: vec![bin], ..Env::default() };
        let found = detect_in(&env);
        let listed: Vec<(&str, &str)> = found.iter().map(|f| (f.id.as_str(), f.name.as_str())).collect();
        assert_eq!(listed, [("prusa#comprusa3dprusaslicer", "PrusaSlicer (Flatpak, beta)"), ("prusa#prusaslicer", "PrusaSlicer (system)")]);
        // Both are still PrusaSlicer to everything that looks at the kind of slicer.
        assert!(found.iter().all(|f| f.base_id() == "prusa"));
        let flatpak = found[0].args("/t/a.3mf", true);
        assert_eq!(flatpak.iter().filter(|a| a.starts_with("--single-instance")).collect::<Vec<_>>(), ["--single-instance-on-url"]);
        assert_eq!(found[1].args("/t/a.3mf", true), ["--single-instance", "/t/a.3mf"]);
        // The same on every run, whatever order the folders are read in.
        let again = detect_in(&Env { data_dirs: env.data_dirs.iter().rev().cloned().collect(), ..env.clone() });
        assert_eq!(found, again);
        // `prusa` alone, as remembered or typed before there were two, is the first; a full id is that install.
        let pick = |spec: &str| resolve_in(found.clone(), &[], spec).map(|s| s.id);
        assert_eq!(pick("prusa").as_deref(), Some("prusa#comprusa3dprusaslicer"));
        assert_eq!(pick("PrusaSlicer").as_deref(), Some("prusa#comprusa3dprusaslicer"));
        assert_eq!(pick("prusa#prusaslicer").as_deref(), Some("prusa#prusaslicer"));
        assert_eq!(pick("PrusaSlicer (system)").as_deref(), Some("prusa#prusaslicer"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_launcher_needs_a_start_and_a_wait_and_other_slicers_do_not() {
        let slicer = |command: &[&str]| Slicer {
            id: "prusa".into(),
            name: "PrusaSlicer".into(),
            command: command.iter().map(|s| s.to_string()).collect(),
            icon: None,
            prusa_major: None,
        };
        // The command lines of this machine's desktop entries.
        let flatpak = slicer(&[
            "flatpak",
            "run",
            "--branch=beta",
            "--arch=x86_64",
            "--command=slic3r-app-launcher",
            "--file-forwarding",
            "com.prusa3d.PrusaSlicer",
            "--single-instance-on-url",
            "@@u",
            "@@",
        ]);
        let launcher = slicer(&["slic3r-app-launcher", "--single-instance-on-url"]);
        assert!(flatpak.waits_for_ready().is_some() && launcher.waits_for_ready().is_some());
        for command in [
            &["prusa-slicer"][..],
            &["flatpak", "run", "com.prusa3d.PrusaSlicer", "@@u", "@@"],
            &["/usr/bin/orca-slicer"],
            &["open", "-a", "/Applications/PrusaSlicer.app"],
        ] {
            assert!(slicer(command).waits_for_ready().is_none(), "{command:?}");
        }
        // Starting on its own: no file, and none of the Flatpak file markers.
        assert_eq!(
            flatpak.start_args(true),
            [
                "run",
                "--branch=beta",
                "--arch=x86_64",
                "--command=slic3r-app-launcher",
                "--file-forwarding",
                "com.prusa3d.PrusaSlicer",
                "--single-instance-on-url"
            ]
        );
        // The preference does not take the flag off the launcher, on a start or a send (its entry's own
        // is kept, and one is added where there is none).
        assert_eq!(launcher.start_args(false), ["--single-instance-on-url"]);
        let bare = slicer(&["slic3r-app-launcher"]);
        assert_eq!(bare.start_args(false), ["--single-instance"]);
        assert_eq!(bare.start_args(true), ["--single-instance"]);
        assert_eq!(bare.args("/t/a.3mf", false), ["--single-instance", "/t/a.3mf"], "a normal send, preference off");
        assert_eq!(bare.args("/t/a.3mf", true), ["--single-instance", "/t/a.3mf"]);
        // A slicer that does not need a start first is started as the preference says.
        assert!(slicer(&["prusa-slicer"]).start_args(false).is_empty());
    }

    #[test]
    fn the_bus_list_says_whether_the_slicer_is_up() {
        let prefix = PRUSA_READY.bus_prefix;
        let without = ":1.5 1234 systemd :1.5 user@1000.service 1 -\norg.freedesktop.Notifications 99 dunst :1.40 u 1 -\ncom.prusa3d.PrusaSlicer 7 x :1.9 s 1 -\n";
        let with = format!("{without}com.prusa3d.prusaslicer.BroadcastListener.3f9a1c 4321 xdg-dbus-proxy :1.77 u 1 -\n");
        assert!(!has_bus_name(without, prefix));
        assert!(has_bus_name(&with, prefix));
        assert!(!has_bus_name("", prefix));
        // The suffix changes with the install; only the start counts, and only at the start of a name.
        assert!(!has_bus_name("x.com.prusa3d.prusaslicer.BroadcastListener.1 1 p :1.1 u 1 -", prefix));
        assert_eq!(bus_names(&with).count(), 4);
    }

    #[test]
    fn waiting_stops_at_once_when_ready_and_gives_up_at_the_limit() {
        let tick = std::time::Duration::from_millis(5);
        let mut asked = 0;
        assert!(wait_until(
            |_| {
                asked += 1;
                true
            },
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(5)
        ));
        assert_eq!(asked, 1, "already up: no sleep");
        let mut asked = 0;
        assert!(wait_until(
            |_| {
                asked += 1;
                asked == 3
            },
            std::time::Duration::from_secs(5),
            tick
        ));
        assert_eq!(asked, 3);
        let started = std::time::Instant::now();
        assert!(!wait_until(|_| false, std::time::Duration::from_millis(40), tick));
        assert!(started.elapsed() >= std::time::Duration::from_millis(40) && started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn a_probe_that_says_yes_after_the_limit_is_accepted_and_the_wait_stays_bounded() {
        let started = std::time::Instant::now();
        let mut asked = 0;
        let ok = wait_until(
            |_| {
                asked += 1;
                std::thread::sleep(std::time::Duration::from_millis(80));
                true
            },
            std::time::Duration::from_millis(30),
            std::time::Duration::from_millis(5),
        );
        // The look began inside the limit, ran past it, and its yes is taken: the file still goes.
        assert!(ok);
        assert_eq!(asked, 1);
        // Limit plus the probe's own time, nothing more.
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        // A probe that began before the limit and says no, then is never followed by another.
        let mut asked = 0;
        let started = std::time::Instant::now();
        assert!(!wait_until(
            |_| {
                asked += 1;
                std::thread::sleep(std::time::Duration::from_millis(80));
                false
            },
            std::time::Duration::from_millis(30),
            std::time::Duration::from_millis(5)
        ));
        assert_eq!(asked, 1, "no probe is started after the limit");
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[test]
    fn a_slow_probe_can_not_start_another_after_the_limit() {
        // The first look takes longer than the whole limit and says no; the second would say yes,
        // but the limit has passed, so it is never asked.
        let mut asked = 0;
        let ok = wait_until(
            |_| {
                asked += 1;
                if asked == 1 {
                    std::thread::sleep(std::time::Duration::from_millis(60));
                }
                asked > 1
            },
            std::time::Duration::from_millis(30),
            std::time::Duration::from_millis(5),
        );
        assert!(!ok);
        assert_eq!(asked, 1);
        // The probe is told how much time is left, and it shrinks.
        let mut lefts = Vec::new();
        wait_until(
            |left| {
                lefts.push(left);
                false
            },
            std::time::Duration::from_millis(40),
            std::time::Duration::from_millis(10),
        );
        assert!(lefts[0] <= std::time::Duration::from_millis(40) && lefts.windows(2).all(|w| w[1] <= w[0]), "{lefts:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_busctl_that_hangs_is_cut_off_and_one_that_answers_is_read() {
        let root = scratch("busctl");
        let fake = |name: &str, body: &str| fake_program(&root, name, body);
        let hang = fake("hang", "exec sleep 30");
        let started = std::time::Instant::now();
        assert_eq!(bus_list(&hang, std::time::Duration::from_millis(100)), Bus::Slow);
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "cut off, not waited for");
        let answers = fake("answers", "echo ':1.5 1 x'; echo 'com.prusa3d.prusaslicer.BroadcastListener.ab 2 y'");
        let Bus::Names(list) = bus_list(&answers, std::time::Duration::from_secs(5)) else { panic!("no names") };
        assert!(has_bus_name(&list, PRUSA_READY.bus_prefix));
        assert_eq!(bus_list(&fake("fails", "exit 1"), std::time::Duration::from_secs(5)), Bus::Unknown);
        // It exits at once but leaves a process running that keeps its output open.
        let lingers = fake("lingers", "echo ':1.5 1 x'; sleep 5 &\nexit 0");
        let started = std::time::Instant::now();
        assert_eq!(bus_list(&lingers, std::time::Duration::from_millis(200)), Bus::Slow);
        assert!(started.elapsed() < std::time::Duration::from_secs(3), "{:?}", started.elapsed());
        assert_eq!(bus_list(&root.join("missing").to_string_lossy(), std::time::Duration::from_secs(1)), Bus::Unknown);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mac_installs_are_labelled_by_their_application_not_by_open() {
        let root = scratch("mac-labels");
        let home = root.join("home");
        let (system, user) = (root.join("Applications"), home.join("Applications"));
        for dir in [&system, &user] {
            std::fs::create_dir_all(dir.join("PrusaSlicer.app")).unwrap();
        }
        let env = Env { home: Some(home), app_dirs: vec![system, user], ..Env::default() };
        let found = detect_in(&env);
        let listed: Vec<(&str, &str)> = found.iter().map(|f| (f.id.as_str(), f.name.as_str())).collect();
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|(_, n)| *n == "PrusaSlicer (system)"), "{listed:?}");
        assert!(listed.iter().any(|(_, n)| *n == "PrusaSlicer (~/Applications)"), "{listed:?}");
        assert_ne!(listed[0].0, listed[1].0, "own ids");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn two_names_for_one_program_give_the_same_ids_whichever_is_read_first() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("aliases");
        let home = root.join("home");
        let bin = root.join("usr/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("prusa-slicer"), b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join("prusa-slicer"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let entry = |dir: &Path, file: &str, exec: &str| {
            std::fs::create_dir_all(dir.join("applications")).unwrap();
            std::fs::write(dir.join("applications").join(file), format!("[Desktop Entry]\nType=Application\nName=PrusaSlicer\nExec={exec}\n"))
                .unwrap();
        };
        // Two desktop files, different names, one program; and a second, real install.
        let (a, b, user) = (root.join("share-a"), root.join("share-b"), home.join(".local/share"));
        entry(&a, "PrusaSlicer.desktop", "prusa-slicer %F");
        entry(&b, "prusa-alias.desktop", &format!("{} %U", bin.join("prusa-slicer").display()));
        entry(&user, "com.prusa3d.PrusaSlicer.desktop", "flatpak run --branch=beta com.prusa3d.PrusaSlicer @@u %U @@");
        let env = |dirs: Vec<PathBuf>| Env { home: Some(home.clone()), data_dirs: dirs, path_dirs: vec![bin.clone()], ..Env::default() };
        let forward = detect_in(&env(vec![a.clone(), b.clone(), user.clone()]));
        let reversed = detect_in(&env(vec![user.clone(), b.clone(), a.clone()]));
        assert_eq!(forward.len(), 2, "{forward:?}");
        assert_eq!(forward, reversed);
        // The place that sorts first speaks for the program: its command, its file name in the id.
        let system = forward.iter().find(|s| s.name.contains("system")).unwrap();
        assert_eq!(system.id, "prusa#prusaslicer");
        assert_eq!(system.command, ["prusa-slicer"]);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn the_same_flatpak_invocation_is_one_install_however_flatpak_is_spelled() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("flatpak-key");
        let launcher = |dir: &str| {
            let p = root.join(dir).join("flatpak");
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, dir.as_bytes()).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        };
        let (on_path, other) = (launcher("bin"), launcher("opt/tool-b"));
        let (a, b) = (root.join("share-a"), root.join("share-b"));
        let entry = |dir: &Path, file: &str, exec: &str| {
            std::fs::create_dir_all(dir.join("applications")).unwrap();
            std::fs::write(dir.join("applications").join(file), format!("[Desktop Entry]\nType=Application\nName=PrusaSlicer\nExec={exec}\n"))
                .unwrap();
        };
        let app = "--branch=beta com.prusa3d.PrusaSlicer @@u %U @@";
        let env = |dirs: Vec<PathBuf>| Env { data_dirs: dirs, path_dirs: vec![root.join("bin")], ..Env::default() };
        entry(&a, "one.desktop", &format!("flatpak run {app}"));
        entry(&b, "two.desktop", &format!("{} run {app}", on_path.display()));
        // `flatpak` found on PATH and the same file by its path: one install.
        let found = detect_in(&env(vec![a.clone(), b.clone()]));
        assert_eq!(found.len(), 1, "{found:?}");
        // A different executable of the same name, with the same arguments: another install.
        entry(&b, "three.desktop", &format!("{} run {app}", other.display()));
        let found = detect_in(&env(vec![a.clone(), b.clone()]));
        assert_eq!(found.len(), 2, "{found:?}");
        // A different branch is another install too.
        entry(&b, "four.desktop", "flatpak run --branch=stable com.prusa3d.PrusaSlicer @@u %U @@");
        assert_eq!(detect_in(&env(vec![a, b])).len(), 3);
        // Where `flatpak` can't be found, its name stands in.
        let none = Env { data_dirs: vec![root.join("share-a")], ..Env::default() };
        assert_eq!(detect_in(&none).len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_slow_busctl_is_not_up_yet_and_a_missing_or_failing_one_is_send_now() {
        let root = scratch("busctl-wait");
        let fake = |name: &str, body: &str| fake_program(&root, name, body);
        let launcher =
            Slicer { id: "prusa".into(), name: "PrusaSlicer".into(), command: vec!["slic3r-app-launcher".into()], icon: None, prusa_major: None };
        let (limit, poll) = (std::time::Duration::from_millis(300), std::time::Duration::from_millis(20));
        // Hangs: not ready, the wait keeps going to its limit and then times out (does not send).
        let hang = fake("hang", "exec sleep 30");
        assert_eq!(launcher.ready_via(&hang, std::time::Duration::from_millis(50)), Some(false));
        let started = std::time::Instant::now();
        assert!(!wait_for_ready_via(&launcher, &hang, limit, poll));
        assert!(started.elapsed() >= limit && started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        // Missing: it can't be told, so no waiting.
        let missing = root.join("missing").to_string_lossy().into_owned();
        assert_eq!(launcher.ready_via(&missing, limit), None);
        let started = std::time::Instant::now();
        assert!(wait_for_ready_via(&launcher, &missing, limit, poll));
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        // Answers "not up" once, then fails: the wait stops and sends.
        let count = root.join("count");
        let flaky = fake(
            "flaky",
            &format!("n=$(cat {c} 2>/dev/null || echo 0); echo $((n+1)) > {c}; [ $n -ge 1 ] && exit 1; echo ':1.5 1 x'", c = count.display()),
        );
        assert!(wait_for_ready_via(&launcher, &flaky, std::time::Duration::from_secs(5), poll));
        assert_eq!(std::fs::read_to_string(&count).unwrap().trim(), "2", "looked twice: not up, then failed");
        // And one that names the slicer is ready at once.
        let up = fake("up", "echo 'com.prusa3d.prusaslicer.BroadcastListener.ab 1 x'");
        assert_eq!(launcher.ready_via(&up, limit), Some(true));
        std::fs::remove_dir_all(root).unwrap();
    }
    /// What the flags and the wait come to for a PrusaSlicer install, by its version.
    fn prusa(command: &str, major: Option<u32>) -> Slicer {
        Slicer {
            id: "prusa".into(),
            name: "PrusaSlicer".into(),
            command: command.split(' ').map(str::to_string).collect(),
            icon: None,
            prusa_major: major,
        }
    }

    // The command lines of this desktop's two Flatpak entries, with `%u` and `%U` dropped as `parse_exec` does.
    const V2_ENTRY: &str =
        "flatpak run --branch=stable --command=entrypoint --file-forwarding com.prusa3d.PrusaSlicer --single-instance-on-url @@u @@";
    const V3_ENTRY: &str = "flatpak run --branch=master --arch=x86_64 --command=slic3r-app-launcher --file-forwarding com.prusa3d.PrusaSlicer --single-instance-on-url @@u @@";

    #[test]
    fn prusa_2_swaps_the_flag_and_does_not_wait_and_prusa_3_keeps_it_and_waits() {
        let with = |flag: &str| {
            let mut v: Vec<String> =
                ["run", "--branch=stable", "--command=entrypoint", "--file-forwarding", "com.prusa3d.PrusaSlicer"].map(String::from).into();
            v.extend([flag, "@@", "/t/a.3mf", "@@"].map(String::from).into_iter().filter(|a| !a.is_empty()));
            v
        };
        // 2.9.6: `--single-instance` in its place, no wait, the file markers as before; nothing without the preference.
        let v2 = prusa(V2_ENTRY, Some(2));
        assert!(v2.waits_for_ready().is_none());
        assert_eq!(v2.args("/t/a.3mf", true), with("--single-instance"));
        assert_eq!(v2.args("/t/a.3mf", false), with(""));
        // Also when its command happens to go through the launcher: the version decides.
        assert!(prusa("slic3r-app-launcher --single-instance-on-url", Some(2)).waits_for_ready().is_none());
        // 3.0.0-alpha10: the launcher's own flag stays on every send and start, and it waits.
        let v3 = prusa(V3_ENTRY, Some(3));
        assert!(v3.waits_for_ready().is_some());
        for single in [true, false] {
            let sent = v3.args("/t/a.3mf", single);
            assert!(sent.contains(&"--single-instance-on-url".to_string()) && !sent.contains(&"--single-instance".to_string()), "{sent:?}");
            assert!(v3.start_args(single).contains(&"--single-instance-on-url".to_string()));
        }
        // Newer than 3 waits too, and so does a 3 whose command is not the launcher.
        assert!(prusa("prusa-slicer", Some(4)).waits_for_ready().is_some());
        assert!(prusa("prusa-slicer", Some(3)).waits_for_ready().is_some());
    }

    #[test]
    fn an_unreadable_version_falls_back_to_the_command() {
        // The launcher's name means 3: it waits and keeps its flag, as before versions were read.
        let launcher = prusa("slic3r-app-launcher --single-instance-on-url", None);
        assert!(launcher.waits_for_ready().is_some());
        assert_eq!(launcher.args("/t/a.3mf", true), ["--single-instance-on-url", "/t/a.3mf"]);
        // Without it nothing is known: no wait, and the entry's flags are left as they are.
        let unknown = prusa(V2_ENTRY, None);
        assert!(unknown.waits_for_ready().is_none());
        let sent = unknown.args("/t/a.3mf", true);
        assert!(sent.contains(&"--single-instance-on-url".to_string()) && !sent.contains(&"--single-instance".to_string()), "{sent:?}");
        assert!(!unknown.args("/t/a.3mf", false).iter().any(|a| a.starts_with("--single-instance")));
    }

    #[test]
    fn a_program_that_is_not_prusaslicer_is_left_alone_whatever_a_version_says() {
        let viewer = Slicer {
            id: "desktop-meshviewer".into(),
            name: "Mesh Viewer".into(),
            command: vec!["mesh-viewer".into(), "--single-instance-on-url".into()],
            icon: None,
            prusa_major: Some(2),
        };
        assert_eq!(viewer.args("/t/a.3mf", true), ["--single-instance-on-url", "/t/a.3mf"]);
        assert_eq!(viewer.args("/t/a.3mf", false), ["/t/a.3mf"]);
        assert!(viewer.waits_for_ready().is_none());
        // One the user picked and named PrusaSlicer is PrusaSlicer, and is told apart by its version like the rest.
        let picked = |name: &str, major| Slicer {
            id: "custom".into(),
            name: name.into(),
            command: vec!["/x/prog".into(), "--single-instance-on-url".into()],
            icon: None,
            prusa_major: major,
        };
        assert_eq!(picked("PrusaSlicer", Some(2)).args("/t/a.3mf", true), ["--single-instance", "/t/a.3mf"]);
        assert_eq!(picked("PrusaSlicer", None).args("/t/a.3mf", true), ["--single-instance-on-url", "/t/a.3mf"]);
        assert_eq!(picked("Mesh Viewer", Some(2)).args("/t/a.3mf", true), ["--single-instance-on-url", "/t/a.3mf"]);
    }

    #[test]
    fn the_version_is_the_leading_number_of_the_top_release() {
        let xml = |v: &str| {
            format!(
                "<component><releases><release version=\"{v}\" date=\"2026-07-28\"><description/></release><release version=\"2.9.5\"/></releases></component>"
            )
        };
        assert_eq!(release_major(&xml("3.0.0-alpha10")), Some(3));
        assert_eq!(release_major(&xml("2.9.6")), Some(2));
        assert_eq!(release_major(&xml("10.1")), Some(10));
        assert_eq!(release_major(&xml("beta")), None);
        assert_eq!(release_major("<component><releases></releases></component>"), None);
        assert_eq!(release_major(""), None);
    }

    #[test]
    fn detection_reads_the_version_from_the_installs_metadata() {
        let root = scratch("versions");
        let (home, share) = (root.join("home"), root.join("home/.local/share"));
        let flatpak = home.join(".local/share/flatpak");
        let deploy = |branch: &str, version: &str| {
            let dir = flatpak.join("app/com.prusa3d.PrusaSlicer/x86_64").join(branch).join("active/files/share/metainfo");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("com.prusa3d.PrusaSlicer.metainfo.xml"),
                format!("<component><releases><release version=\"{version}\"/><release version=\"2.9.5\"/></releases></component>"),
            )
            .unwrap();
        };
        deploy("stable", "2.9.6");
        deploy("master", "3.0.0-alpha10");
        std::fs::create_dir_all(share.join("applications")).unwrap();
        let entry = |file: &str, exec: &str| {
            std::fs::write(share.join("applications").join(file), format!("[Desktop Entry]\nType=Application\nName=PrusaSlicer\nExec={exec}\n"))
                .unwrap();
        };
        entry("v2.desktop", V2_ENTRY);
        entry("v3.desktop", V3_ENTRY);
        // A branch that is not installed has no metadata to read.
        entry("gone.desktop", &V3_ENTRY.replace("--branch=master", "--branch=beta"));
        let env = Env { home: Some(home), data_dirs: vec![share.clone()], flatpak_roots: vec![flatpak], ..Env::default() };
        let found = detect_in(&env);
        let by = |needle: &str| found.iter().find(|s| s.command.iter().any(|a| a == needle)).unwrap();
        assert_eq!(by("--branch=stable").prusa_major, Some(2));
        assert_eq!(by("--branch=master").prusa_major, Some(3));
        assert_eq!(by("--branch=beta").prusa_major, None);
        // And what that decides: 2.9.6 swaps and does not wait; 3.0 keeps its flag and waits; the unread one goes by its launcher.
        assert!(
            by("--branch=stable").waits_for_ready().is_none()
                && by("--branch=stable").args("/t/a.3mf", true).contains(&"--single-instance".to_string())
        );
        assert!(by("--branch=master").waits_for_ready().is_some());
        assert!(by("--branch=beta").waits_for_ready().is_some());
        // A system install: the metadata sits beside its desktop entry.
        let usr = root.join("usr/share");
        std::fs::create_dir_all(usr.join("applications")).unwrap();
        std::fs::create_dir_all(usr.join("metainfo")).unwrap();
        std::fs::write(usr.join("applications/PrusaSlicer.desktop"), "[Desktop Entry]\nType=Application\nName=PrusaSlicer\nExec=prusa-slicer %F\n")
            .unwrap();
        std::fs::write(usr.join("metainfo/PrusaSlicer.metainfo.xml"), "<component><releases><release version=\"2.8.1\"/></releases></component>")
            .unwrap();
        let found = detect_in(&Env { data_dirs: vec![usr], ..Env::default() });
        assert_eq!((found.len(), found[0].prusa_major), (1, Some(2)));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_path_of_a_listed_program_is_accepted_and_any_other_is_not() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("listed-path");
        let apps = root.join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        let image = apps.join("PrusaSlicer-2.9.AppImage");
        std::fs::write(&image, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o755)).unwrap();
        let other = root.join("other");
        std::fs::write(&other, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = root.join("link-to-image");
        std::os::unix::fs::symlink(&image, &link).unwrap();
        let found = detect_in(&Env { program_dirs: vec![apps], ..Env::default() });
        let spec = |s: &std::path::Path| resolve_in(found.clone(), &[], s.to_str().unwrap()).map(|s| s.id);
        // The AppImage as `file.slicers` lists it, and a link to the same file.
        assert_eq!(found[0].command, [image.to_str().unwrap()]);
        assert_eq!(spec(&image).as_deref(), Some("prusa"));
        assert_eq!(spec(&link).as_deref(), Some("prusa"));
        // A program nobody listed or chose is refused, executable or not.
        assert_eq!(spec(&other), None);
        assert_eq!(spec(std::path::Path::new("/bin/sh")), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_keeps_its_id_through_an_upgrade_and_has_its_own_per_folder() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("appimage-ids");
        let exe = |p: &std::path::Path| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        // A second install of PrusaSlicer, so that each one is told apart by an id of its own.
        let bin = root.join("bin");
        exe(&bin.join("prusa-slicer"));
        let ids = |folder: &str, file: &str| {
            let apps = root.join(folder);
            exe(&apps.join(file));
            let found = detect_in(&Env { path_dirs: vec![bin.clone()], program_dirs: vec![apps.clone()], ..Env::default() });
            let image = found.iter().find(|s| s.command == [apps.join(file).to_string_lossy().into_owned()]).unwrap();
            std::fs::remove_file(apps.join(file)).unwrap();
            (image.id.clone(), found.iter().find(|s| s.id != image.id).map(|s| s.id.clone()).unwrap())
        };
        let (v29, path_install) = ids("Apps", "PrusaSlicer-2.9.0.AppImage");
        let (v210, _) = ids("Apps", "PrusaSlicer-2.10.1.AppImage");
        let (elsewhere, _) = ids("Other", "PrusaSlicer-2.9.0.AppImage");
        assert!(v29.starts_with("prusa#appimage-"), "{v29}");
        assert_eq!(v29, v210, "an upgrade in the same folder keeps the id");
        assert_ne!(v29, elsewhere, "another folder is another install");
        assert_eq!(path_install, "prusa#prusaslicer", "installs that are not AppImages are told apart as before");
        // Two versions side by side in one folder are two installs, told apart by their file names.
        let apps = root.join("Apps");
        exe(&apps.join("PrusaSlicer-2.9.0.AppImage"));
        exe(&apps.join("PrusaSlicer-2.10.1.AppImage"));
        let both = detect_in(&Env { path_dirs: vec![bin.clone()], program_dirs: vec![apps.clone()], ..Env::default() });
        let mut images: Vec<&str> = both.iter().filter(|s| s.command[0].ends_with(".AppImage")).map(|s| s.id.as_str()).collect();
        images.sort_unstable();
        assert_eq!(images.len(), 2);
        assert!(images.iter().all(|id| id.starts_with(&format!("{v29}-"))), "{images:?} from {v29}");
        assert_ne!(images[0], images[1]);
        // Each resolves to its own file; the id from when there was only one names neither.
        for id in &images {
            let hit = resolve_in(both.clone(), &[], id).unwrap();
            assert_eq!(&hit.id, id);
        }
        assert_eq!(resolve_in(both.clone(), &[], &v29), None, "an id that is now two installs is not either of them");
        // After the old one is gone, the new one alone has the plain AppImage id again.
        std::fs::remove_file(apps.join("PrusaSlicer-2.9.0.AppImage")).unwrap();
        let after = detect_in(&Env { path_dirs: vec![bin.clone()], program_dirs: vec![apps], ..Env::default() });
        assert!(after.iter().any(|s| s.id == v29 && s.command[0].ends_with("2.10.1.AppImage")), "{after:?}");
        // An id from before this (made of the file name) no longer names an install, here; the picker falls
        // back to the first install of the same slicer for it (`SlicerMenu::key_for`), and the plain `prusa` still resolves.
        let found = detect_in(&Env { path_dirs: vec![bin.clone()], ..Env::default() });
        assert_eq!(resolve_in(found.clone(), &[], "prusa#prusa29"), None);
        assert!(resolve_in(found, &[], "prusa").is_some());
        std::fs::remove_dir_all(root).unwrap();
    }
}
