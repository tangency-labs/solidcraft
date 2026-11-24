//! The golden rows retiring `shell_edges_on_analytics_exact` (WR-545) moves: every shell now
//! ends with `make_edges_on_analytics_exact`, which gives each marched edge between two faces
//! on analytic surfaces the exact line or conic the surfaces meet on, and moves a vertex onto
//! it only within the tolerance (`vertex_move_fits`). 64 rows move: 62 of
//! `offset.offset_executive_p2` (59 by `result.edge_kinds`, B-spline edges becoming lines; 33
//! by `result.geometry` and 3 by `source.after.geometry`, the vertex bounds) and 2 of
//! `blend.fillet_executive_p3` (`shell_mass_properties` and `shell_geometry`, a shelled
//! rounded box).
//!
//! Each row in [`ROWS`] is re-run through its unit's runner with `goldens::dump::capture` on
//! and must reproduce the committed row (`golden::diff` with the unit's floors, as the
//! goldens job compares). Then, replayed on every row:
//!
//! * **Exact edges (PROVEN).** On the captured result body, every edge whose curve is a line
//!   or a conic and whose faces all lie on analytic surfaces lies on each face's implicit
//!   surface: `surface::implicit::curve_lies_on`, every Bernstein coefficient of the surface's
//!   equation on the curve's Bézier segments within the rounding of computing it (nothing
//!   sampled). On every row, every line edge the pass can have made (two distinct faces,
//!   both surfaces `is_analytic()`: the guard it skips every other edge by) and every line or
//!   conic edge between such faces is certified on both surfaces, so each edge that became a
//!   line or a conic lies on both; a line that cannot be certified fails the row, as does
//!   any edge between faces on implicit surfaces that is off one. A conic on a surface of
//!   revolution (no implicit here) fails a row whose `result.edge_kinds` moved; on the two
//!   `issue2174_cap_two_rim_edges` rows, whose kinds did not move (only their vertex
//!   bounds), the pass changed no edge's kind, so it made none of their 4 such ellipses. `is_analytic()` also holds
//!   for a surface of extrusion, which has no implicit here: 13 rows (`issue1528` and
//!   `issue1795` plates, `issue2611` half pipes) have 2 to 4 lines between a plane and an
//!   extrusion of a B-spline along z. Each lies on the plane by its implicit and on the
//!   extrusion as one of its rulings (`on_ruling`: both its ends within a millionth of the
//!   edge tolerance of their points on the ruling through one generator point, clamped to
//!   the surface's STEP domain, a distance that is convex along the segment). Line and conic
//!   edges outside the pass are counted and left as they were. The edges whose kind moved
//!   are not listed one by one (`result.edge_kinds` is stored as an aggregate, so main's
//!   kinds are not kept); `result.counts` did not move on any row, and every line and conic
//!   between analytic faces is certified. Each edge's start vertex is within its own or the
//!   edge's tolerance of the curve's start, its end vertex of the curve's end.
//! * **Vertex bounds (an invariant, checked).** Every moved bound of `result.geometry` or
//!   `source.after.geometry` moved by at most the body's tolerance (its entry 0, unchanged):
//!   a vertex moves onto an exact curve only within the tolerance. The moves are 1e-16 to
//!   1.7e-12 against tolerances of 1e-6.
//! * **Shell mass (PROVEN, closed form).** The two `blend.fillet_executive_p3` bodies are the
//!   box `[0, 30] x [0, 20] x [0, 10]` with one (`1v`) or four (`4v`) vertical edges rounded
//!   r 3, shelled inward by 2 and closed: the box minus the cavity `[2, 28] x [2, 18] x
//!   [2, 8]` rounded r 1 about the same axes. Its area is `3444 + 23 pi` (1v) and `3168 +
//!   92 pi` (4v), its volume `3420 + 21 pi` and `3168 + 84 pi` ([`closed_forms`] derives
//!   them). Each new `shell_mass_properties` entry is within `1e-9` relative of its closed
//!   form; the areas are CLOSER (main's were 1.9e-6 and 7.4e-6 short, 5.3e-10 and 2.1e-9
//!   relative, the `4v` one past `1e-9`); the
//!   volumes (and masses, density 1) are FARTHER by 1.1e-10 and 2.2e-10 (3e-14 and 6e-14
//!   relative, far inside the `1e-4` accuracy the integration is asked for; main's happened
//!   to land within 1.6e-12). Only the `1e-9` check gates; the direction is measured and
//!   printed, not pinned. Their `shell_geometry` (edge intervals and points sampled at
//!   fractions of them) moves because the rounds' edges are now exact arcs and lines,
//!   parameterised as such; the exact-edge certificate above covers them.
//!
//! A later change that legitimately moves one of these rows again updates [`ROWS`] (and
//! [`MAIN_SHELL_MASS`]) with that change's derivation: the replay fails until it does.
//!
//! Run: `cargo test --profile goldens -p goldens --test wr545_moved_rows_are_exact`.
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
use goldens::{golden, CaseContext};
use solidcore::curve::curve::p2::CurveKind;
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::math::{Point2, Vec3};
use solidcore::surface::implicit::{curve_lies_on, AnalyticSupport, Implicit};
use solidcore::surface::surf_of_extrusion::p1::SurfOfExtrusion;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p1;
use solidcore::topology::{edge, face, vertex, Brep};
use solidcore::Kernel;

