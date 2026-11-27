//! The OCCT adapter's row of the kernel support matrix (`docs/kernel-support.md`): every case of
//! the operations it takes, built through the router on solidcore and on Open CASCADE, graded
//! against its closed-form volume and area and by the shared check.
//!
//! `cargo run --release -p solidcraft-kernel --features backend-occt --example occt_matrix`
//! (add `-- --json <file>` for the full results). Only builds with the feature have it.
//!
//! A case passes on a kernel when the kernel builds it, the shared check passes the result, and
//! its volume and area are within 1e-9 (relative) of the case's answer; a case whose answer is a
//! refusal or an empty result passes when the kernel answers so. On Open CASCADE a case is
//! `unsupported` when the router ran it on solidcore instead (the adapter doesn't take the
//! operation or the operand, or the result doesn't cross back as one record). The accuracy
//! columns compare the two kernels on the same case: the worst edge or vertex tolerance the
//! result's record carries, and the volume and area errors against the answer. A case counts as
//! looser when one of them is larger on Open CASCADE than on solidcore by more than 1e-15 (the
//! last bits of a double), and is counted again when it is also above 1e-12.

use std::f64::consts::PI;
use std::time::Instant;

use solidcore_record::Vec3;
use solidcraft_kernel::foreign::{self, Kernel};
use solidcraft_kernel::{Body, BoolOp, Result};

/// Relative error a pass allows in volume and area.
const EXACT: f64 = 1e-9;

/// What a case should give.
enum Answer {
    /// A solid of this volume and area (`None`: area not derived for this case).
    Solid { volume: f64, area: Option<f64> },
    /// Nothing left.
    Empty,
    /// A refusal.
    Refused,
}

struct Case {
    op: &'static str,
    name: &'static str,
    build: fn() -> Result<Option<Body>>,
    answer: Answer,
}

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn some(b: Result<Body>) -> Result<Option<Body>> {
    b.map(Some)
}

fn cube(lo: f64, hi: f64) -> Result<Body> {
    solidcraft_kernel::box_solid(v(lo, lo, lo), v(hi, hi, hi))
}

/// The volume and area a constant round of radius `r` takes off a convex right-angled edge of
/// length `l` whose ends are flat faces: the section r² − πr²/4 along the edge; the two strips r
/// wide traded for a quarter cylinder, and the section off each end face.
fn edge_round(l: f64, r: f64) -> (f64, f64) {
    let section = r * r * (1.0 - PI / 4.0);
    (l * section, 2.0 * l * r - l * PI * r / 2.0 + 2.0 * section)
}

/// A round of radius `r` on the top rim of a cylinder of radius `big`, by Pappus: what it takes
/// off the volume and the area.
fn rim_round(big: f64, r: f64) -> (f64, f64) {
    // The section: the square of side r at the rim less the quarter disc about (big − r, h − r).
    let square = r * r * (big - r / 2.0);
    let disc = PI * r * r / 4.0 * (big - r + 4.0 * r / (3.0 * PI));
    let volume = 2.0 * PI * (square - disc);
    // Off the top disc and the side; on, the quarter torus (arc length πr/2 about its centroid).
    let off = PI * (big * big - (big - r) * (big - r)) + 2.0 * PI * big * r;
    let on = 2.0 * PI * (big - r + 2.0 * r / PI) * (PI * r / 2.0);
    (volume, off - on)
}

