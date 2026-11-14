//! Regression tests for tangency-labs/solidcore#2972: a body's volume sign is read from a
//! certified enclosure `[lo, hi]` of its volume, not from a quadrature estimate.
//!
//! - A thin quarter shell (radius 1e4, thickness 1e-3, length 10) is a valid solid whose
//!   volume, 157.08, lies below the estimate's error at the sign test's accuracy (the
//!   estimate at 1e-3 reads -180.05): its enclosure holds the closed form and is positive,
//!   and `is_outward_solid` calls it a solid.
//! - A box with one edge rounded (radius 3) is enclosed to a millionth of its volume,
//!   around the closed form `24000 - 40 * 9 * (1 - pi / 4)`, on the default kernel and with
//!   the rounds' definitions on.
//! - A box, and the same box with its material outside (every face's material side
//!   swapped), decide positive and negative with enclosures holding +120 and -120; a
//!   sphere (seam and poles) holds `4/3 pi r^3`.
//! - The same box with one side's surface moved out by 1e-3 and its trims kept: the faces
//!   no longer meet, and the closed surface the enclosure bounds (the faces and the strips
//!   across their gaps) holds 120 + 30e-3, which the faces alone do not.
//! - Two unit cubes touching along an edge (their union: four face sides at that edge,
//!   paired by sense) are enclosed around 2.
//! - A G2 round of the box edge (its net rational of degree 7 across, whose weights an
//!   interval proves positive only over small boxes) decides, around the round's volume:
//!   as a bare net, and as a procedural blend whose net is certified within a bound above 0
//!   of its definition, read on its net (#3170 widens by that bound).
//! - The quarter shell R 10, t 1e-6 passes the check, but its faces' boundaries lie apart
//!   by about its thickness: the sign is undecided, early (the strips between them hold 0
//!   whatever the cells become), and `is_outward_solid` reads that as "not a solid". A box
//!   with an offset-surface face cannot be read at all: an error, not a sign.
//! - `nurb_fillet.smb` (a sphere with a pocket) decides positive: its sphere face's loop
//!   closes along the pole, from trim ends a unit in the last place off the net's last
//!   knot, a segment whose 3D length is of the order of rounding.
#![allow(clippy::unwrap_used, clippy::panic)]

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_section::section::GN_THUMBWEIGHT_EQUAL_AREA;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::{KernelOptions, UpstreamFixes};
use solidcore::mass::enclosure::EnclosureStop;
use solidcore::mass::enclosure::{
    enclose_volume, volume_sign, VolumeEnclosure, VolumeSign, CELL_BUDGET,
};
use solidcore::math::{Point3, Vec3, PI};
use solidcore::primitive::api_primitives::{create_box, create_cylindrical_box, create_sphere};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::surface::offset_surface::OffsetSurface;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::is_outward_solid;
use solidcore::topology::brep::p1::{faces, regions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::SolidError;
use solidcore::{Brep, Kernel};

/// The quarter shell between radii `r` and `r + t` about Z, length 10.
fn quarter_shell(k: &mut Kernel, r: f64, t: f64) -> Brep {
    create_cylindrical_box(k, Point3::new(0.0, 0.0, 0.0), 10.0, r, r + t, 0.0, 90.0).unwrap()
}

fn shell_volume(r: f64, t: f64) -> f64 {
    PI / 4.0 * ((r + t) * (r + t) - r * r) * 10.0
}

/// The box 40 x 30 x 20 with its edge through (20, 30, 20) rounded at radius 3.
fn rounded_box(k: &mut Kernel) -> Brep {
    rounded_box_by(k, 1, 1, 1.0)
}

/// [`rounded_box`] with `fillet_edges`' section kind, order and thumbweight.
fn rounded_box_by(k: &mut Kernel, kind: u32, order: u32, weight: f64) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 40.0, 30.0, 20.0).unwrap();
    let q = Point3::new(20.0, 30.0, 20.0);
    let e = b
        .edge_list()
        .iter()
        .copied()
        .find(|&e| edge_closest_point(k, &b, e, q).is_ok_and(|c| c.closest.distance < 1e-9))
        .unwrap();
    fillet_edges(k, &mut b, &[e], 3.0, kind, order, weight).unwrap();
    b
}

