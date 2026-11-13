//! #2878: SolidCraft's oracle 81-fillet-spline-radius, a 40 x 30 x 20 box whose top front edge
//! is rounded by a clamped cubic radius law (2, 5, 1.5, 4, 1, 3 from x = 0), and its action
//! matrix's draft of the front face's neighbours (the bottom and both ends, 2 degrees, the
//! front's plane y = 0 neutral, pulled along -y). It built at 8ddce9016 and was refused once
//! the exact extension replaced the fitted one: the round's rational B-spline face, extended
//! Ruled past its end, could not reach the local operation's margin. In its own test binary:
//! SolidCraft's fixes are process-wide (`session::set_fixes`).

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::disallowed_methods
)] // test, its closed forms independent of the kernel

mod common;

use std::f64::consts::PI;

use common::{edge_at, vertex_at};
use solidcore_ops::{draft, fillet_variable, make_box, session, Body, CheckLevel};
use solidcore_record::brep::Brep as Record;

/// SolidCraft's fixes.
fn solidcraft_fixes() {
    let fixes: Vec<String> = serde_json::from_str(include_str!(
        "../../solidcore/tests/kernel_cases/fixes.json"
    ))
    .unwrap();
    session::set_thread_fixes(fixes.iter().map(String::as_str)).unwrap();
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1.0)
}

/// The record index of the face all of whose edges' vertices have coordinate `axis` at `at`.
fn flat_face(r: &Record, axis: usize, at: f64) -> usize {
    (0..r.faces.len())
        .find(|&f| {
            r.edges
                .iter()
                .filter(|e| e.uses.iter().any(|u| u.face as usize == f))
                .flat_map(|e| e.vertices)
                .all(|v| (r.vertices[v as usize].point[axis] - at).abs() < 1e-9)
        })
        .unwrap()
}

/// The matrix's draft: the neutral plane y = 0 through its point, pulled along -y, 2 degrees,
/// of the bottom (z = 0) and the ends (x = 0, x = 40).
fn draft_bottom_and_ends(b: &Body) -> solidcore_ops::Result<()> {
    let (r, o) = b.to_record().unwrap();
    let faces = [
        flat_face(&r, 2, 0.0),
        flat_face(&r, 0, 0.0),
        flat_face(&r, 0, 40.0),
    ];
    let n = [0.0, -1.0, 0.0];
    let origin = [34.384_992_774_737_924, 0.0, 1.496_356_209_198_337];
    draft(b, &o, &faces, origin, n, n, 2.0_f64.to_radians())
}

/// The box with a round on its top front edge by the clamped cubic law of control radii
/// `radii` from x = 0 (knots 0, 0, 0, 0, 0.25, 0.6, 1, 1, 1, 1).
fn rounded(radii: &[f64; 6]) -> Body {
    let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
    let edge = edge_at(&b, [20.0, 0.0, 20.0]);
    let start = vertex_at(&b, [0.0, 0.0, 20.0]);
    let knots = [0.0, 0.0, 0.0, 0.0, 0.25, 0.6, 1.0, 1.0, 1.0, 1.0];
    fillet_variable(&b, edge, start, 3, &knots, radii).unwrap();
    b
}

/// t = tan 2 degrees; the draft leans the faces out (the part lies against the pull from
/// the neutral plane), so each drafted face moves out by y t: the plain box becomes
/// (40 + 2 y t)(20 + y t) at each y, 24000 + 36000 t + 18000 t^2 in all.
fn grown(t: f64) -> f64 {
    24000.0 + 36000.0 * t + 18000.0 * t * t
}

/// What the round's continuation past an end of radius r takes from that end's added
/// wedge, x within y t past the end, where the continuation is the end's quarter-circle
/// section carried straight along the edge: the corner square less the quarter disc,
/// `t * integral_0^r y (r - sqrt(r^2 - (r - y)^2)) dy = t r^3 (5/6 - pi/4)`.
fn end_take(t: f64, r: f64) -> f64 {
    t * r * r * r * (5.0 / 6.0 - PI / 4.0)
}

/// The oracle's round (control radii 2, 5, 1.5, 4, 1, 3): the draft builds and the body is
/// valid; its volume grows by the plain box's wedges less what the round's continuation
/// takes of them at its ends. That take has no exact form here: where the radius changes
/// along the edge the round's characteristic circles (where each ball touches the
/// envelope) are not planar, leaning along the edge by up to r r' (sqrt 2 - 1), about
/// 0.75 mm at x = 0 (r 2, slope 0.9), so near an end the round comes from balls the law
/// does not define and its continuation is not the end's section carried straight. The
/// take is held between 0 and twice the straight section's at each end (about 0.12 mm^3
/// against wedges of about 1279 mm^3).
#[test]
fn the_oracle_spline_radius_round_box_drafts_its_bottom_and_ends() {
    solidcraft_fixes();
    let t = 2.0_f64.to_radians().tan();
    let plain = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
    draft_bottom_and_ends(&plain).unwrap();
    assert!(rel(plain.volume(1e-10).unwrap(), grown(t)) < 1e-9);

    let b = rounded(&[2.0, 5.0, 1.5, 4.0, 1.0, 3.0]);
    let before = b.volume(1e-10).unwrap();
    draft_bottom_and_ends(&b).unwrap();
    let after = b.volume(1e-10).unwrap();
    let wedges = grown(t) - 24000.0;
    let most = 2.0 * (end_take(t, 2.0) + end_take(t, 3.0));
    let gain = after - before;
    assert!(
        gain <= wedges && gain >= wedges - most,
        "gain {gain}, wedges {wedges}, ends' take at most {most}"
    );
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
}

/// A law flat at both ends (control radii 2, 2, 5, 1.5, 3, 3: r' = 0 at x = 0 and 40): the
/// end sections are the planar quarter circles of radius 2 and 3 and the round's
/// continuation past each end is that section carried along the edge: the law's curvature
/// moves it by at most r'' d^2 / 2 over the d <= r t (0.1 mm) of an end's take, about 1e-3
/// of that take (8e-10 of the volume), so the change is closed form to 2e-9: the wedges
/// less the ends' take.
#[test]
fn a_flat_ended_spline_radius_round_box_drafts_by_its_closed_form() {
    solidcraft_fixes();
    let t = 2.0_f64.to_radians().tan();
    let b = rounded(&[2.0, 2.0, 5.0, 1.5, 3.0, 3.0]);
    let before = b.volume(1e-10).unwrap();
    draft_bottom_and_ends(&b).unwrap();
    let after = b.volume(1e-10).unwrap();
    let expected = before + (grown(t) - 24000.0) - end_take(t, 2.0) - end_take(t, 3.0);
    assert!(rel(after, expected) < 2e-9, "{after} against {expected}");
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
}
