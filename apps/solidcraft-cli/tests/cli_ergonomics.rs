//! CLI ergonomics and flag safety tests for `mcp`, `snapshot`, `run`, and `exec`.

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("solidcraft-cli-ergonomics-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_solidcraft-cli"))
}

fn json_output(output: Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn stl_triangles(path: &Path) -> u32 {
    let bytes = std::fs::read(path).unwrap();
    u32::from_le_bytes(bytes[80..84].try_into().unwrap())
}

fn parametric_design(scratch: &Scratch) -> PathBuf {
    let script = scratch.0.join("parametric.json");
    std::fs::write(
        &script,
        json!({"commands": [
            {"command": "parameters.add", "params": {"name": "width", "expression": "20 mm"}},
            {"command": "parameters.add", "params": {"name": "depth", "expression": "10 mm"}},
            {"command": "solid.box", "params": {"length": "width", "width": "depth", "height": 5}}
        ]})
        .to_string(),
    )
    .unwrap();
    let design = scratch.0.join("parametric.sc");
    let saved = cli().arg("run").arg(&script).arg("--save").arg(&design).arg("--quiet").output().unwrap();
    assert!(saved.status.success(), "{}", String::from_utf8_lossy(&saved.stderr));
    design
}

fn run_with_stdin(args: &[&str], input: &str) -> Output {
    let mut child = cli().args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    child.wait_with_output().unwrap()
}

#[test]
fn mcp_rejects_unknown_flags_and_missing_values() {
    let cases: [(&[&str], &str); 6] = [
        (&["--conect", "127.0.0.1:1234"], "mcp doesn't take --conect"),
        (&["--compcat"], "mcp doesn't take --compcat"),
        (&["--allow-paths", "/tmp"], "mcp doesn't take --allow-paths"),
        (&["stray-arg"], "mcp doesn't take stray-arg"),
        (&["--connect"], "mcp: --connect needs a value"),
        (&["--in"], "mcp: --in needs a value"),
    ];
    for (extra, expected) in cases {
        let output = cli().arg("mcp").args(extra).stdin(Stdio::null()).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{extra:?}");
        assert!(output.stdout.is_empty(), "{extra:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{extra:?}: {stderr}");
    }
    let structured = cli().args(["mcp", "--headless", "--structured"]).stdin(Stdio::null()).output().unwrap();
    assert!(structured.status.success(), "{}", String::from_utf8_lossy(&structured.stderr));
}

#[test]
fn mcp_render_gpu_bridges_to_auto_discovered_running_app_with_note() {
    let dir = Scratch::new("mcp-render-gpu");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::fs::write(dir.0.join("current.port"), format!("{port}\n")).unwrap();
    let output = cli().args(["mcp", "--render-gpu"]).env("SOLIDCRAFT_CONTROL_DIR", &dir.0).stdin(Stdio::null()).output().unwrap();
    drop(listener);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("bridged to the running app"), "{stderr}");
    assert!(stderr.contains("--render-gpu is ignored while bridged"), "{stderr}");

    // Explicit --connect + --render-gpu is still refused.
    let explicit = cli().args(["mcp", "--connect", "127.0.0.1:1234", "--render-gpu"]).stdin(Stdio::null()).output().unwrap();
    assert_eq!(explicit.status.code(), Some(1));
    let explicit_stderr = String::from_utf8_lossy(&explicit.stderr);
    assert!(explicit_stderr.contains("connected application owns its own device"), "{explicit_stderr}");
}

