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
/// entries' are this row's for both. Non-finite sums are compared exactly. A block listed in
/// `beyond` is asserted the other way: its plain sum lies outside that bound.
fn assert_blocks(case: &str, what: &str, xs: &[f64], n: usize, old: &[[f64; 3]], beyond: &[usize]) {
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
        if beyond.contains(&i) {
            assert!(
                Iv::exact(sum).sub(Iv::exact(old[0])).mag() > sum_bound,
                "{case}: {what} block {i} moves by rounding only"
            );
            continue;
        }
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
            38227.38201699721,
            169498.72044883124,
            169498.72044883124,
            -6.17470508552754e-14,
            -3.598662528099317e-05,
            0.00010882600400673944,
            200903696.7103869,
            200903696.710392,
            200904372.36348906,
            248.5924783606351,
            2.0621841193072045e-08,
            -1.775121998202596e-07,
        ],
        1044,
        &[
            [-58.23268090218896, 3250.927863005732, 29064.08462137706],
            [393.3328770159371, 2283.686649549606, -7485.447018390693],
            [54.33302840814239, 2638.6299248618766, -12862.55029671469],
            [46.381889540004025, 6506.48266037973, 120995.81465372315],
            [295.9333227952652, 589.2005679033156, -398.35425121573167],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        [
            38192.70266865796,
            169488.11176517548,
            169488.11176517548,
            -6.123467668148411e-14,
            -0.0009204325605269636,
            0.002759771374899807,
            200882054.24621293,
            200882054.24641633,
            200899145.8075424,
            6597.350500435275,
            2.1070314403190426e-08,
            -1.7784404934032368e-07,
        ],
        1044,
        &[
            [-55.03919549681931, 3250.545663413293, 29403.817902851053],
            [393.3328770159371, 2283.686649549606, -7485.447018390693],
            [59.80533926700125, 2637.3913912121498, -11679.839721201231],
            [54.000137965486786, 6506.360777745887, 121280.04962441738],
            [298.86732094821383, 589.8598849414541, -364.87965045001897],
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
            [-272.3498073482843, 3207.2998882526354, -62268.10704584696],
            [843.8061157357686, 2629.358797853445, 142865.54604706616],
            [-241.66866825730182, 2297.985868069462, -3820.548468043122],
            [-187.22231839074738, 2183.8077466814675, -34648.73480807489],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        [
            38227.38201697493,
            169498.72044883113,
            169498.72044883113,
            -5.105890997328647e-14,
            -3.5986625268484007e-05,
            -0.00010726138746382216,
            200903696.7103868,
            200903696.71039197,
            200904372.36348888,
            -265.43232194894676,
            1.3292566368905216e-08,
            -1.6159650301470757e-07,
        ],
        1044,
        &[
            [178.6567179937589, 3199.1965605748737, -10792.031958227584],
            [836.4701059537721, 2411.4667709300106, 28928.453488857253],
            [557.8215076684595, 2556.2085069476175, 134325.7429057291],
            [1774.276576095138, 6185.019340635833, 268612.7998649341],
            [554.8162293889802, 592.8845954838873, 3694.6506926563716],
        ],
    ),
    (
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        [
            38192.702668157785,
            169488.1117651754,
            169488.1117651754,
            -5.1163609319402794e-14,
            -0.0009204325605082199,
            -0.0027582066604235726,
            200882054.2462129,
            200882054.24641618,
            200899145.80754223,
            -6614.190344023803,
            1.2725687587033705e-08,
            -1.6944582603113434e-07,
        ],
        1044,
        &[
            [175.46753007843859, 3198.810063525761, -11189.450905561564],
            [835.647663434957, 2410.6443284111956, 28870.18203643056],
            [560.4928075122987, 2558.879806791456, 134983.1837505503],
            [1768.929113707121, 6181.880867487534, 268301.85787435813],
            [555.5724208509846, 594.3693320622627, 3691.6057485639076],
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
            [-272.3498073482843, 3207.2998882526354, -62268.10704584696],
            [843.8061157357686, 2629.358797853445, 142865.54604706616],
            [-241.66866825730182, 2297.985868069462, -3820.548468043122],
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
            [-272.3498073482843, 3207.2998882526354, -62268.10704584696],
            [843.8061157357686, 2629.358797853445, 142865.54604706616],
            [-241.66866825730182, 2297.985868069462, -3820.548468043122],
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
            [-272.3498073482843, 3207.2998882526354, -62268.10704584696],
            [843.8061157357686, 2629.358797853445, 142865.54604706616],
            [-241.66866825730182, 2297.985868069462, -3820.548468043122],
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
            [-272.3498073482843, 3207.2998882526354, -62268.10704584696],
            [843.8061157357686, 2629.358797853445, 142865.54604706616],
            [-241.66866825730182, 2297.985868069462, -3820.548468043122],
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
            [-272.3498073482843, 3207.2998882526354, -62268.10704584696],
            [843.8061157357686, 2629.358797853445, 142865.54604706616],
            [-241.66866825730182, 2297.985868069462, -3820.548468043122],
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
/// enclosure (`wr493_moved_rows_are_classified` checks the list against its classes). The
/// result geometry's blocks are held to main's within the sums' rounding bounds
/// ([`assert_blocks`]), except the blocks [`geometry_beyond_rounding`] lists, which are
/// asserted to move beyond them (the round edge ends of
/// [`sphere_diff_r05_round_end_gaps_swap_between_the_mirror_cases`]).
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
        let mut output = run(&blend_fillet_executive_p3::Runner, case);
        // (#20179's five classified trims taken off, proved in
        // `sphere_diff_3cylinders_rows_move_only_by_the_classified_trims`)
        without_classified_trims(blend_fillet_executive_p3::Runner.id(), case, &mut output);
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
                geometry_beyond_rounding(case),
            );
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(beyond, BEYOND_ROUNDING.to_vec(), "{beyond:#?}");
}

/// The two `sphere_diff_3cylinders` cases whose result geometry moves beyond rounding,
/// mirror images of each other, and the round edge (in `p1::edges` order) whose end moves
/// (see [`sphere_diff_r05_round_end_gaps_swap_between_the_mirror_cases`]).
const E1_R05: (&str, usize) = ("stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5", 37);
const E4_R05: (&str, usize) = ("stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5", 36);

/// The result-geometry blocks of `case` that move beyond their sums' rounding bounds: the
/// blocks holding the moved round edge (`e1-r0.5` block 3; `e4-r0.5` blocks 2 and 3) and,
/// in `e4-r0.5`, block 0, the vertex records' block (measured; not attributed entry by
/// entry).
fn geometry_beyond_rounding(case: &str) -> &'static [usize] {
    if case == E1_R05.0 {
        &[3]
    } else if case == E4_R05.0 {
        &[0, 2, 3]
    } else {
        &[]
    }
}

