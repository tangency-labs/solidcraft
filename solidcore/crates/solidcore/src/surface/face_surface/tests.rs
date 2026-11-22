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

fn is_nurbs(f: &FaceSurface) -> bool {
    matches!(f.surface.kind(), SurfaceKind::BSpline(_))
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
    let f = face_surface(&k, &revolution(line), None, [[0.0, 1.0], [0.0, PI]], &[]).unwrap();
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
    let f = face_surface(&k, &revolution(bent), None, [[0.0, 1.0], [0.0, PI]], &[]).unwrap();
    assert!(
        matches!(f.surface.kind(), SurfaceKind::BSpline(_)),
        "{:?}",
        f.surface.kind()
    );
}

/// A cylinder face on its NURBS form is the cylinder holding that form; the form labelled a
/// sphere keeps no label (owner 22254): the face is its exact NURBS, not refused.
#[test]
fn an_analytic_face_on_its_form_is_proved_on_it() {
    let k = k();
    let label = Record::Cylinder {
        frame: frame(),
        radius: 3.0,
    };
    let (net, d) = form::surface(&label, [[0.0, PI], [0.0, 2.0]]).unwrap();
    let f = face_surface(&k, &label, Some(&net), d, &[]).unwrap();
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
    assert!(is_nurbs(
        &face_surface(&k, &wrong, Some(&net), d, &[]).unwrap()
    ));
}

/// F1: a label's own numbers are proved on its form. A cylinder label of radius 5 over the
/// form of a radius-7 cylinder is not read as a 7 mm cylinder under the label: the label is
/// dropped and the face is its exact NURBS.
#[test]
fn a_label_whose_numbers_are_off_its_form_is_dropped_not_rewritten() {
    let k = k();
    let seven = Record::Cylinder {
        frame: frame(),
        radius: 7.0,
    };
    let (net, d) = form::surface(&seven, [[0.0, PI], [0.0, 2.0]]).unwrap();
    let five = Record::Cylinder {
        frame: frame(),
        radius: 5.0,
    };
    let f = face_surface(&k, &five, Some(&net), d, &[]).unwrap();
    assert!(is_nurbs(&f), "{:?}", f.surface.kind());
    let r = record_face(&k, &f.surface, &f.uv_domain).unwrap();
    assert_eq!((r.surface, r.nurbs), (net, None));
}

/// A label whose frame's origin lies elsewhere on the axis than where the form's height starts
/// (a hole's frame on the solid's floor at z = -10, its form from z = 0) is that cylinder: its
/// own numbers, the form's parametrisation, and the same record back.
#[test]
fn a_label_with_its_origin_elsewhere_on_the_axis_keeps_its_numbers() {
    let k = k();
    let label = Record::Cylinder {
        frame: frame(),
        radius: 3.0,
    };
    let (net, d) = form::surface(&label, [[0.0, PI], [10.0, 12.0]]).unwrap();
    let lower = Record::Cylinder {
        frame: Frame {
            origin: [0.0, 0.0, -10.0],
            ..frame()
        },
        radius: 3.0,
    };
    let f = face_surface(&k, &lower, Some(&net), d, &[]).unwrap();
    assert!(
        matches!(f.surface.kind(), SurfaceKind::Cone(c) if c.cylinder),
        "{:?}",
        f.surface.kind()
    );
    let r = record_face(&k, &f.surface, &f.uv_domain).unwrap();
    assert_eq!((&r.surface, r.nurbs.as_ref()), (&lower, Some(&net)));
}

