//! Browser storage acceptance checks using the existing HTTP/Chrome/DevTools harness.
use super::{Cdp, Chrome, Mode, PREFIX, Server, TempDir, find_chrome, wait_for};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn phase(cdp: &mut Cdp, name: &str, identity: &str) -> Result<Value, String> {
    cdp.eval(&format!("(async()=>JSON.parse(await window.runPhase({},{})))()", json!(name), json!(identity)))
}

fn rejects(cdp: &mut Cdp, name: &str, identity: &str, expected: &str) -> Result<(), String> {
    match phase(cdp, name, identity) {
        Err(error) if error.contains(expected) => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(format!("{name} should have failed with {expected}")),
    }
}

pub(super) fn open(browser: &Chrome, url: &str) -> Result<Cdp, String> {
    let mut cdp = browser.connect()?;
    cdp.call("Page.navigate", json!({"url":url}))?;
    wait_for(&mut cdp, "!!window.runPhase", "the cloud storage WASM fixture is ready", |value| value == true)?;
    Ok(cdp)
}

pub(super) fn restart(browser: Chrome, cdp: Cdp, executable: &Path, profile: &Path, url: &str) -> Result<(Chrome, Cdp), String> {
    drop(cdp);
    // The shared harness kills and waits for Chrome, simulating an unclean browser exit.
    drop(browser);
    match std::fs::remove_file(profile.join("DevToolsActivePort")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    let browser = Chrome::launch(executable, profile)?;
    let cdp = open(&browser, url)?;
    Ok((browser, cdp))
}

pub fn run(site: &str) -> Result<(), String> {
    let site = std::fs::canonicalize(site).map_err(|error| format!("{site}: {error}"))?;
    let executable = find_chrome().ok_or("cloud-storage-test: Chrome not found (set CHROME)")?;
    let work = TempDir::new()?;
    let staged = work.0.join("site");
    std::fs::create_dir_all(&staged).map_err(|error| error.to_string())?;
    for file in ["cloud_recovery.js", "cloud_recovery_bg.wasm"] {
        std::fs::copy(site.join(file), staged.join(file))
            .map_err(|error| format!("{file}: {error}; build the cloud-recovery example and run wasm-bindgen first"))?;
    }
    std::fs::write(staged.join("index.html"), BOOTSTRAP).map_err(|error| error.to_string())?;
    let server = Server::start(staged.clone())?;
    server.set(Mode::Serve(staged));
    let url = format!("http://127.0.0.1:{}{PREFIX}", server.port);
    let profile = work.0.join("profile");
    let browser = Chrome::launch(&executable, &profile)?;
    let mut cdp = open(&browser, &url)?;

    phase(&mut cdp, "preview-write", "preview_restart")?;
    let (browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    cdp.eval(OFFLINE)?;
    phase(&mut cdp, "preview-read", "preview_restart")?;

    phase(&mut cdp, "lineage-write", "lineage_restart")?;
    let (browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    cdp.eval(OFFLINE)?;
    phase(&mut cdp, "lineage-read", "lineage_restart")?;

    phase(&mut cdp, "worker-competing-write", "competing_restart")?;
    let (browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    cdp.eval(OFFLINE)?;
    phase(&mut cdp, "worker-competing-read", "competing_restart")?;

    phase(&mut cdp, "worker-write", "worker_restart")?;
    let (browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    cdp.eval(OFFLINE)?;
    phase(&mut cdp, "worker-read", "worker_restart")?;
    cdp.eval(HOLD_FETCH)?;
    phase(&mut cdp, "worker-cancel-start", "worker_restart")?;
    wait_for(&mut cdp, "window.fixtureRequestStarted === true", "the cancellable GitHub request started", |value| value == true)?;
    phase(&mut cdp, "worker-cancel-finish", "worker_restart")?;
    wait_for(&mut cdp, "window.fixtureRequestAborted === true", "Cancel aborts the browser Fetch request", |value| value == true)?;
    phase(&mut cdp, "worker-confirm", "worker_restart")?;
    let (browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    cdp.eval(OFFLINE)?;
    phase(&mut cdp, "worker-cached-read", "worker_restart")?;
    cdp.eval(
        "window.fixtureRequestCount=0; window.fetch=()=>{window.fixtureRequestCount++;return Promise.reject(new TypeError('test offline'))}; true",
    )?;
    phase(&mut cdp, "worker-future-document", "worker_future")?;
    if cdp.eval("window.fixtureRequestCount === 0")? != true {
        return Err("future document recovery contacted the repository".into());
    }
    cdp.eval(RESTORE_FETCH)?;
    println!(
        "cloud-storage-test: actual worker retains connections, isolated credentials and CAD captures across offline restarts; Cancel aborts Fetch"
    );

    phase(&mut cdp, "write", "restart")?;
    let (browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    phase(&mut cdp, "read", "restart")?;
    phase(&mut cdp, "confirm", "restart")?;
    let (browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    phase(&mut cdp, "confirmed-read", "restart")?;
    println!("cloud-storage-test: captured and confirmed revisions survive browser termination/restart");

    let mut other = browser.new_tab()?;
    other.call("Page.navigate", json!({"url":url}))?;
    wait_for(&mut other, "!!window.runPhase", "the second storage tab is ready", |value| value == true)?;
    std::thread::scope(|scope| -> Result<(), String> {
        let first = scope.spawn(|| phase(&mut cdp, "write", "two_tabs"));
        let second = phase(&mut other, "write", "two_tabs");
        first.join().map_err(|_| "first browser tab check failed".to_string())??;
        second?;
        Ok(())
    })?;
    phase(&mut cdp, "read", "two_tabs")?;
    println!("cloud-storage-test: simultaneous tabs retain complete captures");

    for (identity, install, restore, expected) in [
        ("failed_close", FAIL_CLOSE, RESTORE_CLOSE, "test close failure"),
        ("uncertain_commit", UNCERTAIN_COMMIT, RESTORE_COMMIT, "test commit failure"),
        ("aborted_commit", ABORT_COMMIT, RESTORE_COMMIT, "test aborted commit"),
    ] {
        cdp.eval(install)?;
        let result = rejects(&mut cdp, "write", identity, expected);
        cdp.eval(restore)?;
        result?;
        // A failed acknowledgement can leave a committed capture. Retry must retain it
        // without duplicating revisions; the Rust fixture checks the exact queue length.
        phase(&mut cdp, "write", identity)?;
        phase(&mut cdp, "read", identity)?;
    }
    println!("cloud-storage-test: failed close, uncertain commit, and aborted transaction retry safely");

    phase(&mut cdp, "write", "future_queue")?;
    cdp.eval(FUTURE_QUEUE)?;
    rejects(&mut cdp, "read", "future_queue", "Upgrade SolidCraft")?;
    phase(&mut cdp, "write", "future_cache")?;
    cdp.eval(FUTURE_CACHE)?;
    rejects(&mut cdp, "write", "future_cache", "Upgrade SolidCraft")?;
    cdp.eval(FUTURE_DATABASE)?;
    rejects(&mut cdp, "read", "future_database", "Upgrade SolidCraft")?;
    println!("cloud-storage-test: future queue, cache, and database formats require an upgrade");

    phase(&mut cdp, "write", "corrupt")?;
    cdp.eval(CORRUPT_OBJECT)?;
    rejects(&mut cdp, "read", "corrupt", "chunk hash mismatch")?;
    println!("cloud-storage-test: corrupt chunks are rejected");

    phase(&mut cdp, "large-write", "large")?;
    let (_browser, mut cdp) = restart(browser, cdp, &executable, &profile, &url)?;
    let large = phase(&mut cdp, "read", "large")?;
    if large["asset_bytes"] != 101 << 20 || large["objects"] != 2 {
        return Err(format!("large-asset recovery or deduplication failed: {large}"));
    }
    println!("cloud-storage-test: 101 MiB asset recovers after termination with two deduplicated objects");

    cdp.eval(FUTURE_SETTINGS)?;
    phase(&mut cdp, "worker-future-settings", "worker_restart")?;
    if cdp.eval(SETTINGS_UNCHANGED)? != true {
        return Err("future connection settings were overwritten".into());
    }
    println!("cloud-storage-test: worker refuses to load or overwrite future connection settings");
    Ok(())
}

pub(super) const OFFLINE: &str = "window.savedFixtureFetch = window.fetch; window.fetch = () => Promise.reject(new TypeError('test offline')); true";
const RESTORE_FETCH: &str = "window.fetch = window.savedFixtureFetch; true";
const HOLD_FETCH: &str = r#"(() => {
 window.fixtureRequestStarted = false; window.fixtureRequestAborted = false;
 window.fetch = (input,init) => new Promise((resolve,reject) => {
  window.fixtureRequestStarted = true;
  const signal = init?.signal ?? input.signal;
  const abort = () => {window.fixtureRequestAborted = true;reject(new DOMException('test abort','AbortError'))};
  if(signal.aborted) abort(); else signal.addEventListener('abort',abort,{once:true});
 });
 return true;
})()"#;
const FUTURE_SETTINGS: &str = r#"new Promise((resolve,reject)=>{
 const request = indexedDB.open('solidcraft-cloud-settings');
 request.onerror = () => reject(request.error);
 request.onsuccess = () => {
  const db = request.result, tx = db.transaction('settings','readwrite'), store = tx.objectStore('settings');
  const read = store.get('connections');
  read.onsuccess = () => store.put(JSON.stringify({...JSON.parse(read.result),format:'solidcraft-connections/999'}),'connections');
  tx.oncomplete = () => {db.close();resolve(true)};
  tx.onerror = () => reject(tx.error);
 };
})"#;
const SETTINGS_UNCHANGED: &str = r#"new Promise((resolve,reject)=>{
 const request = indexedDB.open('solidcraft-cloud-settings');
 request.onerror = () => reject(request.error);
 request.onsuccess = () => {
  const db=request.result, tx=db.transaction('settings'), read=tx.objectStore('settings').get('connections');
  read.onsuccess = () => {const value=JSON.parse(read.result);resolve(value.format==='solidcraft-connections/999' && value.settings.connections.length===2)};
  tx.oncomplete = () => db.close(); tx.onerror = () => reject(tx.error);
 };
})"#;

const BOOTSTRAP: &str = r#"<!doctype html><html><title>SolidCraft cloud recovery test</title>
<script type="module">
import init, {cloud_recovery_phase} from './cloud_recovery.js';
await init();
window.runPhase = cloud_recovery_phase;
window.fixtureDatabase = async (identity) => 'solidcraft-cloud-' + Array.from(new Uint8Array(
  await crypto.subtle.digest('SHA-256', new TextEncoder().encode(identity))))
  .map(byte => byte.toString(16).padStart(2, '0')).join('');
window.changeFixture = async (identity, store, transform) => {
  const name = await window.fixtureDatabase(identity);
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name);
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      const db = request.result;
      const tx = db.transaction(store, 'readwrite');
      const cursor = tx.objectStore(store).openCursor();
      cursor.onsuccess = () => { const row = cursor.result; row.update(JSON.stringify(transform(JSON.parse(row.value)))); };
      tx.oncomplete = () => { db.close(); resolve(true); };
      tx.onerror = () => reject(tx.error);
    };
  });
};
</script></html>"#;

