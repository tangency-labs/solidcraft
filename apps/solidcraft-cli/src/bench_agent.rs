//! `bench-agent`: a large design built the way an agent builds one over MCP, one small command
//! at a time, timed as it grows and then opened again.
//!
//! The design is a bicycle of about 1,300 bodies: frame tubes joined at root and then moved into
//! a component, two wheels whose spokes and nipples are placed one by one, a chainring with
//! patterned teeth, a cassette, a chain of individual links, cockpit, seat and a few hundred
//! fasteners. Nothing is read from a file; the script is generated here, so the benchmark needs
//! no fixture.
//!
//! ```text
//! solidcraft-cli bench-agent [--scale F] [--script-out script.json] [--save design.sc]
//! ```
//! prints one JSON line per hundred commands (mean and max milliseconds, bodies so far), then the
//! build total, then the phases `bench-open` prints for the saved design.

use serde_json::{Value, json};
use solidcraft_engine::Session;

struct Script {
    commands: Vec<Value>,
}

impl Script {
    fn c(&mut self, command: &str, params: Value) {
        self.commands.push(json!({"command": command, "params": params}));
    }

    /// A round tube from `a` to `b` (an agent's frame tube, spoke or post).
    fn tube(&mut self, a: [f64; 3], b: [f64; 3], radius: f64, body: &str) {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let axis = [d[0] / len, d[1] / len, d[2] / len];
        self.c(
            "solid.cylinder",
            json!({"radius": radius, "height": round3(len), "base": a.map(round3), "axis": axis.map(round6), "body_name": body}),
        );
    }

    /// A part placed after it is built: free, even as the design's first part (#2804).
    fn component(&mut self, name: &str) {
        self.c("component.create", json!({"name": name, "grounded": false}));
    }

    fn root(&mut self) {
        self.c("component.activate", json!({"component": "root"}));
    }

    /// A wheel built at the origin in its own component, then placed where it goes (the
    /// component's occurrence moved), as an agent builds one part and positions it.
    fn wheel(&mut self, label: &str, at: f64, spokes: usize) {
        let cx = 0.0;
        self.component(&format!("{label} wheel"));
        self.c("solid.torus", json!({"major": 311, "minor": 12, "center": [cx, 0, 340], "body_name": format!("{label} rim")}));
        self.c("solid.torus", json!({"major": 330, "minor": 18, "center": [cx, 0, 340], "body_name": format!("{label} tyre")}));
        self.c("solid.cylinder", json!({"radius": 18, "height": 110, "base": [cx, -55, 340], "axis": "Y", "body_name": format!("{label} hub")}));
        for k in 0..spokes {
            let a = std::f64::consts::TAU * k as f64 / spokes as f64;
            let side = if k % 2 == 1 { 1.0 } else { -1.0 };
            let hub = [cx + 30.0 * a.cos(), side * 40.0, 340.0 + 30.0 * a.sin()];
            let (ca, sa) = ((a + 0.12).cos(), (a + 0.12).sin());
            let rim = [cx + 300.0 * ca, side * 4.0, 340.0 + 300.0 * sa];
            self.tube(hub, rim, 1.0, &format!("{label} spoke {k}"));
            let nipple = json!({"radius": 2, "height": 12, "base": rim.map(round3), "axis": [round6(ca), 0, round6(sa)], "body_name": format!("{label} nipple {k}")});
            self.c("solid.cylinder", nipple);
        }
        self.root();
        self.c("occurrence.move", json!({"occurrence": format!("{label} wheel:1"), "translate": [at, 0, 0]}));
    }
}

