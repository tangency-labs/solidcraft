//! The golden rows retiring the four fillet flags of issue #18913 moves
//! (`fillet_chamfer_cone_on_arc` WR-421/572, `fillet_trace_reaches_rails` WR-609/610,
//! `fillet_patch_keeps_fillet_edges` WR-546, `fillet_exact_round_classes` WR-669), each
//! replayed and proven by its class.
//!
//! Every row in [`ROWS`] is re-run through its unit's runner with `goldens::dump::capture`
//! on and must reproduce the committed row (`golden::diff` with the unit's floors, as the
//! goldens job compares). Then, per class ([`Class`]):
//!
//! * **Result bodies (PROVEN, invariants replayed).** [`Class::Body`] and
//!   [`Class::BodyFromRefusal`] rows (`blend.fillet_executive_p3`; topology, geometry or
//!   mass moved): the captured result body passes `check_brep` at Level 2 with no failure;
//!   every face on a plane, cylinder, cone, sphere or torus has its NURBS form proved on
//!   that surface's implicit equation (`AnalyticSupport::check`, every Bernstein coefficient
//!   within the rounding of computing it); every line or conic edge between faces on such
//!   surfaces lies on each of them (`curve_lies_on`), and each edge's end vertices are
//!   within tolerance of its curve's ends. A body that fails one of these fails it no more
//!   often than main's body of the same case does ([`MAIN_FAILURES`], main replayed with
//!   this test): those failures are main's own, unchanged by the flags, and pinned so that
//!   any new one fails here. A row main refused has no body on main, so its body is held
//!   to its input body's failures instead ([`HELD_TO_INPUT`]): each as often, and an edge
//!   off a face's surface only on the very curve of an input edge off it or where its
//!   distance to every face it is off is proved within its tolerance ([`gap_within_tolerance`],
//!   the kernel's certified curve-to-surface bound, not samples).
//!   Each moved `mass_properties` entry is classified
//!   against a certified enclosure of the body's true value ([`MASS_ROWS`], `harness::Table`:
//!   CLOSER, EQUAL or FARTHER, pinned in [`EXPECTED`], none FARTHER); a row that was a
//!   refusal and is now a body has every entry within the rows' `1e-4` accuracy of its
//!   enclosure ([`from_refusal_mass_is_enclosed`]). The enclosures are full sweeps, so
//!   those parts are `#[ignore]`d.
//! * **Shelled rounded boxes (PROVEN, closed form).** [`Class::Shell`]: the box `30 x 20 x
//!   10` with its edges rounded r 3, shelled inward by 2. The body is certified as above and
//!   each `shell_mass_properties` entry is within `1e-9` relative of its closed form
//!   ([`shell_closed_form`]).
//! * **Previews (PROVEN).** [`Class::Preview`]: each preview surface of the replayed call is
//!   a cylinder of the radius, its NURBS form on its implicit (`AnalyticSupport::check`),
//!   its axis along its edge and at the radius from both of the edge's box faces, to the
//!   rounding of the coordinates. [`Class::PreviewPastFaces`] (`preview0018`, main a
//!   preview of 217 blocks, now refused with status 1001, `SM_ERR`): the radius exceeds the
//!   width of every face either selected edge bounds, so no round of that radius fits on a
//!   face and main's preview surfaces could not be tangent within them
//!   ([`radius_past_faces`], closed form from the case's inputs).
//! * **Executive checkpoints (PROVEN, closed form).** [`Class::Checkpoint`]: the
//!   intermediate state of `issue_2020_mitred_put_back_d9`. Every corner vertex the row
//!   lists is one of the eight crossings of the mitred rounds' rails, and every vertex of
//!   the same case's full result body (itself certified as a result body) one of its eight
//!   closed-form vertices ([`check_mitred_corners`]).
//! * **The chamfered cylinder (PROVEN, closed form).** [`Class::ChamferedCylinder`]: every
//!   face area and edge length of the cylinder r 5, h 10 chamfered 1 on both rims is within
//!   the record's accuracy of its closed form ([`chamfered_cylinder`]).
//! * **Downstream rounded boxes (PROVEN, closed form).** [`Class::RoundedBox`]
//!   (`topology.brep_cutting`, `primitive.api_trim_ops`, `topology.api_projection`,
//!   `topology.api_files`): the rows move only through the input body, a box every edge of
//!   which `circular_fillet` rounds. Every such box of the case is rebuilt and certified as a
//!   result body, with 6 planes and 12 cylinders of the radius and 8 corner patches, and its
//!   certified enclosure holds the closed-form area, volume and centroid ([`rounded_box`]).
//! * **Rail / side edge-use intersections (PROVEN, interval arithmetic).** [`Class::RailEu`]
//!   (`blend.fillet_solver_p2`, not found before, found now): the equations are affine (two
//!   offset planar parallelograms of integer points, a degree-1 trim), so the root is
//!   enclosed by Cramer's rule in outward-rounded intervals ([`rail_eu_root`]); it lies
//!   strictly inside the solve's box (so a root exists: found is right) and each new
//!   unknown is within `1e-9` relative of it.
//! * **A chamfer turned about an arc (PROVEN, closed form, for the values the row records).**
//!   [`Class::ArcChamferCone`]
//!   (`gen0224_torus`, a refusal before): `CalcTorusFilletGeom` called directly with a
//!   linear section on an arc edge, which upstream refuses and WR-421 revolves into its
//!   cone. The section runs from the floor foot `(big - r, 0, 0)` to `(big, 0, r)`, so the
//!   cone about the arc's axis `z` is `rho(z) = big - r + z`. Every recorded point of the
//!   fillet surface's grid lies on it and each rail's points on one of its end circles, to
//!   `1e-9` ([`certify_arc_chamfer_cone`]). The row records only those 3 x 3 grid points
//!   and the rails' points, so every value that moved is proven; points of the surface
//!   between the grid points are in no row and this claims nothing of them.
//!
//! A later change that legitimately moves one of these rows again updates the tables with
//! that change's derivation: the replay fails until it does.
//!
//! Run: `cargo test --profile goldens -p goldens --test issue_18913_moved_rows_are_proven`
//! (the enclosure parts: `-- --include-ignored`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::print_stderr,
    missing_docs
)]

use std::collections::BTreeMap;
use std::f64::consts::PI;

use goldens::case::repo_root;
use goldens::compare::values_agree;
use goldens::{golden, CaseContext, Output};
use serde_json::Value;
use solidcore::blend::fillet_executive::api::{circular_fillet, fillet_preview};
use solidcore::blend::fillet_section::interval::Ivl;
use solidcore::curve::curve::p2::CurveKind;
use solidcore::curve::curve::Curve;
use solidcore::mass::enclosure::enclose_mass_properties;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::ControlPointForm;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::curve_surface_gap::{curve_face_gap_bound_within, line_evaluation_slack};
use solidcore::surface::implicit::{curve_lies_on, AnalyticSupport, Implicit};
use solidcore::topology::brep::p1;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{edge, face, vertex, Brep, EdgeId, FaceId};
use solidcore::Kernel;

#[path = "moved_mass_rows/harness.rs"]
mod harness;
use harness::{Row, Side, Table};

/// How a moved row is proven (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Class {
    /// A fillet result body.
    Body,
    /// A fillet result body where main refused.
    BodyFromRefusal,
    /// A shelled rounded box.
    Shell,
    /// Preview surfaces.
    Preview,
    /// A preview whose radius is past the faces, now refused.
    PreviewPastFaces,
    /// The executive's intermediate state.
    Checkpoint,
    /// The attributed chamfer of a cylinder.
    ChamferedCylinder,
    /// A downstream unit's row of a rounded box.
    RoundedBox,
    /// A rail / side edge-use intersection found.
    RailEu,
    /// A linear section revolved about an arc edge (`gen0224_torus`).
    ArcChamferCone,
}

/// A moved row: unit, case, class.
type Moved = (&'static str, &'static str, Class);

include!("issue_18913_moved_rows/rows.rs");
include!("issue_18913_moved_rows/mass_rows.rs");
include!("issue_18913_moved_rows/expected.rs");
include!("issue_18913_moved_rows/main_failures.rs");

/// The goldens' default `--seeds`.
const SEEDS: usize = 16;

/// The fillet unit.
const P3: &str = "blend.fillet_executive_p3";

