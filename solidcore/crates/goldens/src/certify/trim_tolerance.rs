//! The certificate of the goldens rows whose only change under the lazy UV trim's raise
//! (`topology::edgeuse::p3::trim_raise_distance`) is a lower tolerance: for each moved edge
//! or vertex tolerance `n` (it was `o`), `g <= B <= n < o`.
//!
//! [`ROWS`] holds, one line per case, the moved output values (unit, case, `[key, index, o,
//! n]`, the mechanical diff of the two goldens) and the moved entities (`[body, "e"|"v",
//! index, o, n]`, the edges and vertices of the B-reps the runner hands to [`super::written`]
//! whose tolerance differs between the two kernels, matched by entity, never by value). Here
//! each moved output value is checked bit for bit against the case's output and must be the
//! `o -> n` of a moved entity or an [`OTHER`] entry (a value that moved with them: rounding,
//! a box padded by a tolerance, a refit trim, a face domain bound to its trims; each class a
//! check), and each moved entity's tolerance must be `n` and certify:
//! - an edge: `B` is [`trim_image_gap_bound`] of each of its UV trims, a proved upper bound on
//!   the Hausdorff distance between the edge curve and the trim's image on its face (so on the
//!   true gap `g`); an edge whose B-rep keeps no UV trim has `B` the proved bound
//!   ([`curve_face_gap_bound`]) on the distance from its curve to each of its faces' surfaces;
//! - a vertex: its point is within `n` of the end of every edge curve that ends there, the
//!   rounding of each distance counted against it.
//!
//! So every lowered tolerance still covers what it is the tolerance for, and it is lower than
//! before: a tighter tolerance that is still valid.
//!
//! A raised tolerance (`n > o`, the corner fix's `keep_trim_covered` in
//! `topology::face::p2`) must cover the same way, and the old one must not have: for an edge
//! one of its trims' `B` exceeds `o` (the raise was needed for the enclosure; `B` is an upper
//! bound, so this argues rather than proves that `o` was short), for a vertex `o` does not
//! cover an edge end that `n` does.

use solidcore::curve::curve::p2::CurveKind;
use solidcore::kernel::Kernel;
use solidcore::nurbs::math::gamma;
use solidcore::query::curve_surface_gap::{curve_face_gap_bound, trim_image_gap_bound};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

use crate::case::repo_root;
use crate::CaseContext;

/// The moved values of each case whose moved values are all tolerances (below 0.05,
/// positive; lowered, or raised to cover a trim a corner fix moved) and whose output
/// changed nowhere else.
const ROWS: &str = include_str!("trim_tolerance_rows.jsonl");

/// The moved edges (and vertices) this certificate does not certify yet, one per line: unit,
/// case, body, edge index (`v` and the index for a vertex, accepted only when its old
/// tolerance did not cover its edge ends either), class, and (diagnostic only, never part of
/// a proof) a dense-sampling estimate of the edge's largest trim gap and its new tolerance.
/// Classes:
/// - `UNCOVERED`: the sampled gap of one of the edge's trims exceeds the new (and the old)
///   tolerance: an open defect, the tolerance does not cover that trim either way (none
///   left: the 27 there were now keep upstream's tolerance or are raised to cover);
/// - `loose`: the sampled gap is within the new tolerance but the enclosure `B` is not (for
///   `single_face_fillet1019_8` the gap is a sampled Hausdorff distance: the trims' images
///   run up to 6.8e-4 from the edge curve at the same parameter but 2.16e-4 from the curve);
/// - `no-estimate`: the trim's image could not be sampled;
/// - `face-bound-loose`: an edge whose B-rep keeps no UV trim, whose face-gap enclosure
///   exceeds the new tolerance.
/// - `line-trim`: a plane's UV trim that is a line, which [`trim_image_gap_bound`] does not
///   enclose (a line's evaluation rounding moves its parameter, see there):
///   `real-box-fillet1v/4v-r3-shell-closed`, whose tolerances halve (`2e-5 -> 1e-5`,
///   `8e-5 -> 4e-5`) with the trims unchanged, the sampled gaps at most 5.9e-7, 6 % of the new tolerance;
/// - `invalid-input`: a body invalid before and after (`remove_topo_edges_mouse_two_struts`:
///   the case asserts the same validity failures on both): one trim's image runs about 11.5
///   from the edge curve, past the old tolerance (3.674, upstream's raise for the other trim's
///   fit) and the new one (3.630, the zone tolerance; that other trim's `B` is 0.035).
///
/// The test requires the uncertified edges to be exactly these, so an entry that becomes
/// certified (or a new failure) shows up.
const OPEN: &str = include_str!("trim_tolerance_open.tsv");

