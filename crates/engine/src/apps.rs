//! Apps a file can be sent to (Send to App, RFC 0018 section 4): a program the user added, an
//! argument template, and the formats it takes. A target is data, never a script run for the user.
//!
//! Only programs the user added are ever started, and only through [`launch`]: the program and its
//! arguments go to the operating system as a list (no shell, no joined string). The application
//! keeps the targets in its settings and tells the commands which there are with [`set_targets`];
//! no command adds one, so a script or an agent can start only what the user chose.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::slicers;

/// The formats a target can take, in the order they are offered: `sc` is the SolidCraft design
/// file, the rest are what `file.send_to_slicer` writes, and STEP.
pub const FORMATS: &[&str] = &["3mf", "3mf-tagged", "step", "stl", "sc"];

/// The message of a browser build, which can't start a program.
pub const WEB_REFUSAL: &str = "Send to App needs the desktop app: a browser can't start a program (download the file instead)";

/// The placeholders an argument may hold.
const PLACEHOLDERS: &[&str] = &["file", "name", "dir", "format"];
/// Most arguments of a template, and the longest one.
const MAX_ARGS: usize = 32;
const MAX_ARG_LEN: usize = 1024;
/// Most targets the user can keep.
pub const MAX_TARGETS: usize = 24;
/// Longest name of a target.
const MAX_NAME: usize = 80;
/// Most exits remembered at once.
#[cfg(not(target_arch = "wasm32"))]
const MAX_EXITS: usize = 64;
/// Programs that are shells or script interpreters: a target may not be one, since its arguments
/// would then be a program to run.
const NOT_PROGRAMS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "env",
    "cmd",
    "powershell",
    "pwsh",
    "perl",
    "ruby",
    "node",
    "wscript",
    "cscript",
    "osascript",
    "java",
    "lua",
    "php",
];
/// File endings of scripts, which the system hands to an interpreter (Windows batch files are
/// read again by the command processor, so their arguments are not safe).
const SCRIPT_ENDINGS: &[&str] = &["bat", "cmd", "ps1", "vbs", "js", "wsf", "py", "sh"];

/// A program the user added to Send to App.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppTarget {
    /// How a command names it (stable); made from the name when it is added.
    pub id: String,
    /// The name shown in the list and in the status line.
    pub name: String,
    /// The program: an existing executable or app bundle, as the user chose it.
    pub program: String,
    /// The arguments after the program, each of which may hold `{file}`, `{name}`, `{dir}` and
    /// `{format}`. Default: `["{file}"]`.
    pub args: Vec<String>,
    /// The formats it takes, from [`FORMATS`].
    pub formats: Vec<String>,
    /// The one used when none is asked for; one of `formats`.
    pub default_format: String,
}

impl Default for AppTarget {
    fn default() -> Self {
        AppTarget {
            id: String::new(),
            name: String::new(),
            program: String::new(),
            args: vec!["{file}".into()],
            formats: vec!["3mf".into()],
            default_format: "3mf".into(),
        }
    }
}

impl AppTarget {
    /// Whether it takes `format`.
    pub fn accepts(&self, format: &str) -> bool {
        self.formats.iter().any(|f| f == format)
    }

