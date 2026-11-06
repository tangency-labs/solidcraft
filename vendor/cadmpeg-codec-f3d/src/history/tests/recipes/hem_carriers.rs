// SPDX-License-Identifier: Apache-2.0

#[test]
fn hem_bend_carriers_prove_directional_gap_forms() {
    use crate::history_records::AsmHistoricalCylinder;
    use cadmpeg_ir::math::{Point3, Vector3};

    let cylinder = |radius| AsmHistoricalCylinder {
        surface: 1,
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        radius,
    };
    let flat_inner = cylinder(0.01);
    let flat_outer = cylinder(2.51);
    assert_eq!(
        super::super::super::hem_gap_length_form([&flat_inner, &flat_outer].into_iter()),
        Some(super::super::super::HemGapLengthForm::Flat)
    );

    let open_inner = cylinder(1.25);
    let open_outer = cylinder(3.75);
    assert_eq!(
        super::super::super::hem_gap_length_form([&open_inner, &open_outer].into_iter()),
        Some(super::super::super::HemGapLengthForm::Open)
    );

    assert_eq!(
        super::super::super::hem_gap_length_form([&flat_inner].into_iter()),
        None
    );
}
