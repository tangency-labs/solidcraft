//! Weld beads on the generic blend (#15129, the welds epic #15132): the cross-section between the
//! rails is a circular arc of sagitta `h` over the chord (h > 0 a convex bead, h < 0 a concave
//! one, h = 0 the chord itself, a chamfer). The rails sit at the setbacks, as a chamfer's do.
//!
//! In an inside corner a blend adds material, so a bead's section area is the body's volume
//! gained per length of edge. In the plane square to the edge, faces meeting at the wedge angle
//! `alpha` (measured in the air between them) with setbacks `d` on both give
//!
//! - the chord `c = 2 d sin(alpha / 2)` and the triangle `d^2 sin(alpha) / 2` between the edge
//!   and the two feet,
//! - for `h != 0` the circle through both feet at height `|h|` over the chord's middle, of
//!   radius `R = (c^2 / 4 + h^2) / (2 |h|)`, and the circular segment between the arc and the
//!   chord, of area `R^2 phi - (c / 2)(R - |h|)` with `phi = asin(c / (2 R))` its half angle
//!   (`|h| <= c / 2`),
//! - the section: the triangle plus the segment (convex) or less it (concave).
//!
//! This first part checks the bodies and the closed forms on the chord (h = 0, an equal chamfer
//! today) and the segment's formula on its own. The bead's own cases follow with the section.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::weld_bead::{beads_apart, span_lengths};
use solidcore::curve::curve::Curve;
use solidcore::error::blend::{BlendFailure, WeldGap};
use solidcore::error::{ErrorKind, Reason, SolidError};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::bead::{
    equal_legs_for_throat, BeadArc, BeadDefinition, BeadSpan,
};
use solidcore::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use solidcore::surface::procedural_blend::convert::exact_to_record;
use solidcore::surface::procedural_blend::exact::ExactClass;
use solidcore::surface::procedural_blend::BlendRef;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p7::copy_faces;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::edge;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::EntityRef;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, PI};

/// The setbacks of every case (mm).
const SETBACK: f64 = 5.0;
/// How far the profiles are swept: the corner edge's length (mm).
const LENGTH: f64 = 100.0;
/// The arms' thickness (mm).
const ARM: f64 = 10.0;

/// Every fix on but `geometric_g2_continuity`, as ops runs.
fn kernel() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    let mut f = UpstreamFixes::default();
    for &name in UpstreamFixes::NAMES {
        if name != "geometric_g2_continuity" {
            *f.flag_mut(name).unwrap() = true;
        }
    }
    k.fixes = f;
    k
}

/// The circular segment between a chord `c` and its arc at height `h` over the chord's middle.
fn segment_area(c: f64, h: f64) -> f64 {
    let h = h.abs();
    if h == 0.0 {
        return 0.0;
    }
    let r = (c * c / 4.0 + h * h) / (2.0 * h);
    let phi = (c / (2.0 * r)).asin();
    r * r * phi - c / 2.0 * (r - h)
}

/// The bead's section area for setbacks `d` at the wedge angle `alpha` with sagitta `h`.
fn section_area(d: f64, alpha: f64, h: f64) -> f64 {
    let c = 2.0 * d * (alpha / 2.0).sin();
    let triangle = 0.5 * d * d * alpha.sin();
    triangle + h.signum() * segment_area(c, h)
}

/// An inside corner of `alpha` (radians, the air's angle between the faces) at the origin, swept
/// `LENGTH` along z: one arm's face runs along +x, the other's along the ray at `alpha` from it,
/// each arm `ARM` thick on the material's side. The body and a point on the corner's edge.
fn corner(k: &mut Kernel, alpha: f64) -> (Brep, [f64; 3]) {
    let (s, c) = alpha.sin_cos();
    let reach = 40.0;
    // The second face's direction u and the normal n on its material side.
    let u = (c, s);
    let n = (-s, c);
    // Where the two arms' outer faces meet: y = -ARM and the line ARM along n from the second
    // face, (x, y) = ARM n + t u.
    let t = (-ARM - ARM * n.1) / u.1;
    let outer = (ARM * n.0 + t * u.0, -ARM);
    // Counter-clockwise, as the other prisms here are swept.
    let corners = [
        (0.0, 0.0),
        (reach, 0.0),
        (reach, -ARM),
        outer,
        (reach * u.0 + ARM * n.0, reach * u.1 + ARM * n.1),
        (reach * u.0, reach * u.1),
    ];
    let corners: Vec<(f64, f64)> = corners.iter().rev().copied().collect();
    let line = |a: (f64, f64), b: (f64, f64)| {
        Curve::from(
            create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap(),
        )
    };
    let curves: Vec<Curve> = (0..corners.len())
        .map(|i| line(corners[i], corners[(i + 1) % corners.len()]))
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let b = create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), LENGTH, true).unwrap();
    (b, [0.0, 0.0, LENGTH / 2.0])
}

fn pick(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    volume_to(k, b, 1e-9)
}

/// The volume of `b` to the relative accuracy `accuracy`: coarser for the fitted beads round
/// tubes, whose integration at 1e-9 takes seconds each.
fn volume_to(k: &Kernel, b: &mut Brep, accuracy: f64) -> f64 {
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, accuracy, &attrs).unwrap()
}

/// The corner with an equal chamfer of `SETBACK` along its edge, and the volume it gained.
fn chord_corner(alpha: f64) -> (Kernel, Brep, f64) {
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, alpha);
    let before = volume(&k, &mut b);
    let edge = pick(&k, &b, at);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_chamfers_along(&k, &[edge], &[ChamferSize::Distances([SETBACK; 2])], &[])
            .unwrap();
        s.create_corners(&k, None).unwrap();
        s.do_filleting(&mut k, None).unwrap();
    }
    let gain = volume(&k, &mut b) - before;
    (k, b, gain)
}

/// The volume the corner gains from an equal chamfer of `SETBACK` along its edge.
fn chord_gain(alpha: f64) -> f64 {
    chord_corner(alpha).2
}

/// `b` as an ASCII `.smb` file, as `solidcore-diag` reads it.
fn smb(k: &Kernel, b: &Brep) -> Vec<u8> {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &|e| b.owner_identity(e),
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, true).unwrap()
}

/// The chord (h = 0) in a square inside corner: the triangle `d^2 / 2` along the edge.
#[test]
fn the_chord_in_a_square_inside_corner_adds_its_triangle() {
    let alpha = FRAC_PI_2;
    let want = section_area(SETBACK, alpha, 0.0) * LENGTH;
    assert!((want - 1250.0).abs() < 1e-9, "{want}");
    let got = chord_gain(alpha);
    assert!(
        ((got - want) / want).abs() <= 1e-6,
        "volume gained {got} against {want}"
    );
}

/// The chord in a 60 degree inside corner: `d^2 sin(60°) / 2` along the edge.
#[test]
fn the_chord_in_a_sixty_degree_inside_corner_adds_its_triangle() {
    let alpha = FRAC_PI_3;
    let want = section_area(SETBACK, alpha, 0.0) * LENGTH;
    assert!((want - 1_082.531_754_730_548_3).abs() < 1e-9, "{want}");
    let got = chord_gain(alpha);
    assert!(
        ((got - want) / want).abs() <= 1e-6,
        "volume gained {got} against {want}"
    );
}