/// [`OPEN`] entries of `unit`: `(case, body, entity)`, the entity an edge's index or `v` and
/// a vertex's.
fn open_entries(unit: &str) -> std::collections::BTreeSet<(String, usize, String)> {
    OPEN.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.first() == Some(&unit)).then(|| {
                (
                    f[1].to_string(),
                    f[2].parse().expect("body"),
                    f[3].to_string(),
                )
            })
        })
        .collect()
}

/// The moved output values that are not a moved entity's tolerance, one per line: unit,
/// case, key, index, class, and the body ([`super::written`] order) the class is checked on.
/// Each class is a proof checked here:
/// - `rounding`: the value moved by at most two units in the last place: EQUAL (a
///   reordered sum's rounding, far below anything the case measures);
/// - `padding`: a box coordinate padded by an edge's tolerance (`query.brep`'s tight box
///   expanded by its entities' tolerances): it moved inward by exactly a moved edge's
///   `o - n` (to four units in the last place), that edge certified as every other;
/// - `trim`: a value of a UV trim (its interval or a sample) in `topology.brep_mirror`'s
///   `record`, refit while its edge's tolerance was lower: that trim's `B` is within its
///   edge's tolerance (a proved cover, the trim is valid for the tolerance it was fit to);
/// - `domain`: a value of a face's surface record (its natural domain, or a sample over it)
///   that moved with the face's refit trims (the surface's domain is bound to them): some
///   face's surface record holds it, and every UV trim on each face whose record holds it is
///   within its edge's tolerance (`fillet_group_torii_fillet_b_rb23`: the domain's `v` start moved 3.7e-7 and
///   its samples there now lie on the plane `z = 2` they are tangent to, 6.2e-6 off before).
const OTHER: &str = include_str!("trim_tolerance_other.tsv");

/// [`OTHER`] entries of `unit`: `(case, key, index) -> (class, body)`.
fn other_entries(
    unit: &str,
) -> std::collections::BTreeMap<(String, String, usize), (String, usize)> {
    OTHER
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.first() == Some(&unit)).then(|| {
                (
                    (
                        f[1].to_string(),
                        f[2].to_string(),
                        f[3].parse().expect("index"),
                    ),
                    (f[4].to_string(), f[5].parse().expect("body")),
                )
            })
        })
        .collect()
}

/// The distance from `a` to `b` in units in the last place of the larger magnitude (the
/// spacing of doubles just above it).
fn ulps(a: f64, b: f64) -> f64 {
    let m = a.abs().max(b.abs());
    (a - b).abs() / (f64::from_bits(m.to_bits() + 1) - m)
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
    rec.edgeuses
        .iter()
        .filter_map(|&eu| use_bound(k, b, e, eu))
        .collect()
}