/// `blend.fillet_executive_p3` `stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5` and
/// `-e4-r0.5` (rounds of 0.5 on two edges that are mirror images): one FARTHER, the other
/// CLOSER, by the same amount, both far inside tolerance (ARGUED: the gaps are float
/// evaluations, not enclosures). Both rows move only through the kept NURB (the refit
/// replay reproduces main's rows bit for bit, so the refit build here is main's), but in
/// each one round edge's interval end moves beyond rounding, between 70.64830969562033 and
/// 70.6483087419461. At the first value the curve's end lies 2.11e-8 from its end vertex,
/// at the second 9.85e-7. Main has the near end in `e1-r0.5` and the far end in `e4-r0.5`;
/// this branch has them the other way round. So `e1-r0.5`'s edge 37 is FARTHER from its
/// vertex (9.85e-7 against 2.11e-8) and `e4-r0.5`'s edge 36 CLOSER (2.11e-8 against
/// 9.85e-7). Every gap is inside the edge's tolerance (4e-5) and the end vertex's
/// (asserted on both sides). The gaps are evaluated in floating point, good to
/// about 1e-13 here (argued from the curve's size, not derived), so each ranking holds with
/// a margin above 1e5. The edges' starts do not move beyond rounding. The FARTHER one is a
/// defect of the retirement, disclosed in the PR and the WR-493 addendum.
#[test]
fn sphere_diff_r05_round_end_gaps_swap_between_the_mirror_cases() {
    use solidcore::topology::brep::p1;
    use solidcore::topology::edge::{end_vertex, start_vertex};
    // (end parameter, start gap, end gap, end vertex tolerance)
    let gaps = |k: &solidcore::Kernel, b: &solidcore::topology::Brep, i: usize| {
        let e = p1::edges(b, None).unwrap()[i];
        let edge = b.edge(e).unwrap();
        assert_eq!(
            (edge.tolerance, edge.interval.min()),
            (4e-5, 4.000_000_000_004_888_5e-3)
        );
        let c = edge.curve.as_ref().unwrap();
        let at =
            |t: f64, v| (c.evaluate_point(k, t).unwrap() - b.vertex(v).unwrap().point).length();
        let (vs, ve) = (
            start_vertex(b, e).unwrap().unwrap(),
            end_vertex(b, e).unwrap().unwrap(),
        );
        let end_tol = b.vertex(ve).unwrap().tolerance;
        assert!(b.vertex(vs).unwrap().tolerance >= 4e-5 && end_tol >= 4e-5);
        (
            edge.interval.max(),
            at(edge.interval.min(), vs),
            at(edge.interval.max(), ve),
            end_tol,
        )
    };
    let (near_t, far_t) = (70.648_309_695_620_33, 70.648_308_741_946_1);
    let near =
        |t: f64, gap: f64| (t - near_t).abs() <= 1e-13 * near_t && (2.1e-8..2.2e-8).contains(&gap);
    let far =
        |t: f64, gap: f64| (t - far_t).abs() <= 1e-13 * far_t && (9.8e-7..9.9e-7).contains(&gap);
    let mut ends = Vec::new();
    for (case, edge) in [E1_R05, E4_R05] {
        let mut k = solidcore::Kernel::default();
        let now = blend_fillet_executive_p3::result_body_of(&mut k, case);
        let mut k_main = solidcore::Kernel::default();
        let (main, replaced) = crate::refit::with_fitted_spheres(|| {
            blend_fillet_executive_p3::result_body_of(&mut k_main, case)
        });
        assert!(replaced > 0, "{case}");
        let (t_now, start_now, end_now, tol_now) = gaps(&k, &now, edge);
        let (t_main, start_main, end_main, tol_main) = gaps(&k_main, &main, edge);
        eprintln!(
            "{case} edge {edge} end: now t {t_now} gap {end_now:e} vertex tol {tol_now:e}, \
             main t {t_main} gap {end_main:e} vertex tol {tol_main:e}"
        );
        assert!(
            (start_now - start_main).abs() <= 1e-13,
            "{case}: the start moved"
        );
        assert!(end_now < 4e-5 && end_main < 4e-5);
        ends.push((near(t_now, end_now), far(t_now, end_now)));
        ends.push((near(t_main, end_main), far(t_main, end_main)));
        // the gap is inside the end vertex's tolerance on both sides
        assert!(end_now < tol_now && end_main < tol_main, "{case}");
    }
    // e1-r0.5: FARTHER (now far, main near); e4-r0.5: CLOSER (now near, main far)
    assert_eq!(
        ends,
        [(false, true), (true, false), (true, false), (false, true)]
    );
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
        &[],
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
///   in that plane, in closed form: the row (area and round trip alike) is 6.4e-7 from it;
///   main's committed area, 309.8222156047131, is 1.4e-5 from it and its round trip's,
///   309.8221903199704, 1.2e-5 (both CLOSER, by a margin far beyond the closed form's
///   rounding);
/// - `Solids__revolution_mold.smb`: its committed line stays main's (the run agrees with
///   it to the goldens' `1e-9`); the values the run gives are pinned here, and its moved
///   entries are classified against certified enclosures in
///   `heal_edge_tolerance_covers_edge_face_gap.rs`;
/// - `ss_plane_sph1..4`: the plane face is the square of side 5, area 25 exactly: the row
///   is 25, as main's committed row (unmoved); the sphere face's row,
///   25.14456696301113 (sph1, sph4) and 25.144566963011123 (sph2, sph3), is in the face
///   area's certified enclosure and main's is outside it (CLOSER, proved by
///   `wr493_class_proofs`' `ss_plane_sph_area_is_the_area_its_edges_bound`, ARGUED). Their
///   level-2 checks (`check`, `rt_check`) find nothing on the sphere's B-rep, where main's
///   found 11 lines (measured on main at 0b321cbaa4): four edges and six vertices whose
///   tolerance, about 0.016 after the trims were rebuilt on the fitted NURB, exceeds the
///   B-rep's 0.01 (`check_brep` tests 59 and 82), and the line saying that `AssertValid` was
///   skipped because `ValidatePointers` failed. Kept, the file's trims need no such growth.
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
    // ss_plane_sph1..4: the level-2 checks find nothing (main's: 11 lines on the sphere)
    for i in 1..=4 {
        let case = format!("ss_plane_sph{i}.smp");
        let out = run_in_process(&regression_case(&Runner, &case)).unwrap();
        for key in ["check", "rt_check"] {
            assert_eq!(out.ints[key], [0, 0], "{case}: {key}");
        }
    }
    // sphere_plane against its closed form (h: the file's pole vertices, z = +-h)
    let h = 4.980_973_490_458_730_2;
    let exact = 2.0 * PI * 5.0 * 2.0 * h - 2.0 * PI * 5.0 * 1.0 + PI * 9.0;
    let now = 309.822_201_209_352_7_f64;
    // main's committed area and round-trip area
    for before in [309.822_215_604_713_1_f64, 309.822_190_319_970_4] {
        assert!(
            (now - exact).abs() < 1e-6 && (before - exact).abs() > 1e-5,
            "sphere_plane: row {now}, main {before}, closed form {exact}"
        );
    }
}