fn round3(v: f64) -> f64 {
    (v * 1e3).round() / 1e3
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// The bicycle's command script. `scale` multiplies the counts of repeated parts (spokes,
/// chain links, fasteners); 1.0 gives about 1,300 bodies.
pub fn bicycle(scale: f64) -> Value {
    let n = |k: f64| ((k * scale).round() as usize).max(1);
    let mut s = Script { commands: Vec::new() };
    // Frame: tubes at root, the main triangle joined, then everything moved into its component.
    let (bb, head_top, head_bot, seat_top) = ([0.0, 0.0, 300.0], [600.0, 0.0, 860.0], [580.0, 0.0, 740.0], [-150.0, 0.0, 820.0]);
    s.tube(bb, head_bot, 19.0, "Down tube");
    s.tube(seat_top, head_top, 16.0, "Top tube");
    s.tube(bb, seat_top, 16.0, "Seat tube");
    s.tube(head_bot, head_top, 22.0, "Head tube");
    for (side, y) in [("L", 60.0), ("R", -60.0)] {
        s.tube([0.0, y * 0.3, 300.0], [-420.0, y, 340.0], 10.0, &format!("Chainstay {side}"));
        s.tube([-140.0, y * 0.3, 800.0], [-420.0, y, 340.0], 9.0, &format!("Seatstay {side}"));
        s.c("solid.box", json!({"length": 20, "width": 6, "height": 40, "corner": [-430.0, y - 3.0, 320], "body_name": format!("Dropout {side}")}));
    }
    s.c("solid.cylinder", json!({"radius": 24, "height": 80, "base": [0, -40, 300], "axis": "Y", "body_name": "BB shell"}));
    s.c("solid.combine", json!({"target": "Down tube", "tools": ["Top tube", "Seat tube", "Head tube", "BB shell"]}));
    s.component("Frame");
    s.root();
    let frame = ["Down tube", "Chainstay L", "Seatstay L", "Dropout L", "Chainstay R", "Seatstay R", "Dropout R"];
    s.c("component.move_bodies", json!({"bodies": frame, "component": "Frame"}));
    // Wheels: every spoke and nipple a body of its own.
    s.wheel("Front", 640.0, n(36.0));
    s.wheel("Rear", -420.0, n(36.0));
    // Drivetrain: patterned chainring teeth, a cassette and a chain of separate links.
    s.component("Drivetrain");
    s.c("solid.cylinder", json!({"radius": 95, "height": 4, "base": [0, 50, 300], "axis": "Y", "body_name": "Chainring"}));
    s.c("solid.box", json!({"length": 6, "width": 4, "height": 8, "corner": [-3, 50, 395], "name": "Tooth", "body_name": "Tooth"}));
    s.c("solid.pattern.circular", json!({"bodies": ["Tooth"], "axis": {"origin": [0, 50, 300], "dir": [0, 1, 0]}, "count": 44}));
    for g in 0..11 {
        let r = 22.0 + 3.5 * g as f64;
        s.c(
            "solid.cylinder",
            json!({"radius": r, "height": 2, "base": [-420.0, round3(30.0 + 2.6 * g as f64), 340.0], "axis": "Y", "body_name": format!("Cog {g}")}),
        );
    }
    let links = n(112.0);
    for k in 0..links {
        let a = std::f64::consts::TAU * k as f64 / links as f64;
        let (x, z) = (round3(-210.0 + 230.0 * a.cos()), round3(320.0 + 80.0 * a.sin()));
        for (side, y) in [("in", 46), ("out", 56)] {
            s.c("solid.box", json!({"length": 12, "width": 1, "height": 8, "corner": [round3(x - 6.0), y, round3(z - 4.0)], "body_name": format!("Link {k} {side}")}));
        }
        s.c("solid.cylinder", json!({"radius": 3.5, "height": 10, "base": [x, 46, z], "axis": "Y", "body_name": format!("Roller {k}")}));
    }
    for (side, y) in [("L", -90.0), ("R", 90.0)] {
        let x0 = if side == "L" { -15.0 } else { 0.0 };
        s.c("solid.box", json!({"length": 170, "width": 12, "height": 30, "corner": [x0, y - 6.0, 285], "body_name": format!("Crank {side}")}));
        let py = if y > 0.0 { y + 8.0 } else { y - 68.0 };
        s.c("solid.box", json!({"length": 90, "width": 60, "height": 15, "corner": [150.0, py, 280], "body_name": format!("Pedal {side}")}));
    }
    s.root();
    // Cockpit and seat.
    s.component("Cockpit");
    s.tube([600.0, 0.0, 860.0], [620.0, 0.0, 940.0], 14.0, "Steerer");
    s.tube([620.0, 0.0, 940.0], [700.0, 0.0, 950.0], 16.0, "Stem");
    s.tube([700.0, -380.0, 950.0], [700.0, 380.0, 950.0], 11.0, "Handlebar");
    s.c("solid.cylinder", json!({"radius": 16, "height": 120, "base": [700, 260, 950], "axis": "Y", "body_name": "Grip R"}));
    s.c("solid.mirror", json!({"bodies": ["Grip R"], "plane": "XZ"}));
    s.root();
    s.component("Seat");
    s.tube([-150.0, 0.0, 820.0], [-170.0, 0.0, 960.0], 13.0, "Seatpost");
    s.c("solid.box", json!({"length": 260, "width": 140, "height": 40, "center": [-180, 0, 980], "body_name": "Saddle"}));
    s.root();
    // Fasteners: head, shank and washer, each its own body.
    s.component("Hardware");
    let bolts = n(240.0);
    for k in 0..bolts {
        let a = std::f64::consts::TAU * k as f64 / bolts as f64;
        let p = [round3(-300.0 + 2.5 * k as f64), round3(120.0 + 10.0 * a.sin()), round3(400.0 + 10.0 * a.cos())];
        s.c("solid.cylinder", json!({"radius": 5, "height": 4, "base": p, "axis": "Z", "body_name": format!("Bolt {k} head")}));
        s.c(
            "solid.cylinder",
            json!({"radius": 2.5, "height": 16, "base": [p[0], p[1], round3(p[2] - 16.0)], "axis": "Z", "body_name": format!("Bolt {k} shank")}),
        );
        s.c(
            "solid.cylinder",
            json!({"radius": 4.5, "height": 1, "base": [p[0], p[1], round3(p[2] - 1.0)], "axis": "Z", "body_name": format!("Bolt {k} washer")}),
        );
    }
    s.root();
    json!({"commands": s.commands})
}

pub fn run(args: &[String]) -> Result<(), String> {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
    let scale = match flag("--scale") {
        Some(v) => v.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0 && *v <= 20.0).ok_or("--scale takes a number from 0 to 20")?,
        None => 1.0,
    };
    let script = bicycle(scale);
    if let Some(path) = flag("--script-out") {
        let text = serde_json::to_string_pretty(&script).map_err(|e| e.to_string())?;
        return solidcraft_engine::io::vfs::write(path, text.as_bytes()).map_err(|e| format!("{path}: {e}"));
    }
    let commands = script["commands"].as_array().cloned().unwrap_or_default();
    let mut s = Session::default();
    let began = std::time::Instant::now();
    // Per command: the command itself, then what a window needs to draw its result (the
    // bodies placed in the world and their display meshes).
    let (mut run, mut shown): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    for (i, c) in commands.iter().enumerate() {
        let id = c["command"].as_str().unwrap_or_default();
        let t = std::time::Instant::now();
        s.execute(id, &c["params"]).map_err(|e| format!("command {i} ({id}): {e}"))?;
        run.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = std::time::Instant::now();
        s.world_state().map_err(|e| e.to_string())?.warm_meshes(|| true);
        shown.push(t.elapsed().as_secs_f64() * 1000.0);
        if run.len() == 100 || i + 1 == commands.len() {
            let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
            let bodies = s.model.state().bodies.len();
            println!(
                "{}",
                json!({"step": "build", "commands": i + 1, "bodies": bodies, "command_ms": mean(&run), "display_ms": mean(&shown),
                       "max_ms": run.iter().zip(&shown).map(|(a, b)| a + b).fold(0.0, f64::max)})
            );
            run.clear();
            shown.clear();
        }
    }
    let world = s.world_state().map_err(|e| e.to_string())?;
    println!(
        "{}",
        json!({"step": "built", "ms": began.elapsed().as_secs_f64() * 1000.0, "commands": commands.len(), "features": s.doc.features.len(), "bodies": world.bodies.len()})
    );
    let path = match flag("--save") {
        Some(p) => p.clone(),
        None => std::env::temp_dir()
            .join(format!("bench-agent-{}.{}", std::process::id(), solidcraft_engine::io::DESIGN_EXT))
            .to_string_lossy()
            .into_owned(),
    };
    s.execute("file.save", &json!({"path": path})).map_err(|e| e.to_string())?;
    let r = crate::cmd_bench_open(std::slice::from_ref(&path));
    if flag("--save").is_none() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}.bak"));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The generated commands stay valid as the command set changes: a small bicycle builds
    /// without a failing command or feature.
    #[test]
    fn a_small_bicycle_builds() {
        let script = bicycle(0.03);
        let mut s = Session::default();
        for c in script["commands"].as_array().into_iter().flatten() {
            let id = c["command"].as_str().unwrap_or_default();
            if let Err(e) = s.execute(id, &c["params"]) {
                panic!("{id}: {e}");
            }
        }
        assert!(s.model.results.iter().all(|r| r.error.is_none()));
        let world = s.world_state().unwrap();
        assert!(world.bodies.len() > 40, "{} bodies", world.bodies.len());
        assert!(world.bodies.iter().any(|b| b.name.contains("spoke")));
    }
}