    /// Why this target can't be kept or started, if it can't: a blank name, a program that is a
    /// shell or a script, an unknown placeholder, a format that is not one, an argument that could
    /// start with `-` after the design's name is put in.
    pub fn check(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.name.trim().is_empty() || self.name.chars().count() > MAX_NAME {
            return Err(format!("a name of 1 to {MAX_NAME} characters is needed"));
        }
        if self.program.trim().is_empty() {
            return Err("a program is needed".into());
        }
        let base = self.program.rsplit(['/', '\\']).next().unwrap_or(&self.program).to_ascii_lowercase();
        let (stem, ending) = base.rsplit_once('.').unwrap_or((base.as_str(), ""));
        if NOT_PROGRAMS.contains(&stem) || stem.starts_with("python") || SCRIPT_ENDINGS.contains(&ending) {
            return Err(format!("{} is a shell or a script, not an app to send a file to", self.program));
        }
        if self.args.len() > MAX_ARGS {
            return Err(format!("at most {MAX_ARGS} arguments"));
        }
        for arg in &self.args {
            if arg.len() > MAX_ARG_LEN {
                return Err(format!("an argument is longer than {MAX_ARG_LEN} characters"));
            }
            if arg.starts_with("{name}") {
                return Err("an argument may not start with {name}: a design called -rf would become a flag".into());
            }
            expand_arg(arg, None)?;
        }
        if self.formats.is_empty() || self.formats.iter().any(|f| !FORMATS.contains(&f.as_str())) {
            return Err(format!("the formats are some of {}", FORMATS.join(", ")));
        }
        if !self.accepts(&self.default_format) {
            return Err("the default format is not one it takes".into());
        }
        Ok(())
    }
}

/// What the placeholders become.
struct Vars<'a> {
    file: &'a str,
    name: &'a str,
    dir: &'a str,
    format: &'a str,
}

/// `arg` with each `{placeholder}` replaced by its value: inside the argument, never split, and
/// what is put in is not read for placeholders again. With no values it only checks the argument.
fn expand_arg(arg: &str, vars: Option<&Vars<'_>>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = arg;
    while let Some((before, after)) = rest.split_once('{') {
        out.push_str(before);
        let Some((key, tail)) = after.split_once('}') else {
            return Err(format!("`{arg}` has a `{{` with no closing `}}`"));
        };
        if !PLACEHOLDERS.contains(&key) {
            return Err(format!("`{{{key}}}` is not a placeholder (use {{file}}, {{name}}, {{dir}} or {{format}})"));
        }
        if let Some(v) = vars {
            out.push_str(match key {
                "file" => v.file,
                "name" => v.name,
                "dir" => v.dir,
                _ => v.format,
            });
        }
        rest = tail;
    }
    out.push_str(rest);
    Ok(out)
}

/// A target found and ready to start: its program is there and runnable.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub target: AppTarget,
    /// The program and its leading arguments (`open -a <bundle>` for a macOS app bundle).
    pub command: Vec<String>,
}

/// The program and every argument to start `app` with `file`, in the order they are given to the
/// system. `design` is the design's name as a safe file name.
pub fn argv(app: &Resolved, file: &Path, design: &str, format: &str) -> Result<Vec<String>, String> {
    let file = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let dir = file.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let file = file.to_string_lossy().into_owned();
    let vars = Vars { file: &file, name: design, dir: &dir, format };
    let mut out = app.command.clone();
    for arg in &app.target.args {
        out.push(expand_arg(arg, Some(&vars))?);
    }
    Ok(out)
}

/// A command as text for a person to read: an argument is in quotes when it holds a space. Never run.
pub fn command_text(argv: &[String]) -> String {
    let quoted = |a: &String| {
        if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') { format!("\"{}\"", a.replace('"', "\\\"")) } else { a.clone() }
    };
    argv.iter().map(quoted).collect::<Vec<_>>().join(" ")
}

/// The command a template makes with sample values, for the window that adds an app and approves
/// it: `program --title="My part" "/home/me/My part-v1.sc"`. The error says what is wrong with it.
pub fn preview(program: &str, args: &[String], format: &str) -> Result<String, String> {
    let ext = if format == "3mf-tagged" { "3mf" } else { format };
    let file = format!("/home/me/My part-v1.{ext}");
    let vars = Vars { file: &file, name: "My part", dir: "/home/me", format };
    let mut argv = vec![program.to_string()];
    for arg in args {
        argv.push(expand_arg(arg, Some(&vars))?);
    }
    Ok(command_text(&argv))
}

