//! The cases of #2583, pinned against today's reader so that the port into the build of
//! #2578 has an acceptance test: a body built from the file's own topology
//! ([`step_brep::build_body`], `BrepConstructor`) where building each face and stitching
//! is not the way, and what the stitch-only build ([`BuildMode::StitchOnly`]) gives on
//! the same body.
//!
//! Expected values are the file's own, never the reader's output: the exporter's
//! geometric validation volume (read through `step_import`'s parser, see [`validation`],
//! shared with #2580's test), a closed form, and counts from the file's census.
//!
//! | file | bodies | faces | closed | volume expected (mm³) | bound | today: topology first / stitch only |
//! |---|---|---|---|---|---|---|
//! | `fixtures/rod_doubled_edge.step` (SolidCraft) | 1 | 7 | 1 | 74 900.620862 (closed form) | 1e-9 | 7.753e-6 / 7.175e-7, known failing; Level 2 fails (33 / 22) |
//! | `AP203 geometry only/nist_ctc_02_asme1_rc` | 2 | 664 | 1 | `4.710152747498E7` (validation) | 1e-9 + 1.1e-13 | 3.613e-6 / 8.296e-7, known failing |
//! | `AP203 with PMI/nist_ctc_02_asme1_ap203` | 1 | 487 | 1 | `rc`'s (none in the file) | 1e-9 + 1.1e-13 | not built from its topology (face `#1152`, #2928) / 7.601e-4, known failing |
//! | `nist_ctc_02_asme1_ap242-e2` | 3 | 637 | 1 | `47106788.0321802` (validation) | 1e-9 + 1.1e-15 | 9.034e-7 / 1.095e-6, known failing |
//!
//! The extra bodies are one-face `SHELL_BASED_SURFACE_MODEL`s, open sheets.
//!
//! **`rod_doubled_edge`** comes from SolidCraft's `tests/step-regressions/` (MIT OR
//! Apache-2.0; written by SolidCraft from the public commands of SolidCraft #4598): a
//! Ø40 × 60 rod with a Ø8 cross bore, whose bore rim was exported as a B-spline of 1 399
//! control points running back and forth. Its volume in closed form is
//! [`rod_closed_form`].
//!
//! **Bounds.** As in #2580's test: the accuracy the test asks the integrator for,
//! [`ACCURACY`] = 1e-9 (its doc says what the argument controls), plus, against a
//! validation volume, half a unit of the last digit the exporter wrote, relative
//! ([`validation_bound`]): `4.710152747498E7` is 5e-6 / 4.71e7 = 1.1e-13,
//! `47106788.0321802` 5e-8 / 4.71e7 = 1.1e-15. No bound is chosen from a measurement.
//! On #2580's files the volume at 1e-9 is within 7e-11 of the volume at 1e-11, so the
//! deviations below are not the integrator's.
//!
//! **The rod's bound** is the accuracy alone, the tightest the closed form allows. The
//! rim term measured from the file's curve was to be added to it, but the file's rim is
//! no approximation of the exact rim (the bore cylinder's intersection with the rod's
//! side): sampled per knot span it runs up to 3.6 mm off the bore cylinder, across the
//! bore's disk and back near its axis, while staying within 0.06 mm of the rod's side
//! ([`rod_rim_runs_off_the_exact_rim`]). A volume bound from that deviation would be of
//! the order of the bore itself and would pass any build, so no rim term is added and
//! both builds are held known failing on the volume, beside their face, closure and
//! Level 2 checks. The Level 2 check fails on both (the rim's edges carry tolerances up
//! to 8.2 mm and 17.4 mm).
//!
//! **`ctc_02_ap203`** writes no validation volume. Its geometry-only export `rc` is the
//! same part by [`validation::same_form`] (box, radii, and every corner of ap203 at a
//! vertex of rc). `ap242-e2` is another revision (its cylinders of radius 0.41 to 4.6 mm
//! are missing, radius 8 is new). So `rc`'s volume is required: built from the topology
//! (`BuildPath::Topology`), one closed solid, within the bound. That is known failing:
//! the topology build leaves out face `#1152`, a cone whose STEP domain
//! `AdjustSTEPUVDomain` refuses ("end angle outside [-360, 360]",
//! [`ctc_02_ap203_builds_from_its_topology_at_its_volume_known_failing`], #2928). The
//! stitch-only build closes 7.6e-4 below `rc`'s volume. That is too far for a reader
//! error alone, but whether ap203's surfaces are rc's to that precision is not known
//! while its topology build fails.
//!
//! **Follow-up:** the validation helper reads through `step_import::part21` until #2591
//! lands.
//!
//! The corpus tests are ignored by default; the corpus is not in the repository. Fetch it
//! with `scripts/fetch-nist-pmi-corpus.sh` and run them with its directory (or one above
//! it) in `SOLIDCORE_STEP_CORPUS` (searched recursively):
//!
//! ```sh
//! SOLIDCORE_STEP_CORPUS=corpus cargo test --release -p step-brep \
//!     --test issue_2583_topology_first -- --include-ignored --nocapture
//! ```

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)] // test helpers

