//! Which ACP agents are installed here (desktop only). A desktop app often starts without the
//! shell's PATH, so the usual install folders are searched too. Detection never starts a login
//! or a session: each agent found is asked for `--version` (with a time limit), and whether it
//! looks logged in comes only from whether its own credentials file exists.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// An agent SolidCraft knows how to start in ACP mode.
pub struct Known {
    pub id: &'static str,
    pub name: &'static str,
    /// Program names to look for, in order of preference.
    pub programs: &'static [&'static str],
    /// Arguments that put it in ACP mode.
    pub args: &'static [&'static str],
    /// Its one-line install hint.
    pub install: &'static str,
    /// Its own credentials file, relative to the home folder (`None`: can't tell), and the
    /// variable that moves its settings folder (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) with the file's
    /// name inside it.
    pub credentials: Option<&'static str>,
    pub config_dir_var: Option<(&'static str, &'static str)>,
    /// Variables holding a key it uses instead of a login.
    pub key_vars: &'static [&'static str],
    /// Retired by its maker: still used when it is installed, never offered for install.
    pub retired: bool,
    /// Whether `--version` prints its version. Antigravity's server prints its build date and takes
    /// seconds to start, so it is not asked.
    pub reports_version: bool,
}

/// The agents shown in the picker. The first one found is the default.
pub const KNOWN: &[Known] = &[
    Known {
        id: "claude",
        name: "Claude Code",
        programs: &["claude-agent-acp", "claude-code-acp"],
        args: &[],
        install: "npm install -g @agentclientprotocol/claude-agent-acp",
        credentials: Some(".claude/.credentials.json"),
        config_dir_var: Some(("CLAUDE_CONFIG_DIR", ".credentials.json")),
        key_vars: &["ANTHROPIC_API_KEY"],
        retired: false,
        reports_version: true,
    },
    Known {
        id: "codex",
        name: "Codex",
        programs: &["codex-acp"],
        args: &[],
        install: "npm install -g @agentclientprotocol/codex-acp",
        credentials: Some(".codex/auth.json"),
        config_dir_var: Some(("CODEX_HOME", "auth.json")),
        key_vars: &["OPENAI_API_KEY", "CODEX_API_KEY"],
        retired: false,
        reports_version: true,
    },
    // Google's `agy` command line has no ACP mode; its ACP server is a separate download that the
    // ACP registry lists (`agy_acp_server.par`, `agy_acp_server.exe` on Windows).
    Known {
        id: "antigravity",
        name: "Google Antigravity",
        programs: &["agy_acp_server.par", "agy_acp_server"],
        args: &[],
        install: "download the Antigravity ACP server for your system (see agentclientprotocol.com/registry), unzip it and put agy_acp_server.par on your PATH",
        credentials: Some(".gemini/antigravity-acp/acp_token.json"),
        config_dir_var: None,
        key_vars: &["GEMINI_API_KEY"],
        retired: false,
        reports_version: false,
    },
    // Google retired it on 18 June 2026 in favour of Antigravity (#1790). Enterprise licences may
    // still work, so an installed one is found, but nobody is sent to install it.
    Known {
        id: "gemini",
        name: "Gemini CLI",
        programs: &["gemini"],
        args: &["--acp"],
        install: "npm install -g @google/gemini-cli",
        credentials: Some(".gemini/oauth_creds.json"),
        config_dir_var: None,
        key_vars: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        retired: true,
        reports_version: true,
    },
    Known {
        id: "opencode",
        name: "OpenCode",
        programs: &["opencode"],
        args: &["acp"],
        install: "npm install -g opencode-ai",
        credentials: Some(".local/share/opencode/auth.json"),
        config_dir_var: None,
        key_vars: &[],
        retired: false,
        reports_version: true,
    },
    Known {
        id: "grok",
        name: "XAI Grok",
        programs: &["grok"],
        args: &["agent", "stdio"],
        install: "see https://docs.x.ai/build/overview (Grok Build installer); then run grok login",
        credentials: Some(".grok/auth.json"),
        config_dir_var: None,
        key_vars: &["XAI_API_KEY"],
        retired: false,
        reports_version: true,
    },
    Known {
        id: "copilot",
        name: "GitHub Copilot CLI",
        programs: &["copilot"],
        args: &["--acp"],
        install: "npm install -g @github/copilot",
        credentials: None,
        config_dir_var: None,
        key_vars: &["GH_TOKEN", "GITHUB_TOKEN"],
        retired: false,
        reports_version: true,
    },
    Known {
        id: "qwen",
        name: "Qwen Code",
        programs: &["qwen"],
        args: &["--acp"],
        install: "npm install -g @qwen-code/qwen-code",
        credentials: Some(".qwen/oauth_creds.json"),
        config_dir_var: None,
        key_vars: &["DASHSCOPE_API_KEY"],
        retired: false,
        reports_version: true,
    },
    Known {
        id: "goose",
        name: "goose",
        programs: &["goose"],
        args: &["acp"],
        install: "see block.github.io/goose (installer script or Homebrew)",
        credentials: None,
        config_dir_var: None,
        key_vars: &[],
        retired: false,
        reports_version: true,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Login {
    /// Its credentials file is there.
    LooksLoggedIn,
    /// No login file, but one of its key variables is set.
    KeyInEnvironment,
    /// Its credentials file is missing (it may still use a key from the environment).
    NotLoggedIn,
    /// It keeps no file we can look at (system keychain, or unknown).
    Unknown,
}

impl Login {
    pub fn id(self) -> &'static str {
        match self {
            Login::LooksLoggedIn => "login_found",
            Login::KeyInEnvironment => "key",
            Login::NotLoggedIn => "signed_out",
            Login::Unknown => "unknown",
        }
    }
}

/// A known agent that isn't installed, with its one-line install hint.
#[derive(Clone, Debug)]
pub struct Missing {
    pub id: &'static str,
    pub name: &'static str,
    pub install: &'static str,
}

#[derive(Clone, Debug)]
pub struct Found {
    pub id: &'static str,
    pub name: &'static str,
    /// The full command: the program's path and its ACP arguments.
    pub argv: Vec<String>,
    pub version: Option<String>,
    pub login: Login,
    /// Why its program doesn't start (#9034): asked for `--version`, it failed without printing
    /// one, as the `opencode.exe` of an npm install whose setup script didn't run does on Windows.
    pub problem: Option<String>,
    /// Its one-line install hint, to reinstall it with.
    pub install: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct Detection {
    pub found: Vec<Found>,
    /// Known agents not found.
    pub missing: Vec<Missing>,
    pub took_ms: u64,
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// PATH, then the usual places npm, Homebrew, nvm, Volta, bun, pipx and Grok put commands, and on
/// Windows Node's own install folders: a launcher can start SolidCraft with a PATH that lacks
/// them, and an npm agent's `.cmd` shim then can't find `node` (#9297).
pub fn search_dirs() -> Vec<PathBuf> {
    search_dirs_in(&|name| std::env::var_os(name), cfg!(windows))
}

/// [`search_dirs`] in the environment `var` reads, on Windows or not.
fn search_dirs_in(var: &dyn Fn(&str) -> Option<OsString>, windows: bool) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = var("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(prefix) = var("NPM_CONFIG_PREFIX").map(PathBuf::from) {
        dirs.push(if windows { prefix.clone() } else { prefix.join("bin") });
    }
    if let Some(h) = var(if windows { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|p| p.is_absolute()) {
        for d in [".local/bin", ".npm-global/bin", ".npm-packages/bin", ".volta/bin", ".bun/bin", ".cargo/bin", ".grok/bin", "bin"] {
            dirs.push(h.join(d));
        }
        // nvm: every installed Node version's bin.
        if let Ok(rd) = std::fs::read_dir(h.join(".nvm/versions/node")) {
            let mut v: Vec<PathBuf> = rd.flatten().take(64).map(|e| e.path().join("bin")).collect();
            v.sort();
            dirs.extend(v.into_iter().rev());
        }
    }
    if windows {
        if let Some(a) = var("APPDATA") {
            dirs.push(PathBuf::from(a).join("npm"));
        }
        // Node's installers (64-bit and 32-bit), and nvm-windows' link to the version in use.
        for program_files in ["ProgramFiles", "ProgramFiles(x86)"] {
            dirs.extend(var(program_files).map(|d| PathBuf::from(d).join("nodejs")));
        }
        dirs.extend(var("NVM_SYMLINK").map(PathBuf::from));
    } else {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/home/linuxbrew/.linuxbrew/bin", "/usr/bin"] {
            dirs.push(PathBuf::from(d));
        }
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| d.is_absolute() && seen.insert(d.clone()));
    dirs
}

/// The first `program` in `dirs` (on Windows with `.exe`/`.cmd`/`.bat`).
pub fn find_in(program: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let names = names_for(program, cfg!(windows));
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// The file names `program` can have. On Windows only ones it can run: npm puts a Unix shell
/// script with the bare name next to the `.cmd` shim, and starting that fails with "not a valid
/// Win32 application".
fn names_for(program: &str, windows: bool) -> Vec<String> {
    if windows { [".exe", ".cmd", ".bat"].iter().map(|e| format!("{program}{e}")).collect() } else { vec![program.to_string()] }
}

/// A Windows command saved without an extension (`…\npm\codex-acp`, from an older version's
/// lookup): the `.exe`/`.cmd`/`.bat` beside it, if there is one.
pub fn runnable_sibling(program: &Path) -> Option<PathBuf> {
    if program.extension().is_some() {
        return None;
    }
    let dir = program.parent()?;
    let name = program.file_name()?.to_str()?;
    names_for(name, true).into_iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// Start probes and sessions with the same PATH used for discovery. Desktop launchers often
/// omit Node's install folder even when we found an adapter with a `#!/usr/bin/env node` shebang.
/// Only the child's environment changes; inherited absolute PATH entries keep precedence.
pub(super) fn command_for(program: &Path) -> Command {
    // Older saved Windows commands may name npm's bare Unix script rather than its .cmd shim.
    let shim = if cfg!(windows) { runnable_sibling(program) } else { None };
    let program = shim.as_deref().unwrap_or(program);
    let mut cmd = if cfg!(windows) && program.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    };
    if let Ok(path) = std::env::join_paths(search_dirs()) {
        cmd.env("PATH", path);
    }
    cmd
}

/// What `program --version` showed.
#[derive(Debug, Default, PartialEq)]
struct Probe {
    /// Its version number, from the first line it printed.
    version: Option<String>,
    /// Why it can't start: the system refused to start it, or it failed and printed nothing but
    /// an error. A probe that runs out of time says nothing either way (a slow machine).
    problem: Option<String>,
}

/// How long an agent is given to answer `--version`.
const PROBE_LIMIT: Duration = Duration::from_secs(3);

/// The longest reason kept from a failed probe.
const PROBLEM_CHARS: usize = 300;

/// Ask `program --version`; it is stopped after `limit`.
fn probe(program: &Path, limit: Duration) -> Probe {
    let mut cmd = command_for(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: probing a console agent doesn't flash a terminal window.
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = match cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(child) => child,
        // Windows: "not a valid Win32 application" (193), "not compatible with the version of
        // Windows you're running" (216); elsewhere: not allowed to run it, an unknown format.
        // Windows leaves the program's place in its message as `%1`.
        Err(e) => return Probe { version: None, problem: Some(cut(&e.to_string().replace("%1", &program.display().to_string()))) },
    };
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::default();
            }
        }
    };
    let out = read_capped(child.stdout.take());
    if let Some(line) = out.lines().map(str::trim).find(|l| !l.is_empty()) {
        return Probe { version: Some(version_of(line)), problem: None };
    }
    if status.success() {
        return Probe::default();
    }
    // Through a `.cmd` shim, Windows' own refusal arrives on stderr with a failing exit code.
    let err = read_capped(child.stderr.take());
    let reason = err.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string).unwrap_or_else(|| status.to_string());
    Probe { version: None, problem: Some(cut(&reason)) }
}