/// Every fix on but the G2 one, as SolidCraft runs: the round is an exact round, a
/// definition with bound 0.
fn definitions_kernel() -> Kernel {
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

fn holds(e: &VolumeEnclosure, v: f64) -> bool {
    e.lo <= v && v <= e.hi
}

#[test]
fn a_thin_shell_below_the_estimates_error_decides_positive() {
    for (r, t) in [(1e4, 1e-3), (10.0, 1e-4)] {
        let mut k = Kernel::default();
        let mut b = quarter_shell(&mut k, r, t);
        let v = shell_volume(r, t);
        match volume_sign(&k, &mut b).unwrap() {
            VolumeSign::Positive(e) => assert!(holds(&e, v), "r {r} t {t}: {e:?} misses {v}"),
            other => panic!("r {r} t {t}: {other:?}, volume {v}"),
        }
        // the solid test reads the same sign: on the estimate (-180.05 at r 1e4) it was false
        let mut b = quarter_shell(&mut k, r, t);
        assert!(is_outward_solid(&mut k, &mut b).unwrap(), "r {r} t {t}");
    }
}

/// The quarter shell at r 1e4, t 1e-3 is a solid facing out. Its faces are checked with
/// their UV trims made (#15040); two of its loops (area 15.7 against a box of 1e8) came
/// out zero area while `compute_loop_orientation` compared a sampled polygon's area with
/// its box's, and are oriented from their certified signed area (#15107).
#[test]
fn the_far_thin_shell_is_an_outward_solid() {
    let mut k = Kernel::default();
    let mut b = quarter_shell(&mut k, 1e4, 1e-3);
    assert!(is_outward_solid(&mut k, &mut b).unwrap());
}

/// The far thin shell's loops are oriented by a decided sign, not by the absence of a
/// zero-area report: every face's outer loop is `Same` on its UV trims, and run backwards
/// it is `Opposite`, and the body check then fails Loop Orientation (a reversed loop near
/// the shell is still caught).
#[test]
fn the_far_thin_shells_loops_reversed_are_caught() {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::face::p2::loops;
    use solidcore::topology::face::p3::compute_loop_orientation;
    use solidcore::topology::loopuse::flip_loop_orientation;
    let mut k = Kernel::default();
    let count = faces(&quarter_shell(&mut k, 1e4, 1e-3), None)
        .unwrap()
        .len();
    assert_eq!(count, 6);
    for i in 0..count {
        let mut b = quarter_shell(&mut k, 1e4, 1e-3);
        let f = faces(&b, None).unwrap()[i];
        let l = loops(&b, f, false, None).unwrap()[0];
        let o = compute_loop_orientation(&k, &mut b, f, l, None, false).unwrap();
        assert_eq!(o, OrientType::Same, "face {i}");
        flip_loop_orientation(&mut b, l).unwrap();
        let o = compute_loop_orientation(&k, &mut b, f, l, None, false).unwrap();
        assert_eq!(o, OrientType::Opposite, "face {i} reversed");
        let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
        assert!(
            c.lines().iter().any(|l| l.contains("Loop Orientation")),
            "face {i} reversed: {:?}",
            c.lines()
        );
    }
}

#[test]
fn a_rounded_box_is_enclosed_around_its_closed_form() {
    let v = 24000.0 - 40.0 * 9.0 * (1.0 - PI / 4.0);
    for (name, mut k) in [
        ("default", Kernel::default()),
        ("definitions", definitions_kernel()),
    ] {
        let mut b = rounded_box(&mut k);
        let e = enclose_volume(&k, &mut b, 1e-6 * v).unwrap();
        assert!(holds(&e, v), "{name}: {e:?} misses {v}");
        assert!(e.width() <= 1e-6 * v, "{name}: {e:?} wider than 1e-6 V");
        let mut b = rounded_box(&mut k);
        let s = volume_sign(&k, &mut b).unwrap();
        assert!(
            matches!(s, VolumeSign::Positive(e) if holds(&e, v)),
            "{name}: {s:?}"
        );
    }
}

#[test]
fn a_box_and_its_inside_out_twin_decide_their_signs() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(1.0, 2.0, 3.0), 4.0, 5.0, 6.0).unwrap();
    let e = enclose_volume(&k, &mut b, 1e-9).unwrap();
    assert!(holds(&e, 120.0) && e.width() <= 1e-9, "{e:?}");
    // the material outside: every face's material side is the other one
    for r in regions(&b, None).unwrap() {
        let g = b.region_mut(r).unwrap();
        g.is_void = 1 - g.is_void;
    }
    match volume_sign(&k, &mut b).unwrap() {
        VolumeSign::Negative(e) => assert!(holds(&e, -120.0), "{e:?}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_sphere_is_enclosed_across_its_seam_and_poles() {
    let mut k = Kernel::default();
    let mut b = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0).unwrap();
    let v = 4.0 / 3.0 * PI * 8.0;
    let e = enclose_volume(&k, &mut b, 1e-6).unwrap();
    assert!(holds(&e, v) && e.width() <= 1e-6, "{e:?} misses {v}");
}

