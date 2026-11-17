//! The certificate of the goldens rows whose only change under the lazy UV trim's raise
//! (`topology::edgeuse::p3::trim_raise_distance`) is a lower tolerance: for each moved edge
//! or vertex tolerance `n` (it was `o`), `g <= B <= n < o`.
//!
//! [`ROWS`] holds, one line per case, the moved output values (unit, case, `[key, index, o,
//! n]`, the mechanical diff of the two goldens) and the moved entities (`[body, "e"|"v",
//! index, o, n]`, the edges and vertices of the B-reps the runner hands to [`super::written`]
//! whose tolerance differs between the two kernels, matched by entity, never by value). Here
//! each moved output value is checked bit for bit against the case's output and must be the
//! `o -> n` of a moved entity, and each moved entity's tolerance must be `n` and certify:
//! - an edge: `B` is [`trim_image_gap_bound`] of each of its UV trims, a proved upper bound on
//!   the Hausdorff distance between the edge curve and the trim's image on its face (so on the
//!   true gap `g`); an edge whose B-rep keeps no UV trim has `B` the proved bound
//!   ([`curve_face_gap_bound`]) on the distance from its curve to each of its faces' surfaces;
//! - a vertex: its point is within `n` of the end of every edge curve that ends there, the
//!   rounding of each distance counted against it.
//!
//! So every lowered tolerance still covers what it is the tolerance for, and it is lower than
//! before: a tighter tolerance that is still valid.

use solidcore::curve::curve::p2::CurveKind;
use solidcore::kernel::Kernel;
use solidcore::nurbs::math::gamma;
use solidcore::query::curve_surface_gap::{curve_face_gap_bound, trim_image_gap_bound};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, Brep, EdgeId, VertexId};

use crate::case::repo_root;
use crate::CaseContext;

/// The moved values of each case whose moved values are all lower tolerances (below 0.05,
/// positive, decreased) and whose output changed nowhere else.
const ROWS: &str = include_str!("trim_tolerance_rows.jsonl");

/// The moved edges this certificate does not certify yet, one per line: unit, case, body,
/// edge, class, and (diagnostic only, never part of a proof) a dense-sampling estimate of the
/// edge's largest trim gap and its new tolerance. Classes:
/// - `UNCOVERED`: the sampled gap of one of the edge's trims exceeds the new (and the old)
///   tolerance: an open defect, the tolerance does not cover that trim either way;
/// - `loose`: the sampled gap is within the new tolerance but the enclosure `B` is not;
/// - `no-estimate`: the trim's image could not be sampled;
/// - `face-bound-loose`: an edge whose B-rep keeps no UV trim, whose face-gap enclosure
///   exceeds the new tolerance.
///
/// The test requires the uncertified edges to be exactly these, so an entry that becomes
/// certified (or a new failure) shows up.
const OPEN: &str = include_str!("trim_tolerance_open.tsv");

/// [`OPEN`] entries of `unit`: `(case, body, edge)`.
fn open_entries(unit: &str) -> std::collections::BTreeSet<(String, usize, usize)> {
    OPEN.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.first() == Some(&unit)).then(|| {
                (
                    f[1].to_string(),
                    f[2].parse().expect("body"),
                    f[3].parse().expect("edge"),
                )
            })
        })
        .collect()
}

/// The kernel's floor of the bound, as a fraction of the edge's tolerance
/// (`topology::edgeuse::p3::TRIM_BOUND_FLOOR`): below it a tighter bound changes nothing.
const FLOOR: f64 = 1.0e-3;

/// One case's moved values and the B-rep entities whose tolerance moved.
struct Row {
    case: String,
    /// `(key, index, old, new)` of each moved output value.
    moved: Vec<(String, usize, f64, f64)>,
    /// `(body, edge?, index, old, new)` of each moved edge or vertex tolerance: the B-rep
    /// (in the order the runner hands them to [`super::written`]), whether it is an edge
    /// (else a vertex), its index, and its tolerance before and after.
    entities: Vec<(usize, bool, usize, f64, f64)>,
}