fn cases() -> Vec<Case> {
    let (e1v, e1a) = edge_round(10.0, 1.0);
    let (rimv, rima) = rim_round(2.0, 0.5);
    vec![
        Case {
            op: "box",
            name: "1×2×3 at the origin",
            build: || some(solidcraft_kernel::box_solid(v(0.0, 0.0, 0.0), v(1.0, 2.0, 3.0))),
            answer: Answer::Solid { volume: 6.0, area: Some(22.0) },
        },
        Case {
            op: "box",
            name: "10 cube about the origin, corners given high to low",
            build: || some(solidcraft_kernel::box_solid(v(5.0, 5.0, 5.0), v(-5.0, -5.0, -5.0))),
            answer: Answer::Solid { volume: 1000.0, area: Some(600.0) },
        },
        Case {
            op: "box",
            name: "degenerate (flat)",
            build: || some(solidcraft_kernel::box_solid(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0))),
            answer: Answer::Refused,
        },
        Case {
            op: "cylinder",
            name: "r 2, h 3 along z",
            build: || some(solidcraft_kernel::cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 2.0, 3.0)),
            answer: Answer::Solid { volume: 12.0 * PI, area: Some(20.0 * PI) },
        },
        Case {
            op: "cylinder",
            name: "r 1.5, h 4 along (1, 1, 1)",
            build: || some(solidcraft_kernel::cylinder(v(1.0, -2.0, 0.5), v(1.0, 1.0, 1.0), 1.5, 4.0)),
            answer: Answer::Solid { volume: 9.0 * PI, area: Some(16.5 * PI) },
        },
        Case {
            op: "cylinder",
            name: "radius 0",
            build: || some(solidcraft_kernel::cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 0.0, 3.0)),
            answer: Answer::Refused,
        },
        Case {
            op: "sphere",
            name: "r 1.5 at the origin",
            build: || some(solidcraft_kernel::sphere(v(0.0, 0.0, 0.0), 1.5)),
            answer: Answer::Solid { volume: 4.5 * PI, area: Some(9.0 * PI) },
        },
        Case {
            op: "sphere",
            name: "r 100 off the origin",
            build: || some(solidcraft_kernel::sphere(v(10.0, 20.0, 30.0), 100.0)),
            answer: Answer::Solid { volume: 4.0 / 3.0 * PI * 1e6, area: Some(4.0 * PI * 1e4) },
        },
        Case {
            op: "torus",
            name: "R 3, r 1",
            build: || some(solidcraft_kernel::torus(v(0.0, 0.0, 0.0), 3.0, 1.0)),
            answer: Answer::Solid { volume: 6.0 * PI * PI, area: Some(12.0 * PI * PI) },
        },
        Case {
            op: "torus",
            name: "R 10, r 0.5 off the origin",
            build: || some(solidcraft_kernel::torus(v(1.0, 2.0, 3.0), 10.0, 0.5)),
            answer: Answer::Solid { volume: 5.0 * PI * PI, area: Some(20.0 * PI * PI) },
        },
        Case {
            op: "torus",
            name: "minor above major",
            build: || some(solidcraft_kernel::torus(v(0.0, 0.0, 0.0), 1.0, 2.0)),
            answer: Answer::Refused,
        },
        Case {
            op: "boolean",
            name: "2 cube cut by a 2 cube at its corner",
            build: || solidcraft_kernel::boolean(&cube(0.0, 2.0)?, &cube(1.0, 3.0)?, BoolOp::Cut),
            answer: Answer::Solid { volume: 7.0, area: Some(24.0) },
        },
        Case {
            op: "boolean",
            name: "2 cube with a 2 cube at its corner",
            build: || solidcraft_kernel::boolean(&cube(0.0, 2.0)?, &cube(1.0, 3.0)?, BoolOp::Union),
            answer: Answer::Solid { volume: 15.0, area: Some(42.0) },
        },
        Case {
            op: "boolean",
            name: "2 cube and a 2 cube at its corner, in common",
            build: || solidcraft_kernel::boolean(&cube(0.0, 2.0)?, &cube(1.0, 3.0)?, BoolOp::Intersect),
            answer: Answer::Solid { volume: 1.0, area: Some(6.0) },
        },
        Case {
            op: "boolean",
            name: "a through hole: 10 cube less an r 2 cylinder",
            build: || {
                solidcraft_kernel::boolean(
                    &cube(0.0, 10.0)?,
                    &solidcraft_kernel::cylinder(v(5.0, 5.0, -1.0), v(0.0, 0.0, 1.0), 2.0, 12.0)?,
                    BoolOp::Cut,
                )
            },
            answer: Answer::Solid { volume: 1000.0 - 40.0 * PI, area: Some(600.0 + 32.0 * PI) },
        },
        Case {
            op: "boolean",
            name: "the lower half of an r 2 sphere",
            build: || {
                solidcraft_kernel::boolean(
                    &solidcraft_kernel::sphere(v(0.0, 0.0, 0.0), 2.0)?,
                    &solidcraft_kernel::box_solid(v(-3.0, -3.0, 0.0), v(3.0, 3.0, 3.0))?,
                    BoolOp::Cut,
                )
            },
            answer: Answer::Solid { volume: 16.0 * PI / 3.0, area: Some(12.0 * PI) },
        },
        Case {
            op: "boolean",
            name: "two crossing r 1 cylinders, joined",
            build: || {
                let a = solidcraft_kernel::cylinder(v(-3.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 1.0, 6.0)?;
                let b = solidcraft_kernel::cylinder(v(0.0, -3.0, 0.0), v(0.0, 1.0, 0.0), 1.0, 6.0)?;
                solidcraft_kernel::boolean(&a, &b, BoolOp::Union)
            },
            // Each is 6π; they share a bicylinder of 16/3, whose 16 of surface is half in each.
            answer: Answer::Solid { volume: 12.0 * PI - 16.0 / 3.0, area: Some(28.0 * PI - 16.0) },
        },
        Case {
            op: "boolean",
            name: "a cut that removes everything",
            build: || solidcraft_kernel::boolean(&cube(0.0, 1.0)?, &cube(-1.0, 2.0)?, BoolOp::Cut),
            answer: Answer::Empty,
        },
        Case {
            op: "boolean",
            name: "apart cubes in common",
            build: || solidcraft_kernel::boolean(&cube(0.0, 1.0)?, &cube(2.0, 3.0)?, BoolOp::Intersect),
            answer: Answer::Empty,
        },
        Case {
            op: "boolean",
            name: "apart cubes joined",
            build: || solidcraft_kernel::boolean(&cube(0.0, 1.0)?, &cube(2.0, 3.0)?, BoolOp::Union),
            answer: Answer::Solid { volume: 2.0, area: Some(12.0) },
        },
        Case {
            op: "fillet",
            name: "one edge of a 10 cube, r 1",
            build: || some(solidcraft_kernel::fillet(&cube(0.0, 10.0)?, &[v(5.0, 0.0, 10.0)], 1.0)),
            answer: Answer::Solid { volume: 1000.0 - e1v, area: Some(600.0 - e1a) },
        },
        Case {
            op: "fillet",
            name: "the four top edges of a 10 cube, r 1 (mitred)",
            build: || {
                some(solidcraft_kernel::fillet(
                    &cube(0.0, 10.0)?,
                    &[v(5.0, 0.0, 10.0), v(10.0, 5.0, 10.0), v(5.0, 10.0, 10.0), v(0.0, 5.0, 10.0)],
                    1.0,
                ))
            },
            // A mitred frame: each side takes ∫(l − 2u) dA over the section, u its distance in
            // from the edge; ∫u dA = 1/2 − (π/4)(1 − 4/(3π)) = 5/6 − π/4 at r 1.
            answer: Answer::Solid { volume: 1000.0 - 4.0 * (10.0 * (1.0 - PI / 4.0) - 2.0 * (5.0 / 6.0 - PI / 4.0)), area: None },
        },
        Case {
            op: "fillet",
            name: "the top rim of an r 2 cylinder, r 0.5",
            build: || {
                some(solidcraft_kernel::fillet(&solidcraft_kernel::cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 2.0, 3.0)?, &[v(2.0, 0.0, 3.0)], 0.5))
            },
            answer: Answer::Solid { volume: 12.0 * PI - rimv, area: Some(20.0 * PI - rima) },
        },
    ]
}