/// A moved row: unit, case, whether its result's edge kinds moved, and main's moved vertex
/// bounds by key (B-rep tolerance, then the bounding box's min and max).
type Moved = (
    &'static str,
    &'static str,
    bool,
    &'static [(&'static str, [f64; 7])],
);

include!("wr545_moved_rows/rows.rs");

/// The goldens' default `--seeds`.
const SEEDS: usize = 16;

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

/// The grid [`on_ruling`] starts its search for a generator parameter on.
const N: usize = 4096;

/// Whether the line `curve` over the edge's interval `ivl` lies on the surface of extrusion
/// `x`, within its domain. The surface is `S(u, v) = g(u) + v d` over its STEP domain
/// (`evaluate_step_point` clamps `(u, v)` into it, so every point it returns is on the
/// surface). For one `u`, each end `p` of the segment gets `v = v0 + (p - S(u, v0)) . d / |d|^2`
/// and its surface point `q = S(u, v)`, clamped. `q` is affine in `v` along the ruling, and
/// the clamped `v`s of the two ends are in the domain, so the segment from `q(start)` to
/// `q(end)` is on the surface. The edge's point at fraction `l` minus that segment's is
/// affine in `l`, and its length is convex, so every point of the edge is within `delta`,
/// the larger of the two ends' `|p - q|`, of the surface. Any `u` gives an upper bound: `u`
/// is only searched for (a grid over the STEP u interval, then a ternary refinement on the
/// first end's distance to the ruling), and the bound is what is checked. Each distance is a
/// few ulps of the coordinates (1e-14 here) off. `delta` must be within a millionth of the
/// edge's tolerance, far past that rounding.
fn on_ruling(
    k: &Kernel,
    curve: &Curve,
    ivl: Extent1d,
    x: &SurfOfExtrusion,
    tolerance: f64,
) -> bool {
    let CurveKind::Line(_) = curve.kind() else {
        return false;
    };
    let d = x.extrusion_vector();
    let ends = [
        curve.evaluate_point(k, ivl.min()).unwrap(),
        curve.evaluate_point(k, ivl.max()).unwrap(),
    ];
    let dom = x.step_uv_domain();
    let (ui, vi) = (dom.u_interval(), dom.v_interval());
    let at_v0 = |u: f64| x.evaluate_step_point(k, Point2::new(u, vi.min())).unwrap();
    let ruling_off = |p: Vec3, u: f64| (p - at_v0(u)).cross(d).length() / d.length();
    let (lo, hi) = (ui.min(), ui.max());
    let at = |i: usize| lo + (hi - lo) * (i as f64 / N as f64);
    let best = (0..=N)
        .min_by(|&i, &j| ruling_off(ends[0], at(i)).total_cmp(&ruling_off(ends[0], at(j))))
        .unwrap();
    let (mut a, mut b) = (at(best.saturating_sub(1)), at((best + 1).min(N)));
    for _ in 0..200 {
        let (m1, m2) = (a + (b - a) / 3.0, b - (b - a) / 3.0);
        if ruling_off(ends[0], m1) <= ruling_off(ends[0], m2) {
            b = m2;
        } else {
            a = m1;
        }
    }
    let u = 0.5 * (a + b);
    let base = at_v0(u);
    let delta = ends
        .iter()
        .map(|&p| {
            let v = vi.min() + (p - base).dot(d) / d.length_squared();
            let q = x.evaluate_step_point(k, Point2::new(u, v)).unwrap();
            p.distance(q)
        })
        .fold(0.0, f64::max);
    eprintln!("WR545   ruling at u {u}: {delta:e} off (tolerance {tolerance:e})");
    delta <= 1e-6 * tolerance
}

