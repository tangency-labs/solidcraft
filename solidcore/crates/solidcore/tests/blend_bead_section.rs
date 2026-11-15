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
use solidcore::blend::weld_bead::{beads_apart, beads_apart_in_spans, span_lengths};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::procedural_blend::bead::{BeadArc, BeadDefinition, BeadSpan};
use solidcore::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p7::copy_faces;
use solidcore::topology::brep_data_write::FromBrepEnv;
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
    let store = AttributeStore::new();
    let key_of = |_| ObjectKey(0);
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &key_of,
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
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
fn a_sagitta_of_zero_or_not_finite_is_refused() {
    let c = ChamferDefinition::new(ChamferSize::Distances([5.0; 2]), FRAC_PI_2).unwrap();
    for h in [0.0, f64::NAN, f64::INFINITY, -2.0, 9.0] {
        assert!(BeadDefinition::new(c, h).is_err(), "{h}");
    }
    assert!(BeadDefinition::new(c, 1.0).is_ok());
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

// ---- the bead through the blend ----
//
// `FilletSession::prepare_beads_along` lays the bead in the corner as the blend lays a chamfer:
// its rails at the chamfer's setbacks, its section the arc. Between planes along a straight edge
// the blend's face is the arc swept along the edge, a circular cylinder.

/// The corner with a bead of setbacks `SETBACK` and sagitta `h` built by the blend along its
/// edge, and the volume it gained.
fn blended_bead(alpha: f64, h: f64) -> (Kernel, Brep, f64) {
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, alpha);
    let before = volume(&k, &mut b);
    let edge = pick(&k, &b, at);
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_beads_along(&k, &[edge], &[ChamferSize::Distances([SETBACK; 2])], h, &[])
            .unwrap();
        s.create_corners(&k, None).unwrap();
        s.do_filleting(&mut k, None).unwrap();
    }
    let gain = volume(&k, &mut b) - before;
    (k, b, gain)
}