#[test]
fn a_gap_between_faces_is_closed_by_its_strips() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(1.0, 2.0, 3.0), 4.0, 5.0, 6.0).unwrap();
    // the side x = 5 (5 x 6) moved out by delta, its trims and its neighbours kept
    let delta = 1e-3;
    let side = faces(&b, None)
        .unwrap()
        .into_iter()
        .find(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            s.evaluate_point(&k, face.uv_domain.mid())
                .is_ok_and(|p| (p.x - 5.0).abs() < 1e-12)
        })
        .unwrap();
    let mut shift = Axis2Placement::default();
    shift.translate(Vec3::new(delta, 0.0, 0.0));
    b.face_mut(side)
        .unwrap()
        .surface
        .as_mut()
        .unwrap()
        .transform(&k, &shift, None)
        .unwrap();
    let v = 120.0 + 30.0 * delta;
    let e = enclose_volume(&k, &mut b, 1.0).unwrap();
    assert!(holds(&e, v) && e.counts.strips > 0, "{e:?} misses {v}");
    assert!(matches!(
        volume_sign(&k, &mut b).unwrap(),
        VolumeSign::Positive(_)
    ));
}

/// `upstream/TestFiles/pt_TestFiles/Solids/nurb_fillet.smb`, read and healed; `None` where
/// the upstream test files are not present.
fn nurb_fillet(k: &mut Kernel) -> Option<Brep> {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::geom::object::ObjectRef;
    use solidcore::topology::brep::p10::TopologyEnv;
    use solidcore::topology::brep::p7::read_from_file;
    use solidcore::topology::database_io::FileType;
    use solidcore::topology::topology::NotifyRecords;
    use solidcore::EntityRef;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../upstream/TestFiles/pt_TestFiles/Solids/nurb_fillet.smb");
    let Ok(bytes) = std::fs::read(&path) else {
        assert!(
            std::env::var_os("SOLIDCORE_REQUIRE_UPSTREAM").is_none(),
            "SOLIDCORE_REQUIRE_UPSTREAM is set and the upstream test file is missing: {}",
            path.display()
        );
        return None;
    };
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 50,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.use_arena_naming().unwrap();
    Some(b)
}

#[test]
fn a_sphere_closed_along_its_pole_decides() {
    let mut k = Kernel::default();
    let Some(mut b) = nurb_fillet(&mut k) else {
        return;
    };
    let s = volume_sign(&k, &mut b).unwrap();
    assert!(matches!(s, VolumeSign::Positive(_)), "{s:?}");
}

#[test]
fn solids_touching_along_an_edge_pair_their_sides() {
    let mut k = Kernel::default();
    let a = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    let c = create_box(&mut k, Point3::new(1.0, 1.0, 0.0), 1.0, 1.0, 1.0).unwrap();
    let mut b = boolean_union(&mut k, a, c).unwrap();
    let e = enclose_volume(&k, &mut b, 1e-9).unwrap();
    assert!(holds(&e, 2.0) && e.width() <= 1e-9, "{e:?}");
}

#[test]
fn a_g2_round_decides_around_its_volume() {
    // the default kernel: the round a bare net
    let bare = Kernel::default();
    let v = 24000.0 - 40.0 * 9.0 * (1.0 - PI / 4.0);
    for (name, mut k, certified) in [
        ("bare net", bare, false),
        ("definition", definitions_kernel(), true),
    ] {
        let mut b = rounded_box_by(&mut k, 2, 2, GN_THUMBWEIGHT_EQUAL_AREA);
        // with the definitions on, the round is a procedural blend whose net is certified
        // within a bound above 0: it is read on its net
        let bounds: Vec<f64> = faces(&b, None)
            .unwrap()
            .into_iter()
            .filter_map(|f| {
                let s = b.face(f).unwrap().surface.as_ref().unwrap();
                s.procedural_blend().map(|d| d.bound())
            })
            .collect();
        if certified {
            assert!(bounds.iter().any(|&e| e > 0.0), "{name}: {bounds:?}");
        } else {
            assert!(bounds.is_empty(), "{name}: {bounds:?}");
        }
        let e = enclose_volume(&k, &mut b, 1e-3 * v).unwrap();
        assert!(
            holds(&e, v) && e.width() <= 1e-3 * v,
            "{name}: {e:?} misses {v}"
        );
        let mut b = rounded_box_by(&mut k, 2, 2, GN_THUMBWEIGHT_EQUAL_AREA);
        let s = volume_sign(&k, &mut b).unwrap();
        assert!(matches!(s, VolumeSign::Positive(_)), "{name}: {s:?}");
    }
}