#[path = "support/validation.rs"]
mod validation;

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::Brep;
use step_brep::{
    build_body, build_from_topology, is_closed_solid, BuildMode, BuildOptions, BuildPath, BuiltBody,
};
use step_import::Limits;

use validation::{corpus_dir, corpus_file, relative, validation_bound, ACCURACY};

/// What a build gives against its expected volume.
#[derive(Debug, Clone, Copy)]
enum State {
    /// Within the bound. Every case is known failing today; a fixed one becomes this.
    #[allow(dead_code)]
    Within,
    /// Beyond it: today's relative deviation, as measured.
    KnownFailing(f64),
}

/// The rod's bound against its closed form: the integrator's [`ACCURACY`] alone. The
/// rim term from the file's curve is not added: the curve runs up to 3.6 mm off the
/// exact rim ([`rod_rim_runs_off_the_exact_rim`], the module docs).
const ROD_BOUND: f64 = ACCURACY;

/// The rod's volume against its closed form today: (topology first, stitch only).
const ROD_STATE: (State, State) = (State::KnownFailing(7.753e-6), State::KnownFailing(7.175e-7));

/// SolidCraft's `tests/step-regressions/rod_doubled_edge.step`.
const ROD: &[u8] = include_bytes!("fixtures/rod_doubled_edge.step");

/// The volume of `rod_doubled_edge` in closed form, from the file's geometry.
///
/// The rod is the cylinder `#1713` of radius 20 about the z axis through (100, 0)
/// (its side is split into that cylinder and the rational quadratic B-spline `#1688` on
/// the same circle), from the plane z = 0 (`#1730`) to z = 60 (`#1658`): π · 20² · 60. The
/// bore is a cylinder of radius 4 about the line y = 0, z = 30 along x (the rational
/// quadratic B-spline faces `#32` and `#1202`, whose control points lie at distance 4),
/// from its flat bottom, the plane x = 110 (`#1649`), out through the rod's side at
/// x = 100 + √(400 − y²). Its volume is the integral of that depth over the bore's disk:
///
/// `∫∫ (√(400 − y²) − 10) dy dz` over `y² + (z − 30)² ≤ 16`
/// `= ∫_{−π/2}^{π/2} 32 cos²θ √(400 − 16 sin²θ) dθ − 10 · 16π` (y = 4 sin θ),
///
/// an elliptic integral, evaluated here by the trapezoidal rule over a full period of its
/// smooth periodic integrand, which converges geometrically: 32 points already agree
/// with 128 to the last bit.
fn rod_closed_form() -> f64 {
    use std::f64::consts::PI;
    let n = 128_u32;
    let step = 2.0 * PI / f64::from(n);
    let wall: f64 = (0..n)
        .map(|k| {
            let t = step * f64::from(k);
            16.0 * t.cos() * t.cos() * (400.0 - 16.0 * t.sin() * t.sin()).sqrt()
        })
        .sum::<f64>()
        * step;
    let bore = wall - 10.0 * 16.0 * PI;
    PI * 400.0 * 60.0 - bore
}

