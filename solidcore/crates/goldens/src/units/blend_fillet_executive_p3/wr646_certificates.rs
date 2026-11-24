//! WR-646 (#18917): a variable round's span is checked against the fillet solved at its
//! test shares, so the round follows its radius law between traced points. The seven
//! `law-*` rows it moves keep their status and counts and change only their geometry and
//! mass values. Each row is checked here, on the finished body, one test per row:
//! - the fillet succeeds and the vertex, edge and face counts are the golden's, unchanged;
//! - `check_brep` Level 2 reports nothing and the body is one closed manifold solid;
//! - the volume the mass integrator converges to lies in the body's certified volume
//!   enclosure ([`enclose_volume`]), and every other mass value lies in its certified
//!   band ([`mass_bands`]). This is a consistency check: it certifies that the values are
//!   this body's, not that the body is the true round.
//! - the linear-section row's certified enclosure meets a necessary condition from the
//!   closed form, and main's volume fails it. A linear section cuts the triangle `r^2 / 2`
//!   from the box, so with the law `r(u) = 4 - 4u + 4u^2` along the 20-long edge it
//!   removes `20 * int_0^1 r^2 du / 2 = 20 * 11.2 / 2 = 112`. Premise (not checked here):
//!   the round's skin lies within `T = 4 * tol` (the edge's tolerance) of that ruled chord
//!   surface. Then, to first order, the body's volume is within `T * A` of `8000 - 112`,
//!   with `A` the chord surface's area, bounded above by
//!   `sqrt(2.04) * 20 * int_0^1 r du = sqrt(2.04) * 20 * 10 / 3` (the chord is `sqrt(2) r`
//!   long and tilts by at most `|r'(u)| / 20 <= 0.2` along the edge). Main's body removed
//!   `112.0517` (off by `5.2e-2`, past that bound), so main's value is refuted.
//!
//! The six ball-section rows have no closed form. Their removed volume is compared with
//! the balls' envelope by a converged (non-interval) quadrature in
//! `solidcore/crates/solidcore/tests/wr646_variable_round_follows_law.rs`; main's and the
//! new values both lie within `T` times the envelope's area of it, the new ones 3.6 to 10
//! times closer. The skin-to-envelope certificate there is an ignored proof run.
//!
//! [`enclose_volume`]: solidcore::mass::enclosure::enclose_volume
//! [`mass_bands`]: super::round_definition_proof::mass_bands

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use super::round_definition_proof::{mass_bands, volume_band};
use super::round_definition_rows::{counts, inputs};
use super::*;
use solidcore::mass::enclosure::enclose_volume;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};

/// A moved row: name, main's volume (the golden's before), and its status and counts
/// (the same before and after).
type Row = (&'static str, f64, [i64; 4]);

const ROWS: &[Row] = &[
    ("law-box10x20x30-e0-deg2-edge-map-reversed", 5986.9198957359595, [0, 10, 15, 7]),
    ("law-box10x20x30-e0-deg2-edge-map-sub-range", 5986.9198957359595, [0, 10, 15, 7]),
    ("law-cube20-e0-deg2-smooth-4-2-4", 7952.165588609178, [0, 10, 15, 7]),
    ("law-cube20-e0-deg2-smooth-g2", 7952.165588609178, [0, 10, 15, 7]),
    ("law-cube20-e0-deg2-smooth-xsect-linear", 7887.9482893284285, [0, 10, 15, 7]),
    ("law-cube20-e0-deg3-asymmetric", 7953.6430498032405, [0, 10, 15, 7]),
    ("law-cube20-e7-deg3-asymmetric", 7953.643049803992, [0, 10, 15, 7]),
];

/// The linear-section row, whose removed volume has a closed form.
const LINEAR: &str = "law-cube20-e0-deg2-smooth-xsect-linear";

/// One test per moved row, so the test runner spreads the rows over its threads.
macro_rules! row_tests {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                check($name);
            }
        )*

        /// The rows the generated tests take, in [`ROWS`]'s order.
        const ROW_TESTS: &[&str] = &[$($name),*];
    };
}

