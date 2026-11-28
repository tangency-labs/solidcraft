use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::{Arc, Mutex};

const API: &str = "https://api.github.com/repos/maker/gears/releases/latest";
const ASSET: &str = "https://github.com/maker/gears/releases/download/v1.1.0/gears.zip";

fn release(digest: Option<&str>) -> Vec<u8> {
    let mut asset = json!({"name":"gears.zip","browser_download_url":ASSET,"size":1});
    if let Some(digest) = digest {
        asset["digest"] = json!(std::format!("sha256:{digest}"));
    }
    serde_json::to_vec(&json!({"tag_name":"v1.1.0","draft":false,"prerelease":false,"assets":[asset]})).unwrap()
}

fn package(id: &str, version: &str) -> Vec<u8> {
    let manifest = json!({"id":id,"name":"Gears","version":version,"description":"Spur gears","capabilities":["commands@0.1"],
        "updates":{"github":"maker/gears"},
        "runtime":{"module":"gears.wasm","entries":[{"name":"make"}],"commands":["solid.box"],
        "permissions":{"command_groups":["file"],"files":[{"name":"exports","write":true}]}}});
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in
        [(std::format!("{id}/plugin.json"), serde_json::to_vec(&manifest).unwrap()), (std::format!("{id}/gears.wasm"), b"\0asm".to_vec())]
    {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut zip, &bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes).iter().map(|b| std::format!("{b:02x}")).collect()
}

/// Gears 1.0.0 installed through the review, its folder given; the network answers with `answer`
/// and downloads `download`.
fn installed(answer: Result<Vec<u8>, FetchError>, download: Vec<u8>) -> (crate::scenario::Harness, Arc<Mutex<Vec<String>>>) {
    let mut h = crate::scenario::Harness::new();
    h.app.session.plugin_state.plugins_dir = Some(h.dir.join("plugins"));
    let zip = h.dir.join("gears-1.0.0.zip");
    std::fs::write(&zip, package("gears", "1.0.0")).unwrap();
    crate::plugin_install::review(&mut h.app, zip.to_string_lossy().into_owned());
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    let mut ask = h.app.plugin_install.ask.take().expect("asks first");
    assert!(!ask.update);
    ask.grants.folders[0].folder = h.dir.clone();
    crate::plugin_install::install_now(&mut h.app, &ask);
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    assert!(h.app.session.plugin_state.packages.contains_key("gears"));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    h.app.services.http_get = Some(Arc::new(move |url: &str| {
        seen.lock().unwrap().push(url.to_string());
        answer.clone()
    }));
    h.app.services.http_download = Some(Arc::new(move |url: &str, path: &Path, _: &dyn Fn(u64, Option<u64>) -> bool| {
        assert_eq!(url, ASSET);
        std::fs::write(path, &download).map_err(|e| FetchError::Failed(e.to_string()))
    }));
    (h, calls)
}

