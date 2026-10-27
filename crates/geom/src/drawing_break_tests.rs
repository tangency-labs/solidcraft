use super::*;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

fn map(factor: f64, bands: &[Band]) -> BreakMap {
    BreakMap::new(-FRAC_PI_2, Scale::new(factor, 1.0).unwrap(), bands).unwrap()
}

fn band(start: f64, end: f64, gap_mm: f64) -> Band {
    Band { start, end, gap_mm }
}

fn near(a: Vec2, b: Vec2) {
    assert!((a - b).len() < 1e-8, "{a:?} != {b:?}");
}

#[test]
fn shortening_preserves_true_length_and_fixed_paper_gap_at_every_scale() {
    for factor in [0.25, 0.5, 1.0, 2.0] {
        let m = map(factor, &[band(60.0, 140.0, 5.0)]);
        let p = m.to_paper(Vec2::new(200.0, 0.0)).unwrap().unwrap();
        near(p, Vec2::new(120.0 * factor + 5.0, 0.0));
        near(m.to_model(p).unwrap().unwrap(), Vec2::new(200.0, 0.0));
        assert!((m.shortening_mm() - (80.0 * factor - 5.0)).abs() < 1e-9);
        assert!(m.to_paper(Vec2::new(100.0, 0.0)).unwrap().is_none());
        assert!(m.to_model(Vec2::new(60.0 * factor + 2.5, 0.0)).unwrap().is_none());
    }
}

#[test]
fn exact_line_clipping_retains_both_sides_and_reversed_paths() {
    let m = map(0.5, &[band(60.0, 140.0, 5.0)]);
    let a = Vec2::new(0.0, 10.0);
    let b = Vec2::new(200.0, 10.0);
    let pieces = m.line(a, b).unwrap();
    assert_eq!(pieces.len(), 2);
    near(pieces[0][0], Vec2::new(0.0, 5.0));
    near(pieces[0][1], Vec2::new(30.0, 5.0));
    near(pieces[1][0], Vec2::new(35.0, 5.0));
    near(pieces[1][1], Vec2::new(65.0, 5.0));
    let reversed = m.line(b, a).unwrap();
    near(reversed[0][0], pieces[1][1]);
    near(reversed[1][1], pieces[0][0]);
}

#[test]
fn multiple_unsorted_reversed_bands_and_removal() {
    let m = map(1.0, &[band(170.0, 130.0, 4.0), band(60.0, 30.0, 3.0)]);
    assert_eq!(m.bands()[0].start, 30.0);
    let p = m.to_paper(Vec2::new(200.0, 12.0)).unwrap().unwrap();
    near(p, Vec2::new(137.0, 12.0));
    near(m.to_model(p).unwrap().unwrap(), Vec2::new(200.0, 12.0));
    assert_eq!(m.line(Vec2::ZERO, Vec2::new(200.0, 0.0)).unwrap().len(), 3);
    near(map(1.0, &[]).to_paper(Vec2::new(200.0, 12.0)).unwrap().unwrap(), Vec2::new(200.0, 12.0));
}

#[test]
fn oblique_and_horizontal_lines_use_the_same_map() {
    for angle in [0.0, FRAC_PI_4, FRAC_PI_2, -FRAC_PI_4] {
        let m = BreakMap::new(angle, Scale::new(1.0, 2.0).unwrap(), &[band(20.0, 60.0, 5.0)]).unwrap();
        let normal = m.normal();
        let tangent = Vec2::new(normal.y, -normal.x);
        let original = normal * 100.0 + tangent * 30.0;
        let paper = m.to_paper(original).unwrap().unwrap();
        near(paper, normal * 35.0 + tangent * 15.0);
        near(m.to_model(paper).unwrap().unwrap(), original);
        assert_eq!(m.line(normal * 0.0, normal * 100.0).unwrap().len(), 2);
    }
}