/// The volume of `brep`, or `None` where the integrator gives none.
fn volume(brep: &Brep) -> Option<f64> {
    let kernel = Kernel::default();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let mut copy = brep.clone();
    brep_volume(&kernel, &mut copy, ACCURACY, &attrs).ok()
}

/// Every body of `bytes` built in `mode`.
fn build_all(bytes: &[u8], mode: BuildMode) -> Vec<BuiltBody> {
    let model = step_import::import(bytes, &Limits::default()).expect("imports");
    assert!(model.skipped.is_empty(), "skipped: {:?}", model.skipped);
    let options = BuildOptions {
        mode,
        ..BuildOptions::default()
    };
    model
        .bodies
        .iter()
        .map(|body| build_body(body, options).expect("builds"))
        .collect()
}

/// (bodies, faces, closed bodies, summed volume of the closed bodies) of `built`.
fn summary(built: &[BuiltBody]) -> (usize, usize, usize, f64) {
    let closed: Vec<&BuiltBody> = built.iter().filter(|b| is_closed_solid(&b.brep)).collect();
    (
        built.len(),
        built.iter().map(|b| b.brep.face_list().len()).sum(),
        closed.len(),
        closed
            .iter()
            .map(|b| volume(&b.brep).expect("a closed body has a volume"))
            .sum(),
    )
}

/// The census (bodies, faces) of `bytes` ([`validation::body_census`]).
fn census(bytes: &[u8]) -> (usize, usize) {
    let census = validation::body_census(&validation::parse(bytes));
    (census.len(), census.iter().map(|(_, _, faces)| faces).sum())
}

#[test]
fn rod_closed_form_is_the_bored_rod() {
    let v = rod_closed_form();
    assert!((v - 74900.62086234294).abs() <= 1e-9 * v, "{v}");
}

/// The Level 2 failures of `brep` (`check_brep`).
fn level2_failures(brep: &Brep) -> usize {
    let mut copy = brep.clone();
    let mut kernel = Kernel::default();
    check_brep(&mut kernel, &mut copy, CheckLevel::Level2)
        .expect("checks")
        .failures()
        .count()
}

/// The rod's Level 2 check fails today (known failing); fails once it passes, so that
/// the check becomes a required one.
fn rod_level2_known_failing(what: &str, brep: &Brep) {
    let failures = level2_failures(brep);
    println!("{what}: {failures} Level 2 failures");
    assert!(
        failures > 0,
        "{what} passes its Level 2 check now: require it"
    );
}

/// Checks `measured` against `expected` within `bound` in its `state`; a failure is
/// returned as a message.
fn against(what: &str, measured: f64, expected: f64, bound: f64, state: State) -> Option<String> {
    let deviation = relative(measured, expected);
    println!(
        "{what}: {measured:.6} mm³, expected {expected:.6}, {deviation:.3e} relative, \
         bound {bound:.3e}"
    );
    match state {
        State::Within if deviation > bound => Some(format!(
            "{what}: {measured} mm³, expected {expected}, {deviation:.3e} relative, bound \
             {bound:.3e}"
        )),
        State::KnownFailing(today) if !deviation.is_finite() => Some(format!(
            "{what}: no measurable volume ({measured} mm³), so the known failure ({today:.2e}) \
             is not the one pinned"
        )),
        State::KnownFailing(today) if deviation <= bound => Some(format!(
            "{what} is within its bound now ({deviation:.3e} <= {bound:.3e}, was {today:.2e}); \
             mark it State::Within"
        )),
        _ => None,
    }
}