/// The segment's closed form against the arc integrated directly (Simpson, 2000 panels), and the
/// section areas the bead cases will be gated on.
#[test]
fn the_segment_and_section_closed_forms_hold() {
    // h = c / 2 is the half circle, pi (c / 2)^2 / 2 (where its ends stand upright the sum below
    // converges too slowly to check it).
    let half = segment_area(7.0, 3.5);
    let want = PI * 3.5 * 3.5 / 2.0;
    assert!(
        ((half - want) / want).abs() < 1e-12,
        "{half} against {want}"
    );
    for (c, h) in [(7.0, 1.0), (5.0, 1.0), (7.0, 2.5), (2.0, 0.1)] {
        let r = (c * c / 4.0 + h * h) / (2.0 * h);
        // The arc's height over the chord at x from its middle.
        let height = |x: f64| (r * r - x * x).sqrt() - (r - h);
        let n = 2000;
        let step = c / f64::from(n);
        let mut sum = height(-c / 2.0) + height(c / 2.0);
        for i in 1..n {
            let x = -c / 2.0 + step * f64::from(i);
            sum += if i % 2 == 1 { 4.0 } else { 2.0 } * height(x);
        }
        let integrated = sum * step / 3.0;
        let closed = segment_area(c, h);
        assert!(
            ((closed - integrated) / closed).abs() < 1e-9,
            "c {c} h {h}: {closed} against {integrated}"
        );
    }
    let (right, sixty) = (FRAC_PI_2, FRAC_PI_3);
    for (alpha, h, want) in [
        (right, 1.0, 17.2886),
        (right, -1.0, 7.7114),
        (sixty, 1.0, 14.2630),
        (sixty, -1.0, 7.3876),
    ] {
        let got = section_area(SETBACK, alpha, h);
        assert!(
            (got - want).abs() < 5e-5,
            "alpha {alpha} h {h}: {got} against {want}"
        );
    }
}

/// The built corners as `.smb` files in the test build's scratch folder (`bin/tmp`), for
/// pictures: `cargo test -p solidcore --test blend_bead_section -- --ignored`, then
/// `cargo run -p diagnostics --release -- bin/tmp/<name>.smb --out <dir>` for each.
#[test]
#[ignore = "writes the built corners as .smb files for pictures; checks nothing"]
fn write_the_corners_for_pictures() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    for (name, alpha) in [
        ("bead_corner_90_chord", FRAC_PI_2),
        ("bead_corner_60_chord", FRAC_PI_3),
    ] {
        let (k, b, _) = chord_corner(alpha);
        let path = dir.join(format!("{name}.smb"));
        std::fs::write(&path, smb(&k, &b)).unwrap();
        println!("wrote {}", path.display());
    }
}

// ---- the bead's section on its own (`surface::procedural_blend::bead`) ----

fn bead(d: [f64; 2], alpha: f64, h: f64) -> BeadDefinition {
    BeadDefinition {
        chamfer: ChamferDefinition::new(ChamferSize::Distances(d), alpha).unwrap(),
        sagitta: h,
    }
}

/// Equal setbacks of 5: the section areas the weld tests are gated on (closed forms
/// worked by hand above).
#[test]
fn section_areas_match_the_closed_forms() {
    for (alpha, h, want) in [
        (FRAC_PI_2, 1.0, 17.288_6),
        (FRAC_PI_2, -1.0, 7.711_4),
        (FRAC_PI_3, 1.0, 14.263_0),
        (FRAC_PI_3, -1.0, 7.387_6),
    ] {
        let got = bead([5.0; 2], alpha, h).section_area();
        assert!((got - want).abs() < 5e-5, "{alpha} {h}: {got}");
    }
}

/// Equal setbacks: the toes' angles are `(pi - alpha) / 2`, so the bounds are
/// `(c / 2) tan((pi -+ alpha) / 4)`; at 90 degrees and 5 mm, 1.4645 and 8.5355.
#[test]
fn equal_setbacks_bound_both_sides() {
    let b = bead([5.0; 2], FRAC_PI_2, 1.0);
    let half = 5.0 * (FRAC_PI_2 / 2.0).sin();
    assert!((b.bound(-1.0) - half * (PI / 8.0).tan()).abs() < 1e-12);
    assert!((b.bound(1.0) - half * (3.0 * PI / 8.0).tan()).abs() < 1e-12);
    assert!((b.bound(-1.0) - 1.464_466).abs() < 1e-6);
    assert!((b.bound(1.0) - 8.535_534).abs() < 1e-6);
    assert!(bead([5.0; 2], FRAC_PI_2, -1.46).check().is_ok());
    assert_eq!(
        bead([5.0; 2], FRAC_PI_2, -1.47).check().unwrap_err().bound,
        b.bound(-1.0)
    );
    // past h = c / 2 the convex arc overhangs its toes, and still builds
    assert!(bead([5.0; 2], FRAC_PI_2, 5.0).check().is_ok());
    assert!(bead([5.0; 2], FRAC_PI_2, 8.6).check().is_err());
}

/// Unequal setbacks: the steeper toe bounds a concave bead, the flatter a convex one.
#[test]
fn unequal_setbacks_take_the_toes_that_bind() {
    let b = bead([3.0, 6.0], FRAC_PI_2, 1.0);
    let [b1, b2] = b.toe_angles();
    assert!((b1 + b2 - FRAC_PI_2).abs() < 1e-12);
    // the toe on support 1 (setback 3) sees the far foot 6 up: atan(6 / 3)
    assert!((b1 - 2.0_f64.atan()).abs() < 1e-12);
    let half = 45.0_f64.sqrt() / 2.0;
    assert!((b.bound(-1.0) - half * (b2 / 2.0).tan()).abs() < 1e-12);
    assert!((b.bound(1.0) - half * ((PI - b1) / 2.0).tan()).abs() < 1e-12);
}

#[test]
fn a_sagitta_not_finite_or_past_its_bound_is_refused() {
    let c = ChamferDefinition::new(ChamferSize::Distances([5.0; 2]), FRAC_PI_2).unwrap();
    for h in [f64::NAN, f64::INFINITY, -2.0, 9.0] {
        assert!(BeadDefinition::new(c, h).is_err(), "{h}");
    }
    // 0 is the flat bead, the chord itself
    for h in [0.0, 1.0] {
        assert!(BeadDefinition::new(c, h).is_ok(), "{h}");
    }
}