/// `blend.fillet_executive_p3` `source-file-box_w_slot-all-edges`: its result geometry moves
/// only on the found spheres, entity by entity (`util::primitive::append`'s record). The
/// refit build (the file's spheres given upstream's fitted NURB) is main's: its record's
/// blocks are main's committed blocks bit for bit. Against it:
/// - every vertex is the same, bit for bit;
/// - 24 edges, each with a use on a found sphere, count two UV trims where main counted one:
///   the file's trim on the sphere is kept (main dropped it on the fitted NURB, to be made
///   again when asked for); nothing else of an edge moves;
/// - the found sphere faces keep their tolerance and domain; only their samples move (the
///   same fractions of the same domain of the file's NURB here, of the fitted NURB on main:
///   one sphere, two parameterisations), and every sample of both lies on the sphere to
///   `1e-12` of its radius;
/// - every other face is the same, bit for bit.
#[test]
fn box_w_slot_geometry_moves_only_on_the_found_spheres() {
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::brep::p1;
    use solidcore::topology::edgeuse::p1::face as face_of;
    const CASE: &str = "source-file-box_w_slot-all-edges";
    // main's committed `result_geometry` blocks (expected/blend.fillet_executive_p3.jsonl)
    const MAIN: [[f64; 3]; 6] = [
        [144.20814395045426, 233.4204725538878, 19932.70763647065],
        [157.51043968890355, 253.98374739842555, 20754.459396448365],
        [190.23010568688935, 238.8713400597793, 24560.059827425757],
        [188.25075109345423, 240.5245724234976, 25146.63250432756],
        [124.10466015492509, 174.21526575428467, 15854.92371453973],
        [96.1129828979684, 154.29581589814254, 11030.81247736941],
    ];
    let runner = blend_fillet_executive_p3::Runner;
    let now = run(&runner, CASE).values["result_geometry"].clone();
    let (main, replaced) =
        crate::refit::with_fitted_spheres(|| run(&runner, CASE).values["result_geometry"].clone());
    assert!(replaced > 0, "{CASE}: no sphere refit");
    let blocks: Vec<[f64; 3]> = main
        .chunks(BLOCK)
        .map(|b| {
            let (mut sum, mut abs, mut weighted) = (0.0f64, 0.0f64, 0.0f64);
            for (j, &x) in b.iter().enumerate() {
                sum += x;
                abs += x.abs();
                weighted += (j as f64 + 1.0) * x;
            }
            [sum, abs, weighted]
        })
        .collect();
    assert_eq!(blocks, MAIN, "{CASE}: the refit build is not main's");

    let mut k = solidcore::Kernel::default();
    let body = blend_fillet_executive_p3::result_body_of(&mut k, CASE);
    let (vertices, edges, faces) = (
        p1::vertices(&body, None).unwrap(),
        p1::edges(&body, None).unwrap(),
        p1::faces(&body, None).unwrap(),
    );
    let at_edges = 4 * vertices.len();
    let at_faces = at_edges + 18 * edges.len();
    assert_eq!(
        now.len(),
        at_faces + 18 * faces.len(),
        "{CASE}: record length"
    );
    assert_eq!(main.len(), now.len());
    let moved = |range: std::ops::Range<usize>| -> Vec<usize> {
        range
            .clone()
            .filter(|&i| now[i].to_bits() != main[i].to_bits())
            .map(|i| i - range.start)
            .collect()
    };
    let sphere = |f| match body.face(f).unwrap().surface.as_ref().unwrap().kind() {
        SurfaceKind::Sphere(s) => Some((s.base().position().origin(), s.radius())),
        _ => None,
    };
    // vertices
    assert_eq!(moved(0..at_edges), [0_usize; 0], "{CASE}: vertices");
    // edges: the trim count only, 1 to 2, on edges with a use on a found sphere
    let mut trims = 0;
    for (i, &e) in edges.iter().enumerate() {
        let at = at_edges + 18 * i;
        let m = moved(at..at + 18);
        if m.is_empty() {
            continue;
        }
        assert_eq!(m, [17], "{CASE}: edge {i}");
        assert_eq!(
            (main[at + 17], now[at + 17]),
            (1.0, 2.0),
            "{CASE}: edge {i}"
        );
        let on_sphere = body.edge(e).unwrap().edgeuses.iter().any(|&eu| {
            face_of(&body, eu)
                .unwrap()
                .is_some_and(|f| sphere(f).is_some())
        });
        assert!(on_sphere, "{CASE}: edge {i} has no use on a found sphere");
        trims += 1;
    }
    assert_eq!(trims, 24, "{CASE}: edges with a kept trim");
    // faces: the found spheres' samples only, each on the sphere in both
    let mut sampled = 0;
    for (i, &f) in faces.iter().enumerate() {
        let at = at_faces + 18 * i;
        let m = moved(at..at + 18);
        if m.is_empty() {
            continue;
        }
        let Some((centre, radius)) = sphere(f) else {
            panic!("{CASE}: face {i} moved and is not a found sphere: {m:?}");
        };
        assert!(
            m.iter().all(|&j| j >= 7 && j % 4 != 2),
            "{CASE}: face {i} {m:?}"
        );
        for g in [&now, &main] {
            for s in 0..3 {
                let p = &g[at + 7 + 4 * s..at + 10 + 4 * s];
                assert_eq!(g[at + 6 + 4 * s], 0.0, "{CASE}: face {i} sample {s} status");
                let d = (solidcore::math::Vec3::new(p[0], p[1], p[2]) - centre).length();
                assert!(
                    (d - radius).abs() <= 1e-12 * radius,
                    "{CASE}: face {i} sample {s} {:e} off the sphere",
                    d - radius
                );
            }
        }
        sampled += 1;
    }
    assert!(sampled > 0, "{CASE}: no sphere face moved");
}

