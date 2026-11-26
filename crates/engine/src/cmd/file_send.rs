//! Sending a mesh to a program: the one pipeline behind `file.send_to_slicer` and the `slicer`
//! parameter of `file.save_mesh`. Write the file, find the target, start it and wait for it if its
//! rule says so, hand the file over, report. The two commands differ only in where the file goes:
//! `file.send_to_slicer` writes it into this user's slicer folder under a stable name, so sending
//! again replaces it; `file.save_mesh` writes it where the caller said.
//!
//! It takes a [`TargetKind`]: a slicer, or an app the user added (`file.send_to_app`, RFC 0018),
//! which is sent a mesh, STEP or a copy of the design file and started with its argument template.

use serde_json::{Value, json};
use solidcraft_io::Format;

use super::easyprint::{Exported, export_bodies};
use super::export_laid_to;
use crate::apps;
use crate::params::bad;
use crate::schema::save_mesh::SlicerDelivery;
use crate::slicer_files;
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

/// A folder the user chose to keep the sent file in (#18311), in place of the slicer folder.
pub(super) struct Copy {
    pub dir: std::path::PathBuf,
    /// Replace one file named after the design each time, rather than a new `-v<N>` each time.
    pub overwrite: bool,
}

pub(super) struct Send<'a> {
    /// The command, for its error messages.
    pub cmd: &'static str,
    pub kind: TargetKind,
    /// The target as the caller named it: an id, a name, or a program path.
    pub spec: &'a str,
    pub bodies: Vec<String>,
    /// 3MF objects with parts (`objects: "component"`, #8139). Only for [`Dest::Path`].
    pub parts: Vec<(String, Vec<String>)>,
    pub format: Format,
    /// Only for [`Dest::Path`]; the slicer folder's file is always standard.
    pub quality: solidcraft_io::MeshQuality,
    pub dest: Dest<'a>,
    /// Only for [`Dest::Folder`].
    pub copy: Option<Copy>,
    pub single_instance: bool,
    pub wait: Wait,
    /// An app only: send a copy of the design file (`sc`) instead of a mesh; `format` is then not read.
    pub design: bool,
    /// An app only: how long to wait, in milliseconds, for the program to end and report its exit code.
    pub wait_ms: u64,
}

fn format_name(f: Format) -> &'static str {
    match f {
        Format::StlBinary | Format::StlAscii => "stl",
        Format::ThreeMfTagged => "3mf-tagged",
        Format::Step => "step",
        _ => "3mf",
    }
}

/// Send the bodies to the target named by `r.spec`.
pub(super) fn send(s: &mut Session, r: Send<'_>) -> Result<Value> {
    if r.kind == TargetKind::App {
        return send_app(s, &r);
    }
    // The target is found before anything is written, so a name that matches nothing writes nothing.
    let target = slicers::resolve_for(r.kind, r.spec).ok_or_else(|| {
        bad(r.cmd, format!("no slicer `{}` (see file.slicers; a program works only once the user has added it under Custom…)", r.spec))
    })?;
    if !target.target().accepts(format_name(r.format)) {
        return Err(bad(r.cmd, format!("{} does not take {}", target.name, format_name(r.format))));
    }
    let mut out = match r.dest {
        Dest::Path(path) => export_laid_to(s, path, r.format, &r.bodies, &r.parts, r.quality)?,
        Dest::Folder => write_in_folder(s, &r)?,
    };
    let path = out.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
    deliver(&target, &path, r.single_instance, r.wait, &mut out);
    Ok(out)
}

