//! Sending a mesh to a program: the one pipeline behind `file.send_to_slicer` and the `slicer`
//! parameter of `file.save_mesh`. Write the file, find the target, start it and wait for it if its
//! rule says so, hand the file over, report. The two commands differ only in where the file goes:
//! `file.send_to_slicer` writes it into this user's slicer folder under a stable name, so sending
//! again replaces it; `file.save_mesh` writes it where the caller said.
//!
//! It takes a [`TargetKind`], and slicers are the only kind there is; a later Send to App would add
//! one (see `slicers::TargetKind`) and send through this same function.

use serde_json::{Value, json};
use solidcraft_io::Format;

use super::easyprint::{Exported, export_bodies};
use super::export_to;
use crate::params::bad;
use crate::slicers::{self, Slicer, TargetKind};
use crate::{EngineError, Result, Session};

/// What to do when the target has to be started first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Wait {
    /// Start it, wait up to ten seconds for it to be ready, then send.
    Block,
    /// Start it and answer `starting`; the caller waits and sends.
    Return,
    /// Send now, started or not.
    No,
}

impl Wait {
    pub(super) fn parse(s: Option<&str>) -> Option<Wait> {
        match s {
            None | Some("block") => Some(Wait::Block),
            Some("return") => Some(Wait::Return),
            Some("no") => Some(Wait::No),
            Some(_) => None,
        }
    }
}

/// Where the file is written.
pub(super) enum Dest<'a> {
    /// This user's slicer folder, under a name that stays the same for the same design and bodies.
    Folder,
    /// The path the caller gave.
    Path(&'a str),
}

pub(super) struct Send<'a> {
    /// The command, for its error messages.
    pub cmd: &'static str,
    pub kind: TargetKind,
    /// The target as the caller named it: an id, a name, or a program path.
    pub spec: &'a str,
    pub bodies: Vec<String>,
    pub format: Format,
    /// Only for [`Dest::Path`]; the slicer folder's file is always standard.
    pub quality: solidcraft_io::MeshQuality,
    pub dest: Dest<'a>,
    pub single_instance: bool,
    pub wait: Wait,
}

fn format_name(f: Format) -> &'static str {
    match f {
        Format::StlBinary | Format::StlAscii => "stl",
        Format::ThreeMfTagged => "3mf-tagged",
        _ => "3mf",
    }
}

/// Send the bodies to the target named by `r.spec`.
pub(super) fn send(s: &mut Session, r: Send<'_>) -> Result<Value> {
    // The target is found before anything is written, so a name that matches nothing writes nothing.
    let target = slicers::resolve_for(r.kind, r.spec).ok_or_else(|| {
        bad(r.cmd, format!("no slicer `{}` (see file.slicers; a program works only once the user has added it under Custom…)", r.spec))
    })?;
    if !target.target().accepts(format_name(r.format)) {
        return Err(bad(r.cmd, format!("{} does not take {}", target.name, format_name(r.format))));
    }
    let mut out = match r.dest {
        Dest::Path(path) => export_to(s, path, r.format, &r.bodies, r.quality)?,
        Dest::Folder => write_in_folder(s, &r)?,
    };
    let path = out.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
    deliver(&target, &path, r.single_instance, r.wait, &mut out);
    Ok(out)
}

/// Write the bodies into the slicer folder.
fn write_in_folder(s: &mut Session, r: &Send<'_>) -> Result<Value> {
    use crate::slicer_files;
    let Exported { bytes, ext, fell_back, bodies } = export_bodies(s, r.cmd, &r.bodies, r.format)?;
    let dir = slicer_files::dir().ok_or_else(|| bad(r.cmd, "no per-user folder on this system"))?;
    let stem = slicer_files::stem(&s.doc.name, s.path.as_deref(), s.uid, &bodies);
    let (file, numbered) = slicer_files::write(&dir, &stem, ext, &bytes).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    Ok(json!({"path": file.to_string_lossy(), "bytes": bytes.len(), "format": ext, "numbered": numbered, "stl_fallback": fell_back}))
}

/// Hand the file at `path` to `target`, starting it first if it has to be, and say what happened in
/// `out`: `slicer` when it went, `slicer_error` when the program would not start, `starting` (with
/// [`Wait::Return`]) or `slicer_timeout` when the target had to be started.
pub(super) fn deliver(target: &Slicer, path: &str, single_instance: bool, wait: Wait, out: &mut Value) {
    deliver_via(target, path, single_instance, wait, &slicers::busctl_program(), slicers::READY_LIMIT, slicers::READY_POLL, out);
}

