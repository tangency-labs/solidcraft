//! Opt-in real GitHub acceptance. Credentials arrive on stdin, never in argv or site files.
use super::{
    Mode, PREFIX, Server, TempDir,
    cloud_storage_test::{OFFLINE, open, phase, restart},
    find_chrome,
};
use serde_json::{Value, json};
use std::io::Read;

pub fn run(site: &str) -> Result<(), String> {
    let mut bytes = Vec::new();
    std::io::stdin().take(16_385).read_to_end(&mut bytes).map_err(|_| "cannot read live-test input")?;
    if bytes.len() > 16_384 {
        return Err("live-test input exceeds 16 KiB".into());
    }
    let mut config: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid live-test input")?;
    let token = config.get("token").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 8192).ok_or("live-test token missing")?.to_owned();
    if !config.pointer("/connection/branch").and_then(Value::as_str).is_some_and(|s| s.starts_with("cloud-acceptance-")) {
        return Err("live-test requires a disposable cloud-acceptance- branch".into());
    }
    run_inner(site, &mut config).map_err(|e| e.replace(&token, "<credential>"))
}
fn run_inner(site: &str, config: &mut Value) -> Result<(), String> {
    let site = std::fs::canonicalize(site).map_err(|e| e.to_string())?;
    let executable = find_chrome().ok_or("Chrome not found (set CHROME)")?;
    let work = TempDir::new()?;
    let staged = work.0.join("site");
    std::fs::create_dir_all(&staged).map_err(|e| e.to_string())?;
    for file in ["cloud_recovery.js", "cloud_recovery_bg.wasm"] {
        std::fs::copy(site.join(file), staged.join(file)).map_err(|_| "build the cloud-recovery fixture with wasm-bindgen first")?;
    }
    std::fs::write(staged.join("index.html"), BOOTSTRAP).map_err(|e| e.to_string())?;
    let server = Server::start(staged.clone())?;
    server.set(Mode::Serve(staged));
    let url = format!("http://127.0.0.1:{}{PREFIX}", server.port);
    let profile = work.0.join("profile");
    let browser = super::Chrome::launch(&executable, &profile)?;
    let mut cdp = open(&browser, &url)?;
    let online = phase(&mut cdp, "online", &config.to_string())?;
    // Retain the confirmed ref for safe cleanup even if the restart check fails.
    println!("{}", json!({"online":online}));
    config.as_object_mut().ok_or("invalid live-test input")?.remove("token");
    let (_browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    cdp.eval(OFFLINE)?;
    let offline = phase(&mut cdp, "offline", &config.to_string())?;
    println!("{}", json!({"offline":offline}));
    Ok(())
}
const BOOTSTRAP: &str = r#"<!doctype html><meta charset="utf-8"><title>SolidCraft cloud acceptance</title>
<script type="module">import init,{cloud_live_phase} from './cloud_recovery.js';
await init();window.runPhase=cloud_live_phase;</script>"#;