/// A small cylinder far from the origin (a 1.8 mm hole at (150, 150)) is proved on its form:
/// the form's points are held to the rounding at 150, which the proof's bound includes.
#[test]
fn a_small_cylinder_far_from_the_origin_is_proved_on_its_form() {
    let k = k();
    let label = Record::Cylinder {
        frame: Frame {
            origin: [150.0, 150.0, -2.0],
            x_axis: [0.8178681931580377, 0.5754056122600879, 0.0],
            y_axis: [-0.5754056122600879, 0.8178681931580377, 0.0],
        },
        radius: 1.8,
    };
    let (net, d) = form::surface(&label, [[0.0, 2.0 * PI], [2.0, 4.0]]).unwrap();
    let f = face_surface(&k, &label, Some(&net), d, &[]).unwrap();
    assert!(
        matches!(f.surface.kind(), SurfaceKind::Cone(c) if c.cylinder),
        "{:?}",
        f.surface.kind()
    );
    let moved = Record::Cylinder {
        frame: Frame {
            origin: [150.0, 150.0 + 1e-9, -2.0],
            x_axis: [0.8178681931580377, 0.5754056122600879, 0.0],
            y_axis: [-0.5754056122600879, 0.8178681931580377, 0.0],
        },
        radius: 1.8,
    };
    assert!(is_nurbs(
        &face_surface(&k, &moved, Some(&net), d, &[]).unwrap()
    ));
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
    let f = face_surface(&k, &net, None, d, &[]).unwrap();
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
        &[],
    )
    .unwrap();
    assert!(matches!(f.surface.kind(), SurfaceKind::BallPatch(_)));
    // a net off its sphere keeps no label: the face is its exact NURBS (owner 22254)
    assert!(is_nurbs(
        &face_surface(
            &k,
            &Record::BallPatch {
                center: [1e-6, 0.0, 0.0],
                radius: 4.0
            },
            Some(&net),
            d,
            &[]
        )
        .unwrap()
    ));
    assert!(face_surface(
        &k,
        &Record::BallPatch {
            center: [0.0; 3],
            radius: 4.0
        },
        None,
        d,
        &[]
    )
    .is_err());
}

/// A cylinder face goes to the record as its label (its own numbers, proved on its form) on
/// that form, over its box, and comes back to the same record; the form labelled a sphere comes
/// back as its NURBS alone.
#[test]
fn an_analytic_face_goes_back_as_its_label_on_its_form() {
    let k = k();
    let label = Record::Cylinder {
        frame: frame(),
        radius: 3.0,
    };
    let (net, d) = form::surface(&label, [[0.0, PI], [0.0, 2.0]]).unwrap();
    let f = face_surface(&k, &label, Some(&net), d, &[]).unwrap();
    let r = record_face(&k, &f.surface, &f.uv_domain).unwrap();
    // the kernel's reading of the form: the radius to its rounding
    assert!(
        matches!(r.surface, Record::Cylinder { radius, .. } if (radius - 3.0).abs() <= 1e-14),
        "{:?}",
        r.surface
    );
    assert_eq!(r.nurbs.as_ref(), Some(&net));
    assert_eq!((r.scale, r.agrees), ([1.0, 1.0], true));
    let back = face_surface(&k, &r.surface, r.nurbs.as_ref(), r.domain, &[]).unwrap();
    assert_eq!(record_face(&k, &back.surface, &back.uv_domain).unwrap(), r);
    let sphere = Record::Sphere {
        frame: frame(),
        radius: 3.0,
    };
    assert!(is_nurbs(
        &face_surface(&k, &sphere, r.nurbs.as_ref(), r.domain, &[]).unwrap()
    ));
}

/// A plane goes to the record on its placement, its box its kernel box times its scales; a
/// plane whose scales have opposite signs is turned over against the record's normal.
#[test]
fn a_plane_goes_back_on_its_axes_with_its_scales() {
    use crate::geom::vector2d::Vec2;
    use crate::geom::vector3d::Vec3;
    use crate::surface::plane::Plane;
    let k = k();
    let label = Record::Plane {
        origin: [1.0, 2.0, 3.0],
        u_axis: [2.0, 0.0, 0.0],
        v_axis: [0.0, 1.0, 0.0],
    };
    let f = face_surface(&k, &label, None, [[0.0, 1.0], [0.0, 3.0]], &[]).unwrap();
    assert_eq!(f.scale, [2.0, 1.0]);
    let r = record_face(&k, &f.surface, &f.uv_domain).unwrap();
    assert_eq!(
        r.surface,
        Record::Plane {
            origin: [1.0, 2.0, 3.0],
            u_axis: [1.0, 0.0, 0.0],
            v_axis: [0.0, 1.0, 0.0],
        }
    );
    assert_eq!((r.domain, r.agrees), ([[0.0, 2.0], [0.0, 3.0]], true));
    let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
    let flipped = Plane::new(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec2::new(-2.0, 1.0),
        &dom,
    );
    let r = record_face(&k, &Surface::from(flipped), &dom).unwrap();
    assert_eq!(
        (r.scale, r.domain, r.agrees),
        ([-2.0, 1.0], [[-2.0, 0.0], [0.0, 1.0]], false)
    );
}

