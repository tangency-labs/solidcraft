//! Rows the WR-493 retirement (and #18195's integrator change) move by rounding only
//! (tests only), and the sphere-pole points against the sphere they lie on. Each case is
//! run again and held to the row it replaces (#18230's, which the retirement stacks on).
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

/// `blend.fillet_executive_p3`'s `sphere_diff_3cylinders` rows #18230 left (its mass
/// properties, then its result geometry's length and blocks).
const SPHERE_DIFF_3CYLINDERS: [OldRow; 10] = [
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        [
            38227.382010839705,
            169498.7204488185,
            169498.7204488185,
            -3.618993989638384e-12,
            -3.5986625303705324e-05,
            0.00010882600582943419,
            200903696.71037695,
            200903696.71037072,
            200904372.36347744,
            248.5924784747251,
            1.4063812144130523e-05,
            7.987673777205798e-07,
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
            169488.11176530627,
            169488.11176530627,
            -5.889000292071394e-11,
            -0.0009204325594738638,
            0.0027597713564552497,
            200882054.2463133,
            200882054.24663433,
            200899145.80766442,
            6597.350496349905,
            0.00022975309905973266,
            1.18398253316734e-06,
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
            38227.38201074549,
            169498.7204488185,
            169498.7204488185,
            -3.6096460380862713e-12,
            -3.598662529486385e-05,
            -0.00010726138929369984,
            200903696.7103769,
            200903696.71037072,
            200904372.36347732,
            -265.4323220761774,
            -1.402955162266303e-05,
            8.153058577120819e-07,
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
            169488.11176530627,
            169488.11176530627,
            -5.888649560297935e-11,
            -0.0009204325594584959,
            -0.0027582066419855294,
            200882054.24631333,
            200882054.24663436,
            200899145.80766436,
            -6614.190339950579,
            -0.0002297201612639897,
            1.194837502366222e-06,
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

/// `blend.fillet_executive_p3`'s `stubs4-torii_unioned-e2-r0.5` mass properties before
/// #18195's integrator change (each trim piece ends at the surface's v knot lines on
/// every face, not only on banded ones): its only row that change moves, apart from the
/// `sphere_diff_3cylinders` rows.
const TORII_UNIONED: OldRow = (
    "stubs4-torii_unioned-e2-r0.5",
    [
        4329.447683834924,
        8906.447940918944,
        8906.447940918944,
        15.499999999999998,
        -9.41193690342896e-13,
        7.008098429528221e-13,
        1146925.2559878067,
        5573665.8550277185,
        6648497.645755144,
        -4.806595779430989e-08,
        9.674975963972185e-08,
        -1.2970727579522074e-07,
    ],
    0,
    &[],
);

/// The moved mass entries of [`SPHERE_DIFF_3CYLINDERS`] and [`TORII_UNIONED`] FARTHER
/// from their 1e-12 integration than #18230's (case, entry, whether #18230's refit sphere
/// replayed through this branch's integrator gives the row within rounding: `true`, the
/// move is the integrator change's alone; `false`, WR-493's other parametrisation of the
/// sphere moves it). Each stays within the row's requested accuracy (asserted); a
/// quadrature move at 1e-4, not a certified improvement.
const FARTHER: [(&str, usize, bool); 27] = [
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1", 10, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 5, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 9, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 10, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1", 5, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1", 9, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1", 10, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", 5, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1", 9, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", 11, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1", 5, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1", 9, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1", 10, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5", 5, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5", 9, false),
    ("stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5", 10, false),
    ("sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1", 5, false),
    ("sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1", 9, false),
    ("sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1", 10, false),
    ("sweep4-sphere_diff_3cylinders-all-r0.1", 5, false),
    ("sweep4-sphere_diff_3cylinders-all-r0.1", 9, false),
    ("sweep4-sphere_diff_3cylinders-all-r0.1", 10, false),
    ("sweep5-sphere_diff_3cylinders-e29-r1.0", 5, false),
    ("sweep5-sphere_diff_3cylinders-e29-r1.0", 9, false),
    ("sweep5-sphere_diff_3cylinders-e29-r1.0", 10, false),
    ("stubs4-torii_unioned-e2-r0.5", 4, true),
    ("stubs4-torii_unioned-e2-r0.5", 11, true),
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

/// The `sphere_diff_3cylinders` rows (WR-493 and #18195's integrator change) and the
/// `torii_unioned` row (the integrator change). **Not a regression test of WR-493** (the
/// entries' classification is measured, not certified).
///
/// Each case's body is made twice, here and with #18230's refit replayed (the found
/// sphere given upstream's fitted NURB, `refit::state`), and its mass integrated as the
/// record integrates it, keeping each summand (`brep_properties_in_box_terms`). A sum of
/// `k` summands in floating point is within `gamma_(k-1) M` of the exact sum of those
/// summands, `M` the sum of their magnitudes (Higham, section 3.1), so per raw component
/// the sum lies in `[T - gamma_(k-1) M, T + gamma_(k-1) M]`; the 12 recorded properties
/// are derived from those intervals in interval arithmetic ([`derived`]). This branch's
/// row is its sums' value (asserted). Area, volume, mass and the inertias are within
/// their sums' rounding of #18230's (asserted). The centroid and product entries move by
/// more: quadrature at the row's requested accuracy 1e-4, through the sphere's other
/// parametrisation (WR-493: the refit replayed here lands near #18230's values) and the
/// integrator change. Each is classified against the same body integrated to 1e-12
/// (an estimate): those farther are pinned in [`FARTHER`]. The result geometry's blocks
/// are held to #18230's within the sums' rounding bounds ([`assert_blocks`]).
#[test]
fn sphere_diff_3cylinders_moved_mass_entries_are_classified() {
    use crate::refit::state::{self, Refit};
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
    // (the row as integrated, and the derived properties' exact and rounding intervals)
    let integrate = |case: &str, refit: bool, accuracy: f64| {
        if refit {
            state::set(Some(Refit::default()));
        }
        let mut k = solidcore::Kernel::default();
        let mut body = blend_fillet_executive_p3::result_body_of(&mut k, case);
        if refit {
            let seen = state::take().expect("the refit");
            assert!(seen.failures.is_empty(), "{case}: {seen:?}");
        }
        let bbox = calculate_tight_bounding_box(&k, &mut body).unwrap();
        let mid = bbox.mid();
        let (total, terms) = brep_properties_in_box_terms(
            &k,
            &mut body,
            accuracy,
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
        (
            derived(mid.to_array(), &raw, &vec![0.0; raw.len()]),
            derived(mid.to_array(), &raw, &err),
        )
    };
    let mut failures = Vec::new();
    let mut moves = Vec::new();
    for (case, old_mass, n, old_geometry) in
        SPHERE_DIFF_3CYLINDERS.into_iter().chain([TORII_UNIONED])
    {
        let output = run(&blend_fillet_executive_p3::Runner, case);
        let mass = &output.values["mass_properties"];
        assert_eq!(mass.len(), 12, "{case}");
        let (exact, band) = integrate(case, false, 1e-4);
        let (_, refit) = integrate(case, true, 1e-4);
        // the reference: the same body integrated to 1e-12 (an estimate, not certified)
        let (reference, _) = integrate(case, false, 1e-12);
        for (i, &now) in mass.iter().enumerate() {
            // (this branch's row is its sums' value: the record derives it so)
            if !exact[i].contains(now) {
                failures.push(format!(
                    "{case}: mass[{i}] {now} not derived from its sums {:?}",
                    exact[i]
                ));
            }
            if band[i].contains(old_mass[i]) {
                continue;
            }
            // moved by more than the sums' rounding: by WR-493 (the fitted NURB's other
            // parametrisation, `refit`) and the integrator change, each a quadrature move
            let r = 0.5 * (reference[i].lo + reference[i].hi);
            let class = if (now - r).abs() <= (old_mass[i] - r).abs() {
                "closer"
            } else {
                "farther"
            };
            let wr493 = !(band[i].lo > refit[i].hi || refit[i].lo > band[i].hi);
            eprintln!(
                "{case}: mass[{i}] #18230 {}, now {now}, refit here {:?}, 1e-12 reference {r}: {class}",
                old_mass[i], refit[i]
            );
            // (area, volume, mass and the inertias move by no more than their rounding; a
            // moved centroid or product entry stays within the row's requested accuracy
            // 1e-4 of the 1e-12 reference, relative to the body's scale: its radius of
            // gyration for a centroid, its largest inertia for a product)
            let inertia = mass[6].abs().max(mass[7].abs()).max(mass[8].abs());
            let scale = if i < 6 {
                (inertia / mass[2].abs()).sqrt()
            } else {
                inertia
            };
            if i < 3 || (6..9).contains(&i) || (now - r).abs() > 1e-4 * scale {
                failures.push(format!(
                    "{case}: mass[{i}] #18230 {}, now {now}",
                    old_mass[i]
                ));
            }
            if class == "farther" {
                moves.push((case, i, wr493));
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
    assert_eq!(moves, FARTHER, "{moves:#?}");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// `offset.offset_geometry_creation` `gen0051`: **a rounding-only bound, not a regression
/// test of WR-493** (it passes with it reverted): the state's digest within its sums'
/// rounding bounds of the old one ([`assert_blocks`]; #18230: the second block's sum one
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
/// before (#18230: `(-5.000000000000003, -6.123233995736768e-16, 8.205698631791652e-16)`,
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
///   in that plane, in closed form: the row is 6.4e-7 from it, #18230's 309.8222156047109
///   was 1.4e-5 (CLOSER). Splitting each trim piece at the face's interior v knot lines
///   (the `box_w_slot` fix) moved it 3.4e-12 farther than this branch's earlier
///   309.8222012093561 (pinned below);
/// - `Solids__revolution_mold.smb`: moved only by that split (area 19098.629204575394 ->
///   19098.629211118132, volume 100913.14334351341 -> 100913.14334348583), classified
///   against certified enclosures in `heal_edge_tolerance_covers_edge_face_gap.rs`;
/// - `ss_plane_sph1..4`: the plane face is the square of side 5, area 25 exactly: the row
///   is 25 (CLOSER: #18230's was 25.000000000000004); the sphere face's row,
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
            &[100_913.143_343_485_83],
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
        "sphere_plane: row {now}, #18230 {before}, closed form {exact}"
    );
    // FARTHER than this branch's row before the trims were split at the v knot lines
    // (309.8222012093561, 6.440672e-7 off) by 3.4e-12 (1.1e-14 relative): a quadrature
    // move, inside the integrator's accuracy, not rounding. Pinned so it stays visible.
    let unsplit = 309.822_201_209_356_1_f64;
    assert!(
        (unsplit - exact).abs() < (now - exact).abs()
            && (now - exact).abs() - (unsplit - exact).abs() < 4e-12,
        "sphere_plane: row {now}, before the split {unsplit}, closed form {exact}"
    );
    // the planes: 25 exactly, #18230's 25.000000000000004
    assert!((25.0_f64 - 25.0).abs() < (25.000_000_000_000_004_f64 - 25.0).abs());
}