/// What a finished probe wrote on a pipe, up to 64 KiB.
fn read_capped(pipe: Option<impl std::io::Read>) -> String {
    let mut bytes = Vec::new();
    if let Some(p) = pipe {
        let _ = std::io::Read::read_to_end(&mut p.take(64 * 1024), &mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `text`, cut short.
fn cut(text: &str) -> String {
    text.chars().take(PROBLEM_CHARS).collect()
}

/// The version number in a `--version` line (`codex-cli 0.162.1`, `2.1.295 (Claude Code)`,
/// `@scope/pkg 2.2.2`), else the line itself, cut short.
fn version_of(line: &str) -> String {
    let v = line.split_whitespace().find(|w| w.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit())).unwrap_or(line);
    v.chars().take(40).collect()
}

/// Whether `k` looks signed in, from files and variables only: nothing is started or asked.
fn login_of(k: &Known, home: Option<&Path>) -> Login {
    let file = match k.config_dir_var.and_then(|(var, name)| std::env::var_os(var).filter(|v| !v.is_empty()).map(|d| PathBuf::from(d).join(name))) {
        Some(p) => Some(p),
        None => k.credentials.zip(home).map(|(rel, h)| h.join(rel)),
    };
    let key = || k.key_vars.iter().any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()));
    match file {
        Some(f) if f.is_file() => Login::LooksLoggedIn,
        _ if key() => Login::KeyInEnvironment,
        // Claude Code keeps its login in the keychain on macOS: the file says nothing there.
        _ if cfg!(target_os = "macos") && k.id == "claude" => Login::Unknown,
        Some(_) => Login::NotLoggedIn,
        None => Login::Unknown,
    }
}

