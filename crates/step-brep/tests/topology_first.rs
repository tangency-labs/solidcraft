//! The topology-first build ([`step_brep::build_body`]): a body built from the file's
//! own topology when that gives a closed solid, else stitched as before.
//!
//! The ignored tests are the gate's local runs:
//!
//! ```sh
//! # every body of a corpus, stitch-only against topology-first
//! SOLIDCORE_STEP_CORPUS=/path/to/corpus cargo test --release -p step-brep \
//!     --test topology_first -- --ignored --nocapture corpus
//! # every body of one file
//! SOLIDCORE_STEP_FILE=/path/to/file.stp cargo test --release -p step-brep \
//!     --test topology_first -- --ignored --nocapture one_file
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use solidcore::kernel::Kernel;
use solidcore::topology::Brep;
use step_brep::{build_body, is_closed_solid, BuildMode, BuildOptions, BuildPath};
use step_import::model::{
    Body, BodyKind, Bound, Curve, Edge, Face, Frame, Loop, OrientedEdge, Shell, Surface, Vertex,
};

/// Corner `i` of the 10 mm cube: x, y, z from bits 0, 1, 2.
fn corner(i: usize) -> [f64; 3] {
    let bit = |b: usize| if i & (1 << b) == 0 { 0.0 } else { 10.0 };
    [bit(0), bit(1), bit(2)]
}

