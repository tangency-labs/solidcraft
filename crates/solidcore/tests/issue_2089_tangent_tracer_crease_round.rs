//! tangency-labs/solidcore#2089: with `Kernel::fixes.ssi_tangent_curve_tracer` (WR-75)
//! on, an r 1 crease round fails with fillet code 3 ("Fillet Corner Error: Corner Edge
//! Computation Failure"). SolidCraft hit it once the r 2 round it runs onto is a plain
//! B-spline surface; with only the two fillet fixes below (not SolidCraft's whole
//! preset) the round fails as built too.
//!
//! A block x 0..40, y -5..5, z 0..15 and a cylinder r 10, height 30 on the Z axis (both
//! extruded, the circle's seam at 0.6131 rad as SolidCraft's), joined; the concave arc
//! where the block's top meets the cylinder is rounded r 2. Then the block's top edge
//! along y = 5 and the crease running up from it between the side face and the arc
//! round are rounded r 1, with that round's surface replaced by its NURBS form
//! (`turn_to_nurbs`; SolidCraft's flat conversion writes it that way).
//!
//! At the crease's upper end the r 1 round's surface runs on tangent to the cylinder.
//! There the tracer traced a crossing branch through the contact points along that
//! tangency, past the end where the marcher's curve stops, and replaced the marcher's
//! curve with it. The corner then put its vertex 0.15 above the r 2 round's rail, and
//! the next corner edge (cylinder / r 2 round, through that vertex) found no curve. A
//! crossing branch the marcher's curve covers outside the band where the surfaces are
//! within tolerance of each other is now dropped, so the marcher's curve stays.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::boolean::api_brep as sm;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::topology::api_files::turn_to_nurbs;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::face::p1 as fp1;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::EdgeId;
use solidcore::{Brep, EntityRef, Kernel};

/// SMLib (tangency-labs/smlib 26cc451) on SolidCraft's run of the issue's case.
const SMLIB_VOLUME: f64 = 13_992.239_421_88;

/// What happens to the r 2 round before the r 1 round.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    /// As the fillet built it (a surface of revolution).
    AsBuilt,
    /// Only the round's face given its `turn_to_nurbs` surface.
    RoundFaceNurbs,
    /// `turn_to_nurbs` on the whole body.
    AllNurbs,
}

fn pick(k: &Kernel, b: &Brep, points: &[[f64; 3]]) -> Vec<EdgeId> {
    points
        .iter()
        .map(|p| {
            let q = Point3::new(p[0], p[1], p[2]);
            let mut best = (f64::INFINITY, None);
            for e in p1::edges(b, None).unwrap() {
                if let Ok(c) = edge_closest_point(k, b, e, q) {
                    if c.closest.distance < best.0 {
                        best = (c.closest.distance, Some(e));
                    }
                }
            }
            best.1.unwrap()
        })
        .collect()
}

/// SolidCraft's round: the per-edge constant-radius setup, corners, then filleting;
/// the executive's error code (0 on success, after `StitchAndOrient`).
fn round(k: &mut Kernel, b: &mut Brep, points: &[[f64; 3]], r: f64) -> i32 {
    let edges = pick(k, b, points);
    let code = {
        let mut s = FilletSession::new(k, b).unwrap();
        s.set_blend_curve_at_thirds(false);
        s.prepare_per_edge_constant_radius(
            k,
            &edges,
            &vec![r; edges.len()],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(k, None).unwrap();
        s.set_overflow_first(edges.len() > 1);
        s.set_classify_only(false);
        s.set_mark_corner_patches(false);
        let _ = s.do_filleting(k, None);
        s.error_info().code
    };
    if code == 0 {
        stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    }
    b.refresh_cached_topology();
    code
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-9, &attrs).unwrap()
}