/// Look for every known agent. Blocking (each probe has a time limit): run it off the UI thread.
pub fn detect() -> Detection {
    detect_in(&search_dirs(), home().as_deref(), PROBE_LIMIT)
}

fn detect_in(dirs: &[PathBuf], home: Option<&Path>, limit: Duration) -> Detection {
    let t0 = Instant::now();
    let mut d = Detection::default();
    for k in KNOWN {
        let Some(path) = k.programs.iter().find_map(|p| find_in(p, dirs)) else {
            if !k.retired {
                d.missing.push(Missing { id: k.id, name: k.name, install: k.install });
            }
            continue;
        };
        let login = login_of(k, home);
        let mut argv = vec![path.to_string_lossy().into_owned()];
        argv.extend(k.args.iter().map(|a| a.to_string()));
        let Probe { version, problem } = if k.reports_version { probe(&path, limit) } else { Probe::default() };
        d.found.push(Found { id: k.id, name: k.name, version, argv, login, problem, install: k.install });
    }
    d.took_ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    d
}

/// Query only the endpoint's origin; OpenAI-compatible paths such as `/v1` are unrelated.
#[cfg(not(target_arch = "wasm32"))]
fn api_origin(base_url: &str) -> Option<String> {
    let (scheme, rest) = base_url.trim().split_once("://")?;
    if !matches!(scheme, "http" | "https") {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains(['@', ' ']) || authority.len() > 2048 {
        return None;
    }
    Some(format!("{scheme}://{authority}"))
}

/// Blocking, best-effort Ollama probe. Call only on a background thread, without API keys.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn ollama_tools(http: &super::backend::HttpFn, base_url: &str, model: &str, known_ollama: bool) -> Option<bool> {
    use super::backend::HttpRequest;
    let origin = api_origin(base_url)?;
    let request = |method, path: &str, body| {
        let (status, bytes) = http(&HttpRequest {
            method,
            url: format!("{origin}/api/{path}"),
            headers: if method == "POST" { vec![("Content-Type".into(), "application/json".into())] } else { Vec::new() },
            body,
            timeout_s: 5,
        })
        .ok()?;
        if !(200..300).contains(&status) || bytes.len() > 4 * 1024 * 1024 {
            return None;
        }
        serde_json::from_slice::<serde_json::Value>(&bytes).ok()
    };
    if !known_ollama {
        let version = request("GET", "version", None)?;
        version.get("version")?.as_str()?;
    }
    let body = serde_json::to_vec(&serde_json::json!({"model": model})).ok()?;
    tools_capability(&request("POST", "show", Some(body))?)
}