/// One case on one kernel.
#[derive(serde::Serialize)]
struct Run {
    /// pass, fail, unsupported.
    verdict: &'static str,
    why: String,
    volume_error: Option<f64>,
    area_error: Option<f64>,
    worst_tolerance: Option<f64>,
    ms: f64,
}

fn relative(got: f64, want: f64) -> f64 {
    (got - want).abs() / want.abs().max(1.0)
}

fn worst_tolerance(b: &Body) -> f64 {
    let r = b.record();
    r.edges.iter().map(|e| e.tolerance).chain(r.vertices.iter().map(|v| v.tolerance)).fold(r.tolerance, f64::max)
}

fn run(case: &Case, kernel: Kernel) -> Run {
    foreign::set_kernel(kernel);
    let before = foreign::fell_back();
    let t = Instant::now();
    let got = (case.build)();
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let unsupported = kernel == Kernel::Occt && foreign::fell_back() > before;
    let mut out = Run { verdict: "fail", why: String::new(), volume_error: None, area_error: None, worst_tolerance: None, ms };
    match (&case.answer, got) {
        (Answer::Solid { volume, area }, Ok(Some(body))) => {
            out.worst_tolerance = Some(worst_tolerance(&body));
            match solidcraft_kernel::measure(&body) {
                Ok(m) => {
                    let ve = relative(m.volume, *volume);
                    let ae = area.map(|a| relative(m.area, a));
                    out.volume_error = Some(ve);
                    out.area_error = ae;
                    let check = solidcraft_kernel::failing_check(&body);
                    if ve > EXACT || ae.is_some_and(|e| e > EXACT) {
                        out.why = format!("volume {} (want {volume}), area {}", m.volume, m.area);
                    } else if !matches!(check, Ok(None)) {
                        out.why = format!("the shared check fails: {check:?}");
                    } else {
                        out.verdict = "pass";
                    }
                }
                Err(e) => out.why = format!("not measured: {e}"),
            }
        }
        (Answer::Solid { .. }, Ok(None)) => out.why = "empty".into(),
        (Answer::Empty, Ok(None)) | (Answer::Refused, Err(_)) => out.verdict = "pass",
        (Answer::Empty | Answer::Refused, Ok(Some(_))) => out.why = "made a body".into(),
        (Answer::Refused, Ok(None)) => out.why = "empty, not refused".into(),
        (_, Err(e)) => out.why = e.to_string(),
    }
    if unsupported {
        out.verdict = "unsupported";
    }
    out
}

