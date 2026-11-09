//! `solidcraft-cli crashes`: the crash reports the desktop app wrote on this computer
//! (docs/crash-reports.md): list them, show one, replay one, delete them.

use std::sync::Mutex;

use serde_json::{Value, json};
use solidcraft_engine::crash;

/// The command's usage, from the same lines the top-level usage lists.
fn usage() -> String {
    crate::command_usage("crashes")
}

pub fn run(args: &[String]) -> Result<(), String> {
    let dir = crash::default_dir().ok_or("no crash folder on this system")?;
    let id = || args.get(1).map(String::as_str).ok_or_else(usage);
    match args.first().map(String::as_str) {
        None | Some("list") => {
            let reports = crash::list(&dir);
            if args.iter().any(|a| a == "--json") {
                println!("{}", super::pretty(&json!({ "dir": dir.to_string_lossy(), "reports": reports })));
                return Ok(());
            }
            println!("crash reports in {}", dir.display());
            if reports.is_empty() {
                println!("  (none)");
            }
            for r in reports {
                let times = if r.count > 1 { format!(" ×{}", r.count) } else { String::new() };
                let cmd = r.command.map(|c| format!(" in {c}")).unwrap_or_default();
                let replay = if r.replayable { "  [replayable]" } else { "" };
                println!("  {}  {}{times}{cmd}  v{}{replay}\n      {}", r.id, r.kind, r.version, r.message);
            }
            Ok(())
        }
        Some("show") => {
            println!("{}", super::pretty(&crash::read(&dir, id()?)?));
            Ok(())
        }
        Some("replay") => replay(&crash::read(&dir, id()?)?),
        Some("dir") => {
            println!("{}", dir.display());
            Ok(())
        }
        Some("clear") => {
            println!("deleted {} report(s)", crash::clear(&dir, args.get(1).map(String::as_str)));
            Ok(())
        }
        _ => Err(usage()),
    }
}

/// Panics seen while replaying: (location, message).
static PANICS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Run a report's commands on its design and say whether the crash happens again.
fn replay(report: &Value) -> Result<(), String> {
    let id = report["id"].as_str().unwrap_or_default();
    let replay_path = report["replay_path"].as_str().ok_or_else(|| format!("report {id} cannot be replayed (see its notes)"))?;
    let script = super::read_json(replay_path)?;
    let commands = script["commands"].as_array().cloned().unwrap_or_default();
    let mut s = match report["design_path"].as_str() {
        Some(d) => super::load(d)?,
        None => solidcraft_engine::Session::default(),
    };
    std::panic::set_hook(Box::new(|info| {
        let loc = info.location().map_or_else(|| "(unknown)".to_string(), |l| format!("{}:{}", l.file(), l.line()));
        let msg = info.payload_as_str().unwrap_or("(not a text message)").to_string();
        if let Ok(mut p) = PANICS.lock() {
            p.push((loc, msg));
        }
    }));
    if report["kind"] == "unclean_exit" {
        println!("this crash ended the app without a report: if this process ends abruptly, it reproduced");
    }
    println!(
        "replaying {} command(s) on {}",
        commands.len(),
        report["design_path"].as_str().map_or_else(|| "a new design".to_string(), |d| d.to_string())
    );
    for (i, c) in commands.iter().enumerate() {
        let cmd = c["command"].as_str().unwrap_or_default();
        let r = s.execute(cmd, c.get("params").unwrap_or(&Value::Null));
        match r {
            Ok(_) => println!("  {:>3}. {cmd}: ok", i + 1),
            Err(e) => println!("  {:>3}. {cmd}: {e}", i + 1),
        }
    }
    let _ = std::panic::take_hook();
    let seen = PANICS.lock().map(|p| p.clone()).unwrap_or_default();
    let want = report["signature"].as_str().unwrap_or_default();
    let same = seen.iter().any(|(l, m)| format!("{:016x}", crash::signature(l, m)) == want);
    if same {
        println!(
            "reproduced: the same internal error ({}) at {}",
            report["message"].as_str().unwrap_or_default(),
            report["location"].as_str().unwrap_or_default()
        );
    } else if let Some((l, m)) = seen.first() {
        println!("an internal error happened, but not the reported one: {m} at {l}");
        println!("(the report says: {} at {})", report["message"].as_str().unwrap_or_default(), report["location"].as_str().unwrap_or_default());
    } else {
        println!("did not reproduce: no internal error (fixed, or it depends on something the replay does not have)");
    }
    Ok(())
}
