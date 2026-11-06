//! Makes SolidCraft available to the AI apps on this computer without a step from the user: at
//! start-up, off the window thread, the `solidcraft` entry is added to each installed client
//! (`solidcraft_engine::agent_setup`), and a status message names the clients that got it for the
//! first time. The preference "Make SolidCraft available to AI apps on this computer" turns it
//! off, which takes the entries out again. Best effort: nothing here can fail the launch.

use crate::SolveApp;

/// What the app keeps between frames.
#[derive(Default)]
pub struct State {
    /// The host asked for this (`begin`); tests and automation runs never do, so they never
    /// touch the user's client files.
    armed: bool,
    /// The preference as last acted on.
    applied: Option<bool>,
    job: Option<std::sync::mpsc::Receiver<Vec<&'static str>>>,
}

/// Start managing the client files for this run (the desktop app's normal launch).
pub fn begin(app: &mut SolveApp) {
    app.ai_connect.armed = !cfg!(target_arch = "wasm32");
}

/// The clients already announced, so a client is named once however often its entry is made again.
fn announced_file() -> Option<std::path::PathBuf> {
    solidcraft_engine::recovery::data_dir().map(|d| d.join("ai-apps-connected.json"))
}

fn read_announced(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// The clients in `added` that were not announced before, and the list to save.
pub fn new_clients(announced: &[String], added: &[&str]) -> (Vec<String>, Vec<String>) {
    let fresh: Vec<String> = added.iter().filter(|c| !announced.iter().any(|a| a == *c)).map(|c| (*c).to_owned()).collect();
    let mut all = announced.to_vec();
    all.extend(fresh.iter().cloned());
    (fresh, all)
}

/// How a client is named to the user.
pub fn client_name(id: &str) -> &str {
    match id {
        "opencode" => "OpenCode",
        "claude-desktop" => "Claude Desktop",
        "claude-code" => "Claude Code",
        "cursor" => "Cursor",
        "codex" => "Codex",
        "gemini" => "Gemini CLI",
        "copilot" => "Copilot CLI",
        "windsurf" => "Windsurf",
        "zed" => "Zed",
        "vscode" => "VS Code",
        other => other,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(remove: bool) -> Option<std::sync::mpsc::Receiver<Vec<&'static str>>> {
    use solidcraft_engine::agent_setup as setup;
    let cli = crate::agent_chat::find_cli()?;
    let cli = setup::plain_path(&cli.canonicalize().unwrap_or(cli));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("ai-connect".into())
        .spawn(move || {
            let q = setup::quiet(&setup::Env::from_process(), &cli, remove);
            if remove {
                // Turned off: the next time it is on, the clients are news again.
                if let Some(f) = announced_file() {
                    let _ = std::fs::remove_file(f);
                }
                let _ = tx.send(Vec::new());
            } else {
                let _ = tx.send(q.added);
            }
        })
        .ok()?;
    Some(rx)
}

#[cfg(target_arch = "wasm32")]
fn spawn(_remove: bool) -> Option<std::sync::mpsc::Receiver<Vec<&'static str>>> {
    None
}

/// Every frame: start the job when the preference differs from what was last done, and report
/// what a finished one found.
pub fn tick(app: &mut SolveApp) {
    if !app.ai_connect.armed {
        return;
    }
    let want = app.preferences.connect_ai_apps;
    if app.ai_connect.job.is_none() && app.ai_connect.applied != Some(want) {
        // At the first frame, off means nothing to do: entries are removed when it is turned off.
        let remove = !want;
        let first = app.ai_connect.applied.is_none();
        app.ai_connect.applied = Some(want);
        if !(remove && first) {
            app.ai_connect.job = spawn(remove);
        }
    }
    let Some(rx) = &app.ai_connect.job else { return };
    match rx.try_recv() {
        Ok(added) => {
            app.ai_connect.job = None;
            if added.is_empty() {
                return;
            }
            let Some(file) = announced_file() else { return };
            let (fresh, all) = new_clients(&read_announced(&file), &added);
            if fresh.is_empty() {
                return;
            }
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&file, serde_json::to_string(&all).unwrap_or_default());
            let names = fresh.iter().map(|c| client_name(c)).collect::<Vec<_>>().join(", ");
            app.set_status(crate::i18n::format("ai.connected_toast", &[("clients", &names)]), false);
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => app.ai_connect.job = None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_is_named_once() {
        let (fresh, all) = new_clients(&[], &["cursor", "codex"]);
        assert_eq!(fresh, ["cursor", "codex"]);
        let (fresh, all) = new_clients(&all, &["cursor", "zed"]);
        assert_eq!(fresh, ["zed"]);
        assert_eq!(all, ["cursor", "codex", "zed"]);
        assert!(new_clients(&all, &["cursor"]).0.is_empty());
    }

    #[test]
    fn nothing_runs_unless_the_host_asked() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        tick(&mut app);
        assert!(app.ai_connect.applied.is_none() && app.ai_connect.job.is_none());
    }

    #[test]
    fn it_is_on_by_default_and_saved_off_stays_off() {
        assert!(crate::prefs::Prefs::default().connect_ai_apps);
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.load_prefs(&serde_json::json!({ "preferences": { "connect_ai_apps": false } }).to_string());
        assert!(!app.preferences.connect_ai_apps);
    }
}
