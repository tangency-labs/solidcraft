use super::*;
use crate::Session;

fn fresh_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("solidcraft-crash-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    d
}

fn build() -> Build {
    Build { version: "9.9.9", commit: "abc1234", built: "2026-10-12" }
}

/// The child half of the process tests: does nothing unless a parent asks, then installs the
/// hook and crashes the way `SOLIDCRAFT_CRASH_CHILD` says.
#[test]
fn child() {
    let (Some(dir), Some(mode)) = (solidcraft_geom::env::var_os("CRASH_CHILD_DIR"), solidcraft_geom::env::var("CRASH_CHILD")) else {
        return;
    };
    let fresh = install(PathBuf::from(dir), build());
    println!("FRESH {}", serde_json::to_string(&fresh).unwrap());
    // Dying while starting up, before any command: in a graphics driver, with or without what
    // a native crash handler caught.
    if mode == "startup" || mode == "native" || mode == "signal" {
        phase("window");
        phase("adapter request");
        set_environment("Renderer: Vulkan Test GPU (driver 1.2.3)");
        phase("device creation");
        if mode == "native" {
            let (info, dump) = native_paths().unwrap();
            fs::write(
                info,
                r#"{"code":"0xC0000005","name":"EXCEPTION_ACCESS_VIOLATION","detail":"read of 0x0000000000000010","address":"0x00007FFB12345678","module":"C:\\Windows\\System32\\DriverStore\\igvk64.dll","offset":"0x64EA72","thread":4242}"#,
            )
            .unwrap();
            fs::write(dump, b"MDMP fake").unwrap();
        }
        // What the macOS signal handler writes: no minidump, a walk up the stack instead (with
        // a frame that isn't text, one far too long, and far too many).
        if mode == "signal" {
            let (info, _) = native_paths().unwrap();
            let mut frames = vec![json!("libAGXMetalG13X.dylib+0x1A2B3C"), json!(42), json!("x".repeat(5000))];
            frames.extend((0..100).map(|i| json!(format!("solidcraft+0x{i:X} frame{i}"))));
            let v = json!({"signal": "SIGSEGV", "number": 11, "code": "SIGSEGV", "name": "segmentation fault",
                "detail": "read of 0x0000000000000010 (SEGV_MAPERR)", "address": "0x0000000201A2B3C0",
                "module": "/System/Library/Extensions/AGXMetalG13X.bundle/Contents/MacOS/libAGXMetalG13X.dylib",
                "offset": "0x1A2B3C", "thread": 4242, "thread_name": "main", "pid": 77, "frames": frames});
            fs::write(info, v.to_string()).unwrap();
        }
        std::process::abort();
    }
    set_environment("Renderer: test");
    let mut s = Session { crash_journal: true, ..Default::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
    if mode == "read_only" {
        let state = STATE.get().unwrap();
        let before = read_live_journal(state);
        assert_eq!(before.base, json!("new"));
        assert_eq!(before.replay, vec![json!({"command":"solid.box","params":{"length":10,"width":20,"height":30}})]);
        let evaluations = s.evaluations;
        s.execute("document.inspect", &json!({})).unwrap();
        assert_eq!(s.evaluations, evaluations, "reading the design does not evaluate it");
        let after = read_live_journal(state);
        assert_eq!(after.base, before.base, "a read must not rebase the recovery journal");
        assert_eq!(after.replay, before.replay, "the command that made the box must remain replayable");
        assert!(after.in_flight.is_none());
        end_session();
        println!("READ_ONLY_JOURNAL_OK");
        return;
    }
    // Commands that only read are not kept.
    for _ in 0..50 {
        s.execute("document.inspect", &json!({})).unwrap();
    }
    match mode.as_str() {
        "prepared" => {
            let original = s.scratch();
            s.execute("timeline.roll_to", &json!({"position":0})).unwrap();
            let source = s.scratch();
            let state = STATE.get().unwrap();
            let journal_before_error = read_live_journal(state).replay;
            let invalid = vec![(
                "timeline.redefine".into(),
                json!({"feature":1,"command":"solid.box","params":{"length":"missing_extent","width":20,"height":30}}),
            )];
            assert!(s.scratch().compute_editor_commands(&invalid, &source, original.scratch()).is_err());
            assert_eq!(read_live_journal(state).replay, journal_before_error, "failed worker work never enters recovery");
            let evaluations = s.evaluations;
            s.restore_editor(&source, original).unwrap();
            assert_eq!(s.evaluations, evaluations, "Cancel restores without evaluating");
            assert_eq!(s.doc.marker, None);
            let original = s.scratch();
            s.execute("timeline.roll_to", &json!({"position":0})).unwrap();
            let source = s.scratch();
            let commands =
                vec![("timeline.redefine".into(), json!({"feature":1,"command":"solid.box","params":{"length":12,"width":20,"height":30}}))];
            let before = read_live_journal(state).replay;
            let discarded = s.scratch().compute_editor_commands(&commands, &source, original.scratch()).unwrap();
            drop(discarded);
            assert_eq!(read_live_journal(state).replay, before, "discarded work never enters recovery");
            let prepared = s.scratch().compute_editor_commands(&commands, &source, original).unwrap();
            s.accept_commands(prepared).unwrap();
            let journal = read_live_journal(state);
            assert_eq!(journal.base, json!("new"), "accepted steps form one continuous replay");
            assert_eq!(journal.replay.len(), before.len() + 2, "restore marker and redefine are recorded once");
            let mut replay = Session::default();
            for command in &journal.replay {
                replay.execute(command["command"].as_str().unwrap(), &command["params"]).unwrap();
            }
            assert_eq!(replay.doc.marker, None);
            let volume = replay.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
            assert!((volume - 7200.0).abs() < 0.001);
            let path = state.dir.join("prepared-source.solidcraft");
            s.execute("file.save_as", &json!({"path":path})).unwrap();
            // A host can adopt an opened tab before any command has rebased its journal.
            state.journal.lock().unwrap().last_doc = 0;
            let commands = vec![("solid.box".into(), json!({"length":1,"width":1,"height":1}))];
            let prepared = s.scratch().compute_commands(&commands, None).unwrap();
            s.accept_commands(prepared).unwrap();
            assert_eq!(read_live_journal(state).base, json!({"file":path}));
            println!("PREPARED_JOURNAL_OK");
        }
        "render" => {
            s.execute("render.settings.set", &json!({"name":"Studio","set":{"exposure_ev":2}})).unwrap();
            s.execute("render.settings.get", &json!({"name":"Studio"})).unwrap();
            s.execute("render.settings.set", &json!({"name":"Studio","set":{"exposure_ev":3}})).unwrap();
            s.execute("edit.undo", &json!({})).unwrap();
            s.execute("edit.redo", &json!({})).unwrap();
            let _ = s.execute("diagnostics.crash_test", &json!({"mode":"abort"}));
        }
        "command" => {
            for _ in 0..3 {
                let r = s.execute("diagnostics.crash_test", &json!({"mode": "panic"}));
                assert!(matches!(r, Err(crate::EngineError::Internal(..))), "{r:?}");
            }
            assert_eq!(s.doc.features.len(), 1, "the design is unchanged");
        }
        "kernel" => {
            assert!(s.execute("diagnostics.crash_test", &json!({"mode": "kernel"})).is_err());
        }
        // `thread_dies`: the panic ends the app, as one on the window's thread does.
        "thread" | "thread_dies" => {
            let r = std::thread::Builder::new()
                .name("worker".into())
                .spawn(|| std::panic::panic_any(String::from("worker thread 42 failed")))
                .unwrap()
                .join();
            assert!(r.is_err());
            if mode == "thread_dies" {
                std::process::abort();
            }
        }
        "abort" => {
            let _ = s.execute("diagnostics.crash_test", &json!({"mode": "abort"}));
        }
        // Back to the root between two components: only the session's active component changes.
        "components" => {
            s.execute("component.create", &json!({"name": "Tray"})).unwrap();
            s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5})).unwrap();
            s.execute("component.activate", &json!({"component": "root"})).unwrap();
            s.execute("component.create", &json!({"name": "Bracket"})).unwrap();
            s.execute("solid.box", &json!({"length": 4, "width": 4, "height": 4})).unwrap();
            let _ = s.execute("diagnostics.crash_test", &json!({"mode": "abort"}));
        }
        // A new design and commands on it before the app's next frame (a control-channel batch).
        "new" => {
            s.execute("file.new", &json!({})).unwrap();
            s.execute("solid.sphere", &json!({"diameter": 8})).unwrap();
            let _ = s.execute("diagnostics.crash_test", &json!({"mode": "abort"}));
        }
        _ => {}
    }
    end_session();
}