fn main() {
    let json = std::env::args().skip_while(|a| a != "--json").nth(1);
    let mut rows = Vec::new();
    let mut table = String::from(
        "| op | case | solidcore | occt | volume error (sc / occt) | area error (sc / occt) | worst tolerance (sc / occt) | ms (sc / occt) |\n|---|---|---|---|---|---|---|---|\n",
    );
    let e = |x: Option<f64>| x.map_or("-".to_string(), |x| format!("{x:.1e}"));
    for case in cases() {
        let sc = run(&case, Kernel::Solidcore);
        let oc = run(&case, Kernel::Occt);
        table.push_str(&format!(
            "| {} | {} | {}{} | {}{} | {} / {} | {} / {} | {} / {} | {:.1} / {:.1} |\n",
            case.op,
            case.name,
            sc.verdict,
            if sc.why.is_empty() { String::new() } else { format!(" ({})", sc.why) },
            oc.verdict,
            if oc.why.is_empty() { String::new() } else { format!(" ({})", oc.why) },
            e(sc.volume_error),
            e(oc.volume_error),
            e(sc.area_error),
            e(oc.area_error),
            e(sc.worst_tolerance),
            e(oc.worst_tolerance),
            sc.ms,
            oc.ms,
        ));
        rows.push(serde_json::json!({ "op": case.op, "case": case.name, "solidcore": sc, "occt": oc }));
    }
    let mut summary = String::from(
        "\n| op | cases | solidcore pass | occt pass / fail / unsupported | occt looser than solidcore (any / above 1e-12) |\n|---|---|---|---|---|\n",
    );
    for op in ["box", "cylinder", "sphere", "torus", "boolean", "fillet"] {
        let of: Vec<&serde_json::Value> = rows.iter().filter(|r| r["op"] == op).collect();
        let count = |k: &str, v: &str| of.iter().filter(|r| r[k]["verdict"] == v).count();
        // Looser: larger than solidcore's beyond the last bits (1e-15), and also above 1e-12.
        let looser = |floor: f64| {
            of.iter()
                .filter(|r| {
                    ["volume_error", "area_error", "worst_tolerance"].iter().any(|m| match (r["solidcore"][m].as_f64(), r["occt"][m].as_f64()) {
                        (Some(s), Some(o)) => o > s + 1e-15 && o > floor,
                        _ => false,
                    })
                })
                .count()
        };
        summary.push_str(&format!(
            "| {op} | {} | {} | {} / {} / {} | {} / {} |\n",
            of.len(),
            count("solidcore", "pass"),
            count("occt", "pass"),
            count("occt", "fail"),
            count("occt", "unsupported"),
            looser(0.0),
            looser(1e-12)
        ));
    }
    println!("{table}{summary}");
    println!("occt: {}", foreign::occt_version());
    if let Some(path) = json {
        let text = serde_json::to_string_pretty(&rows).unwrap_or_default();
        if let Err(e) = std::fs::write(&path, text) {
            println!("could not write {path}: {e}");
        }
    }
}