/// Write the bodies into the slicer folder.
fn write_in_folder(s: &mut Session, r: &Send<'_>) -> Result<Value> {
    let Exported { bytes, ext, fell_back, bodies, laid } = export_for(s, r)?;
    if let Some(copy) = &r.copy {
        // The file the user keeps: named after the design, in their folder, and the slicer opens it.
        let stem = slicer_files::copy_stem(&s.doc.name);
        let (file, numbered, version) = if copy.overwrite {
            let (file, numbered) =
                slicer_files::write_copy(&copy.dir, &stem, ext, &bytes).map_err(|e| EngineError::Other(format!("{}: {e}", copy.dir.display())))?;
            (file, numbered, None)
        } else {
            let (file, n) =
                slicer_files::write_version(&copy.dir, &stem, ext, &bytes).map_err(|e| EngineError::Other(format!("{}: {e}", copy.dir.display())))?;
            (file, false, Some(n))
        };
        let mut out = json!({"path": file.to_string_lossy(), "bytes": bytes.len(), "format": ext, "numbered": numbered, "stl_fallback": fell_back, "copy": true});
        if let Some(n) = version {
            out["version"] = json!(n);
        }
        laid.report(&mut out);
        return Ok(out);
    }
    let dir = slicer_files::dir().ok_or_else(|| bad(r.cmd, "no per-user folder on this system"))?;
    let stem = slicer_files::stem(&s.doc.name, s.path.as_deref(), s.uid, &bodies);
    if ext == "sc" {
        // A design copy an app may edit is never replaced by a later send: each is a new `-v<N>`.
        let (file, version) =
            slicer_files::write_private_version(&dir, &stem, ext, &bytes).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
        return Ok(json!({"path": file.to_string_lossy(), "bytes": bytes.len(), "format": ext, "numbered": false, "version": version}));
    }
    let (file, numbered) = slicer_files::write(&dir, &stem, ext, &bytes).map_err(|e| EngineError::Other(format!("{}: {e}", dir.display())))?;
    let mut out = json!({"path": file.to_string_lossy(), "bytes": bytes.len(), "format": ext, "numbered": numbered, "stl_fallback": fell_back});
    laid.report(&mut out);
    Ok(out)
}

/// The bytes to send: the bodies as a mesh, as STEP, or a copy of the design file.
fn export_for(s: &Session, r: &Send<'_>) -> Result<Exported> {
    if r.design {
        // The folder the copy will be in decides the relative paths of its links.
        let dir = match &r.copy {
            Some(copy) => copy.dir.clone(),
            None => slicer_files::dir().ok_or_else(|| bad(r.cmd, "no per-user folder on this system"))?,
        };
        let at = dir.join("design.sc");
        let bytes = super::design_copy_bytes(s, r.cmd, &at.to_string_lossy())?;
        return Ok(Exported { bytes, ext: "sc", fell_back: false, bodies: Vec::new(), laid: Default::default() });
    }
    if r.format == Format::Step {
        let mut bodies = r.bodies.clone();
        if bodies.is_empty() {
            bodies = s.selected_items()?.into_iter().filter_map(|x| if let crate::Sel::Body { name } = x { Some(name) } else { None }).collect();
        }
        if bodies.is_empty() {
            return Err(bad(r.cmd, "select the bodies to send, or give `bodies`"));
        }
        let doc = s.doc.painted(&*s.world_state()?);
        let (bytes, _) = solidcraft_io::export_with_parts_noted(
            &doc,
            &bodies,
            &[],
            Format::Step,
            &s.doc.name,
            &s.doc.metadata,
            solidcraft_io::MeshQuality::Standard,
            None,
        )?;
        return Ok(Exported { bytes, ext: "step", fell_back: false, bodies, laid: Default::default() });
    }
    export_bodies(s, r.cmd, &r.bodies, r.format)
}

/// Send to an app the user added: write the file in the send folder (or the copy folder), start the
/// program with its argument template, and say how that went. The file is saved either way.
fn send_app(s: &mut Session, r: &Send<'_>) -> Result<Value> {
    let app = apps::resolve(r.spec).ok_or_else(|| {
        bad(r.cmd, format!("no app `{}` (see file.apps; a program can be started only once the user has added it under Send to App)", r.spec))
    })?;
    let format = if r.design { "sc" } else { format_name(r.format) };
    if !app.target.accepts(format) {
        return Err(bad(r.cmd, format!("{} does not take {format} (it takes {})", app.target.name, app.target.formats.join(", "))));
    }
    if !matches!(r.dest, Dest::Folder) {
        return Err(bad(r.cmd, "an app is sent the file in the send folder"));
    }
    let mut out = write_in_folder(s, r)?;
    let path = out.get("path").and_then(Value::as_str).unwrap_or_default().to_string();
    // What the template calls the format: the one written, which is an STL when a 3MF could not be.
    let written = if out.get("stl_fallback").and_then(Value::as_bool) == Some(true) { "stl" } else { format };
    let design = slicer_files::copy_stem(&s.doc.name);
    let Some(obj) = out.as_object_mut() else { return Ok(out) };
    // What is started, with its real values, whether or not it starts.
    if let Ok(argv) = apps::argv(&app, std::path::Path::new(&path), &design, written) {
        obj.insert("command".into(), json!(argv));
    }
    match apps::launch(&app, std::path::Path::new(&path), &design, written) {
        Ok(pid) => {
            obj.insert("app".into(), json!(app.target.name));
            obj.insert("pid".into(), json!(pid));
            if r.wait_ms > 0
                && let Some(exit) = apps::wait_exit(pid, std::time::Duration::from_millis(r.wait_ms))
            {
                obj.insert("exited".into(), json!(true));
                if let Some(code) = exit.code {
                    obj.insert("exit_code".into(), json!(code));
                }
                if let Some(problem) = exit.problem(&app.target.name, &path) {
                    obj.insert("app_error".into(), json!(problem));
                }
            }
        }
        Err(e) => {
            obj.insert("app_error".into(), json!(e));
        }
    }
    Ok(out)
}

