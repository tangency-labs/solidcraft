use solidcraft_geom::Vec3;

use super::*;
use crate::IDENTITY;
use crate::point_cloud::{ColorBy, Colors, Column, Complex, Floats, Indices, Points};

/// A cloud of `points`, with `segments` and `triangles` between them and `columns` on them.
fn cloud(points: &[[f32; 3]], segments: &[u32], triangles: &[u32], columns: Vec<Column>) -> PointCloud {
    PointCloud {
        id: 1,
        name: "c".into(),
        file: String::new(),
        origin: Vec3::new(0.0, 0.0, 0.0),
        points: Points::new(points.iter().flatten().copied().collect::<Vec<f32>>().into()),
        colors: Colors::default(),
        source_points: points.len() as u64,
        units: "mm".into(),
        transform: IDENTITY,
        point_size: 2.0,
        color_by: ColorBy::File,
        color: None,
        height_range: None,
        visible: true,
        complex: Complex { segments: Indices(segments.to_vec().into()), triangles: Indices(triangles.to_vec().into()), columns, names: Vec::new() },
        color_property: None,
        filters: Vec::new(),
        source: String::new(),
    }
}

fn column(name: &str, on: On, integer: bool, values: &[f32]) -> Column {
    Column { name: name.into(), on, integer, values: Floats(values.to_vec().into()) }
}

fn range(property: &str, range: [f64; 2]) -> Filter {
    Filter { property: property.into(), keep: Keep::Range { range } }
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

#[test]
fn a_segment_spreads_its_length_evenly_over_what_a_range_keeps() {
    // t runs 0 to 10 along a segment 10 long.
    let c = cloud(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], &[0, 1], &[], vec![column("t", On::Point, false, &[0.0, 10.0])]);
    let g = Geometry::of(&c);
    let all = histogram(&c, &g, "t", [0.0, 10.0], 10, &Colour::One, &[]).unwrap();
    assert!(close(&all.slots[0], &[0.1; 10]));
    // Kept from t = 2.5 to 5: the part is cut at the iso-values, as the view cuts it.
    let kept = histogram(&c, &g, "t", [0.0, 10.0], 10, &Colour::One, &[range("t", [2.5, 5.0])]).unwrap();
    assert!(close(&kept.slots[0], &[0.0, 0.0, 0.05, 0.1, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0]));
    // Filtered by another property along the same segment: x from 0 to 4 keeps t from 0 to 4.
    let by_x = histogram(&c, &g, "t", [0.0, 10.0], 5, &Colour::One, &[range("x", [0.0, 4.0])]).unwrap();
    assert!(close(&by_x.slots[0], &[0.2, 0.2, 0.0, 0.0, 0.0]));
    // An axis or a number of bins it can't have.
    for (axis, bins) in [([1.0, 1.0], 4), ([0.0, f64::NAN], 4), ([0.0, 1.0], 0), ([0.0, 1.0], MAX_BINS + 1)] {
        assert!(histogram(&c, &g, "t", axis, bins, &Colour::One, &[]).is_none());
    }
}

#[test]
fn a_triangle_spreads_its_area_as_a_tent_over_its_corners_values() {
    // t is 0, 1 and 2 at the corners: the measure rises to t = 1 and falls to 2.
    let c = cloud(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[], &[0, 1, 2], vec![column("t", On::Point, false, &[0.0, 1.0, 2.0])]);
    let g = Geometry::of(&c);
    let h = histogram(&c, &g, "t", [0.0, 2.0], 4, &Colour::One, &[]).unwrap();
    // The distribution function: x²/2 up to 1, then 1 − (2 − x)²/2.
    assert!(close(&h.slots[0], &[0.125, 0.375, 0.375, 0.125]));
    // Cut at t ≤ 1: half the area is left, all below 1.
    let half = histogram(&c, &g, "t", [0.0, 2.0], 4, &Colour::One, &[range("t", [0.0, 1.0])]).unwrap();
    assert!(close(&half.slots[0], &[0.125, 0.375, 0.0, 0.0]));
    // Cut by x across t: the part where x ≤ 0.5, its area 3/4 of the triangle's.
    let x = histogram(&c, &g, "t", [0.0, 2.0], 1, &Colour::One, &[range("x", [0.0, 0.5])]).unwrap();
    assert!(close(&x.slots[0], &[0.75]));
}

