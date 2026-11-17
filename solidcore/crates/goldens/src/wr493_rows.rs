//! The WR-493 retirement rows that move only by rounding (tests only): each is run again and
//! held to the row it replaces (#18230's, which the retirement stacks on) within
//! [`ROUNDING`] of the value's scale, and the sphere-pole points to the sphere they lie on.
//!
//! The rows record sums: an integrated mass property is a sum over many quadrature pieces,
//! and a stored block of a long array is a sum over up to [`BLOCK`] entries. A sum of `n`
//! terms carries a rounding error of up to `(n - 1) u` times the sum of its terms'
//! magnitudes (`u = 2^-53`); [`ROUNDING`], 16 `EPSILON`, is far below that bound for every
//! sum here, so a value within it of the old one moved by rounding only.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::case::regression_case;
use crate::golden::BLOCK;
use crate::runner::UnitDiff;
use crate::units::{
    blend_fillet_executive_p3, offset_offset_geometry_creation, topology_brep_p4, topology_face_p2,
};

/// What a value may move by rounding, relative to its scale.
const ROUNDING: f64 = 16.0 * f64::EPSILON;

/// The values `runner` produces for `case`.
fn run(runner: &dyn UnitDiff, case: &str) -> crate::output::Output {
    let output = runner.run_rust(&regression_case(runner, case)).unwrap();
    assert!(
        output.error.is_none() && output.deferred.is_none(),
        "{case}: {:?} {:?}",
        output.error,
        output.deferred
    );
    output
}

/// Asserts that `xs` digests (as the golden stores a long array: per [`BLOCK`] entries the
/// sum, the absolute sum and `sum (j + 1) x_j`) to `old` within [`ROUNDING`] of each sum's
/// own scale: the sum of its terms' magnitudes over the finite entries (an entry of
/// magnitude past `1e300`, a box's open end, is held to equality instead).
fn assert_blocks(case: &str, what: &str, xs: &[f64], n: usize, old: &[[f64; 3]]) {
    assert_eq!(xs.len(), n, "{case}: {what} length");
    let blocks: Vec<&[f64]> = xs.chunks(BLOCK).collect();
    assert_eq!(blocks.len(), old.len(), "{case}: {what} blocks");
    for (i, (block, old)) in blocks.iter().zip(old).enumerate() {
        let (mut sum, mut abs, mut weighted) = (0.0f64, 0.0f64, 0.0f64);
        let (mut scale, mut weighted_scale) = (0.0f64, 0.0f64);
        for (j, &x) in block.iter().enumerate() {
            sum += x;
            abs += x.abs();
            weighted += (j as f64 + 1.0) * x;
            // (an entry past 1e300 makes the abs and weighted sums non-finite: those are
            // compared exactly, and the finite sum is held to the finite entries' scale)
            if x.abs() <= 1e300 {
                scale += x.abs();
                weighted_scale += (j as f64 + 1.0) * x.abs();
            }
        }
        for (k, (now, before, s)) in [
            (sum, old[0], scale),
            (abs, old[1], scale),
            (weighted, old[2], weighted_scale),
        ]
        .into_iter()
        .enumerate()
        {
            if !now.is_finite() || !before.is_finite() {
                assert!(
                    now.to_bits() == before.to_bits() || (now.is_nan() && before.is_nan()),
                    "{case}: {what} block {i} sum {k}: {now} against {before}"
                );
                continue;
            }
            assert!(
                (now - before).abs() <= ROUNDING * s,
                "{case}: {what} block {i} sum {k}: {now} against {before} (scale {s:e})"
            );
        }
    }
}

/// A row before: its case, mass properties, result geometry length and blocks.
type OldRow = (&'static str, [f64; 12], usize, &'static [[f64; 3]]);

