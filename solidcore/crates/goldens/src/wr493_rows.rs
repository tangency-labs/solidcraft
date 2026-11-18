//! Rows the WR-493 retirement moves by rounding only (tests only), and the sphere-pole
//! points against the sphere they lie on. Each case is run again and held to main's row.
//!
//! The rows record sums: an integrated mass property is a sum of the integrator's
//! summands, and a stored block of a long array is a sum over up to [`BLOCK`] entries. A
//! floating-point sum of `n` terms is within `gamma_(n-1) = (n - 1) u / (1 - (n - 1) u)`
//! times the sum of their magnitudes of their exact sum (Higham, section 3.1); each test
//! computes that bound from the terms themselves, with its `n`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::case::regression_case;
use crate::golden::BLOCK;
use crate::interval::{exact_sum, gamma_up, two_prod, Iv};
use crate::runner::UnitDiff;
use crate::units::{
    blend_fillet_executive_p3, offset_offset_geometry_creation, topology_brep_p4, topology_face_p2,
};

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
/// sum, the absolute sum and `sum (j + 1) x_j`) to `old` within both sums' rounding bounds:
/// a sum of `n` entries is within `gamma_(n-1)` (`gamma_n` for the weighted sum, whose
/// products round too) times its entries' magnitudes of the exact sum of its entries
/// (Higham, section 3.1). The magnitudes are this row's (summed in interval arithmetic)
/// and the old row's absolute sum (`n` times it for the weighted sum); where the old row's
/// is not finite (an entry past `1e300`, a box's open end, compared exactly) the finite
/// entries' are this row's for both. Non-finite sums are compared exactly.
fn assert_blocks(case: &str, what: &str, xs: &[f64], n: usize, old: &[[f64; 3]]) {
    assert_eq!(xs.len(), n, "{case}: {what} length");
    let blocks: Vec<&[f64]> = xs.chunks(BLOCK).collect();
    assert_eq!(blocks.len(), old.len(), "{case}: {what} blocks");
    for (i, (block, old)) in blocks.iter().zip(old).enumerate() {
        let len = block.len();
        let (mut sum, mut abs, mut weighted) = (0.0f64, 0.0f64, 0.0f64);
        let (mut scale, mut weighted_scale) = (Iv::ZERO, Iv::ZERO);
        for (j, &x) in block.iter().enumerate() {
            sum += x;
            abs += x.abs();
            weighted += (j as f64 + 1.0) * x;
            if x.abs() <= 1e300 {
                scale = scale.add(Iv::exact(x.abs()));
                weighted_scale =
                    weighted_scale.add(Iv::exact(j as f64 + 1.0).mul(Iv::exact(x.abs())));
            }
        }
        let old_scale = if old[1].is_finite() {
            Iv::exact(old[1])
        } else {
            scale
        };
        let bound = |g: f64, mine: Iv, theirs: Iv| Iv::exact(g).mul(mine.add(theirs)).hi;
        let sum_bound = bound(gamma_up(len - 1), scale, old_scale);
        let weighted_bound = bound(
            gamma_up(len),
            weighted_scale,
            old_scale.mul(Iv::exact(len as f64)),
        );
        for (k, (now, before, b)) in [
            (sum, old[0], sum_bound),
            (abs, old[1], sum_bound),
            (weighted, old[2], weighted_bound),
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
                Iv::exact(now).sub(Iv::exact(before)).mag() <= b,
                "{case}: {what} block {i} sum {k}: {now} against {before} (bound {b:e})"
            );
        }
    }
}

/// A row before: its case, mass properties, result geometry length and blocks.
type OldRow = (&'static str, [f64; 12], usize, &'static [[f64; 3]]);

