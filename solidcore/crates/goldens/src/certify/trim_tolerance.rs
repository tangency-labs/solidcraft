//! The certificate of the goldens rows whose only change under the lazy UV trim's raise
//! (`topology::edgeuse::p3::trim_raise_distance`) and the corner fix's cover
//! (`topology::face::p2`) is a moved tolerance: for each moved edge or vertex tolerance `n`
//! (it was `o`), `g <= B <= n`, with `n < o` or the raise certified as below.
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
//!   true gap `g`); on a face of the edge where it holds no UV trim, `B` is the proved bound
//!   ([`curve_face_gap_bound_within`]) on the distance from its curve to the face's surface
//!   (a UV trim that is a line is enclosed as its NURB, the segment between its two control
//!   points, which is the line's geometry up to the slack [`line_evaluation_slack`] bounds,
//!   added to `B`; see `use_bound`);
//! - a vertex: its point is within `n` of the end of every edge curve that ends there, the
//!   evaluation's error bound (`Curve::point_error_bound`, from the curve's data) and the
//!   distance's rounding counted against it.
//!
//! Trust base: the enclosures are the kernel's (`trim_image_gap_bound`,
//! `curve_face_gap_bound_within`, `line_evaluation_slack`, `plane_uv_lipschitz`,
//! `Curve::point_error_bound`), which the stitch cover (`topology::edgeuse::p3`) also raises
//! with; this checks that each final tolerance covers its bounds on the final body, not those
//! enclosures themselves (their own tests do).
//!
//! So every lowered tolerance still covers what it is the tolerance for, and it is lower than
//! before: a tighter tolerance that is still valid.
//!
//! A raised tolerance (`n > o`: the corner fix's `keep_trim_covered` in
//! `topology::face::p2`, or `trim_raise_distance` raising a refit trim's edge to a proved
//! bound past twice upstream's distance) must cover the same way, and the old one must not have: for an edge
//! one of its trims' `B` exceeds `o` (the raise was needed for the enclosure; `B` is an upper
//! bound, so this argues rather than proves that `o` was short), for a vertex `o` does not
//! cover an edge end that `n` does.

mod gap_samples;
mod p6_split;

use gap_samples::{GAP_CHANGES, GAP_MAIN};
use p6_split::{P6_CURVE_CLASS_CHANGES, P6_CURVE_CLASS_MAIN, P6_HEAL_CHANGES, P6_HEAL_MAIN};
use solidcore::curve::curve::p2::CurveKind;
use solidcore::curve::curve::Curve;
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_curve::ControlPointForm;
use solidcore::nurbs::math::CPoint;
use solidcore::query::curve_surface_gap::{
    clamped_trim_gap_bound, crv_on_surf_trim_gap_bound, curve_face_gap_bound_within,
    line_evaluation_slack, plane_uv_lipschitz, trim_image_gap_bound, uv_line_slack_on_surface,
};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, Brep, EdgeId, EdgeuseId, FaceId, VertexId};

use crate::case::repo_root;
use crate::CaseContext;

/// The moved values of each case whose moved values are all tolerances (positive; lowered,
/// or raised to cover a trim's proved bound) and whose output changed nowhere else.
const ROWS: &str = include_str!("trim_tolerance_rows.jsonl");

/// The moved edges (and vertices) this certificate does not certify yet, one per line: unit,
/// case, body, edge index (`v` and the index for a vertex, accepted only when its old
/// tolerance did not cover its edge ends either), class, (diagnostic only, never part of a
/// proof) a dense-sampling estimate of the edge's largest trim gap, and its new tolerance
/// (checked bit for bit for a `loose-raise`, [`open_tolerance`]; diagnostic otherwise).
/// Classes:
/// - `UNCOVERED`: the sampled gap of one of the edge's trims exceeds the new (and the old)
///   tolerance: an open defect, the tolerance does not cover that trim either way (the 27
///   there were before now keep upstream's tolerance or are raised to cover; left:
///   `offset_geometry_creation`'s `gen0642`, the offset's successive bodies 2 to 6, whose
///   edge 0 has a trim with an image up to 5.6e13 from the edge curve, sampled on body 2,
///   and whose vertex 0 the old tolerance did not cover either: that trim, on face 0, runs at
///   `u = 71.94`, `v` from 0 to 139.4, far outside the face's B-spline domain
///   `[-0.05, 1.05]^2`, so its image is the surface's extrapolation; the trim is in another
///   parameterisation than its face's, in both kernels, and no tolerance covers it; and
///   `topology.brep_p10`'s `gen0026` edges 20 and 33, [`DROP_MARGIN`] raises whose trim
///   on another face, edge use 78 on face 3 and 86 on face 4, is enclosed at 1.675e-5, past
///   both tolerances, on main's read as well: #20906);
/// - `rounding-bound`: a vertex of `primitive.api_primitives`' `gen1157`, about 8e8 from
///   the origin, where the evaluation's own error bound at that
///   scale exceeds what the tolerance can show: its edge ends evaluate onto its point, but
///   their curves' error bounds (1.4e-5 to 2.4e-5 a coordinate, times `sqrt 3`) exceed both
///   tolerances;
/// - `invalid-input`: a vertex of `gen0195`, whose input holds a non-finite coordinate
///   (`y = -inf`), so no tolerance covers its edge ends with their error bounds;
/// - `no-estimate`: the trim's image could not be sampled;
/// - `line-trim`: a lowered edge with a UV trim that is a line on a B-spline surface, which
///   is not enclosed (the line's own rounding moves its parameter, and only a plane carries
///   that to the surface: [`plane_uv_lipschitz`]), its other trims enclosed within the new
///   tolerance (`topology.brep_local_operation`'s `fc0000`, `fc0006`, `fc0008` and `fc0025`:
///   edge 89, lowered to 1e-5, its other trim's `B` 4.1e-8);
/// - `loose-raise`: the one class a raise may be open in: an edge raised to the kernel's
///   bound of a trim at the raise, where that bound is loose (its reparametrisation off the
///   correspondence over a short stretch, #19784's trim 3), so this test's enclosure of the
///   trims as kept shows none past the old tolerance; every enclosed trim is within the new
///   one, and a line trim on a B-spline surface stays open as in `line-trim` (the same edge
///   of `Solids/nurb_fillet.smb`, raised to 1.7e-3: `topology.brep_local_operation`'s four
///   `fc` cases, edge 93, its trim on face 39 enclosed at 1.7e-6; `topology.brep_mirror`'s
///   `mat0019`, `mat0049` and `mat0094`, edge 46);
/// - `face-bound-loose`: an edge whose B-rep keeps no UV trim, whose face-gap enclosure
///   exceeds the new tolerance.
/// - `blend-surface`: an edge with a trim on a procedural blend surface, which has no exact
///   NURBS net, so no enclosure (`fillet_executive_p3`'s coons, bottle and letter-g cases;
///   the sampled gaps, over every trim of the edge, at most 50 % of the new tolerance);
///
/// The test requires the uncertified edges to be exactly these, so an entry that becomes
/// certified (or a new failure) shows up.
const OPEN: &str = include_str!("trim_tolerance_open.tsv");

/// The edges raised by `SetUVTrimCurve`'s margin (`topology::edgeuse::p1::set_uv_trim_curve`:
/// an edge below twice its new trim's raise distance `r` is raised to `2 r`, upstream's
/// `TOL_FACTOR`) where this branch's read of `Fillet/sphere_diff_3cylinders.smb` makes the
/// classified trims (`tolsub::state::CLASSIFIED_READ_TRIMS`, #20179: the read's face
/// classification makes its missing trims) and main's read makes none: unit, case, body, edge.
/// `r` is upstream's drop distance `d` (the trim's `max_dist_to_surface`) or, where the trim's
/// proved bound is below it, that bound (`trim_raise_distance`, #18162), so `n = 2 r <= 2 d`.
/// The raise is not needed for the trim (its `B` is within the old tolerance): the margin is
/// upstream's, wider than the B-rep check needs (a gap within the tolerance), and moves every
/// dropped trim kernel-wide (#20912). Each is certified by [`drop_margin_raise`]: the
/// drop replayed on main's read gives `n` bit for bit, and `n` is `trim_raise_distance`'s
/// doubled, `2 d` with the made trim's `B` within `n`, or `2 B` with `B` below `d`. The
/// edge's vertices follow it (`edge::set_tolerance`).
const DROP_MARGIN: &[(&str, &str, usize, usize)] = &[
    ("topology.brep_p10", "gen0026", 0, 20),
    ("topology.brep_p10", "gen0026", 0, 22),
    ("topology.brep_p10", "gen0026", 0, 27),
    ("topology.brep_p10", "gen0026", 0, 31),
    ("topology.brep_p10", "gen0026", 0, 33),
];

/// The smallest coordinate magnitude at which a `rounding-bound` entry is accepted: the class
/// is the evaluation's own error bound at about 8e8, so an entity of that class nearer the
/// origin is a new failure, not this one.
const ROUNDING_BOUND_SCALE: f64 = 1.0e8;

/// Whether every one of `points` has a coordinate of at least [`ROUNDING_BOUND_SCALE`] in
/// magnitude.
fn all_far(points: &[Point3]) -> bool {
    !points.is_empty()
        && points
            .iter()
            .all(|p| p.x.abs().max(p.y.abs()).max(p.z.abs()) >= ROUNDING_BOUND_SCALE)
}

/// [`OPEN`] entries of `unit`: `(case, body, entity)`, the entity an edge's index or `v` and
/// a vertex's, to its class.
fn open_entries(unit: &str) -> std::collections::BTreeMap<(String, usize, String), String> {
    OPEN.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.first() == Some(&unit)).then(|| {
                (
                    (
                        f[1].to_string(),
                        f[2].parse().expect("body"),
                        f[3].to_string(),
                    ),
                    f[4].to_string(),
                )
            })
        })
        .collect()
}

/// The new tolerance an [`OPEN`] entry of `unit` records (its last column), for a class whose
/// entry pins it (`loose-raise`: the raise is open only at exactly that tolerance).
fn open_tolerance(unit: &str, key: &(String, usize, String)) -> Option<f64> {
    OPEN.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split('\t').collect::<Vec<&str>>())
        .find(|f| {
            f.first() == Some(&unit)
                && f.get(1) == Some(&key.0.as_str())
                && f.get(2).and_then(|b| b.parse::<usize>().ok()) == Some(key.1)
                && f.get(3) == Some(&key.2.as_str())
        })
        .and_then(|f| f.get(6).and_then(|t| t.parse().ok()))
}