/// The crease round's error code and, when it builds, the body's volume.
fn crease_round(form: Form, tracer: bool) -> (i32, Option<f64>) {
    let mut k = Kernel::default();
    // the two fillet fixes the issue's preset needs to reach the crease round
    k.fixes.fillet_through_extended_rail_end = true;
    k.fixes.fillet_torus_span_from_start_section = true;
    k.fixes.ssi_tangent_curve_tracer = tracer;
    let v = Vec3::new;
    let sides = [
        ([0., -5.], [40., -5.]),
        ([40., -5.], [40., 5.]),
        ([40., 5.], [0., 5.]),
        ([0., 5.], [0., -5.]),
    ];
    let lines: Vec<Curve> = sides
        .iter()
        .map(|(a, b)| {
            Curve::from(
                Line::create_line_segment(3, v(a[0], a[1], 0.), v(b[0], b[1], 0.), None).unwrap(),
            )
        })
        .collect();
    let refs: Vec<Option<&Curve>> = lines.iter().map(Some).collect();
    let block = create_linear_sweep(&mut k, &refs, v(0., 0., 1.), 15.0, true).unwrap();
    let a0: f64 = 0.6131;
    let x0 = v(a0.cos(), a0.sin(), 0.);
    let frame = Axis2Placement::new(v(0., 0., 0.), x0, v(0., 0., 1.).cross(x0));
    let circle = Curve::from(
        Ellipse::create_canonical_circle(
            &k,
            &frame,
            10.0,
            Some(&Extent1d::from_min_max(0.0, 360.0)),
            None,
        )
        .unwrap(),
    );
    let cyl = create_linear_sweep(&mut k, &[Some(&circle)], v(0., 0., 1.), 30.0, true).unwrap();
    let mut u = sm::boolean(&mut k, block, cyl, BooleanOperation::Union)
        .unwrap()
        .result;
    u.refresh_cached_topology();
    assert_eq!(round(&mut k, &mut u, &[[10.0, 0.0, 15.0]], 2.0), 0);
    let mut b = brep_copy(&mut k, &u).unwrap();
    match form {
        Form::AsBuilt => {}
        Form::AllNurbs => turn_to_nurbs(&mut k, Some(&mut b)).unwrap(),
        Form::RoundFaceNurbs => {
            let mut n = brep_copy(&mut k, &b).unwrap();
            turn_to_nurbs(&mut k, Some(&mut n)).unwrap();
            let is_rev = |br: &Brep, f| {
                format!("{:?}", fp2::surface(br, f).unwrap()).contains("kind: SurfOfRevolution")
            };
            let (bf, nf) = (p1::faces(&b, None).unwrap(), p1::faces(&n, None).unwrap());
            let i = bf.iter().position(|f| is_rev(&b, *f)).unwrap();
            let surf = fp2::surface(&n, nf[i]).unwrap().unwrap().clone();
            let this = b.owner_identity(EntityRef::Face(bf[i]));
            fp1::set_surface(&k, &mut b, bf[i], this, surf, true).unwrap();
        }
    }
    b.refresh_cached_topology();
    // the top edge along y = 5 and the crease up from it
    let code = round(
        &mut k,
        &mut b,
        &[
            [25.454356057317852, 5.0, 15.0],
            [9.37304698630769, 5.0, 15.549716038060165],
        ],
        1.0,
    );
    let vol = (code == 0).then(|| volume(&k, &mut b));
    (code, vol)
}

fn assert_near(v: f64, expected: f64, rel: f64) {
    assert!(
        (v - expected).abs() <= rel * expected,
        "{v} != {expected} (rel {rel})"
    );
}

#[test]
fn nurbs_round_crease_builds_with_the_tracer() {
    for (form, rel) in [(Form::RoundFaceNurbs, 1e-9), (Form::AllNurbs, 1e-6)] {
        let (code, on) = crease_round(form, true);
        assert_eq!(code, 0, "{form:?}");
        let on = on.unwrap();
        assert_near(on, SMLIB_VOLUME, rel);
        // here the tracer finds nothing the marcher has not: the same body
        let (code_off, off) = crease_round(form, false);
        assert_eq!(code_off, 0, "{form:?}");
        assert_eq!(on.to_bits(), off.unwrap().to_bits(), "{form:?}");
    }
}

#[test]
fn revolution_round_crease_is_unchanged() {
    let (code, on) = crease_round(Form::AsBuilt, true);
    assert_eq!(code, 0);
    let (code_off, off) = crease_round(Form::AsBuilt, false);
    assert_eq!(code_off, 0);
    let on = on.unwrap();
    assert_eq!(on.to_bits(), off.unwrap().to_bits());
    assert_near(on, SMLIB_VOLUME, 1e-9);
}
