//! Not upstream: a skin's flat walls as planes (WR-778, `Kernel::fixes.skin_flat_walls_as_planes`).
//!
//! `SmApiCreateSkinPrimitive` makes every side wall a B-spline surface, also where the wall is
//! flat: a loft whose sections' corners line up (through three or more sections, or with lines
//! beside arcs) gets planar trapezoids as bilinear, quadratic or cubic B-splines. Such a face is
//! free-form to everything that asks for a plane (sheet metal, offsets, a sketch on it,
//! solidcraft#13331). A B-spline (or a NURBS with positive weights) whose control points lie in
//! one plane lies in that plane, so the wall is that plane exactly: each such wall takes the
//! plane ([`plane_of`], `IsPlanar` at [`EFF_ZERO`]) and its UV trims are made again from its
//! edges' 3D curves (`ReplaceSurface` with `bCreateUVTrimCurves`).

use crate::blend::fillet_cliff_overflow::plane_of;
use crate::error::{EntityRef, Result};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Vec2, Vec3};
use crate::nurbs::knots::Side;
use crate::nurbs::math::NOW;
use crate::surface::plane::Plane;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;
use crate::topology::brep::p2::{replace_surface, ArenaNotifyState, OldSurface};
use crate::topology::{Brep, FaceId};

/// How far the plane's domain reaches past the wall's control net, as a fraction of its size.
const MARGIN: f64 = 0.01;

/// Each of `faces` of `b` whose surface is a flat B-spline made that plane, its natural normal
/// the way the B-spline's points, its UV trims made from its edges. A face that is not flat, has
/// no clear side, or whose replacement fails is left as it was (the replacement is rolled back):
/// a skin that builds never fails here. Returns the faces made planes.
///
/// # Errors
///
/// Those of [`Brep::with_rollback`]'s snapshot only.
pub fn flat_walls_as_planes(k: &Kernel, b: &mut Brep, faces: &[FaceId]) -> Result<Vec<FaceId>> {
    let mut made = Vec::new();
    for &f in faces {
        let Some(plane) = flat_wall_plane(k, b, f) else {
            continue;
        };
        let replaced = b.with_rollback(
            |b| {
                let mut state = ArenaNotifyState::new(b);
                let mut env = state.env(None);
                let face_ref = (env.key_of)(EntityRef::Face(f));
                replace_surface(k, b, &mut env, f, plane, face_ref, OldSurface::Delete, true)
            },
            Result::is_ok,
        );
        if replaced.is_ok() {
            made.push(f);
        }
    }
    Ok(made)
}

/// The plane of face `f` when its surface is a B-spline lying in one: axes so that its natural
/// normal (x × y) points the way the B-spline's does at the middle of its domain, over the box
/// of its control points in the plane with a margin. `None` otherwise.
fn flat_wall_plane(k: &Kernel, b: &Brep, f: FaceId) -> Option<Surface> {
    let surface = b.face(f).ok()?.surface.as_ref()?;
    let SurfaceKind::BSpline(spline) = surface.kind() else {
        return None;
    };
    let frame = plane_of(k, surface).ok()??;
    let middle = surface
        .natural_uv_domain_with_nurb(k)
        .ok()?
        .evaluate(0.5, 0.5);
    let normal = surface
        .evaluate_normal(k, middle, Side::Left, Side::Left)
        .ok()?;
    let side = normal.dot(frame.z);
    // the B-spline's normal must lie along the plane's: anything else is no clear side
    if side.abs() < 0.5 {
        return None;
    }
    let (x, y): (Vec3, Vec3) = if side > 0.0 {
        (frame.x, frame.y)
    } else {
        (frame.x, -frame.y)
    };
    let (_, _, points) = spline.control_points().ok()?;
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in points {
        let w = if p.w == NOW { 1.0 } else { p.w };
        if !(w > 0.0) {
            return None;
        }
        let d = Vec3::new(p.x / w, p.y / w, p.z / w) - frame.origin;
        for (i, t) in [d.dot(x), d.dot(y)].into_iter().enumerate() {
            lo[i] = lo[i].min(t);
            hi[i] = hi[i].max(t);
        }
    }
    let size = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if !(size > 0.0 && size.is_finite()) {
        return None;
    }
    let m = MARGIN * size;
    let domain = Extent2d::from_bounds(lo[0] - m, lo[1] - m, hi[0] + m, hi[1] + m);
    Some(Surface::from(Plane::new(
        frame.origin,
        x,
        y,
        Vec2::new(1.0, 1.0),
        &domain,
    )))
}

#[cfg(test)]
mod tests {
    use crate::curve::curve::Curve;
    use crate::kernel::Kernel;
    use crate::mass::integrator::properties::brep_volume_default;
    use crate::math::Point3;
    use crate::primitive::api_curves::{create_arc, create_line_segment};
    use crate::surface::surface::p2::SurfaceKind;
    use crate::sweep::api_surfaces::{create_skin_primitive, SkinPrimitive};
    use crate::topology::brep::validation::{check_brep, CheckLevel};

