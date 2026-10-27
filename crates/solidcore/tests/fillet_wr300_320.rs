//! Opt-in fixes of the fork's fillet defects (`Kernel::fixes`): WR-300 (#1023), WR-301
//! (#485), WR-302 (#841, #1024), WR-303 (#841) and WR-320 (#879). Each case runs with the
//! field off (SMLib's behaviour) and on.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_geom::p1::FilletErrorType;
use solidcore::blend::fillet_standard_solver::p1::FilletLaw;
use solidcore::boolean::api_brep::boolean_difference;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::edge;
use solidcore::topology::EdgeId;
use solidcore::{Brep, Kernel};

fn kernel(set: impl FnOnce(&mut UpstreamFixes)) -> Kernel {
    let mut k = Kernel::default();
    set(&mut k.fixes);
    k
}

fn volume(k: &Kernel, b: &mut Brep) -> f64 {
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    brep_volume(k, b, 1e-6, &attrs).unwrap()
}

/// The edge of `b` whose ends are `p` and `q` (either way round).
fn edge_between(k: &Kernel, b: &Brep, p: [f64; 3], q: [f64; 3]) -> EdgeId {
    let near = |a: Point3, c: [f64; 3]| {
        (a.x - c[0]).abs() < 1e-6 && (a.y - c[1]).abs() < 1e-6 && (a.z - c[2]).abs() < 1e-6
    };
    b.edge_list()
        .iter()
        .copied()
        .find(|&e| {
            let curve = edge::curve(b, e).unwrap().unwrap();
            let ivl = edge::interval(b, e).unwrap();
            let s = curve.evaluate_point(k, ivl.min()).unwrap();
            let t = curve.evaluate_point(k, ivl.max()).unwrap();
            (near(s, p) && near(t, q)) || (near(s, q) && near(t, p))
        })
        .unwrap_or_else(|| panic!("no edge {p:?} - {q:?}"))
}

/// What a fillet came to: its volume and whether it is a manifold solid, or the error
/// code the executive recorded (`SmFilletErrorType`).
#[derive(Debug, PartialEq)]
enum Outcome {
    Built { volume: f64, manifold: bool },
    Refused { code: i32 },
}

