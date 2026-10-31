//! tangency-labs/solidcore#1981 (WR-654): chamfering the four rim edges of a pocket (box
//! (0,0,0) 60 x 40 x 10 minus box (15,10,5) 30 x 20 x 6, so the pocket is 5 deep) with
//! leg 5.5, straight on the Boolean's direct body, fails with error code 3 "Fillet Corner
//! Error: Corner Edge Computation Failure" when `keep_operand_uv_trims` (WR-361) is on
//! together with the fillet fixes SolidCraft runs with (`fillet_refuse_rails_leaving_faces`,
//! `fillet_trace_reaches_rails`, `fillet_rail_end_on_continued_surface`,
//! `fillet_cliff_overflow`, `fillet_mitre_concave_face_corners`).
//!
//! The Boolean's copy of the cut box's trims is exact; the cut box's faces at x = 15 and
//! y = 30 carry the exact degree-1 B-spline trims `SwapUV` mirrors onto them, the other
//! walls lines. WR-610's run-on (`fillet_trace_reaches_rails`) widens a side edge's
//! parameter interval past its ends, but a B-spline trim clamps the parameter to its own
//! interval, so one wall's rail met the pocket's vertical edge past the floor and the
//! other's not at all, and the bevel vertex split. `fillet_run_on_bspline_trims` lets the
//! widened B-spline trim run on along its line, as a line trim does.
//!
//! The expected volume is the fork's closed form for mitred chamfers of leg `d` round a
//! 30 x 20 rim: `21000 - (50 d^2 + 4/3 d^3)`, 19265.666666667 at `d = 5.5`.
//!
//! The same gap shows without a Boolean: round r 10 on the long edge (0,5) of a 5 x 5 x 30
//! box, whose side edges mix those B-spline trims with lines, fails with code 4, where the
//! other three long edges, its mirror image (5,0) among them, round.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::primitive::api_primitives::create_box;
use solidcore::topology::{edge, vertex, EdgeId};
use solidcore::{Brep, Kernel, Point3};

const D: f64 = 5.5;

fn expected() -> f64 {
    21000.0 - (50.0 * D * D + 4.0 / 3.0 * D * D * D)
}

/// SolidCraft's fillet flag set, with `keep_operand_uv_trims` and the WR-654 fix as given.
fn kernel(keep_operand_uv_trims: bool, run_on_bspline_trims: bool) -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.fillet_cliff_overflow = true;
    k.fixes.fillet_mitre_concave_face_corners = true;
    k.fixes.keep_operand_uv_trims = keep_operand_uv_trims;
    k.fixes.fillet_run_on_bspline_trims = run_on_bspline_trims;
    k
}

fn ends(b: &Brep, e: EdgeId) -> (Point3, Point3) {
    let (s, t) = edge::start_end_vertices(b, e).unwrap();
    (
        vertex::point(b, s.unwrap()).unwrap(),
        vertex::point(b, t.unwrap()).unwrap(),
    )
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    b.refresh_cached_topology();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-8, &attrs).unwrap()
}

fn pocket(k: &mut Kernel) -> Brep {
    let p = Point3::new;
    let plate = create_box(k, Point3::ZERO, 60.0, 40.0, 10.0).unwrap();
    let cut = create_box(k, p(15.0, 10.0, 5.0), 30.0, 20.0, 6.0).unwrap();
    boolean_difference(k, plate, cut).unwrap()
}

fn rim_edges(b: &Brep) -> Vec<EdgeId> {
    let p = Point3::new;
    let c = [
        p(15.0, 10.0, 10.0),
        p(45.0, 10.0, 10.0),
        p(45.0, 30.0, 10.0),
        p(15.0, 30.0, 10.0),
    ];
    let on = |x: Point3, a: Point3, q: Point3| x.distance(a) < 1e-9 || x.distance(q) < 1e-9;
    (0..4)
        .map(|i| {
            let (a, q) = (c[i], c[(i + 1) % 4]);
            b.edges()
                .ids()
                .find(|&e| {
                    let (s, t) = ends(b, e);
                    on(s, a, q) && on(t, a, q) && s.distance(t) > 1e-9
                })
                .unwrap()
        })
        .collect()
}

