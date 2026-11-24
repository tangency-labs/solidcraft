//! The rows that retiring `offset_analytic_mate_edges` moves (WR-495, tangency-labs/solidcraft#21301),
//! each replayed and held to main's facts (tests only).
//!
//! `MateEdges` now replaces each marched mate piece that lies on an exact line or conic of
//! its two analytic surfaces with that curve. So the moved rows hold the same bodies with
//! exact edge curves where main held marched B-splines. Their keys move with those curves:
//! the entity type codes (`edge_kinds`, `record`), the geometry digests, and the UV trims
//! that a line on two planes does not need (`uv_curve_owners`). The mass properties move
//! with the integration over the new edges.
//!
//! The rows are [`FILLET`] (2 in `blend.fillet_executive_p3`), [`OFFSET`] (109 in
//! `offset.offset_executive_p2`) and [`LOCAL`] (56 in `topology.brep_local_operation`). Main's
//! facts of each row were measured on main with the same measures (written by the ignored
//! [`main_facts`] generator):
//! - [`crate::soundness`]: the kernel's soundness check, the manifold test, the volume's
//!   certified enclosure, and the face, edge and vertex counts;
//! - [`unproved`] on every body the runner hands to [`crate::dump`].
//!
//! Each test runs its unit's moved rows again and asserts, row by row:
//! 1. the encoded row agrees with its committed line, as the goldens compare them;
//! 2. the bodies are sound exactly where main's are, with main's check line counts and
//!    manifold result. Their counts are main's, except in [`ONE_CIRCLE`]. There, main split an
//!    inner circle into two arcs at a degree-2 vertex. This branch keeps it as one closed
//!    circle, like the outer circles, so it has one edge and one vertex fewer and every circle
//!    edge is closed ([`assert_one_circle`]);
//! 3. the certified volume enclosure overlaps main's, so the two bodies' volumes may be the same
//!    (only that: some enclosures are wide);
//! 4. each body has no more unproved edges than main's. An edge is proved when
//!    `curve_on_surfaces` proves its curve within its tolerance of each of its faces, the
//!    measure main's counts were taken with. Where main's count is 0 and that measure leaves
//!    an edge, the edge may instead be proved through the plane that caps a cylinder, for a
//!    circle on the cylinder's end height ([`at_cylinder_end`]). Main's count stays 0 under
//!    that wider measure, so the comparison is by one measure either way. Rows whose main
//!    run had no measured body (`fact` `None`) are held to 1 and 4 only;
//! 5. in `blend.fillet_executive_p3`, both the new area and volume and main's are within the
//!    record's accuracy 1e-4 of a certified enclosure of the shelled body's true values
//!    ([`assert_mass_within_accuracy`]). They differ by a few ulps.
//!
//! Nothing is sampled: every row that moves is in a list, and every list entry is run.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use serde_json::Value;
use solidcore::curve::curve::CurveKind;
use solidcore::mass::enclosure::{enclose_mass_properties, VolumeSign};
use solidcore::offset::curve_on_surfaces::curve_on_surfaces;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::{edge, Brep};
use solidcore::Kernel;

use crate::case::regression_case;
use crate::dump::Captured;
use crate::golden::{default_dir, encode, read};
use crate::soundness::Soundness;
use crate::UnitDiff;

/// What [`crate::soundness`] found of a row's body on main.
struct Fact {
    /// Sound.
    sound: bool,
    /// `check_brep`'s line count; `None` where the check failed to run.
    lines: Option<usize>,
    /// The manifold test.
    manifold: Option<bool>,
    /// Faces, edges, vertices.
    counts: [usize; 3],
    /// The certified volume enclosure of a positive volume.
    volume: Option<(f64, f64)>,
}

/// A moved row: its case, main's soundness facts and main's unproved edge count per
/// captured body, in capture order.
struct Row {
    case: &'static str,
    fact: Option<Fact>,
    unproved: &'static [usize],
}

include!("wr495_rows/main_facts.rs");

/// The offset rows where main split a circle into two arcs and this branch keeps it whole.
const ONE_CIRCLE: [&str; 3] = [
    "api_shell_cylinder_r5_top_open_tol_1e6",
    "api_shell_cylinder_top_extend_merge_in",
    "api_shell_cylinder_top_face",
];