#[test]
fn a_gap_as_large_as_the_volume_leaves_the_sign_undecided() {
    let mut k = Kernel::default();
    let shell = |k: &mut Kernel| quarter_shell(k, 10.0, 1e-6);
    let mut b = shell(&mut k);
    let c = check_brep(&mut k, &mut b, CheckLevel::Level2).unwrap();
    assert!(c.lines().is_empty(), "{:?}", c.lines());
    let v = shell_volume(10.0, 1e-6);
    let mut b = shell(&mut k);
    match volume_sign(&k, &mut b).unwrap() {
        // undecided long before the budget: the strips settle
        VolumeSign::Undecided(e) => {
            assert!(holds(&e, v) && e.counts.cells < CELL_BUDGET / 4, "{e:?}");
        }
        other => panic!("{other:?}"),
    }
    // undecided is "not a solid" on purpose
    let mut b = shell(&mut k);
    assert!(!is_outward_solid(&mut k, &mut b).unwrap());
}

#[test]
fn a_face_without_a_nurbs_form_is_an_error_not_a_sign() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 4.0, 5.0, 6.0).unwrap();
    // one side's plane as an offset surface at distance 0: the same box
    let f = faces(&b, None).unwrap()[0];
    let base = b.face(f).unwrap().surface.as_ref().unwrap().clone();
    let off = OffsetSurface::new(&k, 0.0, base, true, 0).unwrap();
    b.face_mut(f).unwrap().surface = Some(Surface::from(off));
    let r = volume_sign(&k, &mut b);
    assert!(matches!(r, Err(SolidError::Unsupported { .. })), "{r:?}");
}

/// The solid test names the test a body fails (#15035), the first in the order the tests run:
/// a sheet is not manifold (and not closed solids, for the touching test); a box with a vertex
/// tolerance 1000 times the body's, and a box with every face flipped, fail the pointer check;
/// the quarter horn torus of #1954, its internal pole read as upstream reads it, fails
/// `AssertValid`; the box with its material outside is proved inside out; the quarter
/// shell R 10, t 1e-6 leaves its sign undecided. A box passes every test.
#[test]
fn the_solid_test_names_the_test_a_body_fails() {
    use solidcore::primitive::api_primitives::create_plane;
    use solidcore::topology::brep::p2::is_manifold_solid;
    use solidcore::topology::brep::{
        outward_solid_failure, outward_touching_solids_failure, SolidTest,
    };
    use solidcore::topology::face::p1::flip_face_orientation;
    let mut k = Kernel::default();
    let boxed = |k: &mut Kernel| create_box(k, Point3::new(1.0, 2.0, 3.0), 4.0, 5.0, 6.0).unwrap();

    let mut b = boxed(&mut k);
    assert_eq!(outward_solid_failure(&mut k, &mut b).unwrap(), None);
    assert_eq!(
        outward_touching_solids_failure(&mut k, &mut b).unwrap(),
        None
    );

    let mut sheet = create_plane(&mut k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0).unwrap();
    assert_eq!(
        outward_solid_failure(&mut k, &mut sheet).unwrap(),
        Some(SolidTest::NotManifold)
    );
    assert_eq!(
        outward_touching_solids_failure(&mut k, &mut sheet).unwrap(),
        Some(SolidTest::NotClosedSolids)
    );

    let mut loose = boxed(&mut k);
    let v = loose.vertex_list()[0];
    let tolerance = loose.tolerance() * 1000.0;
    loose.vertex_mut(v).unwrap().tolerance = tolerance;
    assert!(is_manifold_solid(&loose).unwrap());
    assert_eq!(
        outward_solid_failure(&mut k, &mut loose).unwrap(),
        Some(SolidTest::PointersInvalid)
    );

    let mut flipped = boxed(&mut k);
    for f in flipped.face_list().to_vec() {
        flip_face_orientation(&mut flipped, f).unwrap();
    }
    assert_eq!(
        outward_solid_failure(&mut k, &mut flipped).unwrap(),
        Some(SolidTest::PointersInvalid)
    );

    // the quarter horn torus of #1954 (a circle of radius 5 revolved a quarter turn about
    // an axis through its seam vertex, capped): with `internal_pole_row_through_knot` off,
    // `AssertValid` reports an internal pole (B-spline surface test 2) after a clean
    // pointer check
    {
        use solidcore::curve::curve::Curve;
        use solidcore::curve::ellipse::Ellipse;
        use solidcore::geom::extent1d::Extent1d;
        use solidcore::sweep::api_brep::create_rotational_sweep;
        let mut k = Kernel::default();
        k.fixes.internal_pole_row_through_knot = false;
        let frame = Axis2Placement::new(Point3::new(0.0, 0.0, 20.0), Vec3::X, Vec3::Y);
        let circle = Curve::from(
            Ellipse::create_canonical_circle(
                &k,
                &frame,
                5.0,
                Some(&Extent1d::from_min_max(0.0, 360.0)),
                None,
            )
            .unwrap(),
        );
        let mut horn = create_rotational_sweep(
            &mut k,
            &[Some(&circle)],
            Point3::new(5.0, 0.0, 20.0),
            Vec3::Y,
            90.0,
            true,
        )
        .unwrap();
        assert_eq!(
            outward_solid_failure(&mut k, &mut horn).unwrap(),
            Some(SolidTest::Invalid)
        );
    }

    let mut inside_out = boxed(&mut k);
    for r in regions(&inside_out, None).unwrap() {
        let g = inside_out.region_mut(r).unwrap();
        g.is_void = 1 - g.is_void;
    }
    assert_eq!(
        outward_solid_failure(&mut k, &mut inside_out).unwrap(),
        Some(SolidTest::VolumeNegative)
    );

    let mut thin = quarter_shell(&mut k, 10.0, 1e-6);
    assert_eq!(
        outward_solid_failure(&mut k, &mut thin).unwrap(),
        Some(SolidTest::VolumeUndecided)
    );
}