/// The blend's bead adds its section along the whole edge, as the bead's own body has it:
/// convex and concave at 90 and 60 degrees against the closed forms. At 60 degrees and 5 mm a
/// convex bead overhangs its toe past 0.6699 mm (`overhang_bound`), so it is laid at 0.5 mm.
#[test]
fn the_blend_lays_the_bead_in_the_corner() {
    for (alpha, h) in [
        (FRAC_PI_2, 1.0),
        (FRAC_PI_2, -1.0),
        (FRAC_PI_3, 0.5),
        (FRAC_PI_3, -1.0),
    ] {
        let (_, _, gain) = blended_bead(alpha, h);
        let (bead, _) = bead_section(alpha, h, 0.0);
        let want = bead.section_area() * LENGTH;
        assert!(
            ((gain - want) / want).abs() <= 1e-6,
            "alpha {alpha} h {h}: gained {gain} against {want}"
        );
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

/// A convex bead that overhangs its toe is refused when it is prepared without the kernel's
/// `fillet_trim_inside_by_chord`: the blend's trimming then took the wrong side of its rails
/// (at 90 degrees and h = 1.48 every face but two deleted, a volume of -90000) or failed.
#[test]
fn without_the_chord_trim_a_bead_that_overhangs_its_toe_is_refused() {
    for (alpha, h) in [(FRAC_PI_3, 0.68), (FRAC_PI_3, 1.0), (FRAC_PI_2, 1.48)] {
        let mut k = kernel();
        k.fixes.fillet_trim_inside_by_chord = false;
        let (mut b, at) = corner(&mut k, alpha);
        let edge = pick(&k, &b, at);
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        let r = s.prepare_beads_along(&k, &[edge], &[ChamferSize::Distances([SETBACK; 2])], h, &[]);
        assert!(r.is_err(), "alpha {alpha} h {h}: {r:?}");
    }
}

/// With `fillet_trim_inside_by_chord` a convex bead that overhangs its toe is built: the body
/// gains the bead's section along the whole edge, just past the bound (90 degrees, 1.47 and
/// 1.48 mm against 1.4645), far past it (a near half circle at 3 mm) and at 60 and 120 degrees.
#[test]
fn a_bead_that_overhangs_its_toe_is_laid_with_the_chord_trim() {
    for (adeg, h) in [
        (90.0_f64, 1.47),
        (90.0, 1.48),
        (90.0, 2.0),
        (90.0, 3.0),
        (60.0, 0.68),
        (60.0, 1.0),
        (120.0, 2.6),
    ] {
        let alpha = adeg.to_radians();
        let (_, b, gain) = blended_bead(alpha, h);
        let (bead, _) = bead_section(alpha, h, 0.0);
        assert!(h >= bead.overhang_bound(), "{adeg} {h}: not past the bound");
        let want = bead.section_area() * LENGTH;
        assert!(
            ((gain - want) / want).abs() <= 1e-6,
            "{adeg} {h}: gained {gain} against {want}"
        );
        // the corner's 8 faces less none, and the bead's face
        assert_eq!(b.face_list().len(), 9, "{adeg} {h}");
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
    ] {
        let alpha = adeg.to_radians();
        let mut k = kernel();
        let (mut b, at) = corner(&mut k, alpha);
        let before = volume(&k, &mut b);
        let faces = b.face_list().len();
        let edge = pick(&k, &b, at);
        let mut apart = beads_apart(
            &mut k,
            &b,
            &[edge],
            &[ChamferSize::Distances([SETBACK; 2])],
            h,
            &[],
        )
        .unwrap();
        let (bead, _) = bead_section(alpha, h, 0.0);
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

/// A sagitta past its bound is refused when the bead is prepared, not built as something else.
#[test]
fn the_blend_refuses_a_bead_past_its_bound() {
    let mut k = kernel();
    let (mut b, at) = corner(&mut k, FRAC_PI_2);
    let edge = pick(&k, &b, at);
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    // at 90 degrees and 5 mm a concave bead stays below 1.4645 mm
    let r = s.prepare_beads_along(
        &k,
        &[edge],
        &[ChamferSize::Distances([SETBACK; 2])],
        -1.5,
        &[],
    );
    assert!(r.is_err(), "{r:?}");
}

/// The corners with the blend's beads, as `.smb` files for pictures.
#[test]
#[ignore = "writes the corners with the blend's beads as .smb files for pictures; checks nothing"]
fn write_the_blended_beads_for_pictures() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    for (name, alpha, h) in [
        ("blended_90_convex", FRAC_PI_2, 1.0),
        ("blended_90_concave", FRAC_PI_2, -1.0),
        ("blended_60_convex", FRAC_PI_3, 0.5),
        ("blended_60_concave", FRAC_PI_3, -1.0),
    ] {
        let (k, b, _) = blended_bead(alpha, h);
        let path = dir.join(format!("{name}.smb"));
        std::fs::write(&path, smb(&k, &b)).unwrap();
        println!("wrote {}", path.display());
    }
}

/// [`beads_apart_in_spans`]: an intermittent bead is one body per span, each the bead's section
/// along its span's share of the edge, ended square to the edge (its face, the two strips and
/// two plane ends), and the corner is left as it was.
#[test]
fn a_bead_apart_in_spans_is_one_body_per_span() {
    let spans = BeadSpan::along(&[(0.1, 0.3), (0.5, 0.9), (0.95, 1.0)]).unwrap();
    for (adeg, h) in [(90.0_f64, 1.0), (60.0, -1.0), (90.0, 3.0)] {
        let alpha = adeg.to_radians();
        let mut k = kernel();
        let (mut b, at) = corner(&mut k, alpha);
        let before = volume(&k, &mut b);
        let edge = pick(&k, &b, at);
        let pieces = beads_apart_in_spans(
            &mut k,
            &b,
            edge,
            ChamferSize::Distances([SETBACK; 2]),
            h,
            &[],
            &spans,
        )
        .unwrap();
        assert_eq!(pieces.len(), spans.len());
        let (bead, _) = bead_section(alpha, h, 0.0);
        for (mut piece, span) in pieces.into_iter().zip(&spans) {
            let want = bead.section_area() * LENGTH * span.share();
            let got = volume(&k, &mut piece);
            assert!(
                ((got - want) / want).abs() <= 1e-6,
                "{adeg} {h} {span:?}: {got} against {want}"
            );
            assert_eq!(piece.face_list().len(), 5, "{adeg} {h} {span:?}");
        }
        assert!((volume(&k, &mut b) - before).abs() <= 1e-9 * before);
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

/// Beads on both sides of a T-joint ([`beads_apart`] with both inside edges): one body holding
/// both beads (two beads of 5 faces each, twice the section along the edge), and the T left as
/// it was.
#[test]
fn beads_on_both_sides_of_a_tee_are_one_body_apart() {
    for h in [1.0, -1.0, 3.0] {
        let mut k = kernel();
        let mut t = tee(&mut k);
        let before = volume(&k, &mut t);
        let faces = t.face_list().len();
        let left = pick(&k, &t, [-5.0, 0.0, LENGTH / 2.0]);
        let right = pick(&k, &t, [5.0, 0.0, LENGTH / 2.0]);
        let mut both = beads_apart(
            &mut k,
            &t,
            &[left, right],
            &[ChamferSize::Distances([SETBACK; 2]); 2],
            h,
            &[],
        )
        .unwrap();
        let (bead, _) = bead_section(FRAC_PI_2, h, 0.0);
        let want = 2.0 * bead.section_area() * LENGTH;
        let got = volume(&k, &mut both);
        assert!(((got - want) / want).abs() <= 1e-6, "h {h}: {got} against {want}");
        assert_eq!(both.face_list().len(), 10, "h {h}");
        assert_eq!(t.face_list().len(), faces, "h {h}: the T changed");
        assert!((volume(&k, &mut t) - before).abs() <= 1e-9 * before);
    }
}

/// A staggered intermittent weld on both sides of a T-joint (ISO 2553's staggered: the spans of
/// one side sit in the gaps of the other's): each span a body of its own with the section along
/// its length ([`span_lengths`]), which add up to the weld's length.
#[test]
fn a_staggered_weld_on_a_tee_is_one_body_per_span() {
    let left_spans = BeadSpan::along(&[(0.0, 0.2), (0.4, 0.6), (0.8, 1.0)]).unwrap();
    let right_spans = BeadSpan::along(&[(0.2, 0.4), (0.6, 0.8)]).unwrap();
    let h = 1.0;
    let mut k = kernel();
    let mut t = tee(&mut k);
    let before = volume(&k, &mut t);
    let (bead, _) = bead_section(FRAC_PI_2, h, 0.0);
    let mut total = 0.0;
    for (at, spans) in [(-5.0, &left_spans), (5.0, &right_spans)] {
        let edge = pick(&k, &t, [at, 0.0, LENGTH / 2.0]);
        let lengths = span_lengths(&k, &t, edge, spans).unwrap();
        let pieces = beads_apart_in_spans(
            &mut k,
            &t,
            edge,
            ChamferSize::Distances([SETBACK; 2]),
            h,
            &[],
            spans,
        )
        .unwrap();
        assert_eq!(pieces.len(), spans.len());
        for (mut piece, length) in pieces.into_iter().zip(lengths) {
            assert!((length - 0.2 * LENGTH).abs() <= 1e-9 * LENGTH, "{at}: {length}");
            let want = bead.section_area() * length;
            let got = volume(&k, &mut piece);
            assert!(((got - want) / want).abs() <= 1e-6, "{at}: {got} against {want}");
            assert_eq!(piece.face_list().len(), 5, "{at}");
            total += length;
        }
    }
    // five spans of a fifth each: the edge's length welded once, half on each side
    assert!((total - LENGTH).abs() <= 1e-9 * LENGTH, "{total}");
    assert!((volume(&k, &mut t) - before).abs() <= 1e-9 * before);
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
                assert!(off <= 1e-12 * arc.radius, "h {h} piece {piece:?} t {t}: {off}");
                assert!((q - arc.centre).dot(along).abs() <= 1e-13, "{h}: off the plane");
            }
        }
    }
}
