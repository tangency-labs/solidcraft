//! Picking what is drawn as lines: the point of a set of drawn polylines nearest the pointer on
//! screen ([`nearest_drawn_point`]), with the polyline it lies on, its segment, where along the
//! segment it is and how far from the pointer in pixels. The lines are the host's (edges, curves,
//! isoparms, guides), named by the host's own ids, so a click or a snap picks the line drawn under
//! the cursor however the host keeps them.

use crate::camera::Camera;
use crate::vec::Vec3Ops;
use solidcore_record::Vec3;

/// The drawn point nearest the pointer ([`nearest_drawn_point`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawnPoint<I> {
    /// The id of the polyline it lies on, as the host passed it.
    pub line: I,
    /// The index of its segment: from the polyline's point `segment` to point `segment + 1`
    /// (0 for a polyline of one point).
    pub segment: usize,
    /// Where along the segment it lies, from 0 at its first point to 1 at its second, in the
    /// segment's own (world) length: `point` is the first point blended this far to the second.
    pub t: f64,
    /// The point in the world.
    pub point: Vec3,
    /// Its distance from the pointer on screen, in pixels.
    pub gap: f64,
}

/// The point of the drawn polylines `lines` (each an id and its points, in world coordinates)
/// nearest `pointer` (pixels, x right, y down) on a `width` by `height` pixel view through
/// `camera`: on each segment as drawn, the point nearest the pointer on screen, and of those the
/// nearest; ties go to the earlier line and segment. Its world point and parameter are the ones
/// under that screen point (in perspective, the segment's screen and world parameters differ).
/// A segment with an end at or behind the eye is not drawn, so it is not picked. `None` when no
/// segment or point is in front of the eye.
pub fn nearest_drawn_point<'a, I: Copy>(
    camera: &Camera,
    width: f64,
    height: f64,
    pointer: (f64, f64),
    lines: impl IntoIterator<Item = (I, &'a [Vec3])>,
) -> Option<DrawnPoint<I>> {
    let view = Projection::new(camera, width, height);
    let mut best: Option<DrawnPoint<I>> = None;
    let mut consider = |found: DrawnPoint<I>| {
        if best.as_ref().is_none_or(|b| found.gap < b.gap) {
            best = Some(found);
        }
    };
    for (line, points) in lines {
        if let [only] = points {
            if let Some(s) = view.screen(*only) {
                consider(DrawnPoint { line, segment: 0, t: 0.0, point: *only, gap: distance(pointer, (s.x, s.y)) });
            }
            continue;
        }
        for (segment, ends) in points.windows(2).enumerate() {
            let [a, b] = ends else { continue };
            let (Some(sa), Some(sb)) = (view.screen(*a), view.screen(*b)) else { continue };
            let s = nearest_on_screen_segment(pointer, (sa.x, sa.y), (sb.x, sb.y));
            let on_screen = (sa.x + (sb.x - sa.x) * s, sa.y + (sb.y - sa.y) * s);
            // the world parameter under screen parameter `s`: blended in 1 / w
            let t = s * sa.w / ((1.0 - s) * sb.w + s * sa.w);
            consider(DrawnPoint { line, segment, t, point: a.lerp(*b, t), gap: distance(pointer, on_screen) });
        }
    }
    best
}

/// A point on screen: pixels and its clip `w` (its depth in front of the eye in perspective, 1
/// in an orthographic view).
#[derive(Clone, Copy, Debug)]
struct Screen {
    x: f64,
    y: f64,
    w: f64,
}

/// The camera's projection onto a view of pixels.
struct Projection {
    matrix: crate::camera::Mat4,
    width: f64,
    height: f64,
}

impl Projection {
    fn new(camera: &Camera, width: f64, height: f64) -> Self {
        // x, y and w do not depend on the clip planes: any radius projects the same pixels
        Projection { matrix: camera.view_proj(width / height.max(1.0), 1.0), width, height }
    }

    /// The point's pixel and clip `w`, as [`Camera::to_screen`] places it; `None` at or behind
    /// the eye.
    fn screen(&self, p: Vec3) -> Option<Screen> {
        let c = self.matrix.apply(p);
        let w = c[3];
        (w > 1e-12 && w.is_finite()).then(|| Screen { x: (c[0] / w + 1.0) * 0.5 * self.width, y: (1.0 - c[1] / w) * 0.5 * self.height, w })
    }
}