/// The unit `id`.
fn unit(id: &str) -> Box<dyn UnitDiff> {
    crate::registry()
        .into_iter()
        .find(|u| u.id() == id)
        .unwrap_or_else(|| panic!("no unit {id}"))
}

/// Two runs of `case`: the encoded row of a plain run, and what the soundness measure found
/// and the bodies the runner handed over in a measured one.
fn replay(u: &dyn UnitDiff, case: &str) -> (Value, Vec<Soundness>, Vec<Captured>) {
    let c = regression_case(u, case);
    // the committed line from a run with nothing captured: measuring names the body's
    // geometry owners, which a runner may measure before recording them (local operation)
    let line = u.run_rust(&c).unwrap_or_else(|e| panic!("{case}: {e:?}"));
    crate::soundness::start();
    crate::dump::capture();
    let out = u.run_rust(&c);
    let bodies = crate::dump::take();
    let found = crate::soundness::take();
    if let Err(e) = out {
        panic!("{case}: {e:?}");
    }
    (encode(case, &line), found, bodies)
}

/// The edges of `b` that are not proved: an edge with a curve is proved when
/// `curve_on_surfaces` proves the curve within the edge's tolerance of both its faces (of
/// its one face for an edge on one face), or, with `at_ends`, when [`at_cylinder_end`]
/// proves it.
fn unproved(b: &Brep, at_ends: bool) -> Vec<u32> {
    let k = Kernel::default();
    let mut left = Vec::new();
    for e in brep_p1::edges(b, None).unwrap() {
        let Some(curve) = b.edge(e).unwrap().curve.as_ref() else {
            continue;
        };
        let tol = edge::tolerance(b, e).unwrap();
        let ivl = edge::interval(b, e).unwrap();
        let surfaces: Vec<_> = edge::faces(b, e, None)
            .unwrap()
            .into_iter()
            .map(|f| face_p2::surface(b, f).unwrap())
            .collect();
        let pair = match surfaces.as_slice() {
            [Some(s0), Some(s1)] => Some([*s0, *s1]),
            [Some(s0)] => Some([*s0, *s0]),
            _ => None,
        };
        let proved = pair.is_some_and(|pair| {
            curve_on_surfaces(&k, curve, ivl, pair, tol).unwrap_or(false)
                || at_ends && at_cylinder_end(&k, curve, ivl, pair[0], pair[1], tol)
                || at_ends && at_cylinder_end(&k, curve, ivl, pair[1], pair[0], tol)
        });
        if !proved {
            left.push(e.index());
        }
    }
    left
}