/// The width asked of each enclosure, relative to its value's scale.
const ENCLOSURE_WIDTH: f64 = 1e-5;
/// The tests the mass rows are split into, by body.
const PARTS: usize = 8;

/// The moved `mass_properties` rows and how they are classified.
const TABLE: Table = Table {
    unit: P3,
    tag: "I18913",
    rows: &MASS_ROWS,
    parts: PARTS,
    width: ENCLOSURE_WIDTH,
    // (`brep_mass_properties` at `1e-4`, relative to each value's scale)
    accuracy: 1e-4,
};

// ---- replay ------------------------------------------------------------------------------

/// The cases and committed golden of each unit, loaded once.
struct Units {
    ctx: CaseContext,
    dir: std::path::PathBuf,
    loaded: BTreeMap<&'static str, (golden::Golden, Vec<goldens::Case>)>,
}

impl Units {
    fn new() -> Self {
        Self {
            ctx: CaseContext {
                root: repo_root(),
                seeds: SEEDS,
            },
            dir: golden::default_dir(),
            loaded: BTreeMap::new(),
        }
    }

    /// Runs `unit`'s case `name` with capture on: the case, its output and the captured
    /// bodies by label. A row the run does not reproduce is a failure in `bad`.
    fn replay(
        &mut self,
        unit: &'static str,
        name: &str,
        bad: &mut Vec<String>,
    ) -> (goldens::Case, Output, Vec<(String, Brep)>) {
        let registry = goldens::registry();
        let runner = registry.iter().find(|u| u.id() == unit).expect("unit");
        let (ctx, dir) = (&self.ctx, &self.dir);
        let (lines, cases) = self
            .loaded
            .entry(unit)
            .or_insert_with(|| (golden::read(dir, unit).unwrap(), runner.cases(ctx).unwrap()));
        let case = cases
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("{unit}/{name}: no such case"))
            .clone();
        goldens::dump::capture();
        let out = runner.run_rust(&case);
        let captured = goldens::dump::take();
        let out = out.unwrap();
        let committed = lines.get(name).expect("committed row");
        if let Some(change) =
            golden::diff(committed, &golden::encode(name, &out), runner.abs_floors())
        {
            bad.push(format!(
                "{unit}/{name}: does not reproduce its row: {change:?}"
            ));
        }
        let bodies = captured
            .into_iter()
            .map(|c| (c.label, c.brep.expect("an exact copy")))
            .collect();
        (case, out, bodies)
    }
}

/// The one captured body labelled `label`.
fn the_body<'a>(bodies: &'a [(String, Brep)], label: &str, name: &str) -> &'a Brep {
    let mut found = bodies.iter().filter(|(l, _)| l == label);
    let body = &found
        .next()
        .unwrap_or_else(|| panic!("{name}: no {label} body"))
        .1;
    assert!(found.next().is_none(), "{name}: more than one {label} body");
    body
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number, not {v}"))
}

fn nums(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap_or_else(|| panic!("an array, not {v}"))
        .iter()
        .map(num)
        .collect()
}

// ---- the result-body certificate ---------------------------------------------------------

/// Whether `curve`, a line or conic, lies on `a` (`None` for another kind).
fn on_implicit(curve: &Curve, a: &Implicit) -> Option<bool> {
    let on = match curve.kind() {
        CurveKind::Line(l) => curve_lies_on(&l.base_with_nurb().unwrap(), a),
        CurveKind::Ellipse(e) => curve_lies_on(e.base(), a),
        CurveKind::Hyperbola(h) => curve_lies_on(h.base(), a),
        CurveKind::Parabola(p) => curve_lies_on(p.base(), a),
        _ => return None,
    };
    Some(on.unwrap())
}

/// What [`certify`] found on one body.
#[derive(Debug, Default)]
struct Found {
    /// Faces with an analytic label, each proved on its implicit.
    analytic_faces: usize,
    /// Faces of every surface kind.
    faces: usize,
    /// Line and conic edges between faces on implicit surfaces, each on all of them.
    exact_edges: usize,
    /// Every surface kind's code, by count.
    kinds: BTreeMap<String, usize>,
    /// Failures.
    failures: Vec<String>,
    /// Each edge off a face's surface: its failure's index in `failures` and its curve
    /// (`{:?}`, which writes every coordinate exactly, so equal text is the same curve).
    off_surface: Vec<(usize, String)>,
    /// Each edge off a face's surface: its failure's index in `failures`, the edge and the
    /// faces whose surface it is off.
    off_edges: Vec<(usize, EdgeId, Vec<FaceId>)>,
}

/// The result-body certificate (see the module docs).
fn certify(body: &Brep) -> Found {
    let mut k = Kernel::default();
    let mut found = Found::default();
    let mut b = body.clone_standing_for_self().unwrap();
    match check_brep(&mut k, &mut b, CheckLevel::Level2) {
        Ok(check) => {
            for line in check.lines() {
                found.failures.push(format!("Level 2: {line}"));
            }
        }
        Err(e) => found.failures.push(format!("Level 2 refused: {e}")),
    }
    for f in p1::faces(body, None).unwrap() {
        found.faces += 1;
        let Some(s) = face::p2::surface(body, f).unwrap() else {
            found.failures.push(format!("face {f:?} without a surface"));
            continue;
        };
        let kind = AnalyticSupport::of(s).map_or_else(
            || "other".to_owned(),
            |a| format!("{a:?}").split(' ').next().unwrap_or("").to_owned(),
        );
        *found.kinds.entry(kind).or_default() += 1;
        if AnalyticSupport::of(s).is_some() {
            match AnalyticSupport::check(&k, s) {
                Ok(_) => found.analytic_faces += 1,
                Err(e) => found
                    .failures
                    .push(format!("face {f:?}: its form is off its label: {e}")),
            }
        }
    }
    for e in p1::edges(body, None).unwrap() {
        let Some(curve) = edge::curve(body, e).unwrap() else {
            continue;
        };
        let faces = edge::faces(body, e, None).unwrap();
        let implicits: Vec<Option<Implicit>> = faces
            .iter()
            .map(|&f| {
                face::p2::surface(body, f)
                    .unwrap()
                    .and_then(AnalyticSupport::of)
                    .map(|a| a.implicit())
            })
            .collect();
        if !implicits.is_empty() && implicits.iter().all(Option::is_some) {
            let on: Vec<Option<bool>> = implicits
                .iter()
                .map(|a| on_implicit(curve, a.as_ref().unwrap()))
                .collect();
            if on.iter().all(|o| *o == Some(true)) {
                found.exact_edges += 1;
            } else if on.iter().all(Option::is_some) {
                let off: Vec<FaceId> = faces
                    .iter()
                    .zip(&on)
                    .filter(|(_, o)| **o == Some(false))
                    .map(|(f, _)| *f)
                    .collect();
                found.off_edges.push((found.failures.len(), e, off));
                found
                    .off_surface
                    .push((found.failures.len(), format!("{curve:?}")));
                found.failures.push(format!(
                    "edge {e:?} ({}) off a face's surface: {on:?}",
                    curve.kind().name()
                ));
            }
        }
        let ivl = edge::interval(body, e).unwrap();
        let ends = [
            curve.evaluate_point(&k, ivl.min()).unwrap(),
            curve.evaluate_point(&k, ivl.max()).unwrap(),
        ];
        for (v, end) in [
            (edge::start_vertex(body, e).unwrap(), ends[0]),
            (edge::end_vertex(body, e).unwrap(), ends[1]),
        ] {
            let Some(v) = v else {
                continue;
            };
            let gap = end.distance(vertex::point(body, v).unwrap());
            let allowed = vertex::tolerance(body, v)
                .unwrap()
                .max(edge::tolerance(body, e).unwrap());
            if !(gap <= allowed) {
                found.failures.push(format!(
                    "vertex {v:?} {gap:e} off edge {e:?}'s end (tolerance {allowed:e})"
                ));
            }
        }
    }
    found
}

/// Certifies `body` into `bad`, printing what was found.
fn certify_into(tag: &str, body: &Brep, bad: &mut Vec<String>) -> Found {
    certify_against_main(tag, "", body, bad)
}

/// The rows main refused (status 1001, so main has no body of theirs): each failure of
/// its body is held to its input body's ([`certify_against_input`]).
const HELD_TO_INPUT: [&str; 5] = [
    "stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1",
    "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1",
    "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5",
    "sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1",
    "sweep5-sphere_diff_3cylinders-e29-r1.0",
];