#[test]
fn boundaries_remain_and_lines_in_removed_spans_disappear() {
    let m = BreakMap::new(0.0, Scale::new(1.0, 1.0).unwrap(), &[band(20.0, 60.0, 5.0)]).unwrap();
    near(m.to_paper(Vec2::new(10.0, 20.0)).unwrap().unwrap(), Vec2::new(10.0, 20.0));
    near(m.to_paper(Vec2::new(10.0, 60.0)).unwrap().unwrap(), Vec2::new(10.0, 25.0));
    assert!(m.line(Vec2::new(0.0, 40.0), Vec2::new(100.0, 40.0)).unwrap().is_empty());
    assert_eq!(m.line(Vec2::new(0.0, 20.0), Vec2::new(100.0, 20.0)).unwrap().len(), 1);
}

#[test]
fn oblique_boundary_points_survive_projection_roundoff() {
    for end in [60.0, 800_000_000.0] {
        for degrees in -179..180 {
            let m = BreakMap::new(f64::from(degrees).to_radians(), Scale::new(1.0, 2.0).unwrap(), &[band(20.0, end, 5.0)]).unwrap();
            let normal = m.normal();
            let tangent = Vec2::new(normal.y, -normal.x);
            assert!(m.to_model(normal * 12.5 + tangent * 3.5).unwrap().is_none());
            for coordinate in [20.0, end] {
                let model = normal * coordinate + tangent * 7.0;
                let paper = m.to_paper(model).unwrap().unwrap_or_else(|| panic!("boundary lost at {degrees} degrees, {coordinate}"));
                let inverse = m.to_model(paper).unwrap().unwrap_or_else(|| panic!("inverse boundary lost at {degrees} degrees, {coordinate}"));
                assert!((inverse - model).len() <= 32.0 * f64::EPSILON * coordinate + 1e-8);
            }
        }
    }
}

#[test]
fn hostile_bands_limits_and_coordinates_are_refused() {
    let scale = Scale::new(1.0, 1.0).unwrap();
    for b in [
        band(f64::NAN, 10.0, 1.0),
        band(0.0, f64::INFINITY, 1.0),
        band(0.0, 0.0, 1.0),
        band(0.0, 10.0, 0.0),
        band(0.0, 10.0, 10.0),
        band(0.0, 10.0, f64::NAN),
        band(0.0, 2e9, 1.0),
    ] {
        assert!(BreakMap::new(0.0, scale, &[b]).is_err(), "{b:?}");
    }
    assert!(BreakMap::new(f64::NAN, scale, &[]).is_err());
    assert!(BreakMap::new(0.0, scale, &[band(1e9 - 1e-6, 1e9, 1e-8)]).is_err());
    assert!(BreakMap::new(0.0, scale, &[band(100_000.0, 100_010.0, 1.1e-9)]).is_err());
    assert!(BreakMap::new(0.0, scale, &[band(0.0, 800_000_000.0, 1e-6)]).is_err());
    assert!(BreakMap::new(0.0, scale, &[band(0.0, 20.0, 1.0), band(10.0, 30.0, 1.0)]).is_err());
    assert!(BreakMap::new(0.0, scale, &[band(0.0, 20.0, 1.0), band(20.0, 30.0, 1.0)]).is_err());
    let bands: Vec<_> = (0..MAX_BREAKS).map(|i| band(i as f64 * 20.0, i as f64 * 20.0 + 10.0, 1.0)).collect();
    assert!(BreakMap::new(0.0, scale, &bands).is_ok());
    let mut excessive = bands;
    excessive.push(band(2000.0, 2010.0, 1.0));
    assert!(BreakMap::new(0.0, scale, &excessive).is_err());
    let m = map(1.0, &[]);
    assert!(m.shortening_mm().is_sign_positive());
    assert!(m.to_paper(Vec2::new(f64::NAN, 0.0)).is_err());
    assert!(m.to_model(Vec2::new(f64::INFINITY, 0.0)).is_err());
    assert!(m.to_paper(Vec2::new(1e6, 0.0)).is_err());
    assert!(m.line(Vec2::ZERO, Vec2::ZERO).is_err());
    assert!(serde_json::from_str::<Band>(r#"{"start":0,"end":10,"gap_mm":1,"extra":true}"#).is_err());
}