/// #15173: an enclosure asked narrower than the part no cut narrows (the caps' and loops'
/// rounding terms, and strips that settle) is still refined, and a tighter request is never
/// answered wider. For the sphere of radius 2, asking 1e-11 returned the 8 starting cells,
/// 3e4 wide, while asking 1e-10 returned 5.3e-10.
#[test]
fn a_width_below_the_floor_is_refined_not_given_up() {
    let mut k = Kernel::default();
    let v = 4.0 / 3.0 * PI * 8.0;
    let mut last = f64::INFINITY;
    for width in [1e-6, 1e-8, 1e-11] {
        let mut b = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 2.0).unwrap();
        let e = enclose_volume(&k, &mut b, width).unwrap();
        assert!(holds(&e, v), "{width}: {e:?} misses {v}");
        assert!(e.width() <= last, "{width}: {e:?} wider than {last}");
        // as narrow as asked, or it says why not
        assert_eq!(
            e.stop == EnclosureStop::Reached,
            e.width() <= width,
            "{width}: {e:?}"
        );
        last = e.width();
    }
    assert!(last <= 1e-9, "1e-11: {last}");
}

/// #15173: refining past where the rounding grows faster than the cells shrink does not
/// widen the enclosure: the narrowest seen is kept. On a box, spending the whole budget took
/// the width from 1.7e-10 (asked 1e-9) to 1.5e-9.
#[test]
fn spending_the_budget_never_widens_the_enclosure() {
    let mut k = Kernel::default();
    let boxed = |k: &mut Kernel| create_box(k, Point3::new(1.0, 2.0, 3.0), 4.0, 5.0, 6.0).unwrap();
    let mut b = boxed(&mut k);
    let asked = enclose_volume(&k, &mut b, 1e-9).unwrap();
    let mut b = boxed(&mut k);
    let spent = enclose_volume(&k, &mut b, f64::MIN_POSITIVE).unwrap();
    assert!(holds(&asked, 120.0), "{asked:?}");
    assert!(holds(&spent, 120.0), "{spent:?}");
    assert_eq!(asked.stop, EnclosureStop::Reached, "{asked:?}");
    assert_ne!(spent.stop, EnclosureStop::Reached, "{spent:?}");
    assert!(
        spent.width() <= asked.width(),
        "{spent:?} wider than {asked:?}"
    );
}