#[test]
fn snapshot_accepts_param_and_params_file_overrides() {
    let dir = Scratch::new("snapshot-params");
    let design = parametric_design(&dir);
    let before = std::fs::read(&design).unwrap();
    let cam = json!({"target": [20, 5, 2.5], "yaw": 0.6, "pitch": 0.25, "distance": 90}).to_string();
    let snap = |out: &str, extra: &[&str]| {
        let path = dir.0.join(out);
        let output = cli()
            .env("SOLIDCRAFT_GPU", "cpu")
            .arg("snapshot")
            .arg(&design)
            .arg("--out")
            .arg(&path)
            .args(["--width", "64", "--height", "48", "--camera", &cam, "--no-grid", "--no-axes"])
            .args(extra)
            .output()
            .unwrap();
        assert!(output.status.success(), "{extra:?}: {}", String::from_utf8_lossy(&output.stderr));
        let png = std::fs::read(&path).unwrap();
        assert!(png.starts_with(b"\x89PNG"), "{extra:?}");
        png
    };

    let default_png = snap("default.png", &[]);
    let wider_png = snap("wider.png", &["--param", "width=40mm"]);
    assert_ne!(wider_png, default_png, "--param must change the rendered geometry");

    let params_file = dir.0.join("params.json");
    std::fs::write(&params_file, json!({"width": "40 mm"}).to_string()).unwrap();
    let from_file_png = snap("from-file.png", &["--params-file", params_file.to_str().unwrap()]);
    assert_eq!(from_file_png, wider_png, "--params-file must match the equivalent --param");

    let overridden_back = snap("overridden-back.png", &["--params-file", params_file.to_str().unwrap(), "--param", "width=20mm"]);
    assert_eq!(overridden_back, default_png, "--param must override --params-file");
    assert_eq!(std::fs::read(&design).unwrap(), before, "snapshot must not modify the input design");

    // Invalid overrides fail without writing a PNG.
    let never = dir.0.join("never.png");
    for (extra, expected) in [
        (&["--param", "no_such_param=5mm"][..], "no_such_param"),
        (&["--param", "width"][..], "--param wants NAME=EXPR"),
        (&["--params-file", params_file.to_str().unwrap(), "--params-file", params_file.to_str().unwrap()][..], "more than once"),
    ] {
        let output = cli().env("SOLIDCRAFT_GPU", "cpu").arg("snapshot").arg(&design).arg("--out").arg(&never).args(extra).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{extra:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{extra:?}: {stderr}");
        assert!(!never.exists(), "{extra:?} wrote a file");
    }

    let help = cli().args(["snapshot", "--help"]).output().unwrap();
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("--param") && stdout.contains("--params-file"), "{stdout}");
}

