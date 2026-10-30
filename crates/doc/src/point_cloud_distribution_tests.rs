use solidcraft_geom::Vec3;

use super::*;
use crate::IDENTITY;
use crate::point_cloud::{ColorBy, Colors, Column, Complex, Floats, Points};

/// A cloud of `points` with `columns` on them.
fn cloud(points: &[[f32; 3]], columns: Vec<Column>) -> PointCloud {
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
        complex: Complex { columns, names: Vec::new() },
        color_property: None,
        filters: Vec::new(),
    }
}

fn column(name: &str, integer: bool, values: &[f32]) -> Column {
    Column { name: name.into(), integer, values: Floats(values.to_vec().into()) }
}

fn range(property: &str, range: [f64; 2]) -> Filter {
    Filter { property: property.into(), keep: Keep::Range { range } }
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// Ten points along x, `t` the same as x.
fn row() -> PointCloud {
    let points: Vec<[f32; 3]> = (0..10).map(|i| [i as f32 + 0.5, 0.0, 0.0]).collect();
    let t: Vec<f32> = points.iter().map(|p| p[0]).collect();
    cloud(&points, vec![column("t", false, &t)])
}

#[test]
fn each_point_weighs_alike_and_a_range_keeps_those_within_it() {
    let c = row();
    let g = Geometry::of(&c);
    let all = histogram(&c, &g, "t", [0.0, 10.0], 5, &Colour::One, &[]).unwrap();
    assert!(close(&all.slots[0], &[0.2; 5]));
    // Kept from t = 2 to 5: three points, ends included.
    let kept = histogram(&c, &g, "t", [0.0, 10.0], 5, &Colour::One, &[range("t", [2.0, 5.0])]).unwrap();
    assert!(close(&kept.slots[0], &[0.0, 0.2, 0.1, 0.0, 0.0]));
    // Filtered by another property: x from 0 to 4 keeps t from 0 to 4.
    let by_x = histogram(&c, &g, "t", [0.0, 10.0], 5, &Colour::One, &[range("x", [0.0, 4.0])]).unwrap();
    assert!(close(&by_x.slots[0], &[0.2, 0.2, 0.0, 0.0, 0.0]));
    // A filter on a property the cloud doesn't have keeps everything, as in the view.
    assert_eq!(histogram(&c, &g, "t", [0.0, 10.0], 5, &Colour::One, &[range("nope", [0.0, 1.0])]), Some(all));
    // An axis or a number of bins it can't have.
    for (axis, bins) in [([1.0, 1.0], 4), ([0.0, f64::NAN], 4), ([0.0, 1.0], 0), ([0.0, 1.0], MAX_BINS + 1)] {
        assert!(histogram(&c, &g, "t", axis, bins, &Colour::One, &[]).is_none());
    }
}

#[test]
fn colours_split_what_is_kept() {
    // Four points, roles 0, 3, 3 and -1, t from 0 to 3.
    let c = cloud(
        &[[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [3.0, 0.0, 0.0]],
        vec![column("t", false, &[0.0, 1.0, 2.0, 3.0]), column("role", true, &[0.0, 3.0, 3.0, -1.0])],
    );
    let g = Geometry::of(&c);
    let colour = Colour::Palette { property: "role" };
    let h = histogram(&c, &g, "t", [0.0, 4.0], 4, &colour, &[]).unwrap();
    assert_eq!(h.slots.len(), colour.slots());
    assert!(close(&h.slots[0], &[0.25, 0.0, 0.0, 0.0]));
    assert!(close(&h.slots[3], &[0.0, 0.25, 0.25, 0.0]));
    assert!(close(&h.slots[PALETTE.len()], &[0.0, 0.0, 0.0, 0.25]), "a negative label");
    // A filter by values keeps role 3 only.
    let three = Filter { property: "role".into(), keep: Keep::Values { values: vec![3] } };
    let kept = histogram(&c, &g, "t", [0.0, 4.0], 4, &colour, &[three]).unwrap();
    let total: f64 = kept.slots.iter().flatten().sum();
    assert!((total - 0.5).abs() < 1e-9 && close(&kept.slots[3], &h.slots[3]));
    // A ramp over t in two bands, and coloured by a property the cloud doesn't have: plain.
    let ramp = Colour::Ramp { property: "t", range: [0.0, 4.0], bands: 2 };
    let r = histogram(&c, &g, "t", [0.0, 4.0], 4, &ramp, &[]).unwrap();
    assert!(close(&r.slots[0], &[0.25, 0.25, 0.0, 0.0]) && close(&r.slots[1], &[0.0, 0.0, 0.25, 0.25]));
    let none = histogram(&c, &g, "t", [0.0, 4.0], 4, &Colour::Palette { property: "nope" }, &[]).unwrap();
    assert!(close(&none.slots[PALETTE.len() + 1], &[0.25; 4]));
}

#[test]
fn labels_change_where_the_next_point_has_another() {
    // Six points in order of t, layers 0 0 1 1 2 2.
    let points: Vec<[f32; 3]> = (0..6).map(|i| [i as f32, 0.0, 0.0]).collect();
    let c = cloud(&points, vec![column("t", false, &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]), column("layer", true, &[0.0, 0.0, 1.0, 1.0, 2.0, 2.0])]);
    let g = Geometry::of(&c);
    assert_eq!(label_changes(&c, &g, "layer", "t"), vec![2.0, 4.0]);
    assert!(label_changes(&c, &g, "nope", "t").is_empty());
}

#[test]
fn the_automatic_range_is_what_every_filter_keeps() {
    // Filtered by x from 2 to 4, coloured by t: the range is what is kept of t, though no filter
    // is on t (its span within filters on t would be all of it).
    let mut c = cloud(&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [4.0, 0.0, 0.0], [20.0, 0.0, 0.0]], vec![column("t", false, &[0.0, 3.0, 5.0, 7.0])]);
    c.color_by = ColorBy::Property;
    c.color_property = Some("t".into());
    let g = Geometry::of(&c);
    let x = [range("x", [2.0, 4.0])];
    assert_eq!(c.color_range(), Some([0.0, 7.0]));
    assert_eq!(color_range(&c, &g, &x), Some([3.0, 5.0]));
    // The point at x = 20 alone: one value, one unit wide.
    assert_eq!(color_range(&c, &g, &[range("x", [19.0, 21.0])]), Some([6.5, 7.5]));
    // Nothing kept: the cloud's own bound; a range set is kept as it is.
    assert_eq!(color_range(&c, &g, &[range("x", [50.0, 60.0])]), Some([0.0, 7.0]));
    c.height_range = Some([1.0, 2.0]);
    assert_eq!(color_range(&c, &g, &x), Some([1.0, 2.0]));
    let h = histogram(&c, &g, "t", [0.0, 1.0], 4, &Colour::One, &x).unwrap();
    assert_eq!(h.extent, Some([3.0, 5.0]), "the extent reaches past the axis");
}