/// The arc runs from foot to foot through the apex at `h` over the chord's middle, on the
/// convex side for `h > 0` and the edge's side for `h < 0`, and every point is `R` from the
/// centre.
#[test]
fn the_arc_runs_from_foot_to_foot_through_its_apex() {
    let f1 = Point3::new(5.0, 0.0, 0.0);
    let f2 = Point3::new(0.0, 5.0, 0.0);
    let along = Vec3::new(0.0, 0.0, 1.0);
    let away = Vec3::new(1.0, 1.0, 0.0);
    for h in [1.0, -1.0, 3.535_533_905_932_737_6, 5.0] {
        let arc = BeadArc::new(f1, f2, along, away, h).unwrap();
        assert!(arc.point(0.0).distance(f1) < 1e-12, "{h}");
        assert!(arc.point(arc.turn).distance(f2) < 1e-12, "{h}");
        let apex = arc.point(arc.turn / 2.0);
        let want = Point3::new(2.5, 2.5, 0.0) + away * (h / 2.0_f64.sqrt());
        assert!(apex.distance(want) < 1e-12, "{h}: {apex:?}");
        for i in 0..=8 {
            let p = arc.point(arc.turn * f64::from(i) / 8.0);
            assert!((p.distance(arc.centre) - arc.radius).abs() < 1e-12);
            assert!(p.z.abs() < 1e-12);
        }
    }
}

// ---- the bead as a body of its own ----
//
// Between planes along a straight edge the bead is a prism: its section (the edge, the two
// feet and the arc between them) swept along the edge. It is a body of its own beside the
// part: the part is not changed.

/// The bead of setbacks `SETBACK` and sagitta `h` in the corner of `alpha` (`corner`'s): its
/// sizes and its section's outline at height `z0`, the edge first, counter-clockwise.
fn bead_section(alpha: f64, h: f64, z0: f64) -> (BeadDefinition, Vec<Curve>) {
    let bead = BeadDefinition::new(
        ChamferDefinition::new(ChamferSize::Distances([SETBACK; 2]), alpha).unwrap(),
        h,
    )
    .unwrap();
    let (s, c) = alpha.sin_cos();
    let edge = Point3::new(0.0, 0.0, z0);
    let f1 = Point3::new(SETBACK, 0.0, z0);
    let f2 = Point3::new(SETBACK * c, SETBACK * s, z0);
    let middle = Point3::new((f1.x + f2.x) / 2.0, (f1.y + f2.y) / 2.0, z0);
    let z = Vec3::new(0.0, 0.0, 1.0);
    let arc = BeadArc::new(f1, f2, z, middle - edge, h).unwrap();
    // `create_arc` turns counter-clockwise about z: from f1 where the arc does, else from f2
    let start = if arc.x.cross(arc.y).dot(z) > 0.0 {
        f1 - arc.centre
    } else {
        f2 - arc.centre
    };
    let from = start.y.atan2(start.x).to_degrees();
    let line = |a: Point3, b: Point3| Curve::from(create_line_segment(a, b).unwrap());
    let curves = vec![
        line(edge, f1),
        Curve::from(
            create_arc(arc.centre, arc.radius, from, from + arc.turn.to_degrees()).unwrap(),
        ),
        line(f2, edge),
    ];
    (bead, curves)
}

/// The bead's bodies, one per span: each its section swept along its share of the edge, from
/// the plane square to the edge at its start to the one at its end (its caps).
fn bead_bodies(
    k: &mut Kernel,
    alpha: f64,
    h: f64,
    spans: &[BeadSpan],
) -> (BeadDefinition, Vec<Brep>) {
    let mut bodies = Vec::new();
    let mut bead = None;
    for span in spans {
        let (b, curves) = bead_section(alpha, h, span.from * LENGTH);
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        bodies.push(
            create_linear_sweep(
                k,
                &refs,
                Vec3::new(0.0, 0.0, 1.0),
                span.share() * LENGTH,
                true,
            )
            .unwrap(),
        );
        bead = Some(b);
    }
    (bead.unwrap(), bodies)
}

/// The bead's body along the whole edge.
fn bead_body(k: &mut Kernel, alpha: f64, h: f64) -> (BeadDefinition, Brep) {
    let (bead, mut bodies) = bead_bodies(k, alpha, h, &[BeadSpan::WHOLE]);
    (bead, bodies.remove(0))
}

/// The bead's body has the section's area along the whole edge: convex and concave beads at 90
/// and 60 degrees, against the closed forms (17.2886, 7.7114, 14.2630, 7.3876 mm^2).
#[test]
fn a_bead_body_is_its_section_along_the_edge() {
    for (alpha, h, area) in [
        (FRAC_PI_2, 1.0, 17.288_6),
        (FRAC_PI_2, -1.0, 7.711_4),
        (FRAC_PI_3, 1.0, 14.263_0),
        (FRAC_PI_3, -1.0, 7.387_6),
    ] {
        let mut k = kernel();
        let (bead, mut b) = bead_body(&mut k, alpha, h);
        let v = volume(&k, &mut b);
        let want = bead.section_area() * LENGTH;
        assert!(
            ((v - want) / want).abs() <= 1e-6,
            "alpha {alpha} h {h}: {v} against {want}"
        );
        assert!(
            (want - area * LENGTH).abs() < 5e-3,
            "alpha {alpha} h {h}: {want}"
        );
    }
}

/// The part with `bead`'s faces copied beside it, unstitched: one file, two bodies, for a
/// picture.
fn beside(k: &mut Kernel, part: &mut Brep, bead: &mut Brep) {
    let faces = bp1::faces(bead, None).unwrap();
    let mut state = ArenaNotifyState::new(part);
    state.seed(bead);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f| key_of(EntityRef::Face(f));
    copy_faces(k, bead, &faces, part, false, &this_face, &mut env).unwrap();
}

/// The corners with their beads beside them as `.smb` files in the test build's scratch folder
/// (`bin/tmp`), for pictures: `cargo test -p solidcore --test blend_bead_section --
/// --ignored`, then `bin/release/solidcore-diag bin/tmp/<name>.smb --out <dir>`.
#[test]
#[ignore = "writes the corners and their beads as .smb files for pictures; checks nothing"]
fn write_the_beads_for_pictures() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    for (name, alpha, h) in [
        ("bead_90_convex", FRAC_PI_2, 1.0),
        ("bead_90_concave", FRAC_PI_2, -1.0),
        ("bead_60_convex", FRAC_PI_3, 1.0),
        ("bead_60_concave", FRAC_PI_3, -1.0),
    ] {
        let mut k = kernel();
        let (mut part, _) = corner(&mut k, alpha);
        let (_, mut bead) = bead_body(&mut k, alpha, h);
        beside(&mut k, &mut part, &mut bead);
        let path = dir.join(format!("{name}.smb"));
        std::fs::write(&path, smb(&k, &part)).unwrap();
        println!("wrote {}", path.display());
    }
}

// ---- spans: where along the edge the bead runs ----

/// A bead from 0.3 to 0.7 of the 100 mm edge is 40 mm long: its section's area times 40, and
/// its caps are the planes square to the edge at 30 and 70 mm.
#[test]
fn a_bead_over_part_of_the_edge_is_its_section_along_that_part() {
    let mut k = kernel();
    let span = BeadSpan::new(0.3, 0.7).unwrap();
    let (bead, mut bodies) = bead_bodies(&mut k, FRAC_PI_2, 1.0, &[span]);
    let v = volume(&k, &mut bodies[0]);
    let want = bead.section_area() * 40.0;
    assert!(((v - want) / want).abs() <= 1e-6, "{v} against {want}");
    assert!((want - 691.545).abs() < 1e-3, "{want}");
}