#[allow(clippy::too_many_arguments)]
fn deliver_via(
    target: &Slicer,
    path: &str,
    mut single_instance: bool,
    wait: Wait,
    busctl: &str,
    limit: std::time::Duration,
    poll: std::time::Duration,
    out: &mut Value,
) {
    let Some(obj) = out.as_object_mut() else { return };
    // A target that drops a file given at a cold start is started alone, and sent the file once it is ready.
    if wait != Wait::No && target.waits_for_ready().is_some() && target.ready_via(busctl, slicers::PROBE_LIMIT) == Some(false) {
        if let Err(e) = target.start(single_instance) {
            obj.insert("slicer_error".into(), json!(e));
            return;
        }
        if wait == Wait::Return {
            obj.insert("starting".into(), json!(target.name));
            return;
        }
        if !slicers::wait_for_ready_via(target, busctl, limit, poll) {
            obj.insert("slicer_timeout".into(), json!(target.name));
            return;
        }
        // After a start the file has to reach the instance that is now running, so the flag goes
        // on whatever `single_instance` says.
        single_instance = true;
    }
    // The file is saved either way; a program that won't start is reported, not fatal.
    match target.open(path, single_instance) {
        Ok(()) => obj.insert("slicer".into(), json!(target.name)),
        Err(e) => obj.insert("slicer_error".into(), json!(e)),
    };
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use crate::slicers::test_support::fake_program;
    use std::time::Duration;

    /// A "PrusaSlicer 3" whose launcher logs what it is started with, and a `busctl` that says the
    /// slicer is not up for the first `cold` looks and is up after that.
    fn setup(tag: &str, cold: u32) -> (std::path::PathBuf, Slicer, String) {
        let root = std::env::temp_dir().join(format!("solidcraft-send-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let launcher_dir = root.join("slic3r-app-launcher");
        std::fs::create_dir_all(&launcher_dir).unwrap();
        let log = root.join("log");
        let launcher = fake_program(&launcher_dir, "PrusaSlicer", &format!("echo \"$@\" >> {}", log.display()));
        let count = root.join("count");
        let busctl = fake_program(
            &root,
            "busctl",
            &format!(
                "n=$(cat {c} 2>/dev/null || echo 0); echo $((n+1)) > {c}; echo ':1.5 1 x'; [ $n -ge {cold} ] && echo 'com.prusa3d.prusaslicer.BroadcastListener.ab 2 y'; exit 0",
                c = count.display()
            ),
        );
        let slicer = Slicer { id: "custom".into(), name: "PrusaSlicer".into(), command: vec![launcher], icon: None, prusa_major: None };
        (root, slicer, busctl)
    }

    fn lines(root: &std::path::Path) -> Vec<String> {
        for _ in 0..100 {
            if root.join("log").exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(100));
        std::fs::read_to_string(root.join("log")).map(|t| t.lines().map(String::from).collect()).unwrap_or_default()
    }

    #[test]
    fn a_cold_launcher_is_started_alone_waited_for_and_then_sent_the_file() {
        let (root, slicer, busctl) = setup("cold", 2);
        let mut out = json!({"path": "/t/part.3mf"});
        deliver_via(&slicer, "/t/part.3mf", false, Wait::Block, &busctl, Duration::from_secs(10), Duration::from_millis(20), &mut out);
        assert_eq!(out["slicer"], "PrusaSlicer", "{out}");
        let log = lines(&root);
        // Started with no file, then sent the file, both with the single-instance flag the preference could not take away.
        assert_eq!(log, ["--single-instance", "--single-instance /t/part.3mf"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_launcher_that_is_up_is_sent_the_file_at_once_and_wait_no_does_not_look() {
        let (root, slicer, busctl) = setup("warm", 0);
        let mut out = json!({"path": "/t/part.3mf"});
        deliver_via(&slicer, "/t/part.3mf", true, Wait::Block, &busctl, Duration::from_secs(10), Duration::from_millis(20), &mut out);
        assert_eq!(lines(&root), ["--single-instance /t/part.3mf"]);
        let (root2, slicer2, busctl2) = setup("no", 99);
        let mut out = json!({"path": "/t/part.3mf"});
        deliver_via(&slicer2, "/t/part.3mf", true, Wait::No, &busctl2, Duration::from_secs(10), Duration::from_millis(20), &mut out);
        assert_eq!(lines(&root2), ["--single-instance /t/part.3mf"], "sent without a start");
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(root2).unwrap();
    }

    #[test]
    fn return_starts_it_and_answers_and_a_launcher_that_never_comes_up_times_out_unsent() {
        let (root, slicer, busctl) = setup("return", 99);
        let mut out = json!({"path": "/t/part.3mf"});
        deliver_via(&slicer, "/t/part.3mf", true, Wait::Return, &busctl, Duration::from_secs(10), Duration::from_millis(20), &mut out);
        assert_eq!(out["starting"], "PrusaSlicer");
        assert_eq!(lines(&root), ["--single-instance"], "started, no file");
        let mut out = json!({"path": "/t/part.3mf"});
        deliver_via(&slicer, "/t/part.3mf", true, Wait::Block, &busctl, Duration::from_millis(200), Duration::from_millis(20), &mut out);
        assert_eq!(out["slicer_timeout"], "PrusaSlicer");
        assert!(out.get("slicer").is_none());
        let log = lines(&root);
        assert!(log.iter().all(|l| !l.contains("part.3mf")), "{log:?}: the file was not sent");
        std::fs::remove_dir_all(root).unwrap();
    }
}
