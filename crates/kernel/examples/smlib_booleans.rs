//! Side-by-side booleans: truck (today's kernel) and SMLib on the same cases, compared on
//! volume, face and edge counts, history and time. It reports; it never changes which kernel the
//! app uses.
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
//! intersection and cut. Then one named sequence, each operation on the previous result: a
//! block, a boss joined on top, three through-cuts from the side faces, the last splitting the
//! body (the "each cut gets slower" case of TASK_kernel-smlib-booleans). Writes
//! `smlib-booleans.json` and `smlib-booleans.md` into DIR (default `target/smlib-booleans`).
//!
//! SMLib runs in a child process per case (this same program, with `--smlib-case`, and the
//! sequence with `--smlib-sequence`), so a crash inside it is reported as that case's result
//! instead of ending the run.
//!
//! Times are wall times of the boolean call alone (`boolean` on truck, `Brep::boolean` on
//! SMLib). The SMLib side's conversions are timed apart: the operands' truck→SMLib import
//! (`smlib_body`) and the result's SMLib→truck export (`body_from_smlib`), which the app pays
//! for as long as truck holds the bodies.
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
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_6, PI};
    use std::fmt::Write as _;
    use std::path::PathBuf;
    use std::time::Instant;

    use serde_json::{Value, json};
    use solidcraft_geom::brep::history::{Change, EdgeOrigin, History};
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
    use solidcraft_kernel::fuzz::{Shape, boolean_shapes};
    use solidcraft_kernel::{Body, BoolOp, boolean, extrude, measure};
    use solidcraft_smlib::MEASURE_ACCURACY;
    use solidcraft_smlib::history::Order;

    const OPS: [BoolOp; 3] = [BoolOp::Union, BoolOp::Intersect, BoolOp::Cut];

    /// Relative volume difference that still counts as the same result: truck measures from a
    /// fine tessellation (`measure`), SMLib integrates exactly.
    const VOLUME_REL: f64 = 1e-3;

    /// What one kernel made of one case: its volume, face, edge and solid counts, history shape
    /// and times, or why it made nothing.
    #[derive(Clone, Debug)]
    struct Outcome {
        error: Option<String>,
        empty: bool,
        volume: f64,
        faces: usize,
        edges: usize,
        solids: usize,
        history: Option<HistoryShape>,
        /// The boolean call alone.
        ms: f64,
        /// SMLib only: the operands' truck→SMLib import.
        import_ms: f64,
        /// SMLib only: the result's SMLib→truck export, and why it failed.
        export_ms: f64,
        export_error: Option<String>,
        /// SMLib only: truck's measure of the exported result (NaN when there is none), which
        /// tells a wrong body from a wrong SMLib measure.
        export_volume: f64,
    }

    impl Default for Outcome {
        fn default() -> Outcome {
            Outcome {
                error: None,
                empty: false,
                volume: 0.0,
                faces: 0,
                edges: 0,
                solids: 0,
                history: None,
                ms: 0.0,
                import_ms: 0.0,
                export_ms: 0.0,
                export_error: None,
                export_volume: f64::NAN,
            }
        }
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

    fn ms_since(t0: Instant) -> f64 {
        t0.elapsed().as_secs_f64() * 1e3
    }

    /// `a op b` on truck, timing the boolean alone.
    fn truck_bodies(a: &Body, b: &Body, op: BoolOp) -> (Outcome, Option<Body>) {
        let mut o = Outcome::default();
        let t0 = Instant::now();
        let result = boolean(a, b, op);
        o.ms = ms_since(t0);
        match result {
            Err(e) => o.error = Some(e.to_string()),
            Ok(None) => o.empty = true,
            Ok(Some(body)) => {
                o.faces = body.face_count();
                o.edges = body.edge_count();
                o.solids = body.shell_count();
                o.history = body.history().map(HistoryShape::of);
                match measure(&body) {
                    Ok(m) => o.volume = m.volume,
                    Err(e) => o.error = Some(format!("measure: {e}")),
                }
                return (o, Some(body));
            }
        }
        (o, None)
    }

    fn truck(a: &Shape, b: &Shape, op: BoolOp) -> Outcome {
        match a.body().and_then(|ba| b.body().map(|bb| (ba, bb))) {
            Err(e) => Outcome { error: Some(format!("truck primitive: {e}")), ..Outcome::default() },
            Ok((ba, bb)) => truck_bodies(&ba, &bb, op).0,
        }
    }

    /// `a op b` on SMLib, operands already in SMLib: tagged as operands 0 and 1, the boolean
    /// timed alone, then measured exactly and exported back to truck (timed apart).
    fn smlib_breps(a: &solidcraft_smlib::Brep, b: &solidcraft_smlib::Brep, op: BoolOp) -> (Outcome, Option<solidcraft_smlib::Brep>) {
        let mut o = Outcome::default();
        let tagged = a.tagged(0).and_then(|ta| b.tagged(1).map(|tb| (ta, tb)));
        let (ta, tb) = match tagged {
            Ok(t) => t,
            Err(e) => return (Outcome { error: Some(format!("tag: {e}")), ..o }, None),
        };
        let t0 = Instant::now();
        let result = side::boolean(&ta, &tb, op);
        o.ms = ms_since(t0);
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
                o.solids = r.solid_count().unwrap_or(0);
                o.history = history.ok().as_ref().map(HistoryShape::of);
                let t0 = Instant::now();
                let back = solidcraft_kernel::body_from_smlib(&r);
                o.export_ms = ms_since(t0);
                match back.map_err(|e| e.to_string()).and_then(|b| measure(&b).map_err(|e| format!("measure: {e}"))) {
                    Ok(m) => o.export_volume = m.volume,
                    Err(e) => o.export_error = Some(e),
                }
                return (o, Some(r));
            }
        }
        (o, None)
    }

    fn smlib(a: &Shape, b: &Shape, op: BoolOp) -> Outcome {
        let t0 = Instant::now();
        let operands = side::shape(a).and_then(|sa| side::shape(b).map(|sb| (sa, sb)));
        let import_ms = ms_since(t0);
        match operands {
            Err(e) => Outcome { error: Some(e), import_ms, ..Outcome::default() },
            Ok(((sa, ia), (sb, ib))) => {
                let (mut o, _) = smlib_breps(&sa, &sb, op);
                o.import_ms = if ia || ib { import_ms } else { 0.0 };
                o
            }
        }
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

        /// The shape in SMLib, and whether it was imported from truck (rather than made there).
        pub fn shape(s: &Shape) -> Result<(Brep, bool), String> {
            match s {
                Shape::Sphere { center, radius } => return Brep::make_sphere(*center, *radius).map(|b| (b, false)).map_err(|e| e.to_string()),
                Shape::Torus { center, major, minor } => {
                    return Brep::make_torus(*center, *major, *minor).map(|b| (b, false)).map_err(|e| e.to_string());
                }
                Shape::Box { .. } | Shape::Cylinder { .. } => {}
            }
            let body = s.body().map_err(|e| format!("truck primitive: {e}"))?;
            solidcraft_kernel::smlib_body(&body).map(|b| (b, true)).map_err(|e| format!("import: {e}"))
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

    /// ∫ over the disc y² + z² ≤ r² of the chord 2√(R² − y²): the volume a cylinder of radius r
    /// shares with a perpendicular one of radius R ≥ r through its axis, both long enough.
    /// Simpson in y = r sin θ, where the integrand is smooth.
    fn crossing_cylinders(r: f64, big: f64) -> f64 {
        simpson(-FRAC_PI_2, FRAC_PI_2, |t| {
            let (s, c) = t.sin_cos();
            4.0 * r * r * c * c * (big * big - r * r * s * s).max(0.0).sqrt()
        })
    }

    fn simpson(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
        let n = 4096;
        let h = (b - a) / n as f64;
        let mut sum = f(a) + f(b);
        for i in 1..n {
            sum += f(a + h * i as f64) * if i % 2 == 1 { 4.0 } else { 2.0 };
        }
        sum * h / 3.0
    }

    /// Union, intersection and cut volumes of bodies of volume `a` and `b` sharing `common`.
    fn by_op(a: f64, b: f64, common: f64) -> [f64; 3] {
        [a + b - common, common, a - common]
    }

    /// Pairs whose results are known by hand, with their union, intersection and cut volumes
    /// (0 is empty).
    fn named() -> Vec<(String, Shape, Shape, Option<[f64; 3]>)> {
        let bx = |a: [f64; 3], b: [f64; 3]| Shape::Box { a, b };
        let z = [0.0, 0.0, 1.0];
        let ball = 4.0 / 3.0 * PI * 125.0;
        // The torus's tube (centre radius 4, radius 2) inside the post's radius 3: the tube's
        // section disc for ρ ≤ 3, swept round, with ρ − 4 = 2 sin θ.
        let ring_in_post = simpson(-FRAC_PI_2, -FRAC_PI_6, |t| {
            let (s, c) = t.sin_cos();
            2.0 * PI * (4.0 + 2.0 * s) * 8.0 * c * c
        });
        vec![
            ("overlapping boxes".into(), bx([0.0; 3], [10.0; 3]), bx([5.0; 3], [15.0; 3]), Some(by_op(1000.0, 1000.0, 125.0))),
            ("boxes sharing faces".into(), bx([0.0; 3], [10.0; 3]), bx([5.0, 0.0, 0.0], [15.0, 10.0, 10.0]), Some(by_op(1000.0, 1000.0, 500.0))),
            ("boxes apart".into(), bx([0.0; 3], [10.0; 3]), bx([20.0, 0.0, 0.0], [30.0, 10.0, 10.0]), Some(by_op(1000.0, 1000.0, 0.0))),
            (
                "slot through a bar".into(),
                bx([0.0; 3], [30.0, 10.0, 10.0]),
                bx([10.0, -1.0, -1.0], [20.0, 11.0, 11.0]),
                Some(by_op(3000.0, 1440.0, 1000.0)),
            ),
            ("pocket".into(), bx([0.0; 3], [10.0; 3]), bx([3.0; 3], [7.0, 7.0, 23.0]), Some(by_op(1000.0, 320.0, 112.0))),
            (
                "hole through a box".into(),
                bx([0.0; 3], [10.0; 3]),
                Shape::Cylinder { base: [5.0, 5.0, -5.0], axis: z, radius: 2.0, height: 20.0 },
                Some(by_op(1000.0, 80.0 * PI, 40.0 * PI)),
            ),
            (
                "crossing cylinders".into(),
                Shape::Cylinder { base: [0.0, 0.0, -10.0], axis: z, radius: 4.0, height: 20.0 },
                Shape::Cylinder { base: [-10.0, 0.0, 0.0], axis: [1.0, 0.0, 0.0], radius: 3.0, height: 20.0 },
                Some(by_op(320.0 * PI, 180.0 * PI, crossing_cylinders(3.0, 4.0))),
            ),
            (
                "sphere into a box corner".into(),
                bx([0.0; 3], [10.0; 3]),
                Shape::Sphere { center: [10.0; 3], radius: 5.0 },
                Some(by_op(1000.0, ball, ball / 8.0)),
            ),
            (
                "torus around a post".into(),
                Shape::Cylinder { base: [0.0; 3], axis: z, radius: 3.0, height: 10.0 },
                Shape::Torus { center: [0.0, 0.0, 5.0], major: 4.0, minor: 2.0 },
                Some(by_op(90.0 * PI, 32.0 * PI * PI, ring_in_post)),
            ),
        ]
    }

    /// One step of the cut sequence: a profile extruded into a tool, combined with the body so
    /// far. The first step (no operation) is the starting body itself.
    struct Step {
        name: &'static str,
        op: Option<BoolOp>,
        plane: Plane,
        region: Region2,
        start: f64,
        end: f64,
        /// The body's volume after this step.
        volume: f64,
        solids: usize,
    }

    impl Step {
        fn tool(&self) -> Result<Body, String> {
            let mut bodies = extrude(&self.plane, std::slice::from_ref(&self.region), self.start, self.end).map_err(|e| e.to_string())?;
            match (bodies.pop(), bodies.is_empty()) {
                (Some(b), true) => Ok(b),
                _ => Err("the profile extruded into other than one body".into()),
            }
        }
    }

    fn region(outer: Loop2) -> Region2 {
        Region2 { outer: outer.ccw(), holes: vec![] }
    }

    /// A slot between the centres `a` and `b` of its two end arcs, `r` wide either side.
    fn slot(a: Vec2, b: Vec2, r: f64) -> Loop2 {
        let d = b - a;
        let side = Vec2::new(-d.y, d.x) * (r / d.x.hypot(d.y));
        let along = d.y.atan2(d.x);
        Loop2 {
            segs: vec![
                Seg2::Line { a: a - side, b: b - side },
                Seg2::Arc { center: b, radius: r, start: along - FRAC_PI_2, sweep: PI },
                Seg2::Line { a: b + side, b: a + side },
                Seg2::Arc { center: a, radius: r, start: along + FRAC_PI_2, sweep: PI },
            ],
        }
    }

    /// Block 63.6 × 31.7 × 9.5; a Ø20 × 10 boss joined on its top; a slot through it front to
    /// back; a Ø6 cross hole through the boss from the right side; a slot front to back through
    /// the whole height that splits the block in two. Tools run 1 mm past the body each side.
    fn sequence() -> Vec<Step> {
        let (w, d, h) = (63.6, 31.7, 9.5);
        let block = w * d * h;
        let boss = block + 1000.0 * PI;
        // Front plane: sketch x = +X, y = −Z, normal +Y.
        let front = Plane { origin: Vec3::new(0.0, -1.0, 0.0), ..Plane::XZ };
        // Left plane: sketch x = +Y, y = +Z, normal +X.
        let left = Plane { origin: Vec3::new(-1.0, 0.0, 0.0), ..Plane::YZ };
        let slot1 = boss - (10.0 * 5.0 + PI * 2.5 * 2.5) * d;
        let cross = slot1 - crossing_cylinders(3.0, 10.0);
        let split = cross - 4.0 * h * d;
        vec![
            Step {
                name: "block",
                op: None,
                plane: Plane::XY,
                region: region(Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, d), Vec2::new(0.0, d)])),
                start: 0.0,
                end: h,
                volume: block,
                solids: 1,
            },
            Step {
                name: "join Ø20 boss",
                op: Some(BoolOp::Union),
                plane: Plane { origin: Vec3::new(0.0, 0.0, h), ..Plane::XY },
                region: region(Loop2::circle(Vec2::new(w / 2.0, d / 2.0), 10.0)),
                start: 0.0,
                end: 10.0,
                volume: boss,
                solids: 1,
            },
            Step {
                name: "cut slot front to back",
                op: Some(BoolOp::Cut),
                plane: front,
                region: region(slot(Vec2::new(8.0, -h / 2.0), Vec2::new(18.0, -h / 2.0), 2.5)),
                start: 0.0,
                end: d + 2.0,
                volume: slot1,
                solids: 1,
            },
            Step {
                name: "cut Ø6 cross hole in the boss",
                op: Some(BoolOp::Cut),
                plane: left,
                region: region(Loop2::circle(Vec2::new(d / 2.0, h + 5.0), 3.0)),
                start: 0.0,
                end: w + 2.0,
                volume: cross,
                solids: 1,
            },
            Step {
                name: "cut splitting slot",
                op: Some(BoolOp::Cut),
                plane: front,
                region: region(slot(Vec2::new(52.0, 3.0), Vec2::new(52.0, -13.0), 2.0)),
                start: 0.0,
                end: d + 2.0,
                volume: split,
                solids: 2,
            },
        ]
    }

    /// The sequence on truck, each step on the previous step's result; stops at the first
    /// failure.
    fn truck_sequence(steps: &[Step]) -> Vec<Outcome> {
        let mut out = Vec::new();
        let mut body: Option<Body> = None;
        for s in steps {
            let tool = match s.tool() {
                Ok(t) => t,
                Err(e) => {
                    out.push(Outcome { error: Some(format!("tool: {e}")), ..Outcome::default() });
                    break;
                }
            };
            let (o, next) = match (&body, s.op) {
                (Some(b), Some(op)) => truck_bodies(b, &tool, op),
                _ => {
                    let o = Outcome {
                        faces: tool.face_count(),
                        edges: tool.edge_count(),
                        solids: tool.shell_count(),
                        volume: measure(&tool).map(|m| m.volume).unwrap_or(f64::NAN),
                        ..Outcome::default()
                    };
                    (o, Some(tool))
                }
            };
            let failed = o.error.is_some() || next.is_none();
            out.push(o);
            body = next;
            if failed {
                break;
            }
        }
        out
    }

    /// The sequence on SMLib, each step on SMLib's own previous result (only the tools come from
    /// truck); stops at the first failure.
    fn smlib_sequence(steps: &[Step]) -> Vec<Outcome> {
        let mut out = Vec::new();
        let mut body: Option<solidcraft_smlib::Brep> = None;
        for s in steps {
            let t0 = Instant::now();
            let tool = s.tool().and_then(|t| solidcraft_kernel::smlib_body(&t).map_err(|e| format!("import: {e}")));
            let import_ms = ms_since(t0);
            let tool = match tool {
                Ok(t) => t,
                Err(e) => {
                    out.push(Outcome { error: Some(format!("tool: {e}")), import_ms, ..Outcome::default() });
                    break;
                }
            };
            let (mut o, next) = match (&body, s.op) {
                (Some(b), Some(op)) => smlib_breps(b, &tool, op),
                _ => {
                    let [faces, edges, _] = tool.counts().unwrap_or_default();
                    let o = Outcome {
                        faces,
                        edges,
                        solids: tool.solid_count().unwrap_or(0),
                        volume: tool.mass_properties(MEASURE_ACCURACY).map(|m| m.volume).unwrap_or(f64::NAN),
                        ..Outcome::default()
                    };
                    (o, Some(tool))
                }
            };
            o.import_ms = import_ms;
            let failed = o.error.is_some() || next.is_none();
            out.push(o);
            body = next;
            if failed {
                break;
            }
        }
        out
    }

    fn outcome_from_json(v: &Value) -> Outcome {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).and_then(|x| usize::try_from(x).ok()).unwrap_or(0);
        let f = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
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
            error: s("error"),
            empty: v.get("empty").and_then(Value::as_bool).unwrap_or(false),
            volume: f("volume"),
            faces: n("faces"),
            edges: n("edges"),
            solids: n("solids"),
            history,
            ms: f("ms"),
            import_ms: f("import_ms"),
            export_ms: f("export_ms"),
            export_error: s("export_error"),
            export_volume: f("export_volume"),
        }
    }

    /// This program again with `args`, its standard output as JSON; a crash is an error.
    fn child(args: &[String]) -> Result<Value, String> {
        let exe = std::env::current_exe().map_err(|e| format!("cannot start a child: {e}"))?;
        let out = std::process::Command::new(exe)
            .args(args)
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("cannot start a child: {e}"))?;
        if !out.status.success() {
            return Err(format!("crashed ({})", out.status));
        }
        serde_json::from_slice::<Value>(&out.stdout).map_err(|e| format!("unreadable child output: {e}"))
    }

    /// [`smlib`] for case `index` and operation `op`, in a child process.
    fn smlib_isolated(index: usize, op: usize, seeds: u64) -> Outcome {
        let args = ["--seeds".to_string(), seeds.to_string(), "--smlib-case".into(), index.to_string(), op.to_string()];
        match child(&args) {
            Ok(v) => outcome_from_json(&v),
            Err(e) => Outcome { error: Some(e), ..Outcome::default() },
        }
    }

    /// [`smlib_sequence`] in a child process.
    fn smlib_sequence_isolated() -> Vec<Outcome> {
        match child(&["--smlib-sequence".to_string()]) {
            Ok(Value::Array(steps)) => steps.iter().map(outcome_from_json).collect(),
            Ok(_) => vec![Outcome { error: Some("unreadable child output".into()), ..Outcome::default() }],
            Err(e) => vec![Outcome { error: Some(e), ..Outcome::default() }],
        }
    }

    fn outcome_json(o: &Outcome) -> Value {
        json!({
            "error": o.error,
            "empty": o.empty,
            "volume": o.volume,
            "faces": o.faces,
            "edges": o.edges,
            "solids": o.solids,
            "history": o.history.map(|h| json!({"faces": h.faces, "edges": h.edges})),
            "ms": o.ms,
            "import_ms": o.import_ms,
            "export_ms": o.export_ms,
            "export_error": o.export_error,
            "export_volume": o.export_volume,
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
        if t.solids != s.solids {
            diffs.push(format!("solids {} vs {}", t.solids, s.solids));
        }
        match (t.history, s.history) {
            (Some(a), Some(b)) if a != b => diffs.push("history".into()),
            (None, Some(_)) => diffs.push("no truck history".into()),
            (Some(_), None) => diffs.push("no SMLib history".into()),
            _ => {}
        }
        if diffs.is_empty() { ("same".into(), "same") } else { (diffs.join(", "), "differs") }
    }

    fn cell(o: &Outcome) -> String {
        match (&o.error, o.empty) {
            (Some(e), _) => format!("error: {}", e.replace('|', "/")),
            (None, true) => "empty".into(),
            (None, false) => {
                format!("{:.3} · {}f {}e {}s · {}", o.volume, o.faces, o.edges, o.solids, o.history.map_or("no history".into(), |h| h.text()))
            }
        }
    }

    /// The export's time, and what went wrong with it or where truck's measure of the exported
    /// body leaves SMLib's own; a dash where SMLib made no body.
    fn exported(o: &Outcome) -> String {
        match &o.export_error {
            Some(e) => format!("{:.1} (failed: {})", o.export_ms, e.replace('|', "/")),
            None if !o.export_volume.is_finite() => "–".into(),
            None if o.error.is_some() || (o.export_volume - o.volume).abs() > VOLUME_REL * o.volume.abs().max(1e-9) => {
                format!("{:.1} (truck measures {:.3})", o.export_ms, o.export_volume)
            }
            None => format!("{:.1}", o.export_ms),
        }
    }

    /// Where `s` holds material along the vertical line through (x, y): one z interval, since
    /// every fuzz primitive (box, axis-aligned cylinder, sphere, torus about +Z) is convex along
    /// z. `None` off it; `Err` for a primitive this can't do (a slanted axis).
    fn z_span(s: &Shape, x: f64, y: f64) -> Result<Option<(f64, f64)>, ()> {
        let half = |r2: f64| (r2 >= 0.0).then(|| r2.sqrt());
        Ok(match *s {
            Shape::Box { a, b } => (x >= a[0] && x <= b[0] && y >= a[1] && y <= b[1]).then_some((a[2], b[2])),
            Shape::Sphere { center: c, radius: r } => half(r * r - (x - c[0]).powi(2) - (y - c[1]).powi(2)).map(|h| (c[2] - h, c[2] + h)),
            Shape::Torus { center: c, major, minor } => {
                let rho = ((x - c[0]).powi(2) + (y - c[1]).powi(2)).sqrt();
                half(minor * minor - (rho - major).powi(2)).map(|h| (c[2] - h, c[2] + h))
            }
            Shape::Cylinder { base: c, axis, radius: r, height: h } => match axis {
                [0.0, 0.0, 1.0] => half(r * r - (x - c[0]).powi(2) - (y - c[1]).powi(2)).map(|_| (c[2], c[2] + h)),
                [1.0, 0.0, 0.0] => {
                    if x < c[0] || x > c[0] + h {
                        None
                    } else {
                        half(r * r - (y - c[1]).powi(2)).map(|d| (c[2] - d, c[2] + d))
                    }
                }
                [0.0, 1.0, 0.0] => {
                    if y < c[1] || y > c[1] + h {
                        None
                    } else {
                        half(r * r - (x - c[0]).powi(2)).map(|d| (c[2] - d, c[2] + d))
                    }
                }
                _ => return Err(()),
            },
        })
    }

    /// The xy box a primitive covers.
    fn xy_bounds(s: &Shape) -> [f64; 4] {
        match *s {
            Shape::Box { a, b } => [a[0], b[0], a[1], b[1]],
            Shape::Sphere { center: c, radius: r } => [c[0] - r, c[0] + r, c[1] - r, c[1] + r],
            Shape::Torus { center: c, major, minor } => {
                let r = major + minor;
                [c[0] - r, c[0] + r, c[1] - r, c[1] + r]
            }
            Shape::Cylinder { base: c, axis, radius: r, height: h } => {
                let lo = |i: usize| c[i] - if axis[i] > 0.5 { 0.0 } else { r };
                let hi = |i: usize| c[i] + if axis[i] > 0.5 { h } else { r };
                [lo(0), hi(0), lo(1), hi(1)]
            }
        }
    }

    /// Cells per side of the reference integration's grid (about; see [`axis_cells`]).
    const GRID: usize = 1600;

    /// Where a primitive's material starts or stops changing smoothly along x (`i` = 0) or y
    /// (`i` = 1): box faces, a cylinder's ends, and where a round outline turns.
    fn breaks(s: &Shape, i: usize) -> Vec<f64> {
        match *s {
            Shape::Box { a, b } => vec![a[i], b[i]],
            Shape::Sphere { center: c, radius: r } => vec![c[i] - r, c[i], c[i] + r],
            Shape::Torus { center: c, major, minor } => {
                let (o, n) = (major + minor, major - minor);
                vec![c[i] - o, c[i] - n, c[i], c[i] + n, c[i] + o]
            }
            Shape::Cylinder { base: c, axis, radius: r, height: h } => {
                if axis[i] > 0.5 {
                    vec![c[i], c[i] + h]
                } else {
                    vec![c[i] - r, c[i], c[i] + r]
                }
            }
        }
    }

    /// Midpoints and widths of cells along one axis over `lo..hi`, about `n` of them, with every
    /// break a cell boundary, so straight faces fall on cell edges and leave no error.
    fn axis_cells(lo: f64, hi: f64, mut cuts: Vec<f64>, n: usize) -> Vec<(f64, f64)> {
        cuts.retain(|x| *x > lo && *x < hi);
        cuts.extend([lo, hi]);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let step = (hi - lo) / n as f64;
        let mut cells = Vec::new();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let k = ((b - a) / step).ceil().max(1.0) as usize;
            let d = (b - a) / k as f64;
            cells.extend((0..k).map(|j| (a + (j as f64 + 0.5) * d, d)));
        }
        cells
    }

    /// The volume of `a op b` on a grid of about `n` × `n` cells (see [`reference_volume`]).
    fn grid_volume(a: &Shape, b: &Shape, op: BoolOp, n: usize) -> Option<f64> {
        let (ba, bb) = (xy_bounds(a), xy_bounds(b));
        let r = [ba[0].min(bb[0]), ba[1].max(bb[1]), ba[2].min(bb[2]), ba[3].max(bb[3])];
        let cut = |i: usize| [breaks(a, i), breaks(b, i)].concat();
        let (xs, ys) = (axis_cells(r[0], r[1], cut(0), n), axis_cells(r[2], r[3], cut(1), n));
        let len = |s: Option<(f64, f64)>| s.map_or(0.0, |(lo, hi)| (hi - lo).max(0.0));
        let mut v = 0.0;
        for &(x, dx) in &xs {
            for &(y, dy) in &ys {
                let (p, q) = (z_span(a, x, y).ok()?, z_span(b, x, y).ok()?);
                let both = match (p, q) {
                    (Some(p), Some(q)) => len(Some((p.0.max(q.0), p.1.min(q.1)))),
                    _ => 0.0,
                };
                v += dx
                    * dy
                    * match op {
                        BoolOp::Union => len(p) + len(q) - both,
                        BoolOp::Intersect => both,
                        BoolOp::Cut => len(p) - both,
                    };
            }
        }
        Some(v)
    }

    /// The volume of `a op b` from the primitives' own definitions, by no kernel, and how far it
    /// may be off: along each vertical line both operands are exact z intervals, combined
    /// exactly, integrated over a grid whose cell edges include every straight face. What error
    /// is left comes from curved outlines; the grid at half the resolution bounds it. `None`
    /// for a primitive [`z_span`] can't do.
    fn reference_volume(a: &Shape, b: &Shape, op: BoolOp) -> Option<(f64, f64)> {
        let fine = grid_volume(a, b, op, GRID)?;
        let coarse = grid_volume(a, b, op, GRID / 2)?;
        Some((fine, (fine - coarse).abs()))
    }

    /// Which kernel's volume is off the reference by more than [`VOLUME_REL`].
    fn graded(o: &Outcome, reference: Option<(f64, f64)>) -> Option<bool> {
        let (r, uncertain) = reference?;
        if o.error.is_some() {
            return None;
        }
        let v = if o.empty { 0.0 } else { o.volume };
        Some((v - r).abs() <= (VOLUME_REL * r.abs()).max(3.0 * uncertain).max(1e-6))
    }

    /// How far a measured volume lies from the analytic one, relative; `None` when it made none.
    fn off(o: &Outcome, analytic: f64) -> Option<f64> {
        if o.error.is_some() {
            return None;
        }
        let v = if o.empty { 0.0 } else { o.volume };
        Some(if analytic == 0.0 { v.abs() } else { (v / analytic - 1.0).abs() })
    }

    /// Median, worst and total of `xs` (ms), or dashes when empty.
    fn stats(xs: &mut Vec<f64>) -> String {
        xs.retain(|x| x.is_finite());
        if xs.is_empty() {
            return "– | – | –".into();
        }
        xs.sort_by(f64::total_cmp);
        let median = xs.get(xs.len() / 2).copied().unwrap_or(f64::NAN);
        let worst = xs.last().copied().unwrap_or(f64::NAN);
        format!("{median:.2} | {worst:.1} | {:.0}", xs.iter().sum::<f64>())
    }

    pub fn main() -> Result<(), String> {
        let mut seeds = 32u64;
        let mut out = PathBuf::from("target/smlib-booleans");
        let mut child_case = None;
        let mut child_sequence = false;
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--seeds" => seeds = args.next().and_then(|n| n.parse().ok()).ok_or("--seeds takes a number")?,
                "--out" => out = args.next().map(PathBuf::from).ok_or("--out takes a directory")?,
                "--smlib-case" => {
                    let mut n = || args.next().and_then(|n| n.parse::<usize>().ok()).ok_or("--smlib-case takes a case and an operation");
                    child_case = Some((n()?, n()?));
                }
                "--smlib-sequence" => child_sequence = true,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        let mut cases = named();
        cases.extend((0..seeds).map(|s| {
            let (a, b) = boolean_shapes(s);
            (format!("seed {s}"), a, b, None)
        }));
        if let Some((index, op)) = child_case {
            let ((_, a, b, _), op) = cases.get(index).zip(OPS.get(op)).ok_or("no such case")?;
            solidcraft_smlib::init().map_err(|e| e.to_string())?;
            println!("{}", outcome_json(&smlib(a, b, *op)));
            return Ok(());
        }
        let steps = sequence();
        if child_sequence {
            solidcraft_smlib::init().map_err(|e| e.to_string())?;
            println!("{}", Value::Array(smlib_sequence(&steps).iter().map(outcome_json).collect()));
            return Ok(());
        }
        solidcraft_smlib::init().map_err(|e| e.to_string())?;

        let mut rows = Vec::new();
        let mut md = String::new();
        let mut tally = std::collections::BTreeMap::<&str, usize>::new();
        let mut grades = std::collections::BTreeMap::<(&str, &str), usize>::new();
        let (mut truck_ms, mut smlib_ms, mut import_ms, mut export_ms) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut truck_fail, mut smlib_fail, mut export_fail) = (0usize, 0usize, 0usize);
        let (mut analytic_ops, mut truck_exact, mut smlib_exact) = (0usize, 0usize, 0usize);
        for (index, (name, a, b, analytic)) in cases.iter().enumerate() {
            for (k, op) in OPS.into_iter().enumerate() {
                let t = truck(a, b, op);
                let s = smlib_isolated(index, k, seeds);
                let (what, class) = verdict(&t, &s);
                *tally.entry(class).or_default() += 1;
                let want = analytic.and_then(|v| v.get(k).copied());
                // Rows without an analytic volume (the seeds): a reference from the primitives'
                // definitions, each kernel graded against it.
                let reference = if want.is_none() { reference_volume(a, b, op) } else { None };
                let grade = |o: &Outcome| match graded(o, reference) {
                    Some(true) => "ok",
                    Some(false) => "WRONG",
                    None => "-",
                };
                let (gt, gs) = (grade(&t), grade(&s));
                if reference.is_some() {
                    *grades.entry(("truck", gt)).or_default() += 1;
                    *grades.entry(("SMLib", gs)).or_default() += 1;
                }
                let what = if gt == "WRONG" || gs == "WRONG" {
                    format!(
                        "{what}; volume vs reference {:.3} ± {:.3}: truck {gt}, SMLib {gs}",
                        reference.map_or(f64::NAN, |r| r.0),
                        reference.map_or(f64::NAN, |r| r.1)
                    )
                } else {
                    what
                };
                if let Some(w) = want {
                    analytic_ops += 1;
                    truck_exact += usize::from(off(&t, w).is_some_and(|d| d <= VOLUME_REL));
                    smlib_exact += usize::from(off(&s, w).is_some_and(|d| d <= 1e-6));
                }
                truck_fail += usize::from(t.error.is_some());
                smlib_fail += usize::from(s.error.is_some());
                export_fail += usize::from(s.export_error.is_some());
                if t.error.is_none() {
                    truck_ms.push(t.ms);
                }
                if s.error.is_none() {
                    smlib_ms.push(s.ms);
                    import_ms.push(s.import_ms);
                    if !s.empty {
                        export_ms.push(s.export_ms);
                    }
                }
                let want_text = want.map_or("–".into(), |w| if w == 0.0 { "empty".into() } else { format!("{w:.3}") });
                let back = exported(&s);
                let _ = writeln!(
                    md,
                    "| {name} | {} | {want_text} | {} | {} | {what} | {:.1} | {:.1} | {:.1} | {back} |",
                    op_name(op),
                    cell(&t),
                    cell(&s),
                    t.ms,
                    s.ms,
                    s.import_ms,
                );
                rows.push(json!({
                    "case": name, "a": a, "b": b, "op": op_name(op), "analytic": want,
                    "truck": outcome_json(&t), "smlib": outcome_json(&s),
                    "verdict": class, "detail": what,
                    "reference_volume": reference.map(|r| r.0), "reference_uncertainty": reference.map(|r| r.1),
                    "truck_volume_grade": gt, "smlib_volume_grade": gs,
                }));
            }
        }

        let truck_steps = truck_sequence(&steps);
        let smlib_steps = smlib_sequence_isolated();
        let mut seq_md = String::new();
        let mut seq_json = Vec::new();
        for (i, s) in steps.iter().enumerate() {
            let (t, m) = (truck_steps.get(i), smlib_steps.get(i));
            let ms = |o: Option<&Outcome>| o.filter(|_| s.op.is_some()).map_or("–".into(), |o| format!("{:.1}", o.ms));
            let vol = |o: Option<&Outcome>| match o {
                None => "not run".into(),
                Some(o) => match &o.error {
                    Some(e) => format!("error: {}", e.replace('|', "/")),
                    None => format!("{:.3} ({}f {}e {}s)", o.volume, o.faces, o.edges, o.solids),
                },
            };
            let export = m.filter(|_| s.op.is_some()).map_or("–".into(), exported);
            let _ = writeln!(
                seq_md,
                "| {} | {:.3} ({}s) | {} | {} | {} | {} | {} | {export} |",
                s.name,
                s.volume,
                s.solids,
                vol(t),
                vol(m),
                ms(t),
                ms(m),
                m.map_or("–".into(), |o| format!("{:.1}", o.import_ms)),
            );
            seq_json.push(json!({
                "step": s.name, "op": s.op.map(op_name), "analytic": s.volume, "analytic_solids": s.solids,
                "truck": t.map(outcome_json), "smlib": m.map(outcome_json),
            }));
        }

        let summary: Vec<String> = tally.iter().map(|(k, v)| format!("{k} {v}")).collect();
        let ops = cases.len() * OPS.len();
        let mut report = String::new();
        let _ = writeln!(report, "# Booleans: truck and SMLib side by side\n");
        let _ = writeln!(report, "{} cases × 3 operations: {}.\n", cases.len(), summary.join(", "));
        let grade_text: Vec<String> = grades.iter().map(|((k, g), n)| format!("{k} {g} {n}")).collect();
        let _ = writeln!(
            report,
            "Rows without an analytic volume, against a reference from the primitives' definitions (exact z \
             intervals on a grid of about {GRID}² cells with every straight face on a cell edge; within \
             {VOLUME_REL:e} relative or 3× the grid's own error bound): {}.\n",
            grade_text.join(", ")
        );
        let _ = writeln!(
            report,
            "Failed: truck {truck_fail}/{ops}, SMLib {smlib_fail}/{ops}; SMLib results that don't export back to truck \
             (or that truck can't measure): {export_fail}. Against the {analytic_ops} analytic volumes: truck within {VOLUME_REL:e} on {truck_exact}, \
             SMLib within 1e-6 on {smlib_exact}.\n"
        );
        let _ = writeln!(report, "## Times (ms, operations that didn't fail)\n");
        let _ = writeln!(report, "| | median | worst | total |\n|---|---|---|---|");
        let _ = writeln!(report, "| truck boolean | {} |", stats(&mut truck_ms));
        let _ = writeln!(report, "| SMLib boolean | {} |", stats(&mut smlib_ms));
        let _ = writeln!(report, "| SMLib import (truck→SMLib, both operands) | {} |", stats(&mut import_ms));
        let _ = writeln!(report, "| SMLib export (SMLib→truck, result) | {} |\n", stats(&mut export_ms));
        let _ = writeln!(
            report,
            "## Cut sequence\n\nEach step on the previous step's result, each kernel on its own (SMLib's results stay in SMLib; \
             only the tools come from truck). Volumes in mm³ (faces, edges, solids); times in ms.\n"
        );
        let _ = writeln!(
            report,
            "| step | analytic | truck | SMLib | truck ms | SMLib ms | SMLib import ms | SMLib export ms |\n|---|---|---|---|---|---|---|---|"
        );
        report.push_str(&seq_md);
        let _ = writeln!(report, "\n## Cases\n");
        let _ = writeln!(
            report,
            "Volume · faces, edges, solids · history shape: faces **K**ept, **M**odified, **S**plit, **J**oined (merged), \
             **G**enerated; edges from an operand **e**dge, new where faces meet (**x**), without origin (**-**). \
             Times in ms: the boolean on each kernel, then SMLib's import of the operands and export of the result.\n"
        );
        let _ = writeln!(
            report,
            "| case | op | analytic | truck | SMLib | verdict | truck ms | SMLib ms | import ms | export ms |\n\
             |---|---|---|---|---|---|---|---|---|---|"
        );
        report.push_str(&md);

        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let json = json!({ "cases": cases.len(), "tally": tally, "reference_grades": grades.iter().map(|((k, g), n)| json!({"kernel": k, "grade": g, "count": n})).collect::<Vec<_>>(), "rows": rows, "sequence": seq_json });
        let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
        std::fs::write(out.join("smlib-booleans.json"), text).map_err(|e| e.to_string())?;
        std::fs::write(out.join("smlib-booleans.md"), &report).map_err(|e| e.to_string())?;
        println!("{} cases × 3: {}; against the reference: {} ({})", cases.len(), summary.join(", "), grade_text.join(", "), out.display());
        Ok(())
    }
}