/// The moved output values that are not a moved entity's tolerance, one per line: unit,
/// case, key, index, class, and the body ([`super::written`] order) the class is checked on.
/// Each class is a proof checked here:
/// - `rounding`: the value moved by at most two units in the last place: EQUAL (a
///   reordered sum's rounding, far below anything the case measures);
/// - `quadrature`: a mass property that moved by at most 1e-13 of itself: EQUAL, MEASURED
///   rather than proved (the integration over the faces sums its pieces in another order
///   once a trim is refit);
/// - `integration`: a mass property (`brep_mass_properties`' estimate at [`MASS_ACCURACY`])
///   of a case whose every other moved value is a moved entity's tolerance (the integrator
///   builds the UV trims it integrates over, each fit to its edge's tolerance, so a lower
///   tolerance moves its estimate): the new value is within [`MASS_ACCURACY`] of its scale of
///   every point of a certified enclosure of the exact property of the body
///   (`mass::enclosure::enclose_mass_properties`), so of the exact value, the accuracy the
///   runner asks ([`MassBands`]); EQUAL at the case's own accuracy, PROVEN;
/// - `padding`: a box coordinate padded by an edge's tolerance (`query.brep`'s tight box
///   expanded by its entities' tolerances): it moved inward by exactly a moved edge's
///   `o - n` (to four units in the last place), that edge certified as every other;
/// - `trim`: a value of a UV trim (its interval or a sample) in `topology.brep_mirror`'s
///   `record` (by the record's layout), or an end of an edge's interval in the graph's
///   `geometry` (by its layout, [`graph_edge_interval`]), refit while its edge's tolerance
///   was lower: that trim's `B` (in `geometry`, that of every trim of the edge) is within its
///   edge's tolerance (a proved cover, the trim is valid for the tolerance it was fit to); a
///   trim of an edge in [`OPEN`] is open with it (counted apart, not proved);
/// - `domain`: a value of a face's surface record (its natural domain, or a sample over it)
///   that moved with the face's refit trims (the surface's domain is bound to them): some
///   face's surface record holds it, and every UV trim on each face whose record holds it is
///   within its edge's tolerance (`fillet_group_torii_fillet_b_rb23`: the domain's `v` start moved 3.7e-7 and
///   its samples there now lie on the plane `z = 2` they are tangent to, 6.2e-6 off before);
/// - `vertex-box`: a corner coordinate of a result's vertex box (`offset.offset_executive_p2`'s
///   `result.geometry`: the body's tolerance, then the box of its vertex points), of a case
///   that writes no bodies, that moved by less than that tolerance, which did not move: EQUAL
///   within the body's own tolerance, MEASURED, not proved (the three shelled cylinders, whose
///   marched rim vertex moved about 5e-8 straight outward under a 1e-6 tolerance, its radial
///   error halved).
const OTHER: &str = include_str!("trim_tolerance_other.tsv");

/// The accuracy `blend.fillet_executive_p3`'s runner asks of the mass integrator
/// (`end_to_end.rs`, `brep_mass_properties`).
const MASS_ACCURACY: f64 = 1e-4;

/// Certified enclosures of a body's mass properties (density 1, about the origin), to check
/// an `integration` value ([`OTHER`]).
struct MassBands {
    /// The enclosures, asked a tenth of [`MASS_ACCURACY`] of their scale wide (one that spends
    /// its budget first is wider, still valid).
    e: solidcore::mass::enclosure::MassEnclosure,
    /// Half the diagonal of the body's box, `R`: the scale of a centroid coordinate (and,
    /// times the volume and `R`, of a moment).
    r: f64,
}

impl MassBands {
    /// The enclosures of `b`'s mass properties; panics where they cannot be made.
    fn of(k: &Kernel, b: &Brep, case: &str) -> Self {
        let mut b = b
            .clone_standing_for_self()
            .unwrap_or_else(|e| panic!("{case}: no copy: {e:?}"));
        let e = solidcore::mass::enclosure::enclose_mass_properties(k, &mut b, 0.1 * MASS_ACCURACY)
            .unwrap_or_else(|e| panic!("{case}: no mass enclosure: {e:?}"));
        let bbox = solidcore::query::brep::calculate_bounding_box(
            k,
            &mut b,
            solidcore::query::brep::BoundingBoxOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{case}: no box: {e:?}"));
        Self {
            e,
            r: 0.5 * bbox.size().length(),
        }
    }

    /// Checks value `index` of a `mass_properties` record (`area, volume, mass`, the centroid,
    /// the axial inertias, the products): `value` is within [`MASS_ACCURACY`] of its scale
    /// (the area's, the volume's, `R` for a centroid coordinate, `V R^2` for a moment, or the
    /// value's own where larger) of both ends of its enclosure, so of the exact value.
    fn check(&self, index: usize, value: f64) -> Result<(), String> {
        let e = &self.e;
        let vr2 = e.volume.hi * self.r * self.r;
        let (band, scale) = match index {
            0 => (e.area, e.area.hi),
            1 | 2 => (e.volume, e.volume.hi),
            3..=5 => (e.centroid[index - 3], self.r),
            6..=8 => (e.inertia[index - 6], vr2),
            9..=11 => (e.products[index - 9], vr2),
            _ => return Err(format!("no mass property {index}")),
        };
        let accuracy = MASS_ACCURACY * scale.max(value.abs());
        // (the farthest the exact value can be; written so a NaN fails)
        let far = (value - band.lo).max(band.hi - value);
        if far <= accuracy {
            Ok(())
        } else {
            Err(format!(
                "{value:e} is up to {far:e} off the certified [{:e}, {:e}] (accuracy {accuracy:e})",
                band.lo, band.hi
            ))
        }
    }
}

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
        .filter_map(|&eu| match use_bound(k, b, e, eu) {
            UseBound::NoTrim => None,
            UseBound::Open => Some(None),
            UseBound::Bound(x) => Some(Some(x)),
        })
        .collect()
}

/// `B` of the UV trim of one edge use ([`use_bound`]).
#[derive(Debug)]
enum UseBound {
    /// The use keeps no trim, or the edge no curve.
    NoTrim,
    /// The bound is not enclosed.
    Open,
    /// The enclosed bound.
    Bound(f64),
}

/// `B` of the UV trim of `e`'s use `eu` ([`trim_bounds`]).
fn use_bound(k: &Kernel, b: &Brep, e: EdgeId, eu: EdgeuseId) -> UseBound {
    use_bound_on(k, b, e, eu, false)
}

/// [`trim_bounds`] with a UV line enclosed on any surface with an exact net, not only a plane
/// ([`uv_line_slack_on_surface`]), each with its edge use: the WR-450 certificate of the trims
/// a copied analytic face keeps (`wr450_rows`).
pub(crate) fn trim_bounds_any_surface(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
) -> Vec<(EdgeuseId, Option<f64>)> {
    let rec = b.edge(e).expect("edge");
    rec.edgeuses
        .iter()
        .filter_map(|&eu| match use_bound_on(k, b, e, eu, true) {
            UseBound::NoTrim => None,
            UseBound::Open => Some((eu, None)),
            UseBound::Bound(x) => Some((eu, Some(x))),
        })
        .collect()
}

/// [`use_bound`]; with `any_surface`, a UV line on a surface other than a plane is enclosed
/// too ([`trim_bounds_any_surface`]).
fn use_bound_on(k: &Kernel, b: &Brep, e: EdgeId, eu: EdgeuseId, any_surface: bool) -> UseBound {
    let rec = b.edge(e).expect("edge");
    let Some(curve) = rec.curve.as_ref() else {
        return UseBound::NoTrim;
    };
    let Some(trim) = b.edgeuse(eu).expect("edge use").uv_trim_curve.as_ref() else {
        return UseBound::NoTrim;
    };
    let face = eu1::face(b, eu).expect("face").expect("a face");
    let surface = b
        .face(face)
        .expect("face")
        .surface
        .as_ref()
        .expect("surface");
    // A UV line on a plane is enclosed as its NURB (the segment between its two control
    // points), which `trim_image_gap_bound` encloses as it does any B-spline, plus how far the
    // line's analytic evaluation (`P + s V`, `s` rounded from the parameter, which
    // `trim_image_gap_bound` refuses) can be from that NURB at the same parameter in UV, `d`
    // ([`line_evaluation_slack`]: the rounding of the analytic point and the control points'
    // measured distance from it, each against the exact line), carried to the plane by
    // [`plane_uv_lipschitz`]. A line on another surface stays open.
    let line_nurb;
    let mut line_slack = 0.0;
    let trim = match trim.kind() {
        CurveKind::Line(l) => {
            let nurb = l.base_with_nurb().expect("a line's NURB").into_owned();
            let cpts = nurb.control_points().expect("a line's NURB").to_vec();
            let Some(d) = line_evaluation_slack(l, true) else {
                return UseBound::Open;
            };
            line_slack = if any_surface {
                match uv_line_slack_on_surface(k, surface, &cpts, d).expect("the slack") {
                    Some(x) => x,
                    None => return UseBound::Open,
                }
            } else {
                let Some(lip) = plane_uv_lipschitz(surface, &cpts, d) else {
                    return UseBound::Open;
                };
                lip * d * (1.0 + 4.0 * f64::EPSILON)
            };
            line_nurb = Curve::from(nurb);
            &line_nurb
        }
        _ => trim,
    };
    let mut surface = surface.clone_for_evaluation().expect("a copy");
    let _ = surface.set_owner(None);
    let ivl = rec.interval;
    let range = [ivl.min(), ivl.max()];
    let floor = FLOOR * rec.tolerance;
    // (with `any_surface`, an edge curve on a surface is enclosed through its own UV curve
    // and surface: [`crv_on_surf_trim_gap_bound`])
    let mut bound = if any_surface && matches!(curve.kind(), CurveKind::CrvOnSurf(_)) {
        crv_on_surf_trim_gap_bound(k, &surface, curve, trim, range, floor).expect("the bound")
    } else {
        trim_image_gap_bound(k, &surface, curve, trim, range, floor).expect("the bound")
    };
    // Past the tolerance (or not enclosed), the same pair is bounded again on an exactly
    // translated copy ([`exact_translated_bound`]): the same geometry, nearer the origin,
    // where the enclosures are not widened by the coordinates; the smaller bound is kept.
    if !bound.is_some_and(|x| x + line_slack <= rec.tolerance) {
        if let Some(x) = exact_translated_bound(k, &surface, curve, trim, range, floor) {
            bound = Some(bound.map_or(x, |b| b.min(x)));
        }
    }
    // A B-spline trim whose domain ends inside the edge's interval is evaluated clamped there:
    // bounded on its domain plus the curve's run over the slivers ([`clamped_trim_gap_bound`]).
    if any_surface && !bound.is_some_and(|x| x + line_slack <= rec.tolerance) {
        if let Some(x) =
            clamped_trim_gap_bound(k, &surface, curve, trim, range, floor).expect("the bound")
        {
            bound = Some(bound.map_or(x, |b| b.min(x)));
        }
    }
    match bound {
        Some(x) if line_slack == 0.0 => UseBound::Bound(x),
        Some(x) => UseBound::Bound((x + line_slack) * (1.0 + 4.0 * f64::EPSILON)),
        None => UseBound::Open,
    }
}