/// [`ROWS`] of `unit`.
fn rows(unit: &str) -> Vec<Row> {
    let mut out = Vec::new();
    for line in ROWS.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).expect("a row");
        if v[0].as_str() != Some(unit) {
            continue;
        }
        let num = |d: &serde_json::Value| d.as_f64().expect("a number");
        let index = |d: &serde_json::Value| {
            usize::try_from(d.as_u64().expect("an index")).expect("an index")
        };
        let moved = v[2]
            .as_array()
            .expect("moved values")
            .iter()
            .map(|d| {
                (
                    d[0].as_str().expect("key").to_string(),
                    index(&d[1]),
                    num(&d[2]),
                    num(&d[3]),
                )
            })
            .collect();
        let entities = v[3]
            .as_array()
            .expect("moved entities")
            .iter()
            .map(|d| {
                (
                    index(&d[0]),
                    d[1].as_str() == Some("e"),
                    index(&d[2]),
                    num(&d[3]),
                    num(&d[4]),
                )
            })
            .collect();
        out.push(Row {
            case: v[1].as_str().expect("case").to_string(),
            moved,
            entities,
        });
    }
    out
}

/// `B` of each UV trim of `e`, on the face of the edge use that holds it, as the kernel
/// computes it for the raise (`trim_deviation_bound`: an owner-less copy of the surface, the
/// edge's interval, the floor a thousandth of the edge's tolerance); `None` where it is not
/// enclosed.
fn trim_bounds(k: &Kernel, b: &Brep, e: EdgeId) -> Vec<Option<f64>> {
    let rec = b.edge(e).expect("edge");
    let Some(curve) = rec.curve.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &eu in &rec.edgeuses {
        let Some(trim) = b.edgeuse(eu).expect("edge use").uv_trim_curve.as_ref() else {
            continue;
        };
        let face = eu1::face(b, eu).expect("face").expect("a face");
        let surface = b
            .face(face)
            .expect("face")
            .surface
            .as_ref()
            .expect("surface");
        let mut surface = surface.clone_for_evaluation().expect("a copy");
        let _ = surface.set_owner(None);
        let ivl = rec.interval;
        out.push(
            trim_image_gap_bound(
                k,
                &surface,
                curve,
                trim,
                [ivl.min(), ivl.max()],
                FLOOR * rec.tolerance,
            )
            .expect("the bound"),
        );
    }
    out
}

/// For an edge whose B-rep keeps no UV trim: a proved bound on the distance from its curve
/// over its interval to the surface of each face of the edge
/// ([`curve_face_gap_bound`], floor a thousandth of `n`), plus the rounding of the curve's
/// evaluation against its NURBS data (none for a B-spline or an ellipse, evaluated on it; for a
/// line, evaluated analytically, `γ_32` of 8 times the largest coordinate of its NURB, as the
/// gap bound counts it for a trim); `None` per face where it is not enclosed or the curve is
/// of another kind.
fn face_bounds(k: &Kernel, b: &Brep, e: EdgeId, n: f64) -> Vec<Option<f64>> {
    let rec = b.edge(e).expect("edge");
    let faces = edge::faces(b, e, None).expect("faces");
    let Some(curve) = rec.curve.as_ref() else {
        return vec![None; faces.len().max(1)];
    };
    let line;
    let (bs, slack) = match curve.kind() {
        CurveKind::BSpline(bs) => (bs, 0.0),
        CurveKind::Ellipse(el) => (el.base(), 0.0),
        CurveKind::Line(l) => {
            line = l.base_with_nurb().expect("a line's NURB");
            let m = line
                .control_points()
                .expect("a line's NURB")
                .iter()
                .fold(0.0_f64, |a, c| {
                    a.max(c.x.abs()).max(c.y.abs()).max(c.z.abs())
                });
            (&*line, gamma(32) * 8.0 * m * (1.0 + 4.0 * f64::EPSILON))
        }
        _ => return vec![None; faces.len().max(1)],
    };
    let ivl = rec.interval;
    faces
        .iter()
        .map(|&f| {
            curve_face_gap_bound(k, b, f, bs, [ivl.min(), ivl.max()], FLOOR * n)
                .expect("the bound")
                .map(|g| (g + slack) * (1.0 + 4.0 * f64::EPSILON))
        })
        .collect()
}

