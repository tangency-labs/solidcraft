//! The boolean family on bodies whose answers are known.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test

use std::f64::consts::{PI, TAU};
use std::io::Read as _;

use serde_json::Value;
use solidcore_ops::boolean::{boolean, imprint, union_all};
use solidcore_ops::sweep::{extrude, revolve, Curve, DraftCorner};
use solidcore_ops::{
    make_box, make_cone, make_sphere, session, Body, BooleanOp, CheckLevel, Error,
};
use solidcore_record::brep::Brep as Record;

fn all_fixes() {
    session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
}

fn volume(b: &Body) -> f64 {
    b.volume(1e-8).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-7 * b.abs().max(1.0)
}

#[test]
fn two_overlapping_boxes() {
    all_fixes();
    let a = || make_box([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap();
    let b = || make_box([1.0, 1.0, 1.0], [2.0, 2.0, 2.0]).unwrap();
    let u = boolean(a(), b(), BooleanOp::Union).unwrap().unwrap();
    assert!(near(volume(&u), 15.0), "{}", volume(&u));
    assert!(u.is_manifold_solid().unwrap());
    let d = boolean(a(), b(), BooleanOp::Subtract).unwrap().unwrap();
    assert!(near(volume(&d), 7.0), "{}", volume(&d));
    let i = boolean(a(), b(), BooleanOp::Intersect).unwrap().unwrap();
    assert!(near(volume(&i), 1.0), "{}", volume(&i));
}

#[test]
fn apart_boxes_intersect_in_nothing() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    assert!(boolean(a, b, BooleanOp::Intersect).unwrap().is_none());
}

#[test]
fn a_list_unions_level_by_level() {
    all_fixes();
    let boxes = (0..5)
        .map(|i| make_box([f64::from(i), 0.0, 0.0], [1.5, 1.0, 1.0]).unwrap())
        .collect();
    let u = union_all(boxes).unwrap();
    assert!(near(volume(&u), 5.5), "{}", volume(&u));
    assert_eq!(union_all(Vec::new()).unwrap_err(), Error::Argument);
}

#[test]
fn boxes_touching_along_an_edge_union_to_a_closed_body() {
    all_fixes();
    let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let b = make_box([1.0, 1.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
    assert!(near(volume(&u), 2.0), "{}", volume(&u));
    assert_eq!(u.solid_count().unwrap(), 2);
}

/// A 20 mm box with a closed void at its middle: a 10 mm cube void, or a sphere void of radius 5.
fn box_with_void(cube: bool) -> Body {
    let tool = if cube {
        make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap()
    } else {
        make_sphere([10.0, 10.0, 10.0], 5.0).unwrap()
    };
    let b = make_box([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]).unwrap();
    boolean(b, tool, BooleanOp::Subtract).unwrap().unwrap()
}

#[test]
fn a_union_that_fills_a_void_is_the_whole_box() {
    all_fixes();
    // Every face of the void is deleted, and the region they joined is material. The tool's
    // faces lie on the void's, or inside the box's material, so none of them is copied in.
    let fillers = [
        (true, make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap()),
        (true, make_box([4.0, 4.0, 4.0], [12.0, 12.0, 12.0]).unwrap()),
        (true, make_box([5.0, 4.0, 4.0], [12.0, 12.0, 12.0]).unwrap()),
        (false, make_sphere([10.0, 10.0, 10.0], 5.0).unwrap()),
        (false, make_sphere([10.0, 10.0, 10.0], 6.0).unwrap()),
        (
            false,
            make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap(),
        ),
    ];
    for (n, (cube, filler)) in fillers.into_iter().enumerate() {
        let u = boolean(box_with_void(cube), filler, BooleanOp::Union)
            .unwrap()
            .unwrap();
        assert!(near(volume(&u), 8000.0), "filler {n}: {}", volume(&u));
        assert_eq!(u.counts().unwrap(), [6, 12, 8], "filler {n}");
        assert!(u.is_manifold_solid().unwrap(), "filler {n}");
        assert!(u.to_record().is_ok(), "filler {n}");
    }
    // A filler that leaves part of the void keeps that part: 20³ − 10 · 10 · 1.
    let part = make_box([6.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap();
    let u = boolean(box_with_void(true), part, BooleanOp::Union)
        .unwrap()
        .unwrap();
    assert!(near(volume(&u), 7900.0), "{}", volume(&u));
}

#[test]
fn a_cube_on_a_void_is_kept_out_by_a_difference_and_shares_nothing_with_it() {
    all_fixes();
    let cube = || make_box([5.0, 5.0, 5.0], [10.0, 10.0, 10.0]).unwrap();
    // The faces bound material on opposite sides: a difference keeps them, so the box is
    // unchanged, and an intersection deletes them and leaves nothing.
    let d = boolean(box_with_void(true), cube(), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    assert!(near(volume(&d), 7000.0), "{}", volume(&d));
    assert_eq!(d.counts().unwrap(), [12, 24, 16]);
    assert!(boolean(box_with_void(true), cube(), BooleanOp::Intersect)
        .unwrap()
        .is_none());
}

/// A cylinder lying along a box's edge, tangent there to the box's face. The two faces leave the
/// edge together and part as the square of the distance over the cylinder's diameter, by less
/// the larger the cylinder, so the fixed steps off the edge took a large one's faces for
/// parallel (a zero sector, from 10 times the size) and for one surface (coincident faces,
/// from 100 times). The union is the two solids touching along that line, at 1 to 1000 times
/// the size, in either order.
#[test]
fn a_large_cylinder_resting_on_a_box_edge_unions() {
    all_fixes();
    for k in [1.0, 10.0, 40.0, 100.0, 1000.0] {
        let cylinder =
            || solidcore_ops::make_cylinder([0.0, 15.0 * k, 2.5 * k], 7.5 * k, 25.0 * k).unwrap();
        let block = || make_box([0.0, 0.0, 0.0], [5.0 * k, 7.5 * k, 5.0 * k]).unwrap();
        let want = (PI * 7.5 * 7.5 * 25.0 + 5.0 * 7.5 * 5.0) * (k * k * k);
        for (a, b) in [(cylinder(), block()), (block(), cylinder())] {
            let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
            let report = u.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{k}: {}", report.text);
            assert_eq!(u.solid_count().unwrap(), 2, "{k}");
            assert!(
                near(volume(&u), want),
                "{k}: {}, closed form {want}",
                volume(&u)
            );
            u.to_record().unwrap();
        }
    }
}

/// Two equal cylinders side by side, touching along a line. Their faces leave the line
/// together and part as the square of the distance over the radius, so the fixed steps off
/// the line took two large ones for one surface, and the union failed (100 times the size)
/// or the intersection was a whole cylinder (1000 times). The union is the two solids
/// touching along the line and nothing is common, in either order.
#[test]
fn two_large_cylinders_touching_along_a_line_union() {
    all_fixes();
    for k in [1.0, 100.0, 1000.0] {
        let left = || solidcore_ops::make_cylinder([0.0; 3], 7.5 * k, 25.0 * k).unwrap();
        let right =
            || solidcore_ops::make_cylinder([15.0 * k, 0.0, 2.5 * k], 7.5 * k, 25.0 * k).unwrap();
        let want = 2.0 * PI * 7.5 * 7.5 * 25.0 * (k * k * k);
        for (a, b) in [(left(), right()), (right(), left())] {
            let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
            let report = u.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{k}: {}", report.text);
            assert_eq!(u.solid_count().unwrap(), 2, "{k}");
            assert!(
                near(volume(&u), want),
                "{k}: {}, closed form {want}",
                volume(&u)
            );
        }
        assert!(
            boolean(left(), right(), BooleanOp::Intersect)
                .unwrap()
                .is_none(),
            "{k}"
        );
    }
}

#[test]
fn an_imprint_cuts_the_faces_the_tool_crosses() {
    all_fixes();
    let body = make_box([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]).unwrap();
    let tool = make_box([1.0, -1.0, -1.0], [4.0, 4.0, 4.0]).unwrap();
    let (r, changed) = imprint(body, tool).unwrap();
    assert!(changed);
    assert!(r.counts().unwrap()[0] > 6, "{:?}", r.counts());
    assert!(r.is_manifold_solid().unwrap());
    // a tool apart from the body cuts nothing: the body comes back, not dropped
    let body = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let tool = make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
    let (r, changed) = imprint(body, tool).unwrap();
    assert!(!changed);
    assert_eq!(r.counts().unwrap(), [6, 12, 8]);
    assert!(near(volume(&r), 1.0), "{}", volume(&r));
}

/// The kernel rig's case `booleans/<case>`: its entry, and its operand bodies by name, read
/// under the corpus's fix set.
fn corpus_case(case: &str) -> (Value, impl Fn(&str) -> Body) {
    let corpus = format!(
        "{}/../solidcore/tests/kernel_cases",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixes: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(format!("{corpus}/fixes.json")).unwrap())
            .unwrap();
    session::set_fixes(fixes.iter().map(String::as_str)).unwrap();
    let entry: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{corpus}/booleans/{case}.json")).unwrap(),
    )
    .unwrap();
    let gz = std::fs::File::open(format!("{corpus}/booleans/{case}.bodies.json.gz")).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_string(&mut text)
        .unwrap();
    let records: Value = serde_json::from_str(&text).unwrap();
    let trusted = entry["bodies"].clone();
    let body = move |name: &str| {
        let record: Record = serde_json::from_value(records[name].clone()).unwrap();
        Body::from_record(&record, trusted[name]["trusted"].as_bool().unwrap())
            .unwrap()
            .0
    };
    (entry, body)
}

/// The case `booleans/13462-rod-cut-by-two-counter-wound-channel-coils`: a rod cut by two
/// one-turn screw sweeps of one channel profile, one of each hand, from its top face. The
/// profile's radial walls meet the rod's cylinder, so each sweep's rail there is a helix on it,
/// and the two rails cross. The crossing is found more than once, a few 1e-12 apart in the
/// rail's parameter; the new edge along the second rail took the interval its UV curve's trim
/// had snapped onto a knot of its own, 2.4e-12 before the 3D curve's end (`SmEdge` test 2).
/// Its operands, and the result graded: one valid solid of the case's closed form (its
/// `expected.method`). The volume is measured to 1e-7, ten times finer than the case's
/// tolerance.
fn counter_wound_sweeps() -> (impl Fn(&str) -> Body, impl Fn(&str, &Body)) {
    let (entry, body) = corpus_case("13462-rod-cut-by-two-counter-wound-channel-coils");
    let want = entry["expected"]["volume_mm3"].as_f64().unwrap();
    let rel = entry["expected"]["tolerance"]["rel"].as_f64().unwrap();
    let graded = move |how: &str, r: &Body| {
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "cut {how}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "cut {how}");
        let v = r.volume(1e-7).unwrap();
        assert!(
            (v - want).abs() <= rel * want,
            "cut {how}: volume {v}, closed form {want}"
        );
    };
    (body, graded)
}

/// The second sweep cut from the rod the first one cut, as SolidCraft does it and the case
/// records it. b14 is the rod cut by the right-handed sweep as the case's run read it back from
/// its record, b5 the left-handed sweep.
#[test]
fn a_rod_cut_by_two_counter_wound_channel_sweeps_is_valid() {
    let (body, graded) = counter_wound_sweeps();
    let r = boolean(body("b14"), body("b5"), BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    graded("in turn", &r);
}

/// The same rod cut by the union of the two sweeps (b4 and b5) at once: the same edge, through
/// the union first. b6 is the rod.
#[test]
#[ignore = "the sweeps' union and the cut by it take about 80 s unoptimised; the cut in turn \
            covers the same edge"]
fn a_rod_cut_by_the_union_of_two_counter_wound_channel_sweeps_is_valid() {
    let (body, graded) = counter_wound_sweeps();
    let both = union_all(vec![body("b4"), body("b5")]).unwrap();
    let r = boolean(body("b6"), both, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    graded("by the union", &r);
}

/// A dome turned about y from a quarter disc whose arc SolidCraft drew through a point written to
/// eight decimals (#13713): the arc's circle has radius 10 + 1.01e-8, and the arc starts 1.01e-9
/// rad above the x axis, so the dome's base circle lies 1.01e-8 above y = 0. Its seam lies in
/// z = 0 on +x.
fn rounded_dome() -> Body {
    let r = 10.000_000_010_132_878;
    let profile = [
        Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: r,
            start: 1.013_287_798_059_198_8e-9,
            end: 1.570_796_325_781_608_9,
        },
        Curve::Line {
            start: [0.0, r, 0.0],
            end: [0.0; 3],
        },
        Curve::Line {
            start: [0.0; 3],
            end: [r, 1.013_287_799_085_951e-8, 0.0],
        },
    ];
    revolve(&profile, [0.0; 3], [0.0, -1.0, 0.0], TAU, true).unwrap()
}

/// One valid solid of `counts` faces, edges and vertices and `want` mm³ (the dome's 1e-8 off
/// its circle moves the volume by about 1e-9 relative).
fn one_valid_solid(r: &Body, counts: [usize; 3], want: f64) {
    let report = r.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(r.solid_count().unwrap(), 1);
    assert_eq!(r.counts().unwrap(), counts);
    assert!(near(volume(r), want), "{}, closed form {want}", volume(r));
}

/// The rounded dome joined flat to a cylinder of radius 10 below it, its seam on the dome's. The
/// two seams end 1.43e-8 apart at the vertex they share, 1.01e-8 radially and 1.01e-8 along the
/// seam, and not at all across it: the result is valid (no face test 36, "Face Near missing
/// Seam"), V = 2/3·π·10³ + π·10²·15.
#[test]
fn a_dome_1e_8_off_its_circle_joins_flat_to_a_cylinder() {
    all_fixes();
    let circle = [Curve::Arc {
        center: [0.0, -15.0, 0.0],
        axis: [0.0, 1.0, 0.0],
        reference: [1.0, 0.0, 0.0],
        radius: 10.0,
        start: 0.0,
        end: TAU,
    }];
    let cylinder = extrude(
        &circle,
        [0.0, 1.0, 0.0],
        15.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let r = boolean(cylinder, rounded_dome(), BooleanOp::Union)
        .unwrap()
        .unwrap();
    one_valid_solid(&r, [3, 4, 3], 2.0 / 3.0 * PI * 1e3 + PI * 1e2 * 15.0);
}

/// The rounded dome joined to its mirror image about y = 0: the two seams end 2.03e-8 apart, along
/// the seam only. The result is a valid sphere, V = 4/3·π·10³.
#[test]
fn a_dome_1e_8_off_its_circle_joins_to_its_mirror() {
    all_fixes();
    let mirror = rounded_dome();
    mirror
        .transform(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .unwrap();
    let r = boolean(rounded_dome(), mirror, BooleanOp::Union)
        .unwrap()
        .unwrap();
    one_valid_solid(&r, [2, 3, 3], 4.0 / 3.0 * PI * 1e3);
}

/// Cancel stops the full check of a long coil cut (solidcore#3247): a 20-turn square coil cut
/// from a cylinder, the case of tangency-labs/solidcraft#3962, whose check takes seconds (most
/// of the cut's time, in the edges' curve analyses). Measured in the token's polls, not in
/// time: a counting token (`CancelToken::after_polls`) first counts the polls of the whole
/// check, `whole`; a token cancelled from a quarter of them then stops a second check there.
/// It returns `Cancelled`, and the token is polled exactly once after the poll that cancelled
/// it: by the session as the call returns, asking whether it was cancelled (`run_with`, after
/// the kernel's call). Any further poll would be work done after the cancel.
#[test]
fn the_check_of_a_long_coil_cut_stops_on_cancel() {
    use solidcore_ops::cancel::{within, CancelToken};
    use solidcore_ops::sweep::{screw_sweep, Curve};
    let corners = [
        [19.0, 0.0, 12.0],
        [32.0, 0.0, 12.0],
        [32.0, 0.0, 25.0],
        [19.0, 0.0, 25.0],
    ];
    let square: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: corners[i],
            end: corners[(i + 1) % 4],
        })
        .collect();
    let coil = screw_sweep(
        &square,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        15.24,
        20.0,
        None,
        15.24e-4,
    )
    .unwrap();
    let stock = solidcore_ops::make_cylinder([0.0; 3], 20.0, 304.5).unwrap();
    let cut = boolean(stock, coil, BooleanOp::Subtract).unwrap().unwrap();
    let probe = CancelToken::after_polls(u64::MAX);
    within(&probe, || cut.check(CheckLevel::All)).unwrap();
    let whole = probe.polls().unwrap();
    let n = whole / 4;
    // cancelled well inside the check, not at its last poll
    assert!(n >= 1 && whole >= 2 * n, "the check polls {whole} times");
    let token = CancelToken::after_polls(n);
    let r = within(&token, || cut.check(CheckLevel::All));
    eprintln!(
        "the check polls {whole} times; cancelled at {n}, polled {:?}",
        token.polls()
    );
    assert_eq!(r.map(|_| ()), Err(Error::Cancelled));
    assert_eq!(token.polls(), Some(n + 1));
}

/// #13295: a bar of L section (100 long; its step's top edge along x at y 12.527, z 19.567)
/// cut by a circle of radius 4.717 sketched on the plane at 45 degrees through that edge and
/// extruded into the bar. The sketch puts the circle's centre 7.9e-6 off the edge, so the cut
/// wall's corner on the edge, where its seam edge ends, lies that far across the wall's
/// seam: inside the vertex's 1e-5 tolerance, with no sliver beside the seam. The check
/// refused it (face test 36) for centres 1e-7 to 1e-5 off. Every offset now gives one valid
/// solid, its volume moving with the offset by no more than the wall's swept area allows.
#[test]
fn a_cut_whose_seam_edge_ends_within_tolerance_across_the_seam_is_valid() {
    all_fixes();
    let (y0, z1, yc, z2) = (
        16.423877938511588,
        13.06859205815335,
        12.527075811882755,
        19.566787029154483,
    );
    let p = [
        [0.0, 0.0, 0.0],
        [0.0, y0, 0.0],
        [0.0, y0, z1],
        [0.0, yc, z1],
        [0.0, yc, z2],
        [0.0, 0.0, z2],
    ];
    let profile: Vec<Curve> = (0..6)
        .map(|i| Curve::Line {
            start: p[i],
            end: p[(i + 1) % 6],
        })
        .collect();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let radius = 4.716_990_566_008_734;
    let cut = |d: f64| {
        let bar = extrude(
            &profile,
            [1.0, 0.0, 0.0],
            100.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        // the circle's centre `d` off the edge, in the sketch plane (normal (0, 1, 1) / sqrt 2)
        let centre = [20.4216146277246, yc + d * s, z2 - d * s];
        let circle = [Curve::Arc {
            center: centre,
            axis: [0.0, s, s],
            reference: [1.0, 0.0, 0.0],
            radius,
            start: 0.0,
            end: TAU,
        }];
        let tool = extrude(
            &circle,
            [0.0, -s, -s],
            60.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        boolean(bar, tool, BooleanOp::Subtract)
            .unwrap_or_else(|e| panic!("d {d}: {e:?}"))
            .unwrap()
    };
    let exact = volume(&cut(0.0));
    // (1e-6 off, the cut is no solid, before this change too: #14840)
    for d in [1e-9, 1e-7, 3e-7, 2e-6, 7.868_858_162e-6, 1e-5, 3e-5, 1e-4] {
        let r = cut(d);
        let report = r.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "d {d}: {}", report.text);
        assert_eq!(r.solid_count().unwrap(), 1, "d {d}");
        // the wall moves by d: the volume by at most its perimeter times its length times d
        let bound = TAU * radius * 60.0 * d + 1e-7 * exact;
        assert!(
            (volume(&r) - exact).abs() <= bound,
            "d {d}: {} vs {exact}",
            volume(&r)
        );
    }
}

/// A 120 degree sector of height 119 with its apex at the origin, its legs ending at the
/// sketch point (±74.0452, 42.75) (85.5 at 30 and 150 degrees, rounded to four decimals, so
/// each leg turns 1.636e-7 rad outward), its arc about the origin, turned `turn` degrees
/// about z.
#[allow(clippy::disallowed_methods)] // the test's operands and closed form, not ops geometry
fn sector_of_rounded_sketch(turn: f64) -> Body {
    let (x, y) = (74.0452, 42.75);
    let profile = [
        Curve::Line {
            start: [0.0; 3],
            end: [x, y, 0.0],
        },
        Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: x.hypot(y),
            start: y.atan2(x),
            end: y.atan2(-x),
        },
        Curve::Line {
            start: [-x, y, 0.0],
            end: [0.0; 3],
        },
    ];
    let b = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        119.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let (s, c) = turn.to_radians().sin_cos();
    b.transform(&[
        [c, -s, 0.0, 0.0],
        [s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    b
}

/// Two copies of the sector of a sketch whose leg points were rounded, turned 120 degrees
/// apart (SolidCraft, a case joined from three such sectors): the legs that should coincide
/// cross at 3.3e-7 rad along the z axis, so the sectors overlap in a sliver, and the arc of
/// one runs 2.8e-5 past the other's leg. The imprint splits that arc there; once the leg is
/// gone the split vertex is redundant and its two arc pieces are joined back. The join kept
/// the split as a knot, a span of 1.6e-7 of the arc whose control points coincide, and the
/// cap faces' trim curves made from it failed the check (`SmBSplineCurve` test 15, the
/// polar conversion test): the union was refused as not a solid. Joined into one span, the
/// arc is the operand's again and the union is one valid solid of 6 faces, 12 edges and 8
/// vertices, V = (2α - 2d)/2 · r² · 119 for the sector angle α = 120° + 2d and the overlap
/// 2d, d = 30° - atan2(42.75, 74.0452).
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn sectors_of_a_rounded_sketch_turned_120_degrees_union() {
    session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    let r = 74.0452_f64.hypot(42.75);
    let d = 30_f64.to_radians() - 42.75_f64.atan2(74.0452);
    let alpha = 2.0 * PI / 3.0 + 2.0 * d;
    let want = (2.0 * alpha - 2.0 * d) / 2.0 * r * r * 119.0;
    let u = boolean(
        sector_of_rounded_sketch(0.0),
        sector_of_rounded_sketch(120.0),
        BooleanOp::Union,
    )
    .unwrap()
    .unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 1);
    assert_eq!(u.counts().unwrap(), [6, 12, 8]);
    let v = volume(&u);
    let off = (v - want).abs() / want;
    assert!(!(off.is_nan() || off > 1e-9), "{v}, closed form {want}");
}

/// A 120° pie slice of height 119 symmetric about +y, turned by `turn` about z: its legs run
/// from the origin at 90° ∓ (60° + `widen`) to corners `rho` away, its arc is the circle about
/// (0, `c`) through the corners (SolidCraft draws the arc through its rounded end points, so
/// its centre can miss the apex).
#[allow(clippy::disallowed_methods)] // the test's operand geometry, not ops computing it
fn sector(widen: f64, rho: f64, c: f64, turn: f64) -> Body {
    let half = PI / 3.0 + widen;
    let p0 = [rho * (PI / 2.0 - half).cos(), rho * (PI / 2.0 - half).sin()];
    let p1 = [-p0[0], p0[1]];
    let (ct, st) = (turn.cos(), turn.sin());
    let at = |p: [f64; 2]| [ct * p[0] - st * p[1], st * p[0] + ct * p[1], 0.0];
    let profile = [
        Curve::Line {
            start: [0.0; 3],
            end: at(p0),
        },
        Curve::Arc {
            center: at([0.0, c]),
            axis: [0.0, 0.0, 1.0],
            reference: at([1.0, 0.0]),
            radius: (p0[0].powi(2) + (p0[1] - c).powi(2)).sqrt(),
            start: (p0[1] - c).atan2(p0[0]),
            end: (p1[1] - c).atan2(p1[0]),
        },
        Curve::Line {
            start: at(p1),
            end: [0.0; 3],
        },
    ];
    extrude(
        &profile,
        [0.0, 0.0, 1.0],
        119.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap()
}

/// Two pie slices side by side, the second turned by 120° + `apart` about the apex: their
/// facing walls meet at the apex edge and part linearly, `apart` · 85.5 at the arc. Within
/// the 1e-5 tolerance across the wall (`apart` <= 1e-7) the walls are one and the slices join
/// into one solid; 3.3e-7 rad (2.8e-5 at the arc) and 1e-6 rad apart they are two walls, and
/// the union is the two slices touching along the apex edge, V = 2 · r²/2 · 2π/3 · h exactly.
/// Before, the radial sector read the two walls (closer than 0.001°) as coincident and merged
/// their corner edges 2.8e-5 apart: not a closed solid. Overlapping slices (`apart` < 0) join
/// into one solid of V = r²/2 · (4π/3 + apart) · h.
#[test]
fn pie_slices_whose_walls_part_beyond_tolerance_join_along_the_apex() {
    all_fixes();
    let (r, h) = (85.5, 119.0);
    let slice = r * r / 2.0 * (2.0 * PI / 3.0) * h;
    for apart in [0.0, 1e-8, 1e-7, 3.3e-7, 1e-6, -1e-8, -1e-7, -3.3e-7, -1e-6] {
        let a = sector(0.0, r, 0.0, 0.0);
        let b = sector(0.0, r, 0.0, 2.0 * PI / 3.0 + apart);
        let u = boolean(a, b, BooleanOp::Union).unwrap().unwrap();
        let report = u.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{apart}: {}", report.text);
        let v = volume(&u);
        // the wedge between the walls, which a join within tolerance may absorb or keep
        let wedge = r * r * h * apart.abs();
        if apart > 2e-7 {
            assert_eq!(u.solid_count().unwrap(), 2, "{apart}");
            assert_eq!(u.counts().unwrap(), [10, 17, 10], "{apart}");
            assert!(near(v, 2.0 * slice), "{apart}: {v}");
        } else {
            assert_eq!(u.solid_count().unwrap(), 1, "{apart}");
            assert_eq!(u.counts().unwrap(), [6, 12, 8], "{apart}");
            let want = 2.0 * slice + r * r / 2.0 * apart.min(0.0) * h;
            let err = (v - want).abs();
            assert!(
                !(err.is_nan() || err > wedge + 1e-7 * want),
                "{apart}: {v}, closed form {want}"
            );
        }
    }
}

/// The half roll r 48..50 about the z axis, z 0..40, on the y > 0 side, its walls extruded from
/// a profile whose arcs are framed by `frame % 4` (counter-clockwise from +x or clockwise from
/// -x, about +z or -z), then mirrored onto itself in x (`frame & 4`) and in z (`frame & 8`):
/// sixteen frames of the walls' cylinders, one solid.
fn half_roll(frame: u32) -> Body {
    let (axis, reference, start, end) = match frame % 4 {
        0 => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 0.0, PI),
        1 => ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], 0.0, PI),
        2 => ([0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], -PI, 0.0),
        _ => ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0], -PI, 0.0),
    };
    let arc = |radius| Curve::Arc {
        center: [0.0; 3],
        axis,
        reference,
        radius,
        start,
        end,
    };
    let profile = [
        arc(50.0),
        Curve::Line {
            start: [-50.0, 0.0, 0.0],
            end: [-48.0, 0.0, 0.0],
        },
        arc(48.0),
        Curve::Line {
            start: [48.0, 0.0, 0.0],
            end: [50.0, 0.0, 0.0],
        },
    ];
    let b = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        40.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let sx = if frame & 4 == 0 { 1.0 } else { -1.0 };
    let (sz, tz) = if frame & 8 == 0 {
        (1.0, 0.0)
    } else {
        (-1.0, 40.0)
    };
    if frame >= 4 {
        b.transform(&[
            [sx, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, sz, 0.0],
            [0.0, 0.0, tz, 1.0],
        ])
        .unwrap();
    }
    b
}

/// The area of the disc of radius `rho` centred `c` from the origin inside the circle of radius
/// `r` about the origin (the lens of two circles).
#[allow(clippy::disallowed_methods)] // the test's reference, not ops computing geometry
fn lens(r: f64, rho: f64, c: f64) -> f64 {
    if rho <= 0.0 || c >= r + rho {
        return 0.0;
    }
    if c <= (r - rho).abs() {
        return PI * r.min(rho).powi(2);
    }
    let a1 = ((c * c + r * r - rho * rho) / (2.0 * c * r))
        .clamp(-1.0, 1.0)
        .acos();
    let a2 = ((c * c + rho * rho - r * r) / (2.0 * c * rho))
        .clamp(-1.0, 1.0)
        .acos();
    let k = ((-c + r + rho) * (c + r - rho) * (c - r + rho) * (c + r + rho))
        .max(0.0)
        .sqrt();
    r * r * a1 + rho * rho * a2 - 0.5 * k
}

/// `f` integrated over `[a, b]` by tanh-sinh quadrature at step `h` (its error falls doubly
/// exponentially in `1/h`, the endpoint singularities of `f` included).
#[allow(clippy::disallowed_methods)] // the test's reference, not ops computing geometry
fn tanh_sinh(f: impl Fn(f64) -> f64, a: f64, b: f64, h: f64) -> f64 {
    let (mid, half) = ((a + b) / 2.0, (b - a) / 2.0);
    let mut sum = 0.0;
    for k in 0..10_000 {
        let t = f64::from(k) * h;
        let u = PI / 2.0 * t.sinh();
        let (x, w) = (u.tanh(), PI / 2.0 * t.cosh() / u.cosh().powi(2));
        if w < 1e-300 || 1.0 - x < 1e-17 {
            break;
        }
        sum += w * f(mid + half * x);
        if k > 0 {
            sum += w * f(mid - half * x);
        }
    }
    sum * h * half
}

/// The r 6 ball about `center`, its seam meridian turned `turn` radians about its axis from +x.
#[allow(clippy::disallowed_methods)] // the test's operand, not ops computing geometry
fn ball(center: [f64; 3], turn: f64) -> Body {
    let b = make_sphere([0.0; 3], 6.0).unwrap();
    let (s, c) = turn.sin_cos();
    let [x, y, z] = center;
    b.transform(&[
        [c, s, 0.0, 0.0],
        [-s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [x, y, z, 1.0],
    ])
    .unwrap();
    b
}

/// The volume of the r 6 ball about (0, `c`, 20) inside the radius `radius(z)` about the z axis:
/// the integral over z of the lens areas of the ball's section (radius √(36 - (z-20)²), centred
/// `c` from the axis) within that radius, by tanh-sinh quadrature over the pieces between the
/// heights where a lens turns whole or empty (the section's radius is |radius(z) - c|, found by
/// bisection). Asserts that steps 1/32 and 1/64 agree to 1e-13 relative.
#[allow(clippy::disallowed_methods)] // the test's reference, not ops computing geometry
fn ball_inside(radius: impl Fn(f64) -> f64, c: f64) -> f64 {
    let section = |z: f64| (36.0 - (z - 20.0) * (z - 20.0)).max(0.0).sqrt();
    let kink = |z: f64| section(z) - (radius(z) - c).abs();
    let mut cuts = vec![14.0];
    let n = 1200;
    for i in 0..n {
        let (mut a, mut b) = (
            14.0 + 12.0 * f64::from(i) / f64::from(n),
            14.0 + 12.0 * f64::from(i + 1) / f64::from(n),
        );
        if (kink(a) > 0.0) != (kink(b) > 0.0) {
            for _ in 0..200 {
                let m = (a + b) / 2.0;
                if (kink(a) > 0.0) == (kink(m) > 0.0) {
                    a = m;
                } else {
                    b = m;
                }
            }
            cuts.push((a + b) / 2.0);
        }
    }
    cuts.push(26.0);
    let v = |h: f64| {
        cuts.windows(2)
            .map(|w| tanh_sinh(|z| lens(radius(z), section(z), c), w[0], w[1], h))
            .sum::<f64>()
    };
    let (coarse, fine) = (v(1.0 / 32.0), v(1.0 / 64.0));
    assert!(
        (coarse - fine).abs() <= 1e-13 * fine.abs().max(1.0),
        "{coarse} {fine}"
    );
    fine
}

/// `a` less `b` is one solid that checks valid at every level, of `want` mm³ within 1e-8
/// relative (`what` names the case in the failure).
fn cut_is_valid_solid(a: Body, b: Body, want: f64, what: &str) -> Option<String> {
    match boolean(a, b, BooleanOp::Subtract) {
        Ok(Some(r)) => {
            let report = r.check(CheckLevel::All).unwrap();
            let v = volume(&r);
            let off = (v - want).abs() / want;
            (!report.is_valid() || r.solid_count().unwrap() != 1 || off.is_nan() || off > 1e-8)
                .then(|| format!("{what}: {v} mm³ ({off:e} off {want}), {}", report.text))
        }
        other => Some(format!("{what}: {other:?}")),
    }
}

/// The half roll less the r 6 ball about (0, 50, 20) (SolidCraft's sheet metal, a ball cut
/// centred on a roll's face): the ball's poles (0, 50, 14) and (0, 50, 26) lie on the roll's
/// outside, and the line where the ball crosses that wall runs into each pole along the ball's
/// seam meridian, so on the ball the line's trim ends at a corner of its domain, the pole on the
/// seam. The trace's UV tangent line met the seam a little before the pole, the step was cut
/// short there though the curve does not cross the seam, and a second step 1e-4 or less went
/// on to the pole: stepped back 1 % of that, the pole's UV tangent and singular parameter were
/// below the coordinates' precision, and 12 of the 16 frames gave trims with coincident control
/// points, cusps, jumps across the seam or ends outside the face, refused as not a solid. Each
/// frame is one valid solid of V = 3920π less the ball between radii 48 and 50
/// ([`ball_inside`]). The volume the kernel measures is 2.1e-9 to 2.5e-9 relative above it at
/// every accuracy asked (the traced trims' deviation), inside the 1e-8 bound.
#[test]
fn a_ball_centred_on_a_rolls_face_with_its_poles_on_the_face_is_cut_in_every_frame() {
    all_fixes();
    let want = 3920.0 * PI - (ball_inside(|_| 50.0, 50.0) - ball_inside(|_| 48.0, 50.0));
    let failed: Vec<String> = (0..16)
        .filter_map(|frame| {
            let b = ball([0.0, 50.0, 20.0], 0.0);
            cut_is_valid_solid(half_roll(frame), b, want, &format!("frame {frame}"))
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Four frames of the cut above that failed, the ball's seam turned 1e-8, 1e-10 and -1e-8 rad
/// off the meridian the line runs into the poles along: the trim's end is then that far off the
/// corner of the ball's domain, and these were refused too. Each is one valid solid of the same
/// volume as above (turning the ball about its axis moves no point of it).
#[test]
fn a_ball_with_its_poles_on_a_rolls_face_and_its_seam_turned_slightly_off_is_cut() {
    all_fixes();
    let want = 3920.0 * PI - (ball_inside(|_| 50.0, 50.0) - ball_inside(|_| 48.0, 50.0));
    let mut failed = Vec::new();
    for turn in [1e-8, 1e-10, -1e-8] {
        for frame in [2, 5, 8, 14] {
            let what = format!("turned {turn:e}, frame {frame}");
            let b = ball([0.0, 50.0, 20.0], turn);
            failed.extend(cut_is_valid_solid(half_roll(frame), b, want, &what));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The ball's poles on a cylinder's seam and on a face's boundary edge, its own seam on the
/// line's meridian there or square to it. A solid cylinder r 50, z 0..40, extruded from a whole
/// circle starting at +x (its seam on +x), less the ball about (50, 0, 20): its poles are on
/// that seam, V = π·50²·40 less the ball inside radius 50. The half roll (frame 0) less the
/// same ball: its poles are on the edge between the roll's outside and its end at y = 0, and
/// the ball is symmetric about that end's plane, so V = 3920π less half the ball between radii
/// 48 and 50.
#[test]
fn a_ball_with_its_poles_on_a_cylinders_seam_or_on_a_faces_edge_is_cut() {
    all_fixes();
    let inside_50 = ball_inside(|_| 50.0, 50.0);
    let between = inside_50 - ball_inside(|_| 48.0, 50.0);
    let mut failed = Vec::new();
    for turn in [0.0, PI / 2.0] {
        let circle = [Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 50.0,
            start: 0.0,
            end: TAU,
        }];
        let cylinder = extrude(
            &circle,
            [0.0, 0.0, 1.0],
            40.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        failed.extend(cut_is_valid_solid(
            cylinder,
            ball([50.0, 0.0, 20.0], turn),
            PI * 2500.0 * 40.0 - inside_50,
            &format!("cylinder's seam, turned {turn}"),
        ));
        failed.extend(cut_is_valid_solid(
            half_roll(0),
            ball([50.0, 0.0, 20.0], turn),
            3920.0 * PI - between / 2.0,
            &format!("roll's end edge, turned {turn}"),
        ));
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The ball's poles on a plane and on a cone. A box on one side of the plane y = 0 (which holds
/// the ball's seam meridian) or of x = 0 (square to it) less the ball about the origin: the
/// plane cuts the ball in a great circle through both poles, V = 20·10·20 less half the ball,
/// 2/3·π·6³. A frustum about +z, radius 60 at z = 0 to 40 at z = 40, less the ball about
/// (0, 47, 20): its top pole (0, 47, 26) is on the cone (radius 60 - 26/2 = 47 there) and the
/// line runs into it along the ball's seam meridian; V = π·40/3·(60² + 60·40 + 40²) less the
/// ball inside radius 60 - z/2.
#[test]
fn a_ball_with_its_poles_on_a_plane_or_a_cone_is_cut() {
    all_fixes();
    let half_ball = 2.0 / 3.0 * PI * 216.0;
    let mut failed = Vec::new();
    failed.extend(cut_is_valid_solid(
        make_box([-10.0, 0.0, -10.0], [20.0, 10.0, 20.0]).unwrap(),
        ball([0.0; 3], 0.0),
        4000.0 - half_ball,
        "plane y = 0",
    ));
    failed.extend(cut_is_valid_solid(
        make_box([0.0, -10.0, -10.0], [10.0, 20.0, 20.0]).unwrap(),
        ball([0.0; 3], 0.0),
        4000.0 - half_ball,
        "plane x = 0",
    ));
    let frustum = PI * 40.0 / 3.0 * (3600.0 + 2400.0 + 1600.0);
    failed.extend(cut_is_valid_solid(
        make_cone([0.0; 3], 60.0, 40.0, 40.0).unwrap(),
        ball([0.0, 47.0, 20.0], 0.0),
        frustum - ball_inside(|z| 60.0 - z / 2.0, 47.0),
        "cone",
    ));
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The half roll less the r 6 ball about (0, 50 + δ, 20), its poles δ off the roll's outside:
/// 1e-6 inside it and 1e-6 outside, within the tolerance the trace refines with, in the four
/// frames of the walls' cylinders that failed above. The line where the ball crosses the wall
/// passes the poles δ from them, and the trace takes each pole as a point of it (within that
/// tolerance) and ends two curves there. `StepBackFromSingularity` took the pole point's
/// singular parameter 1 % of the last span back from the pole, where the curve's direction
/// seen from the pole is set by the pole's offset (an angle of about δ over the distance), not
/// by the curve: every span into the pole failed the span test by about 100 δ, the steps
/// shrank to the tolerance, and the pole points got singular parameters a quarter turn off
/// (0.73 for 0.5) and UV tangents of 3e5, refused as not a solid. Each frame is one valid
/// solid of V = 3920π less the ball between radii 48 and 50 at that distance ([`ball_inside`]).
#[test]
fn a_ball_with_its_poles_within_tolerance_of_a_rolls_face_is_cut() {
    all_fixes();
    let mut failed = Vec::new();
    for delta in [-1e-6, 1e-6] {
        let d = 50.0 + delta;
        let want = 3920.0 * PI - (ball_inside(|_| 50.0, d) - ball_inside(|_| 48.0, d));
        for frame in [2, 5, 8, 14] {
            let what = format!("δ {delta:e}, frame {frame}");
            failed.extend(cut_is_valid_solid(
                half_roll(frame),
                ball([0.0, d, 20.0], 0.0),
                want,
                &what,
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The same cut with the poles 1e-4 inside and outside the roll's outside, past the
/// tolerance: each of the four frames is one valid solid of the closed-form volume, as it
/// was before the trace placed its points at poles.
#[test]
fn a_ball_with_its_poles_past_tolerance_of_a_rolls_face_is_cut() {
    all_fixes();
    let mut failed = Vec::new();
    for delta in [-1e-4, 1e-4] {
        let d = 50.0 + delta;
        let want = 3920.0 * PI - (ball_inside(|_| 50.0, d) - ball_inside(|_| 48.0, d));
        for frame in [2, 5, 8, 14] {
            let what = format!("δ {delta:e}, frame {frame}");
            failed.extend(cut_is_valid_solid(
                half_roll(frame),
                ball([0.0, d, 20.0], 0.0),
                want,
                &what,
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A box read from its record and mirrored across x = 0 into one of two openings cut into the
/// side of a loft through two closed cubic splines, 0.2 mm clear of the loft on every side
/// (#15862). The mirror keeps each of the box's straight edges a line: before, each became the
/// degree 1 B-spline of the whole line, its control points some 1.8e7 mm out, and the loft
/// minus the box was nothing, the loft and the box intersected in the whole loft, and their union
/// was refused (1001). The loft is SolidCraft's, as its record (`data/`).
#[test]
fn a_box_mirrored_into_an_opening_of_a_spline_loft_stays_clear_of_it() {
    all_fixes();
    let record: Record = serde_json::from_str(include_str!(
        "data/issue_15862_spline_loft_with_openings.record.json"
    ))
    .unwrap();
    let loft = || Body::from_record(&record, true).unwrap().0;
    let lofted = volume(&loft());
    // the box as SolidCraft makes it: its rectangle extruded, read back from its record
    let mirrored = || {
        let corners = [[-24.8, 12.0], [-10.2, 12.0], [-10.2, 25.0], [-24.8, 25.0]];
        let rectangle: Vec<Curve> = (0..4)
            .map(|i| {
                let ([x0, y0], [x1, y1]) = (corners[i], corners[(i + 1) % 4]);
                Curve::Line {
                    start: [x0, y0, 16.2],
                    end: [x1, y1, 16.2],
                }
            })
            .collect();
        let made = extrude(
            &rectangle,
            [0.0, 0.0, 1.0],
            7.6,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        let b = Body::from_record(&made.to_record().unwrap().0, true)
            .unwrap()
            .0;
        b.transform(&[
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .unwrap();
        b
    };
    let boxed = 14.6 * 13.0 * 7.6;
    let cut = boolean(loft(), mirrored(), BooleanOp::Subtract).unwrap();
    assert!(cut.is_some(), "the loft minus the box is nothing");
    let cut = cut.unwrap();
    let report = cut.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(cut.solid_count().unwrap(), 1);
    assert_eq!(cut.counts().unwrap(), [13, 27, 18]);
    assert!(
        near(volume(&cut), lofted),
        "{}, the loft {lofted}",
        volume(&cut)
    );

    for (a, b) in [(loft(), mirrored()), (mirrored(), loft())] {
        let common = boolean(a, b, BooleanOp::Intersect).unwrap();
        assert!(
            common.is_none(),
            "the loft and the box share {:?} mm3",
            common.map(|c| volume(&c))
        );
    }

    let u = boolean(loft(), mirrored(), BooleanOp::Union);
    assert!(
        matches!(u, Ok(Some(_))),
        "{:?}",
        u.as_ref().map(Option::is_some)
    );
    let u = u.unwrap().unwrap();
    let report = u.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(u.solid_count().unwrap(), 2);
    assert_eq!(u.counts().unwrap(), [19, 39, 26]);
    assert!(
        near(volume(&u), lofted + boxed),
        "{}, the loft and the box {}",
        volume(&u),
        lofted + boxed
    );

    let (mirrored_record, _) = mirrored().to_record().unwrap();
    for e in &mirrored_record.edges {
        assert!(
            matches!(e.curve, solidcore_record::brep::Curve::Line { .. }),
            "{:?}",
            e.curve
        );
    }
    assert!(near(volume(&mirrored()), boxed), "{}", volume(&mirrored()));
}

/// Sets the calling thread's fixes: none, or every one (SolidCraft's set).
fn with_fixes(all: bool) {
    if all {
        all_fixes();
    } else {
        session::set_thread_fixes(std::iter::empty::<&str>()).unwrap();
    }
}

/// `body` as SolidCraft hands it to the next feature: through its record.
fn round_tripped(body: &Body) -> Body {
    let (record, _) = body.to_record().unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// solidcraft#12785 (solidcore #13650): a block with a hexagonal pocket through it and a
/// round hole drilled from an end face along the pocket's axis plane, ending exactly at a
/// corner of the pocket, so the hole's bottom meets the two pocket walls along a segment of
/// the corner edge (a solid touching itself along a line). The cut leaves that segment as one
/// edge of four faces; a record round trip (`to_record`, `from_record`: SolidCraft's body
/// between features) writes it as two edges of two faces each, and filling the pocket with
/// the same hexagon as a join failed there (`topology_intersector::p1::relate`, status 1001,
/// or a result that is not a closed solid). Without the hole, or without the round trip, the
/// same join works. Carried from bherbruck's
/// `boolean_join_fills_pocket_with_hole_at_its_corner.rs`.
#[allow(clippy::expect_used, clippy::panic)] // test helpers
mod pocket_with_hole_at_its_corner {
    use std::f64::consts::TAU;

    use solidcore_ops::boolean::{boolean, BooleanOp};
    use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
    use solidcore_ops::{Body, CheckLevel};

    use super::{round_tripped, with_fixes};

    /// The hexagon's circumradius.
    const R: f64 = 6.3685;
    const Z: [f64; 3] = [0.0, 0.0, 1.0];

    fn line(start: [f64; 3], end: [f64; 3]) -> Curve {
        Curve::Line { start, end }
    }

    /// The regular hexagon of circumradius `R` about the origin with a corner on +X, at z = 0
    /// (the reporter's sketch: the corner at (R, 0), the next at (R/2, R sqrt(3)/2)).
    fn hexagon() -> Vec<Curve> {
        const H: f64 = 5.515_282_784_001_197;
        let corners = [
            [R, 0.0, 0.0],
            [R / 2.0, H, 0.0],
            [-R / 2.0, H, 0.0],
            [-R, 0.0, 0.0],
            [-R / 2.0, -H, 0.0],
            [R / 2.0, -H, 0.0],
        ];
        (0..6)
            .map(|i| line(corners[i], corners[(i + 1) % 6]))
            .collect()
    }

    /// The block 100.29 x 32 x 10.
    fn block() -> Body {
        extrude(
            &[
                line([-50.145, -16.0, 0.0], [50.145, -16.0, 0.0]),
                line([50.145, -16.0, 0.0], [50.145, 16.0, 0.0]),
                line([50.145, 16.0, 0.0], [-50.145, 16.0, 0.0]),
                line([-50.145, 16.0, 0.0], [-50.145, -16.0, 0.0]),
            ],
            Z,
            10.0,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap()
    }

    /// The hexagon extruded through the block's height: the pocket's cutter and the join's
    /// tool.
    fn tool() -> Body {
        extrude(&hexagon(), Z, 10.0, 0.0, DraftCorner::Curvature, true).unwrap()
    }

    /// The block with the pocket through it and the hole: 2.5 mm across, along -X from the
    /// end face x = 50.145, 43.7765 deep, so it ends at the pocket's corner (R, 0).
    fn target() -> Body {
        let pocketed = boolean(block(), tool(), BooleanOp::Subtract)
            .unwrap()
            .expect("the pocketed block");
        let hole = extrude(
            &[Curve::Arc {
                center: [50.145, 0.0, 5.0],
                axis: [-1.0, 0.0, 0.0],
                reference: [0.0, 1.0, 0.0],
                radius: 1.25,
                start: 0.0,
                end: TAU,
            }],
            [-1.0, 0.0, 0.0],
            50.145 - R,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap();
        boolean(pocketed, hole, BooleanOp::Subtract)
            .unwrap()
            .expect("the drilled block")
    }

    /// Joins `target` and the hexagon, checks the result and compares its volume with the
    /// sum.
    fn join_fills(target: Body, what: &str) {
        let want = target.volume(1e-9).unwrap() + tool().volume(1e-9).unwrap();
        let b = match boolean(target, tool(), BooleanOp::Union) {
            Ok(Some(b)) => b,
            Ok(None) => panic!("{what}: no body"),
            Err(e) => panic!("{what}: {e}"),
        };
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{what}:\n{}", report.text);
        let got = b.volume(1e-9).unwrap();
        assert!(
            (got - want).abs() <= 1e-6 * want,
            "{what}: volume {got} vs {want}"
        );
    }

    /// The cut's own body: the corner segment is one edge of four faces, and the join works.
    #[test]
    fn filling_a_pocket_whose_corner_a_hole_reaches_joins() {
        let target = target();
        assert_eq!(target.counts().unwrap(), [15, 37, 24]);
        join_fills(target, "the cut's body");
    }

    /// The same body after its record round trip: the corner segment is two coincident edges
    /// of two faces each, and the join works as well, with no fixes and with every fix.
    #[test]
    fn filling_a_pocket_whose_corner_a_hole_reaches_joins_after_a_record_round_trip() {
        for all in [false, true] {
            with_fixes(all);
            let target = round_tripped(&target());
            assert_eq!(target.counts().unwrap(), [15, 38, 24]);
            assert!(target.check(CheckLevel::All).unwrap().is_valid());
            join_fills(target, &format!("the round-tripped body, all fixes {all}"));
        }
    }

    /// The control: without the hole the same join works, round trip or not.
    #[test]
    fn filling_a_pocket_without_the_hole_joins() {
        let pocketed = boolean(block(), tool(), BooleanOp::Subtract)
            .unwrap()
            .expect("the pocketed block");
        join_fills(round_tripped(&pocketed), "the pocketed block");
    }
}

/// Two unit boxes touching along the edge x = y = 1 in one body (two solids), after a record
/// round trip (`split_shared` writes the edge once per solid, so the operand carries it as two
/// coincident edges, which the boolean glues back into the one edge the kernel's union shares).
/// Booleans on it give the exact volumes, whether the tool reaches the shared edge or not,
/// with no fixes and with every fix: subtract `[0.5, 1.5]^2 x [0, 0.5]` (an eighth of a unit
/// from each box, across the edge) leaves 1.75; intersect `[0.5, 1.5]^2 x [0, 1]` keeps 0.5;
/// union with a far unit box gives 3.
#[test]
fn two_solids_touching_along_an_edge_survive_a_round_trip_and_booleans() {
    for all in [false, true] {
        with_fixes(all);
        let touching = || {
            let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
            let b = make_box([1.0, 1.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
            round_tripped(&boolean(a, b, BooleanOp::Union).unwrap().unwrap())
        };
        let cases = [
            (
                BooleanOp::Subtract,
                make_box([0.5, 0.5, 0.0], [1.0, 1.0, 0.5]).unwrap(),
                1.75,
            ),
            (
                BooleanOp::Intersect,
                make_box([0.5, 0.5, 0.0], [1.0, 1.0, 1.0]).unwrap(),
                0.5,
            ),
            (
                BooleanOp::Union,
                make_box([5.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap(),
                3.0,
            ),
        ];
        for (op, tool, want) in cases {
            let r = boolean(touching(), tool, op).unwrap().unwrap();
            let report = r.check(CheckLevel::All).unwrap();
            assert!(
                report.is_valid(),
                "all fixes {all}, {op:?}:\n{}",
                report.text
            );
            assert!(
                near(volume(&r), want),
                "all fixes {all}, {op:?}: {} vs {want}",
                volume(&r)
            );
        }
    }
}