/// `check_brep` level 2's lines for `b`, sorted.
fn level2(k: &mut solidcore::Kernel, b: &mut solidcore::topology::Brep) -> Vec<String> {
    use solidcore::topology::brep::validation::{check_brep, CheckLevel};
    let mut lines = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    lines.sort();
    lines
}

/// The level 2 lines `now` reports more often than `main` (as multisets).
fn level2_excess(now: &[String], main: &[String]) -> Vec<String> {
    let mut left: Vec<&String> = main.iter().collect();
    let mut excess = Vec::new();
    for l in now {
        match left.iter().position(|m| *m == l) {
            Some(i) => {
                left.remove(i);
            }
            None => excess.push(l.clone()),
        }
    }
    excess
}

/// Asserts that the body this branch builds reports no level 2 line, counted with its
/// repeats, beyond those main's build (the fitted sphere put back) reports: clean where
/// main's is clean.
fn assert_level2_no_worse(what: &str, now: &[String], main: &[String]) {
    eprintln!(
        "{what}: level 2 lines now {}, main {}",
        now.len(),
        main.len()
    );
    if now != main {
        eprintln!("{what}: now {now:#?}\nmain {main:#?}");
    }
    let excess = level2_excess(now, main);
    assert!(
        excess.is_empty(),
        "{what}: level 2 lines beyond main's: {excess:?}"
    );
}