/// A bead broken into three pieces along the edge (an intermittent weld, 3 x 20 mm at a pitch of
/// 35 mm from 10 mm): three bodies of 20 mm each, 60 mm of bead in all.
#[test]
fn a_bead_broken_along_the_edge_is_one_body_per_span() {
    let mut k = kernel();
    let spans = BeadSpan::along(&[(0.10, 0.30), (0.45, 0.65), (0.80, 1.00)]).unwrap();
    let (bead, bodies) = bead_bodies(&mut k, FRAC_PI_3, -1.0, &spans);
    assert_eq!(bodies.len(), 3);
    let mut total = 0.0;
    for mut b in bodies {
        let v = volume(&k, &mut b);
        let want = bead.section_area() * 20.0;
        assert!(((v - want) / want).abs() <= 1e-6, "{v} against {want}");
        total += v;
    }
    let want = bead.section_area() * 60.0;
    assert!(
        ((total - want) / want).abs() <= 1e-6,
        "{total} against {want}"
    );
}

/// Spans outside the edge, of no length, out of order or overlapping are refused; spans that
/// meet end to start are fine.
#[test]
fn spans_off_the_edge_or_overlapping_are_refused() {
    for (from, to) in [
        (-0.1, 0.5),
        (0.5, 1.1),
        (0.5, 0.5),
        (0.7, 0.3),
        (f64::NAN, 0.5),
    ] {
        assert!(BeadSpan::new(from, to).is_err(), "{from} {to}");
    }
    assert!(BeadSpan::along(&[]).is_err());
    assert!(BeadSpan::along(&[(0.5, 0.8), (0.1, 0.3)]).is_err());
    assert!(BeadSpan::along(&[(0.1, 0.5), (0.4, 0.8)]).is_err());
    assert_eq!(BeadSpan::along(&[(0.1, 0.5), (0.5, 0.8)]).unwrap().len(), 2);
    assert!((BeadSpan::new(0.3, 0.7).unwrap().share() - 0.4).abs() < 1e-15);
}

/// The 90 degree corner with a bead over 0.3 to 0.7 and the 60 degree corner with the bead
/// broken in three, as `.smb` files for pictures (as `write_the_beads_for_pictures`).
#[test]
#[ignore = "writes the corners with spans of bead as .smb files for pictures; checks nothing"]
fn write_the_spans_for_pictures() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    for (name, alpha, h, spans) in [
        ("bead_span_90_convex", FRAC_PI_2, 1.0, vec![(0.3, 0.7)]),
        (
            "bead_spans_60_concave",
            FRAC_PI_3,
            -1.0,
            vec![(0.10, 0.30), (0.45, 0.65), (0.80, 1.00)],
        ),
    ] {
        let mut k = kernel();
        let (mut part, _) = corner(&mut k, alpha);
        let spans = BeadSpan::along(&spans).unwrap();
        let (_, bodies) = bead_bodies(&mut k, alpha, h, &spans);
        for mut b in bodies {
            beside(&mut k, &mut part, &mut b);
        }
        let path = dir.join(format!("{name}.smb"));
        std::fs::write(&path, smb(&k, &part)).unwrap();
        println!("wrote {}", path.display());
    }
}

/// Where a convex bead starts to overhang its toe: `(c / 2) tan((pi / 2 - beta) / 2)`, 0.6699 mm
/// at 60 degrees, 1.4645 mm at 90 and 2.5 mm at 120 (5 mm setbacks).
#[test]
fn the_overhang_bound_holds_its_closed_form() {
    for (adeg, want) in [
        (60.0_f64, 0.669_872_981),
        (90.0, 1.464_466_094),
        (120.0, 2.5),
    ] {
        let (bead, _) = bead_section(adeg.to_radians(), 0.1, 0.0);
        let got = bead.overhang_bound();
        assert!((got - want).abs() < 1e-9, "{adeg}: {got}");
    }
}

/// [`beads_apart`]: the bead is a body of its own and the corner is left as it was. The bead's
/// body is its face, the strips of the corner's faces under it and its two ends, with the
/// bead's section along the edge: convex, concave, overhanging its toe, at 60, 90 and 120
/// degrees.
#[test]
fn a_bead_apart_is_its_own_body_and_leaves_the_corner() {
    for (adeg, h) in [
        (90.0_f64, 1.0),
        (90.0, -1.0),
        (90.0, 3.0),
        (60.0, 1.0),
        (60.0, -1.0),
        (120.0, 2.6),
        // flat beads: the chord's triangle swept along the edge
        (90.0, 0.0),
        (60.0, 0.0),
    ] {
        let alpha = adeg.to_radians();
        let mut k = kernel();
        let (mut b, at) = corner(&mut k, alpha);
        let before = volume(&k, &mut b);
        let faces = b.face_list().len();
        let edge = pick(&k, &b, at);
        let mut apart = beads_apart(
            &mut k,
            &mut b,
            &[edge],
            &[ChamferSize::Distances([SETBACK; 2])],
            h,
            &[],
        )
        .unwrap()
        .remove(0);
        let bead = BeadDefinition::new(
            ChamferDefinition::new(ChamferSize::Distances([SETBACK; 2]), alpha).unwrap(),
            h,
        )
        .unwrap();
        let want = bead.section_area() * LENGTH;
        let got = volume(&k, &mut apart);
        assert!(
            ((got - want) / want).abs() <= 1e-6,
            "{adeg} {h}: {got} against {want}"
        );
        assert_eq!(apart.face_list().len(), 5, "{adeg} {h}");
        assert_eq!(b.face_list().len(), faces, "{adeg} {h}: the corner changed");
        assert!(
            (volume(&k, &mut b) - before).abs() <= 1e-9 * before,
            "{adeg} {h}: the corner's volume changed"
        );
    }
}

/// A T-joint: a web 10 wide standing on a base 80 wide and 10 thick, swept `LENGTH` along z.
/// Its two inside corners are square, along the web's feet at x = -5 and x = 5 (y = 0).
fn tee(k: &mut Kernel) -> Brep {
    let corners = [
        (-40.0, -10.0),
        (40.0, -10.0),
        (40.0, 0.0),
        (5.0, 0.0),
        (5.0, 40.0),
        (-5.0, 40.0),
        (-5.0, 0.0),
        (-40.0, 0.0),
    ];
    let line = |a: (f64, f64), b: (f64, f64)| {
        Curve::from(
            create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap(),
        )
    };
    let curves: Vec<Curve> = corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(&a, &b)| line(a, b))
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), LENGTH, true).unwrap()
}