/// What [`certify`] found on one body.
#[derive(Debug, Default)]
struct Found {
    /// Lines and conics between analytic faces, each certified on every face's surface.
    exact: usize,
    /// Line and conic edges the pass cannot have made, not certified: not between two distinct
    /// faces whose surfaces are both `is_analytic()`, the guard
    /// `make_edges_on_analytics_exact` skips an edge by.
    outside_the_pass: usize,
    /// Line edges between two such faces not certified on both (a failure).
    uncertifiable: usize,
    /// Conic edges between two such faces not certified on both, a face's surface having no
    /// implicit here (a surface of revolution): a failure on a row whose edge kinds moved;
    /// on a row whose did not, the pass changed no edge's kind, so it made none of them.
    conics_unchanged: usize,
    /// Of `exact`, lines certified on a surface of extrusion as one of its rulings.
    rulings: usize,
    /// Failures: an edge off one of its surfaces, a vertex off its edge's ends.
    failures: Vec<String>,
}

/// The exact-edge and vertex certificate of one body (see the module docs).
fn certify(k: &Kernel, b: &Brep) -> Found {
    let mut found = Found::default();
    for e in p1::edges(b, None).unwrap() {
        let Some(curve) = edge::curve(b, e).unwrap() else {
            continue;
        };
        let faces = edge::faces(b, e, None).unwrap();
        let is_line = matches!(curve.kind(), CurveKind::Line(_));
        let exact_kind = matches!(
            curve.kind(),
            CurveKind::Line(_)
                | CurveKind::Ellipse(_)
                | CurveKind::Hyperbola(_)
                | CurveKind::Parabola(_)
        );
        if exact_kind {
            let surfaces: Vec<_> = faces
                .iter()
                .map(|&f| face::p2::surface(b, f).unwrap())
                .collect();
            let analytic = |s: Option<&Surface>| s.is_some_and(|s| s.is_analytic().unwrap());
            // the guard `make_edges_on_analytics_exact` skips every other edge by
            let eligible = matches!(faces.as_slice(), &[f0, f1] if f0 != f1)
                && surfaces.iter().copied().all(analytic);
            let all_implicit = !surfaces.is_empty()
                && surfaces
                    .iter()
                    .all(|s| s.and_then(AnalyticSupport::of).is_some());
            // each face: a plane, cylinder, cone, sphere or torus by its implicit equation,
            // a surface of extrusion by its rulings (`on_ruling`)
            let mut ruled = false;
            let on: Vec<bool> = surfaces
                .iter()
                .map(|s| match s {
                    Some(s) => {
                        if let Some(a) = AnalyticSupport::of(s) {
                            on_implicit(curve, &a.implicit()) == Some(true)
                        } else if let SurfaceKind::SurfOfExtrusion(x) = s.kind() {
                            ruled = true;
                            on_ruling(
                                k,
                                curve,
                                edge::interval(b, e).unwrap(),
                                x,
                                edge::tolerance(b, e).unwrap(),
                            )
                        } else {
                            false
                        }
                    }
                    None => false,
                })
                .collect();
            if !on.is_empty() && on.iter().all(|&x| x) {
                found.exact += 1;
                found.rulings += usize::from(ruled);
            } else if eligible && is_line {
                found.uncertifiable += 1;
            } else if eligible && !all_implicit {
                found.conics_unchanged += 1;
            } else if all_implicit {
                found.failures.push(format!(
                    "edge {e:?} ({}) off a face's surface: {on:?}",
                    curve.kind().name()
                ));
            } else {
                found.outside_the_pass += 1;
            }
        }
        let ivl = edge::interval(b, e).unwrap();
        let ends = [
            curve.evaluate_point(k, ivl.min()).unwrap(),
            curve.evaluate_point(k, ivl.max()).unwrap(),
        ];
        for (v, end) in [
            (edge::start_vertex(b, e).unwrap(), ends[0]),
            (edge::end_vertex(b, e).unwrap(), ends[1]),
        ] {
            let Some(v) = v else {
                continue;
            };
            let gap = end.distance(vertex::point(b, v).unwrap());
            let allowed = vertex::tolerance(b, v)
                .unwrap()
                .max(edge::tolerance(b, e).unwrap());
            if !(gap <= allowed) {
                found.failures.push(format!(
                    "vertex {v:?} {gap:e} off edge {e:?}'s end (tolerance {allowed:e})"
                ));
            }
        }
    }
    found
}