/// Whether `curve` over `ivl` is within `tol` of the bounded cylinder `cyl`, shown through the
/// plane `plane` that caps it, where `curve_on_surfaces` cannot show it directly: a circle on
/// the cylinder's end height. Its interval box of the curve's heights there is as wide as
/// its rounding of the rational arc, so the clamp to the cylinder's heights costs it a slack
/// of that size (`Shape::slack` in `curve_on_surfaces`).
///
/// With `Z` the cylinder's axis, `v(x) = (x - O) . Z / |Z|^2` the height and `[bot, top]`
/// its heights:
/// 1. `curve_on_surfaces` proves the curve within `tol / 4` of the cylinder evaluated out of
///    bounds (no height clamp): each `x` has a point `s` of it with `|s - x| <= tol / 4`, so
///    `|v(s) - v(x)| <= tol / (4 |Z|)`;
/// 2. it proves the curve within `tol / 4` of `plane`, normal `n`, through `P`:
///    `|(x - P) . n| <= |n| tol / 4`;
/// 3. so with `Z = n (n . Z) / |n|^2 + Zp`, `|v(x) - v(P)| <= beta` where
///    `beta = (|n . Z| tol / (4 |n|) + D |Zp|) / |Z|^2`, `D` bounding `|x - P|` over the
///    curve (its NURB's control points, whose hull holds it: the weights are positive);
/// 4. the surface is `O + v Z + R c(u)` (zero semi-angle, so `R` is constant), taken as its
///    formula as `curve_on_surfaces` takes it, so clamping `s` to `[bot, top]` moves it
///    along the axis by `|Z|` times its height's
///    excess, at most `beta + past + tol / (4 |Z|)` with `past` the distance of `v(P)` from
///    `[bot, top]`. So `x` is within `tol / 2 + (beta + past) |Z|` of the bounded cylinder,
///    which must be at most `tol` after a rounding allowance: each term is a sum of at most
///    a few dozen roundings of quantities at most `M` (the largest coordinate, radius or
///    height in play), so `1e-12` relative plus `1e-13 M` absolute covers it many times over.
///    It also covers the kernel clamping its height parameter rather than `v`: they differ by
///    `|R| |c . Z| / |Z|^2`, a few ulps of `|R|` for the frames `curve_on_surfaces` accepts.
fn at_cylinder_end(
    k: &Kernel,
    curve: &solidcore::curve::curve::Curve,
    ivl: solidcore::geom::extent1d::Extent1d,
    cyl: &Surface,
    plane: &Surface,
    tol: f64,
) -> bool {
    let (SurfaceKind::Cone(c), SurfaceKind::Plane(p)) = (cyl.kind(), plane.kind()) else {
        return false;
    };
    // the kinds `curve_on_surfaces` measures by their NURB, whose control points bound `D`
    // (it measures a line by its segment, which its base's span need not hold)
    if !matches!(curve.kind(), CurveKind::BSpline(_) | CurveKind::Ellipse(_)) {
        return false;
    }
    // a zero semi-angle: the surface's points at one angle differ by multiples of `Z`
    if c.semi_angle_deg() != 0.0 || c.rev().is_out_of_bounds_enabled() {
        return false;
    }
    let mut free = cyl.clone();
    if let SurfaceKind::Cone(c) = free.kind_mut() {
        c.set_out_of_bounds_enabled(true);
    }
    let quarter = tol / 4.0;
    if !curve_on_surfaces(k, curve, ivl, [&free, &free], quarter).unwrap_or(false)
        || !curve_on_surfaces(k, curve, ivl, [plane, plane], quarter).unwrap_or(false)
    {
        return false;
    }
    let Some(Ok((points, weights))) = curve
        .b_spline_base()
        .map(solidcore::nurbs::b_spline_curve::BSplineCurve::control_polygon)
    else {
        return false;
    };
    if weights.iter().any(|&w| w <= 0.0) {
        return false;
    }
    let pos = c.rev().position();
    let (o, z) = (pos.origin(), pos.z_axis());
    let zz = z.dot(z);
    let heights = cyl.step_uv_domain().v_interval();
    let (bot, top) = (heights.min(), heights.max());
    let pp = p.position();
    let (at, n) = (pp.origin(), pp.x_axis().cross(pp.y_axis()));
    let nn = n.dot(n);
    let d = points
        .iter()
        .map(|&x| (x - at).length())
        .fold(0.0, f64::max);
    let nz = n.dot(z);
    let zp = z - n * (nz / nn);
    let beta = (nz.abs() * quarter / nn.sqrt() + d * zp.length()) / zz;
    let hp = (at - o).dot(z) / zz;
    let past = (hp - top).max(bot - hp).max(0.0);
    let m = [
        o.length(),
        at.length(),
        d,
        bot.abs(),
        top.abs(),
        c.base_radius().abs(),
        1.0,
    ]
    .into_iter()
    .fold(0.0, f64::max);
    let bound = 2.0 * quarter + (beta + past) * zz.sqrt();
    let allowance = 1e-12 * bound + 1e-13 * m;
    bound + allowance <= tol
}

/// The type code of an exact circle (`SmCircle`).
const CIRCLE: i64 = 14021;

/// In an [`ONE_CIRCLE`] row: every circle edge of `b` is closed (one vertex at both ends),
/// so no circle is split into arcs.
fn assert_one_circle(case: &str, b: &Brep) {
    for e in brep_p1::edges(b, None).unwrap() {
        let Some(curve) = b.edge(e).unwrap().curve.as_ref() else {
            continue;
        };
        if curve.type_code() == CIRCLE {
            let v = edge::vertices(b, e, None).unwrap();
            assert_eq!(
                v.len(),
                1,
                "{case}: circle edge {} has vertices {v:?}",
                e.index()
            );
        }
    }
}

/// Whether two closed intervals meet.
fn overlap((a, b): (f64, f64), (c, d): (f64, f64)) -> bool {
    a <= d && c <= b
}