/// Whether the point of `v` is within `n` of the end of every edge curve of `v` that ends
/// there, the rounding of each distance counted against it (as [`super::tolerance_covers`]).
fn vertex_covers_its_ends(k: &Kernel, b: &Brep, v: VertexId, n: f64) -> bool {
    let at = b.vertex(v).expect("vertex").point;
    for e in solidcore::topology::vertex::edges(b, v, None).expect("edges") {
        let rec = b.edge(e).expect("edge");
        let Some(curve) = rec.curve.as_ref() else {
            continue;
        };
        let (v0, v1) = edge::start_end_vertices(b, e).expect("ends");
        for (end, t) in [
            (v0, rec.interval.evaluate(0.0)),
            (v1, rec.interval.evaluate(1.0)),
        ] {
            if end != Some(v) {
                continue;
            }
            let p = curve.evaluate_point(k, t).expect("end point");
            let size = at.max_dimension().max(p.max_dimension());
            if at.distance(p) + gamma(8) * size * 3.0_f64.sqrt() > n {
                return false;
            }
        }
    }
    true
}

/// What one unit's moved values were certified by.
#[derive(Debug, Default)]
struct Tally {
    cases: usize,
    /// Moved output values, each the new tolerance of a moved entity.
    values: usize,
    /// Moved edges whose every UV trim is bounded.
    edges: usize,
    /// Moved vertices within their tolerance of their edges' ends.
    vertices: usize,
    /// Trims bounded (or faces, for an edge without trims).
    trims: usize,
    /// Moved edges without a UV trim, bounded against their faces' surfaces instead.
    trimless: usize,
    /// Moved edges listed in [`OPEN`].
    open: usize,
    /// The largest `B / n`.
    worst: f64,
}

/// Runs every moved case of `unit` and checks each moved value and entity (see the module
/// docs).
fn certify(unit: &str) -> Tally {
    let runner = crate::registry()
        .into_iter()
        .find(|u| u.id() == unit)
        .expect("the unit");
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let cases = runner.cases(&ctx).expect("cases");
    let rows = rows(unit);
    assert!(!rows.is_empty(), "{unit}: no rows");
    let k = Kernel::default();
    let mut tally = Tally::default();
    let mut failures = Vec::new();
    let open = open_entries(unit);
    let mut open_seen = std::collections::BTreeSet::new();
    for row in &rows {
        let case = cases
            .iter()
            .find(|c| c.name == row.case)
            .unwrap_or_else(|| panic!("{unit}: no case {}", row.case));
        super::watch_written();
        let out = runner.run_rust(case);
        let bodies = super::take_written();
        let out = out.unwrap_or_else(|e| panic!("{unit} {}: {e:?}", row.case));
        tally.cases += 1;
        // each moved output value is the new tolerance of a moved entity, old to new
        for (key, index, old, new) in &row.moved {
            let at = format!("{unit} {} {key}[{index}]", row.case);
            let now = out
                .values
                .get(key)
                .and_then(|v| v.get(*index))
                .unwrap_or_else(|| panic!("{at}: not in the output"));
            assert_eq!(
                now.to_bits(),
                new.to_bits(),
                "{at}: output {now:e}, table {new:e}"
            );
            assert!(
                row.entities
                    .iter()
                    .any(|e| e.3.to_bits() == old.to_bits() && e.4.to_bits() == new.to_bits()),
                "{at}: {old:e} -> {new:e} is no moved entity's"
            );
            tally.values += 1;
        }
        // each moved entity: its tolerance is lower, and covers what it is the tolerance for
        for &(body, is_edge, index, old, new) in &row.entities {
            let kind = if is_edge { "edge" } else { "vertex" };
            let at = format!("{unit} {} body {body} {kind} {index}", row.case);
            assert!(new < old && new > 0.0, "{at}: {old:e} -> {new:e}");
            let b = bodies.get(body).unwrap_or_else(|| panic!("{at}: no body"));
            if is_edge {
                let (e, rec) = b
                    .edges()
                    .iter()
                    .find(|(e, _)| usize::try_from(e.index()) == Ok(index))
                    .unwrap_or_else(|| panic!("{at}: no edge"));
                assert_eq!(rec.tolerance.to_bits(), new.to_bits(), "{at}: tolerance");
                let mut bounds = trim_bounds(&k, b, e);
                if bounds.is_empty() {
                    bounds = face_bounds(&k, b, e, new);
                    tally.trimless += 1;
                }
                let mut bad = Vec::new();
                for x in &bounds {
                    tally.trims += 1;
                    match x {
                        Some(x) if *x <= new => tally.worst = tally.worst.max(x / new),
                        Some(x) => bad.push(format!("B {x:e}")),
                        None => bad.push("B not enclosed".to_string()),
                    }
                }
                let key = (row.case.clone(), body, index);
                if bad.is_empty() {
                    tally.edges += 1;
                    assert!(!open.contains(&key), "{at}: certified, remove it from OPEN");
                } else if open.contains(&key) {
                    open_seen.insert(key);
                    tally.open += 1;
                } else {
                    failures.push(format!("{at}: {old:e} -> {new:e}: {bad:?}"));
                }
            } else {
                let (v, rec) = b
                    .vertices()
                    .iter()
                    .find(|(v, _)| usize::try_from(v.index()) == Ok(index))
                    .unwrap_or_else(|| panic!("{at}: no vertex"));
                assert_eq!(rec.tolerance.to_bits(), new.to_bits(), "{at}: tolerance");
                if vertex_covers_its_ends(&k, b, v, new) {
                    tally.vertices += 1;
                } else {
                    failures.push(format!("{at}: {old:e} -> {new:e}: an edge end is farther"));
                }
            }
        }
    }
    println!("{unit}: {tally:?}");
    assert_eq!(open_seen, open, "{unit}: OPEN entries not reached");
    assert!(
        failures.is_empty(),
        "{unit}: {} not certified:\n{}",
        failures.len(),
        failures.join("\n")
    );
    tally
}

