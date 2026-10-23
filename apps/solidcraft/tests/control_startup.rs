//! Real desktop regression for #1789's control-channel startup delay.
#![cfg(target_os = "windows")]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Desktop(Child);
impl Drop for Desktop {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

struct Marker {
    path: PathBuf,
    previous: Option<Vec<u8>>,
}
impl Drop for Marker {
    fn drop(&mut self) {
        let result = if let Some(bytes) = &self.previous {
            std::fs::write(&self.path, bytes)
        } else if self.path.exists() {
            std::fs::remove_file(&self.path)
        } else {
            Ok(())
        };
        if let Err(error) = result {
            eprintln!("could not restore startup marker {}: {error}", self.path.display());
        }
    }
}

/// Requires an unattended interactive desktop. Safe mode does not persist preferences;
/// preserve the real Windows startup marker, which eframe's settings directory uses.
#[test]
#[ignore = "launches the native app; run explicitly on an unattended Windows desktop"]
fn control_answers_before_the_startup_marker_settles() {
    use std::os::windows::process::CommandExt;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/issue-1789-control-startup").join(format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let path = eframe::storage_dir("SolidCraft").unwrap().join("starting");
    let marker = Marker { previous: std::fs::read(&path).ok(), path };
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reservation.local_addr().unwrap();
    drop(reservation);
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).unwrap();
    let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let launched = Instant::now();
    let mut app = Desktop(
        Command::new(std::env::var_os("SOLIDCRAFT_STARTUP_TEST_EXE").unwrap_or_else(|| env!("CARGO_BIN_EXE_solidcraft").into()))
            .args(["--safe-mode", "--sample", "--control", &addr.port().to_string()])
            .env("SOLIDCRAFT_CONTROL_TOKEN", &token)
            .env("SOLIDCRAFT_CRASH_DIR", root.join("crashes"))
            .env("SOLIDCRAFT_RECOVERY_DIR", root.join("recovery"))
            .env("LOCALAPPDATA", root.join("local"))
            .env_remove("WGPU_BACKEND")
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(root.join("stderr.txt")).unwrap())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap(),
    );
    // Attempt connections immediately; do not sleep for the ten-second recovery window
    // or send UI input. Shader/device setup can still take time on a loaded machine.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut socket = loop {
        assert!(app.0.try_wait().unwrap().is_none(), "app exited; see {}", root.display());
        if let Ok(socket) = TcpStream::connect(addr) {
            break socket;
        }
        assert!(Instant::now() < deadline, "control listener never opened; see {}", root.display());
        std::thread::sleep(Duration::from_millis(20));
    };
    socket.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut replies = BufReader::new(socket.try_clone().unwrap());
    let mut ask = |request: serde_json::Value| {
        writeln!(socket, "{request}").unwrap();
        let mut reply = String::new();
        replies.read_line(&mut reply).unwrap();
        let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
    };
    ask(serde_json::json!({"id":1,"token":token,"method":"ui.inspect"}));
    assert!(marker.path.exists(), "control answered only after the recovery window settled");
    println!("control replied after {:?} while startup was unsettled", launched.elapsed());
    ask(serde_json::json!({"id":2,"method":"ui.screenshot","params":{"path":root.join("startup.png")}}));
    ask(serde_json::json!({"id":3,"method":"app.quit","params":{"force":true}}));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = app.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "app did not quit");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!marker.path.exists(), "a clean early quit must clear the startup marker");
    println!("control replied while startup was unsettled; screenshot: {}", root.join("startup.png").display());
}
