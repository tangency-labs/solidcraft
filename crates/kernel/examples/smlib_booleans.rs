//! Side-by-side booleans: truck (today's kernel) and SMLib on the same cases, compared on
//! volume, face and edge counts and history. It reports; it never changes which kernel the app
//! uses.
//!
//! ```sh
//! scripts/fetch-smlib.sh
//! cargo run --release -p solidcraft-kernel --features smlib --example smlib_booleans -- \
//!     [--seeds N] [--out DIR]
//! ```
//!
//! Cases: named pairs of primitives whose results are known by hand (the ones the kernel's
//! boolean and history tests build), then `fuzz::boolean_shapes(seed)` for seeds 0..N (the
//! random pairs of `solidcraft-cli fuzz` and the robustness runs). Each case runs union,
//! intersection and cut. Writes `smlib-booleans.json` and `smlib-booleans.md` into DIR (default
//! `target/smlib-booleans`).
//!
//! SMLib runs in a child process per case (this same program, with `--smlib-case`), so a crash
//! inside it is reported as that case's result instead of ending the run.
//!
//! History is compared by its shape, since the two kernels number faces differently until the
//! SMLib export renumbers into canonical order: how many result faces are Kept, Modified, Split,
//! Merged and Generated, and how many edges come from an operand edge, are new where two faces
//! meet, or have no origin.

#[cfg(not(feature = "smlib"))]
fn main() {
    eprintln!("smlib_booleans needs SMLib: run scripts/fetch-smlib.sh, then add --features smlib");
    std::process::exit(2);
}

#[cfg(feature = "smlib")]
fn main() {
    if let Err(e) = run::main() {
        eprintln!("smlib_booleans: {e}");
        std::process::exit(1);
    }
}

#[cfg(feature = "smlib")]
mod run {
    use std::fmt::Write as _;
    use std::path::PathBuf;
    use std::time::Instant;

    use serde_json::{Value, json};
    use solidcraft_geom::brep::history::{Change, EdgeOrigin, History};
    use solidcraft_kernel::fuzz::{Shape, boolean_shapes};
    use solidcraft_kernel::{BoolOp, boolean, measure};
    use solidcraft_smlib::MEASURE_ACCURACY;
    use solidcraft_smlib::history::Order;

    const OPS: [BoolOp; 3] = [BoolOp::Union, BoolOp::Intersect, BoolOp::Cut];

    /// Relative volume difference that still counts as the same result: truck measures from a
    /// fine tessellation (`measure`), SMLib integrates exactly.
    const VOLUME_REL: f64 = 1e-3;

    /// What one kernel made of one case: its volume, face and edge counts, history shape and
    /// time, or why it made nothing.
    #[derive(Clone, Debug, Default)]
    struct Outcome {
        error: Option<String>,
        empty: bool,
        volume: f64,
        faces: usize,
        edges: usize,
        history: Option<HistoryShape>,
        ms: f64,
    }