/// Owner ruling (item 8, option B): a cone of half angle under 1e-12 and a torus whose tube
/// reaches its axis keep their analytic label on their form, both ways, with their own numbers.
#[test]
fn degenerate_cone_and_torus_faces_go_round_with_their_label() {
    let k = k();
    for (label, domain) in [
        (
            Record::Cone {
                frame: frame(),
                radius_at_origin: 3.0,
                half_angle: 1e-13,
            },
            [[0.0, PI], [0.0, 2.0]],
        ),
        (
            Record::Torus {
                frame: frame(),
                major_radius: 2.0,
                minor_radius: 3.0,
                tube_start: 0.0,
            },
            [[0.0, PI / 2.0], [-PI / 4.0, PI / 4.0]],
        ),
    ] {
        let (net, d) = form::surface(&label, domain).unwrap();
        let f = face_surface(&k, &label, Some(&net), d, &[]).unwrap();
        assert!(!is_nurbs(&f), "{label:?}: label dropped");
        let r = record_face(&k, &f.surface, &f.uv_domain).unwrap();
        assert_eq!(r.nurbs.as_ref(), Some(&net), "{label:?}");
        match (&r.surface, &label) {
            (
                Record::Cone {
                    radius_at_origin,
                    half_angle,
                    ..
                },
                Record::Cone { .. },
            ) => {
                assert_eq!(*radius_at_origin, 3.0);
                assert!((half_angle - 1e-13).abs() <= 1e-28, "{half_angle}");
            }
            (r, l) => assert_eq!(r, l),
        }
        let back = face_surface(&k, &r.surface, r.nurbs.as_ref(), r.domain, &[]).unwrap();
        assert_eq!(record_face(&k, &back.surface, &back.uv_domain).unwrap(), r);
    }
}

/// The kernel's own check of a face surface, at every level: the failed reports as
/// `(reporting type, test number, message)`.
fn check_failures(k: &Kernel, s: &Surface) -> Vec<(i64, u64, String)> {
    use crate::geom::assert_array::{AssertArray, ObjectKey};
    let mut failed = Vec::new();
    for level in [0, 1, 2, crate::surface::surface::assert_valid::LEVEL_GIVEN] {
        let mut list = AssertArray::new();
        s.assert_valid(k, Some(&mut list), level, ObjectKey(1))
            .unwrap();
        failed.extend(list.reports().iter().filter(|r| !r.ok).map(|r| {
            (
                r.reporting_type,
                r.test_index,
                r.message.as_deref().unwrap_or_default().to_string(),
            )
        }));
    }
    failed
}

/// A round read back from its record (ladder/09 005, a shelled box's r5 rounds): its
/// cylinder label's frame starts on the axis away from where its form's height does, so
/// the cylinder moves its heights by that offset, and its generator line moves with them.
/// The kernel's check passes (`SmSurfOfRevolution::Test` #2 and `SmCone::Test` #1 compare
/// the generator's interval with the surface's), for a cylinder and a cone, either way
/// along the axis.
#[test]
fn a_label_with_its_origin_elsewhere_on_the_axis_passes_the_kernels_check() {
    let k = k();
    for h in [-10.0, 7.5] {
        let at = |z: f64| Frame {
            origin: [0.0, 0.0, z],
            ..frame()
        };
        let labels = [
            (
                Record::Cylinder {
                    frame: frame(),
                    radius: 5.0,
                },
                Record::Cylinder {
                    frame: at(h),
                    radius: 5.0,
                },
            ),
            (
                Record::Cone {
                    frame: frame(),
                    radius_at_origin: 5.0,
                    half_angle: 0.25,
                },
                Record::Cone {
                    frame: at(h),
                    radius_at_origin: 5.0 + h * 0.25_f64.tan(),
                    half_angle: 0.25,
                },
            ),
        ];
        for (label, moved) in labels {
            let (net, d) = form::surface(&label, [[0.0, 0.5 * PI], [2.0, 6.0]]).unwrap();
            let f = face_surface(&k, &moved, Some(&net), d, &[]).unwrap();
            assert!(
                matches!(f.surface.kind(), SurfaceKind::Cone(_)),
                "{h} {:?}",
                f.surface.kind()
            );
            assert_eq!(check_failures(&k, &f.surface), vec![], "{h} {moved:?}");
            let r = record_face(&k, &f.surface, &f.uv_domain).unwrap();
            assert_eq!(r.nurbs.as_ref(), Some(&net));
        }
    }
}