/// The largest proved bound on the distance from edge `e`'s curve over its interval to the
/// surface of each face of `faces` ([`curve_face_gap_bound_within`], pieces cut until shown
/// within the edge's tolerance where they are, floor a thousandth of it), plus a line's
/// evaluation slack against its NURB ([`line_evaluation_slack`]) and the sum's rounding, where
/// every one is at most the edge's tolerance: the edge then meets the B-rep's own contract
/// on those faces. `None` where a bound is not enclosed or past the tolerance.
fn gap_within_tolerance(body: &Brep, e: EdgeId, faces: &[FaceId]) -> Option<f64> {
    let k = Kernel::default();
    let tol = edge::tolerance(body, e).unwrap();
    let rec = body.edge(e).unwrap();
    let curve = rec.curve.as_ref()?;
    let line;
    let (bs, slack) = match curve.kind() {
        CurveKind::BSpline(bs) => (bs, 0.0),
        CurveKind::Ellipse(el) => (el.base(), 0.0),
        CurveKind::Line(l) => {
            line = l.base_with_nurb().unwrap();
            (&*line, line_evaluation_slack(l, false)?)
        }
        _ => return None,
    };
    let ivl = rec.interval;
    let enough = tol / (1.0 + 4.0 * f64::EPSILON) - slack;
    let mut worst = 0.0f64;
    for &f in faces {
        let g = curve_face_gap_bound_within(
            &k,
            body,
            f,
            bs,
            [ivl.min(), ivl.max()],
            1e-3 * tol,
            [enough, f64::INFINITY],
        )
        .unwrap()?;
        let g = (g + slack) * (1.0 + 4.0 * f64::EPSILON);
        if !(g <= tol) {
            return None;
        }
        worst = worst.max(g);
    }
    Some(worst)
}

/// A failure as [`MAIN_FAILURES`] writes it: its edge ids `EdgeId(_)` (a rebuilt body
/// numbers its edges afresh).
fn without_ids(failure: &str) -> String {
    let mut out = String::new();
    let mut rest = failure;
    while let Some(i) = rest.find("EdgeId(") {
        out.push_str(&rest[..i + "EdgeId(".len()]);
        rest = &rest[i + "EdgeId(".len()..];
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits > 0 {
            out.push('_');
            rest = &rest[digits..];
        }
    }
    out.push_str(rest);
    out
}

/// Certifies `body` into `bad` as [`certify_into`], except that the failures main's body
/// of `case` has, each as often, are main's own and pass.
fn certify_against_main(tag: &str, case: &str, body: &Brep, bad: &mut Vec<String>) -> Found {
    let found = certify(body);
    eprintln!(
        "I18913 {tag}: {} faces ({} analytic, proved), {} exact edges, kinds {:?}",
        found.faces, found.analytic_faces, found.exact_edges, found.kinds
    );
    let on_main = case;
    let mut allowed: BTreeMap<&str, usize> = MAIN_FAILURES
        .iter()
        .filter(|(c, _, _)| *c == on_main)
        .map(|&(_, f, n)| (f, n))
        .collect();
    for f in &found.failures {
        let key = without_ids(f);
        match allowed.get_mut(key.as_str()) {
            Some(n) if *n > 0 => {
                *n -= 1;
                eprintln!("I18913 {tag}: main's own (as {on_main}): {f}");
            }
            _ => bad.push(format!("{tag}: {f}")),
        }
    }
    found
}

/// Certifies the body of a [`HELD_TO_INPUT`] row into `bad`, holding its failures to its
/// input body's: a failure passes only as one the input body read from `case`'s file has,
/// each as often, and an edge off a face's surface only where the input has an edge off a
/// face's surface on the same curve (exactly: the same `{:?}` text), so the failure is that
/// unchanged input edge's, not one the fillet made. An edge off a face's surface that is
/// not the input's passes only where its distance to each face it is off is proved within
/// the edge's tolerance ([`gap_within_tolerance`]): off the exact implicit, but within the
/// gap the body records for it.
fn certify_against_input(tag: &str, case: &goldens::Case, body: &Brep, bad: &mut Vec<String>) {
    let path = source_file(&case.inputs).unwrap_or_else(|| panic!("{tag}: no file source"));
    let input = goldens::read_source_file(&mut Kernel::default(), &path).unwrap();
    let (found, before) = (certify(body), certify(&input));
    eprintln!(
        "I18913 {tag}: {} faces ({} analytic, proved), {} exact edges, kinds {:?}; input: {} \
         failures",
        found.faces,
        found.analytic_faces,
        found.exact_edges,
        found.kinds,
        before.failures.len()
    );
    let mut allowed: BTreeMap<String, usize> = BTreeMap::new();
    for f in &before.failures {
        *allowed.entry(without_ids(f)).or_default() += 1;
    }
    let mut input_curves: Vec<(String, &str)> = before
        .off_surface
        .iter()
        .map(|(i, curve)| (without_ids(&before.failures[*i]), curve.as_str()))
        .collect();
    for (i, f) in found.failures.iter().enumerate() {
        let key = without_ids(f);
        if let Some((_, e, off)) = found.off_edges.iter().find(|(j, _, _)| *j == i) {
            if let Some(g) = gap_within_tolerance(body, *e, off) {
                eprintln!(
                    "I18913 {tag}: within its tolerance ({:e}, proved gap {g:e}): {f}",
                    edge::tolerance(body, *e).unwrap()
                );
                continue;
            }
        }
        let tied = match found.off_surface.iter().find(|(j, _)| *j == i) {
            Some((_, curve)) => input_curves
                .iter()
                .position(|(k, c)| *k == key && c == curve)
                .map(|at| input_curves.remove(at))
                .is_some(),
            None => true,
        };
        match allowed.get_mut(&key) {
            Some(n) if *n > 0 && tied => {
                *n -= 1;
                eprintln!("I18913 {tag}: the input's own: {f}");
            }
            _ => bad.push(format!("{tag}: {f} (not the input's own)")),
        }
    }
}

/// The path of the `{"kind": "file", "path": ..}` source anywhere in a case's inputs.
fn source_file(inputs: &Value) -> Option<String> {
    match inputs {
        Value::Object(m) => {
            if m.get("kind").and_then(Value::as_str) == Some("file") {
                if let Some(p) = m.get("path").and_then(Value::as_str) {
                    return Some(p.to_owned());
                }
            }
            m.values().find_map(source_file)
        }
        Value::Array(a) => a.iter().find_map(source_file),
        _ => None,
    }
}

// ---- closed forms ------------------------------------------------------------------------

/// The box `a x b x c` with every edge rounded `r` (`r` at most half the smallest side):
/// `(area, volume)`. With `A = a - 2r` (and `B`, `C`): the six faces `2 (AB + BC + CA)`,
/// the twelve quarter cylinders `2 pi r (A + B + C)`, the eight sphere octants `4 pi r^2`;
/// the volume the core `ABC`, the six slabs `2r (AB + BC + CA)`, the quarter cylinders
/// `pi r^2 (A + B + C)` and the octants `4/3 pi r^3`.
fn rounded_box_closed_form(a: f64, b: f64, c: f64, r: f64) -> (f64, f64) {
    let (aa, bb, cc) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
    let faces = aa * bb + bb * cc + cc * aa;
    let edges = aa + bb + cc;
    let area = 2.0 * faces + 2.0 * PI * r * edges + 4.0 * PI * r * r;
    let volume = aa * bb * cc + 2.0 * r * faces + PI * r * r * edges + 4.0 / 3.0 * PI * r * r * r;
    (area, volume)
}