/// `rod_doubled_edge` builds from its topology into one closed solid of seven faces; its
/// volume against the closed form is [`ROD_STATE`]'s first. **Known failing:** its Level 2
/// check fails (33 failures today: the rim's edges and vertices carry tolerances up to
/// 8.2 mm, an edge/edge gap of 7.6 mm), see [`rod_level2_known_failing`].
#[test]
fn rod_doubled_edge_builds_topology_first() {
    assert_eq!(census(ROD), (1, 7));
    let built = build_all(ROD, BuildMode::Default);
    let (bodies, faces, closed, measured) = summary(&built);
    assert_eq!((bodies, faces, closed), (1, 7, 1));
    assert_eq!(built[0].path, BuildPath::Topology);
    assert!(
        built[0].face_failures.is_empty(),
        "{:?}",
        built[0].face_failures
    );
    rod_level2_known_failing("rod topology first", &built[0].brep);
    let failure = against(
        "rod topology first",
        measured,
        rod_closed_form(),
        ROD_BOUND,
        ROD_STATE.0,
    );
    assert!(failure.is_none(), "{failure:?}");
}

/// What the stitch-only build gives on `rod_doubled_edge` today: one closed solid of seven
/// faces; its volume is [`ROD_STATE`]'s second. **Known failing:** its Level 2 check fails
/// too (22 failures today: an edge tolerance of 17.4 mm).
#[test]
fn rod_doubled_edge_stitch_only() {
    let built = build_all(ROD, BuildMode::StitchOnly);
    let (bodies, faces, closed, measured) = summary(&built);
    assert_eq!((bodies, faces, closed), (1, 7, 1));
    assert_eq!(built[0].path, BuildPath::Stitch);
    rod_level2_known_failing("rod stitch only", &built[0].brep);
    let failure = against(
        "rod stitch only",
        measured,
        rod_closed_form(),
        ROD_BOUND,
        ROD_STATE.1,
    );
    assert!(failure.is_none(), "{failure:?}");
}

/// The point of a non-rational B-spline `curve` at `u` (de Boor's algorithm over its
/// knot vector; `u` inside the curve's domain).
fn de_boor(curve: &step_import::model::BSplineCurve, u: f64) -> [f64; 3] {
    let p = curve.degree as usize;
    let knots: Vec<f64> = curve
        .knots
        .iter()
        .zip(&curve.mults)
        .flat_map(|(&k, &m)| std::iter::repeat_n(k, m as usize))
        .collect();
    let n = curve.points.len() - 1;
    let span = (p..=n).rev().find(|&i| knots[i] <= u).unwrap_or(p);
    let mut d: Vec<[f64; 3]> = (0..=p).map(|j| curve.points[j + span - p]).collect();
    for r in 1..=p {
        for j in (r..=p).rev() {
            let i = j + span - p;
            let a = (u - knots[i]) / (knots[i + 1 + p - r] - knots[i]);
            let previous = d[j - 1];
            for (value, before) in d[j].iter_mut().zip(previous) {
                *value = (1.0 - a) * before + a * *value;
            }
        }
    }
    d[p]
}

/// The file's bore rim, edges `#1174` and `#1640` (cubic B-splines `#1171` of 1 399
/// control points and `#1639` of 433), is not an approximation of the exact rim, the
/// intersection of the bore cylinder (radius 4 about y = 0, z = 30) with the rod's side
/// (radius 20 about x = 100, y = 0): sampled at eight points per knot span, it runs up to
/// 3.6 mm off the bore cylinder (across the bore's disk and back, near its axis) while
/// staying within 0.06 mm of the rod's side. A volume bound from that deviation would be
/// of the order of the bore's volume, so [`ROD_BOUND`] adds no rim term. The sampled
/// maxima are lower bounds of the curves' deviation.
#[test]
fn rod_rim_runs_off_the_exact_rim() {
    let model = step_import::import(ROD, &Limits::default()).expect("imports");
    let body = &model.bodies[0];
    let (mut off_bore, mut off_rod) = (0.0_f64, 0.0_f64);
    for id in [1174, 1640] {
        let edge = body
            .edges
            .iter()
            .find(|e| e.step_id == id)
            .unwrap_or_else(|| panic!("edge #{id}"));
        let step_import::model::Curve::BSpline(curve) = &edge.curve else {
            panic!("edge #{id} is not a B-spline");
        };
        assert!(curve.weights.is_none(), "edge #{id}: rational");
        for pair in curve.knots.windows(2) {
            for s in 0..8 {
                let u = pair[0] + (pair[1] - pair[0]) * (f64::from(s) + 0.5) / 8.0;
                let [x, y, z] = de_boor(curve, u);
                off_bore = off_bore.max(((y * y + (z - 30.0) * (z - 30.0)).sqrt() - 4.0).abs());
                off_rod = off_rod.max((((x - 100.0) * (x - 100.0) + y * y).sqrt() - 20.0).abs());
            }
        }
    }
    println!("rim: up to {off_bore:.4} mm off the bore cylinder, {off_rod:.4} mm off the rod");
    assert!(off_bore >= 3.6, "{off_bore}");
    assert!(off_rod <= 0.06, "{off_rod}");
}