/// Chamfers the rim; `Err` carries the fillet session's error code and comment.
fn chamfer_rim(
    keep_operand_uv_trims: bool,
    run_on_bspline_trims: bool,
) -> Result<f64, (i32, String)> {
    let mut k = kernel(keep_operand_uv_trims, run_on_bspline_trims);
    let mut b = pocket(&mut k);
    let ids = rim_edges(&b);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &ids, D, 0, 1, 1.0)
        .unwrap();
    if s.do_fillet(&mut k).is_err() {
        let info = s.error_info();
        return Err((info.code, info.comment.clone()));
    }
    drop(s);
    Ok(volume(&k, &mut b))
}

/// Rounds the long edge at (`x`, `y`) of a 5 x 5 x 30 box with radius `r`.
fn round_bar_edge(x: f64, y: f64, r: f64, run_on_bspline_trims: bool) -> Result<f64, i32> {
    let mut k = kernel(false, run_on_bspline_trims);
    let mut b = create_box(&mut k, Point3::ZERO, 5.0, 5.0, 30.0).unwrap();
    let e = b
        .edges()
        .ids()
        .find(|&e| {
            let (s, t) = ends(&b, e);
            s.x == x && s.y == y && t.x == x && t.y == y
        })
        .unwrap();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    s.prepare_constant_radius_edges(&mut k, &[e], r, 1, 1, 1.0)
        .unwrap();
    if s.do_fillet(&mut k).is_err() {
        return Err(s.error_info().code);
    }
    drop(s);
    Ok(volume(&k, &mut b))
}

#[test]
fn pocket_chamfer_on_a_boolean_body() {
    let v = chamfer_rim(true, true).unwrap_or_else(|(code, comment)| panic!("{code} {comment}"));
    let want = expected();
    eprintln!("pocket chamfer, operand trims kept: {v:.12} (want {want:.12})");
    assert!((v - want).abs() / want < 1e-8, "{v} vs {want}");
}

/// Without the WR-654 fix, the issue's flag set still fails as reported.
#[test]
fn pocket_chamfer_without_the_fix_fails_at_the_corner_edges() {
    let (code, comment) = chamfer_rim(true, false).expect_err("fails without the fix");
    assert_eq!(code, 3, "{comment}");
}

/// With `keep_operand_uv_trims` off the pocket's walls carry line trims only, so the fix
/// changes nothing: both give the expected volume, bit for bit the same.
#[test]
fn pocket_chamfer_without_operand_trims() {
    let want = expected();
    let off =
        chamfer_rim(false, false).unwrap_or_else(|(code, comment)| panic!("{code} {comment}"));
    assert!((off - want).abs() / want < 1e-8, "{off} vs {want}");
    let fixed =
        chamfer_rim(false, true).unwrap_or_else(|(code, comment)| panic!("{code} {comment}"));
    eprintln!("pocket chamfer, no operand trims: {off} (with the fix {fixed})");
    assert_eq!(fixed.to_bits(), off.to_bits(), "{fixed} vs {off}");
}

/// The 5 x 5 x 30 bar after a round of radius `r` (between 5 and 10) on one long edge: 30
/// times the part of its 5 x 5 section inside the circle of radius `r` tangent to both
/// faces, `int_x0^5 (5 - r + sqrt(r^2 - (x - r)^2)) dx` from `x0 = r - sqrt(r^2 - (r - 5)^2)`.
fn rounded_bar_volume(r: f64) -> f64 {
    let a = 5.0;
    let f = |u: f64| (u * (r * r - u * u).sqrt() + r * r * (u / r).asin()) / 2.0;
    let x0 = r - (r * r - (r - a) * (r - a)).sqrt();
    30.0 * ((a - r) * (a - x0) + f(a - r) - f(x0 - r))
}

/// The same gap on a box primitive, no Boolean: its long edge (0,5), whose side edges
/// carry `SwapUV`'s B-spline trims, fails (code 4) without the fix; with it, it rounds to
/// the closed-form volume, as its mirror image (5,0) does without it.
#[test]
fn bar_edge_with_bspline_side_trims_rounds() {
    let r = 10.0;
    let want = rounded_bar_volume(r);
    assert_eq!(round_bar_edge(0.0, 5.0, r, false), Err(4));
    let mirror = round_bar_edge(5.0, 0.0, r, false).unwrap_or_else(|code| panic!("{code}"));
    let v = round_bar_edge(0.0, 5.0, r, true).unwrap_or_else(|code| panic!("{code}"));
    eprintln!("bar edge (0,5) r {r}: {v} (mirror image (5,0): {mirror}; want {want})");
    assert!((mirror - want).abs() / want < 1e-9, "{mirror} vs {want}");
    assert!((v - want).abs() / want < 1e-9, "{v} vs {want}");
}