/// The shelled boxes' `(area, volume)`: the box `30 x 20 x 10` with its edges rounded r 3,
/// shelled inward by 2, so the cavity is the box `[2, 28] x [2, 18] x [2, 8]` with the same
/// edges rounded r 1.
///
/// * all twelve edges, closed: the rounded box less the rounded cavity, both with the
///   cores `24 x 14 x 4` ([`rounded_box_closed_form`]): area `(976 + 288 pi) + (976 + 88
///   pi)`, volume `(4272 + 414 pi) - (2320 + 130/3 pi) = 1952 + 1112/3 pi`.
/// * all twelve edges, open at the top: the top face `24 x 14` of the box and of the cavity
///   are gone (`-672` of area) and the rim between them, the band `x in [0, 3]` ... of
///   height 2 around the `24 x 14` rectangle (`2 * 2 (24 + 14) = 152`) joins them; the
///   cavity reaches the top, `24 x 14 x 2 = 672` more: area `1432 + 376 pi`, volume `1280 +
///   1112/3 pi`.
/// * one (`1v`) or four (`4v`) vertical edges, closed (as `wr545_moved_rows_are_exact`
///   derives): area `3444 + 23 pi` and `3168 + 92 pi`, volume `3420 + 21 pi` and `3168 + 84
///   pi`.
fn shell_closed_form(name: &str) -> (f64, f64) {
    match name {
        "real-box-fillet12-r3-shell-closed-issue-460" => {
            (1952.0 + 376.0 * PI, 1952.0 + 1112.0 / 3.0 * PI)
        }
        "real-box-fillet12-r3-shell-open-top-issue-460" => {
            (1432.0 + 376.0 * PI, 1280.0 + 1112.0 / 3.0 * PI)
        }
        "real-box-fillet1v-r3-shell-closed" => (3444.0 + 23.0 * PI, 3420.0 + 21.0 * PI),
        "real-box-fillet4v-r3-shell-closed" => (3168.0 + 92.0 * PI, 3168.0 + 84.0 * PI),
        _ => panic!("{name}: no closed form"),
    }
}

#[test]
fn closed_forms_match_their_derivations() {
    // the closed twelve-edge shell from the rounded-box formula
    let (so, vo) = rounded_box_closed_form(30.0, 20.0, 10.0, 3.0);
    let (si, vi) = rounded_box_closed_form(26.0, 16.0, 6.0, 1.0);
    let (s, v) = shell_closed_form("real-box-fillet12-r3-shell-closed-issue-460");
    assert!((so + si - s).abs() < 1e-11 * s);
    assert!((vo - vi - v).abs() < 1e-11 * v);
    assert!((so - (976.0 + 288.0 * PI)).abs() < 1e-11 * so);
    assert!((vo - (4272.0 + 414.0 * PI)).abs() < 1e-11 * vo);
    assert!((si - (976.0 + 88.0 * PI)).abs() < 1e-11 * si);
    assert!((vi - (2320.0 + 130.0 / 3.0 * PI)).abs() < 1e-11 * vi);
    // the open top: 336 + 336 of top faces off, the 152 rim on; 672 more cavity
    let (so2, vo2) = shell_closed_form("real-box-fillet12-r3-shell-open-top-issue-460");
    assert!((s - 672.0 + 152.0 - so2).abs() < 1e-11 * s);
    assert!((v - 672.0 - vo2).abs() < 1e-11 * v);
    // a cube with every edge rounded half its side is a sphere
    let (a, v) = rounded_box_closed_form(2.0, 2.0, 2.0, 1.0);
    assert!((a - 4.0 * PI).abs() < 1e-14 && (v - 4.0 / 3.0 * PI).abs() < 1e-14);
}

/// The cylinder r `R` = 5, h 10 with both rims chamfered by `SmApiChamferFillet` at
/// distance 1, its section's chord (`SmConstantDistanceFS`): on a right-angle rim the legs
/// are `a = 1 / sqrt 2`. Face areas: the side `2 pi R (h - 2a)`, the two discs
/// `pi (R - a)^2`, the two cone bands `pi (2R - a) a sqrt 2` (Pappus: the chord `a sqrt 2`
/// about the axis at mean radius `R - a/2`). Edge lengths: the two circles `2 pi R` and the
/// two `2 pi (R - a)` (the rims, listed first), the side's seam `h - 2a` and a cone's seam,
/// the chord `a sqrt 2`.
fn chamfered_cylinder() -> (Vec<f64>, Vec<f64>) {
    let (r, h) = (5.0, 10.0);
    let a = 1.0 / 2f64.sqrt();
    let cone = PI * (2.0 * r - a) * a * 2f64.sqrt();
    let disc = PI * (r - a) * (r - a);
    let areas = vec![2.0 * PI * r * (h - 2.0 * a), disc, disc, cone, cone];
    let lengths = vec![
        2.0 * PI * r,
        2.0 * PI * r,
        2.0 * PI * (r - a),
        2.0 * PI * (r - a),
        h - 2.0 * a,
        a * 2f64.sqrt(),
    ];
    (areas, lengths)
}

// ---- previews ----------------------------------------------------------------------------

/// The box and the selected edges of a `fillet_preview` case.
fn preview_inputs(case: &goldens::Case) -> ([f64; 3], Vec<usize>, f64) {
    let p = &case.inputs["fillet_preview"];
    let d = nums(&p["dimensions"]);
    let edges = p["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_u64().unwrap() as usize)
        .collect();
    ([d[0], d[1], d[2]], edges, num(&p["radius"]))
}

/// The axis (0, 1, 2) a box edge runs along and its corner point.
fn box_edge(b: &Brep, e: solidcore::topology::EdgeId) -> (usize, Vec3, Vec3) {
    let vs = edge::vertices(b, e, None).unwrap();
    let pts: Vec<Vec3> = vs
        .iter()
        .flatten()
        .map(|v| vertex::point(b, *v).unwrap())
        .collect();
    assert_eq!(pts.len(), 2);
    let d = pts[1] - pts[0];
    let axis = [d.x, d.y, d.z]
        .iter()
        .position(|c| *c != 0.0)
        .expect("an edge of the box");
    assert_eq!(
        [d.x, d.y, d.z].iter().filter(|c| **c != 0.0).count(),
        1,
        "an axis-parallel edge"
    );
    (axis, pts[0], pts[1])
}

fn coord(p: Vec3, i: usize) -> f64 {
    [p.x, p.y, p.z][i]
}

/// The preview certificate: each surface a cylinder of radius `r`, on its implicit, its
/// axis parallel to its edge, at `r` from both of the edge's faces (the planes through the
/// edge across the box), to `8` ulps of the box's size.
fn certify_preview(case: &goldens::Case, bad: &mut Vec<String>) -> usize {
    let (dims, sel, r) = preview_inputs(case);
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Vec3::ZERO, dims[0], dims[1], dims[2]).unwrap();
    let all = p1::edges(&b, None).unwrap();
    let edges: Vec<_> = sel.iter().map(|&i| all[i]).collect();
    let geo: Vec<_> = edges.iter().map(|&e| box_edge(&b, e)).collect();
    let surfaces = fillet_preview(&mut k, &mut b, &edges, r).unwrap();
    assert_eq!(
        surfaces.len(),
        edges.len(),
        "{}: a surface per edge",
        case.name
    );
    let size = dims.iter().copied().fold(r.abs(), f64::max);
    let slack = 8.0 * f64::EPSILON * size;
    let mut n = 0;
    for (s, (axis, p0, _)) in surfaces.iter().zip(&geo) {
        let Some(s) = s else {
            bad.push(format!("{}: an edge without a preview surface", case.name));
            continue;
        };
        let Ok(a) = AnalyticSupport::check(&k, s) else {
            bad.push(format!("{}: a preview surface off its label", case.name));
            continue;
        };
        let Implicit::Cone {
            origin,
            axis: dir,
            radius,
            tan_angle,
        } = a.implicit()
        else {
            bad.push(format!("{}: a preview surface not a cylinder", case.name));
            continue;
        };
        if tan_angle != 0.0 || (radius - r.abs()).abs() > slack {
            bad.push(format!(
                "{}: a cylinder of radius {radius}, not {r}",
                case.name
            ));
        }
        let along = [dir.x, dir.y, dir.z];
        if (along[*axis].abs() - 1.0).abs() > 4.0 * f64::EPSILON {
            bad.push(format!("{}: an axis {dir:?} not along the edge", case.name));
        }
        // the two faces through the edge: the planes `x_j = p0_j` across the box
        for j in (0..3).filter(|j| j != axis) {
            let off = (coord(origin, j) - coord(*p0, j)).abs();
            if (off - r.abs()).abs() > slack {
                bad.push(format!(
                    "{}: the axis {off} from a face, not {r}",
                    case.name
                ));
            }
        }
        n += 1;
    }
    n
}