/// Run [`child`] in a new process; returns its stdout and whether it succeeded.
fn run_child(dir: &Path, mode: &str) -> (String, bool) {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash::tests::child", "--test-threads=1", "--nocapture"])
        .env("SOLIDCRAFT_CRASH_CHILD_DIR", dir)
        .env("SOLIDCRAFT_CRASH_CHILD", mode)
        .env("SOLIDCRAFT_CRASH_TEST", "1")
        .env("RUST_BACKTRACE", "0")
        .output()
        .unwrap();
    (String::from_utf8_lossy(&out.stdout).into_owned(), out.status.success())
}

#[test]
fn accepted_prepared_commands_replay_and_discarded_work_is_not_journaled() {
    let directory = fresh_dir("prepared");
    let (output, ok) = run_child(&directory, "prepared");
    assert!(ok && output.contains("PREPARED_JOURNAL_OK"), "{output}");
    let _ = fs::remove_dir_all(directory);
}

#[test]
fn a_read_only_command_keeps_the_replay_after_body_names_are_pinned() {
    let directory = fresh_dir("read-only");
    let (output, ok) = run_child(&directory, "read_only");
    assert!(ok && output.contains("READ_ONLY_JOURNAL_OK"), "{output}");
    let _ = fs::remove_dir_all(directory);
}