/// Beads on both sides of a T-joint ([`beads_apart`] with both inside edges): one body per bead
/// (5 faces each, the section along the edge), and the T left as it was.
#[test]
fn beads_on_both_sides_of_a_tee_are_bodies_apart() {
    for h in [1.0, -1.0, 3.0] {
        let mut k = kernel();
        let mut t = tee(&mut k);
        let before = volume(&k, &mut t);
        let faces = t.face_list().len();
        let left = pick(&k, &t, [-5.0, 0.0, LENGTH / 2.0]);
        let right = pick(&k, &t, [5.0, 0.0, LENGTH / 2.0]);
        let beads = beads_apart(
            &mut k,
            &mut t,
            &[left, right],
            &[ChamferSize::Distances([SETBACK; 2]); 2],
            h,
            &[],
        )
        .unwrap();
        assert_eq!(beads.len(), 2);
        let (bead, _) = bead_section(FRAC_PI_2, h, 0.0);
        let want = bead.section_area() * LENGTH;
        for mut one in beads {
            let got = volume(&k, &mut one);
            assert!(
                ((got - want) / want).abs() <= 1e-6,
                "h {h}: {got} against {want}"
            );
            assert_eq!(one.face_list().len(), 5, "h {h}");
        }
        assert_eq!(t.face_list().len(), faces, "h {h}: the T changed");
        assert!((volume(&k, &mut t) - before).abs() <= 1e-9 * before);
    }
}

/// A staggered intermittent weld on both sides of a T-joint (ISO 2553's staggered: the spans of
/// one side sit in the gaps of the other's): [`span_lengths`] gives each span's length, a
/// fifth of the edge each, which add up to the edge's length welded once.
#[test]
fn a_staggered_weld_s_span_lengths_add_up_to_the_edge() {
    let left_spans = BeadSpan::along(&[(0.0, 0.2), (0.4, 0.6), (0.8, 1.0)]).unwrap();
    let right_spans = BeadSpan::along(&[(0.2, 0.4), (0.6, 0.8)]).unwrap();
    let mut k = kernel();
    let t = tee(&mut k);
    let mut total = 0.0;
    for (at, spans) in [(-5.0, &left_spans), (5.0, &right_spans)] {
        let edge = pick(&k, &t, [at, 0.0, LENGTH / 2.0]);
        let lengths = span_lengths(&k, &t, edge, spans).unwrap();
        assert_eq!(lengths.len(), spans.len());
        for length in lengths {
            assert!(
                (length - 0.2 * LENGTH).abs() <= 1e-9 * LENGTH,
                "{at}: {length}"
            );
            total += length;
        }
    }
    assert!((total - LENGTH).abs() <= 1e-9 * LENGTH, "{total}");
}

/// The bead face, if it is one face carrying an exact round.
fn bead_face(b: &Brep) -> solidcore::surface::surface::Surface {
    let mut found = Vec::new();
    for &f in b.face_list() {
        let Some(s) = b.face(f).unwrap().surface.as_ref() else {
            continue;
        };
        if matches!(s.procedural_blend(), Some(BlendRef::Exact { .. })) {
            found.push(s.clone());
        }
    }
    assert_eq!(found.len(), 1, "one bead face with a definition");
    found.remove(0)
}

/// The bead apart's face keeps its definition, the generic blend's chord (order 0, exact,
/// bound 0) of its sizes with its sagitta beside them, re-certified on the cylinder through the
/// feet's lines. No record holds a sagitta yet: writing the definition is refused
/// (`Unsupported`), never written as the flat chord, and the face still saves as its cylinder.
#[test]
fn the_bead_face_is_the_chord_with_its_sagitta() {
    for (alpha, h) in [
        (FRAC_PI_2, 1.0),
        (FRAC_PI_2, -1.0),
        (FRAC_PI_3, 0.5),
        (FRAC_PI_2, 3.0),
    ] {
        let mut k = kernel();
        let (mut corner_body, at) = corner(&mut k, alpha);
        let edge = pick(&k, &corner_body, at);
        let size = ChamferSize::Distances([SETBACK; 2]);
        let b = beads_apart(&mut k, &mut corner_body, &[edge], &[size], h, &[])
            .unwrap()
            .remove(0);
        let face = bead_face(&b);
        let blend = face.procedural_blend().unwrap();
        assert_eq!(blend.order(), 0, "{alpha} {h}");
        assert_eq!(blend.bound(), 0.0, "{alpha} {h}");
        let BlendRef::Exact { round, .. } = blend else {
            panic!("{alpha} {h}: the bead face is exact");
        };
        assert_eq!(round.class(), ExactClass::Chord, "{alpha} {h}");
        assert_eq!(round.sagitta(), h, "{alpha} {h}");
        let err = exact_to_record(&k, &face).unwrap_err();
        assert!(
            matches!(err, SolidError::Unsupported { .. }),
            "{alpha} {h}: {err:?}"
        );
        assert!(!smb(&k, &b).is_empty(), "{alpha} {h}: the bead saves");
    }
}

/// A point of a rational quadratic Bezier piece at `t`.
fn rational_quadratic(p: [Point3; 3], w: [f64; 3], t: f64) -> Point3 {
    let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
    let d = b[0] * w[0] + b[1] * w[1] + b[2] * w[2];
    (p[0] * (b[0] * w[0]) + p[1] * (b[1] * w[1]) + p[2] * (b[2] * w[2])) * (1.0 / d)
}

/// [`BeadArc::rows`] is the arc: both rational quadratic pieces lie on its circle (checked at
/// 65 points each, a verification of the closed form, not a certificate), start and end at the
/// feet and meet at the apex, the sagitta from the chord's middle; concave, convex and
/// overhanging past a half turn.
#[test]
fn the_arc_rows_are_the_arc() {
    let f1 = Point3::new(5.0, 0.0, 0.0);
    let f2 = Point3::new(0.0, 5.0, 0.0);
    let along = Vec3::new(0.0, 0.0, 1.0);
    let away = Vec3::new(1.0, 1.0, 0.0);
    for h in [-1.0, 0.5, 2.0, 3.5, 6.0] {
        let arc = BeadArc::new(f1, f2, along, away, h).unwrap();
        let (p, w) = arc.rows();
        assert!((p[0] - f1).length() <= 1e-13, "{h}");
        assert!((p[4] - f2).length() <= 1e-13, "{h}");
        let middle = f1 + (f2 - f1) * 0.5;
        assert!(((p[2] - middle).length() - h.abs()).abs() <= 1e-13, "{h}");
        for piece in [[0, 1, 2], [2, 3, 4]] {
            for i in 0..=64 {
                let t = f64::from(i) / 64.0;
                let q = rational_quadratic(piece.map(|j| p[j]), piece.map(|j| w[j]), t);
                let off = ((q - arc.centre).length() - arc.radius).abs();
                assert!(
                    off <= 1e-12 * arc.radius,
                    "h {h} piece {piece:?} t {t}: {off}"
                );
                assert!(
                    (q - arc.centre).dot(along).abs() <= 1e-13,
                    "{h}: off the plane"
                );
            }
        }
    }
}