/// Asserts each of `rows` of unit `id` (module docs, steps 1 to 4).
fn assert_rows(id: &str, rows: &[Row]) {
    let u = unit(id);
    let golden = read(&default_dir(), id).unwrap();
    let mut failures = Vec::new();
    for row in rows {
        let case = row.case;
        let (encoded, found, bodies) = replay(u.as_ref(), case);
        let line = golden
            .get(case)
            .unwrap_or_else(|| panic!("{case}: no committed line"));
        if let Some(change) = crate::golden::diff(line, &encoded, u.abs_floors()) {
            failures.push(format!("{case}: not its committed line: {change:?}"));
        }
        match (&row.fact, found.as_slice()) {
            (None, []) => {}
            (Some(main), [now]) => {
                if now.sound() != main.sound
                    || now.check.as_ref().ok().map(Vec::len) != main.lines
                    || now.manifold != main.manifold
                {
                    failures.push(format!("{case}: soundness {}", now.line()));
                }
                let mut counts = main.counts;
                if ONE_CIRCLE.contains(&case) {
                    counts[1] -= 1;
                    counts[2] -= 1;
                }
                if now.counts != counts {
                    failures.push(format!("{case}: counts {:?}", now.counts));
                }
                let volume = match &now.volume {
                    Some(VolumeSign::Positive(e)) => Some((e.lo, e.hi)),
                    _ => None,
                };
                match (main.volume, volume) {
                    (None, None) => {}
                    (Some(m), Some(n)) if overlap(m, n) => {}
                    (m, n) => failures.push(format!("{case}: volume {n:?} against main's {m:?}")),
                }
            }
            (m, n) => failures.push(format!(
                "{case}: {} soundness measures against main's {}",
                n.len(),
                usize::from(m.is_some())
            )),
        }
        if bodies.len() != row.unproved.len() {
            failures.push(format!("{case}: {} bodies", bodies.len()));
        }
        for (c, &main) in bodies.iter().zip(row.unproved) {
            let b = c.brep.as_ref().unwrap_or_else(|e| panic!("{case}: {e}"));
            // main's count is `curve_on_surfaces`' alone; where that is 0, it stays 0 with
            // `at_cylinder_end` too, so either way the two counts are by the same measure
            let mut left = unproved(b, false);
            if left.len() > main && main == 0 {
                left = unproved(b, true);
            }
            if left.len() > main {
                failures.push(format!(
                    "{case}: {} {}: unproved {left:?}, main {main}",
                    c.label, c.n
                ));
            }
            if ONE_CIRCLE.contains(&case) {
                assert_one_circle(case, b);
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn offset_rows_keep_mains_facts() {
    assert_rows("offset.offset_executive_p2", &OFFSET);
}

#[test]
fn local_operation_rows_keep_mains_facts() {
    assert_rows("topology.brep_local_operation", &LOCAL);
}

/// The shell mass properties `[area, volume, mass]` of [`FILLET`]'s rows on main.
const FILLET_MAIN_MASS: [(&str, [f64; 3]); 2] = [
    (
        "real-box-fillet1v-r3-shell-closed",
        [3516.2566310324937, 3485.973445725496, 3485.973445725496],
    ),
    (
        "real-box-fillet4v-r3-shell-closed",
        [3457.026524130091, 3431.893782901762, 3431.893782901762],
    ),
];

/// The record's mass accuracy (`brep_mass_properties`' relative accuracy 1e-4).
const MASS_ACCURACY: f64 = 1e-4;

/// Asserts that the new value `n` and main's `o` are each within the record's accuracy of
/// the certified enclosure `[lo, hi]` of the true value, and that the enclosure is narrow
/// enough to say so: `|n - o| / 2 + (hi - lo)` within that accuracy (as
/// `tests/moved_mass_rows` classifies an EQUAL entry).
fn assert_mass_within_accuracy(what: &str, o: f64, n: f64, lo: f64, hi: f64) {
    let scale = o.abs().max(n.abs());
    let allowed = MASS_ACCURACY * scale;
    for x in [o, n] {
        assert!(
            lo - allowed <= x && x <= hi + allowed,
            "{what}: {x} beyond [{lo}, {hi}]"
        );
    }
    assert!(
        (n - o).abs() / 2.0 + (hi - lo) <= allowed,
        "{what}: enclosure [{lo}, {hi}] too wide for {o} against {n}"
    );
}

/// [`FILLET`]'s rows (module docs, steps 1 to 5).
#[test]
fn fillet_rows_keep_mains_facts_and_mass_within_accuracy() {
    let id = "blend.fillet_executive_p3";
    assert_rows(id, &FILLET);
    let u = unit(id);
    for (case, main) in FILLET_MAIN_MASS {
        let (encoded, _, bodies) = replay(u.as_ref(), case);
        let now: Vec<f64> = encoded["values"]["shell_mass_properties"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let [c] = bodies.as_slice() else {
            panic!("{case}: {} bodies", bodies.len());
        };
        let mut b = c.brep.as_ref().unwrap().clone_standing_for_self().unwrap();
        let k = Kernel::default();
        let e = enclose_mass_properties(&k, &mut b, 1e-5).unwrap();
        assert_mass_within_accuracy(case, main[0], now[0], e.area.lo, e.area.hi);
        for i in [1, 2] {
            assert_mass_within_accuracy(case, main[i], now[i], e.volume.lo, e.volume.hi);
        }
    }
}

/// One [`Row`] as `main_facts.rs` writes it: `case`'s soundness facts and the unproved edge
/// count of each captured body, as this tree measures them.
fn row_line(u: &dyn UnitDiff, case: &str) -> String {
    let (_, found, bodies) = replay(u, case);
    let fact = match found.as_slice() {
        [] => "None".to_string(),
        [s] => {
            let volume = match &s.volume {
                Some(VolumeSign::Positive(e)) => format!("Some(({:?}, {:?}))", e.lo, e.hi),
                _ => "None".to_string(),
            };
            format!(
                "Some(Fact {{ sound: {}, lines: {:?}, manifold: {:?}, counts: {:?}, volume: {volume} }})",
                s.sound(),
                s.check.as_ref().ok().map(Vec::len),
                s.manifold,
                s.counts,
            )
        }
        many => panic!("{case}: {} soundness measures", many.len()),
    };
    let unproved: Vec<usize> = bodies
        .iter()
        .map(|c| {
            unproved(
                c.brep.as_ref().unwrap_or_else(|e| panic!("{case}: {e}")),
                false,
            )
            .len()
        })
        .collect();
    format!("    Row {{ case: {case:?}, fact: {fact}, unproved: &{unproved:?} }},\n")
}

/// Writes `main_facts.rs` (to `$WR495_FACTS_OUT`) as this tree measures [`FILLET`]'s,
/// [`OFFSET`]'s and [`LOCAL`]'s cases. Run on main (`$MAIN_SHA`), with this module, `lib.rs`'s
/// line for it, the runners' `crate::dump::body` hooks it reads and this branch's
/// `offset/curve_on_surfaces.rs` (the measure [`unproved`] uses) copied in, it writes the
/// committed file: the kernel is main's but for that file, so the facts are main's, counted by
/// this branch's measure. Main's `keeps_old_edge` calls the file too; its one change cuts a
/// crease crossing whose parameter rounds onto a segment's end at that end, where main left a
/// piece of no length that no halving proves; no other crossing is cut differently (`git diff origin/main -- <those files> | git apply` in a worktree at main, then
/// `cargo test --profile goldens -p goldens --lib -- --ignored --exact wr495_rows::main_facts`).
#[test]
#[ignore = "generator: run on main with this module's hooks"]
fn main_facts() {
    use std::fmt::Write as _;
    let out = std::env::var("WR495_FACTS_OUT").expect("WR495_FACTS_OUT");
    let sha = std::env::var("MAIN_SHA").expect("MAIN_SHA");
    let mut s = format!(
        "// Main's facts of each moved row, measured on main {sha} by `wr495_rows::main_facts`\n\
         // (see the module docs), with this branch's `curve_on_surfaces`, so both sides are\n\
         // counted by one measure. Generated: do not edit.\n"
    );
    let units: [(&str, &str, &[Row]); 3] = [
        ("FILLET", "blend.fillet_executive_p3", &FILLET),
        ("OFFSET", "offset.offset_executive_p2", &OFFSET),
        ("LOCAL", "topology.brep_local_operation", &LOCAL),
    ];
    for (name, id, rows) in units {
        let u = unit(id);
        let n = rows.len();
        writeln!(
            s,
            "\n/// `{id}`'s moved rows.\nconst {name}: [Row; {n}] = ["
        )
        .expect("written");
        for row in rows {
            s.push_str(&row_line(u.as_ref(), row.case));
        }
        s.push_str("];\n");
    }
    std::fs::write(&out, s).expect("written");
}
