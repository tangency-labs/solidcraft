//! Split Body by the face of a cone that narrows along its axis (#13917).
use crate::*;
use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

/// #13917: a box split by the side of a tapered extrusion of a circle gives two parts whose
/// volumes sum to the box's, whether the cone widens along the extrusion (a positive taper) or
/// narrows (a negative one: its record's half angle is negative, which the SMLib surface for the
/// split tool refused as unsupported).
#[test]
fn a_box_splits_by_a_cone_narrowing_along_its_axis() {
    let a = box_solid(Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, 20.0, 30.0)).unwrap();
    let circle = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![] };
    for taper in [0.15f64, -0.15] {
        let cone = extrude_tapered(&Plane::XY.offset(-5.0), &circle, 40.0, 1.0, taper, DraftCorner::default()).unwrap();
        let side = cone
            .record()
            .faces
            .iter()
            .position(|f| matches!(f.surface, solidcore_record::brep::Surface::Cone { .. }))
            .unwrap_or_else(|| panic!("taper {taper}: no cone face"));
        let parts = split_body(&a, &SplitTool::Face { body: &cone, face: side }).unwrap_or_else(|e| panic!("taper {taper}: {e}"));
        assert_eq!(parts.len(), 2, "taper {taper}");
        let total: f64 = parts.iter().map(|p| measure(p).unwrap().volume).sum();
        assert!((total - 48000.0).abs() < 1e-6 * 48000.0, "taper {taper}: {total}");
        for p in &parts {
            assert!(p.validity().is_empty(), "taper {taper}: {:?}", p.validity());
        }
    }
}