/// Every body the WR-493 retirement moves passes `check_brep` level 2 wherever main's
/// passes it, and reports nothing main's does not:
/// - every upstream test file whose read finds a sphere (the files are swept, so no read
///   is left out; the refit seam reports the found spheres), read as the runners read
///   it, with and without `bRebuildUVTrimCurves`;
/// - the result body of each moved `blend.fillet_executive_p3` case, but for the mirror
///   pair `sphere_diff_3cylinders-e1-r0.5` / `-e4-r0.5`, whose lines swap between the two
///   cases (asserted exactly: each reports the other's lines on main).
///
/// The lines main's builds already report are those of the files and of main's rounds
/// (`Test #15`, `Test #36` and the like); this branch adds none.
///
/// Main's body is the same build with upstream's fitted NURB put back
/// ([`crate::refit::with_fitted_spheres`]), which `refit::replay` shows to be main's
/// build bit for bit.
#[test]
fn wr493_moved_bodies_pass_level_2() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(crate::fixtures::UPSTREAM_TEST_FILES);
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "smb") {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut found = Vec::new();
    for path in &files {
        let name = path.strip_prefix(&root).unwrap().display().to_string();
        let p = path.to_str().unwrap();
        for rebuild in [false, true] {
            let mut k_main = crate::runner_kernel();
            let (main, replaced) = crate::refit::with_fitted_spheres(|| {
                crate::own_reader::read_file_rebuild(&mut k_main, p, rebuild).unwrap()
            });
            if replaced == 0 {
                continue;
            }
            let mut k = crate::runner_kernel();
            let now = crate::own_reader::read_file_rebuild(&mut k, p, rebuild).unwrap();
            let what = format!("{name} rebuild {rebuild}");
            let (mut now, mut main) = match (now, main) {
                (Ok(n), Ok(m)) => (n, m),
                (n, m) => {
                    assert_eq!(n.is_ok(), m.is_ok(), "{what}: {:?} {:?}", n.err(), m.err());
                    continue;
                }
            };
            let (ln, lm) = (level2(&mut k, &mut now), level2(&mut k_main, &mut main));
            assert_level2_no_worse(&what, &ln, &lm);
            found.push(what);
        }
    }
    eprintln!("files with a found sphere: {found:?}");
    for f in [
        "Fillet/box_w_slot.smb",
        "Fillet/sphere_diff_3cylinders.smb",
        "Fillet/sphere_plane.smb",
        "Solids/nurb_fillet.smb",
        "Solids/revolution_360.smb",
        "Solids/revolution_mold.smb",
        "sphere.smb",
        "surf_sphere150.smb",
    ] {
        assert!(
            found.iter().any(|w| w.starts_with(f)),
            "{f}: no found sphere read"
        );
    }

    let mut pair = Vec::new();
    for case in [
        "source-file-box_w_slot-all-edges",
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.1",
        "stubs4-delete-edge-sphere_diff_3cylinders-e1-r0.5",
        "stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1",
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.1",
        "stubs4-delete-edge-sphere_diff_3cylinders-e4-r0.5",
        "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1",
        "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5",
        "stubs4-sphere_plane-e3-r0.5",
        "sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1",
        "sweep4-sphere_diff_3cylinders-all-r0.1",
        "sweep5-sphere_diff_3cylinders-e29-r1.0",
    ] {
        let mut k = solidcore::Kernel::default();
        let mut now = blend_fillet_executive_p3::result_body_of(&mut k, case);
        let mut k_main = solidcore::Kernel::default();
        let (mut main, replaced) = crate::refit::with_fitted_spheres(|| {
            blend_fillet_executive_p3::result_body_of(&mut k_main, case)
        });
        assert!(replaced > 0, "{case}");
        let (ln, lm) = (level2(&mut k, &mut now), level2(&mut k_main, &mut main));
        if [E1_R05.0, E4_R05.0].contains(&case) {
            pair.push((ln, lm));
        } else {
            assert_level2_no_worse(case, &ln, &lm);
        }
    }
    // The mirror pair `e1-r0.5` / `e4-r0.5` swaps its round edge's end between the two
    // cases (`sphere_diff_r05_round_end_gaps_swap_between_the_mirror_cases`), and with it
    // two `Test #15` lines (internal coincident control points of a B-spline curve, the
    // round's), which main reports on `e1` and this branch on `e4`: each case's lines are
    // the other's on main, so the pair reports exactly main's lines.
    let [(n1, m1), (n4, m4)] = <[_; 2]>::try_from(pair).unwrap();
    assert_eq!(n1, m4, "e1-r0.5 now, e4-r0.5 on main");
    assert_eq!(n4, m1, "e4-r0.5 now, e1-r0.5 on main");
    let excess = level2_excess(&n4, &m4);
    assert_eq!(excess.len(), 2, "{excess:?}");
    assert!(
        excess
            .iter()
            .all(|l| l.starts_with("SmBSplineCurve::Test #15 ")),
        "{excess:?}"
    );
    assert!(level2_excess(&n1, &m1).is_empty());
}

