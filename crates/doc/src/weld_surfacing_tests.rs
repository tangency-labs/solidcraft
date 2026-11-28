//! Surfacing: a layer on a flat face, its edges square, run out and rounded against their closed
//! forms, two layers, a round region, the outside of a shaft and the inside of a bore, a bore the
//! layer closes (refused with the thickness that built), two faces across an edge, bead rows; the
//! part unchanged.

use std::f64::consts::PI;

use solidcore_record::{Loop2, Vec2};
use solidcraft_kernel::{SmlibCheck, box_solid, cylinder, validity};

use super::*;

fn volume(b: &Body) -> f64 {
    b.volume_at(1e-9).unwrap()
}

fn check(b: &Body, what: &str) {
    let v = validity(b);
    assert!(v.record.is_ok() && matches!(v.smlib, Some(SmlibCheck::Valid)), "{what}: {v:?}");
}

fn close(a: f64, b: f64, rel: f64, what: &str) {
    assert!((a - b).abs() <= rel * b.abs(), "{what}: {a} against {b}");
}

fn style(thickness: f64, layers: usize, edges: LayerEdges) -> SurfacingStyle {
    SurfacingStyle { thickness, layers, edges, rows: None }
}

fn block() -> Body {
    box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 25.0, 10.0)).unwrap()
}

const TOP: [f64; 3] = [20.0, 12.5, 10.0];

/// A slab `l` x `w` x `r` whose top edges are rounded (`round`) or chamfered at `r`, the whole
/// height: the middle, the four edge strips (a quarter circle or a half square across), and the
/// four corners where two meet (`2 r³ / 3` rounded, `r³ / 3` chamfered).
fn slab(l: f64, w: f64, r: f64, round: bool) -> f64 {
    let (strip, corner) = if round { (PI * r * r / 4.0, 2.0 * r * r * r / 3.0) } else { (r * r / 2.0, r * r * r / 3.0) };
    (l - 2.0 * r) * (w - 2.0 * r) * r + 2.0 * (l - 2.0 * r + w - 2.0 * r) * strip + 4.0 * corner
}

#[test]
fn a_layer_on_a_flat_face_with_each_kind_of_edge() {
    let part = block();
    let before = part.record().clone();
    let square = surfacing(&part, &[TOP], None, &style(3.0, 1, LayerEdges::Square)).unwrap();
    check(&square.layers[0], "square");
    close(volume(&square.layers[0]), 40.0 * 25.0 * 3.0, 1e-9, "square");
    let bb = square.layers[0].exact_bounds().unwrap();
    assert!((bb.min.z - 10.0).abs() < 1e-9 && (bb.max.z - 13.0).abs() < 1e-9, "away from the material: {bb:?}");
    for (edges, round) in [(LayerEdges::Rounded, true), (LayerEdges::RunOut, false)] {
        let s = surfacing(&part, &[TOP], None, &style(3.0, 1, edges)).unwrap();
        check(&s.layers[0], "edged");
        assert!(s.notices.is_empty(), "{:?}", s.notices);
        close(volume(&s.layers[0]), slab(40.0, 25.0, 3.0, round), 1e-9, "edged");
    }
    assert_eq!(part.record(), &before, "part changed");
}

#[test]
fn two_layers_are_two_bodies() {
    let part = block();
    let s = surfacing(&part, &[TOP], None, &style(2.0, 2, LayerEdges::Square)).unwrap();
    assert_eq!(s.layers.len(), 2);
    for l in &s.layers {
        check(l, "layer");
        close(volume(l), 40.0 * 25.0 * 2.0, 1e-9, "layer");
    }
    let bb = s.layers[1].exact_bounds().unwrap();
    assert!((bb.min.z - 12.0).abs() < 1e-9 && (bb.max.z - 14.0).abs() < 1e-9, "{bb:?}");
    // rounded: only the outermost layer's edges, radius one layer
    let s = surfacing(&part, &[TOP], None, &style(2.0, 2, LayerEdges::Rounded)).unwrap();
    close(volume(&s.layers[0]), 40.0 * 25.0 * 2.0, 1e-9, "inner layer");
    // the whole 4 thick, its top edges rounded at r 2: strips lose (1 - pi / 4) r²; below the full
    // height the kernel blends each corner with a sphere, keeping an eighth of a ball, pi r³ / 6, of
    // the corner's r³
    let (l, w, r) = (40.0, 25.0, 2.0);
    let whole = l * w * 4.0 - 2.0 * (l - 2.0 * r + w - 2.0 * r) * (1.0 - PI / 4.0) * r * r - 4.0 * (r * r * r - PI * r * r * r / 6.0);
    close(volume(&s.layers[1]), whole - l * w * 2.0, 1e-9, "outer layer");
}