/// The throat `a` by hand: 5 mm legs at 90 degrees give 5 / sqrt 2 = 3.535534; the 1/4 x 1/2
/// inch unequal legs 0.25 x 0.5 / sqrt(0.3125) = 0.223607 (root to the line between the toes);
/// 4 and 6 mm at 60 degrees 24 sin 60 / sqrt 28 = 3.927922. Convexity does not count; a concave
/// face with equal legs dips its sagitta along the bisector (3.535534 - 1).
#[test]
fn the_throat_holds_its_hand_values() {
    let throat = |alpha: f64, d: [f64; 2], h: f64| {
        BeadDefinition::new(
            ChamferDefinition::new(ChamferSize::Distances(d), alpha).unwrap(),
            h,
        )
        .unwrap()
        .throat()
    };
    for (alpha, d, h, want) in [
        (FRAC_PI_2, [5.0, 5.0], 1.0, 3.535_533_906),
        (FRAC_PI_2, [5.0, 5.0], 3.0, 3.535_533_906),
        (FRAC_PI_2, [0.25, 0.5], 0.01, 0.223_606_798),
        (FRAC_PI_3, [4.0, 6.0], 0.5, 3.927_922_024),
        (FRAC_PI_2, [5.0, 5.0], -1.0, 2.535_533_906),
    ] {
        let got = throat(alpha, d, h);
        assert!(
            (got - want).abs() <= 1e-9,
            "{alpha} {d:?} {h}: {got} against {want}"
        );
    }
}

/// A concave throat with unequal legs is the root's distance to the arc: no point of the arc
/// is nearer (checked at 20001 points, a verification of the closed form, not a certificate),
/// and the nearest checked point is within the sampling's reach of it.
#[test]
fn a_concave_throat_is_the_nearest_point_of_the_arc() {
    for (alpha, d, h) in [
        (FRAC_PI_2, [4.0, 7.0], -0.8),
        (FRAC_PI_3, [5.0, 3.0], -0.3),
        (2.0 * FRAC_PI_3, [6.0, 4.0], -0.5),
        // an obtuse toe at the second foot (98.2 degrees): the arc's nearest point is that foot
        (FRAC_PI_3, [8.0, 3.0], -0.5),
    ] {
        let bead = BeadDefinition::new(
            ChamferDefinition::new(ChamferSize::Distances(d), alpha).unwrap(),
            h,
        )
        .unwrap();
        let (s, c) = f64::sin_cos(alpha);
        let f1 = Point3::new(d[0], 0.0, 0.0);
        let f2 = Point3::new(d[1] * c, d[1] * s, 0.0);
        let root = Point3::new(0.0, 0.0, 0.0);
        let away = (f1 + (f2 - f1) * 0.5) - root;
        let arc = BeadArc::new(f1, f2, Vec3::new(0.0, 0.0, 1.0), away, h).unwrap();
        let nearest = (0..=20_000)
            .map(|i| (arc.point(arc.turn * f64::from(i) / 20_000.0) - root).length())
            .fold(f64::INFINITY, f64::min);
        let a = bead.throat();
        assert!(
            nearest >= a - 1e-12,
            "{alpha} {d:?} {h}: a point at {nearest} under {a}"
        );
        assert!(
            nearest - a <= 1e-6,
            "{alpha} {d:?} {h}: nearest {nearest} against {a}"
        );
    }
}

/// A shallow concave bead's throat keeps its digits: with equal legs the arc's nearest point
/// to the root is its apex, `|h|` nearer than the chord's height, at a sagitta whose radius
/// (6.25e9 mm here) would swallow it if the throat were the difference of two lengths.
#[test]
fn a_shallow_concave_throat_keeps_its_digits() {
    let h = -1e-9;
    let bead = BeadDefinition::new(
        ChamferDefinition::new(ChamferSize::Distances([5.0; 2]), FRAC_PI_2).unwrap(),
        h,
    )
    .unwrap();
    let want = 5.0 / 2.0_f64.sqrt() - h.abs();
    let got = bead.throat();
    assert!((got - want).abs() < 1e-12, "{got} {want}");
}

/// A bead apart whose toe runs past its face (the corner's arms reach 40 mm) is refused before
/// anything is built, naming the face and the largest leg whose toe stays on it, which
/// builds; the part's file is unchanged.
#[test]
fn a_bead_apart_whose_toe_runs_past_its_face_is_refused_with_the_leg_that_fits() {
    use solidcore::error::{ErrorKind, FitMeasure};
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, FRAC_PI_2);
    let edge = pick(&k, &b, at);
    let before = smb(&k, &b);
    let size = ChamferSize::Distances([50.0, SETBACK]);
    let err = beads_apart(&mut k, &mut b, &[edge], &[size], 1.0, &[]).unwrap_err();
    assert!(smb(&k, &b) == before, "the corner changed");
    assert_eq!(err.kind(), ErrorKind::FilletTooLarge, "{err}");
    let fit = err.fit().unwrap();
    assert_eq!(fit.measure, FitMeasure::Leg, "{err}");
    assert!(fit.size > 39.9 && fit.size <= 40.0 + 1e-6, "{err}");
    assert!(err.site().is_some_and(|s| s.face.is_some()), "{err:?}");
    let size = ChamferSize::Distances([fit.size, SETBACK]);
    assert!(
        beads_apart(&mut k, &mut b, &[edge], &[size], 1.0, &[]).is_ok(),
        "the leg that fits builds"
    );
    assert!(smb(&k, &b) == before, "the corner changed");
}

/// A bead apart past its sagitta's bound is refused as too large with the bound as its fit:
/// at 90 degrees with 5 mm legs the convex bound is (c / 2) tan(67.5 degrees) = 8.5355; a
/// sagitta a hair under it builds.
#[test]
fn a_bead_apart_past_its_sagitta_bound_is_refused_with_the_bound() {
    use solidcore::error::{ErrorKind, FitMeasure};
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, FRAC_PI_2);
    let edge = pick(&k, &b, at);
    let before = smb(&k, &b);
    let size = ChamferSize::Distances([5.0; 2]);
    let err = beads_apart(&mut k, &mut b, &[edge], &[size], 9.0, &[]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::FilletTooLarge, "{err}");
    let fit = err.fit().unwrap();
    assert_eq!(fit.measure, FitMeasure::Sagitta, "{err}");
    let want = 50.0_f64.sqrt() / 2.0 * (3.0 * PI / 8.0).tan();
    assert!((fit.size - want).abs() < 1e-9, "{} {want}", fit.size);
    assert!(
        beads_apart(
            &mut k,
            &mut b,
            &[edge],
            &[size],
            fit.size * (1.0 - 1e-6),
            &[]
        )
        .is_ok(),
        "the sagitta that fits builds"
    );
    assert!(smb(&k, &b) == before, "the corner changed");
}