/// Tool support is unknown unless Ollama returns a capabilities list of strings.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn tools_capability(value: &serde_json::Value) -> Option<bool> {
    let capabilities = value.get("capabilities")?.as_array()?;
    if capabilities.iter().any(|capability| !capability.is_string()) {
        return None;
    }
    Some(capabilities.iter().any(|capability| capability.as_str() == Some("tools")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_capabilities_distinguish_tools_chat_and_unknown() {
        use serde_json::json;
        assert_eq!(tools_capability(&json!({"capabilities": ["completion", "tools"]})), Some(true));
        assert_eq!(tools_capability(&json!({"capabilities": ["completion"]})), Some(false));
        assert_eq!(tools_capability(&json!({"capabilities": []})), Some(false));
        for value in [json!({}), json!({"capabilities": null}), json!({"capabilities": [false]})] {
            assert_eq!(tools_capability(&value), None);
        }
        assert_eq!(api_origin("http://[::1]:11434/proxy/v1/").as_deref(), Some("http://[::1]:11434"));
        assert_eq!(api_origin("file:///tmp/model"), None);
        let http: super::super::backend::HttpFn = std::sync::Arc::new(|request| {
            assert_eq!(request.timeout_s, 5);
            match request.method {
                "GET" => {
                    assert_eq!(request.url, "http://localhost:11434/api/version");
                    Ok((200, br#"{"version":"0.0"}"#.to_vec()))
                }
                "POST" => {
                    assert_eq!(request.url, "http://localhost:11434/api/show");
                    assert_eq!(serde_json::from_slice::<serde_json::Value>(request.body.as_ref().unwrap()).unwrap(), json!({"model":"chat:4b"}));
                    Ok((200, br#"{"capabilities":["completion"]}"#.to_vec()))
                }
                _ => Err("unexpected HTTP method".into()),
            }
        });
        assert_eq!(ollama_tools(&http, "http://localhost:11434/custom/v1", "chat:4b", false), Some(false));
        assert_eq!(ollama_tools(&http, "http://localhost:11434/v1", "chat:4b", true), Some(false));
        let unavailable: super::super::backend::HttpFn = std::sync::Arc::new(|_| Err("unavailable".into()));
        assert_eq!(ollama_tools(&unavailable, "http://localhost:11434/v1", "chat:4b", false), None);
    }

    #[test]
    fn a_program_is_found_in_the_first_folder_that_has_it() {
        let root = std::env::temp_dir().join(format!("sc-agent-find-{}", std::process::id()));
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        // A name this platform runs: Windows looks only for `.exe`/`.cmd`/`.bat` (#1554).
        let name = names_for("codex-acp", cfg!(windows)).remove(0);
        std::fs::write(b.join(&name), "").unwrap();
        assert_eq!(find_in("codex-acp", &[a.clone(), b.clone()]), Some(b.join(name)));
        assert_eq!(find_in("gemini", &[a, b]), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn on_windows_only_runnable_shims_are_candidates() {
        // npm's bare `codex-acp` is a shell script; Windows can't start it (os error 193).
        assert_eq!(names_for("codex-acp", true), ["codex-acp.exe", "codex-acp.cmd", "codex-acp.bat"]);
        assert_eq!(names_for("codex-acp", false), ["codex-acp"]);
    }

    #[test]
    fn a_saved_bare_npm_path_runs_its_cmd_shim() {
        let root = std::env::temp_dir().join(format!("sc-agent-shim-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("codex-acp"), "#!/bin/sh").unwrap();
        std::fs::write(root.join("codex-acp.cmd"), "@echo off").unwrap();
        assert_eq!(runnable_sibling(&root.join("codex-acp")), Some(root.join("codex-acp.cmd")));
        assert_eq!(runnable_sibling(&root.join("codex-acp.cmd")), None);
        assert_eq!(runnable_sibling(&root.join("gemini")), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_version_number_is_picked_out_of_the_line() {
        assert_eq!(version_of("@agentclientprotocol/codex-acp 2.2.2"), "2.2.2");
        assert_eq!(version_of("2.1.295 (Claude Code)"), "2.1.295");
        assert_eq!(version_of("goose v1.9.0"), "v1.9.0");
        assert_eq!(version_of("grok 1.0.58 (26283d10dd58) [alpha]"), "1.0.58");
        assert_eq!(version_of("dev build"), "dev build");
    }

    #[test]
    fn signed_in_comes_from_the_agents_own_file_or_a_key_variable() {
        let home = std::env::temp_dir().join(format!("sc-agent-login-{}", std::process::id()));
        std::fs::create_dir_all(home.join(".gemini")).unwrap();
        let gemini = KNOWN.iter().find(|k| k.id == "gemini").unwrap();
        let qwen = KNOWN.iter().find(|k| k.id == "qwen").unwrap();
        let goose = KNOWN.iter().find(|k| k.id == "goose").unwrap();
        let grok = KNOWN.iter().find(|k| k.id == "grok").unwrap();
        if std::env::var_os("GEMINI_API_KEY").is_none() && std::env::var_os("GOOGLE_API_KEY").is_none() {
            assert_eq!(login_of(gemini, Some(&home)), Login::NotLoggedIn);
        }
        std::fs::write(home.join(".gemini/oauth_creds.json"), "{}").unwrap();
        assert_eq!(login_of(gemini, Some(&home)), Login::LooksLoggedIn);
        if std::env::var_os("DASHSCOPE_API_KEY").is_none() {
            assert_eq!(login_of(qwen, Some(&home)), Login::NotLoggedIn);
        }
        assert_eq!(login_of(goose, Some(&home)), Login::Unknown, "keeps no file we know");
        if std::env::var_os("XAI_API_KEY").is_none() {
            assert_eq!(login_of(grok, Some(&home)), Login::NotLoggedIn);
        }
        // Antigravity's server keeps its own login, apart from the retired Gemini CLI's.
        let agy = KNOWN.iter().find(|k| k.id == "antigravity").unwrap();
        if std::env::var_os("GEMINI_API_KEY").is_none() {
            assert_eq!(login_of(agy, Some(&home)), Login::NotLoggedIn, "the Gemini CLI's file is not its login");
        }
        std::fs::create_dir_all(home.join(".gemini/antigravity-acp")).unwrap();
        std::fs::write(home.join(".gemini/antigravity-acp/acp_token.json"), "not credential data").unwrap();
        assert_eq!(login_of(agy, Some(&home)), Login::LooksLoggedIn);
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        // Discovery checks only for the file; it never needs to parse the user's tokens.
        std::fs::write(home.join(".grok/auth.json"), "not credential data").unwrap();
        assert_eq!(login_of(grok, Some(&home)), Login::LooksLoggedIn);
        let _ = std::fs::remove_dir_all(home);
    }

    /// Google retired the Gemini CLI (#1790): one that is installed is still found, but nobody is
    /// told to install it. Antigravity's ACP server is offered, and found under its file name.
    #[test]
    #[cfg(unix)]
    fn a_retired_agent_is_found_but_not_offered_and_antigravity_is() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("sc-agent-retired-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let run = |name: &str, body: &str| {
            let path = root.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let ids = |d: &Detection| (d.found.iter().map(|f| f.id).collect::<Vec<_>>(), d.missing.iter().map(|m| m.id).collect::<Vec<_>>());

        let none = detect_in(std::slice::from_ref(&root), None, PROBE_LIMIT);
        let (found, missing) = ids(&none);
        assert!(found.is_empty());
        assert!(missing.contains(&"antigravity") && !missing.contains(&"gemini"), "{missing:?}");
        let hint = none.missing.iter().find(|m| m.id == "antigravity").map(|m| m.install).unwrap_or_default();
        assert!(hint.contains("agy_acp_server.par") && !hint.contains("npm"), "{hint}");

        run("gemini", "echo gemini 0.64.0");
        // A server that would take seconds to answer `--version`, which Antigravity is never asked.
        run("agy_acp_server.par", "sleep 30");
        let both = detect_in(std::slice::from_ref(&root), None, PROBE_LIMIT);
        let (found, missing) = ids(&both);
        assert_eq!(found, ["antigravity", "gemini"]);
        assert!(!missing.contains(&"antigravity") && !missing.contains(&"gemini"), "{missing:?}");
        let agy = both.found.iter().find(|f| f.id == "antigravity");
        assert_eq!(agy.map(|f| f.version.clone()), Some(None), "its --version is a build date, not asked");
        assert_eq!(agy.and_then(|f| f.argv.get(1)), None, "the registry starts it without arguments");
        assert!(both.took_ms < 5000, "Antigravity's server was started to read a version");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn every_known_agent_has_a_program_and_an_install_hint() {
        for k in KNOWN {
            assert!(!k.programs.is_empty() && !k.install.is_empty(), "{}", k.id);
        }
        assert_eq!(KNOWN.first().map(|k| k.id), Some("claude"), "the Claude Code adapter is the default");
    }

    /// #9034: npm installed OpenCode without running its setup script, so the `opencode.exe` its
    /// `.cmd` shim starts is one Windows refuses. It is still found (its shim is there), with
    /// the system's reason and the command to install it again, while an agent that answers
    /// `--version`, or ends without a word but successfully, has no problem.
    #[test]
    fn an_agent_that_does_not_start_is_found_with_the_reason() {
        let root = std::env::temp_dir().join(format!("sc-agent-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        #[cfg(windows)]
        {
            // npm's layout: the shim in the prefix, the program under node_modules.
            let bin = root.join("node_modules").join("opencode-ai").join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("opencode.exe"), "not a program for this computer").unwrap();
            std::fs::write(root.join("opencode.cmd"), "@ECHO off\r\n\"%~dp0\\node_modules\\opencode-ai\\bin\\opencode.exe\" %*\r\n").unwrap();
            // Started directly, the system itself refuses it.
            std::fs::write(root.join("copilot.exe"), "not a program either").unwrap();
            std::fs::write(root.join("grok.cmd"), "@echo off\r\nexit /b 3\r\n").unwrap();
            std::fs::write(root.join("qwen.cmd"), "@echo off\r\necho 0.1.2\r\n").unwrap();
            std::fs::write(root.join("goose.cmd"), "@echo off\r\n").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let run = |name: &str, body: &str| {
                let path = root.join(name);
                std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            };
            run("opencode", "echo 'cannot execute binary file: Exec format error' >&2; exit 126");
            // Not allowed to run: the system refuses it.
            std::fs::write(root.join("copilot"), "not a program").unwrap();
            std::fs::set_permissions(root.join("copilot"), std::fs::Permissions::from_mode(0o644)).unwrap();
            run("grok", "exit 3");
            run("qwen", "echo 0.1.2");
            run("goose", "true");
        }
        // A generous limit: on a loaded machine `cmd` alone can take longer than three seconds to
        // start, and a probe that runs out of time reports nothing.
        let d = detect_in(std::slice::from_ref(&root), None, Duration::from_secs(60));
        let found = |id: &str| d.found.iter().find(|f| f.id == id).unwrap_or_else(|| panic!("{id} not found: {:?}", d.found));

        let opencode = found("opencode");
        assert_eq!(opencode.version, None);
        assert!(opencode.problem.as_deref().is_some_and(|p| !p.trim().is_empty()), "{opencode:?}");
        assert_eq!(opencode.install, "npm install -g opencode-ai");
        assert_eq!(opencode.argv.get(1).map(String::as_str), Some("acp"), "still offered with its ACP arguments");
        let copilot = found("copilot");
        assert!(copilot.problem.as_deref().is_some_and(|p| !p.trim().is_empty()), "{copilot:?}");
        #[cfg(windows)]
        assert!(
            copilot.problem.as_deref().is_some_and(|p| (p.contains("216") || p.contains("193")) && p.contains("copilot.exe") && !p.contains("%1")),
            "the system's reason, naming the program: {copilot:?}"
        );
        // Silent but failing: the exit status is the reason.
        let grok = found("grok");
        assert!(grok.problem.as_deref().is_some_and(|p| p.contains('3')), "{grok:?}");

        let qwen = found("qwen");
        assert_eq!((qwen.version.as_deref(), qwen.problem.as_deref()), (Some("0.1.2"), None));
        let goose = found("goose");
        assert_eq!((goose.version.as_deref(), goose.problem.as_deref()), (None, None));
        assert!(d.found.iter().all(|f| f.problem.as_ref().is_none_or(|p| p.chars().count() <= PROBLEM_CHARS)));
        let _ = std::fs::remove_dir_all(root);
    }

    /// A launcher can start SolidCraft with a PATH that lacks Node (#9297). On Windows, Node's
    /// install folders are searched after it, so `node` is found, and an agent started with these
    /// folders as its PATH (`command_for`) finds it too.
    #[test]
    fn on_windows_nodes_install_folders_are_searched() {
        let root = std::env::temp_dir().join(format!("sc-agent-node-{}", std::process::id()));
        let system = root.join("Windows");
        let program_files = root.join("Program Files");
        let x86 = root.join("Program Files (x86)");
        let nvm = root.join("nvm4w").join("nodejs");
        let env = |name: &str| -> Option<OsString> {
            match name {
                "PATH" => Some(system.clone().into()),
                "ProgramFiles" => Some(program_files.clone().into()),
                "ProgramFiles(x86)" => Some(x86.clone().into()),
                "NVM_SYMLINK" => Some(nvm.clone().into()),
                _ => None,
            }
        };
        let dirs = search_dirs_in(&env, true);
        assert_eq!(dirs.first(), Some(&system), "the PATH SolidCraft was given comes first");
        for node in [program_files.join("nodejs"), x86.join("nodejs"), nvm.clone()] {
            assert!(dirs.contains(&node), "{} in {dirs:?}", node.display());
        }
        std::fs::create_dir_all(program_files.join("nodejs")).unwrap();
        for name in ["node.exe", "node"] {
            std::fs::write(program_files.join("nodejs").join(name), "").unwrap();
        }
        assert_eq!(find_in("node", std::slice::from_ref(&system)), None, "not on the PATH the launcher gave");
        assert!(find_in("node", &dirs).is_some_and(|p| p.starts_with(program_files.join("nodejs"))), "{dirs:?}");
        // Nothing is made up where the variables are missing, and elsewhere they mean nothing.
        assert_eq!(search_dirs_in(&|name| (name == "PATH").then(|| system.clone().into()), true), std::slice::from_ref(&system));
        assert!(!search_dirs_in(&env, false).iter().any(|d| d.ends_with("nodejs")));
        let _ = std::fs::remove_dir_all(root);
    }

    /// Finder's PATH lacks user-installed runtimes. Discovery, the version probe and a live
    /// agent must all work with an env shebang whose interpreter is in ~/.local/bin.
    #[test]
    #[cfg(unix)]
    fn desktop_path_can_start_an_agent_and_its_runtime() {
        use super::super::process::{AgentProcess, FromAgent};
        use std::os::unix::fs::PermissionsExt;

        const CHILD: &str = "SOLIDCRAFT_AGENT_PATH_TEST";
        if let Some(root) = std::env::var_os(CHILD) {
            let root = PathBuf::from(root);
            let agent = find_in("solidcraft-test-agent", &search_dirs()).unwrap();
            // Generous limits: both answer at once, but a whole test run on a loaded machine
            // can keep `env`, the shell and the script from starting within 3 s.
            assert_eq!(probe(&agent, Duration::from_secs(20)).version.as_deref(), Some("1.2.3"));
            let mut p = AgentProcess::spawn(&[agent.to_string_lossy().into_owned()], &root, &[], || {}).unwrap();
            let message = serde_json::json!({"id": 1, "method": "initialize"});
            p.send(&message);
            let reply = p.from_agent.recv_timeout(Duration::from_secs(20)).unwrap();
            p.shutdown(true);
            match reply {
                FromAgent::Line(line) => assert_eq!(serde_json::from_str::<serde_json::Value>(&line).unwrap(), message),
                FromAgent::Exited(tail) => panic!("the agent could not start its runtime: {tail:?}"),
            }
            return;
        }

        let root = std::env::temp_dir().join(format!("sc-agent-desktop-path-{}", std::process::id()));
        let bin = root.join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        for (name, script) in [
            ("solidcraft-test-agent", "#!/usr/bin/env solidcraft-test-runtime\n"),
            ("solidcraft-test-runtime", "#!/bin/sh\nif [ \"$2\" = \"--version\" ]; then printf 'test-agent 1.2.3\\n'; else cat; fi\n"),
        ] {
            let path = bin.join(name);
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Change only a child process's environment: no global mutation or unsafe set_var.
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "agent_chat::discover::tests::desktop_path_can_start_an_agent_and_its_runtime", "--nocapture"])
            .env(CHILD, &root)
            .env("HOME", &root)
            .env("PATH", "/usr/bin:/bin")
            .env_remove("NPM_CONFIG_PREFIX")
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(root);
        assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }
}