/// The validation volume of `name` in the corpus `dir` (its bytes) and its bound.
fn validation_of(name: &str, bytes: &[u8]) -> (f64, f64) {
    let volume = validation::validation_volume(&validation::parse(bytes), bytes)
        .unwrap_or_else(|error| panic!("{name}: {error}"))
        .unwrap_or_else(|| panic!("{name}: no validation volume"));
    (volume.mm3, validation_bound(ACCURACY, &volume))
}

/// `ctc_02`'s two exports with a validation volume, built topology first (the default)
/// and stitched only: the bodies and faces of their census, one closed solid by the
/// expected path, and the volume against the file's within its bound (the integrator's
/// [`ACCURACY`] plus the digits written, [`validation_bound`]) or known failing.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn ctc_02_builds_both_ways_within_their_bound_or_known_failing() {
    // (file, bodies, faces, (default, stitch only) volume state)
    const CASES: [(&str, usize, usize, (State, State)); 2] = [
        (
            "nist_ctc_02_asme1_rc.stp",
            2,
            664,
            (State::KnownFailing(3.613e-6), State::KnownFailing(8.296e-7)),
        ),
        (
            "nist_ctc_02_asme1_ap242-e2.stp",
            3,
            637,
            (State::KnownFailing(9.034e-7), State::KnownFailing(1.095e-6)),
        ),
    ];
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let mut failures = Vec::new();
    for (name, bodies, faces, states) in CASES {
        let bytes = corpus_file(&dir, name);
        if census(&bytes) != (bodies, faces) {
            failures.push(format!("{name}: census {:?}", census(&bytes)));
        }
        let (expected, bound) = validation_of(name, &bytes);
        for (mode, path, state) in [
            (BuildMode::Default, BuildPath::Topology, states.0),
            (BuildMode::StitchOnly, BuildPath::Stitch, states.1),
        ] {
            let built = build_all(&bytes, mode);
            let (got_bodies, got_faces, closed, measured) = summary(&built);
            let solid_path = built
                .iter()
                .find(|b| is_closed_solid(&b.brep))
                .map(|b| b.path);
            println!("{name} {mode:?}: ({got_bodies}, {got_faces}, {closed}) solid {solid_path:?}");
            if (got_bodies, got_faces, closed) != (bodies, faces, 1) {
                failures.push(format!(
                    "{name} {mode:?}: (bodies, faces, closed) ({got_bodies}, {got_faces}, \
                     {closed}), expected ({bodies}, {faces}, 1)"
                ));
            }
            if solid_path != Some(path) {
                failures.push(format!("{name} {mode:?}: solid built {solid_path:?}"));
            }
            failures.extend(against(
                &format!("{name} {mode:?}"),
                measured,
                expected,
                bound,
                state,
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The reason the topology build of `ctc_02_ap203` gives for face `#1152` today: the
/// cone's STEP domain (u −180.0000000000005…179.99999999999955°, a whole turn whose ends
/// lie on one half-line) is refused, as its sweep circle is built a turn above it (#2928).
const FACE_1152: &str = "AdjustSTEPUVDomain: nurbs::b_spline_curve::p1::BSplineCurve::\
     create_circle_segment: invalid input: end angle outside [-360, 360]";

/// `ctc_02`'s AP203 export with PMI writes no validation volume; its geometry-only export
/// `rc` is the same part ([`validation::same_form`]) and writes `4.710152747498E7` mm³.
/// The default build must build it from its topology, one closed solid of its 487
/// faces, at that volume within its bound.
///
/// **Known failing:** the topology build leaves out face `#1152` ([`FACE_1152`]), so the
/// body does not close and [`build_body`] does not keep the topology build. Fails once it
/// does: then drop the known failure and require the volume.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn ctc_02_ap203_builds_from_its_topology_at_its_volume_known_failing() {
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let name = "nist_ctc_02_asme1_ap203.stp";
    let bytes = corpus_file(&dir, name);
    assert_eq!(census(&bytes), (1, 487));
    let file = validation::parse(&bytes);
    assert_eq!(
        validation::validation_volume(&file, &bytes).expect("units"),
        None
    );
    let sister = "nist_ctc_02_asme1_rc.stp";
    let sister_bytes = corpus_file(&dir, sister);
    validation::same_form(
        &validation::form(&file).expect("form"),
        &validation::form(&validation::parse(&sister_bytes)).expect("form"),
    )
    .unwrap_or_else(|error| panic!("{name} and {sister} are not one part: {error}"));
    let (expected, bound) = validation_of(sister, &sister_bytes);

    let built = build_all(&bytes, BuildMode::Default);
    let (bodies, faces, closed, measured) = summary(&built);
    let deviation = relative(measured, expected);
    println!(
        "{name} default: ({bodies}, {faces}, {closed}) by {:?}, {measured:.6} mm³, {sister}'s \
         {expected:.6}, {deviation:.3e} relative, bound {bound:.3e}",
        built[0].path
    );
    let met = built[0].path == BuildPath::Topology
        && (bodies, faces, closed) == (1, 487, 1)
        && deviation <= bound;

    let model = step_import::import(&bytes, &Limits::default()).expect("imports");
    let topology = build_from_topology(&model.bodies[0], BuildOptions::default()).expect("builds");
    let failed: Vec<(u64, &str)> = topology
        .face_failures
        .iter()
        .map(|f| (f.face, f.reason.as_str()))
        .collect();
    println!(
        "topology build: {} faces, closed {}, failures {failed:?}",
        topology.brep.face_list().len(),
        is_closed_solid(&topology.brep)
    );
    assert!(
        !met,
        "{name} builds from its topology at its volume now: drop the known failure"
    );
    assert_eq!(
        (
            failed,
            topology.brep.face_list().len(),
            is_closed_solid(&topology.brep)
        ),
        (vec![(1152, FACE_1152)], 486, false),
        "the known failure changed"
    );
}

/// What the stitch-only build gives on `ctc_02_ap203`: one closed solid of its 487 faces;
/// its volume against `rc`'s validation volume within its bound, or known failing.
#[test]
#[ignore = "needs SOLIDCORE_STEP_CORPUS"]
fn ctc_02_ap203_stitch_only() {
    // 7.6e-4 below rc's: rc is the same part by its corners and radii, but whether its
    // surfaces are ap203's to that precision is not known while ap203's topology build
    // fails (the module docs)
    const STATE: State = State::KnownFailing(7.601e-4);
    let Some(dir) = corpus_dir() else {
        eprintln!("SOLIDCORE_STEP_CORPUS is not set");
        return;
    };
    let name = "nist_ctc_02_asme1_ap203.stp";
    let bytes = corpus_file(&dir, name);
    let sister = "nist_ctc_02_asme1_rc.stp";
    let (expected, bound) = validation_of(sister, &corpus_file(&dir, sister));
    let built = build_all(&bytes, BuildMode::StitchOnly);
    let (bodies, faces, closed, measured) = summary(&built);
    assert_eq!((bodies, faces, closed), (1, 487, 1));
    assert_eq!(built[0].path, BuildPath::Stitch);
    let failure = against(
        &format!("{name} stitch only"),
        measured,
        expected,
        bound,
        STATE,
    );
    assert!(failure.is_none(), "{failure:?}");
}