/// Frames until the background check and download are done.
fn settle(h: &mut crate::scenario::Harness) {
    for _ in 0..500 {
        tick(&mut h.app, &h.ctx);
        if h.app.plugin_updates.check.is_none() && h.app.plugin_updates.download.is_none() {
            crate::plugins::tick(&mut h.app, &h.ctx, true);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the plugin update check never finished");
}

fn plugins_dir(h: &crate::scenario::Harness) -> PathBuf {
    h.dir.join("plugins")
}

fn page(h: &mut crate::scenario::Harness, size: egui::Vec2, evidence: Option<&str>) {
    let dir = plugins_dir(h);
    let ctx = crate::fonts::test_context();
    let mut textures = std::collections::HashMap::new();
    for frame in 0..6 {
        crate::scenario::clear_handles();
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| {
            crate::ui_kit::scope(ui, |ui| crate::plugins::page_in_dir(&mut h.app, ui, Some(&dir)))
        });
        crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
        output.textures_delta.clear();
        if frame == 5
            && let (Some(name), Ok(folder)) = (evidence, std::env::var("SOLIDCRAFT_PLUGINS_EVIDENCE"))
        {
            let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
            std::fs::create_dir_all(&folder).unwrap();
            image.save(Path::new(&folder).join(name)).unwrap();
        }
    }
}

fn review_dialog(h: &mut crate::scenario::Harness, evidence: &str) -> String {
    let ctx = crate::fonts::test_context();
    let mut textures = std::collections::HashMap::new();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
    let mut text = String::new();
    for frame in 0..6 {
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(bounds), time: Some(frame as f64 / 60.0), ..Default::default() }, |ui| {
            crate::ui_kit::scope(ui, |ui| crate::plugin_install::show(&mut h.app, ui.ctx()));
        });
        crate::ui_kit::test_textures_update(&mut textures, &output.textures_delta);
        output.textures_delta.clear();
        if frame == 5 {
            text = crate::test_support::text_shapes(&output).map(|shape| shape.galley.text().to_owned()).collect::<Vec<_>>().join("\n");
            if let Ok(folder) = std::env::var("SOLIDCRAFT_PLUGINS_EVIDENCE") {
                let image = crate::ui_kit::test_raster(&ctx, output.shapes, output.pixels_per_point, bounds, &textures);
                std::fs::create_dir_all(&folder).unwrap();
                image.save(Path::new(&folder).join(evidence)).unwrap();
            }
        }
    }
    text
}

#[test]
fn a_release_is_offered_only_from_its_own_repository_as_a_stable_version() {
    let offer = parse_release(&release(Some(&"AB".repeat(32))), "maker/gears", "gears").unwrap();
    assert_eq!(offer, Offer { version: "1.1.0".into(), url: ASSET.into(), sha256: "ab".repeat(32), repo: "maker/gears".into() });
    assert_eq!(parse_release(&release(Some(&"ab".repeat(32))), "Maker/Gears", "gears").unwrap().url, ASSET, "the owner's case may differ");
    let versioned = json!({"tag_name":"1.1.0","assets":[{"name":"gears-1.1.0.zip","digest":std::format!("sha256:{}", "cd".repeat(32)),
        "browser_download_url":"https://github.com/maker/gears/releases/download/1.1.0/gears-1.1.0.zip"}]});
    assert_eq!(parse_release(&serde_json::to_vec(&versioned).unwrap(), "maker/gears", "gears").unwrap().version, "1.1.0");
    for (field, value) in [
        ("prerelease", json!(true)),
        ("draft", json!(true)),
        ("tag_name", json!("v1.1.0-rc.1")),
        ("tag_name", json!("latest")),
        ("assets", json!([{"name":"gears.zip","browser_download_url":"https://github.com/maker/gears-evil/releases/download/v1.1.0/gears.zip"}])),
        ("assets", json!([{"name":"gears.zip","browser_download_url":"https://example.com/maker/gears/releases/download/v1.1.0/gears.zip"}])),
        ("assets", json!([{"name":"other.zip","browser_download_url":"https://github.com/maker/gears/releases/download/v1.1.0/other.zip"}])),
    ] {
        let mut answer: Value = serde_json::from_slice(&release(Some(&"ab".repeat(32)))).unwrap();
        answer[field] = value;
        assert!(parse_release(&serde_json::to_vec(&answer).unwrap(), "maker/gears", "gears").is_err(), "{field}");
    }
}

#[test]
fn a_rate_limit_ends_the_round_and_a_failure_skips_one_plugin() {
    let plugins = [("a".to_string(), "maker/a".to_string()), ("b".into(), "maker/b".into()), ("c".into(), "maker/c".into())];
    let asked = Mutex::new(Vec::new());
    let (offers, limited) = check_round(&plugins, &|url: &str| {
        asked.lock().unwrap().push(url.to_string());
        if url.contains("/a/") { Err(FetchError::Failed("offline".into())) } else { Err(FetchError::RateLimited { retry_after_s: 120 }) }
    });
    assert!(offers.is_empty());
    assert_eq!(limited, Some(120));
    assert_eq!(asked.lock().unwrap().len(), 2, "nothing more is asked after the rate limit");
}