#[test]
fn run_chord_controls_mesh_export_and_rejects_invalid_values() {
    let dir = Scratch::new("run-chord");
    let script = dir.0.join("cylinder.json");
    std::fs::write(&script, json!({"commands": [{"command": "solid.cylinder", "params": {"diameter": 40, "height": 10}}]}).to_string()).unwrap();

    let export = |name: &str, chord: &str| {
        let stl = dir.0.join(name);
        let output = cli().arg("run").arg(&script).args(["--quiet", "--out"]).arg(&stl).args(["--chord", chord]).output().unwrap();
        assert!(output.status.success(), "{chord}: {}", String::from_utf8_lossy(&output.stderr));
        stl_triangles(&stl)
    };
    let coarse = export("coarse.stl", "0.5");
    let fine = export("fine.stl", "0.01");
    assert!(coarse > 0 && coarse < fine, "coarse {coarse}, fine {fine}");

    let never = dir.0.join("never.stl");
    let cases: [(&[&str], &str); 7] = [
        (&["--chord"], "--chord needs a value"),
        (&["--chord", "0.0001"], "--chord must be"),
        (&["--chord", "1001"], "--chord must be"),
        (&["--chord", "-1"], "--chord must be"),
        (&["--chord", "fine"], "--chord must be"),
        (&["--chord", "0.1", "--chord", "0.2"], "--chord is given more than once"),
        (&["--chord", "0.1", "--quality", "fine"], "not both"),
    ];
    for (extra, expected) in cases {
        let output = cli().arg("run").arg(&script).arg("--out").arg(&never).args(extra).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{extra:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{extra:?}: {stderr}");
        assert!(!never.exists(), "{extra:?} wrote a file");
    }
}

#[test]
fn run_reads_script_from_stdin_dash() {
    let dir = Scratch::new("run-stdin");
    let box_script = json!({"commands": [{"command": "solid.box", "params": {"length": 2, "width": 3, "height": 4}}]}).to_string();

    let measured = json_output(run_with_stdin(&["run", "-"], &box_script));
    assert_eq!(measured["body_count"], 1, "{measured}");
    assert!((measured["total"]["volume_mm3"].as_f64().unwrap() - 24.0).abs() < 1e-6, "{measured}");

    let results = json_output(run_with_stdin(&["run", "-", "--results"], &box_script));
    assert_eq!(results.as_array().map(Vec::len), Some(1), "{results}");

    let design = parametric_design(&dir);
    let add_script =
        json!({"commands": [{"command": "solid.box", "params": {"length": 2, "width": 3, "height": 4, "corner": [100, 0, 0]}}]}).to_string();
    let on_base = json_output(run_with_stdin(&["run", "-", "--in", design.to_str().unwrap(), "--param", "width=30mm"], &add_script));
    assert_eq!(on_base["body_count"], 2, "{on_base}");
    assert!((on_base["total"]["volume_mm3"].as_f64().unwrap() - 1524.0).abs() < 1e-6, "{on_base}");

    let stl = dir.0.join("stdin.stl");
    let exported = run_with_stdin(&["run", "-", "--quiet", "--out", stl.to_str().unwrap(), "--chord", "0.1"], &box_script);
    assert!(exported.status.success(), "{}", String::from_utf8_lossy(&exported.stderr));
    assert!(std::fs::metadata(&stl).unwrap().len() > 0);

    let bad_json = run_with_stdin(&["run", "-"], "{");
    assert_eq!(bad_json.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad_json.stderr).contains("stdin:"), "{}", String::from_utf8_lossy(&bad_json.stderr));

    let rel = run_with_stdin(&["run", "-", "--script-relative"], &box_script);
    assert_eq!(rel.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rel.stderr).contains("--script-relative"), "{}", String::from_utf8_lossy(&rel.stderr));

    let no_design = run_with_stdin(&["run", "-", "--param", "width=30mm"], &box_script);
    assert_eq!(no_design.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&no_design.stderr).contains("--param"), "{}", String::from_utf8_lossy(&no_design.stderr));
}

#[test]
fn exec_supports_optional_in_and_save() {
    let dir = Scratch::new("exec-in-save");
    let base = dir.0.join("base.sc");
    let two = dir.0.join("two.sc");

    let created =
        json_output(cli().args(["exec", "--save"]).arg(&base).args(["solid.box", r#"{"length":2,"width":3,"height":4}"#]).output().unwrap());
    assert!(created.is_object(), "{created}");
    assert!(base.is_file());

    let measured = json_output(cli().args(["exec", "--in"]).arg(&base).arg("inspect.measure").output().unwrap());
    assert_eq!(measured["body_count"], 1, "{measured}");
    assert!((measured["total"]["volume_mm3"].as_f64().unwrap() - 24.0).abs() < 1e-6, "{measured}");

    let added = json_output(
        cli()
            .args(["exec", "--in"])
            .arg(&base)
            .arg("--save")
            .arg(&two)
            .args(["solid.box", r#"{"length":5,"width":3,"height":4,"corner":[10,0,0]}"#])
            .output()
            .unwrap(),
    );
    assert!(added.is_object(), "{added}");
    assert!(two.is_file());

    let measured_two = json_output(cli().args(["exec", "inspect.measure", "--in"]).arg(&two).output().unwrap());
    assert_eq!(measured_two["body_count"], 2, "{measured_two}");
    assert!((measured_two["total"]["volume_mm3"].as_f64().unwrap() - 84.0).abs() < 1e-6, "{measured_two}");

    let measured_base = json_output(cli().args(["exec", "--in"]).arg(&base).arg("inspect.measure").output().unwrap());
    assert_eq!(measured_base["body_count"], 1, "original design must remain untouched");

    let never = dir.0.join("never.sc");
    let bad = cli().args(["exec", "--in"]).arg(&base).arg("--save").arg(&never).args(["solid.box", "{bad"]).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    assert!(!never.exists());

    let unknown = cli().args(["exec", "--unknown", "file.new"]).output().unwrap();
    assert_eq!(unknown.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("exec doesn't take --unknown"));
}