/// The `blend.fillet_executive_p3` `sphere_diff_3cylinders` rows #20179 moves: their
/// result body holds five more UV trims than on main. The file's read now point-tests on
/// the cone face [`CLASSIFIED_TRIM_FACE`] (it has loop uses; main's cache, built in editing
/// mode, stored point testing off), the face classification makes the face's missing trims
/// (`CreateUVTrimCurves`) on the read (`tolsub::state::CLASSIFIED_READ_TRIMS`, the same
/// edge uses), and the fillet keeps them.
pub(crate) const CLASSIFIED_TRIM_ROWS: [&str; 6] = [
    "stubs4-delete-edge-sphere_diff_3cylinders-e2-r0.1",
    "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.1",
    "stubs4-delete-edge-sphere_diff_3cylinders-e5-r0.5",
    "sweep4-native-crash-sphere_diff_3cylinders-e30-r0.1",
    "sweep4-sphere_diff_3cylinders-all-r0.1",
    "sweep5-sphere_diff_3cylinders-e29-r1.0",
];
/// Those rows' result body's vertex count (the record's edges start at 4 per vertex).
const CLASSIFIED_TRIM_VERTICES: usize = 25;
/// The face (in `faces` order) whose classification makes the trims.
const CLASSIFIED_TRIM_FACE: usize = 6;
/// The edges (in `edges` order) whose use on [`CLASSIFIED_TRIM_FACE`] now holds a trim.
const CLASSIFIED_TRIM_EDGES: [usize; 5] = [20, 22, 27, 31, 33];

/// Offset of an edge's trim count in its 18 record entries (`util::primitive::append`).
const TRIM_COUNT: usize = 17;

/// `output` with each [`CLASSIFIED_TRIM_EDGES`] edge's trim count lowered from 2 to 1, for a
/// [`CLASSIFIED_TRIM_ROWS`] row of `blend.fillet_executive_p3` (any other row is left as
/// is): the row the result body records without the trims the classification made
/// ([`sphere_diff_3cylinders_rows_move_only_by_the_classified_trims`] proves it entry for
/// entry). The replays that hold these rows to main's (`refit::replay`, `tolsub::replay`)
/// hash it.
pub(crate) fn without_classified_trims(unit: &str, case: &str, output: &mut crate::output::Output) {
    if unit != "blend.fillet_executive_p3" || !CLASSIFIED_TRIM_ROWS.contains(&case) {
        return;
    }
    let g = output
        .values
        .get_mut("result_geometry")
        .unwrap_or_else(|| panic!("{case}: no result_geometry"));
    for i in CLASSIFIED_TRIM_EDGES {
        let at = 4 * CLASSIFIED_TRIM_VERTICES + 18 * i + TRIM_COUNT;
        assert_eq!(g[at], 2.0, "{case}: edge {i} trim count");
        g[at] = 1.0;
    }
}