/// [`trim_image_gap_bound`] of `trim` against `curve` on `surface`, computed on a copy of the
/// curve and the surface moved by `-T`, where the move is exact: every distance of the copy
/// is the original's, so its bound is a bound of the original pair (#18166: a body built at
/// the origin, its tolerances proved there, then moved about 8e8 away, where the enclosures
/// widen with the coordinates: `primitive.api_primitives`' `gen1157`, whose moved edges the
/// body as built leaves uncertified are certified on this copy).
///
/// `T` is a sum of steps, each exact on its own, so the whole move is exact. A step is, per
/// axis, the power of two at most the middle of the moved curve's control points' coordinate
/// (`0` where that middle is under `1` in magnitude or not normal): it about halves the
/// middle, so the steps bring the middle of the copy's curve under `1` on each axis, within
/// [`TRANSLATION_STEPS`] steps, stopping early at the first step that would not be
/// exact (the copy then stays where the last exact step left it). The copy is built here,
/// not by a transform (which divides by the weight and multiplies back, rounding): each
/// stored homogeneous coordinate `X` of weight `w` (`1` for a non-rational point) becomes
/// `X - w t` for a step `t`, so the Euclidean point `X / w` moves by exactly `-t`. Exactness
/// is checked, never assumed: `w t` exact (a power of two times a weight, normal or zero),
/// `X - w t` with no rounding error (Knuth's two-sum error `0`), and the copy's stored
/// coordinates the last step's values, bit for bit, its weights unchanged. `None` where no
/// step is exact, for a curve or surface that is not a B-spline, or where the bound is not
/// enclosed.
fn exact_translated_bound(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    trim: &Curve,
    range: [f64; 2],
    floor: f64,
) -> Option<f64> {
    let CurveKind::BSpline(c) = curve.kind() else {
        return None;
    };
    let SurfaceKind::BSpline(sb) = surface.kind() else {
        return None;
    };
    let weight = |p: &CPoint| if p.w > 0.0 { p.w } else { 1.0 };
    let cp = c.control_points().ok()?;
    let (n, m, sp) = sb.control_points().ok()?;
    if m == 0 {
        return None;
    }
    // `p` moved by `-t`, or `None` where a coordinate's move is not exact
    let step = |p: &CPoint, t: [f64; 3]| -> Option<CPoint> {
        let mut out = [0.0; 3];
        for (o, (x, tc)) in out.iter_mut().zip([(p.x, t[0]), (p.y, t[1]), (p.z, t[2])]) {
            let s = weight(p) * tc;
            let d = x - s;
            // (Knuth's two-sum of `x` and `-s`: its error, exactly)
            let bb = d - x;
            let err = (x - (d - bb)) + (-s - bb);
            if !((s == 0.0 || s.is_normal()) && d.is_finite() && err == 0.0) {
                return None;
            }
            *o = d;
        }
        Some(CPoint {
            x: out[0],
            y: out[1],
            z: out[2],
            w: p.w,
        })
    };
    let (mut cur_c, mut cur_s) = (cp.to_vec(), sp.clone());
    let mut moved_any = false;
    for _ in 0..TRANSLATION_STEPS {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in &cur_c {
            for (i, x) in [p.x, p.y, p.z].into_iter().enumerate() {
                let e = x / weight(p);
                lo[i] = lo[i].min(e);
                hi[i] = hi[i].max(e);
            }
        }
        let t: [f64; 3] = std::array::from_fn(|i| {
            let mid = 0.5 * lo[i] + 0.5 * hi[i];
            if mid.is_normal() && mid.abs() >= 1.0 {
                // (the exponent bits alone: the largest power of two at most |mid|)
                f64::from_bits(mid.abs().to_bits() & 0x7ff0_0000_0000_0000).copysign(mid)
            } else {
                0.0
            }
        });
        if t == [0.0; 3] {
            break;
        }
        let next_c: Option<Vec<CPoint>> = cur_c.iter().map(|p| step(p, t)).collect();
        let next_s: Option<Vec<CPoint>> = cur_s.iter().map(|p| step(p, t)).collect();
        let (Some(next_c), Some(next_s)) = (next_c, next_s) else {
            break;
        };
        (cur_c, cur_s, moved_any) = (next_c, next_s, true);
    }
    if !moved_any {
        return None;
    }
    let same = |p: &CPoint, q: &CPoint| {
        [(p.x, q.x), (p.y, q.y), (p.z, q.z), (p.w, q.w)]
            .into_iter()
            .all(|(a, b)| a.to_bits() == b.to_bits())
    };
    let mut mc = c.clone();
    for (i, p) in cur_c.iter().enumerate() {
        mc.set_control_point(
            ControlPointForm::HomogeneousRational,
            i,
            Point3::new(p.x, p.y, p.z),
            p.w,
        )
        .ok()?;
    }
    let mcp = mc.control_points().ok()?;
    if mcp.len() != cp.len()
        || !cp
            .iter()
            .zip(&cur_c)
            .all(|(p, q)| p.w.to_bits() == q.w.to_bits())
        || !cur_c.iter().zip(mcp).all(|(p, q)| same(p, q))
    {
        return None;
    }
    let mut ms = sb.clone();
    // (`n` rows of `m` points: point `[i][j]` at `i m + j`)
    for (i, row) in cur_s.chunks(m).enumerate() {
        for (j, p) in row.iter().enumerate() {
            ms.set_control_point(
                ControlPointForm::HomogeneousRational,
                i,
                j,
                Point3::new(p.x, p.y, p.z),
                p.w,
            )
            .ok()?;
        }
    }
    let (mn, mm, mp) = ms.control_points().ok()?;
    if (n, m) != (mn, mm)
        || sp.len() != mp.len()
        || !sp
            .iter()
            .zip(&cur_s)
            .all(|(p, q)| p.w.to_bits() == q.w.to_bits())
        || !cur_s.iter().zip(&mp).all(|(p, q)| same(p, q))
    {
        return None;
    }
    let moved_curve = Curve::from(mc);
    let moved_surface = Surface::from(ms);
    trim_image_gap_bound(k, &moved_surface, &moved_curve, trim, range, floor).ok()?
}