/// `B` of the UV trim of `e`'s use `eu` ([`trim_bounds`]): `None` when the use keeps no trim
/// or the edge no curve, `Some(None)` where the bound is not enclosed.
fn use_bound(k: &Kernel, b: &Brep, e: EdgeId, eu: EdgeuseId) -> Option<Option<f64>> {
    let rec = b.edge(e).expect("edge");
    let curve = rec.curve.as_ref()?;
    let trim = b.edgeuse(eu).expect("edge use").uv_trim_curve.as_ref()?;
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
    Some(
        trim_image_gap_bound(
            k,
            &surface,
            curve,
            trim,
            [ivl.min(), ivl.max()],
            FLOOR * rec.tolerance,
        )
        .expect("the bound"),
    )
}

/// The owner of a value of a geometry record (`topology_brep_mirror::geometry`).
#[derive(Clone, Copy, PartialEq)]
enum Owner {
    /// A count, an edge curve's value, a status.
    Other,
    /// A value of the UV trim of that edge use (its type, natural interval and samples).
    Trim(EdgeId, EdgeuseId),
    /// A value of that face's surface (its type, natural domain and samples).
    Face(FaceId),
}

/// The owner of each value of a geometry record of `b`: `topology_brep_mirror::geometry`,
/// with the UV trims when `uv_trim` (`topology.brep_mirror`'s `record`, after its status:
/// `status`), without them in `blend.feature_executive`'s `record`.
fn record_owners(k: &Kernel, b: &Brep, status: bool, uv_trim: bool) -> Vec<Owner> {
    // as `curve_samples`: -1 for no curve, else the type, the interval and five samples
    // (four values each, one for a sample that does not evaluate)
    let curve = |c: Option<&solidcore::curve::curve::Curve>| match c {
        None => 1,
        Some(c) => {
            let d = c.natural_interval();
            3 + (0..5)
                .map(|i| {
                    if c.evaluate_point(k, d.evaluate(f64::from(i) / 4.0)).is_ok() {
                        4
                    } else {
                        1
                    }
                })
                .sum::<usize>()
        }
    };
    // as `surface_samples`: -1 for no surface, else the type, the domain's corners and the
    // 3 x 3 grid of samples
    let surface = |s: Option<&solidcore::surface::surface::Surface>| match s {
        None => 1,
        Some(s) => {
            let d = s.natural_uv_domain_if_built();
            let (a, z) = (d.min(), d.max());
            let mut n = 5;
            for i in 0..3 {
                for j in 0..3 {
                    let uv = solidcore::geom::vector2d::Point2::new(
                        a.x + (z.x - a.x) * f64::from(i) / 2.0,
                        a.y + (z.y - a.y) * f64::from(j) / 2.0,
                    );
                    n += if s.evaluate_point(k, uv).is_ok() {
                        4
                    } else {
                        1
                    };
                }
            }
            n
        }
    };
    // the status, the edge count
    let mut out = vec![Owner::Other; usize::from(status) + 1];
    for e in solidcore::topology::brep::p1::edges(b, None).expect("edges") {
        let rec = b.edge(e).expect("edge");
        out.extend(std::iter::repeat_n(
            Owner::Other,
            curve(rec.curve.as_ref()) + 1,
        ));
        if uv_trim {
            for &eu in &rec.edgeuses {
                let n = curve(b.edgeuse(eu).expect("edge use").uv_trim_curve.as_ref());
                out.extend(std::iter::repeat_n(Owner::Trim(e, eu), n));
            }
        }
    }
    // the face count
    out.push(Owner::Other);
    for f in solidcore::topology::brep::p1::faces(b, None).expect("faces") {
        let n = surface(b.face(f).expect("face").surface.as_ref());
        out.extend(std::iter::repeat_n(Owner::Face(f), n));
    }
    out
}

