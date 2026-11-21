//! Kill the process while it saves: the design file is always the old version or the new one,
//! whole, and opens.

use std::process::Command;
use std::time::Duration;

fn round_of(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let doc = solidcraft_engine::io::read_design(&bytes).ok()?;
    doc.params.iter().find(|p| p.name == "save_round").map(|p| p.expr.clone())
}

#[test]
fn killed_mid_save_the_old_file_survives() {
    let exe = env!("CARGO_BIN_EXE_solidcraft-cli");
    let dir = std::env::temp_dir().join(format!("solidcraft-kill-mid-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("design.solidcraft");
    let ok = Command::new(exe).args(["save-stress", &path.to_string_lossy(), "1"]).status().unwrap();
    assert!(ok.success());
    assert_eq!(round_of(&path).as_deref(), Some("0"));
    let mut kills = 0;
    let mut seen = std::collections::BTreeSet::new();
    let modified = || std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    for i in 0..12u64 {
        let before = modified();
        let mut child = Command::new(exe).args(["save-stress", &path.to_string_lossy(), "100000"]).spawn().unwrap();
        // Wait until it is in its save loop (its first save replaced the file) and has reached a
        // round that varies from kill to kill, however busy the machine is; then kill it a
        // varying while later.
        let target = i % 10;
        let reached = || modified() != before && round_of(&path).and_then(|r| r.parse::<u64>().ok()).is_some_and(|r| r >= target);
        let t0 = std::time::Instant::now();
        while !reached() && t0.elapsed() < Duration::from_secs(20) && child.try_wait().unwrap().is_none() {
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis((i * 7) % 30));
        if child.try_wait().unwrap().is_none() {
            child.kill().unwrap();
            kills += 1;
        }
        let _ = child.wait();
        let round = round_of(&path);
        assert!(round.is_some(), "after kill {i} the design file is torn or missing");
        seen.insert(round);
        let bak = solidcraft_engine::io::backup_path(&path);
        if bak.exists() {
            assert!(round_of(&bak).is_some(), "after kill {i} the backup is torn");
        }
    }
    assert!(kills > 9, "the saver was killed while running ({kills})");
    assert!(seen.len() > 5, "the saver got to saving before it was killed ({seen:?})");
    let _ = std::fs::remove_dir_all(&dir);
}