/// The command a target would start, as its template says it (placeholders unreplaced), for a
/// question asked before an agent's send; `None` when no target has this id or name.
pub fn describe(spec: &str) -> Option<String> {
    let all = targets();
    let wanted = normalize(spec);
    let t = all.iter().find(|t| t.id == spec).or_else(|| all.iter().find(|t| !wanted.is_empty() && normalize(&t.name) == wanted))?;
    let mut argv = vec![t.program.clone()];
    argv.extend(t.args.iter().cloned());
    Some(command_text(&argv))
}

/// The targets the application has, set by it; commands may start these and nothing else.
static TARGETS: std::sync::Mutex<Vec<AppTarget>> = std::sync::Mutex::new(Vec::new());

/// Tell the commands which targets the user has (the application keeps them in its settings).
pub fn set_targets(targets: &[AppTarget]) {
    *TARGETS.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = targets.to_vec();
}

/// The targets the user has, in the order they are listed.
pub fn targets() -> Vec<AppTarget> {
    TARGETS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// Lower-case letters and digits only: how names are compared.
fn normalize(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// `target` ready to start, or `None` when it is not valid or its program is missing or not
/// runnable.
pub fn resolved(target: &AppTarget) -> Option<Resolved> {
    target.check().ok()?;
    let program = slicers::custom(&target.program)?;
    Some(Resolved { target: target.clone(), command: program.command })
}

/// The target a command names by its id or name. A program path is never taken: only what the user
/// added.
pub fn resolve(spec: &str) -> Option<Resolved> {
    let all = targets();
    let wanted = normalize(spec);
    let found = all.iter().find(|t| t.id == spec).or_else(|| all.iter().find(|t| !wanted.is_empty() && normalize(&t.name) == wanted))?;
    resolved(found)
}

/// An id for a new target named `name`, not among `taken`: its letters and digits, then a number
/// when that is taken.
pub fn new_id(name: &str, taken: &[AppTarget]) -> String {
    let base = normalize(name);
    let base = if base.is_empty() { "app".to_string() } else { base };
    let mut id = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.id == id) {
        id = format!("{base}{n}");
        n += 1;
    }
    id
}

/// How a started program ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit {
    /// The exit code; `None` when a signal ended it.
    pub code: Option<i32>,
}

impl Exit {
    /// What went wrong, in a sentence, when it did not end with code 0.
    pub fn problem(&self, app: &str, path: &str) -> Option<String> {
        match self.code {
            Some(0) => None,
            Some(c) => Some(format!("{app} exited with code {c}; the file is saved at {path}")),
            None => Some(format!("{app} was stopped; the file is saved at {path}")),
        }
    }
}

/// Programs that ended and have not been asked about, by process id.
static EXITS: std::sync::Mutex<Vec<(u32, Exit)>> = std::sync::Mutex::new(Vec::new());

/// How the process `pid`, started by [`launch`], ended, once it has; asking takes the answer.
pub fn take_exit(pid: u32) -> Option<Exit> {
    let mut exits = EXITS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let at = exits.iter().position(|(p, _)| *p == pid)?;
    Some(exits.remove(at).1)
}

#[cfg(not(target_arch = "wasm32"))]
fn record_exit(pid: u32, status: std::io::Result<std::process::ExitStatus>) {
    let exit = Exit { code: status.ok().and_then(|s| s.code()) };
    let mut exits = EXITS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    exits.retain(|(p, _)| *p != pid);
    if exits.len() >= MAX_EXITS {
        exits.remove(0);
    }
    exits.push((pid, exit));
}

