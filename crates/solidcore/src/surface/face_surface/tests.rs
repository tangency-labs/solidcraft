use super::*;
use crate::eval::{Curve, Frame};
use crate::geom::vector2d::Point2;
use core::f64::consts::PI;

fn k() -> Kernel {
    Kernel {
        fixes: crate::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    }
}

const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn revolution(profile: Curve) -> Record {
    Record::PlacedRevolution {
        profile: Box::new(profile),
        origin: [0.0; 3],
        axis: [0.0, 0.0, 1.0],
        matrix: IDENTITY,
        forward: true,
    }
}

fn frame() -> Frame {
    Frame {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    }
}

/// The points of `s` at its box's corners and middle are at `r` from the z axis.
fn on_cylinder(k: &Kernel, f: &FaceSurface, r: f64) {
    let (lo, hi) = (f.uv_domain.min(), f.uv_domain.max());
    for (u, v) in [
        (lo.x, lo.y),
        (hi.x, hi.y),
        (0.5 * (lo.x + hi.x), 0.5 * (lo.y + hi.y)),
    ] {
        let p = f.surface.evaluate_point(k, Point2::new(u, v)).unwrap();
        assert!((p.x.hypot(p.y) - r).abs() <= 1e-14 * 16.0, "{p:?}");
    }
}

/// A revolution of a line parallel to its axis is that cylinder exactly: proved, holding the
/// revolution's exact form (its parameters unchanged).
#[test]
fn a_revolved_line_along_the_axis_is_its_cylinder() {
    let k = k();
    let line = Curve::Line {
        start: [5.0, 0.0, 0.0],
        end: [5.0, 0.0, 4.0],
    };
    let f = face_surface(&k, &revolution(line), None, [[0.0, 1.0], [0.0, PI]]).unwrap();
    assert!(
        matches!(f.surface.kind(), SurfaceKind::Cone(c) if c.cylinder && c.base_radius == 5.0),
        "{:?}",
        f.surface.kind()
    );
    assert!(!f.trims);
    on_cylinder(&k, &f, 5.0);
}

/// A spline profile a hair off a line (solidcore#2540's sliver) is no cylinder: it stays the
/// revolution's NURBS, never a cylinder the check then refuses.
#[test]
fn a_sliver_off_a_cylinder_stays_its_nurbs() {
    let k = k();
    let bent = Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![
            [5.0, 0.0, 0.0],
            [5.0 + 7e-7, 0.0, 4.0 / 3.0],
            [5.0 + 4e-7, 0.0, 8.0 / 3.0],
            [5.0, 0.0, 4.0],
        ],
    };
    let f = face_surface(&k, &revolution(bent), None, [[0.0, 1.0], [0.0, PI]]).unwrap();
    assert!(
        matches!(f.surface.kind(), SurfaceKind::BSpline(_)),
        "{:?}",
        f.surface.kind()
    );
}

/// A cylinder face on its NURBS form is the cylinder holding that form; the form labelled a
/// sphere is refused.
#[test]
fn an_analytic_face_on_its_form_is_proved_on_it() {
    let k = k();
    let label = Record::Cylinder {
        frame: frame(),
        radius: 3.0,
    };
    let (net, d) = form::surface(&label, [[0.0, PI], [0.0, 2.0]]).unwrap();
    let f = face_surface(&k, &label, Some(&net), d).unwrap();
    assert!(
        matches!(f.surface.kind(), SurfaceKind::Cone(c) if c.cylinder),
        "{:?}",
        f.surface.kind()
    );
    assert!(f.trims);
    on_cylinder(&k, &f, 3.0);
    let wrong = Record::Sphere {
        frame: frame(),
        radius: 3.0,
    };
    assert!(face_surface(&k, &wrong, Some(&net), d).is_err());
}

/// A rational spline that is an extruded circle is its cylinder.
#[test]
fn an_extruded_circle_is_its_cylinder() {
    let k = k();
    let (net, d) = form::surface(
        &Record::Cylinder {
            frame: frame(),
            radius: 2.0,
        },
        [[0.0, PI / 2.0], [0.0, 1.0]],
    )
    .unwrap();
    let f = face_surface(&k, &net, None, d).unwrap();
    assert!(
        matches!(f.surface.kind(), SurfaceKind::Cone(c) if c.cylinder),
        "{:?}",
        f.surface.kind()
    );
    assert!(f.trims);
    on_cylinder(&k, &f, 2.0);
}

/// A ball patch is its net, certified on its sphere: the net of a sphere's patch is one, and the
/// same net about a centre moved by 1e-6 is refused.
#[test]
fn a_ball_patch_is_certified_on_its_sphere() {
    let k = k();
    let (net, d) = form::surface(
        &Record::Sphere {
            frame: frame(),
            radius: 4.0,
        },
        [[0.0, PI / 2.0], [0.0, PI / 4.0]],
    )
    .unwrap();
    let f = face_surface(
        &k,
        &Record::BallPatch {
            center: [0.0; 3],
            radius: 4.0,
        },
        Some(&net),
        d,
    )
    .unwrap();
    assert!(matches!(f.surface.kind(), SurfaceKind::BallPatch(_)));
    assert!(face_surface(
        &k,
        &Record::BallPatch {
            center: [1e-6, 0.0, 0.0],
            radius: 4.0
        },
        Some(&net),
        d
    )
    .is_err());
    assert!(face_surface(
        &k,
        &Record::BallPatch {
            center: [0.0; 3],
            radius: 4.0
        },
        None,
        d
    )
    .is_err());
}