/// The most steps of [`exact_translated_bound`]'s move (a cap): each about halves a middle
/// of at least `1`, so 1100 steps cover any finite middle (under `2^1024`).
const TRANSLATION_STEPS: usize = 1100;

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
    let curve = |c: Option<&Curve>| match c {
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
    let surface = |s: Option<&Surface>| match s {
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

/// The edge (its number in the graph's walk, [`Canon::reachable`]) and the end (`0` the
/// start, `1` the end) of its interval that index `i` of a `graph` output's `geometry` holds,
/// or `None` for any other value. The layout is `units::boolean_merge_p1::graph`'s: each
/// vertex use's UV (2 values), each vertex's point and tolerance (4), each face's UV domain
/// and tolerance (5), each edge's interval and tolerance (3); the counts of faces, edges,
/// vertex uses and vertices are `graph`'s ints 3, 7, 8 and 9.
///
/// [`Canon::reachable`]: crate::units::boolean_merge_p1::Canon::reachable
fn graph_edge_interval(graph: &[i64], i: usize) -> Option<(usize, usize)> {
    let count = |j: usize| graph.get(j).and_then(|&n| usize::try_from(n).ok());
    let (nf, ne, nvu, nv) = (count(3)?, count(7)?, count(8)?, count(9)?);
    let r = i.checked_sub(2 * nvu + 4 * nv + 5 * nf)?;
    (r < 3 * ne && r % 3 < 2).then_some((r / 3, r % 3))
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

/// The values a `record` holds for curve `c` (as `brep_mirror`'s `curve_samples`): its
/// natural interval's ends and the coordinates of its five samples.
fn curve_record_values(k: &Kernel, c: &Curve) -> Vec<f64> {
    let d = c.natural_interval();
    let mut vals = vec![d.min(), d.max()];
    for i in 0..5 {
        if let Ok(p) = c.evaluate_point(k, d.evaluate(f64::from(i) / 4.0)) {
            vals.extend([p.x, p.y, p.z]);
        }
    }
    vals
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
                UseBound::NoTrim => {}
                UseBound::Bound(x) if x <= rec.tolerance => {
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

/// A proved bound on the distance from `e`'s curve over its interval to the surface of its
/// face `f` ([`curve_face_gap_bound_within`], floor a thousandth of `n`, cut until shown within
/// `n` where it is), plus the rounding of the
/// curve's evaluation against its NURBS data (none for a B-spline or an ellipse, evaluated on
/// it; for a line, evaluated analytically, [`line_evaluation_slack`]); `None` where it is not
/// enclosed or the curve is of another kind.
fn face_bound(k: &Kernel, b: &Brep, e: EdgeId, f: FaceId, n: f64) -> Option<f64> {
    let rec = b.edge(e).expect("edge");
    let curve = rec.curve.as_ref()?;
    let line;
    let (bs, slack) = match curve.kind() {
        CurveKind::BSpline(bs) => (bs, 0.0),
        CurveKind::Ellipse(el) => (el.base(), 0.0),
        CurveKind::Line(l) => {
            line = l.base_with_nurb().expect("a line's NURB");
            (&*line, line_evaluation_slack(l, false)?)
        }
        _ => return None,
    };
    let ivl = rec.interval;
    // (pieces whose bound shows the gap within `n` close at once; a gap just within it is cut
    // until shown so)
    let enough = n / (1.0 + 4.0 * f64::EPSILON) - slack;
    curve_face_gap_bound_within(
        k,
        b,
        f,
        bs,
        [ivl.min(), ivl.max()],
        FLOOR * n,
        [enough, f64::INFINITY],
    )
    .expect("the bound")
    .map(|g| (g + slack) * (1.0 + 4.0 * f64::EPSILON))
}

/// The bounds that certify `e`'s tolerance `n`, one per checked gap, for every face of the
/// edge: the `B` of each UV trim the edge's uses on that face hold ([`use_bound`]), or, on a
/// face where none holds one, its curve's distance to the face's surface ([`face_bound`]);
/// `None` where a gap is not enclosed, and a single `None` for an edge on no face. With the
/// number of faces bounded without a trim.
fn edge_bounds(k: &Kernel, b: &Brep, e: EdgeId, n: f64) -> (Vec<Option<f64>>, usize) {
    let rec = b.edge(e).expect("edge");
    let faces = edge::faces(b, e, None).expect("faces");
    if faces.is_empty() {
        return (vec![None], 0);
    }
    let mut out = Vec::new();
    let mut trimless = 0;
    for f in faces {
        let mut on_face = Vec::new();
        for &eu in &rec.edgeuses {
            if eu1::face(b, eu).expect("face") != Some(f) {
                continue;
            }
            match use_bound(k, b, e, eu) {
                UseBound::NoTrim => {}
                UseBound::Open => on_face.push(None),
                UseBound::Bound(x) => on_face.push(Some(x)),
            }
        }
        if on_face.is_empty() {
            on_face.push(face_bound(k, b, e, f, n));
            trimless += 1;
        }
        out.extend(on_face);
    }
    (out, trimless)
}

/// Certifies a [`DROP_MARGIN`] raise of edge `e` of `b` (body `body` of `case`) from `old` to
/// `new`: the edge's one classified use that holds a trim (the made trim, `eu`); `case` run
/// again with main's read put back (`tolsub::state::strip_classified_trims`: the classified
/// trims off, the raises at the zone tolerance), where the edge holds `old` and `eu` no trim;
/// there `get_or_create_uv_trim_curve` on `eu` (the drop the classification makes) reports the
/// drop distance `d` and leaves the edge at `new`, bit for bit, with `old < new`; and with the
/// made trim's `B` on `b` ([`use_bound`]), `new` is one of `trim_raise_distance`'s raises,
/// doubled: `2 d` with `B <= new` (upstream's, where `B` is from `d` to `2 d`, or where `2 B`
/// would not cover another trim of the edge; then `B < new / 2`), or `2 B` with `B < d` (the
/// proved distance, below upstream's). Either way `B <= new` and `new <= 2 d`. Returns
/// `(d, B)`.
fn drop_margin_raise(
    unit: &str,
    case: &str,
    body: usize,
    b: &Brep,
    e: EdgeId,
    old: f64,
    new: f64,
) -> Result<(f64, f64), String> {
    use crate::tolsub::state::{strip_classified_trims, CLASSIFIED_READ_TRIMS};
    let made: Vec<EdgeuseId> = b
        .edge(e)
        .expect("edge")
        .edgeuses
        .iter()
        .copied()
        .filter(|&eu| {
            CLASSIFIED_READ_TRIMS.contains(&eu.index())
                && b.edgeuse(eu).expect("edge use").uv_trim_curve.is_some()
        })
        .collect();
    let [eu] = made[..] else {
        return Err(format!("made trims {made:?}"));
    };
    let ((), reads) = strip_classified_trims(|| {
        super::watch_written();
        let _ = replay_case(unit, case);
    });
    let mut bodies = super::take_written();
    if reads == 0 {
        return Err("no classified read put back".to_string());
    }
    let m = bodies.get_mut(body).ok_or("no body on main's read")?;
    let was = m.edge(e).map_err(|x| format!("{x:?}"))?.tolerance;
    if was.to_bits() != old.to_bits() {
        return Err(format!("main's read holds {was:e}, not {old:e}"));
    }
    if m.edgeuse(eu)
        .map_err(|x| format!("{x:?}"))?
        .uv_trim_curve
        .is_some()
    {
        return Err("main's read holds the trim".to_string());
    }
    let k = crate::runner_kernel();
    let fetch =
        solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve(&k, m, eu, true, false, true)
            .map_err(|x| format!("{x:?}"))?;
    fetch.status.map_err(|x| format!("the drop: {x:?}"))?;
    let d = fetch.dist_to_surf;
    let replayed = m.edge(e).map_err(|x| format!("{x:?}"))?.tolerance;
    if replayed.to_bits() != new.to_bits() {
        return Err(format!(
            "the drop leaves {replayed:e}, not {new:e} (d {d:e})"
        ));
    }
    let bm = match use_bound(&Kernel::default(), b, e, eu) {
        UseBound::Bound(x) => x,
        x => return Err(format!("the made trim: {x:?}")),
    };
    // (written so a NaN fails; doubling is exact)
    let upstream = new.to_bits() == (2.0 * d).to_bits() && bm <= new;
    let proved = new.to_bits() == (2.0 * bm).to_bits() && bm < d;
    if !(d > 0.0 && new > old && (upstream || proved)) {
        return Err(format!(
            "{old:e} -> {new:e} is neither 2 d (d {d:e}) covering B {bm:e} nor 2 B below 2 d"
        ));
    }
    Ok((d, bm))
}

/// Whether the point of `v` is within `n` of the end of every edge curve of `v` that ends
/// there, the evaluation's error bound and the distance's rounding counted against it.
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
            // The evaluation's error per coordinate, bounded from the curve's data (its
            // control points), not from the size of the point it returns; no bound, not
            // covered.
            let p = curve.evaluate_point(k, t).expect("end point");
            let Some(error) = curve.point_error_bound(t) else {
                return false;
            };
            let up = |x: f64| x * (1.0 + 4.0 * f64::EPSILON);
            if up(up(at.distance(p)) + up(error * up(3.0_f64.sqrt()))) > n {
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
    /// Faces of moved edges on which the edge holds no UV trim, bounded against the face's
    /// surface instead.
    trimless: usize,
    /// Moved edges listed in [`OPEN`].
    open: usize,
    /// `trim` values on a trim of an edge listed in [`OPEN`] (open with it).
    open_values: usize,
    /// Raised edges: every trim's `B` within the new tolerance, one above the old.
    raised: usize,
    /// [`DROP_MARGIN`] raises certified by [`drop_margin_raise`], every trim within the new
    /// tolerance.
    drop_margin: usize,
    /// [`OTHER`] values: `rounding`, `quadrature`, `integration`, `padding`, `trim`,
    /// `domain` and `vertex-box`.
    rounding: usize,
    quadrature: usize,
    integration: usize,
    padding: usize,
    trim_values: usize,
    domain: usize,
    vertex_box: usize,
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
        // (a row the WR-450 retirement moves too: main's run had main's choices at its seams)
        let out =
            crate::wr450_rows::with_mains_choices_for(unit, &row.case, || runner.run_rust(case));
        let bodies = super::take_written();
        let mut mass_bands = std::collections::BTreeMap::new();
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
            // (a value is a moved entity's tolerance by its old and new bits, not by the
            // output's layout, which 21 units lay out 21 ways: a coordinate that moved by
            // exactly a moved tolerance's pair, both bit for bit, would pass as one. Every
            // entity is certified below whichever values match it)
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
                "quadrature" if (new - old).abs() <= 1e-13 * old.abs().max(new.abs()) => {
                    tally.quadrature += 1;
                }
                "integration"
                    if key.ends_with("mass_properties")
                        && row.moved.iter().all(|(key, _, old, new)| {
                            key.ends_with("mass_properties")
                                || row.entities.iter().any(|e| {
                                    e.3.to_bits() == old.to_bits() && e.4.to_bits() == new.to_bits()
                                })
                        })
                        && bodies.get(*body).is_some() =>
                {
                    let b = bodies.get(*body).expect("a body");
                    let bands = mass_bands
                        .entry(*body)
                        .or_insert_with(|| MassBands::of(&k, b, &row.case));
                    match bands.check(*index, *new) {
                        Ok(()) => tally.integration += 1,
                        Err(why) => failures.push(format!("{at}: {old:e} -> {new:e}: {why}")),
                    }
                }
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
                    // a `record` by its layout, the graph's `geometry` by the value (an end of
                    // the trims' natural intervals, every trim holding it checked)
                    let uses = if key.ends_with("record") {
                        let owner = record_owners(&k, b, true, true).get(*index).copied();
                        let Some(Owner::Trim(e, eu)) = owner else {
                            panic!("{at}: not a UV trim's value");
                        };
                        // (and the trim's record holds the value: the layout is right)
                        let trim = b.edgeuse(eu).expect("edge use").uv_trim_curve.as_ref();
                        assert!(
                            trim.is_some_and(|c| curve_record_values(&k, c)
                                .iter()
                                .any(|v| v.to_bits() == new.to_bits())),
                            "{at}: not in its trim's record"
                        );
                        vec![(e, eu)]
                    } else {
                        // the graph's `geometry` by its layout: an end of an edge's interval,
                        // that edge's every trim checked
                        let graph = key
                            .strip_suffix("geometry")
                            .and_then(|p| out.ints.get(&format!("{p}graph")))
                            .unwrap_or_else(|| panic!("{at}: no graph"));
                        let Some((e, end)) = graph_edge_interval(graph, *index) else {
                            panic!("{at}: not an end of an edge's interval");
                        };
                        let walked = crate::units::boolean_merge_p1::Canon::reachable(b);
                        let e = *walked
                            .edges
                            .items
                            .get(e)
                            .unwrap_or_else(|| panic!("{at}: no edge {e}"));
                        let ivl = b.edge(e).expect("edge").interval;
                        let now = if end == 0 { ivl.min() } else { ivl.max() };
                        assert_eq!(now.to_bits(), new.to_bits(), "{at}: the edge's interval");
                        b.edge(e)
                            .expect("edge")
                            .edgeuses
                            .iter()
                            .map(|&eu| (e, eu))
                            .filter(|&(_, eu)| {
                                b.edgeuse(eu).expect("edge use").uv_trim_curve.is_some()
                            })
                            .collect()
                    };
                    if uses.is_empty() {
                        failures.push(format!("{at}: no UV trim's interval ends at {new:e}"));
                    }
                    let mut covered = true;
                    for (e, eu) in uses {
                        let tol = b.edge(e).expect("edge").tolerance;
                        let key = (row.case.clone(), *body, e.index().to_string());
                        match use_bound(&k, b, e, eu) {
                            UseBound::Bound(x) if x <= tol => {
                                tally.worst = tally.worst.max(x / tol);
                            }
                            // the trim of an edge in OPEN: open with it (the edge's own
                            // entry is checked below)
                            _ if open.contains_key(&key) => {
                                covered = false;
                                tally.open_values += 1;
                            }
                            x => {
                                covered = false;
                                failures.push(format!(
                                    "{at}: the trim of edge {} use {}: B {x:?} over {tol:e}",
                                    e.index(),
                                    eu.index()
                                ));
                            }
                        }
                    }
                    if covered {
                        tally.trim_values += 1;
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
                "vertex-box"
                    if key.ends_with("result.geometry")
                        && *index > 0
                        && bodies.is_empty()
                        && !row.moved.iter().any(|m| m.0 == *key && m.1 == 0) =>
                {
                    let tol = out
                        .values
                        .get(key)
                        .and_then(|v| v.first())
                        .copied()
                        .unwrap_or(f64::NAN);
                    // (written so a NaN fails)
                    if tol.is_finite() && tol > 0.0 && (new - old).abs() < tol {
                        tally.vertex_box += 1;
                    } else {
                        failures.push(format!(
                            "{at}: {old:e} -> {new:e} moved past the body's tolerance {tol:e}"
                        ));
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
                // (each face of the edge: its trims' bounds, or the curve's distance to its
                // surface where the edge holds no trim on it)
                let (bounds, trimless) = edge_bounds(&k, b, e, new);
                tally.trimless += trimless;
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
                    // above the old tolerance (in OPEN only as a `loose-raise`: at least one
                    // trim enclosed, none past the old tolerance, every one within the new,
                    // and the new tolerance exactly the entry's, so a raise that moves is
                    // seen; counted open, never proved)
                    let past = bounds.iter().flatten().any(|x| *x > old);
                    if DROP_MARGIN.contains(&(unit, row.case.as_str(), body, index)) {
                        // a raise by `SetUVTrimCurve`'s margin: derived and certified by
                        // [`drop_margin_raise`]; every trim within the new tolerance, or the
                        // edge in OPEN as `UNCOVERED`, every bound past the new tolerance past
                        // the old as well (the old tolerance did not cover it either: #20906)
                        match drop_margin_raise(unit, &row.case, body, b, e, old, new) {
                            Err(why) => failures.push(format!("{at}: drop margin: {why}")),
                            Ok((d, bm)) if bad.is_empty() => {
                                println!("{at}: drop margin d {d:e} B {bm:e} n {new:e}");
                                assert!(
                                    !open.contains_key(&key),
                                    "{at}: certified, remove it from OPEN"
                                );
                                tally.drop_margin += 1;
                            }
                            Ok((d, bm))
                                if open.get(&key).is_some_and(|c| c == "UNCOVERED")
                                    && bounds
                                        .iter()
                                        .all(|x| x.is_some_and(|x| x <= new || x > old)) =>
                            {
                                println!("{at}: drop margin d {d:e} B {bm:e} n {new:e}, open");
                                open_seen.insert(key);
                                tally.open += 1;
                            }
                            Ok(_) => failures.push(format!("{at}: drop margin: {bad:?}")),
                        }
                    } else if bad.is_empty() && past {
                        tally.raised += 1;
                    } else if open.get(&key).is_some_and(|c| c == "loose-raise")
                        && !past
                        && bounds.iter().any(Option::is_some)
                        && bounds.iter().flatten().all(|x| *x <= new)
                        && open_tolerance(unit, &key).map(f64::to_bits) == Some(new.to_bits())
                    {
                        open_seen.insert(key);
                        tally.open += 1;
                    } else {
                        failures.push(format!(
                            "{at}: raised {old:e} -> {new:e}: {bad:?}, a bound past old: {past}"
                        ));
                    }
                } else if bad.is_empty() {
                    tally.edges += 1;
                    assert!(
                        !open.contains_key(&key),
                        "{at}: certified, remove it from OPEN"
                    );
                } else if let Some(class) = open.get(&key) {
                    // open: the old tolerance does not certify the new geometry either (some
                    // trim's `B` past it, or not enclosed). Not a claim about the old geometry,
                    // which this test does not build: the entry is counted open, never proved.
                    assert!(
                        class != "rounding-bound",
                        "{at}: `rounding-bound` is a vertex class"
                    );
                    if bounds.iter().all(|x| x.is_some_and(|x| x <= old)) {
                        failures.push(format!(
                            "{at}: {old:e} -> {new:e}: certified at the old tolerance, not \
                             the new: {bad:?}"
                        ));
                    }
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
                // (a vertex raised to exactly the new tolerance of an edge of its that this row
                // raises: `edge::set_tolerance` keeps a vertex's tolerance at least its edges',
                // so it follows that edge, whose own raise is certified above or fails; the
                // raise is then the edge's, not the vertex's, and the new tolerance must still
                // cover the ends of every edge of the vertex)
                let follows_a_raised_edge = || {
                    solidcore::topology::vertex::edges(b, v, None)
                        .expect("edges")
                        .into_iter()
                        .any(|e| {
                            let tol = b.edge(e).expect("edge").tolerance;
                            tol.to_bits() == new.to_bits()
                                && row.entities.iter().any(|&(eb, ie, ei, eo, en)| {
                                    eb == body
                                        && ie
                                        && usize::try_from(e.index()) == Ok(ei)
                                        && en > eo
                                })
                        })
                };
                let follows = raised && follows_a_raised_edge();
                if follows && vertex_covers_its_ends(&k, b, v, new) {
                    tally.vertices += 1;
                    assert!(
                        !open.contains_key(&key),
                        "{at}: certified, remove it from OPEN"
                    );
                } else if raised && !follows && vertex_covers_its_ends(&k, b, v, old) {
                    failures.push(format!("{at}: raised {old:e} -> {new:e}: old covered"));
                } else if vertex_covers_its_ends(&k, b, v, new) {
                    tally.vertices += 1;
                    assert!(
                        !open.contains_key(&key),
                        "{at}: certified, remove it from OPEN"
                    );
                } else if open.contains_key(&key) && !vertex_covers_its_ends(&k, b, v, old) {
                    // open: neither tolerance covers the new edge ends (the old geometry is
                    // not built here: counted open, never proved)
                    if open.get(&key).is_some_and(|c| c == "rounding-bound") {
                        assert!(
                            all_far(&[rec.point]),
                            "{at}: `rounding-bound` near the origin"
                        );
                    }
                    open_seen.insert(key);
                    tally.open += 1;
                } else {
                    failures.push(format!("{at}: {old:e} -> {new:e}: an edge end is farther"));
                }
            }
        }
    }
    println!("{unit}: {tally:?}");
    assert_eq!(
        open_seen,
        open.keys().cloned().collect(),
        "{unit}: OPEN entries not reached"
    );
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
                assert!(t.values + t.rounding + t.quadrature + t.integration + t.padding + t.trim_values + t.domain + t.vertex_box > 0, "{t:?}");
            }
        )*
    };
}

certified_units! {
    api_primitives: "primitive.api_primitives",
    primitive_creation_p1: "primitive.primitive_creation_p1",
    fillet_executive_p3: "blend.fillet_executive_p3",
    feature_executive: "blend.feature_executive",
    offset_geometry_creation: "offset.offset_geometry_creation",
    offset_executive_p2: "offset.offset_executive_p2",
    stitch_p1: "heal.stitch_p1",
    stitch_p2: "heal.stitch_p2",
    brep: "query.brep",
    face_p2: "query.face_p2",
    api_files: "topology.api_files",
    brep_cutting: "topology.brep_cutting",
    brep_local_operation: "topology.brep_local_operation",
    brep_mirror: "topology.brep_mirror",
    brep_p10: "topology.brep_p10",
    brep_p2: "topology.brep_p2",
    brep_p4: "topology.brep_p4",
    edgeuse_p3: "topology.edgeuse_p3",
    face_p3: "topology.face_p3",
}

/// The digest of each [`ROWS`] unit's expected file on main, one `unit<TAB>digest` (hex) per
/// line after the `#` lines that name main's commit: [`expected_digest`] of the file with
/// nothing put back, written by [`main_expected_digests`] from main's files, in which the line
/// of each [`ROWS`] case is the one main's kernel writes for it (the line main's file keeps can
/// lag main's own run within the golden compare's tolerance, which the compare accepts, and a
/// moved line is written whole from the run; the regen blesses those cases again on a tree whose
/// kernel is main's).
const MAIN: &str = include_str!("trim_tolerance_main.tsv");

/// A later golden change independent of the tolerance changes in [`ROWS`]. Both hashes
/// are FNV-1a of the whole canonical row, including its newline, as in [`expected_digest`].
struct GoldenChange {
    case: &'static str,
    old_hash: u64,
    new_hash: u64,
    reason: &'static str,
}

/// The complete diff of `blend.fillet_executive_p3` from main at this PR's merge.
/// These rows already moved in b318df14c; the merged tolerance change moved none of them,
/// and none belongs to [`ROWS`]. The vertex crossing correction itself moves no golden.
/// Derivation: `plan/findings/blend.fillet_solver_p5--issue-17016-non-tangent-rollover.md`,
/// "Three goldens rows that move, and why". No topology or status changes in these rows.
/// #18294 moves the two setbacks rows again (each reason names the change and the row before
/// it); its other seven `issue-880-*` rows are in [`MAIN`]'s digest.
const FILLET_UV_CHANGES: &[GoldenChange] = &[
    GoldenChange {
        case: "issue-880-half-arc-rim-0-setbacks-1-1",
        old_hash: 0x0c84b1bb5aabdeb6,
        new_hash: 0xff20292224de653a,
        reason: "Keep the domain-proved oblique UV jets on the procedural surface: the old \
                 boundary fix-up flattened slopes of 5.3e-5 to 8.5e-4 and moved control \
                 polygons by up to 2.2e-5 in UV without a bound on the procedural surface. \
                 Then #18294: the glue keeps a trim curve only when it spans the surviving \
                 edge, so two vertex and one edge tolerance fall from 6.82e-5 to 4e-5 and \
                 three of the ten vertices are listed in another order (row was 064c8b893b765eaf).",
    },
    GoldenChange {
        case: "issue-880-half-arc-rim-0-setbacks-1-2",
        old_hash: 0x0c29e482175fe1e2,
        new_hash: 0x338bc7a48039bef7,
        reason: "Keep the domain-proved oblique UV jets on the procedural surface for the \
                 unequal setbacks too; the old boundary flattening and polygon moves have \
                 no bound on that surface, and preserving the jets moves the mass integral. \
                 Then #18294: the glue keeps a trim curve only when it spans the surviving \
                 edge, so two vertex and one edge tolerance fall from 7.47e-5 to 4.15e-5 \
                 (row was 94a9c0596e8f0db7).",
    },
    GoldenChange {
        case: "source-file-nurb_box-edge0",
        old_hash: 0xfa66dbe7062ca1e2,
        new_hash: 0xdcf4807bfc81eec3,
        reason: "Preserve the procedural drop's transverse UV tangent (at most 7e-13) \
                 instead of zeroing it in four spans. The polygon stays unchanged; the \
                 resulting mass estimate moves far below its requested 1e-4 accuracy.",
    },
];

/// The three original whole rows from origin/main (2f632eafb), also unchanged by #18166.
/// Putting these back keeps [`MAIN`]'s original digest and every tolerance proof intact.
const FILLET_UV_MAIN: &str = include_str!("trim_tolerance_fillet_uv_main.jsonl");

/// Reverse the listed changes, rejecting a missing, duplicate, overlapping or
/// differently changed row. Every other line is retained for the whole-file digest.
fn put_back_independent_changes(
    text: &str,
    rows: &[Row],
    changes: &[GoldenChange],
    originals: &str,
) -> Result<String, String> {
    use crate::json::to_canonical_string;
    let mut original = std::collections::BTreeMap::new();
    for line in originals.lines() {
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let case = v["case"].as_str().ok_or("an original row has no case")?;
        let change = changes
            .iter()
            .find(|r| r.case == case)
            .ok_or_else(|| format!("{case}: unlisted original row"))?;
        if change.reason.trim().is_empty() || rows.iter().any(|r| r.case == case) {
            return Err(format!(
                "{case}: missing reason or overlap with tolerance rows"
            ));
        }
        let got = crate::rng::fnv1a64(&to_canonical_string(&v));
        if got != change.old_hash {
            return Err(format!(
                "{case}: original {got:016x}, row {:016x}",
                change.old_hash
            ));
        }
        if original.insert(case.to_string(), line).is_some() {
            return Err(format!("{case}: duplicate original row"));
        }
    }
    if original.len() != changes.len() {
        return Err("the independent changes and their original rows differ".into());
    }
    let mut out = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let change = v["case"]
            .as_str()
            .and_then(|case| changes.iter().find(|r| r.case == case));
        let line = if let Some(change) = change {
            if !seen.insert(change.case) {
                return Err(format!("{}: duplicate changed row", change.case));
            }
            let got = crate::rng::fnv1a64(&to_canonical_string(&v));
            if got != change.new_hash {
                return Err(format!(
                    "{}: changed {got:016x}, row {:016x}",
                    change.case, change.new_hash
                ));
            }
            original
                .remove(change.case)
                .ok_or_else(|| format!("{}: no original row", change.case))?
        } else {
            line
        };
        out.push_str(line);
        out.push('\n');
    }
    if let Some(change) = changes.iter().find(|r| !seen.contains(r.case)) {
        return Err(format!("{}: missing changed row", change.case));
    }
    Ok(out)
}

/// Whole-row pins reject an unrecorded field change, a missing or duplicate case, and a
/// later change that would bypass one of the tolerance proofs.
#[test]
fn independent_golden_changes_cannot_hide_other_changes_or_tolerance_rows() {
    for (text, changes, originals) in [
        (
            include_str!("../../expected/blend.fillet_executive_p3.jsonl"),
            FILLET_UV_CHANGES,
            FILLET_UV_MAIN,
        ),
        (
            include_str!("../../expected/heal.gap.jsonl"),
            GAP_CHANGES,
            GAP_MAIN,
        ),
        (
            include_str!("../../expected/heal.face_props.jsonl"),
            P6_HEAL_CHANGES,
            P6_HEAL_MAIN,
        ),
        (
            include_str!("../../expected/intersect.curve_class_p5.jsonl"),
            P6_CURVE_CLASS_CHANGES,
            P6_CURVE_CLASS_MAIN,
        ),
    ] {
        let put_back =
            |text: &str, rows: &[Row]| put_back_independent_changes(text, rows, changes, originals);
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| {
                let v: serde_json::Value = serde_json::from_str(line).expect("a golden row");
                changes.iter().any(|r| v["case"] == r.case)
            })
            .collect();
        let changed = lines.join("\n") + "\n";
        assert_eq!(
            put_back(&changed, &[]).expect("the documented changes"),
            originals
        );
        let first = lines[0];
        let missing = lines[1..].join("\n");
        assert!(put_back(&missing, &[]).is_err());
        let duplicate = format!("{changed}{first}\n");
        assert!(put_back(&duplicate, &[]).is_err());
        let mut v: serde_json::Value = serde_json::from_str(first).expect("a golden row");
        v["error"] = serde_json::json!("an unrecorded change");
        let corrupted = changed.replacen(first, &serde_json::to_string(&v).expect("a row"), 1);
        assert!(put_back(&corrupted, &[]).is_err());
        let overlap = Row {
            case: changes[0].case.to_string(),
            moved: Vec::new(),
            entities: Vec::new(),
        };
        assert!(put_back(&changed, &[overlap]).is_err());
    }
}