fn only_report(dir: &Path) -> (Summary, Value) {
    let all = list(dir);
    assert_eq!(all.len(), 1, "{all:?}");
    let s = all.into_iter().next().unwrap();
    let v = read(dir, &s.id).unwrap();
    (s, v)
}

#[test]
fn a_command_panic_is_reported_once_with_the_design_and_the_command() {
    let d = fresh_dir("command");
    let (_, ok) = run_child(&d, "command");
    assert!(ok, "the panics were caught");
    let (s, v) = only_report(&d);
    assert_eq!(s.kind, "command_panic");
    assert_eq!(s.count, 3, "repeats are counted, not written again");
    assert_eq!(s.command.as_deref(), Some("diagnostics.crash_test"));
    assert!(s.replayable);
    assert_eq!(v["format"], FORMAT);
    assert_eq!(v["app"]["version"], "9.9.9");
    assert_eq!(v["app"]["commit"], "abc1234");
    assert_eq!(v["system"]["environment"], "Renderer: test");
    assert!(v["message"].as_str().unwrap().contains("an internal error in a command"), "{v}");
    assert!(v["location"].as_str().unwrap().contains("diagnostics.rs"), "{v}");
    let bt = v["backtrace"].as_array().unwrap();
    assert!(bt.iter().any(|f| f.as_str().unwrap().contains("crash_test")), "{bt:?}");
    assert!(!bt.iter().any(|f| f.as_str().unwrap().contains("crash::hook")), "the hook's own frames are dropped: {bt:?}");
    assert_eq!(v["recent_commands"][0]["command"], "solid.box");
    // The design from before the command, and the command: together they reproduce the crash.
    let design = solidcraft_io::read_design(&fs::read(v["design_path"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(design.features.len(), 1);
    let replay: Value = serde_json::from_slice(&fs::read(v["replay_path"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(replay["commands"], json!([{"command": "diagnostics.crash_test", "params": {"mode": "panic"}}]));
    assert!(fs::read_dir(d.join("sessions")).unwrap().next().is_none(), "a clean exit leaves no journal");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_kernel_panic_is_reported_as_recovered() {
    let d = fresh_dir("kernel");
    let (_, ok) = run_child(&d, "kernel");
    assert!(ok);
    let (s, v) = only_report(&d);
    assert_eq!(s.kind, "kernel_panic_recovered");
    assert!(s.replayable);
    assert!(v["message"].as_str().unwrap().contains("in the kernel"), "{v}");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_panic_outside_commands_is_reported() {
    let d = fresh_dir("thread");
    let (_, ok) = run_child(&d, "thread");
    assert!(ok);
    let (s, v) = only_report(&d);
    assert_eq!(s.kind, "panic");
    assert_eq!(v["thread"], "worker");
    assert_eq!(v["message"], "worker thread 42 failed");
    // What the journal knew: a new design and the box.
    assert_eq!(v["base"], "new");
    assert_eq!(v["recent_commands"][0]["command"], "solid.box");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_known_panic_that_ends_the_app_is_counted_not_reported_again_as_an_unclean_exit() {
    let d = fresh_dir("repeat");
    assert!(run_child(&d, "thread").1);
    assert!(!run_child(&d, "thread_dies").1, "the child aborted");
    assert!(run_child(&d, "none").1);
    let (s, _) = only_report(&d);
    assert_eq!(s.kind, "panic");
    assert_eq!(s.count, 2);
    // A repeat the app survives is counted once, though it then ends cleanly.
    assert!(run_child(&d, "thread").1);
    assert_eq!(only_report(&d).0.count, 3);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn an_abort_is_reported_at_the_next_start_with_a_replay() {
    let d = fresh_dir("abort");
    let (_, ok) = run_child(&d, "abort");
    assert!(!ok, "the child aborted");
    assert!(list(&d).is_empty(), "no hook sees an abort");
    // The next start finds the journal nobody holds and tells about it.
    let (out, ok) = run_child(&d, "none");
    assert!(ok);
    let fresh: Vec<Value> = out.lines().find_map(|l| l.split_once("FRESH ").map(|x| x.1)).and_then(|j| serde_json::from_str(j).ok()).unwrap();
    assert_eq!(fresh.len(), 1, "{out}");
    let (s, v) = only_report(&d);
    assert_eq!(s.kind, "unclean_exit");
    assert!(s.message.contains("while running `diagnostics.crash_test`"), "{}", s.message);
    assert_eq!(v["app"]["version"], "9.9.9");
    // From a new design: the box, then the command in flight.
    let replay: Value = serde_json::from_slice(&fs::read(v["replay_path"].as_str().unwrap()).unwrap()).unwrap();
    let ids: Vec<&str> = replay["commands"].as_array().unwrap().iter().map(|c| c["command"].as_str().unwrap()).collect();
    assert_eq!(ids, ["solid.box", "diagnostics.crash_test"], "read-only commands are not journaled");
    assert!(v.get("design_path").is_none());
    // Told once: the start after that has nothing new.
    let (out, _) = run_child(&d, "none");
    assert!(out.contains("FRESH []"), "{out}");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn render_settings_survive_unclean_exit_and_journal_replay() {
    let d = fresh_dir("render");
    assert!(!run_child(&d, "render").1);
    assert!(run_child(&d, "none").1);
    let (_, report) = only_report(&d);
    let replay: Value = serde_json::from_slice(&fs::read(report["replay_path"].as_str().unwrap()).unwrap()).unwrap();
    let commands = replay["commands"].as_array().unwrap();
    assert!(!commands.iter().any(|c| c["command"] == "render.settings.get"));
    assert_eq!(commands.iter().filter(|c| c["command"] == "render.settings.set").count(), 2);
    let mut recovered = Session::default();
    for command in commands {
        let id = command["command"].as_str().unwrap();
        if id != "diagnostics.crash_test" {
            recovered.execute(id, &command["params"]).unwrap();
        }
    }
    assert_eq!(recovered.doc.features.len(), 1);
    assert_eq!(recovered.doc.render.setups["Studio"].exposure_ev, 3.0);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn activating_a_component_is_journaled_so_the_replay_builds_in_the_same_component() {
    let d = fresh_dir("components");
    assert!(!run_child(&d, "components").1);
    assert!(run_child(&d, "none").1);
    let (_, report) = only_report(&d);
    let replay: Value = serde_json::from_slice(&fs::read(report["replay_path"].as_str().unwrap()).unwrap()).unwrap();
    let commands = replay["commands"].as_array().unwrap();
    assert_eq!(commands.iter().filter(|c| c["command"] == "component.activate").count(), 1, "{replay}");
    let mut recovered = Session::default();
    for command in commands {
        let id = command["command"].as_str().unwrap();
        if id != "diagnostics.crash_test" {
            recovered.execute(id, &command["params"]).unwrap();
        }
    }
    let bracket = recovered.doc.components.iter().find(|c| c.name == "Bracket").unwrap();
    assert_eq!(bracket.parent, 0, "the Bracket was made under the root");
    let _ = fs::remove_dir_all(&d);
}

/// A new design is journaled at once, not from the app's next frame: commands sent right after
/// `file.new` are in the replay, and the report names the command in flight, not `file.new`.
#[test]
fn commands_right_after_a_new_design_are_journaled() {
    let d = fresh_dir("new");
    assert!(!run_child(&d, "new").1);
    assert!(run_child(&d, "none").1);
    let (s, report) = only_report(&d);
    assert!(s.message.contains("while running `diagnostics.crash_test`"), "{}", s.message);
    let replay: Value = serde_json::from_slice(&fs::read(report["replay_path"].as_str().unwrap()).unwrap()).unwrap();
    let commands = replay["commands"].as_array().unwrap();
    let ids: Vec<&str> = commands.iter().map(|c| c["command"].as_str().unwrap()).collect();
    assert_eq!(ids, ["solid.box", "file.new", "solid.sphere", "diagnostics.crash_test"]);
    let mut recovered = Session::default();
    for command in commands {
        let id = command["command"].as_str().unwrap();
        if id != "diagnostics.crash_test" {
            recovered.execute(id, &command["params"]).unwrap();
        }
    }
    assert_eq!(recovered.doc.features.len(), 1, "the sphere, on the new design");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_crash_while_starting_up_names_the_step_and_the_renderer() {
    let d = fresh_dir("startup");
    let (_, ok) = run_child(&d, "startup");
    assert!(!ok, "the child aborted");
    let (_, ok) = run_child(&d, "none");
    assert!(ok);
    let (s, v) = only_report(&d);
    assert_eq!(s.kind, "unclean_exit");
    assert!(s.message.contains("during startup (device creation"), "{}", s.message);
    assert_eq!(v["location"], "unclean exit during startup: device creation");
    assert_eq!(v["startup_phase"]["step"], "device creation");
    assert_eq!(v["system"]["environment"], "Renderer: Vulkan Test GPU (driver 1.2.3)", "{v}");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_native_crash_is_reported_with_the_module_the_step_and_the_minidump() {
    let d = fresh_dir("native");
    let (_, ok) = run_child(&d, "native");
    assert!(!ok, "the child aborted");
    let (_, ok) = run_child(&d, "none");
    assert!(ok);
    let (s, v) = only_report(&d);
    assert_eq!(s.kind, "unclean_exit");
    assert!(s.message.contains("EXCEPTION_ACCESS_VIOLATION (0xC0000005) in igvk64.dll+0x64EA72 during startup (device creation"), "{}", s.message);
    assert_eq!(v["location"], "native exception in igvk64.dll+0x64EA72");
    assert_eq!(v["native"]["thread"], 4242);
    let bt: Vec<&str> = v["backtrace"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(bt[0], "0xC0000005 EXCEPTION_ACCESS_VIOLATION (read of 0x0000000000000010)");
    assert!(bt[1].contains("igvk64.dll+0x64EA72"), "{bt:?}");
    assert_eq!(fs::read(v["minidump_path"].as_str().unwrap()).unwrap(), b"MDMP fake");
    assert_eq!(v["files"]["minidump"], "minidump.dmp");
    // Nothing is left behind next to the journals.
    let left: Vec<_> = fs::read_dir(d.join("sessions")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.iter().all(|n| n.to_string_lossy().ends_with(".jsonl")), "{left:?}");
    // The same crash again is counted, not written again.
    let _ = run_child(&d, "native");
    let _ = run_child(&d, "none");
    let (s, _) = only_report(&d);
    assert_eq!(s.count, 2);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_crash_signal_is_reported_with_its_thread_and_its_walk_up_the_stack() {
    let d = fresh_dir("signal");
    let (_, ok) = run_child(&d, "signal");
    assert!(!ok, "the child aborted");
    let (_, ok) = run_child(&d, "none");
    assert!(ok);
    let (s, v) = only_report(&d);
    assert_eq!(s.kind, "unclean_exit");
    assert!(s.message.contains("segmentation fault (SIGSEGV) in libAGXMetalG13X.dylib+0x1A2B3C during startup (device creation"), "{}", s.message);
    assert_eq!(v["native"]["signal"], "SIGSEGV");
    let bt: Vec<&str> = v["backtrace"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(bt[0], "SIGSEGV segmentation fault (read of 0x0000000000000010 (SEGV_MAPERR))");
    assert_eq!(bt[2], "on thread 4242 (main)");
    assert_eq!(bt[3], "libAGXMetalG13X.dylib+0x1A2B3C");
    // Frames that aren't text are dropped, long ones cut, and only so many kept.
    assert_eq!(bt[4], format!("{}… (5000 bytes)", "x".repeat(300)));
    assert_eq!(bt[5], "solidcraft+0x0 frame0");
    assert_eq!(bt.len(), 3 + MAX_FRAMES, "{bt:?}");
    assert!(v.get("minidump_path").is_none(), "{v}");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn startup_steps_survive_a_rebase_and_running_ends_startup() {
    let d = fresh_dir("rebase");
    fs::create_dir_all(d.join("sessions")).unwrap();
    let mut j = Journal::open(&d, &build(), now());
    j.phase = Some(json!({"phase": "first frame", "ms": 5}).to_string());
    j.environment = Some(json!({"environment": "Renderer: X"}).to_string());
    j.rebase(&json!("new"), 1);
    // Read through the journal's own handle, as `read_live_journal` does: Windows refuses a
    // second handle on a live journal's locked range (#1448).
    let r = read_journal_file(j.file.as_ref().unwrap().try_clone().unwrap());
    assert_eq!(r.phase, Some(("first frame".to_string(), 5)));
    assert_eq!(r.environment.as_deref(), Some("Renderer: X"));
    j.write(&json!({"phase": RUNNING, "ms": 9}).to_string());
    let r = read_journal_file(j.file.as_ref().unwrap().try_clone().unwrap());
    assert_eq!(r.phase, Some((RUNNING.to_string(), 9)));
    assert!(from_journal(r).fields.get("startup_phase").is_none(), "a running app is past startup");
    drop(j);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_journal_reads_back_its_base_commands_and_the_one_in_flight() {
    let d = fresh_dir("journal");
    fs::create_dir_all(&d).unwrap();
    let p = d.join("j.jsonl");
    let lines = [
        r#"{"start":"2026-10-12T10:00:00Z","version":"1.0"}"#,
        r#"{"base":"new"}"#,
        r#"{"run":"solid.box","params":{"length":1}}"#,
        r#"{"ok":true}"#,
        r#"{"base":{"autosave":"/tmp/a.solidcraft"}}"#,
        r#"{"run":"solid.fillet","params":{"radius":1}}"#,
        r#"{"ok":false,"error":"no edges"}"#,
        "not json",
        r#"{"run":"solid.shell","params":{"thickness":1}}"#,
        r#"{"ok":true}"#,
        r#"{"run":"solid.extrude","params_omitted":true}"#,
    ];
    fs::write(&p, lines.join("\n")).unwrap();
    let j = read_journal(&p);
    assert_eq!(j.header["version"], "1.0");
    assert_eq!(j.base, json!({"autosave": "/tmp/a.solidcraft"}));
    assert_eq!(j.replay, vec![json!({"command": "solid.shell", "params": {"thickness": 1}})], "failed commands change nothing");
    assert_eq!(j.recent.len(), 2);
    assert_eq!(j.recent[0]["error"], "no edges");
    assert_eq!(j.in_flight.unwrap()["command"], "solid.extrude");
    assert!(!j.complete, "omitted parameters make the replay incomplete");
    assert!(!j.reported);
    let _ = fs::remove_dir_all(&d);
}

fn fake_report(dir: &Path, secs: u64, sig: u64, kind: &str) -> String {
    let id = format!("{}-{sig:016x}", stamp(secs));
    fs::create_dir_all(dir.join(&id)).unwrap();
    let f = dir.join(&id).join("report.json");
    fs::write(&f, json!({"format": FORMAT, "id": id, "kind": kind, "message": "m", "count": 1, "first_seen": utc(secs)}).to_string()).unwrap();
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    File::options().write(true).open(&f).unwrap().set_modified(t).unwrap();
    id
}

#[test]
fn reports_stay_within_their_limits() {
    let d = fresh_dir("limits");
    let t0 = 1_790_000_000;
    let old = fake_report(&d, t0 - (MAX_AGE_DAYS + 1) * 86_400, 1, "panic");
    let recovered = fake_report(&d, t0 - 100, 2, "kernel_panic_recovered");
    let mut ids = Vec::new();
    for i in 0..MAX_REPORTS as u64 {
        ids.push(fake_report(&d, t0 - 50 + i, 100 + i, "panic"));
    }
    prune(&d, t0);
    let left: Vec<String> = list(&d).into_iter().map(|s| s.id).collect();
    assert_eq!(left.len(), MAX_REPORTS);
    assert!(!left.contains(&old), "expired");
    assert!(!left.contains(&recovered), "recovered kernel panics go before real crashes");
    // One more: the oldest goes.
    let newest = fake_report(&d, t0 + 10, 999, "panic");
    prune(&d, t0 + 10);
    let left: Vec<String> = list(&d).into_iter().map(|s| s.id).collect();
    assert_eq!(left.len(), MAX_REPORTS);
    assert_eq!(left.first(), Some(&newest), "newest first");
    assert!(!left.contains(&ids[0]));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn hostile_crash_folders_never_panic() {
    let d = fresh_dir("hostile");
    fs::create_dir_all(d.join("sessions")).unwrap();
    let good = fake_report(&d, 1_790_000_000, 7, "panic");
    // Junk: a report that is not JSON, one that is an array, stray files and folders.
    let broken = format!("{}-{:016x}", stamp(1_790_000_001), 8u64);
    fs::create_dir_all(d.join(&broken)).unwrap();
    fs::write(d.join(&broken).join("report.json"), b"\xff\xfe{{{").unwrap();
    let array = format!("{}-{:016x}", stamp(1_790_000_002), 9u64);
    fs::create_dir_all(d.join(&array)).unwrap();
    fs::write(d.join(&array).join("report.json"), "[1,2]").unwrap();
    fs::create_dir_all(d.join("..evil")).unwrap();
    fs::write(d.join("notes.txt"), "x").unwrap();
    fs::write(d.join("sessions").join("junk.jsonl"), b"\x00\x01garbage\n{\"run\":5}\n{\"ok\":\"yes\"}").unwrap();
    let all = list(&d);
    assert_eq!(all.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), [good.as_str()]);
    for id in ["", "../..", "..", "a/b", &"x".repeat(500), &broken] {
        assert!(read(&d, id).is_err(), "{id}");
        assert_eq!(clear(&d, Some(id)), usize::from(id == broken), "{id}");
    }
    let j = read_journal(&d.join("sessions").join("junk.jsonl"));
    assert!(j.replay.is_empty());
    assert!(read_journal(&d.join("missing.jsonl")).replay.is_empty());
    for (loc, msg) in [("", ""), ("x", &"é".repeat(5000))] {
        let _ = signature(loc, msg);
        let _ = limited(msg, 7);
    }
    assert!(frames("garbage\n  at nothing\n 99999999999999999999: x").len() <= MAX_FRAMES);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn signatures_ignore_numbers_but_not_places() {
    assert_eq!(signature("a.rs:1", "face 12 of 40"), signature("a.rs:1", "face 7 of 3"));
    assert_ne!(signature("a.rs:1", "face 12"), signature("a.rs:2", "face 12"));
    assert_ne!(signature("a.rs:1", "edge"), signature("a.rs:1", "face"));
}

#[test]
fn dates_are_formatted_in_utc() {
    assert_eq!(utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(utc(1_791_656_982), "2026-10-10T18:29:42Z");
    assert_eq!(stamp(1_791_656_982), "20261010-182942");
}

#[test]
fn backtraces_lose_the_panic_machinery_and_the_home_folder() {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| "/home/u".into());
    let bt = format!(
        "   0: std::backtrace::Backtrace::force_capture\n             at /rustc/abc123/library/std/src/backtrace.rs:312:13\n   1: hook\n   1: {{closure#3}}\n   1: <alloc::boxed::Box<F,A> as core::ops::function::Fn<Args>>::call\n   2: std::panicking::rust_panic_with_hook\n   3: core::panicking::panic_fmt\n   4: solidcraft_kernel::fillet::blend\n             at {home}/src/kernel/fillet.rs:10:5\n   5: do_call<F>\n             at /rustc/abc/library/std/src/panicking.rs:1:1\n   5: std::sys::backtrace::__rust_begin_short_backtrace\n   6: main"
    );
    assert_eq!(frames(&bt), ["solidcraft_kernel::fillet::blend (~/src/kernel/fillet.rs:10:5)"]);
}

#[test]
fn without_install_nothing_is_recorded() {
    // Tests, the CLI and the web build never install the hook: commands cost nothing extra.
    let mut s = Session { crash_journal: true, ..Default::default() };
    s.execute("solid.box", &json!({"length": 1, "width": 1, "height": 1})).unwrap();
    assert!(!enabled());
    assert!(PENDING.with(|p| p.borrow().is_empty()));
}
