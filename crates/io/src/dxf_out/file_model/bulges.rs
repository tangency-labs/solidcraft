//! Bounds of owned bulged polylines through the kernel's interval primitive.
use super::super::DxfVertex;
#[cfg(test)]
use super::super::path::MAX_PATH_ITEMS;
use super::DxfGeometryBounds;
use crate::Result;
#[cfg(test)]
use solidcore_record::Vec2;
#[cfg(test)]
const MAX_WORK: usize = 64 << 20;
pub(super) fn bounds(vertices: &[DxfVertex], closed: bool, max_vertices: usize, max_work: usize) -> Result<DxfGeometryBounds> {
    let data = vertices.iter().map(|v| solidcore_ops::path_export::Vertex { at: v.at, bulge: v.bulge });
    let b =
        solidcore_ops::export_geometry::bulged_bounds(data, vertices.len(), closed, max_vertices, max_work).map_err(super::geometry::export_error)?;
    Ok(DxfGeometryBounds { min: b.min, max: b.max })
}
#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, bulge: f64) -> DxfVertex {
        DxfVertex { at: Vec2::new(x, y), bulge }
    }
    fn read(vertices: &[DxfVertex], closed: bool) -> Result<DxfGeometryBounds> {
        bounds(vertices, closed, MAX_PATH_ITEMS, MAX_WORK)
    }
    fn contains(b: DxfGeometryBounds, x: f64, y: f64) -> bool {
        b.min.x <= x && x <= b.max.x && b.min.y <= y && y <= b.max.y
    }

    #[test]
    fn lines_use_exact_endpoint_hull_without_arc_arithmetic() {
        let b = read(&[v(-2.0, 3.0, 0.0), v(4.0, -1.0, 0.0), v(0.0, 8.0, 0.0)], false).unwrap();
        assert_eq!(b, DxfGeometryBounds { min: Vec2::new(-2.0, -1.0), max: Vec2::new(4.0, 8.0) });
        assert!(read(&[v(-f64::MAX, 0.0, 0.0), v(f64::MAX, 0.0, 0.0)], false).is_ok());
    }

    #[test]
    fn signed_semicircles_include_all_four_exact_support_cardinals() {
        for bulge in [-1.0, 1.0] {
            let b = read(&[v(-1.0, 0.0, bulge), v(1.0, 0.0, 0.0)], false).unwrap();
            for (x, y) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                assert!(contains(b, x, y));
            }
        }
    }

    #[test]
    fn asymmetric_signed_bulges_use_analytic_centre_and_radius_not_writer_output() {
        // Chord(0,0)->(4,0), b=1/2: centre(2,3/2), radius5/2.
        for (bulge, cy) in [(0.5, 1.5), (-0.5, -1.5)] {
            let b = read(&[v(0.0, 0.0, bulge), v(4.0, 0.0, 0.0)], false).unwrap();
            for (x, y) in [(-0.5, cy), (4.5, cy), (2.0, cy - 2.5), (2.0, cy + 2.5)] {
                assert!(contains(b, x, y));
            }
        }
        // Oblique chord(0,0)->(4,3), b=1/2: centre(7/8,3), r=25/8.
        let b = read(&[v(0.0, 0.0, 0.5), v(4.0, 3.0, 0.0)], false).unwrap();
        for (x, y) in [(-2.25, 3.0), (4.0, 3.0), (0.875, -0.125), (0.875, 6.125)] {
            assert!(contains(b, x, y));
        }
    }

    #[test]
    fn closed_last_bulge_is_used_and_duplicate_seam_is_refused() {
        let b = read(&[v(-1.0, 0.0, 0.0), v(1.0, 0.0, 1.0)], true).unwrap();
        assert!(contains(b, 0.0, 1.0) && contains(b, 0.0, -1.0));
        assert!(read(&[v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(0.0, 0.0, 0.0)], true).is_err());
        assert!(read(&[v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.5)], false).is_err());
    }

    #[test]
    fn nonfinite_or_out_of_domain_values_refuse_without_mutating_inputs() {
        for input in [
            [v(f64::NAN, 0.0, 0.0), v(1.0, 0.0, 0.0)],
            [v(0.0, 0.0, f64::INFINITY), v(1.0, 0.0, 0.0)],
            [v(0.0, 0.0, 1.0000000000000002), v(1.0, 0.0, 0.0)],
            [v(0.0, 0.0, -1.0000000000000002), v(1.0, 0.0, 0.0)],
            [v(0.0, 0.0, 0.0), v(1.0, f64::NEG_INFINITY, 0.0)],
        ] {
            let before = input.map(|v| (v.at.x.to_bits(), v.at.y.to_bits(), v.bulge.to_bits()));
            assert!(read(&input, false).is_err());
            assert_eq!(input.map(|v| (v.at.x.to_bits(), v.at.y.to_bits(), v.bulge.to_bits())), before);
        }
    }

    #[test]
    fn tiny_representable_support_survives_without_squaring_chord() {
        let b = read(&[v(0.0, 0.0, 1.0), v(1e-200, 0.0, 0.0)], false).unwrap();
        assert!(contains(b, 0.0, 0.0) && contains(b, 1e-200, 0.0));
        assert!(b.min.y < 0.0 && b.max.y > 0.0 && b.min.is_finite() && b.max.is_finite());
        assert!(read(&[v(0.0, 0.0, 1.0), v(f64::from_bits(1), 0.0, 0.0)], false).is_err());
        assert!(read(&[v(0.0, 0.0, 1e-300), v(1.0, 0.0, 0.0)], false).is_err());
    }

    #[test]
    fn overflow_or_outward_infinite_support_is_explicitly_refused() {
        assert!(read(&[v(-f64::MAX, 0.0, 1.0), v(f64::MAX, 0.0, 0.0)], false).is_err());
        assert!(read(&[v(f64::MAX, 0.0, 0.5), v(f64::MAX, 1e308, 0.0)], false).is_err());
    }

    #[test]
    fn vertex_and_work_admission_precede_payload_scan_and_are_inclusive() {
        let input = [v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0)];
        assert!(bounds(&input, false, 2, 514).is_ok());
        assert!(bounds(&input, false, 1, 514).is_err());
        assert!(bounds(&input, false, 2, 513).is_err());
        assert!(bounds(&input, true, 2, 1026).is_ok());
        assert!(bounds(&input, true, 2, 1025).is_err());
        let hostile = [v(f64::NAN, 0.0, 0.0), v(1.0, 0.0, 0.0)];
        assert!(bounds(&hostile, false, 2, 513).unwrap_err().to_string().contains("work budget"));
        assert!(bounds(&hostile, false, 1, usize::MAX).unwrap_err().to_string().contains("vertex count"));
        assert!(bounds(&[], false, usize::MAX, usize::MAX).is_err());
        assert!(bounds(&input[..1], false, usize::MAX, usize::MAX).is_err());
    }

    #[test]
    fn authored_zero_length_line_keeps_exact_point_bounds_and_input_bits() {
        // Exact migrated former Err fixture, now SUPPORTED to match X1's
        // original polylines() zero-length LINE -> open two-vertex entity.
        let original = [v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0)];
        assert_eq!(read(&original, false).unwrap(), DxfGeometryBounds { min: Vec2::ZERO, max: Vec2::ZERO });
        for point in [Vec2::new(3.0, -4.0), Vec2::new(f64::from_bits(1), 0.0), Vec2::new(f64::MAX, -f64::MAX)] {
            let input = [DxfVertex { at: point, bulge: 0.0 }, DxfVertex { at: point, bulge: 0.0 }];
            let before = input.map(|v| (v.at.x.to_bits(), v.at.y.to_bits(), v.bulge.to_bits()));
            assert_eq!(read(&input, false).unwrap(), DxfGeometryBounds { min: point, max: point });
            assert_eq!(input.map(|v| (v.at.x.to_bits(), v.at.y.to_bits(), v.bulge.to_bits())), before);
        }
        let mixed = [v(2.0, 3.0, 0.0), v(2.0, 3.0, 0.0), v(5.0, 7.0, 0.0)];
        assert_eq!(read(&mixed, false).unwrap(), DxfGeometryBounds { min: Vec2::new(2.0, 3.0), max: Vec2::new(5.0, 7.0) });
    }

    #[test]
    fn zero_chord_arc_and_explicit_closed_duplicate_seam_still_refuse() {
        for bulge in [-1.0, -0.5, 0.5, 1.0] {
            assert!(read(&[v(2.0, 3.0, bulge), v(2.0, 3.0, 0.0)], false).is_err());
        }
        for input in [
            vec![v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0)],
            vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(0.0, 0.0, 0.0)],
            vec![v(0.0, 0.0, 1.0), v(1.0, 0.0, 0.0), v(0.0, 0.0, 0.0)],
        ] {
            assert!(read(&input, true).unwrap_err().to_string().contains("closed polyline must not duplicate"));
        }
        let nonfinite = [v(f64::INFINITY, 0.0, 0.0), v(f64::INFINITY, 0.0, 0.0)];
        assert!(read(&nonfinite, false).is_err());
        assert!(bounds(&[v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0)], false, 2, 513).is_err());
    }
}