/// The face of `b` whose surface record (`topology_brep_mirror`'s `surface_samples`: its
/// natural domain's corners and the coordinates of the 3 x 3 grid of samples over it) holds
/// `value` bit for bit.
fn faces_with_surface_value(k: &Kernel, b: &Brep, value: f64) -> Vec<FaceId> {
    solidcore::topology::brep::p1::faces(b, None)
        .expect("faces")
        .into_iter()
        .filter(|&f| {
            let Some(s) = b.face(f).expect("face").surface.as_ref() else {
                return false;
            };
            let d = s.natural_uv_domain_if_built();
            let (a, z) = (d.min(), d.max());
            let mut vals = vec![a.x, a.y, z.x, z.y];
            for i in 0..3 {
                for j in 0..3 {
                    let uv = solidcore::geom::vector2d::Point2::new(
                        a.x + (z.x - a.x) * f64::from(i) / 2.0,
                        a.y + (z.y - a.y) * f64::from(j) / 2.0,
                    );
                    if let Ok(p) = s.evaluate_point(k, uv) {
                        vals.extend([p.x, p.y, p.z]);
                    }
                }
            }
            vals.iter().any(|v| v.to_bits() == value.to_bits())
        })
        .collect()
}

/// `B` of every UV trim on face `f` against its edge's tolerance: `Ok(worst B / tol)`, or the
/// first trim that is not covered.
fn face_trims_covered(k: &Kernel, b: &Brep, f: FaceId) -> Result<f64, String> {
    let mut worst = 0.0_f64;
    let mut any = false;
    for e in solidcore::topology::brep::p1::edges(b, None).expect("edges") {
        let rec = b.edge(e).expect("edge");
        for &eu in &rec.edgeuses {
            if eu1::face(b, eu).expect("face") != Some(f) {
                continue;
            }
            match use_bound(k, b, e, eu) {
                None => {}
                Some(Some(x)) if x <= rec.tolerance => {
                    any = true;
                    worst = worst.max(x / rec.tolerance);
                }
                x => {
                    return Err(format!(
                        "edge {} use {}: B {x:?} over {:e}",
                        e.index(),
                        eu.index(),
                        rec.tolerance
                    ))
                }
            }
        }
    }
    if any {
        Ok(worst)
    } else {
        Err("no UV trim on the face".to_string())
    }
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
    /// Raised edges: every trim's `B` within the new tolerance, one above the old.
    raised: usize,
    /// [`OTHER`] values: `rounding`, `padding`, `trim` and `domain`.
    rounding: usize,
    padding: usize,
    trim_values: usize,
    domain: usize,
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
    let other = other_entries(unit);
    let mut other_seen = std::collections::BTreeSet::new();
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
            if row
                .entities
                .iter()
                .any(|e| e.3.to_bits() == old.to_bits() && e.4.to_bits() == new.to_bits())
            {
                tally.values += 1;
                continue;
            }
            let entry = (row.case.clone(), key.clone(), *index);
            let (class, body) = other
                .get(&entry)
                .unwrap_or_else(|| panic!("{at}: {old:e} -> {new:e} is no moved entity's"));
            other_seen.insert(entry);
            match class.as_str() {
                "rounding" if ulps(*old, *new) <= 2.0 => tally.rounding += 1,
                "padding"
                    if row.entities.iter().any(|e| {
                        e.1 && e.4 < e.3
                            && ((new - old).abs() - (e.3 - e.4)).abs()
                                <= 4.0 * f64::EPSILON * old.abs().max(new.abs())
                    }) && new.abs() < old.abs() =>
                {
                    tally.padding += 1;
                }
                "trim" => {
                    let b = bodies.get(*body).unwrap_or_else(|| panic!("{at}: no body"));
                    let owner = record_owners(&k, b, true, true).get(*index).copied();
                    let Some(Owner::Trim(e, eu)) = owner else {
                        panic!("{at}: not a UV trim's value");
                    };
                    let tol = b.edge(e).expect("edge").tolerance;
                    match use_bound(&k, b, e, eu) {
                        Some(Some(x)) if x <= tol => {
                            tally.trim_values += 1;
                            tally.worst = tally.worst.max(x / tol);
                        }
                        x => failures.push(format!(
                            "{at}: the trim of edge {} use {}: B {x:?} over {tol:e}",
                            e.index(),
                            eu.index()
                        )),
                    }
                }
                "domain" => {
                    let b = bodies.get(*body).unwrap_or_else(|| panic!("{at}: no body"));
                    // a `record` by its layout, a graph's `geometry` by the value
                    let faces = if key.ends_with("record") {
                        match record_owners(&k, b, false, false).get(*index) {
                            // (and the face's record holds the value: the layout is right)
                            Some(&Owner::Face(f))
                                if faces_with_surface_value(&k, b, *new).contains(&f) =>
                            {
                                vec![f]
                            }
                            _ => Vec::new(),
                        }
                    } else {
                        faces_with_surface_value(&k, b, *new)
                    };
                    let covered = if faces.is_empty() {
                        Err("no face's surface record".to_string())
                    } else {
                        faces.iter().try_fold(0.0_f64, |w, &f| {
                            face_trims_covered(&k, b, f).map(|x| w.max(x))
                        })
                    };
                    match covered {
                        Ok(w) => {
                            tally.domain += 1;
                            tally.worst = tally.worst.max(w);
                        }
                        Err(why) => failures.push(format!("{at}: {why}")),
                    }
                }
                _ => failures.push(format!("{at}: {old:e} -> {new:e} is not `{class}`")),
            }
        }
        // each moved entity: its tolerance covers what it is the tolerance for, and is lower,
        // or raised past what the old one covered
        for &(body, is_edge, index, old, new) in &row.entities {
            let kind = if is_edge { "edge" } else { "vertex" };
            let at = format!("{unit} {} body {body} {kind} {index}", row.case);
            assert!(new != old && new > 0.0, "{at}: {old:e} -> {new:e}");
            let raised = new > old;
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
                let key = (row.case.clone(), body, index.to_string());
                if raised {
                    // a raise: the new tolerance covers every trim, and one trim's bound is
                    // above the old tolerance (not in OPEN: a raise is certified or fails)
                    let past = bounds.iter().flatten().any(|x| *x > old);
                    if bad.is_empty() && past {
                        tally.raised += 1;
                    } else {
                        failures.push(format!(
                            "{at}: raised {old:e} -> {new:e}: {bad:?}, a bound past old: {past}"
                        ));
                    }
                } else if bad.is_empty() {
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
                let key = (row.case.clone(), body, format!("v{index}"));
                if raised && vertex_covers_its_ends(&k, b, v, old) {
                    failures.push(format!("{at}: raised {old:e} -> {new:e}: old covered"));
                } else if vertex_covers_its_ends(&k, b, v, new) {
                    tally.vertices += 1;
                    assert!(!open.contains(&key), "{at}: certified, remove it from OPEN");
                } else if open.contains(&key) && !vertex_covers_its_ends(&k, b, v, old) {
                    // uncovered either way: not farther than it was
                    open_seen.insert(key);
                    tally.open += 1;
                } else {
                    failures.push(format!("{at}: {old:e} -> {new:e}: an edge end is farther"));
                }
            }
        }
    }
    println!("{unit}: {tally:?}");
    assert_eq!(open_seen, open, "{unit}: OPEN entries not reached");
    assert_eq!(
        other_seen,
        other.keys().cloned().collect(),
        "{unit}: OTHER entries not reached"
    );
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
                assert!(t.values + t.rounding + t.padding + t.trim_values + t.domain > 0, "{t:?}");
            }
        )*
    };
}

certified_units! {
    api_primitives: "primitive.api_primitives",
    primitive_creation_p1: "primitive.primitive_creation_p1",
    fillet_executive_p1: "blend.fillet_executive_p1",
    fillet_executive_p3: "blend.fillet_executive_p3",
    feature_executive: "blend.feature_executive",
    stitch_p1: "heal.stitch_p1",
    stitch_p2: "heal.stitch_p2",
    brep: "query.brep",
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