    /// A history's shape: faces Kept, Modified, Split, Merged, Generated; edges from an operand
    /// edge, new where two faces meet, without origin.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct HistoryShape {
        faces: [usize; 5],
        edges: [usize; 3],
    }

    impl HistoryShape {
        fn of(h: &History) -> HistoryShape {
            let mut s = HistoryShape::default();
            for f in &h.faces {
                let i = match f.change {
                    Change::Kept => 0,
                    Change::Modified => 1,
                    Change::Split => 2,
                    Change::Merged => 3,
                    Change::Generated => 4,
                };
                if let Some(n) = s.faces.get_mut(i) {
                    *n += 1;
                }
            }
            for e in &h.edges {
                let i = if e.origins.iter().any(|o| matches!(o, EdgeOrigin::Edge { .. })) {
                    0
                } else if e.origins.is_empty() {
                    2
                } else {
                    1
                };
                if let Some(n) = s.edges.get_mut(i) {
                    *n += 1;
                }
            }
            s
        }

        fn text(&self) -> String {
            let [k, m, s, g, n] = self.faces;
            let [e, meet, none] = self.edges;
            format!("K{k} M{m} S{s} J{g} G{n} / e{e} x{meet} -{none}")
        }
    }

    fn op_name(op: BoolOp) -> &'static str {
        match op {
            BoolOp::Union => "union",
            BoolOp::Intersect => "intersect",
            BoolOp::Cut => "cut",
        }
    }

    fn truck(a: &Shape, b: &Shape, op: BoolOp) -> Outcome {
        let t0 = Instant::now();
        let mut o = Outcome::default();
        let result = a.body().and_then(|ba| b.body().map(|bb| (ba, bb))).and_then(|(ba, bb)| boolean(&ba, &bb, op));
        o.ms = t0.elapsed().as_secs_f64() * 1e3;
        match result {
            Err(e) => o.error = Some(e.to_string()),
            Ok(None) => o.empty = true,
            Ok(Some(body)) => {
                o.faces = body.face_count();
                o.edges = body.edge_count();
                o.history = body.history().map(HistoryShape::of);
                match measure(&body) {
                    Ok(m) => o.volume = m.volume,
                    Err(e) => o.error = Some(format!("measure: {e}")),
                }
            }
        }
        o
    }

    fn smlib(a: &Shape, b: &Shape, op: BoolOp) -> Outcome {
        let t0 = Instant::now();
        let mut o = Outcome::default();
        let result = (|| -> Result<Option<solidcraft_smlib::Brep>, String> {
            let ta = side::shape(a)?.tagged(0).map_err(|e| e.to_string())?;
            let tb = side::shape(b)?.tagged(1).map_err(|e| e.to_string())?;
            side::boolean(&ta, &tb, op)
        })();
        o.ms = t0.elapsed().as_secs_f64() * 1e3;
        match result {
            Err(e) => o.error = Some(e),
            Ok(None) => o.empty = true,
            Ok(Some(r)) => {
                let counts = r.counts().map_err(|e| e.to_string());
                let volume = r.mass_properties(MEASURE_ACCURACY).map(|m| m.volume).map_err(|e| e.to_string());
                let history = r
                    .history()
                    .map_err(|e| e.to_string())
                    .and_then(|raw| raw.history(&[Order::default(), Order::default()], &Order::default()).map_err(|e| e.to_string()));
                match (counts, volume) {
                    (Ok([f, e, _]), Ok(v)) => {
                        o.faces = f;
                        o.edges = e;
                        o.volume = v;
                    }
                    (Err(e), _) | (_, Err(e)) => o.error = Some(e),
                }
                o.history = history.ok().as_ref().map(HistoryShape::of);
            }
        }
        o
    }

    /// The SMLib side, through `solidcraft-smlib`. Boxes and cylinders are the very B-reps truck
    /// starts from, through their schema 4 records imported into SMLib (`smlib_body`), so both
    /// kernels get the same input whatever the axis. Spheres and tori are SMLib's own primitives
    /// (about +Z, like truck's): the import doesn't take truck's revolved surfaces yet. The
    /// boolean is SMLib's own (`Brep::boolean`, through smlib_ops.cpp).
    mod side {
        use solidcraft_kernel::BoolOp;
        use solidcraft_kernel::fuzz::Shape;
        use solidcraft_smlib::{BooleanOp, Brep};

        pub fn shape(s: &Shape) -> Result<Brep, String> {
            match s {
                Shape::Sphere { center, radius } => return Brep::make_sphere(*center, *radius).map_err(|e| e.to_string()),
                Shape::Torus { center, major, minor } => return Brep::make_torus(*center, *major, *minor).map_err(|e| e.to_string()),
                Shape::Box { .. } | Shape::Cylinder { .. } => {}
            }
            let body = s.body().map_err(|e| format!("truck primitive: {e}"))?;
            solidcraft_kernel::smlib_body(&body).map_err(|e| format!("import: {e}"))
        }

        /// `a op b`; `None` when the result is empty.
        pub fn boolean(a: &Brep, b: &Brep, op: BoolOp) -> Result<Option<Brep>, String> {
            let op = match op {
                BoolOp::Union => BooleanOp::Union,
                BoolOp::Intersect => BooleanOp::Intersect,
                BoolOp::Cut => BooleanOp::Subtract,
            };
            a.boolean(b, op).map_err(|e| e.to_string())
        }
    }

    /// Pairs whose results are known by hand.
    fn named() -> Vec<(String, Shape, Shape)> {
        let bx = |a: [f64; 3], b: [f64; 3]| Shape::Box { a, b };
        let z = [0.0, 0.0, 1.0];
        vec![
            ("overlapping boxes".into(), bx([0.0; 3], [10.0; 3]), bx([5.0; 3], [15.0; 3])),
            ("boxes sharing faces".into(), bx([0.0; 3], [10.0; 3]), bx([5.0, 0.0, 0.0], [15.0, 10.0, 10.0])),
            ("boxes apart".into(), bx([0.0; 3], [10.0; 3]), bx([20.0, 0.0, 0.0], [30.0, 10.0, 10.0])),
            ("slot through a bar".into(), bx([0.0; 3], [30.0, 10.0, 10.0]), bx([10.0, -1.0, -1.0], [20.0, 11.0, 11.0])),
            ("pocket".into(), bx([0.0; 3], [10.0; 3]), bx([3.0; 3], [7.0, 7.0, 23.0])),
            ("hole through a box".into(), bx([0.0; 3], [10.0; 3]), Shape::Cylinder { base: [5.0, 5.0, -5.0], axis: z, radius: 2.0, height: 20.0 }),
            (
                "crossing cylinders".into(),
                Shape::Cylinder { base: [0.0, 0.0, -10.0], axis: z, radius: 4.0, height: 20.0 },
                Shape::Cylinder { base: [-10.0, 0.0, 0.0], axis: [1.0, 0.0, 0.0], radius: 3.0, height: 20.0 },
            ),
            ("sphere into a box corner".into(), bx([0.0; 3], [10.0; 3]), Shape::Sphere { center: [10.0; 3], radius: 5.0 }),
            (
                "torus around a post".into(),
                Shape::Cylinder { base: [0.0; 3], axis: z, radius: 3.0, height: 10.0 },
                Shape::Torus { center: [0.0, 0.0, 5.0], major: 4.0, minor: 2.0 },
            ),
        ]
    }

    fn outcome_from_json(v: &Value) -> Outcome {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).and_then(|x| usize::try_from(x).ok()).unwrap_or(0);
        let f = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let arr = |h: &Value, k: &str| -> Vec<usize> {
            h.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_u64).filter_map(|x| usize::try_from(x).ok()).collect())
                .unwrap_or_default()
        };
        let history = v
            .get("history")
            .filter(|h| !h.is_null())
            .and_then(|h| Some(HistoryShape { faces: arr(h, "faces").try_into().ok()?, edges: arr(h, "edges").try_into().ok()? }));
        Outcome {
            error: v.get("error").and_then(Value::as_str).map(str::to_string),
            empty: v.get("empty").and_then(Value::as_bool).unwrap_or(false),
            volume: f("volume"),
            faces: n("faces"),
            edges: n("edges"),
            history,
            ms: f("ms"),
        }
    }

    /// [`smlib`] for case `index` and operation `op`, in a child process.
    fn smlib_isolated(index: usize, op: usize, seeds: u64) -> Outcome {
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(e) => return Outcome { error: Some(format!("cannot start a child: {e}")), ..Outcome::default() },
        };
        let run = std::process::Command::new(exe)
            .args(["--seeds", &seeds.to_string(), "--smlib-case", &index.to_string(), &op.to_string()])
            .stderr(std::process::Stdio::null())
            .output();
        match run {
            Err(e) => Outcome { error: Some(format!("cannot start a child: {e}")), ..Outcome::default() },
            Ok(out) if !out.status.success() => Outcome { error: Some(format!("crashed ({})", out.status)), ..Outcome::default() },
            Ok(out) => match serde_json::from_slice::<Value>(&out.stdout) {
                Ok(v) => outcome_from_json(&v),
                Err(e) => Outcome { error: Some(format!("unreadable child output: {e}")), ..Outcome::default() },
            },
        }
    }

    fn outcome_json(o: &Outcome) -> Value {
        json!({
            "error": o.error,
            "empty": o.empty,
            "volume": o.volume,
            "faces": o.faces,
            "edges": o.edges,
            "history": o.history.map(|h| json!({"faces": h.faces, "edges": h.edges})),
            "ms": o.ms,
        })
    }

    /// How the two compare: "same", what differs, or why it couldn't be compared.
    fn verdict(t: &Outcome, s: &Outcome) -> (String, &'static str) {
        if s.error.as_deref().is_some_and(|e| e.starts_with("not supported yet")) {
            return ("not run on SMLib".into(), "skipped");
        }
        match (&t.error, &s.error) {
            (Some(_), Some(_)) => return ("both failed".into(), "failed"),
            (Some(_), None) => return ("truck failed, SMLib didn't".into(), "smlib-only"),
            (None, Some(_)) => return ("SMLib failed, truck didn't".into(), "truck-only"),
            (None, None) => {}
        }
        if t.empty || s.empty {
            return if t.empty == s.empty { ("both empty".into(), "same") } else { ("one empty".into(), "differs") };
        }
        let mut diffs = Vec::new();
        let scale = t.volume.abs().max(s.volume.abs()).max(1e-9);
        if (t.volume - s.volume).abs() > VOLUME_REL * scale {
            diffs.push(format!("volume {:.4} vs {:.4}", t.volume, s.volume));
        }
        if t.faces != s.faces {
            diffs.push(format!("faces {} vs {}", t.faces, s.faces));
        }
        if t.edges != s.edges {
            diffs.push(format!("edges {} vs {}", t.edges, s.edges));
        }
        match (t.history, s.history) {
            (Some(a), Some(b)) if a != b => diffs.push("history".into()),
            (None, Some(_)) => diffs.push("no truck history".into()),
            (Some(_), None) => diffs.push("no SMLib history".into()),
            _ => {}
        }
        if diffs.is_empty() { ("same".into(), "same") } else { (diffs.join(", "), "differs") }
    }

    pub fn main() -> Result<(), String> {
        let mut seeds = 32u64;
        let mut out = PathBuf::from("target/smlib-booleans");
        let mut child = None;
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--seeds" => seeds = args.next().and_then(|n| n.parse().ok()).ok_or("--seeds takes a number")?,
                "--out" => out = args.next().map(PathBuf::from).ok_or("--out takes a directory")?,
                "--smlib-case" => {
                    let mut n = || args.next().and_then(|n| n.parse::<usize>().ok()).ok_or("--smlib-case takes a case and an operation");
                    child = Some((n()?, n()?));
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        let mut cases = named();
        cases.extend((0..seeds).map(|s| {
            let (a, b) = boolean_shapes(s);
            (format!("seed {s}"), a, b)
        }));
        if let Some((index, op)) = child {
            let ((_, a, b), op) = cases.get(index).zip(OPS.get(op)).ok_or("no such case")?;
            solidcraft_smlib::init().map_err(|e| e.to_string())?;
            println!("{}", outcome_json(&smlib(a, b, *op)));
            return Ok(());
        }
        solidcraft_smlib::init().map_err(|e| e.to_string())?;

        let mut rows = Vec::new();
        let mut md = String::new();
        let mut tally = std::collections::BTreeMap::<&str, usize>::new();
        for (index, (name, a, b)) in cases.iter().enumerate() {
            for (k, op) in OPS.into_iter().enumerate() {
                let t = truck(a, b, op);
                let s = smlib_isolated(index, k, seeds);
                let (what, class) = verdict(&t, &s);
                *tally.entry(class).or_default() += 1;
                let cell = |o: &Outcome| match (&o.error, o.empty) {
                    (Some(e), _) => format!("error: {}", e.replace('|', "/")),
                    (None, true) => "empty".into(),
                    (None, false) => format!(
                        "{:.3} · {}f {}e · {} · {:.0} ms",
                        o.volume,
                        o.faces,
                        o.edges,
                        o.history.map_or("no history".into(), |h| h.text()),
                        o.ms
                    ),
                };
                let _ = writeln!(md, "| {name} | {} | {} | {} | {what} |", op_name(op), cell(&t), cell(&s));
                rows.push(json!({
                    "case": name, "a": a, "b": b, "op": op_name(op),
                    "truck": outcome_json(&t), "smlib": outcome_json(&s),
                    "verdict": class, "detail": what,
                }));
            }
        }

        let summary: Vec<String> = tally.iter().map(|(k, v)| format!("{k} {v}")).collect();
        let mut report = String::new();
        let _ = writeln!(report, "# Booleans: truck and SMLib side by side\n");
        let _ = writeln!(report, "{} cases × 3 operations: {}.\n", cases.len(), summary.join(", "));
        let _ = writeln!(
            report,
            "History shape: faces **K**ept, **M**odified, **S**plit, **J**oined (merged), **G**enerated; \
             edges from an operand **e**dge, new where faces meet (**x**), without origin (**-**).\n"
        );
        let _ = writeln!(report, "| case | op | truck | SMLib | verdict |\n|---|---|---|---|---|");
        report.push_str(&md);

        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let json = json!({ "cases": cases.len(), "tally": tally, "rows": rows });
        let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
        std::fs::write(out.join("smlib-booleans.json"), text).map_err(|e| e.to_string())?;
        std::fs::write(out.join("smlib-booleans.md"), &report).map_err(|e| e.to_string())?;
        println!("{} cases × 3: {} ({})", cases.len(), summary.join(", "), out.display());
        Ok(())
    }
}