/// The parameter in [0, 1] of the point of segment `a`–`b` nearest `p` (0 for a segment of no
/// length).
fn nearest_on_screen_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length2 = dx * dx + dy * dy;
    if length2 <= 0.0 || !length2.is_finite() {
        return 0.0;
    }
    (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length2).clamp(0.0, 1.0)
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 800.0;
    const H: f64 = 600.0;

    /// Looking down on the XY plane (top view), orthographic or in perspective.
    fn top(fov: f64) -> Camera {
        let mut c = Camera { target: Vec3::ZERO, distance: 100.0, fov, ..Camera::default() };
        c.set_view(crate::camera::StandardView::Top);
        c
    }

    fn pixel(c: &Camera, p: Vec3) -> (f64, f64) {
        let (x, y, _) = c.to_screen(p, W, H, 100.0).unwrap_or((f64::NAN, f64::NAN, 0.0));
        (x, y)
    }

    #[test]
    fn the_nearest_line_its_segment_and_the_point_under_the_pointer() {
        for fov in [0.0, 0.6] {
            let c = top(fov);
            let near = [Vec3::new(-10.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 5.0, 0.0)];
            let far = [Vec3::new(-10.0, 30.0, 0.0), Vec3::new(10.0, 30.0, 0.0)];
            let under = Vec3::new(5.0, 2.5, 0.0);
            let (x, y) = pixel(&c, under);
            let lines: [(u32, &[Vec3]); 2] = [(7, &far), (3, &near)];
            let found = nearest_drawn_point(&c, W, H, (x, y + 4.0), lines);
            let Some(found) = found else { panic!("fov {fov}: nothing picked") };
            assert_eq!((found.line, found.segment), (3, 1), "fov {fov}");
            // the segment runs at a slant on screen: the nearest point is within the 4 px offset
            assert!(found.gap > 0.0 && found.gap <= 4.0 + 1e-9, "fov {fov}: gap {}", found.gap);
            let on = pixel(&c, found.point);
            assert!((distance(on, (x, y + 4.0)) - found.gap).abs() < 1e-6, "fov {fov}: the point is drawn where it is found");
            assert!((found.point.dist(near[1].lerp(near[2], found.t))) < 1e-9);
        }
    }

    /// In perspective a segment receding from the eye is drawn shorter at its far end: the world
    /// point under the pointer is not the one at the screen fraction, but the one drawn there.
    #[test]
    fn in_perspective_the_world_point_is_the_one_drawn_under_the_pointer() {
        let c = Camera { target: Vec3::ZERO, distance: 50.0, yaw: 0.0, pitch: 0.3, fov: 0.8, ..Camera::default() };
        let line = [Vec3::new(0.0, -20.0, 0.0), Vec3::new(0.0, 60.0, 0.0)];
        let under = line[0].lerp(line[1], 0.25);
        let found = nearest_drawn_point(&c, W, H, pixel(&c, under), [(0usize, &line[..])]);
        let Some(found) = found else { panic!("nothing picked") };
        assert!(found.gap < 1e-6, "gap {}", found.gap);
        assert!((found.t - 0.25).abs() < 1e-9, "t {}", found.t);
        assert!(found.point.dist(under) < 1e-9);
        let a = pixel(&c, line[0]);
        let b = pixel(&c, line[1]);
        let p = pixel(&c, under);
        let screen_fraction = distance(a, p) / distance(a, b);
        assert!((screen_fraction - 0.25).abs() > 0.01, "the view should foreshorten the line: {screen_fraction}");
    }

    #[test]
    fn past_an_end_the_end_is_nearest_and_a_single_point_is_a_point() {
        let c = top(0.0);
        let line = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)];
        let (x, y) = pixel(&c, Vec3::new(20.0, 0.0, 0.0));
        let found = nearest_drawn_point(&c, W, H, (x, y), [(1u8, &line[..])]);
        assert_eq!(found.map(|f| (f.segment, f.t, f.point)), Some((0, 1.0, line[1])));

        let dot = [Vec3::new(3.0, 3.0, 0.0)];
        let found = nearest_drawn_point(&c, W, H, pixel(&c, dot[0]), [(2u8, &dot[..])]);
        assert_eq!(found.map(|f| (f.line, f.segment, f.t, f.point, f.gap)), Some((2, 0, 0.0, dot[0], 0.0)));
    }

    #[test]
    fn nothing_drawn_or_behind_the_eye_picks_nothing() {
        let c = Camera { target: Vec3::ZERO, distance: 50.0, fov: 0.8, ..Camera::default() };
        let none: [(u8, &[Vec3]); 0] = [];
        assert_eq!(nearest_drawn_point(&c, W, H, (400.0, 300.0), none), None);
        let empty: [Vec3; 0] = [];
        assert_eq!(nearest_drawn_point(&c, W, H, (400.0, 300.0), [(0u8, &empty[..])]), None);
        // a segment from behind the eye to further behind it
        let back = c.basis().2;
        let behind = [c.eye() + back * 10.0, c.eye() + back * 20.0];
        assert_eq!(nearest_drawn_point(&c, W, H, (400.0, 300.0), [(0u8, &behind[..])]), None);
    }

    #[test]
    fn a_tie_goes_to_the_earlier_line() {
        let c = top(0.0);
        let line = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 0.0)];
        let found = nearest_drawn_point(&c, W, H, pixel(&c, Vec3::new(5.0, 0.0, 0.0)), [(1u8, &line[..]), (2u8, &line[..])]);
        assert_eq!(found.map(|f| f.line), Some(1));
    }
}
