//! The surface chart on a cylinder face: its origin, point, trim region and the face's (u, v)
//! of a chart point.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)] // test

use solidcore_ops::face_surface::{Exact, FaceSurface};
use solidcore_ops::make_cylinder;
use solidcore_ops::surface_chart::{Region, SurfaceChart};
use solidcore_record::brep::{Brep, Surface};

fn cylinder_face(record: &Brep) -> usize {
    (0..record.faces.len())
        .find(|&f| {
            FaceSurface::of(record, f)
                .is_ok_and(|s| matches!(s.exact(), Exact::Label(Surface::Cylinder { .. })))
        })
        .expect("a cylinder face")
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (a[0] - b[0]).abs() <= 1e-9 && (a[1] - b[1]).abs() <= 1e-9 && (a[2] - b[2]).abs() <= 1e-9
}

#[test]
fn a_cylinder_chart_keeps_its_pick_and_reports_the_face() {
    let body = make_cylinder([0.0, 0.0, 0.0], 5.0, 10.0).expect("a cylinder");
    let (record, _) = body.to_record().expect("a record");
    let face = cylinder_face(&record);
    let on = FaceSurface::of(&record, face).expect("a face surface");
    let [[u0, u1], [v0, v1]] = on.domain();
    let uv = [(u0 + u1) * 0.5, (v0 + v1) * 0.5];
    let pick = on.point(uv).expect("a pick");
    let chart = SurfaceChart::build(&record, face, pick, 1e-6).expect("a chart");

    let at = chart.face_uv(0.0, 0.0).expect("the face's uv");
    assert!(
        (at[0] - uv[0]).abs() <= 1e-6 && (at[1] - uv[1]).abs() <= 1e-6,
        "{at:?} vs {uv:?}"
    );
    assert_eq!(chart.region(0.0, 0.0), Region::OnFace);
    assert!(near(chart.point(0.0, 0.0).expect("a point"), pick));
    let lower = chart.lower_face_uv(uv).expect("chart coordinates");
    assert!(
        lower[0].abs() <= 1e-6 && lower[1].abs() <= 1e-6,
        "{lower:?}"
    );
    // The surface's own domain bounds the chart: its edge is still in it, a millimetre past it
    // is off the surface, not merely off the face.
    let edge = chart.chart().lower([uv[0], v1]).expect("chart coordinates");
    assert!(chart.in_domain(edge[0], edge[1]), "{edge:?}");
    let past = chart.chart().lower([uv[0], v1 + 1.0]).expect("chart coordinates");
    assert!(!chart.in_domain(past[0], past[1]), "{past:?}");
    assert_eq!(chart.region(past[0], past[1]), Region::OffSurface);
    assert_eq!(chart.region(0.0, 1_000_000.0), Region::OffSurface);
}
