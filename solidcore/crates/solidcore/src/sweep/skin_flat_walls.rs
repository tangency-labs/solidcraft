//! Not upstream: a skin's flat walls as planes (WR-782), always (upstream every wall is a B-spline).
//!
//! `SmApiCreateSkinPrimitive` makes every side wall a B-spline surface, also where the wall is
//! flat: a loft whose sections' corners line up (through three or more sections, or with lines
//! beside arcs) gets planar trapezoids as bilinear, quadratic or cubic B-splines. Such a face is
//! free-form to everything that asks for a plane (sheet metal, offsets, a sketch on it,
//! solidcraft#13331). A B-spline (or a NURBS with positive weights) whose control points lie in
//! one plane lies in that plane, so the wall is that plane exactly: each such wall takes the
//! plane ([`plane_of`], `IsPlanar` at [`EFF_ZERO`]) and its UV trims are made again from its
//! edges' 3D curves (`ReplaceSurface` with `bCreateUVTrimCurves`).

use crate::blend::fillet_cliff_overflow::{plane_of, PlaneFrame};
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

/// The plane's axes in `frame` with its natural normal (x × y) the way `normal` points (the
/// B-spline's, which `plane_of`'s may oppose): `frame`'s, or with y reversed. `None` when
/// `normal` does not lie along the plane's: no clear side.
fn axes(frame: &PlaneFrame, normal: Vec3) -> Option<(Vec3, Vec3)> {
    let side = normal.dot(frame.z);
    if side.abs() < 0.5 {
        None
    } else if side > 0.0 {
        Some((frame.x, frame.y))
    } else {
        Some((frame.x, -frame.y))
    }
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
    let (x, y) = axes(&frame, normal)?;
    let (_, _, points) = spline.control_points().ok()?;
    let (mut u0, mut v0) = (f64::INFINITY, f64::INFINITY);
    let (mut u1, mut v1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        let w = if p.w == NOW { 1.0 } else { p.w };
        if w.is_nan() || w <= 0.0 {
            return None;
        }
        let d = Vec3::new(p.x / w, p.y / w, p.z / w) - frame.origin;
        let (u, v) = (d.dot(x), d.dot(y));
        (u0, u1, v0, v1) = (u0.min(u), u1.max(u), v0.min(v), v1.max(v));
    }
    let size = (u1 - u0).max(v1 - v0);
    if !size.is_finite() || size <= 0.0 {
        return None;
    }
    let m = MARGIN * size;
    let domain = Extent2d::from_bounds(u0 - m, v0 - m, u1 + m, v1 + m);
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
    use super::axes;
    use crate::blend::fillet_cliff_overflow::PlaneFrame;
    use crate::curve::curve::Curve;
    use crate::kernel::Kernel;
    use crate::mass::integrator::properties::brep_volume_default;
    use crate::math::Point3;
    use crate::math::Vec3;
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
        square_by(h, z, false, |p| p)
    }

    /// The square of half side `h` at height `z`, clockwise when `cw`, each corner moved by `to`.
    fn square_by(h: f64, z: f64, cw: bool, to: impl Fn([f64; 3]) -> [f64; 3]) -> Vec<Curve> {
        let mut c = vec![[-h, -h, z], [h, -h, z], [h, h, z], [-h, h, z]];
        if cw {
            c.reverse();
        }
        let c: Vec<[f64; 3]> = c.into_iter().map(to).collect();
        c.iter()
            .zip(c.iter().cycle().skip(1))
            .map(|(&a, &b)| line(a, b))
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

    /// The capped skin through `sections`, its side faces' count of
    /// planes and B-splines, its volume, after the B-rep check passes.
    fn skin(sections: &[Vec<Curve>], degree: usize) -> (usize, usize, f64) {
        let mut k = Kernel::default();
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
    /// quadratic skin's four walls are flat, so they are planes; the solid is the frustum.
    #[test]
    fn a_skin_through_three_squares_in_line_has_planar_walls() {
        let sections = [square(55.0, 0.0), square(77.5, 60.0), square(100.0, 120.0)];
        let want = frustum(120.0, 4.0 * 55.0 * 55.0, 4.0 * 100.0 * 100.0);
        let (planes, splines, volume) = skin(&sections, 2);
        assert_eq!((planes, splines), (4, 0), "planes");
        assert!(near(volume, want), "{volume} vs {want}");
    }

    /// Three squares out of line (55, 80, 100): the walls bend, so they stay B-splines.
    #[test]
    fn a_skin_through_three_squares_out_of_line_keeps_curved_walls() {
        let sections = [square(55.0, 0.0), square(80.0, 60.0), square(100.0, 120.0)];
        let (planes, splines, _) = skin(&sections, 2);
        assert_eq!((planes, splines), (0, 4));
    }

    /// Two D shapes, the top 0.6 of the bottom: the walls on the lines are flat and become
    /// planes, the wall on the arc stays a B-spline; the solid is the frustum.
    #[test]
    fn a_skin_of_lines_and_an_arc_has_planar_walls_on_the_lines() {
        let sections = [d_shape(50.0, 0.0), d_shape(30.0, 50.0)];
        let area = |s: f64| 2.0 * s * s + std::f64::consts::PI * s * s / 2.0;
        let want = frustum(50.0, area(50.0), area(30.0));
        let (planes, splines, volume) = skin(&sections, 1);
        assert_eq!((planes, splines), (3, 1));
        assert!(near(volume, want), "{volume} vs {want}");
    }

    /// The squares in line run clockwise: the walls' B-splines face the other way, and each is
    /// still a plane facing out (the check passes and the volume is positive).
    #[test]
    fn walls_are_planes_whichever_way_the_sections_run() {
        let want = frustum(120.0, 4.0 * 55.0 * 55.0, 4.0 * 100.0 * 100.0);
        for cw in [false, true] {
            let sections = [55.0, 77.5, 100.0]
                .iter()
                .zip([0.0, 60.0, 120.0])
                .map(|(&h, z)| square_by(h, z, cw, |p| p))
                .collect::<Vec<_>>();
            let (planes, splines, volume) = skin(&sections, 2);
            assert_eq!((planes, splines), (4, 0), "cw {cw}");
            assert!(near(volume, want), "cw {cw}: {volume} vs {want}");
        }
    }

    /// The plane's normal follows the B-spline's whichever way `plane_of`'s points: kept along
    /// it, y reversed against it, none across it. (The skins above all meet the first: their
    /// `IsPlanar` normal already agrees.)
    #[test]
    fn the_plane_faces_the_way_the_spline_does() {
        let frame = PlaneFrame {
            origin: Point3::new(1.0, 2.0, 3.0),
            x: Vec3::new(1.0, 0.0, 0.0),
            y: Vec3::new(0.0, 1.0, 0.0),
            z: Vec3::new(0.0, 0.0, 1.0),
        };
        for normal in [
            frame.z,
            -frame.z,
            Vec3::new(0.6, 0.0, 0.8),
            Vec3::new(0.0, -0.6, -0.8),
        ] {
            let (x, y) = axes(&frame, normal).expect("a clear side");
            assert!(x.cross(y).dot(normal) > 0.5, "{normal:?}");
        }
        assert!(axes(&frame, Vec3::new(1.0, 0.0, 0.0)).is_none());
        assert!(axes(&frame, Vec3::new(0.0, 0.9, 0.3)).is_none());
    }

    /// A duct twenty times the size (half sides 1100 / 1550 / 2000, 2.4 m long) turned about an
    /// oblique axis: rounding in the skin's net grows with its size and the tilt, and the walls
    /// are still found flat.
    #[test]
    fn a_large_tilted_skin_still_has_planar_walls() {
        let (axis, angle): ([f64; 3], f64) = ([1.0, 2.0, 3.0], 37f64.to_radians());
        let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
        let a = [axis[0] / n, axis[1] / n, axis[2] / n];
        let (s, c) = angle.sin_cos();
        // Rodrigues: p cos + (a × p) sin + a (a · p)(1 − cos)
        let turn = move |p: [f64; 3]| {
            let d = a[0] * p[0] + a[1] * p[1] + a[2] * p[2];
            let x = [
                a[1] * p[2] - a[2] * p[1],
                a[2] * p[0] - a[0] * p[2],
                a[0] * p[1] - a[1] * p[0],
            ];
            [0, 1, 2].map(|i| p[i] * c + x[i] * s + a[i] * d * (1.0 - c))
        };
        let sections = [1100.0, 1550.0, 2000.0]
            .iter()
            .zip([0.0, 1200.0, 2400.0])
            .map(|(&h, z)| square_by(h, z, false, turn))
            .collect::<Vec<_>>();
        let want = frustum(2400.0, 4.0 * 1100.0 * 1100.0, 4.0 * 2000.0 * 2000.0);
        let (planes, splines, volume) = skin(&sections, 2);
        assert_eq!((planes, splines), (4, 0));
        assert!(near(volume, want), "{volume} vs {want}");
    }
}