/// Equal legs back from a throat: 3.535534 at 90 degrees is 5 mm legs, at 60 degrees 4.330127
/// (5 cos 30) is 5 mm, and a concave throat adds its dip back; each round trip through
/// [`BeadDefinition::throat`] returns the throat.
#[test]
fn equal_legs_come_back_from_their_throat() {
    for (alpha, h) in [
        (FRAC_PI_2, 1.0),
        (FRAC_PI_3, 0.5),
        (FRAC_PI_2, -1.0),
        (2.0 * FRAC_PI_3, 0.4),
    ] {
        let bead = BeadDefinition::new(
            ChamferDefinition::new(ChamferSize::Distances([5.0; 2]), alpha).unwrap(),
            h,
        )
        .unwrap();
        let z = equal_legs_for_throat(bead.throat(), alpha, h);
        assert!((z - 5.0).abs() <= 1e-12, "{alpha} {h}: {z}");
    }
    assert!((equal_legs_for_throat(3.0, FRAC_PI_2, 0.5) - 3.0 * 2f64.sqrt()).abs() <= 1e-12);
}

// ---- the refusals and the part left as it was ----
//
// A weld never changes the part it lies on: its file is the same byte for byte before and
// after, built or refused. Every refusal comes before any bead is built.

/// No failure of the whole-body check (`SmBrep::AssertValid`, level 2).
fn assert_sound(k: &mut Kernel, b: &mut Brep, what: &str) {
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    let c = check_brep(k, b, CheckLevel::Level2).unwrap();
    let fails: Vec<String> = c.failures().map(|r| format!("{r:?}")).collect();
    assert!(fails.is_empty(), "{what}: {fails:?}");
}

/// `err` is the refusal of a bead not built yet for `gap` (class `Unsupported`).
fn assert_not_built(err: &SolidError, gap: WeldGap) {
    assert!(
        matches!(
            err.failure().map(|f| &f.reason),
            Some(Reason::Blend(BlendFailure::WeldNotBuilt(g))) if *g == gap
        ),
        "not {gap:?}: {err:?}"
    );
    assert_eq!(err.kind(), ErrorKind::Unsupported, "{err:?}");
}

/// The edge of `b` named by `err`'s site.
fn named_edge(err: &SolidError) -> Option<EdgeId> {
    err.site()
        .and_then(|s| s.edge)
        .map(solidcore::error::TargetEdge::id)
}

/// The bead apart is a sound body of 5 faces that saves, its section along the edge: convex,
/// concave and overhanging its toe, at 60, 90 and 120 degrees; the corner's file is the same
/// byte for byte.
#[test]
fn a_bead_apart_is_sound_and_leaves_the_corner_s_file_as_it_was() {
    for (adeg, h) in [
        (90.0_f64, 1.0),
        (90.0, -1.0),
        (90.0, 3.0),
        (60.0, 1.0),
        (60.0, -1.0),
        (120.0, 2.6),
    ] {
        let alpha = adeg.to_radians();
        let mut k = kernel();
        let (mut b, at) = corner(&mut k, alpha);
        let edge = pick(&k, &b, at);
        let before = smb(&k, &b);
        let size = ChamferSize::Distances([SETBACK; 2]);
        let mut body = beads_apart(&mut k, &mut b, &[edge], &[size], h, &[])
            .unwrap()
            .remove(0);
        assert!(smb(&k, &b) == before, "{adeg} {h}: the corner changed");
        let want = bead_section(alpha, h, 0.0).0.section_area() * LENGTH;
        let got = volume(&k, &mut body);
        assert!(
            ((got - want) / want).abs() <= 1e-6,
            "{adeg} {h}: {got} against {want}"
        );
        assert_eq!(body.face_list().len(), 5, "{adeg} {h}");
        assert_sound(&mut k, &mut body, "the bead apart");
        assert!(!smb(&k, &body).is_empty());
    }
}

/// Unequal legs, the first along the face in `along`: in the square corner, legs 6 and 3 put
/// the toe 6 from the edge on the face picked and 3 on the other, whichever face is picked;
/// the volume is the triangle of the legs and the segment over its chord.
#[test]
fn unequal_legs_run_their_first_leg_along_the_face_picked() {
    let legs: [f64; 2] = [6.0, 3.0];
    let h = 0.8;
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, FRAC_PI_2);
    let edge = pick(&k, &b, at);
    let faces = edge::faces(&b, edge, None).unwrap();
    assert_eq!(faces.len(), 2);
    let chord = (legs[0] * legs[0] + legs[1] * legs[1]).sqrt();
    let want = (0.5 * legs[0] * legs[1] + segment_area(chord, h)) * LENGTH;
    let mut toes_seen = Vec::new();
    for &face in &faces {
        let mut body = beads_apart(
            &mut k,
            &mut b,
            &[edge],
            &[ChamferSize::Distances(legs)],
            h,
            &[face],
        )
        .unwrap()
        .remove(0);
        let got = volume(&k, &mut body);
        assert!(((got - want) / want).abs() <= 1e-6, "{got} against {want}");
        // the toes at z = 0: the bead's vertices off the edge there
        let mut toes: Vec<Point3> = body
            .vertex_list()
            .iter()
            .map(|&v| solidcore::topology::vertex::point(&body, v).unwrap())
            .filter(|p| p.z.abs() <= 1e-9 && (p.x.abs() + p.y.abs()) > 1e-9)
            .collect();
        assert_eq!(toes.len(), 2, "{toes:?}");
        toes.sort_by(|p, q| p.x.total_cmp(&q.x));
        toes_seen.push(toes);
    }
    // the square corner's faces are y = 0 (along +x) and x = 0 (along +y): one pick puts the
    // first leg along x, the other along y
    let mut pairs: Vec<[(f64, f64); 2]> = toes_seen
        .iter()
        .map(|t| [(t[0].x, t[0].y), (t[1].x, t[1].y)])
        .collect();
    pairs.sort_by(|p, q| p[1].0.total_cmp(&q[1].0));
    let close =
        |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() <= 1e-9 && (a.1 - b.1).abs() <= 1e-9;
    assert!(
        close(pairs[0][0], (0.0, 6.0)) && close(pairs[0][1], (3.0, 0.0)),
        "{pairs:?}"
    );
    assert!(
        close(pairs[1][0], (0.0, 3.0)) && close(pairs[1][1], (6.0, 0.0)),
        "{pairs:?}"
    );
}

/// Two beads that meet at a vertex are refused before anything is built, as a bead not built
/// yet naming the vertex, and the corner's file is unchanged.
#[test]
fn beads_that_meet_at_a_vertex_are_refused_naming_it() {
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, FRAC_PI_2);
    let first = pick(&k, &b, at);
    let (start, _) = edge::start_end_vertices(&b, first).unwrap();
    let start = start.unwrap();
    let second = b
        .edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            e != first && (s == Some(start) || t == Some(start))
        })
        .unwrap();
    let before = smb(&k, &b);
    let size = ChamferSize::Distances([SETBACK; 2]);
    let err = beads_apart(&mut k, &mut b, &[first, second], &[size; 2], 1.0, &[]).unwrap_err();
    assert_not_built(&err, WeldGap::BeadsMeetAtVertex);
    let named = err
        .site()
        .and_then(|s| s.vertex)
        .map(solidcore::error::TargetVertex::id);
    assert_eq!(named, Some(start), "{err:?}");
    assert!(smb(&k, &b) == before, "the corner changed");
}