/// Stitch what a session built, or report the executive's error code.
fn outcome(k: &mut Kernel, b: &mut Brep, result: Result<(), i32>) -> Outcome {
    if let Err(code) = result {
        return Outcome::Refused { code };
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
    Outcome::Built {
        volume: volume(k, b),
        manifold: is_manifold_solid(b).unwrap(),
    }
}

/// `SmApiFilletEdges` with a constant radius, reporting the executive's error code.
fn fillet(k: &mut Kernel, b: &mut Brep, edges: &[EdgeId], radius: f64, xsect: u32) -> Outcome {
    let result = {
        let mut session = FilletSession::new(k, b).unwrap();
        session
            .prepare_constant_radius_edges(k, edges, radius, xsect, 1, 1.0)
            .unwrap();
        session.do_fillet(k).map_err(|_| session.error_info().code)
    };
    outcome(k, b, result)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---- WR-301 / WR-302: the cube of 20 with three openings (SolidCraft rig fillets/1080) ----

/// A cube of 20 with three orthogonal 10 x 10 openings through it, 5 in from its sides.
fn holed_cube(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    for (o, d) in [
        ([-1.0, 5.0, 5.0], [22.0, 10.0, 10.0]),
        ([5.0, -1.0, 5.0], [10.0, 22.0, 10.0]),
        ([5.0, 5.0, -1.0], [10.0, 10.0, 22.0]),
    ] {
        let tool = create_box(k, Point3::new(o[0], o[1], o[2]), d[0], d[1], d[2]).unwrap();
        b = boolean_difference(k, b, tool).unwrap();
    }
    b
}

/// The top front edge (y = 0, z = 20) chamfered (`xsect` 0) or rounded (1) by `size`.
fn holed_cube_blend(k: &mut Kernel, size: f64, xsect: u32) -> Outcome {
    let mut b = holed_cube(k);
    let e = edge_between(k, &b, [0.0, 0.0, 20.0], [20.0, 0.0, 20.0]);
    fillet(k, &mut b, &[e], size, xsect)
}

/// A blend whose setback reaches exactly the openings' rims (5): SMLib's rollover fails
/// on the rail touching the rim's corners and running along its top edge. With
/// `fillet_rail_rim_on_face` it builds, to the closed forms 4000 - 12.5 * 20 (chamfer) and
/// 4000 - (1 - pi/4) * 25 * 20 (round).
#[test]
fn rail_along_its_face_rim_rolls_over_only_where_it_leaves_the_face() {
    for (xsect, expected) in [
        (0, 3750.0),
        (1, 4000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 500.0),
    ] {
        let mut k = Kernel::default();
        assert_eq!(
            holed_cube_blend(&mut k, 5.0, xsect),
            Outcome::Refused { code: 4 },
            "xsect {xsect}: SM_FILERR_EDGE_PROBLEM (Unable to Process Rollover)"
        );
        let mut k = kernel(|f| f.fillet_rail_rim_on_face = true);
        let Outcome::Built { volume, manifold } = holed_cube_blend(&mut k, 5.0, xsect) else {
            panic!("xsect {xsect}: refused");
        };
        assert!(
            manifold && near(volume, expected, 1e-4),
            "xsect {xsect}: {volume}"
        );
    }
}

/// Sizes that leave the rim alone build the same either way.
#[test]
fn rail_rim_on_face_leaves_other_sizes_alone() {
    for xsect in [0, 1] {
        let mut k = Kernel::default();
        let off = holed_cube_blend(&mut k, 4.0, xsect);
        let mut k = kernel(|f| f.fillet_rail_rim_on_face = true);
        assert_eq!(holed_cube_blend(&mut k, 4.0, xsect), off);
    }
}

// ---- WR-303: the end of a round that crosses more than one face at a three-edge vertex ----

/// A 60 x 40 x 20 box, its top back edge rounded 5, its top right edge rounded 3, then
/// its vertical edge x = 60, y = 40 rounded `r` (SolidCraft ladder 07-fillets/016).
fn box_with_two_rounds_then(k: &mut Kernel, r: f64) -> Outcome {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let e = edge_between(k, &b, [0.0, 40.0, 20.0], [60.0, 40.0, 20.0]);
    assert!(matches!(
        fillet(k, &mut b, &[e], 5.0, 1),
        Outcome::Built { .. }
    ));
    let e = edge_between(k, &b, [60.0, 0.0, 20.0], [60.0, 35.0, 20.0]);
    assert!(matches!(
        fillet(k, &mut b, &[e], 3.0, 1),
        Outcome::Built { .. }
    ));
    let e = edge_between(k, &b, [60.0, 40.0, 0.0], [60.0, 40.0, 15.0]);
    fillet(k, &mut b, &[e], r, 1)
}

/// The third round's end runs across the R5 round's short end arc onto the R3 round: SMLib's
/// Nx1 corner builds an end rib and fails ("Corner Edge Computation Failure"). With
/// `fillet_nx1_end_rib_global_merge` the rib is deleted and the global merge cuts the round
/// off against the body: each builds, to the volumes of the integral of min(R5, R3 top)
/// over the corner column outside the vertical round (47418.926881, 47546.786538 and
/// 47591.811067). The kernel's own volume is a quadrature: it lands within ~2e-3 of these on arm
/// and ~1e-2 on x86 (CI: 47546.79569 for r 4), about 2e-7 relative, hence the 2e-2 tolerance.
#[test]
fn round_end_across_a_short_arc_is_cut_off_by_the_body() {
    for (r, expected) in [
        (7.0, 47_418.926_881),
        (4.0, 47_546.786_538),
        (2.0, 47_591.811_067),
    ] {
        let mut k = Kernel::default();
        assert_eq!(
            box_with_two_rounds_then(&mut k, r),
            Outcome::Refused { code: 3 },
            "r {r}"
        );
        let mut k = kernel(|f| f.fillet_nx1_end_rib_global_merge = true);
        let Outcome::Built { volume, manifold } = box_with_two_rounds_then(&mut k, r) else {
            panic!("r {r}: refused");
        };
        assert!(manifold && near(volume, expected, 2e-2), "r {r}: {volume}");
    }
}

// ---- WR-300: a variable round whose rail ends at a rounded corner's arc ----

/// `SmApiVariableRadiusFillet` with a linear law, reporting the executive's error code.
fn variable(k: &mut Kernel, b: &mut Brep, edges: &[EdgeId], r0: f64, r1: f64) -> Outcome {
    let result = {
        let mut session = FilletSession::new(k, b).unwrap();
        session
            .prepare_variable_radius_edges(k, edges, r0, FilletLaw::linear(r0, r1, None), 1, 1)
            .unwrap();
        session.do_fillet(k).map_err(|_| session.error_info().code)
    };
    outcome(k, b, result)
}

/// A 60 x 40 x 20 box, its vertical edge x = 60, y = 40 rounded 5, then its top back edge
/// rounded with a radius law `r0` to `r1` along it, ending at that corner round's arc
/// (SolidCraft ladder 07-fillets/012).
fn round_into_a_rounded_corner(k: &mut Kernel, r0: f64, r1: f64) -> Outcome {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let e = edge_between(k, &b, [60.0, 40.0, 0.0], [60.0, 40.0, 20.0]);
    assert!(matches!(
        fillet(k, &mut b, &[e], 5.0, 1),
        Outcome::Built { .. }
    ));
    let e = edge_between(k, &b, [0.0, 40.0, 20.0], [55.0, 40.0, 20.0]);
    variable(k, &mut b, &[e], r0, r1)
}

/// The law runs on past the edge's end, to 5.3636 where the rail meets the arc's end. SMLib's
/// retry rail search holds the radius at 5 there, finds the arc's end at 5 and builds a body
/// that is 3.8 mm3 too small (1 to 5; 3.95 for 2 to 5). With `fillet_retry_radius_as_first`
/// the rail misses the arc and the round is cut off against the body, as the first search
/// does, which needs `fillet_nx1_end_rib_global_merge` (the corner's end rib). Laws that
/// end small at the corner do not reach the retry and are unchanged.
#[test]
fn retry_rail_search_holds_the_radius_as_the_first_does() {
    let both = |f: &mut UpstreamFixes| {
        f.fillet_retry_radius_as_first = true;
        f.fillet_nx1_end_rib_global_merge = true;
    };
    for (r0, off, gain) in [(1.0, 47_751.094_836, 3.774), (2.0, 47_719.705_412, 3.952)] {
        let mut k = Kernel::default();
        let Outcome::Built { volume: v, .. } = round_into_a_rounded_corner(&mut k, r0, 5.0) else {
            panic!("{r0}: refused");
        };
        assert!(near(v, off, 1e-3), "{r0} off: {v}");

        let mut k = kernel(both);
        let Outcome::Built {
            volume: v,
            manifold,
        } = round_into_a_rounded_corner(&mut k, r0, 5.0)
        else {
            panic!("{r0}: refused");
        };
        assert!(manifold && near(v, off + gain, 1e-3), "{r0} on: {v}");

        // alone, the field leaves the corner's end rib to fail
        let mut k = kernel(|f| f.fillet_retry_radius_as_first = true);
        assert_eq!(
            round_into_a_rounded_corner(&mut k, r0, 5.0),
            Outcome::Refused { code: 3 }
        );
    }
    for r1 in [1.0, 2.0] {
        let mut k = Kernel::default();
        let off = round_into_a_rounded_corner(&mut k, 5.0, r1);
        let mut k = kernel(both);
        assert_eq!(round_into_a_rounded_corner(&mut k, 5.0, r1), off);
    }
}

/// `FilletErrorType::NoRoom`, the fork's error type for a blend larger than its faces.
const NO_ROOM: i32 = FilletErrorType::NoRoom as i32;

/// Blends of 6 and 9 run across the openings: SMLib builds a body that is no manifold
/// solid. With `fillet_refuse_rails_leaving_faces` they are refused (`NoRoom`);
/// the sizes that fit are unchanged.
#[test]
fn blends_larger_than_their_faces_are_refused() {
    for xsect in [0, 1] {
        for size in [6.0, 9.0] {
            let mut k = Kernel::default();
            assert!(
                matches!(
                    holed_cube_blend(&mut k, size, xsect),
                    Outcome::Built {
                        manifold: false,
                        ..
                    }
                ),
                "xsect {xsect} size {size}"
            );
            let mut k = kernel(|f| f.fillet_refuse_rails_leaving_faces = true);
            assert_eq!(
                holed_cube_blend(&mut k, size, xsect),
                Outcome::Refused { code: NO_ROOM },
                "xsect {xsect} size {size}"
            );
        }
        let mut k = Kernel::default();
        let off = holed_cube_blend(&mut k, 4.0, xsect);
        let mut k = kernel(|f| f.fillet_refuse_rails_leaving_faces = true);
        assert_eq!(holed_cube_blend(&mut k, 4.0, xsect), off);
    }
}

// ---- WR-320: blends as wide as the face they roll on ----

/// A cylinder of radius 10 and height 20, its top rim blended by `size`.
fn cylinder_rim(k: &mut Kernel, size: f64, xsect: u32) -> Outcome {
    let mut b = create_cylinder(k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0).unwrap();
    let rim = *b
        .edge_list()
        .iter()
        .find(|&&e| {
            let c = edge::curve(&b, e).unwrap().unwrap();
            let ivl = edge::interval(&b, e).unwrap();
            let p = c.evaluate_point(k, ivl.min()).unwrap();
            (p.z - 20.0).abs() < 1e-6 && (p.x.hypot(p.y) - 10.0).abs() < 1e-6
        })
        .unwrap();
    fillet(k, &mut b, &[rim], size, xsect)
}

/// Blends as wide as the face they roll on: the chamfer's cone would end in a point at the
/// top's centre, where SMLib goes on (the fork's `FacesOutlastTheirBlends`, WR-320). With
/// `fillet_refuse_face_consuming_blends` a chamfer of 10 on the top rim of a cylinder of
/// radius 10 is refused (`NoRoom`). A round of 10 closes the top as a hemisphere
/// tangent to it, as the fork's amendment (tangency-labs/smlib `8bffb23`) lets it, and
/// builds either way, as does everything below the full width.
#[test]
fn chamfer_as_wide_as_its_face_is_refused() {
    let mut k = kernel(|f| f.fillet_refuse_face_consuming_blends = true);
    assert_eq!(
        cylinder_rim(&mut k, 10.0, 0),
        Outcome::Refused { code: NO_ROOM }
    );
    let mut k = Kernel::default();
    assert!(!matches!(
        cylinder_rim(&mut k, 10.0, 0),
        Outcome::Refused { code: NO_ROOM }
    ));

    // the round of 10: the cylinder (pi 100 10) and a hemisphere (2/3 pi 1000)
    let hemisphere = std::f64::consts::PI * (1000.0 + 2000.0 / 3.0);
    for on in [false, true] {
        let mut k = kernel(|f| f.fillet_refuse_face_consuming_blends = on);
        let Outcome::Built { volume, manifold } = cylinder_rim(&mut k, 10.0, 1) else {
            panic!("on {on}: refused");
        };
        assert!(
            manifold && near(volume, hemisphere, 5e-3),
            "on {on}: {volume}"
        );
        for xsect in [0, 1] {
            assert!(
                matches!(
                    cylinder_rim(&mut k, 9.99, xsect),
                    Outcome::Built { manifold: true, .. }
                ),
                "on {on} xsect {xsect}"
            );
        }
    }
}