/// `blend.fillet_executive_p3`'s `sphere_diff_3cylinders` rows #18230 left (its mass
/// properties, then its result geometry's length and blocks).
const SPHERE_DIFF_3CYLINDERS: [OldRow; 10] = [
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        [
            38227.38201083971,
            169498.72044881847,
            169498.72044881847,
            -3.6096624680305518e-12,
            -3.5986625287811855e-05,
            0.00010882600582886543,
            200903696.71037692,
            200903696.7103708,
            200904372.3634775,
            248.5924784411102,
            1.4046594158019348e-05,
            7.545911702051489e-07,
        ],
        1044,
        &[
            [-58.236307597353125, 3250.9242363105673, 29239.842900965297],
            [393.3322996667531, 2283.6860722004217, -7485.5603598700045],
            [54.332258767866165, 2638.6291550565006, -12862.645915802019],
            [46.38188858324311, 6506.482660065286, 120995.81463561457],
            [295.9333227941589, 589.2005679057166, -398.35425128616663],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        [
            38192.70266321393,
            169488.11176530624,
            169488.11176530624,
            -5.88806115759219e-11,
            -0.000920432559458109,
            0.0027597713564543914,
            200882054.24631333,
            200882054.24663445,
            200899145.80766448,
            6597.350496316261,
            0.00022973577047272241,
            1.139456285719715e-06,
        ],
        1044,
        &[
            [-55.04282221986392, 3250.542036690248, 29403.501350519473],
            [393.3322996667531, 2283.6860722004217, -7485.5603598700045],
            [59.80456960408308, 2637.390621401923, -11679.935345347028],
            [54.00013727115852, 6506.360776884594, 121280.04962217313],
            [298.8673209428467, 589.8598849540451, -364.87965080912534],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1",
        [
            38235.99267453257,
            169499.1503543268,
            169499.1503543268,
            -6.229794975861349e-14,
            -3.911533220714917e-07,
            7.823071677343901e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318608058,
            2.2638975726078466e-08,
            -1.9749435724341126e-07,
        ],
        946,
        &[
            [-272.3531330514662, 3207.2965625494535, -62268.30896168256],
            [843.8053053338348, 2629.357987451512, 142865.50468114667],
            [-241.66997710761865, 2297.9845592191455, -3820.746445578192],
            [-187.22231839074738, 2183.8077466814675, -34648.73480807489],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        [
            38227.38201074548,
            169498.72044881838,
            169498.72044881838,
            -3.6220088204561712e-12,
            -3.598662529211659e-05,
            -0.0001072613892919614,
            200903696.7103769,
            200903696.71037066,
            200904372.36347744,
            -265.4323221051357,
            -1.402542921957747e-05,
            7.61060085421342e-07,
        ],
        1044,
        &[
            [178.65341115016753, 3199.1932537312828, -10439.8950758711],
            [836.4691576206195, 2411.4658225968583, 28928.31526957864],
            [557.8207318103367, 2556.2077310894942, 134325.61965144213],
            [1774.2765745295997, 6185.019339547512, 268612.7998603277],
            [554.8162293878556, 592.8845954862852, 3694.6506925855856],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        [
            38192.70266312969,
            169488.11176530627,
            169488.11176530627,
            -5.888649560297935e-11,
            -0.0009204325594516273,
            -0.0027582066419829967,
            200882054.24631327,
            200882054.24663448,
            200899145.80766442,
            -6614.190339977122,
            -0.00022971704892204995,
            1.1530370726930553e-06,
        ],
        1044,
        &[
            [175.46422321934455, 3198.8067566666673, -10836.357228535842],
            [835.6467151018047, 2410.6433800780437, 28870.04381715195],
            [560.4920316533521, 2558.879030932509, 134983.06049605613],
            [1768.929112111031, 6181.880866392351, 268301.85786924587],
            [555.572420845353, 594.3693320743239, 3691.6057482088704],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1",
        [
            38235.99267453257,
            169499.1503543268,
            169499.1503543268,
            -6.229794975861349e-14,
            -3.911533220714917e-07,
            7.823071677343901e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318608058,
            2.2638975726078466e-08,
            -1.9749435724341126e-07,
        ],
        946,
        &[
            [-272.3531330514662, 3207.2965625494535, -62268.30896168256],
            [843.8053053338348, 2629.357987451512, 142865.50468114667],
            [-241.66997710761865, 2297.9845592191455, -3820.746445578192],
            [-187.22231839074738, 2183.8077466814675, -34648.73480807489],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5",
        [
            38235.99267453257,
            169499.1503543268,
            169499.1503543268,
            -6.229794975861349e-14,
            -3.911533220714917e-07,
            7.823071677343901e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318608058,
            2.2638975726078466e-08,
            -1.9749435724341126e-07,
        ],
        946,
        &[
            [-272.3531330514662, 3207.2965625494535, -62268.30896168256],
            [843.8053053338348, 2629.357987451512, 142865.50468114667],
            [-241.66997710761865, 2297.9845592191455, -3820.746445578192],
            [-187.22231839074738, 2183.8077466814675, -34648.73480807489],
        ],
    ),
    (
        "sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1",
        [
            38235.99267453257,
            169499.1503543268,
            169499.1503543268,
            -6.229794975861349e-14,
            -3.911533220714917e-07,
            7.823071677343901e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318608058,
            2.2638975726078466e-08,
            -1.9749435724341126e-07,
        ],
        946,
        &[
            [-272.3531330514662, 3207.2965625494535, -62268.30896168256],
            [843.8053053338348, 2629.357987451512, 142865.50468114667],
            [-241.66997710761865, 2297.9845592191455, -3820.746445578192],
            [-187.22231839074738, 2183.8077466814675, -34648.73480807489],
        ],
    ),
    (
        "sweep4-sphere_diff_3cylinders-all-r0.1",
        [
            38235.99267453257,
            169499.1503543268,
            169499.1503543268,
            -6.229794975861349e-14,
            -3.911533220714917e-07,
            7.823071677343901e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318608058,
            2.2638975726078466e-08,
            -1.9749435724341126e-07,
        ],
        946,
        &[
            [-272.3531330514662, 3207.2965625494535, -62268.30896168256],
            [843.8053053338348, 2629.357987451512, 142865.50468114667],
            [-241.66997710761865, 2297.9845592191455, -3820.746445578192],
            [-187.22231839074738, 2183.8077466814675, -34648.73480807489],
        ],
    ),
    (
        "sweep5-sphere_diff_3cylinders-e29-r1.0",
        [
            38235.99267453257,
            169499.1503543268,
            169499.1503543268,
            -6.229794975861349e-14,
            -3.911533220714917e-07,
            7.823071677343901e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318608058,
            2.2638975726078466e-08,
            -1.9749435724341126e-07,
        ],
        946,
        &[
            [-272.3531330514662, 3207.2965625494535, -62268.30896168256],
            [843.8053053338348, 2629.357987451512, 142865.50468114667],
            [-241.66997710761865, 2297.9845592191455, -3820.746445578192],
            [-187.22231839074738, 2183.8077466814675, -34648.73480807489],
        ],
    ),
];

/// `sphere_diff_3cylinders`: every mass property within [`ROUNDING`] of the old one, on its
/// component's scale (the area, the volume, the radius of gyration `sqrt(I / V)` for the
/// centroid, the largest axial inertia `I` for the inertias and products), and the result
/// geometry's digest within [`ROUNDING`] of its sums' scales. The counts and topology are
/// those of the goldens, compared exactly there.
#[test]
fn sphere_diff_3cylinders_rows_move_by_rounding_only() {
    for (case, old_mass, n, old_geometry) in SPHERE_DIFF_3CYLINDERS {
        let output = run(&blend_fillet_executive_p3::Runner, case);
        let mass = &output.values["mass_properties"];
        assert_eq!(mass.len(), 12, "{case}");
        let inertia = mass[6..9].iter().fold(0.0f64, |m, x| m.max(x.abs()));
        let gyration = (inertia / mass[1].abs()).sqrt();
        let scales = [
            mass[0].abs(),
            mass[1].abs(),
            mass[2].abs(),
            gyration,
            gyration,
            gyration,
            inertia,
            inertia,
            inertia,
            inertia,
            inertia,
            inertia,
        ];
        for (i, ((now, before), s)) in mass.iter().zip(old_mass).zip(scales).enumerate() {
            assert!(
                (now - before).abs() <= ROUNDING * s,
                "{case}: mass[{i}] {now} against {before} (scale {s:e})"
            );
        }
        assert_blocks(
            case,
            "result_geometry",
            &output.values["result_geometry"],
            n,
            old_geometry,
        );
    }
}

/// `offset.offset_geometry_creation` `gen0051`: the state's digest within [`ROUNDING`] of
/// the old one (#18230: the second block's sum one unit in the last place higher).
#[test]
fn gen0051_moves_by_rounding_only() {
    let output = run(&offset_offset_geometry_creation::Runner, "gen0051");
    assert_blocks(
        "gen0051",
        "state",
        &output.values["state"],
        394,
        &[
            [15304.415989596599, f64::INFINITY, f64::NAN],
            [43.92201188381077, f64::INFINITY, f64::NAN],
        ],
    );
}

/// `|p| - 5`: the distance of `p` from `sphere.smb`'s sphere, the sphere of radius 5 about
/// the origin (its poles are the file's vertices `(0, 0, -5)` and `(0, 0, 5)`).
fn off_sphere(p: [f64; 3]) -> f64 {
    ((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 5.0).abs()
}

/// The point on `sphere.smb` that `topology.face_p2` (`state[4..7]`) and
/// `topology.brep_p4` (`faces_values[9..12]`) record: CLOSER to the sphere than the point
/// before (#18230: `(-5.000000000000003, -6.123233995736768e-16, 8.205698631791652e-16)`,
/// 2.7e-15 off), and within [`ROUNDING`] of its radius.
#[test]
fn sphere_point_rows_are_closer_to_the_sphere() {
    let before = off_sphere([
        -5.000000000000003,
        -6.123233995736768e-16,
        8.205698631791652e-16,
    ]);
    let mut points = Vec::new();
    for case in [
        "extra01050",
        "points_file_sphere.smb_0",
        "points_file_sphere.smb_1",
        "points_file_sphere.smb_2",
    ] {
        let state = &run(&topology_face_p2::Runner, case).values["state"];
        points.push((case, [state[4], state[5], state[6]]));
    }
    for case in ["gen0000", "gen0009", "gen0042", "gen0043", "gen0142"] {
        let faces = &run(&topology_brep_p4::Runner, case).values["faces_values"];
        points.push((case, [faces[9], faces[10], faces[11]]));
    }
    for (case, p) in points {
        let now = off_sphere(p);
        assert!(now < before, "{case}: {p:?} {now:e} off, before {before:e}");
        assert!(
            now <= ROUNDING * 5.0,
            "{case}: {p:?} {now:e} off the sphere"
        );
    }
}