/// `preview0018`'s invalidity, by closed form from its inputs: for each selected edge, the
/// radius exceeds the width of both box faces through it (the box's sides across the edge),
/// so the rail of any round of that radius is off both faces.
fn radius_past_faces(case: &goldens::Case) -> bool {
    let (dims, sel, r) = preview_inputs(case);
    let mut k = Kernel::default();
    let b = create_box(&mut k, Vec3::ZERO, dims[0], dims[1], dims[2]).unwrap();
    let all = p1::edges(&b, None).unwrap();
    sel.iter().all(|&i| {
        let (axis, _, _) = box_edge(&b, all[i]);
        (0..3).filter(|&j| j != axis).all(|j| r > dims[j])
    })
}

// ---- downstream rounded boxes ------------------------------------------------------------

/// Every filleted box in a downstream case's inputs: `(origin, size, radius)`. A box is an
/// object with `size` and `fillet` (`fillet > 0`) or with `length`, `width`, `height` and
/// `fillet`.
fn filleted_boxes(v: &Value, out: &mut Vec<([f64; 3], [f64; 3], f64)>) {
    match v {
        Value::Object(m) => {
            let fillet = m.get("fillet").and_then(Value::as_f64);
            if let (Some(size), Some(r)) = (m.get("size"), fillet) {
                let s = nums(size);
                if r > 0.0 {
                    out.push(([0.0; 3], [s[0], s[1], s[2]], r));
                }
            } else if let (Some(l), Some(r)) = (m.get("length"), fillet) {
                let o = m.get("origin").map_or_else(|| vec![0.0; 3], nums);
                out.push((
                    [o[0], o[1], o[2]],
                    [num(l), num(&m["width"]), num(&m["height"])],
                    r,
                ));
            }
            for x in m.values() {
                filleted_boxes(x, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|x| filleted_boxes(x, out)),
        _ => {}
    }
}

/// Rebuilds the rounded box and certifies it: the result-body certificate, 6 planes, 12
/// cylinders and 8 corner patches, and the closed form inside its certified enclosure (area,
/// volume, centroid `origin + size / 2`).
fn rounded_box(tag: &str, (origin, size, r): ([f64; 3], [f64; 3], f64), bad: &mut Vec<String>) {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Vec3::from(origin), size[0], size[1], size[2]).unwrap();
    circular_fillet(&mut k, &mut b, r).unwrap();
    let found = certify_into(tag, &b, bad);
    // The 18 analytic faces (6 planes, 12 edge rounds) are proved by `certify_into`; the 8
    // corner patches are the kernel's own vertex blends (a sphere octant or a B-spline
    // patch). Their shape is held by the whole-body closed form below: the area, volume and
    // centroid of the box with sphere-octant corners each lie in the certified enclosure.
    if found.faces != 26 || found.analytic_faces < 18 {
        bad.push(format!(
            "{tag}: {} faces, {} analytic, not 26 with at least 18",
            found.faces, found.analytic_faces
        ));
    }
    let mut radii = Vec::new();
    for f in p1::faces(&b, None).unwrap() {
        let s = face::p2::surface(&b, f).unwrap().unwrap();
        match AnalyticSupport::of(s) {
            Some(AnalyticSupport::Plane { .. }) => radii.push(("plane", 0.0)),
            Some(AnalyticSupport::Cylinder { radius, .. }) => radii.push(("cylinder", radius)),
            // an edge round carried as a cone of semi-angle exactly 0 is a cylinder
            Some(AnalyticSupport::Cone {
                base_radius,
                semi_angle_deg: 0.0,
                ..
            }) => radii.push(("cylinder", base_radius)),
            Some(AnalyticSupport::Sphere { radius, .. }) => radii.push(("corner", radius)),
            None => radii.push(("corner", r)),
            Some(other) => bad.push(format!("{tag}: a face {other:?}")),
        }
    }
    for (kind, want) in [("plane", 6), ("cylinder", 12), ("corner", 8)] {
        let n = radii.iter().filter(|x| x.0 == kind).count();
        if n != want {
            bad.push(format!("{tag}: {n} {kind}s, not {want}"));
        }
    }
    let scale = size.iter().copied().fold(r, f64::max);
    for (kind, radius) in &radii {
        if *kind != "plane" && (radius - r).abs() > 4.0 * f64::EPSILON * scale {
            bad.push(format!("{tag}: a {kind} of radius {radius}, not {r}"));
        }
    }
    let (area, volume) = rounded_box_closed_form(size[0], size[1], size[2], r);
    let e = enclose_mass_properties(&k, &mut b, ENCLOSURE_WIDTH).unwrap();
    let mut held = vec![("area", e.area, area), ("volume", e.volume, volume)];
    for c in 0..3 {
        held.push(("centroid", e.centroid[c], origin[c] + size[c] / 2.0));
    }
    for (what, band, exact) in held {
        // the closed form's own rounding: a few ulps of the value
        let slack = 16.0 * f64::EPSILON * exact.abs().max(scale);
        if !(band.lo - slack <= exact && exact <= band.hi + slack) {
            bad.push(format!(
                "{tag}: the {what} {exact} outside its enclosure [{}, {}]",
                band.lo, band.hi
            ));
        }
    }
    eprintln!(
        "I18913 {tag}: closed form area {area}, volume {volume} enclosed ({:e}, {:e} wide)",
        e.area.width(),
        e.volume.width()
    );
}

// ---- rail_eu: the root by Cramer's rule in intervals --------------------------------------

type I3 = [Ivl; 3];

fn iv(x: f64) -> Ivl {
    Ivl::from(x)
}

fn sub3(a: I3, b: I3) -> I3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add3(a: I3, b: I3) -> I3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(s: Ivl, a: I3) -> I3 {
    [s * a[0], s * a[1], s * a[2]]
}

fn cross3(a: I3, b: I3) -> I3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: I3, b: I3) -> Ivl {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn det3(a: I3, b: I3, c: I3) -> Ivl {
    dot3(a, cross3(b, c))
}

/// A planar parallelogram net `P(u, v) = P00 + (u - u0)/(u1 - u0) E + (v - v0)/(v1 - v0) F`
/// (degree 1 by 1, integer points, `P00 + P11 = P10 + P01`): `(P00, E, F, u0, du, v0, dv)`.
struct Para {
    p00: I3,
    e: I3,
    f: I3,
    u0: f64,
    du: f64,
    v0: f64,
    dv: f64,
}

impl Para {
    /// A base surface input: a bilinear B-spline net, or a real `SmPlane` (`Plane::from_normal`,
    /// whose `P(u, v) = O + u X + v Y` is affine: its point and first derivatives at
    /// `(0, 0)`, the kernel's own floats, are the exact parallelogram the kernel evaluates).
    fn of(s: &Value) -> Para {
        if let Some(p) = s.get("plane") {
            let (o, n) = (nums(&p["o"]), nums(&p["n"]));
            let plane = solidcore::surface::plane::Plane::from_normal(
                Vec3::new(o[0], o[1], o[2]),
                Vec3::new(n[0], n[1], n[2]),
            );
            let d = plane
                .evaluate(solidcore::math::Point2::new(0.0, 0.0), 1, 1, false)
                .unwrap();
            let i3 = |v: Vec3| [iv(v.x), iv(v.y), iv(v.z)];
            return Para {
                p00: i3(d[0][0]),
                e: i3(d[1][0]),
                f: i3(d[0][1]),
                u0: 0.0,
                du: 1.0,
                v0: 0.0,
                dv: 1.0,
            };
        }
        Para::from_bspline(&s["bspline"])
    }

    fn from_bspline(b: &Value) -> Para {
        assert_eq!(b["p"], 1, "degree 1");
        assert_eq!(b["q"], 1, "degree 1");
        let net: Vec<Vec<Vec<f64>>> = b["net"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_array().unwrap().iter().map(nums).collect())
            .collect();
        assert!(net.len() == 2 && net.iter().all(|r| r.len() == 2), "2 x 2");
        // non-rational (`w` unset)
        assert!(
            net.iter().flatten().all(|cp| !(cp[3] > -1e300)),
            "non-rational"
        );
        let p = |i: usize, j: usize| -> [f64; 3] { [net[i][j][0], net[i][j][1], net[i][j][2]] };
        for c in 0..3 {
            assert!(
                net.iter()
                    .flatten()
                    .all(|cp| cp[c].fract() == 0.0 && cp[c].abs() < 1e6),
                "integer points"
            );
            assert_eq!(
                p(0, 0)[c] + p(1, 1)[c],
                p(0, 1)[c] + p(1, 0)[c],
                "a parallelogram"
            );
        }
        let (u, v) = (nums(&b["u"]), nums(&b["v"]));
        assert!(u.len() == 4 && v.len() == 4 && u[0] == u[1] && u[2] == u[3]);
        assert!(v[0] == v[1] && v[2] == v[3]);
        let i3 = |a: [f64; 3]| a.map(iv);
        // the differences of integers below 1e6 are exact
        let d = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        Para {
            p00: i3(p(0, 0)),
            e: i3(d(p(1, 0), p(0, 0))),
            f: i3(d(p(0, 1), p(0, 0))),
            u0: u[0],
            du: u[2] - u[0],
            v0: v[0],
            dv: v[2] - v[0],
        }
    }

    /// `S_u`, `S_v`.
    fn derivs(&self) -> (I3, I3) {
        (
            scale3(iv(1.0) / iv(self.du), self.e),
            scale3(iv(1.0) / iv(self.dv), self.f),
        )
    }

    /// The unit normal `S_u x S_v / |S_u x S_v|`.
    fn normal(&self) -> I3 {
        let (su, sv) = self.derivs();
        let n = cross3(su, sv);
        let len = dot3(n, n).sqrt();
        n.map(|c| c / len)
    }

    /// The offset point `P(u, v) + d n` for interval `u`, `v`.
    fn offset(&self, u: Ivl, v: Ivl, d: f64) -> I3 {
        let (su, sv) = self.derivs();
        let p = add3(
            self.p00,
            add3(scale3(u - iv(self.u0), su), scale3(v - iv(self.v0), sv)),
        );
        add3(p, scale3(iv(d), self.normal()))
    }
}

/// The root of a `rail_eu` case's affine equations (`trim(t) = uv_rail`,
/// `P_rail(uv_rail) + d_rail n_rail = P_other(uv_other) + d_other n_other`) enclosed:
/// `[t, u_rail, v_rail, u_other, v_other]`. With the trim `c(t) = C0 + (t - k0) a`, the rail
/// offset point is `G + t H`, so `t H - u_o S_u^o - v_o S_v^o = K`, solved by Cramer's rule.
fn rail_eu_root(
    rail: &Para,
    d_rail: f64,
    other: &Para,
    d_other: f64,
    c0: [f64; 2],
    c1: [f64; 2],
    k0: f64,
    k1: f64,
) -> [Ivl; 5] {
    let dk = iv(k1) - iv(k0);
    let a = [(iv(c1[0]) - iv(c0[0])) / dk, (iv(c1[1]) - iv(c0[1])) / dk];
    let (ru, rv) = rail.derivs();
    // c(t) = (C0 - k0 a) + t a
    let base = [iv(c0[0]) - iv(k0) * a[0], iv(c0[1]) - iv(k0) * a[1]];
    let g = rail.offset(base[0], base[1], d_rail);
    let h = add3(scale3(a[0], ru), scale3(a[1], rv));
    let (ou, ov) = other.derivs();
    let o0 = other.offset(iv(0.0), iv(0.0), d_other);
    let k = sub3(o0, g);
    let (m1, m2) = (ou.map(|c| iv(0.0) - c), ov.map(|c| iv(0.0) - c));
    let det = det3(h, m1, m2);
    assert!(det.lo > 0.0 || det.hi < 0.0, "a regular system: {det:?}");
    let t = det3(k, m1, m2) / det;
    let uo = det3(h, k, m2) / det;
    let vo = det3(h, m1, k) / det;
    let ur = base[0] + t * a[0];
    let vr = base[1] + t * a[1];
    [t, ur, vr, uo, vo]
}

fn certify_rail_eu(case: &goldens::Case, out: &Output, bad: &mut Vec<String>) {
    let name = &case.name;
    let inputs = &case.inputs;
    assert_eq!(inputs["op"], "rail_eu", "{name}: a rail_eu case");
    let extra = inputs["cfg"]["extra"].as_u64().unwrap() as usize;
    let target = num(&inputs["cfg"]["target"]);
    let rail = inputs["rail"].as_u64().unwrap() as usize;
    let surfs = &inputs["surfs"];
    assert!(surfs["has"].as_array().unwrap().iter().all(|h| h == true));
    let v = out.values.get("flat").expect("flat");
    assert_eq!(v[4], 1.0, "{name}: found");
    assert_eq!(
        v[10 + extra].to_bits(),
        v[5].to_bits(),
        "{name}: curve_parameter is t"
    );
    let (ir, io) = if rail == 0 { (6, 8) } else { (8, 6) };
    let mut xk = vec![v[5], v[ir], v[ir + 1], v[io], v[io + 1]];
    xk.extend_from_slice(&v[10..10 + extra]);
    let x = [v[5], v[ir], v[ir + 1], v[io], v[io + 1]];
    let sc = goldens::units::blend_fillet_solver_p2::rail_eu_scene(case, &[xk])
        .unwrap()
        .expect("the case builds its solve");
    // a B-spline trim, or a line whose NURBS form (degree 1, its two ends) the kernel's
    // evaluation follows
    let tb: std::borrow::Cow<'_, _> = match sc.trim.kind() {
        CurveKind::BSpline(b) => std::borrow::Cow::Borrowed(b),
        CurveKind::Line(l) => l.base_with_nurb().unwrap(),
        other => panic!("{name}: a {} trim", other.name()),
    };
    assert_eq!(tb.degree().unwrap(), 1, "{name}: a degree-1 trim");
    assert_eq!(
        tb.number_control_points().unwrap(),
        2,
        "{name}: one segment"
    );
    let (kv, _) = tb.knots(None).unwrap();
    let cp = |i: usize| {
        let (q, w) = tb
            .control_point(ControlPointForm::HomogeneousRational, i)
            .unwrap();
        assert_eq!(w, 1.0, "{name}: non-rational");
        [q.x, q.y]
    };
    let para = |i: usize| Para::of(&surfs["base"][i]);
    let root = rail_eu_root(
        &para(rail),
        num(&surfs["off"][rail]),
        &para(1 - rail),
        num(&surfs["off"][1 - rail]),
        cp(0),
        cp(1),
        kv[0],
        kv[kv.len() - 1],
    );
    // a root strictly inside the solve's box: found is right
    let inside = |r: Ivl, lo: f64, hi: f64| lo < r.lo && r.hi < hi;
    let mut ok = inside(root[0], sc.interval.min(), sc.interval.max());
    for (k, d) in sc.domains.iter().enumerate() {
        ok &= inside(root[1 + 2 * k], d.min().x, d.max().x);
        ok &= inside(root[2 + 2 * k], d.min().y, d.max().y);
    }
    if !ok {
        bad.push(format!(
            "{name}: the root {root:?} not strictly inside the solve's box: t in {:?}, uv in {:?}",
            sc.interval, sc.domains
        ));
    }
    for (i, (&xi, r)) in x.iter().zip(&root).enumerate() {
        let gap = (xi - r.lo).abs().max((xi - r.hi).abs());
        // relative to the value, at least the unit scale of the boxes' parameters
        if !(gap <= 1e-9 * xi.abs().max(r.mag()).max(1.0)) {
            bad.push(format!(
                "{name}: unknown {i} {xi} is {gap:e} off the root {r:?}"
            ));
        }
        eprintln!(
            "I18913 {name}: unknown {i} {xi}, root [{}, {}], {gap:e} off",
            r.lo, r.hi
        );
    }
    for k in 0..extra {
        if !values_agree(v[10 + k], target + k as f64, None) {
            bad.push(format!(
                "{name}: extra {k} {} not {}",
                v[10 + k],
                target + k as f64
            ));
        }
    }
}