#[test]
fn kinds_weigh_alike_and_colours_split_what_is_kept() {
    // Two segments, roles 0 and 3, and a dot; t along each segment, the dot at t = 5.
    let c = cloud(
        &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [4.0, 0.0, 0.0], [9.0, 9.0, 9.0]],
        &[0, 1, 2, 3],
        &[],
        vec![
            column("t", On::Point, false, &[0.0, 1.0, 1.0, 4.0, 5.0]),
            column("role", On::Segment, true, &[0.0, 3.0]),
            column("role", On::Point, true, &[0.0, 0.0, 0.0, 0.0, 2.0]),
        ],
    );
    let g = Geometry::of(&c);
    let colour = Colour::Palette { property: "role" };
    let h = histogram(&c, &g, "t", [0.0, 5.0], 5, &colour, &[]).unwrap();
    assert_eq!(h.slots.len(), colour.slots());
    // The segments share their kind's 1 by length (1 and 3), the dot has its kind's 1.
    assert!(close(&h.slots[0], &[0.25, 0.0, 0.0, 0.0, 0.0]));
    assert!(close(&h.slots[3], &[0.0, 0.25, 0.25, 0.25, 0.0]));
    assert!(close(&h.slots[2], &[0.0, 0.0, 0.0, 0.0, 1.0]));
    // A filter by values keeps role 3 only; the dot has a role too and is left out.
    let three = Filter { property: "role".into(), keep: Keep::Values { values: vec![3] } };
    let kept = histogram(&c, &g, "t", [0.0, 5.0], 5, &colour, &[three]).unwrap();
    let total: f64 = kept.slots.iter().flatten().sum();
    assert!((total - 0.75).abs() < 1e-9 && close(&kept.slots[3], &h.slots[3]));
    // A ramp over t in two bands splits the long segment where t crosses 2.5.
    let ramp = Colour::Ramp { property: "t", range: [0.0, 5.0], bands: 2 };
    let r = histogram(&c, &g, "t", [0.0, 5.0], 5, &ramp, &[]).unwrap();
    assert!(close(&r.slots[0], &[0.25, 0.25, 0.125, 0.0, 0.0]) && close(&r.slots[1], &[0.0, 0.0, 0.125, 0.25, 1.0]));
}

#[test]
fn labels_change_where_the_next_element_has_another() {
    // Five segments in order of t, layers 0 0 1 1 2.
    let points: Vec<[f32; 3]> = (0..6).map(|i| [i as f32, 0.0, 0.0]).collect();
    let c = cloud(
        &points,
        &[0, 1, 1, 2, 2, 3, 3, 4, 4, 5],
        &[],
        vec![column("t", On::Point, false, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]), column("layer", On::Segment, true, &[0.0, 0.0, 1.0, 1.0, 2.0])],
    );
    let g = Geometry::of(&c);
    assert_eq!(label_changes(&c, &g, "layer", "t"), vec![2.0, 4.0]);
    assert!(label_changes(&c, &g, "nope", "t").is_empty());
}

#[test]
fn the_automatic_range_is_what_every_filter_keeps() {
    // t runs 0 to 10 along x; filtered by x from 2 to 4, coloured by t: the range is 2 to 4,
    // though no filter is on t (its span within filters on t would be 0 to 10).
    let mut c = cloud(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [20.0, 0.0, 0.0]], &[0, 1], &[], vec![column("t", On::Point, false, &[0.0, 10.0, 7.0])]);
    c.color_by = ColorBy::Property;
    c.color_property = Some("t".into());
    let g = Geometry::of(&c);
    let x = [range("x", [2.0, 4.0])];
    assert_eq!(c.color_range(), Some([0.0, 10.0]));
    assert_eq!(color_range(&c, &g, &x), Some([2.0, 4.0]));
    // The dot at x = 20 is kept by nothing; by itself it would be one value, one unit wide.
    assert_eq!(color_range(&c, &g, &[range("x", [19.0, 21.0])]), Some([6.5, 7.5]));
    // Nothing kept: the cloud's own bound; a range set is kept as it is.
    assert_eq!(color_range(&c, &g, &[range("x", [50.0, 60.0])]), Some([0.0, 10.0]));
    c.height_range = Some([1.0, 2.0]);
    assert_eq!(color_range(&c, &g, &x), Some([1.0, 2.0]));
    let h = histogram(&c, &g, "t", [0.0, 1.0], 4, &Colour::One, &x).unwrap();
    assert_eq!(h.extent, Some([2.0, 4.0]), "the extent reaches past the axis");
}