#[test]
fn a_newer_release_puts_update_beside_the_toggle_and_updates_keeping_the_folders() {
    crate::i18n::install();
    let new = package("gears", "1.1.0");
    let (mut h, calls) = installed(Ok(release(Some(&sha256(&new)))), new);
    settle(&mut h);
    assert_eq!(*calls.lock().unwrap(), [API], "one request, to the plugin's own repository");
    assert_eq!(h.app.plugin_updates.offers["gears"].version, "1.1.0");
    // A turned-off plugin is offered its update too.
    h.app.preferences.disabled_plugins.insert("gears".into());
    page(&mut h, egui::vec2(1280.0, 720.0), Some("plugin-update-card-off.png"));
    assert!(crate::scenario::handle_at("prefs:plugin:gears:update").is_some());
    h.app.preferences.disabled_plugins.remove("gears");
    page(&mut h, egui::vec2(1280.0, 720.0), Some("plugin-update-card.png"));
    let update = crate::scenario::handle_at("prefs:plugin:gears:update").expect("Update shows");
    let toggle = crate::scenario::handle_at("prefs:plugin:gears").unwrap();
    assert!((update.y - toggle.y).abs() < 1.0 && update.x < toggle.x, "Update sits beside the toggle, before it: {update:?} {toggle:?}");
    // Update: download, check, then the same review as Install, keeping the folder at the same access.
    let dir = plugins_dir(&h);
    let plugin = crate::plugin_slots::preferences_inventory(&mut h.app, &dir).into_iter().find(|p| p.id == "gears").unwrap();
    start(&mut h.app, &h.ctx, &plugin);
    settle(&mut h);
    let ask = h.app.plugin_install.ask.as_ref().expect("the update is reviewed");
    assert!(ask.update);
    assert_eq!(ask.grants.folders[0].folder, h.dir, "the folder given to 1.0.0 stays");
    let text = review_dialog(&mut h, "plugin-update-review.png");
    assert!(text.contains("Update plugin") && text.contains("Gears 1.1.0") && text.contains("replaces the installed Gears 1.0.0"), "{text}");
    let ask = h.app.plugin_install.ask.take().unwrap();
    let cached = h.app.plugin_updates.pending.as_ref().unwrap().path.clone();
    assert!(cached.exists());
    crate::plugin_install::install_now(&mut h.app, &ask);
    crate::plugins::tick(&mut h.app, &h.ctx, true);
    tick(&mut h.app, &h.ctx);
    assert!(!cached.exists(), "the downloaded .zip is deleted after the install");
    let manifest: Value = serde_json::from_slice(&std::fs::read(plugins_dir(&h).join("gears/plugin.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], "1.1.0");
    assert_eq!(h.app.preferences.plugin_grants["gears"].folders[0].folder, h.dir);
    page(&mut h, egui::vec2(1280.0, 720.0), None);
    assert!(crate::scenario::handle_at("prefs:plugin:gears:update").is_none(), "1.1.0 is installed: no button");
    assert_eq!(calls.lock().unwrap().len(), 1, "no second check until tomorrow");
}

#[test]
fn a_release_from_another_repository_is_not_offered() {
    crate::i18n::install();
    let (mut h, _) = installed(Err(FetchError::Failed("offline".into())), Vec::new());
    let dir = plugins_dir(&h);
    let mut plugin = crate::plugin_slots::preferences_inventory(&mut h.app, &dir).into_iter().find(|p| p.id == "gears").unwrap();
    let offer = Offer { version: "1.1.0".into(), url: ASSET.into(), sha256: "ab".repeat(32), repo: "maker-a/gears".into() };
    h.app.plugin_updates.offers.insert("gears".into(), offer);
    plugin.updates = Some("maker-b/gears".into());
    assert!(offered(&h.app, &plugin).is_none(), "maker-a's release is not an update for maker-b's gears");
    plugin.updates = Some("Maker-A/Gears".into());
    assert!(offered(&h.app, &plugin).is_some(), "the owner's case may differ");
}

#[test]
fn a_download_that_is_not_the_offered_package_is_refused() {
    crate::i18n::install();
    let new = package("gears", "1.1.0");
    // The checksum GitHub gives doesn't match: nothing is reviewed and nothing is left behind.
    let (mut h, _) = installed(Ok(release(Some(&"0".repeat(64)))), new.clone());
    settle(&mut h);
    let dir = plugins_dir(&h);
    let plugin = crate::plugin_slots::preferences_inventory(&mut h.app, &dir).into_iter().find(|p| p.id == "gears").unwrap();
    start(&mut h.app, &h.ctx, &plugin);
    settle(&mut h);
    assert!(h.app.plugin_install.ask.is_none());
    assert!(h.app.status.as_ref().unwrap().0.contains("checksum"), "{:?}", h.app.status);
    assert_eq!(std::fs::read_dir(plugins_dir(&h).join(".updates")).unwrap().count(), 0);
    // A release whose .zip holds another version is refused at the review.
    let wrong = package("gears", "1.2.0");
    let (mut h, _) = installed(Ok(release(Some(&sha256(&wrong)))), wrong);
    settle(&mut h);
    let dir = plugins_dir(&h);
    let plugin = crate::plugin_slots::preferences_inventory(&mut h.app, &dir).into_iter().find(|p| p.id == "gears").unwrap();
    start(&mut h.app, &h.ctx, &plugin);
    settle(&mut h);
    assert!(h.app.plugin_install.ask.is_none());
    assert!(h.app.status.as_ref().unwrap().0.contains("not gears 1.1.0"), "{:?}", h.app.status);
    tick(&mut h.app, &h.ctx);
    assert_eq!(std::fs::read_dir(plugins_dir(&h).join(".updates")).unwrap().count(), 0);
    let manifest: Value = serde_json::from_slice(&std::fs::read(plugins_dir(&h).join("gears/plugin.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], "1.0.0");
}

#[test]
fn offline_or_with_automatic_checks_off_nothing_shows() {
    crate::i18n::install();
    let (mut h, calls) = installed(Err(FetchError::Failed("offline".into())), Vec::new());
    let status = h.app.status.clone();
    settle(&mut h);
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert!(h.app.plugin_updates.offers.is_empty());
    assert_eq!(h.app.status, status, "a failed check says nothing");
    let (mut h, calls) = installed(Ok(release(Some(&"ab".repeat(32)))), Vec::new());
    h.app.preferences.check_updates = false;
    settle(&mut h);
    assert!(calls.lock().unwrap().is_empty(), "no request at all");
    page(&mut h, egui::vec2(1280.0, 720.0), None);
    assert!(crate::scenario::handle_at("prefs:plugin:gears:update").is_none());
}

#[test]
fn a_release_without_a_digest_is_not_offered() {
    let error = parse_release(&release(None), "maker/gears", "gears").unwrap_err();
    assert_eq!(error, FetchError::Failed("the latest release lists no sha256 digest for gears.zip".into()));
    for bad in ["md5:00".to_string(), std::format!("sha256:{}", "z".repeat(64)), std::format!("sha256:{}", "a".repeat(63))] {
        let mut answer: Value = serde_json::from_slice(&release(None)).unwrap();
        answer["assets"][0]["digest"] = json!(bad);
        assert!(parse_release(&serde_json::to_vec(&answer).unwrap(), "maker/gears", "gears").is_err(), "{bad}");
    }
}

#[test]
fn a_rate_limit_holds_the_app_check_too() {
    crate::i18n::install();
    let (mut h, _) = installed(Err(FetchError::RateLimited { retry_after_s: 600 }), Vec::new());
    settle(&mut h);
    let until = h.app.preferences.update.rate_limited_until;
    assert!(until > unix_now() + 500.0 && until <= unix_now() + 600.0, "{until}");
}
