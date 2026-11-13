//! Trim along curves on a face against closed forms: the square sheet [0, 20]^2 at z = 0 cut by
//! curves drawn above it and projected down onto it (`sweep::project_curves`), its regions'
//! areas and the points inside them, the regions kept and discarded, and the refusals.
// The tests index and unwrap as tests do.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use solidcore_ops::sweep::{planar_faces, project_curves, Curve};
use solidcore_ops::trim::{remove_faces, split_face_by_curves, Region};
use solidcore_ops::{Body, Error, Order};

const DOWN: [f64; 3] = [0.0, 0.0, -1.0];
const TOL: f64 = 1e-6;

fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
    Curve::Line { start: a, end: b }
}

/// The closed polygon through `pts`.
fn polygon(pts: &[[f64; 3]]) -> Vec<Curve> {
    (0..pts.len())
        .map(|i| line(pts[i], pts[(i + 1) % pts.len()]))
        .collect()
}

/// The square from `lo` to `hi` along x and y at height `z`, as a closed polygon.
fn square(lo: f64, hi: f64, z: f64) -> Vec<Curve> {
    polygon(&[[lo, lo, z], [hi, lo, z], [hi, hi, z], [lo, hi, z]])
}

/// The square [0, 20]^2 at z = 0 as a sheet of one planar face, and its record order.
fn sheet() -> (Body, Order) {
    let b = planar_faces(&square(0.0, 20.0, 0.0)).unwrap();
    let (_, order) = b.to_record().unwrap();
    (b, order)
}

/// `curves` (drawn at z = 5) projected down onto the sheet's face.
fn projected(b: &Body, o: &Order, curves: &[Curve]) -> Vec<Curve> {
    project_curves(b, o, 0, curves, DOWN, TOL)
        .unwrap()
        .into_iter()
        .map(|(c, _)| c)
        .collect()
}

fn rel(a: f64, want: f64) -> f64 {
    (a - want).abs() / want.abs()
}

fn area(b: &Body) -> f64 {
    b.area(1e-8).unwrap()
}

/// The two regions of a cut, the smaller first, with their areas.
fn two(cut: &Body, regions: &[Region]) -> [(Region, f64); 2] {
    assert_eq!(regions.len(), 2, "{regions:?}");
    let (_, o) = cut.to_record().unwrap();
    assert_eq!(o.faces.len(), 2, "two faces");
    let mut out = [0, 1].map(|i| {
        let r = regions[i];
        (r, cut.face_area(&o, r.face, 1e-8).unwrap())
    });
    out.sort_by(|a, b| a.1.total_cmp(&b.1));
    out
}

/// Whether `p` is on the sheet, inside its boundary.
fn on_sheet(p: [f64; 3]) -> bool {
    p[2].abs() < 1e-9 && (0.0..=20.0).contains(&p[0]) && (0.0..=20.0).contains(&p[1])
}

/// Whether `p` is strictly inside the square [5, 15]^2.
fn in_middle(p: [f64; 3]) -> bool {
    p[0] > 5.0 && p[0] < 15.0 && p[1] > 5.0 && p[1] < 15.0
}