// ---- a chamfer turned about an arc ---------------------------------------------------------

/// `gen0224_torus` (a refusal before) calls `CalcTorusFilletGeom` directly (`which` 2) with a
/// linear section (`fsg` 0) on an arc edge of radius `big` about the `z` axis. Upstream
/// refuses a section that is not circular; WR-421 revolves the linear one into its cone. The
/// builder reads the section's two rail points, not the base surfaces: the start section
/// runs from `(rho, 0, 0)` on the floor (`rho = big - r`) to `(big, 0, r)` on the wall, so
/// the revolution is the cone `|xy| = rho + z` about `z`, between the circles `|xy| = rho,
/// z = 0` and `|xy| = big, z = r`. Every point of the fillet surface's 3 x 3 grid lies on
/// that cone with `0 <= z <= r`, and each rail's three points on one end circle, the two
/// rails on different circles, each to `1e-9 max(big, 1)`. These are every point the row
/// records, so the proof covers the row, not the surface between its grid points. Flat layout: `0 0`, the surface
/// `1 u0 v0 u1 v1` and its 27 grid coordinates, two rails `1 t0 t1` and 9 coordinates each,
/// the four interval values, the centre line `-1` (none).
fn certify_arc_chamfer_cone(case: &goldens::Case, out: &Output, bad: &mut Vec<String>) {
    let name = &case.name;
    assert_eq!(case.inputs["which"], 2, "{name}: CalcTorusFilletGeom");
    assert_eq!(case.inputs["fsg"], 0, "{name}: a linear section");
    let surfs = &case.inputs["surfs"];
    let off = nums(&surfs["off"]);
    let r = off[0];
    assert_eq!(off[1], -r, "{name}: the floor offset r, the wall -r");
    let net = &surfs["base"][1]["bspline"]["net"];
    let big = num(&net[0][0][0]);
    for row in net.as_array().unwrap() {
        for p in row.as_array().unwrap() {
            assert_eq!(num(&p[0]), big, "{name}: the wall is the plane x = big");
        }
    }
    let rho = big - r;
    let v = out.values.get("flat").expect("flat");
    assert_eq!(v.len(), 63, "{name}: the flat record");
    assert_eq!(&v[..3], &[0.0, 0.0, 1.0], "{name}: built, with a surface");
    assert_eq!(v[62], -1.0, "{name}: no centre line");
    let tol = 1e-9 * big.max(1.0);
    let point = |i: usize| [v[i], v[i + 1], v[i + 2]];
    let radial = |p: [f64; 3]| (p[0] * p[0] + p[1] * p[1]).sqrt();
    let off_cone = |p: [f64; 3]| radial(p) - (rho + p[2]);
    for g in 0..9 {
        let p = point(7 + 3 * g);
        let e = off_cone(p);
        if !(e.abs() <= tol && -tol <= p[2] && p[2] <= r + tol) {
            bad.push(format!(
                "{name}: grid point {g} {p:?} is {e:e} off the cone"
            ));
        }
    }
    let mut feet = Vec::new();
    for rail in 0..2 {
        let at = 34 + 12 * rail;
        assert_eq!(v[at], 1.0, "{name}: rail {rail} built");
        let ps: Vec<[f64; 3]> = (0..3).map(|q| point(at + 3 + 3 * q)).collect();
        let floor = ps
            .iter()
            .all(|&p| p[2].abs() <= tol && (radial(p) - rho).abs() <= tol);
        let wall = ps
            .iter()
            .all(|&p| (p[2] - r).abs() <= tol && (radial(p) - big).abs() <= tol);
        if !(floor || wall) {
            bad.push(format!(
                "{name}: rail {rail} {ps:?} is on neither end circle"
            ));
        }
        feet.push(floor);
    }
    if feet[0] == feet[1] {
        bad.push(format!("{name}: both rails on the same circle"));
    }
    eprintln!(
        "I18913 {name}: the cone |xy| = {rho} + z about z, z in [0, {r}]: grid and rails checked"
    );
}

