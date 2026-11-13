//! The boolean family on bodies whose answers are known.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test

use std::f64::consts::{PI, TAU};
use std::io::Read as _;

use serde_json::Value;
use solidcore_ops::boolean::{boolean, imprint, union_all};
use solidcore_ops::sweep::{extrude, revolve, Curve, DraftCorner};
use solidcore_ops::{make_box, make_cylinder, make_sphere, session, Body, BooleanOp, CheckLevel, Error};
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

/// A tube revolved from a rectangle (bore radius 1.5, z -10..0) about z, turned `spin` about
/// its own axis and moved `d` along x, so its bore runs `d` off the axis of a pin of the same
/// radius (z -2..14).
#[allow(clippy::disallowed_methods)] // the test's operand geometry, not ops computing it
fn tube_off_the_pin(spin: f64, d: f64) -> Body {
    let p = |x: f64, z: f64| [x, 0.0, z];
    let corners = [p(1.5, 0.0), p(4.0, 0.0), p(4.0, -10.0), p(1.5, -10.0)];
    let profile: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: corners[i],
            end: corners[(i + 1) % 4],
        })
        .collect();
    let tube = revolve(&profile, [0.0; 3], [0.0, 0.0, 1.0], TAU, true).unwrap();
    let (s, c) = spin.to_radians().sin_cos();
    tube.transform(&[
        [c, s, 0.0, 0.0],
        [-s, c, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [d, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    tube
}

/// The pin and the tube overlap only in the crescent between the pin and the bore, 2 mm long:
/// about 2·r·d·2 mm³. With the bore turned 147° and 1e-4 off, the intersection was the whole
/// first operand (the tube, 431.97 mm³, or the pin, 98.96 mm³), a valid solid. A result must be
/// the crescent; a refusal is acceptable.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_pin_a_hair_off_its_bores_axis_meets_the_tube_in_a_sliver() {
    all_fixes();
    for (spin, d) in [(147.0, 1e-4), (180.0, 1e-4), (147.0, 3.8e-4), (147.0, 9.6e-4)] {
        let crescent = 2.0 * 1.5 * d * 2.0;
        for swap in [false, true] {
            let tube = tube_off_the_pin(spin, d);
            let pin = make_cylinder([0.0, 0.0, -2.0], 1.5, 14.0).unwrap();
            let (a, b) = if swap { (pin, tube) } else { (tube, pin) };
            let Ok(Some(r)) = boolean(a, b, BooleanOp::Intersect) else {
                continue;
            };
            let v = volume(&r);
            assert!(
                (v - crescent).abs() <= 0.05 * crescent,
                "spin {spin} d {d} swap {swap}: {v} mm³ {:?}, crescent {crescent}",
                r.counts()
            );
        }
    }
}