/// FNV-1a 64 of the canonical lines of the expected file `text` ([`crate::json`]: each line
/// parsed and written again, so the digest is of the values, not of their spelling), with
/// each moved value of `rows` (`key[index]` of its case, bit for bit its `n`) put back to its
/// `o`. An `Err` names a moved value the file does not hold as the row has it, or a row the
/// file does not have.
///
/// A key the file stores in blocks (`{"n", "blocks"}`, [`crate::golden::encode`]) holds no
/// single value to put back: its case is run again (`replay`), its output must encode to the
/// file's line bit for bit, and the line is that output encoded with the moved values put
/// back.
fn expected_digest(
    text: &str,
    rows: &[Row],
    replay: &mut dyn FnMut(&str) -> Result<crate::Output, String>,
) -> Result<u64, String> {
    use crate::json::{num, num_of, to_canonical_string};
    let mut canonical = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let mut v: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("a line: {e}"))?;
        let case = v["case"].as_str().map(str::to_string);
        if let Some(row) = case.and_then(|c| rows.iter().find(|r| r.case == c)) {
            seen.insert(row.case.clone());
            let blocked = row.moved.iter().any(|(key, ..)| {
                v["values"]
                    .get(key)
                    .is_some_and(|a| a.get("blocks").is_some())
            });
            if blocked {
                let mut out = replay(&row.case)?;
                let run = to_canonical_string(&crate::golden::encode(&row.case, &out));
                if run != to_canonical_string(&v) {
                    return Err(format!("{}: the run is not the file's line", row.case));
                }
                for (key, index, old, new) in &row.moved {
                    let at = format!("{} {key}[{index}]", row.case);
                    let slot = out
                        .values
                        .get_mut(key)
                        .and_then(|a| a.get_mut(*index))
                        .ok_or_else(|| format!("{at}: not in the run"))?;
                    if slot.to_bits() != new.to_bits() {
                        return Err(format!("{at}: run {slot:e}, row {new:e}"));
                    }
                    *slot = *old;
                }
                v = crate::golden::encode(&row.case, &out);
            } else {
                for (key, index, old, new) in &row.moved {
                    let at = format!("{} {key}[{index}]", row.case);
                    let slot = v["values"]
                        .get_mut(key)
                        .and_then(|a| a.get_mut(*index))
                        .ok_or_else(|| format!("{at}: not in the expected file"))?;
                    match num_of(slot) {
                        Some(x) if x.to_bits() == new.to_bits() => *slot = num(*old),
                        x => return Err(format!("{at}: expected {x:?}, row {new:e}")),
                    }
                }
            }
        }
        canonical.push_str(&to_canonical_string(&v));
    }
    if let Some(row) = rows.iter().find(|r| !seen.contains(&r.case)) {
        return Err(format!("{}: not in the expected file", row.case));
    }
    Ok(crate::rng::fnv1a64(&canonical))
}