/// `blend.fillet_executive_p3`'s `sphere_diff_3cylinders` rows on main (its mass
/// properties, then its result geometry's length and blocks).
const SPHERE_DIFF_3CYLINDERS: [OldRow; 10] = [
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        [
            38227.3820108397,
            169498.7204488313,
            169498.7204488313,
            -6.114560407317939e-14,
            -3.598662527301659e-05,
            0.00010882600400569041,
            200903696.71038696,
            200903696.710392,
            200904372.36348903,
            248.59247835671283,
            1.973124187014311e-08,
            -1.773516745017131e-07,
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
            38192.70266321392,
            169488.11176517553,
            169488.11176517553,
            -6.121288540895723e-14,
            -0.0009204325605192529,
            0.002759771374899137,
            200882054.24621296,
            200882054.24641633,
            200899145.80754238,
            6597.350500432896,
            2.0507525094612877e-08,
            -1.7776391437163526e-07,
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
            38235.992674532565,
            169499.1503543268,
            169499.1503543268,
            -6.229576286325623e-14,
            -3.9115332202330604e-07,
            7.823071671252264e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318633904,
            2.5275099149794463e-08,
            -1.9533483200075475e-07,
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
            169498.72044883113,
            169498.72044883113,
            -4.979080091814951e-14,
            -3.598662526162161e-05,
            -0.0001072613874655039,
            200903696.7103869,
            200903696.71039194,
            200904372.3634889,
            -265.4323219548221,
            1.1120885514499355e-08,
            -1.6280852434782863e-07,
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
            38192.7026631297,
            169488.11176517545,
            169488.11176517545,
            -5.5565903320905924e-14,
            -0.0009204325605013881,
            -0.0027582066604249548,
            200882054.246213,
            200882054.24641624,
            200899145.80754232,
            -6614.190344030134,
            9.501001257738387e-09,
            -1.6906407974309621e-07,
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
            38235.992674532565,
            169499.1503543268,
            169499.1503543268,
            -6.229576286325623e-14,
            -3.9115332202330604e-07,
            7.823071671252264e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318633904,
            2.5275099149794463e-08,
            -1.9533483200075475e-07,
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
            38235.992674532565,
            169499.1503543268,
            169499.1503543268,
            -6.229576286325623e-14,
            -3.9115332202330604e-07,
            7.823071671252264e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318633904,
            2.5275099149794463e-08,
            -1.9533483200075475e-07,
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
            38235.992674532565,
            169499.1503543268,
            169499.1503543268,
            -6.229576286325623e-14,
            -3.9115332202330604e-07,
            7.823071671252264e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318633904,
            2.5275099149794463e-08,
            -1.9533483200075475e-07,
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
            38235.992674532565,
            169499.1503543268,
            169499.1503543268,
            -6.229576286325623e-14,
            -3.9115332202330604e-07,
            7.823071671252264e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318633904,
            2.5275099149794463e-08,
            -1.9533483200075475e-07,
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
            38235.992674532565,
            169499.1503543268,
            169499.1503543268,
            -6.229576286325623e-14,
            -3.9115332202330604e-07,
            7.823071671252264e-07,
            200904581.2882041,
            200904581.28820112,
            200904581.2882041,
            -8.419921318633904,
            2.5275099149794463e-08,
            -1.9533483200075475e-07,
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

/// The record's 12 mass properties as `brep_mass_properties(1e-4, density 1, about the
/// origin)` derives them from the raw sums about the box's middle `mid` (`estimate`), in
/// interval arithmetic: `raw[c]` is the raw component `c` (area, volume, mass, then the 8
/// moment vectors' x, y, z) within `raw_err[c]` of the computed sum.
fn derived(mid: [f64; 3], raw: &[f64], raw_err: &[f64]) -> [Iv; 12] {
    let r = |c: usize| Iv::around(raw[c], raw_err[c]);
    let moment = |i: usize, j: usize| r(3 + 3 * i + j);
    let mass = r(2);
    let d = mid.map(Iv::exact);
    let s = [0, 1, 2].map(|j| moment(4, j));
    let shift = |a: usize, b: usize| {
        d[a].mul(s[b])
            .add(d[b].mul(s[a]))
            .add(d[a].mul(d[b]).mul(mass))
    };
    [
        r(0),
        r(1),
        mass,
        d[0].add(s[0].div(mass)),
        d[1].add(s[1].div(mass)),
        d[2].add(s[2].div(mass)),
        moment(7, 0).add(shift(1, 1).add(shift(2, 2))),
        moment(7, 1).add(shift(0, 0).add(shift(2, 2))),
        moment(7, 2).add(shift(0, 0).add(shift(1, 1))),
        moment(6, 0).add(shift(1, 2)),
        moment(6, 1).add(shift(2, 0)),
        moment(6, 2).add(shift(0, 1)),
    ]
}

// The entries of [`SPHERE_DIFF_3CYLINDERS`] main's row lies outside this branch's
// summation bands for (case, entry): `BEYOND_ROUNDING`, each classified EQUAL within the
// rows' accuracy against a certified enclosure by `tests/wr493_moved_rows_are_classified`.
include!("../tests/wr493_moved_rows/beyond_rounding.rs");

/// The `sphere_diff_3cylinders` rows the WR-493 retirement moves: **a rounding-of-summation
/// check, not a classification** (`goldens/tests/wr493_moved_rows_are_classified` classifies
/// every moved entry against a certified enclosure of the body's true value).
///
/// Each case's body is integrated as the record integrates it, keeping each summand
/// (`brep_properties_in_box_terms`). A sum of `k` summands in floating point is within
/// `gamma_(k-1) M` of the exact sum of those summands, `M` the sum of their magnitudes
/// (Higham, section 3.1), so per raw component the sum lies in `[T - gamma_(k-1) M, T +
/// gamma_(k-1) M]`; the 12 recorded properties are derived from those intervals in interval
/// arithmetic ([`derived`]). This branch's row is its sums' value (asserted). The entries
/// main's row lies outside those bands for are pinned (`BEYOND_ROUNDING`): the found
/// sphere's other parametrisation moves them by quadrature at the row's accuracy 1e-4, not
/// by rounding, and each is EQUAL within that accuracy against the body's certified
/// enclosure (`wr493_moved_rows_are_classified` checks the list against its classes). The result geometry's blocks are held to main's within the sums' rounding
/// bounds ([`assert_blocks`]).
#[test]
fn sphere_diff_3cylinders_rows_are_their_sums() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{
        brep_properties_in_box_terms, MassPropertyBehavior, PreciseProperties, PropertyAttributes,
    };
    use solidcore::query::brep::calculate_tight_bounding_box;
    let store = AttributeStore::default();
    let attributes = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let flat = |t: &PreciseProperties| {
        let mut v = vec![t.area, t.volume, t.mass];
        for m in t.moments {
            v.extend(m.to_array());
        }
        v
    };
    let mut beyond = Vec::new();
    let mut failures = Vec::new();
    for (case, old_mass, n, old_geometry) in SPHERE_DIFF_3CYLINDERS {
        let output = run(&blend_fillet_executive_p3::Runner, case);
        let mass = &output.values["mass_properties"];
        assert_eq!(mass.len(), 12, "{case}");
        let mut k = solidcore::Kernel::default();
        let mut body = blend_fillet_executive_p3::result_body_of(&mut k, case);
        let bbox = calculate_tight_bounding_box(&k, &mut body).unwrap();
        let mid = bbox.mid();
        let (total, terms) = brep_properties_in_box_terms(
            &k,
            &mut body,
            1e-4,
            mid,
            MassPropertyBehavior {
                density: 1.0,
                ..MassPropertyBehavior::default()
            },
            &attributes,
            &bbox,
        )
        .unwrap();
        let raw = flat(&total);
        let rows: Vec<Vec<f64>> = terms.iter().map(flat).collect();
        // the total is the summands added in order (so k is the count of summands)
        let mut sum = vec![0.0; raw.len()];
        for row in &rows {
            for (s, x) in sum.iter_mut().zip(row) {
                *s += x;
            }
        }
        assert_eq!(sum, raw, "{case}: the total is not its summands' sum");
        let gamma = gamma_up(rows.len() - 1);
        let err: Vec<f64> = (0..raw.len())
            .map(|c| {
                // (M rounded up: summed in interval arithmetic)
                let m = rows
                    .iter()
                    .fold(Iv::ZERO, |m, t| m.add(Iv::exact(t[c].abs())));
                Iv::exact(gamma).mul(m).hi
            })
            .collect();
        let exact = derived(mid.to_array(), &raw, &vec![0.0; raw.len()]);
        let band = derived(mid.to_array(), &raw, &err);
        for (i, &now) in mass.iter().enumerate() {
            // (this branch's row is its sums' value: the record derives it so)
            if !exact[i].contains(now) {
                failures.push(format!(
                    "{case}: mass[{i}] {now} not derived from its sums {:?}",
                    exact[i]
                ));
            }
            if !band[i].contains(old_mass[i]) {
                eprintln!(
                    "{case}: mass[{i}] main {}, now {now}, band {:?}",
                    old_mass[i], band[i]
                );
                beyond.push((case, i));
            }
        }
        if n > 0 {
            assert_blocks(
                case,
                "result_geometry",
                &output.values["result_geometry"],
                n,
                old_geometry,
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(beyond, BEYOND_ROUNDING.to_vec(), "{beyond:#?}");
}

/// `offset.offset_geometry_creation` `gen0051`: **a rounding-only bound, not a regression
/// test of WR-493** (it passes with it reverted): the state's digest within its sums'
/// rounding bounds of the old one ([`assert_blocks`]; main's: the second block's sum one
/// unit in the last place higher).
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

/// `||p| - 5|`, the distance of `p` from `sphere.smb`'s sphere, the sphere of radius 5
/// about the origin (its poles are the file's vertices `(0, 0, -5)` and `(0, 0, 5)`),
/// enclosed: `(|p|^2 - 25) / (|p| + 5)`, the numerator summed exactly from exact squares
/// ([`exact_sum`], [`two_prod`]), the rest in interval arithmetic.
fn off_sphere(p: [f64; 3]) -> Iv {
    let mut terms: Vec<f64> = p
        .iter()
        .flat_map(|&c| <[f64; 2]>::from(two_prod(c, c)))
        .collect();
    terms.push(-25.0);
    let [x, y, z] = p.map(Iv::exact);
    let r = x.mul(x).add(y.mul(y)).add(z.mul(z)).sqrt();
    let d = exact_sum(&terms).div(r.add(Iv::exact(5.0)));
    Iv {
        lo: d.mig(),
        hi: d.mag(),
    }
}

/// The point on `sphere.smb` that `topology.face_p2` (`state[4..7]`) and
/// `topology.brep_p4` (`faces_values[9..12]`) record: CLOSER to the sphere than the point
/// before (main's: `(-5.000000000000003, -6.123233995736768e-16, 8.205698631791652e-16)`,
/// 2.7e-15 off): each distance in interval arithmetic, this one's upper bound below the
/// old one's lower bound.
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
        let p = [state[4], state[5], state[6]];
        points.push((case, p));
        // The pole normal (`state[7..10]`, moved with the point): the sphere's normal at
        // the recorded point is the radial direction `p / |p|`. Each component here is
        // `p_i / sqrt(p_x^2 + p_y^2 + p_z^2)`, rounded within gamma_5 of its magnitude
        // (at most 1): EQUAL at rounding.
        let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        let gamma5 = gamma_up(5);
        for i in 0..3 {
            let (n, radial) = (state[7 + i], p[i] / r);
            assert!(
                (n - radial).abs() <= gamma5,
                "{case}: normal[{i}] {n:e} against the radial {radial:e}"
            );
        }
    }
    for case in ["gen0000", "gen0009", "gen0042", "gen0043", "gen0142"] {
        let faces = &run(&topology_brep_p4::Runner, case).values["faces_values"];
        points.push((case, [faces[9], faces[10], faces[11]]));
    }
    for (case, p) in points {
        let now = off_sphere(p);
        assert!(
            now.hi < before.lo,
            "{case}: {p:?} {now:?} off, before {before:?}"
        );
    }
}

/// `corpus.parity`'s rows the retirement moves: the round trip's area and volume are the
/// read's bit for bit (`rt_area`, `rt_volume`), so they move with the area and volume,
/// which are held to their references:
/// - `Fillet__sphere_plane.smb`: the sphere of radius 5 between z = +-h (h the file's pole
///   vertices' height) less its cap beyond the plane x = -4, plus the disk of radius 3
///   in that plane, in closed form: the row is 6.4e-7 from it, main's 309.8222156047109
///   is 1.4e-5 (CLOSER);
/// - `Solids__revolution_mold.smb`: its committed line stays main's (the run agrees with
///   it to the goldens' `1e-9`); the values the run gives are pinned here, and its moved
///   entries are classified against certified enclosures in
///   `heal_edge_tolerance_covers_edge_face_gap.rs`;
/// - `ss_plane_sph1..4`: the plane face is the square of side 5, area 25 exactly: the row
///   is 25 (CLOSER: main's is 25.000000000000004); the sphere face's row,
///   25.14456696301113 (sph1, sph4) and 25.144566963011123 (sph2, sph3), is the value
///   `wr493_class_proofs`' `ss_plane_sph_area_is_the_area_its_edges_bound` holds to the
///   area the face's edge loop bounds.
#[test]
fn corpus_parity_rows_are_closer() {
    use crate::units::corpus_parity::{run_in_process, Runner};
    use std::f64::consts::PI;
    let rows: [(&str, &[f64], &[f64]); 6] = [
        (
            "Fillet__sphere_plane.smb",
            &[309.822_201_209_352_7],
            &[f64::NAN],
        ),
        (
            "Solids__revolution_mold.smb",
            &[19_098.629_211_118_132],
            &[100_913.143_343_685_2],
        ),
        (
            "ss_plane_sph1.smp",
            &[25.144_566_963_011_13, 25.0],
            &[f64::NAN, f64::NAN],
        ),
        (
            "ss_plane_sph2.smp",
            &[25.144_566_963_011_123, 25.0],
            &[f64::NAN, f64::NAN],
        ),
        (
            "ss_plane_sph3.smp",
            &[25.144_566_963_011_123, 25.0],
            &[f64::NAN, f64::NAN],
        ),
        (
            "ss_plane_sph4.smp",
            &[25.144_566_963_011_13, 25.0],
            &[f64::NAN, f64::NAN],
        ),
    ];
    let same = |a: f64, b: f64| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan());
    let mut moved = Vec::new();
    for (case, area, volume) in rows {
        let out = run_in_process(&regression_case(&Runner, case)).unwrap();
        for (key, row) in [
            ("area", area),
            ("rt_area", area),
            ("volume", volume),
            ("rt_volume", volume),
        ] {
            let now = &out.values[key];
            if !(now.len() == row.len() && now.iter().zip(row).all(|(&a, &b)| same(a, b))) {
                moved.push(format!("{case}: {key} {now:?}, row {row:?}"));
            }
        }
    }
    assert!(moved.is_empty(), "{moved:#?}");
    // sphere_plane against its closed form (h: the file's pole vertices, z = +-h)
    let h = 4.980_973_490_458_730_2;
    let exact = 2.0 * PI * 5.0 * 2.0 * h - 2.0 * PI * 5.0 * 1.0 + PI * 9.0;
    let (now, before) = (309.822_201_209_352_7_f64, 309.822_215_604_710_9_f64);
    assert!(
        (now - exact).abs() < (before - exact).abs() && (now - exact).abs() < 1e-6,
        "sphere_plane: row {now}, main {before}, closed form {exact}"
    );
    // the planes: 25 exactly, main's 25.000000000000004
    assert!((25.0_f64 - 25.0).abs() < (25.000_000_000_000_004_f64 - 25.0).abs());
}