macro_rules! certified_units {
    ($($name:ident: $unit:literal,)*) => {
        $(
            #[test]
            fn $name() {
                let t = certify($unit);
                assert!(t.values > 0, "{t:?}");
            }
        )*
    };
}

certified_units! {
    api_primitives: "primitive.api_primitives",
    primitive_creation_p1: "primitive.primitive_creation_p1",
    fillet_executive_p3: "blend.fillet_executive_p3",
    feature_executive: "blend.feature_executive",
    stitch_p1: "heal.stitch_p1",
    stitch_p2: "heal.stitch_p2",
    face_p2: "query.face_p2",
    brep_cutting: "topology.brep_cutting",
    brep_local_operation: "topology.brep_local_operation",
    brep_mirror: "topology.brep_mirror",
    brep_p10: "topology.brep_p10",
    brep_p2: "topology.brep_p2",
    brep_p4: "topology.brep_p4",
    edgeuse_p3: "topology.edgeuse_p3",
    face_p3: "topology.face_p3",
}

/// #18162 regression: `heal.stitch_p2` `cov_tjunc11_do_nonmanifold_gaps` closes a corner's
/// UV gap by moving the ends of two trims onto the vertex's UV point
/// (`topology::face::p2::fix_uv_trim_curves_notified`, upstream `sm_FixUVTrimCurves`) after
/// their edges' tolerances were set. A step of 1.4e-7 in UV is 1.4e-5 in 3D on those faces,
/// and upstream leaves the tolerances as they were: edge 0 kept 1.311e-5 (upstream) under a
/// trim 1.424e-5 away. Every edge of the stitched body now has each of its trims within
/// its tolerance by the enclosure `B` ([`trim_bounds`]).
#[test]
fn stitched_corner_fix_keeps_its_edges_covering_their_trims() {
    let unit = "heal.stitch_p2";
    let runner = crate::registry()
        .into_iter()
        .find(|u| u.id() == unit)
        .expect("the unit");
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let cases = runner.cases(&ctx).expect("cases");
    let case = cases
        .iter()
        .find(|c| c.name == "cov_tjunc11_do_nonmanifold_gaps")
        .expect("the case");
    super::watch_written();
    let out = runner.run_rust(case);
    let bodies = super::take_written();
    out.expect("the case runs");
    let k = Kernel::default();
    let mut bad = Vec::new();
    let mut trims = 0;
    for (i, b) in bodies.iter().enumerate() {
        for (e, rec) in b.edges().iter() {
            for x in trim_bounds(&k, b, e) {
                trims += 1;
                if !matches!(x, Some(x) if x <= rec.tolerance) {
                    bad.push(format!(
                        "body {i} edge {}: tol {:e}, B {x:?}",
                        e.index(),
                        rec.tolerance
                    ));
                }
            }
        }
    }
    assert!(trims > 0, "no trims");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