    fn line(a: [f64; 3], b: [f64; 3]) -> Curve {
        Curve::from(
            create_line_segment(Point3::new(a[0], a[1], a[2]), Point3::new(b[0], b[1], b[2]))
                .expect("line"),
        )
    }

    /// The square of half side `h` about the Z axis at height `z`, counter-clockwise.
    fn square(h: f64, z: f64) -> Vec<Curve> {
        let c = [[-h, -h], [h, -h], [h, h], [-h, h]];
        (0..4)
            .map(|i| {
                let (a, b) = (c[i], c[(i + 1) % 4]);
                line([a[0], a[1], z], [b[0], b[1], z])
            })
            .collect()
    }

    /// A D of size `s` at height `z`: the left half of the square of half side `s` and the half
    /// circle of radius `s` on its right, counter-clockwise.
    fn d_shape(s: f64, z: f64) -> Vec<Curve> {
        vec![
            line([-s, -s, z], [0.0, -s, z]),
            Curve::from(create_arc(Point3::new(0.0, 0.0, z), s, -90.0, 90.0).expect("arc")),
            line([0.0, s, z], [-s, s, z]),
            line([-s, s, z], [-s, -s, z]),
        ]
    }

    /// The capped skin through `sections` with the fix `on` or off, its side faces' count of
    /// planes and B-splines, its volume, after the B-rep check passes.
    fn skin(sections: &[Vec<Curve>], degree: usize, on: bool) -> (usize, usize, f64) {
        let mut k = Kernel::default();
        k.fixes.skin_flat_walls_as_planes = on;
        let counts: Vec<usize> = sections.iter().map(Vec::len).collect();
        let refs: Vec<Option<&Curve>> = sections.iter().flatten().map(Some).collect();
        let SkinPrimitive {
            mut brep,
            side_faces,
            ..
        } = create_skin_primitive(&mut k, &counts, &refs, degree, true).expect("skin");
        let (mut planes, mut splines) = (0, 0);
        for f in side_faces {
            match brep
                .face(f)
                .expect("face")
                .surface
                .as_ref()
                .expect("surface")
                .kind()
            {
                SurfaceKind::Plane(_) => planes += 1,
                SurfaceKind::BSpline(_) => splines += 1,
                other => panic!("a skin wall on {other:?}"),
            }
        }
        let check = check_brep(&mut k, &mut brep, CheckLevel::Level2).expect("check");
        let failures: Vec<_> = check.failures().collect();
        assert!(
            failures.is_empty(),
            "the skin fails the check: {failures:?}"
        );
        let volume = brep_volume_default(&k, &mut brep, 1e-9).expect("volume");
        (planes, splines, volume)
    }

    /// A frustum's volume: height `h` between similar ends of areas `a` and `b`.
    fn frustum(h: f64, a: f64, b: f64) -> f64 {
        h / 3.0 * (a + b + (a * b).sqrt())
    }

    fn near(v: f64, want: f64) -> bool {
        ((v - want) / want).abs() < 1e-6
    }

    /// Three squares whose corners line up (half sides 55, 77.5, 100 at z 0, 60, 120): the
    /// quadratic skin's four walls are flat, and with the fix they are planes; the solid is the
    /// same frustum either way.
    #[test]
    fn a_skin_through_three_squares_in_line_has_planar_walls() {
        let sections = [square(55.0, 0.0), square(77.5, 60.0), square(100.0, 120.0)];
        let want = frustum(120.0, 4.0 * 55.0 * 55.0, 4.0 * 100.0 * 100.0);
        let (planes, splines, volume) = skin(&sections, 2, false);
        assert_eq!((planes, splines), (0, 4), "upstream: B-spline walls");
        assert!(near(volume, want), "{volume} vs {want}");
        let (planes, splines, volume) = skin(&sections, 2, true);
        assert_eq!((planes, splines), (4, 0), "with the fix: planes");
        assert!(near(volume, want), "{volume} vs {want}");
    }

    /// Three squares out of line (55, 80, 100): the walls bend, so they stay B-splines.
    #[test]
    fn a_skin_through_three_squares_out_of_line_keeps_curved_walls() {
        let sections = [square(55.0, 0.0), square(80.0, 60.0), square(100.0, 120.0)];
        let (planes, splines, _) = skin(&sections, 2, true);
        assert_eq!((planes, splines), (0, 4));
    }

    /// Two D shapes, the top 0.6 of the bottom: the walls on the lines are flat and become
    /// planes, the wall on the arc stays a B-spline; the solid is the frustum.
    #[test]
    fn a_skin_of_lines_and_an_arc_has_planar_walls_on_the_lines() {
        let sections = [d_shape(50.0, 0.0), d_shape(30.0, 50.0)];
        let area = |s: f64| 2.0 * s * s + std::f64::consts::PI * s * s / 2.0;
        let want = frustum(50.0, area(50.0), area(30.0));
        let (planes, splines, volume) = skin(&sections, 1, true);
        assert_eq!((planes, splines), (3, 1));
        assert!(near(volume, want), "{volume} vs {want}");
    }
}