const FAIL_CLOSE: &str = r#"(() => {
  window.savedClose = FileSystemWritableFileStream.prototype.close;
  FileSystemWritableFileStream.prototype.close = () => Promise.reject(new DOMException('test close failure', 'QuotaExceededError'));
  return true;
})()"#;
const RESTORE_CLOSE: &str = "FileSystemWritableFileStream.prototype.close = window.savedClose; true";
const UNCERTAIN_COMMIT: &str = r#"(() => {
  window.savedCommit = IDBTransaction.prototype.commit;
  IDBTransaction.prototype.commit = function () {
    if (this.mode === 'readwrite') throw new DOMException('test commit failure', 'QuotaExceededError');
    return window.savedCommit.call(this);
  };
  return true;
})()"#;
const ABORT_COMMIT: &str = r#"(() => {
  IDBTransaction.prototype.commit = function () {
    if (this.mode === 'readwrite') { this.abort(); throw new DOMException('test aborted commit', 'QuotaExceededError'); }
    return window.savedCommit.call(this);
  };
  return true;
})()"#;
const RESTORE_COMMIT: &str = "IDBTransaction.prototype.commit = window.savedCommit; true";
const FUTURE_QUEUE: &str = r#"window.changeFixture('future_queue', 'pending', value => ({...value,format:'solidcraft-outbox/999'}))"#;
const FUTURE_CACHE: &str = r#"window.changeFixture('future_cache', 'cached', value => ({...value,format:'solidcraft-cache/999'}))"#;
const FUTURE_DATABASE: &str = r#"(async () => {
  const name = await window.fixtureDatabase('future_database');
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 2);
    request.onupgradeneeded = () => request.result.createObjectStore('future-marker');
    request.onerror = () => reject(request.error);
    request.onsuccess = () => { request.result.close(); resolve(true); };
  });
})()"#;
const CORRUPT_OBJECT: &str = r#"(async () => {
  const hash = (await window.fixtureDatabase('corrupt')).slice('solidcraft-cloud-'.length);
  const root = await navigator.storage.getDirectory();
  const cloud = await root.getDirectoryHandle('cloud');
  const connection = await cloud.getDirectoryHandle(hash);
  const objects = await connection.getDirectoryHandle('objects');
  const prefix = (await objects.values().next()).value;
  const handle = (await prefix.values().next()).value;
  const bytes = new Uint8Array(await (await handle.getFile()).arrayBuffer());
  bytes[0] ^= 1;
  const stream = await handle.createWritable();
  await stream.write(bytes); await stream.close();
  return true;
})()"#;