/// A closed 10 mm cube, outward faces, its edges lines from the lower corner index.
fn cube() -> Body {
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for a in 0..8usize {
        for b in [1usize, 2, 4] {
            if a & b == 0 {
                pairs.push((a, a | b));
            }
        }
    }
    let edges: Vec<Edge> = pairs
        .iter()
        .enumerate()
        .map(|(index, &(a, b))| {
            let (p, q) = (corner(a), corner(b));
            Edge {
                step_id: index as u64 + 1,
                start: a,
                end: b,
                curve: Curve::Line {
                    origin: p,
                    dir: [
                        (q[0] - p[0]) / 10.0,
                        (q[1] - p[1]) / 10.0,
                        (q[2] - p[2]) / 10.0,
                    ],
                    magnitude: 1.0,
                },
                same_sense: true,
                t_start: 0.0,
                t_end: 10.0,
            }
        })
        .collect();
    let use_of = |a: usize, b: usize| {
        let (lo, hi) = (a.min(b), a.max(b));
        OrientedEdge {
            edge: pairs
                .iter()
                .position(|&p| p == (lo, hi))
                .unwrap_or_default(),
            forward: a < b,
        }
    };
    // (corners counter-clockwise about the outward normal, frame x, frame y)
    let sides: [([usize; 4], [f64; 3], [f64; 3]); 6] = [
        ([0, 2, 3, 1], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        ([4, 5, 7, 6], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0, 1, 5, 4], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([2, 6, 7, 3], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0, 4, 6, 2], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
        ([1, 3, 7, 5], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let faces = sides
        .iter()
        .enumerate()
        .map(|(index, (c, x, y))| {
            let z = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            Face {
                step_id: 100 + index as u64,
                surface: Surface::Plane {
                    frame: Frame {
                        origin: corner(c[0]),
                        x: *x,
                        y: *y,
                        z,
                    },
                },
                same_sense: true,
                loops: vec![Loop {
                    outer: true,
                    orientation: true,
                    bound: Bound::Edges(
                        c.iter()
                            .zip(c.iter().cycle().skip(1))
                            .map(|(&a, &b)| use_of(a, b))
                            .collect(),
                    ),
                }],
            }
        })
        .collect();
    Body {
        step_id: 1,
        name: Some("cube".into()),
        kind: BodyKind::ManifoldSolid,
        unit_mm: 1.0,
        uncertainty_mm: None,
        shells: vec![Shell {
            step_id: 1,
            closed: true,
            orientation: true,
            faces: (0..6).collect(),
        }],
        faces,
        edges,
        vertices: (0..8)
            .map(|i| Vertex {
                step_id: 1000 + i as u64,
                point: corner(i),
            })
            .collect(),
    }
}

fn options(stitch_only: bool) -> BuildOptions {
    BuildOptions {
        mode: if stitch_only {
            BuildMode::StitchOnly
        } else {
            BuildMode::Default
        },
        ..BuildOptions::default()
    }
}

fn volume(brep: &Brep) -> Option<f64> {
    let kernel = Kernel::default();
    let store = solidcore::geom::attribute::AttributeStore::default();
    let attrs = solidcore::mass::integrator::properties::PropertyAttributes {
        store: &store,
        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
    };
    let mut copy = brep.clone();
    solidcore::mass::integrator::properties::brep_volume(&kernel, &mut copy, 1e-9, &attrs).ok()
}

#[test]
fn a_closed_body_is_built_from_its_topology() {
    let built = build_body(&cube(), options(false)).unwrap();
    assert_eq!(built.path, BuildPath::Topology);
    assert!(built.face_failures.is_empty(), "{:?}", built.face_failures);
    assert!(is_closed_solid(&built.brep));
    assert_eq!(built.brep.face_list().len(), 6);
    assert_eq!(built.brep.edge_list().len(), 12);
    assert_eq!(built.brep.vertex_list().len(), 8);
    let v = volume(&built.brep).unwrap();
    assert!((v - 1000.0).abs() < 1e-6, "volume {v}");
    // The old build stitches.
    let stitched = build_body(&cube(), options(true)).unwrap();
    assert_eq!(stitched.path, BuildPath::Stitch);
}

/// One build's outcome as counts and classes (no geometry).
struct Outcome {
    path: Option<BuildPath>,
    counts: (usize, usize, usize, usize),
    closed: bool,
    display: bool,
    volume: Option<f64>,
    left_out: Vec<(u64, String)>,
    error: Option<String>,
    /// Why the topology-first build was not kept.
    note: Option<String>,
    seconds: f64,
}

fn shells(brep: &Brep) -> usize {
    let mut kernel = Kernel::default();
    solidcore::topology::brep::p1::shells(&mut kernel, brep, None).map_or(0, |s| s.len())
}

/// Builds `body` (stitch-only or default) and measures it.
fn outcome(body: &Body, stitch_only: bool) -> Outcome {
    let start = Instant::now();
    let built = build_body(body, options(stitch_only));
    let seconds = start.elapsed().as_secs_f64();
    measure(built, seconds)
}

/// Measures a build.
fn measure(built: Result<step_brep::BuiltBody, step_brep::BuildError>, seconds: f64) -> Outcome {
    match built {
        Ok(built) => Outcome {
            path: Some(built.path),
            counts: (
                built.brep.face_list().len(),
                built.brep.edge_list().len(),
                built.brep.vertex_list().len(),
                shells(&built.brep),
            ),
            closed: is_closed_solid(&built.brep),
            display: step_brep::validate_for_display(&built.brep).is_ok(),
            volume: volume(&built.brep),
            left_out: built
                .face_failures
                .iter()
                .map(|f| (f.face, f.reason.clone()))
                .collect(),
            error: None,
            note: built.topology_note.clone(),
            seconds,
        },
        Err(error) => Outcome {
            path: None,
            counts: (0, 0, 0, 0),
            closed: false,
            display: false,
            volume: None,
            left_out: Vec::new(),
            error: Some(error.to_string()),
            note: None,
            seconds,
        },
    }
}

impl Outcome {
    /// Counts and classes; the volume only as ok or failed when `values` is false.
    fn describe(&self, values: bool) -> String {
        if let Some(error) = &self.error {
            return format!("error ({:.2}s): {error}", self.seconds);
        }
        let volume = match (self.volume, values) {
            (Some(v), true) => format!("{v:.9e}"),
            (Some(_), false) => "ok".to_string(),
            (None, _) => "failed".to_string(),
        };
        let (faces, edges, vertices, shells) = self.counts;
        let mut extra = String::new();
        for (face, call) in &self.left_out {
            let _ = write!(extra, "\n    LEFT-OUT face #{face}: {call}");
        }
        if let Some(note) = &self.note {
            let _ = write!(extra, "\n    topology not kept: {note}");
        }
        format!(
            "{:?} faces={faces} edges={edges} vertices={vertices} shells={shells} closed={} display={} volume={volume} left_out={} ({:.2}s){}",
            self.path,
            self.closed,
            self.display,
            self.left_out.len(),
            self.seconds,
            extra,
        )
    }
}

/// How far the body's edge curves end from their vertices, against the B-rep
/// tolerance (1e-5): a face built on its own refuses a loop whose curve ends are apart
/// by more than 100 times that; a topology build limits each curve to its vertices.
fn edge_end_gaps(body: &Body) -> String {
    let tol = 1e-5;
    let mut largest = 0.0_f64;
    let (mut over, mut over_100) = (0usize, 0usize);
    for edge in &body.edges {
        for (s, vertex) in [(0.0, edge.start), (1.0, edge.end)] {
            let (Some(p), Some(v)) = (
                step_import::script::edge_point(edge, s),
                body.vertices.get(vertex),
            ) else {
                continue;
            };
            let gap = ((p[0] - v.point[0]).powi(2)
                + (p[1] - v.point[1]).powi(2)
                + (p[2] - v.point[2]).powi(2))
            .sqrt();
            largest = largest.max(gap);
            over += usize::from(gap > tol);
            over_100 += usize::from(gap > 100.0 * tol);
        }
    }
    format!(
        "edge-end gaps: largest {:.1}x the tolerance; {over} edge ends past it, {over_100} past 100x",
        largest / tol
    )
}

fn relative(a: f64, b: f64) -> f64 {
    (a - b).abs() / a.abs().max(b.abs()).max(f64::MIN_POSITIVE)
}

/// The cube with one edge's line 2e-3 off its corners: each face's loop gap is past
/// the face builder's tolerance, so stitching fails, but the file's topology joins the
/// edge to its vertices and closes the cube.
#[test]
fn a_body_stitching_fails_on_closes_from_its_topology() {
    let mut body = cube();
    if let Some(Curve::Line { origin, .. }) = body.edges.first_mut().map(|e| &mut e.curve) {
        origin[1] += 2e-3;
        origin[2] += 2e-3;
    }
    let stitched = build_body(&body, options(true));
    assert!(stitched.is_err(), "stitching closes it now");
    let built = build_body(&body, options(false)).unwrap();
    assert_eq!(built.path, BuildPath::Topology);
    assert!(is_closed_solid(&built.brep));
    let v = volume(&built.brep).unwrap();
    assert!((v - 1000.0).abs() < 1e-6, "volume {v}");
}

/// Every body of the file in `SOLIDCORE_STEP_FILE`, both ways: counts and classes
/// only (the file may be private).
#[test]
#[ignore = "needs SOLIDCORE_STEP_FILE"]
fn one_file() {
    let Some(path) = std::env::var_os("SOLIDCORE_STEP_FILE") else {
        eprintln!("SOLIDCORE_STEP_FILE is not set");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let model = step_import::import(&bytes, &step_import::Limits::default()).unwrap();
    println!(
        "{} bodies, {} skipped by the mapper",
        model.bodies.len(),
        model.skipped.len()
    );
    let mut closed = (0usize, 0usize);
    for (index, body) in model.bodies.iter().enumerate() {
        println!(
            "body {index} {:?}: {} file faces, {} file shells",
            body.kind,
            body.faces.len(),
            body.shells.len()
        );
        println!("  {}", edge_end_gaps(body));
        let old = outcome(body, true);
        let new = outcome(body, false);
        println!("  stitch-only: {}", old.describe(false));
        println!("  default:     {}", new.describe(false));
        if new.path != Some(BuildPath::Topology) {
            let start = Instant::now();
            let built = step_brep::build_from_topology(body, options(false));
            let alone = measure(built, start.elapsed().as_secs_f64());
            println!("  topology alone: {}", alone.describe(false));
        }
        if new.seconds > 2.0 * old.seconds.max(0.01) {
            println!(
                "  SLOWER body {index}: {:.2}s to {:.2}s",
                old.seconds, new.seconds
            );
        }
        closed.0 += usize::from(old.closed);
        closed.1 += usize::from(new.closed);
    }
    println!(
        "closed solids: {} stitch-only, {} default, of {}",
        closed.0,
        closed.1,
        model.bodies.len()
    );
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        paths.push(entry.path());
    }
    paths.sort();
    for path in paths {
        if path.is_dir() {
            files(&path, out);
        } else if path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "stp" | "step"))
        {
            out.push(path);
        }
    }
}

/// Every body of a corpus, stitch-only (today's build) against topology-first.
///
/// Fails when a body gets worse: stops building, closed to open, volume ok to
/// failing, display-valid to not. A body closed today that stays closed but with other
/// face, edge, vertex or shell counts, or its volume more than 1e-9 relative apart, is
/// listed (CHANGED) with both values for the referee table. Bodies that go from open
/// to closed, or from an error to a built body, are listed as gains; a body more than
/// twice as slow is flagged, and the total compares the summed build times.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn corpus() {
    let Some(dir) = std::env::var_os("SOLIDCORE_STEP_CORPUS") else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut paths = Vec::new();
    files(Path::new(&dir), &mut paths);
    assert!(!paths.is_empty(), "no STEP files under {dir:?}");
    let mut worse = Vec::new();
    let (mut bodies, mut topology, mut gains, mut slower, mut changed) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut old_seconds, mut new_seconds) = (0.0, 0.0);
    for path in &paths {
        let name = path
            .strip_prefix(&dir)
            .unwrap_or(path)
            .display()
            .to_string();
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(model) = step_import::import(&bytes, &step_import::Limits::default()) else {
            continue;
        };
        for (index, body) in model.bodies.iter().enumerate() {
            bodies += 1;
            let old = outcome(body, true);
            let new = outcome(body, false);
            old_seconds += old.seconds;
            new_seconds += new.seconds;
            println!("{name} body {index}:");
            println!("  stitch-only: {}", old.describe(true));
            println!("  default:     {}", new.describe(true));
            if new.path == Some(BuildPath::Topology) {
                topology += 1;
            }
            if new.seconds > 2.0 * old.seconds.max(0.01) {
                slower += 1;
                println!(
                    "  SLOWER {name} body {index}: {:.2}s to {:.2}s",
                    old.seconds, new.seconds
                );
            }
            let mut problems = Vec::new();
            match (&old.error, &new.error) {
                (None, Some(error)) => problems.push(format!("no longer builds: {error}")),
                (Some(_), None) => {
                    gains += 1;
                    println!(
                        "  GAIN {name} body {index}: error to {} ({} faces left out)",
                        if new.closed { "closed" } else { "open" },
                        new.left_out.len()
                    );
                }
                _ => {}
            }
            if old.error.is_none() && new.error.is_none() {
                if old.closed {
                    let same_volume = match (old.volume, new.volume) {
                        (Some(a), Some(b)) => relative(a, b) <= 1e-9,
                        _ => false,
                    };
                    if !new.closed {
                        problems.push("closed to open".to_string());
                    } else if old.counts != new.counts || !same_volume {
                        changed += 1;
                        println!(
                            "  CHANGED {name} body {index}: counts {:?} to {:?}, volume {:?} to {:?}",
                            old.counts, new.counts, old.volume, new.volume
                        );
                    }
                } else if new.closed {
                    gains += 1;
                    println!("  GAIN {name} body {index}: open to closed");
                }
                if old.volume.is_some() && new.volume.is_none() {
                    problems.push("volume ok to failing".to_string());
                }
                if old.display && !new.display {
                    problems.push("display-valid to invalid".to_string());
                }
            }
            for problem in problems {
                println!("  WORSE {name} body {index}: {problem}");
                worse.push(format!("{name} body {index}: {problem}"));
            }
        }
    }
    println!(
        "total: {bodies} bodies, {topology} built from their topology, {gains} gains, {changed} changed, {} worse, {slower} slower than 2x; {old_seconds:.1}s stitch-only, {new_seconds:.1}s default ({:.2}x)",
        worse.len(),
        new_seconds / old_seconds.max(f64::MIN_POSITIVE)
    );
    assert!(worse.is_empty(), "worse:\n{}", worse.join("\n"));
}