// ---- the tests ----------------------------------------------------------------------------

#[test]
fn issue_18913_moved_rows_replay_and_are_proven() {
    let mut units = Units::new();
    let mut bad = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut full_vertices: Option<Vec<(Vec3, f64)>> = None;
    let mut checkpoints: Vec<(String, Vec<f64>)> = Vec::new();
    for &(unit, name, class) in &ROWS {
        *counts.entry(format!("{class:?}")).or_default() += 1;
        let (case, out, bodies) = units.replay(unit, name, &mut bad);
        let tag = format!("{unit}/{name}");
        match class {
            Class::Body | Class::BodyFromRefusal => {
                let body = the_body(&bodies, "mass", name);
                if HELD_TO_INPUT.contains(&name) {
                    certify_against_input(&tag, &case, body, &mut bad);
                } else {
                    certify_against_main(&tag, name, body, &mut bad);
                }
                if name == "issue_2020_mitred_put_back_d9_full" {
                    let vs = p1::vertices(body, None).unwrap();
                    full_vertices = Some(
                        vs.iter()
                            .map(|&v| {
                                (
                                    vertex::point(body, v).unwrap(),
                                    vertex::tolerance(body, v).unwrap(),
                                )
                            })
                            .collect(),
                    );
                }
            }
            Class::Shell => {
                let body = the_body(&bodies, "shell_mass", name);
                certify_into(&tag, body, &mut bad);
                let (area, volume) = shell_closed_form(name);
                let new = out.values.get("shell_mass_properties").expect("shell mass");
                for (i, exact) in [area, volume, volume].into_iter().enumerate() {
                    let off = (new[i] - exact).abs();
                    eprintln!(
                        "I18913 {tag}: shell_mass_properties[{i}] {:e} off its closed form",
                        new[i] - exact
                    );
                    if !(off <= 1e-9 * exact) {
                        bad.push(format!(
                            "{tag}: shell_mass_properties[{i}] {} not within 1e-9 of {exact}",
                            new[i]
                        ));
                    }
                }
            }
            Class::Preview => {
                let n = certify_preview(&case, &mut bad);
                eprintln!("I18913 {tag}: {n} preview cylinders certified");
            }
            Class::PreviewPastFaces => {
                let v = out.values.get("preview").expect("preview");
                assert_eq!(v[0], 1001.0, "{tag}: refused with SM_ERR");
                if !radius_past_faces(&case) {
                    bad.push(format!("{tag}: a refusal of a radius that fits a face"));
                }
            }
            Class::Checkpoint => {
                // checked below, against the full body
                let v = out
                    .values
                    .get("all_corner_vertices")
                    .expect("all_corner_vertices");
                assert_eq!(v.len() % 5, 0, "{tag}: x y z tolerance t per vertex");
                checkpoints.push((tag.clone(), v.clone()));
            }
            Class::ChamferedCylinder => {
                let v = out.values.get("attributes").expect("attributes");
                let (areas, lengths) = chamfered_cylinder();
                check_chamfered_cylinder(&tag, v, &areas, &lengths, &mut bad);
            }
            Class::RoundedBox => {
                let mut boxes = Vec::new();
                filleted_boxes(&case.inputs, &mut boxes);
                if boxes.is_empty() {
                    bad.push(format!("{tag}: no rounded box in its inputs"));
                }
                for b in boxes {
                    rounded_box(&tag, b, &mut bad);
                }
            }
            Class::RailEu => certify_rail_eu(&case, &out, &mut bad),
            Class::ArcChamferCone => certify_arc_chamfer_cone(&case, &out, &mut bad),
        }
    }
    eprintln!("I18913 SUMMARY: {} rows: {counts:?}", ROWS.len());
    let full = full_vertices.expect("the full mitred case");
    check_mitred_corners(&full, &checkpoints, &mut bad);
    assert!(
        bad.is_empty(),
        "{} failures:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// `issue_2020_mitred_put_back_d9` in closed form (as `issue_2020_mitred_put_back_trimmed_at_
/// creases` derives it): the four r 3 rounds of the 40 x 30 box whose top moved down to
/// `z = 1`. Each round's axis runs 3 inside its side wall and 3 below the top (`z = -2`), so
/// its rail on the top is the line 3 inside that side at `z = 1` and its rail on the wall
/// (the plane of the side) the line at `z = -2`, below the 1 high slab (the wall run on
/// past its bottom edge, a cliff overflow). Two adjacent rounds meet in a mitre on the
/// diagonal, through the crossings of their rails: `(3 | 37, 3 | 27, 1)` on the top and
/// `(0 | 40, 0 | 30, -2)` on the walls. The result trims each round at its crease with
/// the bottom (WR-695), so the full body's vertices are the top's four and
/// `(a | 40 - a, a | 30 - a, 0)` with `a = 3 - sqrt(5)`.
///
/// Each vertex of the full body (itself certified as a result body) is one of those eight,
/// and each corner vertex a checkpoint row lists is one of the mitres' eight rail
/// crossings, each to `1e-9` of the box's size.
fn check_mitred_corners(
    full: &[(Vec3, f64)],
    checkpoints: &[(String, Vec<f64>)],
    bad: &mut Vec<String>,
) {
    let tol = 1e-9 * 40.0;
    let a = 3.0 - 5f64.sqrt();
    let points = |xs: [f64; 2], ys: [f64; 2], z: f64| {
        let mut p = Vec::new();
        for x in xs {
            for y in ys {
                p.push(Vec3::new(x, y, z));
            }
        }
        p
    };
    let top = points([3.0, 37.0], [3.0, 27.0], 1.0);
    let mut result = top.clone();
    result.extend(points([a, 40.0 - a], [a, 30.0 - a], 0.0));
    let mut crossings = top;
    crossings.extend(points([0.0, 40.0], [0.0, 30.0], -2.0));
    let nearest = |p: Vec3, to: &[Vec3]| {
        to.iter()
            .map(|q| p.distance(*q))
            .fold(f64::INFINITY, f64::min)
    };
    if full.len() != 8 {
        bad.push(format!(
            "the full mitred body: {} vertices, not 8",
            full.len()
        ));
    }
    for (i, (p, _)) in full.iter().enumerate() {
        let gap = nearest(*p, &result);
        eprintln!("I18913 issue_2020_mitred_put_back_d9_full: vertex {i} {p:?}: {gap:e} off its closed form");
        if !(gap <= tol) {
            bad.push(format!(
                "the full mitred body: vertex {i} {p:?} {gap:e} off its closed form"
            ));
        }
    }
    for (tag, v) in checkpoints {
        if v.len() != 8 * 5 {
            bad.push(format!("{tag}: {} corner vertices, not 8", v.len() / 5));
        }
        for (i, c) in v.chunks(5).enumerate() {
            let p = Vec3::new(c[0], c[1], c[2]);
            let gap = nearest(p, &crossings);
            eprintln!(
                "I18913 {tag}: corner vertex {i} {p:?} t {}: {gap:e} off a rail crossing",
                c[4]
            );
            if !(gap <= tol) {
                bad.push(format!(
                    "{tag}: corner vertex {i} {p:?} {gap:e} off every rail crossing"
                ));
            }
        }
    }
}

/// The chamfered cylinder's rows (`n`, then per face `area k (id behavior value users) x
/// k`, then the same for edges): the five face areas are the five closed forms, each within
/// the record's area accuracy (`1e-4` relative, `AREA_ACCURACY`), and every edge length is
/// a closed form within its length accuracy (`1e-9` relative), the four rims once each.
fn check_chamfered_cylinder(
    tag: &str,
    v: &[f64],
    areas: &[f64],
    lengths: &[f64],
    bad: &mut Vec<String>,
) {
    let mut at = 0;
    let mut measures = |v: &[f64]| -> Vec<f64> {
        let n = v[at] as usize;
        at += 1;
        let mut out = Vec::new();
        for _ in 0..n {
            out.push(v[at]);
            let k = v[at + 1] as usize;
            at += 2 + 4 * k;
        }
        out
    };
    let faces = measures(v);
    let edges = measures(v);
    assert_eq!(at, v.len(), "{tag}: the whole record read");
    eprintln!("I18913 {tag}: faces {faces:?}, edges {edges:?}");
    let mut unused: Vec<f64> = areas.to_vec();
    for a in &faces {
        match unused.iter().position(|x| (a - x).abs() <= 1e-4 * x) {
            Some(i) => {
                eprintln!(
                    "I18913 {tag}: area {a} is {} off {}",
                    a - unused[i],
                    unused[i]
                );
                unused.remove(i);
            }
            None => bad.push(format!("{tag}: the face area {a} is no closed form")),
        }
    }
    if !unused.is_empty() {
        bad.push(format!("{tag}: closed-form areas {unused:?} with no face"));
    }
    let mut rims = [0usize; 2];
    for l in &edges {
        match lengths.iter().position(|x| (l - x).abs() <= 1e-9 * x) {
            Some(i) => {
                if let Some(n) = rims.get_mut(i / 2) {
                    *n += 1;
                }
                eprintln!(
                    "I18913 {tag}: length {l} is {:e} off {}",
                    l - lengths[i],
                    lengths[i]
                );
            }
            None => bad.push(format!("{tag}: the edge length {l} is no closed form")),
        }
    }
    if rims != [2, 2] {
        bad.push(format!(
            "{tag}: rim circles {rims:?} (outer, inner), not two of each"
        ));
    }
}

/// The pinned table is the moved entries of [`MASS_ROWS`], in order, and every row is in a
/// part.
/// [`MAIN_FAILURES`] names only result-body rows main built, and [`HELD_TO_INPUT`] only
/// rows main refused.
#[test]
fn main_failures_name_rows_main_built() {
    let class = |case: &str| {
        ROWS.iter()
            .find(|(unit, name, _)| *unit == P3 && *name == case)
            .map(|r| r.2)
    };
    for (case, failure, n) in MAIN_FAILURES {
        assert_eq!(class(case), Some(Class::Body), "{case}: {failure}");
        assert!(
            n > 0 && without_ids(failure) == failure,
            "{case}: {failure}"
        );
    }
    for row in HELD_TO_INPUT {
        assert_eq!(class(row), Some(Class::BodyFromRefusal), "{row}");
        assert!(MAIN_FAILURES.iter().all(|(c, _, _)| *c != row), "{row}");
    }
    assert_eq!(
        without_ids("edge EdgeId(47) (line) EdgeId(5)"),
        "edge EdgeId(_) (line) EdgeId(_)"
    );
}

#[test]
fn expected_lists_every_moved_entry() {
    let pinned: Vec<(&str, usize)> = EXPECTED.iter().map(|e| (e.0, e.1)).collect();
    assert_eq!(TABLE.moved(), pinned);
    assert!((0..MASS_ROWS.len()).all(|i| TABLE.part_of(i) < PARTS));
    assert!(
        EXPECTED.iter().all(|e| e.2 != Side::Farther),
        "a FARTHER entry"
    );
}

/// Each [`Class::BodyFromRefusal`] row's new `mass_properties` within the rows' `1e-4`
/// accuracy of the certified enclosure of its body.
#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn from_refusal_mass_is_enclosed() {
    let mut units = Units::new();
    let mut bad = Vec::new();
    for &(unit, name, class) in &ROWS {
        if class != Class::BodyFromRefusal {
            continue;
        }
        let (_, out, bodies) = units.replay(unit, name, &mut bad);
        let body = the_body(&bodies, "mass", name);
        let new = out.values.get("mass_properties").expect("mass");
        let k = Kernel::default();
        let mut b = body.clone_standing_for_self().unwrap();
        let e = enclose_mass_properties(&k, &mut b, ENCLOSURE_WIDTH).unwrap();
        let bb = solidcore::query::brep::calculate_bounding_box(
            &k,
            &mut b,
            solidcore::query::brep::BoundingBoxOptions::default(),
        )
        .unwrap();
        let r = 0.5 * bb.size().length();
        let vol = e.volume.hi;
        let mut bands = vec![(e.area, e.area.hi), (e.volume, vol), (e.volume, vol)];
        for (group, scale) in
            [e.centroid, e.inertia, e.products]
                .into_iter()
                .zip([r, vol * r * r, vol * r * r])
        {
            bands.extend(group.into_iter().map(|g| (g, scale)));
        }
        for (i, ((band, scale), &n)) in bands.iter().zip(new).enumerate() {
            let a = 1e-4 * scale;
            eprintln!(
                "I18913 {name}: {} {n:e}, enclosure [{:e}, {:e}]",
                harness::NAMES[i],
                band.lo,
                band.hi
            );
            if !(band.lo - a <= n && n <= band.hi + a) {
                bad.push(format!(
                    "{name}: {} {n} past its enclosure",
                    harness::NAMES[i]
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_mass_entries_are_classified_part_0() {
    TABLE.check(0, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_mass_entries_are_classified_part_1() {
    TABLE.check(1, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_mass_entries_are_classified_part_2() {
    TABLE.check(2, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_mass_entries_are_classified_part_3() {
    TABLE.check(3, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_mass_entries_are_classified_part_4() {
    TABLE.check(4, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_mass_entries_are_classified_part_5() {
    TABLE.check(5, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_mass_entries_are_classified_part_6() {
    TABLE.check(6, &EXPECTED, &WIDE);
}

#[test]
#[ignore = "encloses whole bodies (minutes); see the module docs"]
fn moved_mass_entries_are_classified_part_7() {
    TABLE.check(7, &EXPECTED, &WIDE);
}
