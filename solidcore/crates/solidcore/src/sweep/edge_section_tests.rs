//! Closed-form areas of the original router sections.

use super::*;
use crate::prism::profile::Loop2;
use std::f64::consts::PI;

#[test]
fn router_section_areas_are_closed_forms() {
    let sizes = CornerSizes {
        radius: 6.0,
        width: 12.0,
        depth: 12.0,
        angle: PI / 4.0,
        step: 1.0,
        fillet: 0.0,
    };
    for (profile, expected) in [
        (CornerProfile::Chamfer, 72.0),
        (CornerProfile::Bevel, 72.0),
        (CornerProfile::Roundover, 36.0 * (1.0 - PI / 4.0)),
        (CornerProfile::Cove, 9.0 * PI),
        (CornerProfile::Rabbet, 144.0),
        (CornerProfile::Ogee, 72.0),
        (CornerProfile::Bead, 36.0 * (1.0 - PI / 4.0) + 15.0),
    ] {
        let segs = corner_section(profile, sizes).unwrap();
        let area = Loop2 { segs }.signed_area().unwrap();
        assert!(
            (area.value - expected).abs() < 1e-10,
            "{profile:?}: {} != {expected}",
            area.value
        );
    }
}