/// The closed forms of the two shelled boxes: `(area, volume)` for `n` rounded vertical
/// edges. The box `30 x 20 x 10`, area `2200`, volume `6000`; a vertical edge rounded r 3
/// trades its two `3 x 10` wall strips for a quarter cylinder (`-60 + 15 pi`) and takes
/// `9 - 9 pi / 4` off the top and the bottom each, so `-78 + 19.5 pi` of area and
/// `-10 (9 - 9 pi / 4) = -90 + 22.5 pi` of volume. The cavity is the box `26 x 16 x 6` (area
/// `1336`, volume `2496`) with each such edge rounded r 1: `-14 + 3.5 pi` of area, `-6 + 1.5
/// pi` of volume. The shell's area is the sum, its volume the difference.
fn closed_forms(n: f64) -> (f64, f64) {
    let area = 2200.0 + n * (-78.0 + 19.5 * PI) + 1336.0 + n * (-14.0 + 3.5 * PI);
    let volume = 6000.0 + n * (-90.0 + 22.5 * PI) - (2496.0 + n * (-6.0 + 1.5 * PI));
    (area, volume)
}

/// The rows where WR-495's exact mate (retired after WR-545) keeps an inner circle whole
/// instead of main's two arcs, so the body has one vertex fewer (`goldens::wr495_rows`'
/// `ONE_CIRCLE` proves the counts and that every circle edge is closed). The removed vertex
/// set main's vertex box, so its bounds cannot be compared one by one. The check is still
/// derived: every remaining vertex moves by at most the tolerance, and removing a vertex can
/// only shrink the box. So the new box must lie inside main's box grown by the tolerance.
const ONE_CIRCLE: [&str; 1] = ["api_shell_cylinder_r5_top_open_tol_1e6"];

/// Main's `shell_mass_properties` of the two `blend.fillet_executive_p3` rows.
const MAIN_SHELL_MASS: [(&str, f64, [f64; 3]); 2] = [
    (
        "real-box-fillet1v-r3-shell-closed",
        1.0,
        [3516.25662917359, 3485.973445725385, 3485.973445725385],
    ),
    (
        "real-box-fillet4v-r3-shell-closed",
        4.0,
        [3457.026516694307, 3431.893782901541, 3431.893782901541],
    ),
];

/// Whether `new` is nearer the closed form `exact` than `old` (`Some(true)`), farther
/// (`Some(false)`), or neither by more than `slack` (`None`).
fn closer(old: f64, new: f64, exact: f64, slack: f64) -> Option<bool> {
    let (o, n) = ((old - exact).abs(), (new - exact).abs());
    if n + slack < o {
        Some(true)
    } else if o + slack < n {
        Some(false)
    } else {
        None
    }
}

#[test]
fn closed_forms_match_the_derivation() {
    // 1v: 3444 + 23 pi and 3420 + 21 pi; 4v: 3168 + 92 pi and 3168 + 84 pi.
    let (a1, v1) = closed_forms(1.0);
    let (a4, v4) = closed_forms(4.0);
    assert!((a1 - (3444.0 + 23.0 * PI)).abs() < 1e-12);
    assert!((v1 - (3420.0 + 21.0 * PI)).abs() < 1e-12);
    assert!((a4 - (3168.0 + 92.0 * PI)).abs() < 1e-12);
    assert!((v4 - (3168.0 + 84.0 * PI)).abs() < 1e-12);
}