/// The square [5, 15]^2 projected onto the sheet cuts it into the square, area 100, and the
/// sheet round it, area 300, each with a point inside it. Keeping the inside discards the
/// sheet round it, keeping the outside discards the square.
#[test]
fn a_closed_curve_cuts_a_sheet_in_two() {
    let (b, o) = sheet();
    let curves = projected(&b, &o, &square(5.0, 15.0, 5.0));
    let (cut, regions) = split_face_by_curves(&b, &o, 0, &curves, TOL).unwrap();
    let [(inner, inner_area), (outer, outer_area)] = two(&cut, &regions);
    assert!(rel(inner_area, 100.0) < 1e-6, "inside {inner_area}");
    assert!(rel(outer_area, 300.0) < 1e-6, "outside {outer_area}");
    assert!(
        on_sheet(inner.inside) && in_middle(inner.inside),
        "{inner:?}"
    );
    assert!(
        on_sheet(outer.inside) && !in_middle(outer.inside),
        "{outer:?}"
    );
    let (_, co) = cut.to_record().unwrap();
    let inside = remove_faces(&cut, &co, &[outer.face]).unwrap();
    assert_eq!(inside.counts().unwrap()[0], 1, "keep inside: one face");
    assert!(rel(area(&inside), 100.0) < 1e-6, "keep inside");
    let outside = remove_faces(&cut, &co, &[inner.face]).unwrap();
    assert_eq!(outside.counts().unwrap()[0], 1, "keep outside: one face");
    assert!(rel(area(&outside), 300.0) < 1e-6, "keep outside");
    assert_eq!(cut.counts().unwrap()[0], 2, "the cut body stays as it was");
}

/// A line across the whole sheet at y = 10 cuts it into two halves of area 200, one point
/// below the line and one above.
#[test]
fn an_open_curve_from_edge_to_edge_cuts_a_sheet_in_two() {
    let (b, o) = sheet();
    let curves = projected(&b, &o, &[line([-5.0, 10.0, 5.0], [25.0, 10.0, 5.0])]);
    let (cut, regions) = split_face_by_curves(&b, &o, 0, &curves, TOL).unwrap();
    let [(a, a_area), (c, c_area)] = two(&cut, &regions);
    assert!(rel(a_area, 200.0) < 1e-6 && rel(c_area, 200.0) < 1e-6);
    assert!(on_sheet(a.inside) && on_sheet(c.inside), "{regions:?}");
    let below = |p: [f64; 3]| p[1] < 10.0;
    assert!(below(a.inside) != below(c.inside), "{regions:?}");
}

/// A line that stops short of the sheet's boundary at both ends leaves a slit, no region:
/// refused, and so is a curve off the face.
#[test]
fn an_open_curve_inside_the_face_is_refused() {
    let (b, o) = sheet();
    let curves = projected(&b, &o, &[line([5.0, 10.0, 5.0], [15.0, 10.0, 5.0])]);
    assert_eq!(
        split_face_by_curves(&b, &o, 0, &curves, TOL).err(),
        Some(Error::CurveDoesNotCut)
    );
    let off = [line([30.0, 30.0, 0.0], [40.0, 30.0, 0.0])];
    assert_eq!(
        split_face_by_curves(&b, &o, 0, &off, TOL).err(),
        Some(Error::CurveDoesNotCut)
    );
    assert_eq!(b.counts().unwrap()[0], 1, "the sheet stays as it was");
}

#[test]
fn bad_asks_are_refused() {
    let (b, o) = sheet();
    let curves = projected(&b, &o, &square(5.0, 15.0, 5.0));
    let split = |f, c: &[Curve], t| split_face_by_curves(&b, &o, f, c, t).err();
    assert_eq!(split(1, &curves, TOL), Some(Error::Argument), "no face 1");
    assert_eq!(split(0, &[], TOL), Some(Error::Argument), "no curve");
    assert_eq!(
        split(0, &curves, f64::NAN),
        Some(Error::Argument),
        "tolerance"
    );
    assert_eq!(split(0, &curves, 0.0), Some(Error::Argument), "tolerance");
    let remove = |faces: &[usize]| remove_faces(&b, &o, faces).err();
    assert_eq!(remove(&[]), Some(Error::Argument), "no face");
    assert_eq!(remove(&[0, 0]), Some(Error::Argument), "a face twice");
    assert_eq!(remove(&[1]), Some(Error::Argument), "no face 1");
    assert_eq!(remove(&[usize::MAX]), Some(Error::Argument));
    assert_eq!(remove(&[0]), Some(Error::Empty), "nothing left");
    assert_eq!(b.counts().unwrap()[0], 1, "the sheet stays as it was");
}
