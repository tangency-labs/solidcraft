// SPDX-License-Identifier: Apache-2.0
//! The depth the source-less writer walks over an inline pcurve basis.

use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, TrimmedPcurve};
use cadmpeg_ir::geometry::MAX_GEOMETRY_NESTING;
use cadmpeg_ir::math::Point2;

use super::native_pcurve_geometry;

fn trimmed_chain(carriers: usize) -> Result<PcurveGeometry, &'static str> {
    let mut geometry = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            None,
            false,
        )
        .expect("a degree-one pcurve over two poles"),
    };
    for _ in 0..carriers {
        geometry = PcurveGeometry::Trimmed(TrimmedPcurve::try_new(
            [0.0, 1.0],
            true,
            Box::new(geometry),
        )?);
    }
    Ok(geometry)
}

#[test]
fn a_pcurve_nesting_chain_past_the_bound_is_unwritable() {
    let accepted = trimmed_chain(MAX_GEOMETRY_NESTING).expect("admitted nesting");
    assert!(
        native_pcurve_geometry(&accepted, [0.0, 1.0]).is_ok(),
        "a chain at the admitted depth is written"
    );

    // One carrier deeper is unwritable because it is unbuildable.
    assert_eq!(
        trimmed_chain(MAX_GEOMETRY_NESTING + 1),
        Err("TrimmedPcurve.basis nests past the admitted inline basis depth")
    );
}