/// The units of [`ROWS`].
fn row_units() -> std::collections::BTreeSet<String> {
    ROWS.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).expect("a row");
            v[0].as_str().expect("unit").to_string()
        })
        .collect()
}

/// The ledger is the whole golden diff from main: every expected file, with each moved value
/// of [`ROWS`] put back to its old value, is main's file ([`MAIN`] holds the digest of every
/// unit's expected file on main, each row case's line as main's kernel writes it). So no unit,
/// case, moved value or other change is left out of the rows, and each row's new value is the
/// expected one. The ledger restores the original lines of [`FILLET_UV_CHANGES`] and
/// [`GAP_CHANGES`], [`P6_HEAL_CHANGES`] and [`P6_CURVE_CLASS_CHANGES`], and main's lines of WR-587's loop normal rows (replayed with main's
/// loop normal, [`crate::wr587_replay::put_back_main_lines`]), before it reverses the
/// tolerance changes.
#[test]
fn the_rows_are_the_whole_golden_diff_from_main() {
    let main: std::collections::BTreeMap<String, u64> = MAIN
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (unit, hex) = l.split_once('\t').expect("unit and digest");
            (
                unit.to_string(),
                u64::from_str_radix(hex, 16).expect("a hex digest"),
            )
        })
        .collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("expected");
    let units = expected_units(&dir);
    assert_eq!(
        main.keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        units,
        "the expected units here and on main"
    );
    let missing: Vec<_> = row_units()
        .into_iter()
        .filter(|u| !units.contains(u))
        .collect();
    assert!(
        missing.is_empty(),
        "ROWS units with no expected file: {missing:?}"
    );
    let mut left_out: Vec<(String, String)> = Vec::new();
    let mut moved = Vec::new();
    for (unit, want) in &main {
        let text = std::fs::read_to_string(dir.join(format!("{unit}.jsonl")))
            .unwrap_or_else(|e| panic!("{unit}: {e}"));
        // (a known-failing case has no line here or on main, so it is outside the golden diff;
        // its rows are certified by the unit's own test all the same. Each one left out must be
        // known-failing on main too: [`LEDGER_KNOWN_FAILING`])
        let known = crate::golden::known_failing(&dir, unit).unwrap_or_else(|e| panic!("{e}"));
        let (out, rows): (Vec<Row>, Vec<Row>) = rows(unit)
            .into_iter()
            .partition(|r| known.contains_key(&r.case));
        left_out.extend(out.into_iter().map(|r| (unit.clone(), r.case)));
        let text = match unit.as_str() {
            "blend.fillet_executive_p3" => {
                put_back_independent_changes(&text, &rows, FILLET_UV_CHANGES, FILLET_UV_MAIN)
            }
            "heal.gap" => put_back_independent_changes(&text, &rows, GAP_CHANGES, GAP_MAIN),
            "heal.face_props" => {
                put_back_independent_changes(&text, &rows, P6_HEAL_CHANGES, P6_HEAL_MAIN)
            }
            "intersect.curve_class_p5" => put_back_independent_changes(
                &text,
                &rows,
                P6_CURVE_CLASS_CHANGES,
                P6_CURVE_CLASS_MAIN,
            ),
            _ => Ok(text),
        }
        .unwrap_or_else(|e| panic!("{unit}: {e}"));
        // (the rows the WR-450 retirement moved: main's lines, [`crate::wr450_rows::DATA`])
        let text = crate::wr450_rows::with_mains_lines(unit, &text);
        // and WR-587's loop normal rows (#21291), each replayed to main's line
        let keep: Vec<&str> = rows.iter().map(|r| r.case.as_str()).collect();
        let text = crate::wr587_replay::put_back_main_lines(unit, &text, &keep)
            .unwrap_or_else(|e| panic!("{unit}: {e}"));
        let mut replay = |case: &str| replay_case(unit, case);
        let got =
            expected_digest(&text, &rows, &mut replay).unwrap_or_else(|e| panic!("{unit}: {e}"));
        if got != *want {
            moved.push(format!("{unit}: {got:016x}, main {want:016x}"));
        }
    }
    assert!(moved.is_empty(), "{}", moved.join("\n"));
    let pinned: Vec<(String, String)> = LEDGER_KNOWN_FAILING
        .iter()
        .map(|&(u, c)| (u.to_string(), c.to_string()))
        .collect();
    assert_eq!(
        left_out, pinned,
        "the known-failing ROWS cases the ledger leaves out (each must be known-failing on main)"
    );
}