row_tests! {
    law_box10x20x30_e0_deg2_edge_map_reversed => "law-box10x20x30-e0-deg2-edge-map-reversed",
    law_box10x20x30_e0_deg2_edge_map_sub_range => "law-box10x20x30-e0-deg2-edge-map-sub-range",
    law_cube20_e0_deg2_smooth_4_2_4 => "law-cube20-e0-deg2-smooth-4-2-4",
    law_cube20_e0_deg2_smooth_g2 => "law-cube20-e0-deg2-smooth-g2",
    law_cube20_e0_deg2_smooth_xsect_linear => "law-cube20-e0-deg2-smooth-xsect-linear",
    law_cube20_e0_deg3_asymmetric => "law-cube20-e0-deg3-asymmetric",
    law_cube20_e7_deg3_asymmetric => "law-cube20-e7-deg3-asymmetric",
}

/// Every row of [`ROWS`] has exactly one test.
#[test]
fn every_moved_row_has_its_own_test() {
    let rows: Vec<&str> = ROWS.iter().map(|row| row.0).collect();
    assert_eq!(rows, ROW_TESTS);
}

/// The [`ROWS`] row named `row`, as the module header says.
fn check(row: &str) {
    let picked: Vec<&Row> = ROWS.iter().filter(|r| r.0 == row).collect();
    assert_eq!(picked.len(), 1, "{row}: one row of that name");
    for &&(name, old_volume, before) in &picked {
        let input = inputs(name).real_solid.expect("a real solid");
        assert_eq!(input.checkpoint, Checkpoint::Full, "{name}");
        assert!(input.radius_law.is_some(), "{name}: a radius law");
        let mut k = Kernel::default();
        let mut body = build(&mut k, &input).unwrap().unwrap();
        let (selected, _) = select_edges(&k, &body, &input).ok().unwrap();
        assert_eq!(selected.len(), 1, "{name}");
        let tol = edge::tolerance(&body, selected[0]).unwrap();
        let result = fillet_selected(&mut k, &mut body, &selected, &input).ok().unwrap();
        assert!(result.is_ok(), "{name}: {result:?}");
        assert_eq!(counts(&body, 0), before, "{name}");
        let lines = check_brep(&mut k, &mut body, CheckLevel::Level2).unwrap().lines();
        assert!(lines.is_empty(), "{name}: Level 2: {lines:?}");
        assert!(is_manifold_solid(&body).unwrap(), "{name}: not one closed manifold solid");
        // the law rounds are bare nets, not definitions: no spread to add
        let estimates = volume_band(&k, &mut body, &[], old_volume, name);
        mass_bands(&k, &mut body, &estimates, name);
        if name == LINEAR {
            linear_holds_its_closed_form(&k, &mut body, tol, old_volume);
        }
    }
}

/// The linear-section row's certified volume enclosure lies within `T * A` of
/// `8000 - 112` (the module header derives both), and main's volume did not.
fn linear_holds_its_closed_form(k: &Kernel, body: &mut Brep, tol: f64, old_volume: f64) {
    let exact = 8000.0 - 112.0;
    let area = 2.04_f64.sqrt() * 20.0 * 10.0 / 3.0;
    let bound = 4.0 * tol * area;
    let e = enclose_volume(k, body, 1e-9 * exact).unwrap();
    eprintln!(
        "{LINEAR}: volume in [{}, {}], closed form {exact}, bound {bound:e}, main {old_volume}",
        e.lo, e.hi
    );
    assert!(
        exact - bound <= e.lo && e.hi <= exact + bound,
        "{LINEAR}: [{}, {}] not within {bound:e} of {exact}",
        e.lo,
        e.hi
    );
    assert!(
        (old_volume - exact).abs() > bound,
        "{LINEAR}: main's {old_volume} was already within {bound:e} of {exact}"
    );
}