/// Start `app` with `file`, in the folder that holds the file, with no input and no output kept;
/// the process id. A thread waits for the program to end, so it is never left defunct and
/// [`take_exit`] can say how it ended; nothing here waits for it.
#[cfg(not(target_arch = "wasm32"))]
pub fn launch(app: &Resolved, file: &Path, design: &str, format: &str) -> Result<u32, String> {
    use std::process::{Command, Stdio};
    let argv = argv(app, file, design, format)?;
    let (program, args) = argv.split_first().ok_or_else(|| "no program to start".to_string())?;
    let file = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(dir) = file.parent() {
        command.current_dir(dir);
    }
    let name = &app.target.name;
    let mut child = command.spawn().map_err(|e| format!("couldn't start {name} ({e}); the file is saved at {}", file.display()))?;
    let pid = child.id();
    let _ = std::thread::Builder::new().name("send-to-app-exit".into()).spawn(move || record_exit(pid, child.wait()));
    Ok(pid)
}

/// Wait up to `limit` for the process `pid` to end: how, when it did.
#[cfg(not(target_arch = "wasm32"))]
pub fn wait_exit(pid: u32, limit: std::time::Duration) -> Option<Exit> {
    let end = std::time::Instant::now() + limit;
    loop {
        if let Some(exit) = take_exit(pid) {
            return Some(exit);
        }
        if std::time::Instant::now() >= end {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

/// Tests of the commands set the targets, which are process-wide: they take turns.
#[cfg(test)]
pub(crate) static TEST_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn target(program: &str, args: &[&str]) -> AppTarget {
        AppTarget {
            id: "viewer".into(),
            name: "Viewer".into(),
            program: program.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            ..AppTarget::default()
        }
    }

    fn resolved_for(args: &[&str]) -> Resolved {
        Resolved { target: target("/opt/viewer", args), command: vec!["/opt/viewer".into()] }
    }

    #[test]
    fn placeholders_become_one_argument_each_and_spaces_stay_inside_it() {
        let app = resolved_for(&["--title", "Part {name} ({format})", "--dir={dir}", "{file}"]);
        let argv = argv(&app, Path::new("/tmp/my designs/Bracket v2-v1.sc"), "Bracket v2", "sc").unwrap();
        assert_eq!(
            argv,
            ["/opt/viewer", "--title", "Part Bracket v2 (sc)", "--dir=/tmp/my designs", "/tmp/my designs/Bracket v2-v1.sc"].map(String::from)
        );
    }

    #[test]
    fn a_value_after_an_option_is_written_joined_and_a_leading_name_is_refused() {
        // `--title={name}` starts with the option, so a design called -rf can't become a flag.
        let app = resolved_for(&["--title={name}", "{file}"]);
        let argv = argv(&app, Path::new("/t/x.sc"), "-rf", "sc").unwrap();
        assert_eq!(argv[1..], ["--title=-rf", "/t/x.sc"].map(String::from));
        assert!(target("/opt/viewer", &["--title={name}", "{file}"]).check().is_ok());
        for leading in ["{name}", "{name}.txt", "{name}={file}"] {
            let why = target("/opt/viewer", &["--title", leading]).check().unwrap_err();
            assert!(why.contains("{name}") && why.contains("flag"), "{leading}: {why}");
        }
    }

    #[test]
    fn the_command_is_shown_with_sample_values_and_quotes_what_has_a_space() {
        let args = ["--title={name}".to_string(), "{file}".to_string()];
        assert_eq!(preview("/usr/bin/syncapp", &args, "sc").unwrap(), r#"/usr/bin/syncapp "--title=My part" "/home/me/My part-v1.sc""#);
        assert!(preview("/x", &["{path}".to_string()], "sc").is_err());
        assert_eq!(command_text(&["a".to_string(), "b c".to_string()]), "a \"b c\"");
    }

    #[test]
    fn what_is_put_in_is_not_read_for_placeholders_again() {
        let app = resolved_for(&["{name}-{file}"]);
        let argv = argv(&app, Path::new("/t/x.3mf"), "{dir}", "3mf").unwrap();
        assert_eq!(argv.last().map(String::as_str), Some("{dir}-/t/x.3mf"));
    }

    #[test]
    fn no_shell_syntax_is_ever_interpreted() {
        let app = resolved_for(&["{file}; rm -rf ~", "$(id)", "`id`"]);
        let argv = argv(&app, Path::new("/t/a b.stl"), "d", "stl").unwrap();
        assert_eq!(argv[1..], ["/t/a b.stl; rm -rf ~", "$(id)", "`id`"].map(String::from));
    }

    #[test]
    fn a_template_with_an_unknown_placeholder_or_a_leading_name_is_refused() {
        assert!(target("/opt/viewer", &["{file}"]).check().is_ok());
        assert!(target("/opt/viewer", &["{path}"]).check().unwrap_err().contains("not a placeholder"));
        assert!(target("/opt/viewer", &["{file"]).check().unwrap_err().contains("closing"));
        assert!(target("/opt/viewer", &["{name}.txt"]).check().unwrap_err().contains("flag"));
        assert!(target("/opt/viewer", &["--title={name}"]).check().is_ok());
    }

    #[test]
    fn shells_scripts_and_bad_formats_are_refused() {
        for program in ["/bin/sh", "/usr/bin/bash", "C:\\Windows\\System32\\cmd.exe", "/usr/bin/python3", "/x/run.bat", "/x/go.ps1", "/x/y.py"] {
            assert!(target(program, &["{file}"]).check().is_err(), "{program}");
        }
        let mut t = target("/opt/viewer", &["{file}"]);
        t.formats = vec!["dwg".into()];
        assert!(t.check().is_err());
        t.formats = vec!["sc".into()];
        assert!(t.check().is_err(), "the default is not among them");
        t.default_format = "sc".into();
        assert!(t.check().is_ok());
        t.name = " ".into();
        assert!(t.check().is_err());
    }

    #[test]
    fn only_a_program_the_user_added_resolves_and_never_by_its_path() {
        let _turn = TEST_TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::env::temp_dir().join(format!("solidcraft-apps-resolve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("viewer");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let added = target(&program.to_string_lossy(), &["{file}"]);
        set_targets(&[]);
        assert!(resolve("viewer").is_none(), "nothing added yet");
        assert!(resolve(&program.to_string_lossy()).is_none(), "a program path is not a target");
        set_targets(std::slice::from_ref(&added));
        let found = resolve("viewer").map(|r| r.target.id);
        let by_name = resolve("VIEWER").map(|r| r.target.id);
        let by_path = resolve(&program.to_string_lossy());
        let missing = resolve("nothing");
        let blank = resolve("");
        // A target whose program went away is not started.
        let mut gone = added.clone();
        gone.program = dir.join("gone").to_string_lossy().into_owned();
        set_targets(&[gone]);
        let vanished = resolve("viewer");
        set_targets(&[]);
        let _ = std::fs::remove_dir_all(&dir);
        #[cfg(unix)]
        {
            assert_eq!(found.as_deref(), Some("viewer"));
            assert_eq!(by_name.as_deref(), Some("viewer"));
        }
        assert!(by_path.is_none() && missing.is_none() && blank.is_none() && vanished.is_none());
    }

    #[test]
    fn ids_are_made_from_the_name_and_never_repeat() {
        assert_eq!(new_id("My Viewer!", &[]), "myviewer");
        let taken = [AppTarget { id: "myviewer".into(), ..AppTarget::default() }];
        assert_eq!(new_id("My Viewer", &taken), "myviewer2");
        assert_eq!(new_id("???", &[]), "app");
    }

    #[test]
    fn a_nonzero_exit_is_a_problem_and_zero_is_not() {
        assert!(Exit { code: Some(0) }.problem("A", "/f").is_none());
        assert!(Exit { code: Some(3) }.problem("A", "/f").is_some_and(|m| m.contains("code 3") && m.contains("/f")));
        assert!(Exit { code: None }.problem("A", "/f").is_some());
    }

    #[test]
    fn the_web_refusal_names_the_desktop_app_and_the_download() {
        assert!(WEB_REFUSAL.contains("desktop app") && WEB_REFUSAL.contains("download"));
    }
}
