//! A knife edge chamfered by its sizes (`FilletSession::prepare_chamfers`) at equal distances
//! `d`: the kernel rig's `fillets/851-chamfer-knife-edge-tip-*` (#2793). The faces fold back
//! on each other along the edge, so there is no wedge for a chamfer's definition. The chamfer
//! is the chord between the contacts of the ball of radius `d` that fits there, as
//! `tests/wr_611_steep_fillet_trims_its_face.rs` makes it through the linear section. Other
//! sizes have no such chord and stay refused.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;

use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::sweep::api_brep::create_taper_extrude;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

const R: f64 = 21.0;
const TIP: f64 = 62.0 + 1.0 / 6.0;
const H: f64 = 10.0;

fn kernel() -> Kernel {
    let mut k = Kernel::default();
    k.fixes.fillet_knife_edge_rounds = true;
    k.fixes.fillet_trim_inside_by_best_dot = true;
    k
}

fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
    Curve::from(create_line_segment(Vec3::new(a.0, a.1, 0.0), Vec3::new(b.0, b.1, 0.0)).unwrap())
}

/// The part: a plate whose corner a circle of radius 21 about (TIP, -21) cuts, so its wall runs
/// out tangent into the back face y = 0 at the tip; extruded 10.
fn part(k: &mut Kernel) -> Brep {
    let curves = [
        line((0.0, 0.0), (TIP, 0.0)),
        Curve::from(create_arc(Vec3::new(TIP, -R, 0.0), R, 90.0, 180.0).unwrap()),
        line((TIP - R, -R), (0.0, -R)),
        line((0.0, -R), (0.0, 0.0)),
    ];
    let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
    create_taper_extrude(k, &refs, H, 0.0, 3, false).unwrap()
}

fn knife_edge(k: &Kernel, b: &Brep) -> EdgeId {
    let at_tip = |p: Point3| (p.x - TIP).abs() < 1e-6 && p.y.abs() < 1e-6;
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let p = curve.evaluate_point(k, ivl.min()).unwrap();
            let q = curve.evaluate_point(k, ivl.max()).unwrap();
            at_tip(p) && at_tip(q) && (p.z - q.z).abs() > H / 2.0
        })
        .expect("the knife edge")
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-11, &attrs).unwrap()
}

/// What the chord takes off: the triangle back contact, tip, wall contact, less the wall's
/// circular segment, over the height.
fn removed(d: f64) -> f64 {
    let flat = TIP - (4.0 * R * d).sqrt();
    let wy = -R + (R - d) * R / (R + d);
    let triangle = 0.5 * ((TIP - flat) * wy).abs();
    let b = ((R - d) / (R + d)).acos();
    (triangle - 0.5 * R * R * (b - b.sin())) * H
}

#[test]
fn equal_distances_are_the_chord_between_the_balls_contacts() {
    let before = (TIP - R) * R * H + R * R * (1.0 - PI / 4.0) * H;
    // the rig's values: 9432.157649 at d 2, 9027.684392 at d 5
    assert!((before - removed(2.0) - 9_432.157_648_674_35).abs() < 1e-6);
    assert!((before - removed(5.0) - 9_027.684_392_451_565).abs() < 1e-6);
    for d in [0.5, 2.0, 5.0] {
        let mut k = kernel();
        let mut b = part(&mut k);
        let e = knife_edge(&k, &b);
        {
            let mut s = FilletSession::new(&mut k, &mut b).unwrap();
            s.prepare_chamfers(&k, &[e], &[ChamferSize::Distances([d, d])])
                .unwrap();
            s.do_fillet(&mut k).unwrap();
        }
        stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
        assert!(is_manifold_solid(&b).unwrap(), "d {d}: not a solid");
        let v = volume(&k, &mut b);
        let want = before - removed(d);
        assert!(
            (v - want).abs() <= 1e-9 * want,
            "d {d}: volume {v}, expected {want}"
        );
    }
}

#[test]
fn other_sizes_stay_refused() {
    for size in [
        ChamferSize::Distances([2.0, 3.0]),
        ChamferSize::DistanceAngle {
            distance: 2.0,
            angle: 0.5,
        },
        ChamferSize::Chord(2.0),
    ] {
        let mut k = kernel();
        let mut b = part(&mut k);
        let e = knife_edge(&k, &b);
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        assert!(
            s.prepare_chamfers(&k, &[e], &[size]).is_err(),
            "{size:?} was accepted on a knife edge"
        );
    }
}