/// #20179, the [`CLASSIFIED_TRIM_ROWS`]: their result body differs from main's only by the
/// five trims the classification on [`CLASSIFIED_TRIM_FACE`] made, each within its edge's
/// tolerance of the edge (certified). For each row:
/// * the run's record is the result body's record, and the face is a cone with loop uses;
/// * each [`CLASSIFIED_TRIM_EDGES`] edge has exactly two uses holding a trim, one of them
///   its use on the face;
/// * with those five trims taken off, the body's record is the run's with exactly those
///   five trim counts lowered from 2 to 1 ([`without_classified_trims`]) and nothing else
///   changed, bit for bit; that row hashes to main's row (`refit::replay`'s and
///   `tolsub::replay`'s put-back runs) and digests to main's committed blocks within
///   rounding ([`sphere_diff_3cylinders_rows_are_their_sums`]);
/// * one face classification (`point_classify` at the face's middle) makes the five again,
///   bit for bit the same, and the record is the run's again: they are what the
///   classification makes, not another change;
/// * each trim's same-parameter gap to its edge is certified (`curve_uv_gap_bound`,
///   interval Taylor models, every rounding included) within the edge's tolerance.
///
/// The mass properties do not move (the sums test holds them to main's).
#[test]
fn sphere_diff_3cylinders_rows_move_only_by_the_classified_trims() {
    use solidcore::curve::curve::CurveKind;
    use solidcore::geom::vector2d::Point2;
    use solidcore::query::curve_surface_gap::curve_uv_gap_bound;
    use solidcore::surface::surface::SurfaceKind;
    use solidcore::topology::brep::p1;
    use solidcore::topology::edgeuse::p1 as eu1;
    use solidcore::topology::{edge, face};
    let record = |k: &solidcore::Kernel, b: &solidcore::topology::Brep| {
        let mut out = Vec::new();
        crate::util::primitive::append(k, b, &mut out).unwrap();
        out
    };
    let encoding = |k: &solidcore::Kernel, c: &solidcore::curve::curve::Curve| {
        let mut enc = Vec::new();
        crate::util::trim_curve::put_trim_curve(k, &mut enc, Some(c)).unwrap();
        enc.iter().map(|x| x.to_bits()).collect::<Vec<u64>>()
    };
    for case in CLASSIFIED_TRIM_ROWS {
        let mut output = run(&blend_fillet_executive_p3::Runner, case);
        let now = output.values["result_geometry"].clone();
        without_classified_trims(blend_fillet_executive_p3::Runner.id(), case, &mut output);
        let main = output.values["result_geometry"].clone();
        let mut k = solidcore::Kernel::default();
        let mut body = blend_fillet_executive_p3::result_body_of(&mut k, case);
        assert_eq!(
            record(&k, &body),
            now,
            "{case}: the run is not the body's record"
        );
        assert_eq!(
            p1::vertices(&body, None).unwrap().len(),
            CLASSIFIED_TRIM_VERTICES,
            "{case}: vertices"
        );
        let edges = p1::edges(&body, None).unwrap();
        let f = p1::faces(&body, None).unwrap()[CLASSIFIED_TRIM_FACE];
        let surface = face::p2::surface(&body, f).unwrap().unwrap().clone();
        assert!(
            matches!(surface.kind(), SurfaceKind::Cone(_)),
            "{case}: the face is not a cone"
        );
        assert!(face::p4::has_loopuses(&body, f).unwrap());
        // the five trims, taken off
        let mut taken = Vec::new();
        for i in CLASSIFIED_TRIM_EDGES {
            let e = edges[i];
            let uses = body.edge(e).unwrap().edgeuses.clone();
            let trimmed: Vec<_> = uses
                .iter()
                .copied()
                .filter(|&eu| body.edgeuse(eu).unwrap().uv_trim_curve.is_some())
                .collect();
            assert_eq!(trimmed.len(), 2, "{case}: edge {i}: uses holding a trim");
            let on_face: Vec<_> = trimmed
                .iter()
                .copied()
                .filter(|&eu| eu1::face(&body, eu).unwrap() == Some(f))
                .collect();
            assert_eq!(on_face.len(), 1, "{case}: edge {i}: trims on the face");
            let eu = on_face[0];
            let trim = body.edgeuse_mut(eu).unwrap().uv_trim_curve.take().unwrap();
            // the trim fits its edge: certified same-parameter gap within the tolerance
            let tol = edge::tolerance(&body, e).unwrap();
            let interval = edge::interval(&body, e).unwrap();
            let rail = edge::curve(&body, e).unwrap().unwrap().clone();
            let c = match rail.kind() {
                CurveKind::BSpline(c) => c.clone_exact(),
                CurveKind::Line(l) => l.base_with_nurb().unwrap().into_owned(),
                other => panic!("{case}: edge {i}: an edge curve {other:?}"),
            };
            let w = match trim.kind() {
                CurveKind::BSpline(w) => w.clone_exact(),
                CurveKind::Line(l) => l.base_with_nurb().unwrap().into_owned(),
                other => panic!("{case}: edge {i}: a trim {other:?}"),
            };
            let gap = curve_uv_gap_bound(
                &k,
                &c,
                &w,
                &surface,
                [interval.min(), interval.max()],
                tol / 100.0,
            )
            .unwrap()
            .unwrap_or_else(|| panic!("{case}: edge {i}: the gap is not enclosed"));
            assert!(
                gap <= tol,
                "{case}: edge {i}: certified gap {gap:e} over the tolerance {tol:e}"
            );
            taken.push((i, eu, encoding(&k, &trim)));
        }
        let without = record(&k, &body);
        let moved: Vec<usize> = (0..now.len())
            .filter(|&j| without[j].to_bits() != now[j].to_bits())
            .collect();
        let at = |i: usize| 4 * CLASSIFIED_TRIM_VERTICES + 18 * i + TRIM_COUNT;
        assert_eq!(
            moved,
            CLASSIFIED_TRIM_EDGES.map(at).to_vec(),
            "{case}: entries the five trims move"
        );
        assert_eq!(
            without, main,
            "{case}: the record without them is not the row"
        );
        let mut ids: Vec<u32> = taken.iter().map(|t| t.1.index()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            crate::tolsub::state::CLASSIFIED_READ_TRIMS,
            "{case}: not the read's classified trims"
        );
        // one classification on the face makes them again, bit for bit
        let d = body.face(f).unwrap().uv_domain;
        let uv = Point2::new(0.5 * (d.min().x + d.max().x), 0.5 * (d.min().y + d.max().y));
        let mut pc = solidcore::intersect::point_class::PointClassification::new(0.0);
        solidcore::query::face::p2::point_classify(
            &k,
            &mut body,
            f,
            uv,
            &mut pc,
            solidcore::query::face::p2::PointClassifyOptions::default(),
        )
        .unwrap();
        for (i, eu, bits) in &taken {
            let again = body.edgeuse(*eu).unwrap().uv_trim_curve.as_ref();
            let again = again.unwrap_or_else(|| panic!("{case}: edge {i}: not made again"));
            assert_eq!(&encoding(&k, again), bits, "{case}: edge {i}: another trim");
        }
        assert_eq!(
            record(&k, &body),
            now,
            "{case}: the record after the classification"
        );
    }
}