/// An edge listed twice is refused as invalid input before anything is built.
#[test]
fn an_edge_listed_twice_is_refused() {
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, FRAC_PI_2);
    let edge = pick(&k, &b, at);
    let before = smb(&k, &b);
    let size = ChamferSize::Distances([SETBACK; 2]);
    let err = beads_apart(&mut k, &mut b, &[edge, edge], &[size; 2], 1.0, &[]).unwrap_err();
    assert!(matches!(err, SolidError::InvalidInput { .. }), "{err:?}");
    assert!(smb(&k, &b) == before, "the corner changed");
}

/// An outside edge (the chord would run through the part) and a tangent edge between two
/// faces of one plane (the chord would run through the edge) are refused as invalid input
/// before anything is built, and the part's file is unchanged.
#[test]
fn an_outside_or_a_tangent_edge_is_refused() {
    let mut k = kernel();
    // an outside edge of the corner: its first arm's end at x = 40 meets its top at y = 0
    let (mut b, _) = corner(&mut k, FRAC_PI_2);
    let outside = pick(&k, &b, [40.0, 0.0, LENGTH / 2.0]);
    let before = smb(&k, &b);
    let size = ChamferSize::Distances([SETBACK; 2]);
    let err = beads_apart(&mut k, &mut b, &[outside], &[size], 1.0, &[]).unwrap_err();
    assert!(
        matches!(err, SolidError::InvalidInput { .. }),
        "outside: {err:?}"
    );
    assert!(smb(&k, &b) == before, "outside: the corner changed");
    // a tangent edge: a block whose bottom is two faces of y = 0 meeting at x = 10
    let points = [
        (0.0, 0.0),
        (10.0, 0.0),
        (20.0, 0.0),
        (20.0, 10.0),
        (0.0, 10.0),
    ];
    let curves: Vec<Curve> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &c)| {
            Curve::from(
                create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(c.0, c.1, 0.0)).unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    let mut block =
        create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), LENGTH, true).unwrap();
    let tangent = pick(&k, &block, [10.0, 0.0, LENGTH / 2.0]);
    assert_eq!(
        edge::faces(&block, tangent, None).unwrap().len(),
        2,
        "the tangent edge lies between two faces"
    );
    let before = smb(&k, &block);
    let err = beads_apart(&mut k, &mut block, &[tangent], &[size], 1.0, &[]).unwrap_err();
    assert!(
        matches!(err, SolidError::InvalidInput { .. }),
        "tangent: {err:?}"
    );
    assert!(smb(&k, &block) == before, "tangent: the block changed");
}

/// A joint that is not a straight edge between two planes (the circle where a cylinder meets
/// its cap) is refused as a bead not built yet, naming
/// the edge, and the part's file is unchanged.
#[test]
fn a_curved_joint_is_refused_naming_the_edge() {
    let mut k = kernel();
    let mut tube = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, LENGTH).unwrap();
    let rim = pick(&k, &tube, [10.0, 0.0, 0.0]);
    let before = smb(&k, &tube);
    let size = ChamferSize::Distances([2.0; 2]);
    let err = beads_apart(&mut k, &mut tube, &[rim], &[size], 0.5, &[]).unwrap_err();
    assert_not_built(&err, WeldGap::CurvedJoint);
    assert_eq!(named_edge(&err), Some(rim), "{err:?}");
    assert!(smb(&k, &tube) == before, "the cylinder changed");
}

/// Legs with an obtuse toe (at 60 degrees, legs 8 and 3: 3 is less than 8 cos 60) are refused
/// as a bead not built yet, naming the edge, before anything is built.
#[test]
fn an_obtuse_toe_is_refused_naming_the_edge() {
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, FRAC_PI_3);
    let edge = pick(&k, &b, at);
    let before = smb(&k, &b);
    let err = beads_apart(
        &mut k,
        &mut b,
        &[edge],
        &[ChamferSize::Distances([8.0, 3.0])],
        0.5,
        &[],
    )
    .unwrap_err();
    assert_not_built(&err, WeldGap::ObtuseToe);
    assert_eq!(named_edge(&err), Some(edge), "{err:?}");
    assert!(smb(&k, &b) == before, "the corner changed");
}

/// #21138: a leg below the body's resolution (eight times its tolerance) is a toe on the edge
/// within tolerance, a sliver, and is refused as a chamfer's setback that small is (#21004):
/// `SizeBelowResolution` naming the edge and the leg, before anything is built and before
/// the sagitta's bound. In the square corner a distance of 0.001 at an angle of 1e-300 gives
/// the second leg 0.001 sin(1e-300) / sin(pi/2 + 1e-300) = 1e-303; a leg of 1e-9 given as a
/// distance is refused alike. The same distance at 22.5 degrees (second leg 0.001 tan(22.5
/// degrees) = 0.000414) builds.
#[test]
fn a_leg_below_the_body_s_resolution_is_refused_naming_the_edge() {
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, FRAC_PI_2);
    let edge = pick(&k, &b, at);
    let tolerance = b.tolerance();
    let before = smb(&k, &b);
    let sliver = ChamferSize::DistanceAngle {
        distance: 0.001,
        angle: 1e-300,
    };
    for (size, sagitta, leg) in [
        (sliver, 0.0, 1e-303),
        (sliver, 1.0, 1e-303),
        (ChamferSize::Distances([SETBACK, 1e-9]), 0.0, 1e-9),
    ] {
        let err = beads_apart(&mut k, &mut b, &[edge], &[size], sagitta, &[]).unwrap_err();
        let Some(&BlendFailure::SizeBelowResolution {
            size: got,
            tolerance: t,
        }) = err.blend_failure()
        else {
            panic!("{size:?}, sagitta {sagitta}: {err:?}");
        };
        // the leg below resolution, not the distance asked: positive, finite, a sliver
        assert!(got > 0.0 && (got - leg).abs() <= 1e-12 * leg, "{got} {leg}");
        assert_eq!(t, tolerance);
        assert_eq!(err.kind(), ErrorKind::InvalidInput, "{err:?}");
        assert_eq!(err.entity(), Some(EntityRef::Edge(edge)), "{err:?}");
        assert_eq!(named_edge(&err), Some(edge), "{err:?}");
        assert_eq!(err.operation(), "blend::weld_bead");
        assert_eq!(
            solidcore::error::upstream_status(&err),
            Some(solidcore::error::SM_ERR_INVALID_INPUT)
        );
        assert!(smb(&k, &b) == before, "the corner changed");
    }
    let good = ChamferSize::DistanceAngle {
        distance: 0.001,
        angle: PI / 8.0,
    };
    let beads = beads_apart(&mut k, &mut b, &[edge], &[good], 0.0, &[]).unwrap();
    assert_eq!(beads.len(), 1);
    assert!(smb(&k, &b) == before, "the corner changed");
}