/// Hand the file at `path` to `target`, starting it first if it has to be, and say what happened in
/// `out`: `slicer` when it went, `slicer_error` when the program would not start, `starting` (with
/// [`Wait::Return`]) or `slicer_timeout` when the target had to be started.
pub(super) fn deliver(target: &Slicer, path: &str, single_instance: bool, wait: Wait, out: &mut Value) {
    deliver_via(target, path, single_instance, wait, &slicers::busctl_program(), slicers::ready_limit(), slicers::READY_POLL, out);
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
    // What happened, as the typed value the reply's schema describes; only its one key is set.
    let mut say = |delivery: SlicerDelivery| {
        if let Ok(Value::Object(keys)) = serde_json::to_value(delivery) {
            obj.extend(keys);
        }
    };
    // A target that drops a file given at a cold start is started alone, and sent the file once it is ready.
    if wait != Wait::No && target.waits_for_ready().is_some() && target.ready_via(busctl, slicers::PROBE_LIMIT) == Some(false) {
        if let Err(e) = target.start(single_instance) {
            say(SlicerDelivery { slicer_error: Some(e), ..Default::default() });
            return;
        }
        if wait == Wait::Return {
            say(SlicerDelivery { starting: Some(target.name.clone()), ..Default::default() });
            return;
        }
        if !slicers::wait_for_ready_via(target, busctl, limit, poll) {
            say(SlicerDelivery { slicer_timeout: Some(target.name.clone()), ..Default::default() });
            return;
        }
        // After a start the file has to reach the instance that is now running, so the flag goes
        // on whatever `single_instance` says.
        single_instance = true;
    }
    // The file is saved either way; a program that won't start is reported, not fatal.
    match target.open(path, single_instance) {
        Ok(()) => say(SlicerDelivery { slicer: Some(target.name.clone()), ..Default::default() }),
        Err(e) => say(SlicerDelivery { slicer_error: Some(e), ..Default::default() }),
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

    fn design_send<'a>(copy: Option<Copy>) -> Send<'a> {
        Send {
            cmd: "file.send_to_app",
            kind: TargetKind::App,
            spec: "x",
            bodies: Vec::new(),
            parts: Vec::new(),
            format: Format::ThreeMf,
            quality: Default::default(),
            dest: Dest::Folder,
            copy,
            single_instance: false,
            wait: Wait::No,
            design: true,
            wait_ms: 0,
        }
    }

    #[test]
    fn a_design_copy_is_never_replaced_by_a_later_send_and_the_original_is_not_touched() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Part"})).unwrap();
        let a = write_in_folder(&mut s, &design_send(None)).unwrap();
        let b = write_in_folder(&mut s, &design_send(None)).unwrap();
        assert_ne!(a["path"], b["path"], "{a} {b}");
        assert_eq!(b["version"].as_u64(), a["version"].as_u64().map(|v| v + 1));
        assert!(std::path::Path::new(a["path"].as_str().unwrap()).exists() && std::path::Path::new(b["path"].as_str().unwrap()).exists());
        assert_eq!(a["format"], "sc");
        // The same into the user's own folder: -v1, -v2, and what is there is kept.
        let dir = std::env::temp_dir().join(format!("solidcraft-send-sc-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let copy = || Some(Copy { dir: dir.clone(), overwrite: false });
        let c = write_in_folder(&mut s, &design_send(copy())).unwrap();
        let first = std::fs::read(c["path"].as_str().unwrap()).unwrap();
        let d = write_in_folder(&mut s, &design_send(copy())).unwrap();
        assert_eq!((c["version"].as_u64(), d["version"].as_u64()), (Some(1), Some(2)), "{c} {d}");
        assert!(c["path"].as_str().unwrap().ends_with("-v1.sc") && d["path"].as_str().unwrap().ends_with("-v2.sc"));
        assert_eq!(std::fs::read(c["path"].as_str().unwrap()).unwrap(), first, "the first copy is untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