/// #20179, `corpus.parity`'s `Fillet__sphere_diff_3cylinders.smb`: the read now classifies
/// on the cone face whose cache was built in editing mode (main's stored point testing off
/// there), and the classification makes five of its missing UV trims, so the area is
/// integrated over those trims (with main's read put back the row is main's to the bit,
/// `tolsub::replay::classified_read_rows_are_mains_without_the_trims`): it moves by 1.3e-9 of
/// itself (`area` 38235.9926466384 to 38235.99269705083, `rt_area` 38235.99654566674 to
/// 38235.996596079174). The record asks the area to `ACCURACY` (`1e-6`) relative. A
/// certified enclosure of the read's total face area (`enclose_face_area_relative`, every
/// rounding included, asked `1e-8` wide) holds main's and this branch's values, both sides,
/// each within that accuracy of it: the move is a re-integration within the requested
/// accuracy, not a change of the body. The retirement of `uv_trim_corner_end_points_only`
/// (#21272: the read closes its faces' trim corners by moving only the trims' end control
/// points) moves both again, to 38235.9926957725 and 38235.99659480028 (by 3.3e-11 of
/// themselves), both within `ACCURACY` of the same certified enclosure (asserted below), and
/// `tests/uvtc_corpus_row.rs` classifies that move against a certified enclosure.
#[test]
fn sphere_diff_3cylinders_corpus_area_is_within_its_accuracy() {
    use crate::units::corpus_parity::{run_in_process, Runner, ACCURACY};
    use solidcore::mass::enclosure::enclose_face_area_relative;
    use solidcore::topology::brep::p1;
    const CASE: &str = "Fillet__sphere_diff_3cylinders.smb";
    // (key, main's committed value, this branch's)
    const VALUES: [(&str, f64, f64); 2] = [
        ("area", 38_235.992_646_638_4, 38_235.992_695_772_5),
        ("rt_area", 38_235.996_545_666_74, 38_235.996_594_800_28),
    ];
    let out = run_in_process(&regression_case(&Runner, CASE)).unwrap();
    for (key, _, now) in VALUES {
        assert_eq!(out.values[key], [now], "{CASE}: {key}");
    }
    let path = crate::case::repo_root()
        .join(crate::fixtures::UPSTREAM_TEST_FILES)
        .join("Fillet/sphere_diff_3cylinders.smb");
    let mut k = crate::runner_kernel();
    let mut b = crate::own_reader::read_file(&mut k, path.to_str().unwrap())
        .unwrap()
        .unwrap();
    let faces = p1::faces(&b, None).unwrap();
    let e = enclose_face_area_relative(&k, &mut b, &faces, 1e-8).unwrap();
    assert!(
        e.lo.is_finite() && e.hi.is_finite() && e.lo <= e.hi,
        "{CASE}: {e:?}"
    );
    for (key, main, now) in VALUES {
        for v in [main, now] {
            let off = (v - e.lo).abs().max((e.hi - v).abs());
            assert!(
                off <= ACCURACY * v,
                "{CASE}: {key} {v} is {off:e} from the enclosure [{}, {}]",
                e.lo,
                e.hi
            );
        }
    }
}