#[test]
fn a_round_region_of_the_face() {
    let part = block();
    let plane = Plane { origin: Vec3::new(0.0, 0.0, 10.0), ..Plane::XY };
    let regions = [Region2 { outer: Loop2::circle(Vec2::new(20.0, 12.5), 8.0), holes: vec![] }];
    let region = SurfacingRegion { plane: &plane, regions: &regions };
    let s = surfacing(&part, &[TOP], Some(region), &style(3.0, 1, LayerEdges::Square)).unwrap();
    check(&s.layers[0], "region");
    close(volume(&s.layers[0]), PI * 64.0 * 3.0, 1e-9, "region");
    // rounded: a cylinder of radius 5 and a quarter torus round it (Pappus)
    let s = surfacing(&part, &[TOP], Some(region), &style(3.0, 1, LayerEdges::Rounded)).unwrap();
    let want = PI * 25.0 * 3.0 + 2.0 * PI * (5.0 + 4.0 * 3.0 / (3.0 * PI)) * (PI * 9.0 / 4.0);
    close(volume(&s.layers[0]), want, 1e-7, "rounded region");
    let far = Plane { origin: Vec3::new(0.0, 0.0, 10.0), ..Plane::XY };
    let off = [Region2 { outer: Loop2::circle(Vec2::new(200.0, 12.5), 8.0), holes: vec![] }];
    assert!(matches!(
        surfacing(&part, &[TOP], Some(SurfacingRegion { plane: &far, regions: &off }), &style(3.0, 1, LayerEdges::Square)),
        Err(SurfacingRefusal::RegionOff)
    ));
}

#[test]
fn a_shaft_a_bore_and_a_bore_the_layer_closes() {
    let shaft = cylinder(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 4.0, 20.0).unwrap();
    let s = surfacing(&shaft, &[[4.0, 0.0, 10.0]], None, &style(2.0, 1, LayerEdges::Square)).unwrap();
    check(&s.layers[0], "shaft");
    close(volume(&s.layers[0]), PI * (36.0 - 16.0) * 20.0, 1e-9, "shaft");
    let bored = |r: f64| {
        let block = box_solid(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 20.0)).unwrap();
        let hole = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, 1.0), r, 22.0).unwrap();
        solidcraft_kernel::boolean(&block, &hole, BoolOp::Cut).unwrap().unwrap()
    };
    let s = surfacing(&bored(5.0), &[[5.0, 0.0, 10.0]], None, &style(2.0, 1, LayerEdges::Square)).unwrap();
    close(volume(&s.layers[0]), PI * (25.0 - 9.0) * 20.0, 1e-9, "bore");
    let got = surfacing(&bored(2.5), &[[2.5, 0.0, 10.0]], None, &style(3.0, 1, LayerEdges::Square));
    assert!(matches!(got, Err(SurfacingRefusal::TooThick { fits }) if fits > 2.4 && fits < 2.5), "{got:?}");
}

#[test]
fn two_faces_across_an_edge_take_one_layer() {
    let part = block();
    let s = surfacing(&part, &[TOP, [40.0, 12.5, 5.0]], None, &style(3.0, 1, LayerEdges::Square)).unwrap();
    check(&s.layers[0], "two faces");
    // the top, the side, and the square corner between them
    close(volume(&s.layers[0]), 40.0 * 25.0 * 3.0 + 25.0 * 10.0 * 3.0 + 3.0 * 3.0 * 25.0, 1e-9, "two faces");
}

#[test]
fn bead_rows_lie_within_the_layer() {
    let part = block();
    let rows = Rows { width: FIRST_ROW_WIDTH, overlap: FIRST_ROW_OVERLAP, along: None };
    let s = surfacing(&part, &[TOP], None, &SurfacingStyle { thickness: 3.0, layers: 1, edges: LayerEdges::Square, rows: Some(rows) }).unwrap();
    check(&s.layers[0], "rows");
    let v = volume(&s.layers[0]);
    assert!(v > 0.5 * 3000.0 && v < 3000.0, "{v}");
    let bb = s.layers[0].exact_bounds().unwrap();
    assert!(bb.min.z >= 10.0 - 1e-9 && bb.max.z <= 13.0 + 1e-9 && bb.min.x >= -1e-9 && bb.max.x <= 40.0 + 1e-9, "{bb:?}");
    let shaft = cylinder(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 4.0, 20.0).unwrap();
    let got =
        surfacing(&shaft, &[[4.0, 0.0, 10.0]], None, &SurfacingStyle { thickness: 1.0, layers: 1, edges: LayerEdges::Square, rows: Some(rows) });
    assert!(matches!(got, Err(SurfacingRefusal::RowsNeedFlat)), "{got:?}");
}

#[test]
fn bad_input_is_refused() {
    let part = block();
    assert!(matches!(surfacing(&part, &[], None, &style(3.0, 1, LayerEdges::Square)), Err(SurfacingRefusal::Invalid(_))));
    assert!(matches!(surfacing(&part, &[TOP], None, &style(0.0, 1, LayerEdges::Square)), Err(SurfacingRefusal::Invalid(_))));
    assert!(matches!(surfacing(&part, &[TOP], None, &style(3.0, 0, LayerEdges::Square)), Err(SurfacingRefusal::Invalid(_))));
}