/// The [`ROWS`] cases the ledger leaves out as known-failing (`expected/known_failing.json`),
/// each known-failing on main as well (main's list at [`MAIN`]'s commit, checked when the
/// digests were written): unit, case, in the ledger's order. Pinned, so a case that becomes
/// known-failing on this branch only is not dropped from the diff unseen.
const LEDGER_KNOWN_FAILING: &[(&str, &str)] = &[
    ("topology.brep_local_operation", "gen0014"),
    ("topology.brep_local_operation", "gen0019"),
    ("topology.brep_mirror", "mat0076"),
];

/// The Rust output of `case` of `unit`, run as [`certify`] runs it.
fn replay_case(unit: &str, case: &str) -> Result<crate::Output, String> {
    let runner = crate::registry()
        .into_iter()
        .find(|u| u.id() == unit)
        .ok_or_else(|| format!("no unit {unit}"))?;
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let cases = runner.cases(&ctx).map_err(|e| format!("{unit}: {e:?}"))?;
    let c = cases
        .iter()
        .find(|c| c.name == case)
        .ok_or_else(|| format!("{unit}: no case {case}"))?;
    crate::wr450_rows::with_mains_choices_for(unit, case, || runner.run_rust(c))
        .map_err(|e| format!("{unit} {case}: {e:?}"))
}

/// The units of the `*.jsonl` expected files in `dir`.
fn expected_units(dir: &std::path::Path) -> std::collections::BTreeSet<String> {
    std::fs::read_dir(dir)
        .expect("the expected directory")
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .expect("a unit name")
                .to_string()
        })
        .collect()
}

/// Writes [`MAIN`] from main's expected files in `MAIN_EXPECTED_DIR` (every unit, each
/// [`ROWS`] case's line blessed again by main's kernel) to `MAIN_EXPECTED_OUT`, after the line
/// `# main <MAIN_SHA>`.
///
/// To make that directory: in a worktree at main's commit, check out main's
/// `crates/goldens/expected`, delete from it the line of every [`ROWS`] case, and bless those
/// units again (`cargo run --profile goldens -p goldens -- --unit <unit> ... --goldens
/// --bless`), which writes each deleted line from main's kernel and leaves every other line as
/// main's file has it; then run this test (`MAIN_EXPECTED_DIR=<that expected dir>
/// MAIN_SHA=<main's commit> MAIN_EXPECTED_OUT=<file> cargo test --profile goldens -p goldens
/// --lib certify::trim_tolerance::main_expected_digests -- --ignored --exact`) and copy the
/// file to `trim_tolerance_main.tsv`.
#[test]
#[ignore = "writes the main digests of the ledger; run on main's expected files"]
fn main_expected_digests() {
    use std::fmt::Write as _;
    let dir = std::env::var("MAIN_EXPECTED_DIR").expect("MAIN_EXPECTED_DIR");
    let sha = std::env::var("MAIN_SHA").expect("MAIN_SHA");
    let mut out = format!("# main {sha}\n");
    for unit in expected_units(std::path::Path::new(&dir)) {
        let text =
            std::fs::read_to_string(std::path::Path::new(&dir).join(format!("{unit}.jsonl")))
                .unwrap_or_else(|e| panic!("{unit}: {e}"));
        let mut replay = |case: &str| Err(format!("{case}: main's file has no rows"));
        let digest = expected_digest(&text, &[], &mut replay).expect("main's file");
        writeln!(out, "{unit}\t{digest:016x}").expect("written");
    }
    std::fs::write(
        std::env::var("MAIN_EXPECTED_OUT").expect("MAIN_EXPECTED_OUT"),
        out,
    )
    .expect("written");
}

/// An edge that holds a UV trim on one of its faces only is bounded on the other against
/// that face's surface ([`edge_bounds`]): a box edge with its trims made on its first face, its
/// second face's surface then swapped for that of a face the edge does not touch (so the edge
/// lies up to the box's size off it). The first face's trims still certify; the second face's
/// bound does not, where bounding the trims alone saw nothing wrong (#18166 review).
#[test]
fn an_edge_face_without_a_trim_is_bounded_against_its_surface() {
    let mut k = Kernel::default();
    let mut b = solidcore::primitive::api_primitives::create_box(
        &mut k,
        Point3::new(0.0, 0.0, 0.0),
        10.0,
        20.0,
        30.0,
    )
    .expect("a box");
    let e = b.edges().iter().map(|(e, _)| e).next().expect("an edge");
    let faces = edge::faces(&b, e, None).expect("faces");
    assert_eq!(faces.len(), 2);
    let uses = b.edge(e).expect("edge").edgeuses.clone();
    for &eu in &uses {
        if eu1::face(&b, eu).expect("face") == Some(faces[0]) {
            solidcore::topology::edgeuse::p3::get_or_create_uv_trim_curve(
                &k, &mut b, eu, false, false, true,
            )
            .expect("a trim")
            .status
            .expect("a trim");
        }
    }
    // (the box's own trims, or a trim made with its mate's, are dropped from the second face:
    // the edge must lie on one face without a trim)
    for &eu in &uses {
        if eu1::face(&b, eu).expect("face") == Some(faces[1]) {
            b.edgeuse_mut(eu).expect("use").uv_trim_curve = None;
        }
    }
    let on_second = |b: &Brep| {
        uses.iter()
            .filter(|&&eu| eu1::face(b, eu).expect("face") == Some(faces[1]))
            .any(|&eu| b.edgeuse(eu).expect("use").uv_trim_curve.is_some())
    };
    assert!(!on_second(&b), "the second face holds no trim");
    let tol = b.edge(e).expect("edge").tolerance;
    let certified = |bounds: &[Option<f64>]| bounds.iter().all(|x| x.is_some_and(|x| x <= tol));
    let (bounds, trimless) = edge_bounds(&k, &b, e, tol);
    assert_eq!(trimless, 1, "{bounds:?}");
    assert!(bounds.len() >= 2 && certified(&bounds), "{bounds:?}");
    let others: Vec<FaceId> = b
        .faces()
        .iter()
        .map(|(f, _)| f)
        .filter(|f| !faces.contains(f))
        .collect();
    assert_eq!(others.len(), 4);
    for o in others {
        let mut moved = b.clone_standing_for_self().expect("a copy");
        let surface = moved.face(o).expect("face").surface.clone();
        moved.face_mut(faces[1]).expect("face").surface = surface;
        let (bounds, trimless) = edge_bounds(&k, &moved, e, tol);
        assert_eq!(trimless, 1);
        assert!(!certified(&bounds), "face {}: {bounds:?}", o.index());
        // (the trims alone still certify)
        let trims: Vec<Option<f64>> = uses
            .iter()
            .filter_map(|&eu| match use_bound(&k, &moved, e, eu) {
                UseBound::NoTrim => None,
                UseBound::Open => Some(None),
                UseBound::Bound(x) => Some(Some(x)),
            })
            .collect();
        assert!(!trims.is_empty() && certified(&trims), "{trims:?}");
    }
}