#[test]
fn wr545_moved_rows_replay_and_are_exact() {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    let registry = goldens::registry();
    let dir = golden::default_dir();
    let mut bad = Vec::new();
    let mut units: BTreeMap<&str, (golden::Golden, Vec<goldens::Case>)> = BTreeMap::new();
    let (mut exact_edges, mut bounds) = (0_usize, 0_usize);
    for &(unit, name, kinds_moved, old_bounds) in &ROWS {
        let runner = registry.iter().find(|u| u.id() == unit).expect("unit");
        let (lines, cases) = units.entry(unit).or_insert_with(|| {
            (
                golden::read(&dir, unit).unwrap(),
                runner.cases(&ctx).unwrap(),
            )
        });
        let case = cases.iter().find(|c| c.name == name).expect("case");
        goldens::dump::capture();
        let out = runner.run_rust(case);
        let captured = goldens::dump::take();
        let out = out.unwrap();
        let committed = lines.get(name).expect("committed row");
        if let Some(change) =
            golden::diff(committed, &golden::encode(name, &out), runner.abs_floors())
        {
            bad.push(format!(
                "{unit}/{name}: does not reproduce its row: {change:?}"
            ));
            continue;
        }
        let label = if unit == "blend.fillet_executive_p3" {
            "shell_mass"
        } else {
            "result"
        };
        let bodies: Vec<_> = captured.into_iter().filter(|c| c.label == label).collect();
        assert_eq!(bodies.len(), 1, "{unit}/{name}: {label} bodies");
        let body = bodies.into_iter().next().unwrap().brep.unwrap();
        let k = Kernel::default();
        let found = certify(&k, &body);
        eprintln!(
            "WR545 {unit}/{name}: {} exact edges ({} rulings), {} outside the pass, {} uncertifiable, {} conics on unchanged kinds",
            found.exact,
            found.rulings,
            found.outside_the_pass,
            found.uncertifiable,
            found.conics_unchanged
        );
        for f in &found.failures {
            bad.push(format!("{unit}/{name}: {f}"));
        }
        if found.exact == 0 {
            bad.push(format!("{unit}/{name}: no exact edge"));
        }
        if found.uncertifiable != 0 {
            bad.push(format!(
                "{unit}/{name}: {} edges the pass may have made not certifiable",
                found.uncertifiable
            ));
        }
        if kinds_moved && found.conics_unchanged != 0 {
            bad.push(format!(
                "{unit}/{name}: {} conic edges the pass may have made not certifiable",
                found.conics_unchanged
            ));
        }
        exact_edges += found.exact;
        for &(key, old) in old_bounds {
            let new = out.values.get(key).expect("bounds");
            assert_eq!(new.len(), 7, "{unit}/{name} {key}");
            assert_eq!(new[0], old[0], "{unit}/{name} {key}: the tolerance moved");
            for i in 1..7 {
                if new[i] == old[i] {
                    continue;
                }
                bounds += 1;
                if ONE_CIRCLE.contains(&name) {
                    // vertices only removed or moved within the tolerance: the box of the
                    // rest is inside main's grown by it (min bounds 1..4, max bounds 4..7)
                    let inside = if i < 4 {
                        new[i] >= old[i] - new[0]
                    } else {
                        new[i] <= old[i] + new[0]
                    };
                    if !inside {
                        bad.push(format!(
                            "{unit}/{name} {key}[{i}]: {} -> {} outside main's box grown by {:e}",
                            old[i], new[i], new[0]
                        ));
                    }
                    continue;
                }
                let moved = (new[i] - old[i]).abs();
                if !(moved <= new[0]) {
                    bad.push(format!(
                        "{unit}/{name} {key}[{i}]: {} -> {} moved {moved:e}, past the tolerance {:e}",
                        old[i], new[i], new[0]
                    ));
                }
            }
        }
        if let Some(&(_, n, main)) = MAIN_SHELL_MASS.iter().find(|m| m.0 == name) {
            let (area, volume) = closed_forms(n);
            let new = out.values.get("shell_mass_properties").expect("shell mass");
            // the gate is the closed form within 1e-9; whether each moved closer or
            // farther is reported only (a move of about 1e-10, not a proven direction)
            for (i, exact) in [area, volume, volume].into_iter().enumerate() {
                // the closed form's own rounding: a few ulps of a value near 3500
                let slack = 1e-12;
                if !((new[i] - exact).abs() <= 1e-9 * exact) {
                    bad.push(format!(
                        "{name} shell_mass_properties[{i}]: {} not within 1e-9 of {exact}",
                        new[i]
                    ));
                }
                let class = closer(main[i], new[i], exact, slack);
                eprintln!(
                    "WR545 {name} shell_mass_properties[{i}]: main {:e} off, now {:e} off: {class:?}",
                    main[i] - exact,
                    new[i] - exact
                );
            }
        }
    }
    eprintln!(
        "WR545 SUMMARY: {} rows, {exact_edges} exact edges certified, {bounds} moved bounds within tolerance",
        ROWS.len()
    );
    assert!(
        bad.is_empty(),
        "{} failures:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