/// #18166 regression: `heal.stitch_p2` `cov_tjunc8_do_cand_v` joins an edge onto edge 3,
/// which keeps its own curve for the joined edge's face, 2.427e-4 from that face's surface,
/// and upstream raises the tolerance only to the two curves' distance (`GlueEdges`,
/// `UpdateObjectTolerance`): edge 3 was left at 2.181e-4. When the stitch ends it now raises
/// the tolerance to the kept curve's proved distance to every face its joins gave it without
/// a trim (`topology::edgeuse::p3::cover_joined_faces`), so every edge of the stitched body is within
/// its tolerance of each face on which it holds no trim ([`face_bound`]).
#[test]
fn a_stitch_join_keeps_its_edge_covering_the_faces_it_joins() {
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
        .find(|c| c.name == "cov_tjunc8_do_cand_v")
        .expect("the case");
    super::watch_written();
    let out = runner.run_rust(case);
    let bodies = super::take_written();
    out.expect("the case runs");
    let k = Kernel::default();
    let mut bad = Vec::new();
    let mut checked = 0;
    for (i, b) in bodies.iter().enumerate() {
        for (e, rec) in b.edges().iter() {
            for f in edge::faces(b, e, None).expect("faces") {
                let trimmed = rec.edgeuses.iter().any(|&eu| {
                    eu1::face(b, eu).expect("face") == Some(f)
                        && b.edgeuse(eu).expect("use").uv_trim_curve.is_some()
                });
                if trimmed {
                    continue;
                }
                checked += 1;
                let x = face_bound(&k, b, e, f, rec.tolerance);
                if !matches!(x, Some(x) if x <= rec.tolerance) {
                    bad.push(format!(
                        "body {i} edge {} face {}: tol {:e}, B {x:?}",
                        e.index(),
                        f.index(),
                        rec.tolerance
                    ));
                }
            }
        }
    }
    assert!(checked > 0, "no face without a trim");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The bodies `unit`'s case `name` writes.
fn stitched_bodies(unit: &str, name: &str) -> Vec<Brep> {
    let runner = crate::registry()
        .into_iter()
        .find(|u| u.id() == unit)
        .expect("the unit");
    let ctx = CaseContext {
        root: repo_root(),
        seeds: 16,
    };
    let cases = runner.cases(&ctx).expect("cases");
    let case = cases.iter().find(|c| c.name == name).expect("the case");
    super::watch_written();
    let out = runner.run_rust(case);
    let bodies = super::take_written();
    out.expect("the case runs");
    bodies
}

/// #18166 regression: `primitive.api_primitives` `gen1108` builds a pyramid whose stitch
/// cover raises edges 6, 9 and 15 (lines 5.5e8 long, given a tolerance of 1e-5) for their
/// trimless faces, while their UV line trims are bounded at 2.5e-5 to 2.8e-5. The cover now
/// bounds a UV line trim on a plane (`topology::edgeuse::p3` `raised_trim_bound`) and raises
/// once to the largest trim bound (`cover_raised_trims`; bounding again at each raised
/// tolerance climbed to 5.8e-5). Every raised edge covers its trims and faces, and is no
/// looser than the largest of those bounds (within 1%).
#[test]
fn a_stitch_cover_raises_its_edges_to_their_trims_bounds() {
    let bodies = stitched_bodies("primitive.api_primitives", "gen1108");
    let k = Kernel::default();
    let mut bad = Vec::new();
    let mut raised = 0;
    for (i, b) in bodies.iter().enumerate() {
        for (e, rec) in b.edges().iter() {
            let tol = rec.tolerance;
            if tol <= 1.0e-5 {
                continue;
            }
            raised += 1;
            let mut bounds = trim_bounds(&k, b, e);
            for f in edge::faces(b, e, None).expect("faces") {
                let trimmed = rec.edgeuses.iter().any(|&eu| {
                    eu1::face(b, eu).expect("face") == Some(f)
                        && b.edgeuse(eu).expect("edgeuse").uv_trim_curve.is_some()
                });
                if !trimmed {
                    bounds.push(face_bound(&k, b, e, f, tol));
                }
            }
            let Some(most) = bounds
                .iter()
                .map(|x| x.unwrap_or(f64::INFINITY))
                .reduce(f64::max)
            else {
                continue;
            };
            if !(most <= tol && tol <= 1.01 * most) {
                bad.push(format!(
                    "body {i} edge {}: tol {tol:e} bounds {bounds:?}",
                    e.index()
                ));
            }
        }
    }
    assert!(raised >= 3, "the cover raised {raised} edges");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// #18166 regression: `heal.stitch_p1` `cov_tjunc9_into_shell` raises vertex 22 with an edge
/// its stitch cover raised (`edge::set_tolerance` keeps a vertex at least its edges'
/// tolerance), but an edge of that vertex ends 1.02 from it, under 1.1e-3. When the cover
/// raises an edge it now raises each of the edge's vertices to its proved distance to the
/// ends of all its edges (`topology::edgeuse::p3` `cover_vertex_ends`), so every vertex of
/// the stitched body covers its edges' ends ([`vertex_covers_its_ends`]).
#[test]
fn a_stitch_cover_keeps_its_vertices_covering_their_edges_ends() {
    let bodies = stitched_bodies("heal.stitch_p1", "cov_tjunc9_into_shell");
    let k = Kernel::default();
    let mut bad = Vec::new();
    let mut checked = 0;
    for (i, b) in bodies.iter().enumerate() {
        for (v, rec) in b.vertices().iter() {
            if solidcore::topology::vertex::edges(b, v, None)
                .expect("edges")
                .is_empty()
            {
                continue;
            }
            checked += 1;
            if !vertex_covers_its_ends(&k, b, v, rec.tolerance) {
                bad.push(format!(
                    "body {i} vertex {}: tol {:e}",
                    v.index(),
                    rec.tolerance
                ));
            }
        }
    }
    assert!(checked > 0, "no vertex with an edge");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// What leaves edge `e` of `b` uncovered: each of its trims ([`trim_bounds`]) and each face on
/// which it holds no trim ([`face_bound`]) whose enclosure is not within its tolerance; and how
/// many trims and faces were checked.
fn uncovered(k: &Kernel, b: &Brep, e: EdgeId) -> (Vec<String>, usize) {
    let rec = b.edge(e).expect("edge");
    let mut bad = Vec::new();
    let mut checked = 0;
    for x in trim_bounds(k, b, e) {
        checked += 1;
        if !matches!(x, Some(x) if x <= rec.tolerance) {
            bad.push(format!("trim: tol {:e}, B {x:?}", rec.tolerance));
        }
    }
    for f in edge::faces(b, e, None).expect("faces") {
        let trimmed = rec.edgeuses.iter().any(|&eu| {
            eu1::face(b, eu).expect("face") == Some(f)
                && b.edgeuse(eu).expect("use").uv_trim_curve.is_some()
        });
        if trimmed {
            continue;
        }
        checked += 1;
        let x = face_bound(k, b, e, f, rec.tolerance);
        if !matches!(x, Some(x) if x <= rec.tolerance) {
            bad.push(format!(
                "face {}: tol {:e}, B {x:?}",
                f.index(),
                rec.tolerance
            ));
        }
    }
    (bad, checked)
}

/// #18166 regression: the stitch covers the faces its joins give an edge when the operation
/// ends, on the trims and faces it ends with, not at the join. `heal.stitch_p1` `cov_cube4_stitch`
/// joins a face to edge 28 that holds no trim yet; a join-time cover raised the edge from 1e-5
/// to 1.0237e-5, past anything the result needs: the stitch then fits trims on both of the
/// edge's faces, and the edge's tolerance is the larger of their enclosures `B` exactly
/// (1.0119866231728448e-5, the raise a corner fix made to cover the trim it moved), with
/// nothing added by the join. Edge 17 of the same body ends with no trim on the face its join
/// gave it: its tolerance covers that face's enclosure after the stitch's orientation reverses
/// the face's surface (the cover at the end of the stitch, 1.20009989803294e-5, is the
/// enclosure before it, which comes out 2.4e-19 larger on the reversed surface). Both edges
/// are within their tolerance of every trim and every trimless face.
#[test]
fn a_stitch_covers_its_joins_on_the_final_trims() {
    let k = Kernel::default();
    let bodies = stitched_bodies("heal.stitch_p1", "cov_cube4_stitch");
    let b = bodies.first().expect("the stitched body");
    let e28 = EdgeId::from_index(28);
    let trims: Vec<f64> = trim_bounds(&k, b, e28)
        .into_iter()
        .map(|x| x.expect("an enclosed trim of edge 28"))
        .collect();
    assert_eq!(trims.len(), 2, "edge 28: a trim on each face");
    let needed = trims.iter().copied().fold(0.0_f64, f64::max);
    assert_eq!(
        b.edge(e28).expect("edge 28").tolerance.to_bits(),
        needed.to_bits(),
        "edge 28: the tolerance is its trims' largest B, {needed:e}"
    );
    let mut bad = Vec::new();
    for e in [EdgeId::from_index(17), e28] {
        let (x, checked) = uncovered(&k, b, e);
        assert!(checked > 0, "edge {}: nothing checked", e.index());
        bad.extend(x.into_iter().map(|x| format!("edge {}: {x}", e.index())));
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// #18166 regression: `heal.stitch_p1` `cov_tjunc11_stitch` gives edge 12 a face by a join and
/// ends with no trim of the edge there; the edge ends within its tolerance of that face's
/// enclosure and of every trim. The cover at the end of the stitch raises it to the face's
/// enclosure, 1.0687382852519145e-5, and the orientation that follows reverses the face's
/// surface, on which the same enclosure is 1.0687382852519804e-5; the edge is kept covering
/// the face across it.
#[test]
fn a_stitch_covers_the_faces_its_joins_give_an_edge() {
    let k = Kernel::default();
    let bodies = stitched_bodies("heal.stitch_p1", "cov_tjunc11_stitch");
    let b = bodies.first().expect("the stitched body");
    let (bad, checked) = uncovered(&k, b, EdgeId::from_index(12));
    assert!(checked > 0, "edge 12: nothing checked");
    assert!(bad.is_empty(), "edge 12: {}", bad.join("\n"));
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
