//! What is drawn for the controllers: each one's body where the hand holds it, and its ray
//! from where it points. Triangles in the room (metres), flat-coloured per face, wound
//! counter-clockwise seen from outside, so drawing them with back faces culled shows each box
//! right without a depth buffer.

use solidcraft_xr::input::HandInput;
use solidcraft_xr::view::Pose;

use crate::vector::{add, turn};

/// The controller's body: half its width, height and length (metres).
pub const BODY: [f64; 3] = [0.02, 0.015, 0.06];

/// The ray: half its width (metres).
pub const RAY_HALF_WIDTH: f64 = 0.001;

/// How far the ray reaches (metres).
pub const RAY_LENGTH: f64 = 3.0;

/// One corner of a triangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// Where it is in the room (metres).
    pub position: [f32; 3],
    /// Its colour, linear light, alpha 1.
    pub colour: [f32; 4],
}

/// The colours the marks are drawn in (linear light).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub body: [f32; 3],
    /// The body while its grip is held.
    pub body_held: [f32; 3],
    pub ray: [f32; 3],
    /// The ray while its trigger is held.
    pub ray_pressed: [f32; 3],
}

/// The dot where a ray stops on the model: half its side (metres).
pub const DOT_HALF: f64 = 0.004;

/// The most vertices [`marks`] makes: three boxes for each hand (body, ray, dot).
pub const MAX_VERTICES: usize = 2 * 3 * 36;

/// The marks for both hands. A hand whose pose is not tracked has no mark for it; `held` says,
/// per hand, whether its grip is held and its trigger pressed; `reach`, per hand, how far its ray
/// goes before it stops on the model (metres), with a dot there; `None` reaches `ray_length`
/// (metres; [`RAY_LENGTH`] unless asked otherwise).
pub fn marks(hands: &[HandInput; 2], held: [[bool; 2]; 2], reach: [Option<f64>; 2], ray_length: f64, style: &Style) -> Vec<Vertex> {
    let ray_length = if ray_length.is_finite() && ray_length > 0.0 { ray_length } else { RAY_LENGTH };
    let mut out = Vec::with_capacity(MAX_VERTICES);
    for ((hand, [grip, trigger]), reach) in hands.iter().zip(held).zip(reach) {
        if let Some(aim) = &hand.aim {
            // From just in front of the controller, down its −Z, as far as it reaches.
            let stop = reach.filter(|r| r.is_finite() && *r > 0.0).map(|r| r.min(ray_length));
            let length = stop.unwrap_or(ray_length);
            let colour = if trigger { style.ray_pressed } else { style.ray };
            cuboid(&mut out, aim, [0.0, 0.0, -length * 0.5], [RAY_HALF_WIDTH, RAY_HALF_WIDTH, length * 0.5], colour);
            if let Some(at) = stop {
                cuboid(&mut out, aim, [0.0, 0.0, -at], [DOT_HALF; 3], colour);
            }
        }
        if let Some(body) = &hand.grip {
            cuboid(&mut out, body, [0.0; 3], BODY, if grip { style.body_held } else { style.body });
        }
    }
    out
}

/// How tall a sign is for each metre between the head and what it names: it looks about as big
/// near or far.
pub const SIGN_HEIGHT_PER_METRE: f64 = 0.04;

/// A sign is never smaller than this, nor taller than [`SIGN_MAX`] (metres).
pub const SIGN_MIN: f64 = 0.012;

/// See [`SIGN_MIN`].
pub const SIGN_MAX: f64 = 0.2;

/// The corners of a sign naming what is at `at`, seen from `head` (both in the room, metres):
/// upright, turned toward the head, its lower edge just above `at` and a little in front of it,
/// `aspect` times as wide as tall. Top left, top right, bottom right, bottom left. `None` when
/// the head is at `at` or a number is not one.
pub fn sign_corners(at: [f64; 3], head: [f64; 3], aspect: f64) -> Option<[[f64; 3]; 4]> {
    sign_corners_sized(at, head, aspect, 1.0)
}

/// [`sign_corners`], the sign `factor` times as big.
pub fn sign_corners_sized(at: [f64; 3], head: [f64; 3], aspect: f64, factor: f64) -> Option<[[f64; 3]; 4]> {
    let factor = if factor.is_finite() && factor > 0.0 { factor } else { 1.0 };
    use crate::vector::{cross, length, scale, sub, unit};
    let toward = sub(head, at);
    let distance = length(toward);
    let back = unit(toward)?;
    if !aspect.is_finite() || aspect <= 0.0 {
        return None;
    }
    // Upright where it can be; seen from straight above or below, along the head's line instead.
    let right = unit(cross([0.0, 1.0, 0.0], back)).or_else(|| unit(cross(back, [0.0, 0.0, 1.0])))?;
    let up = cross(back, right);
    let height = (distance * SIGN_HEIGHT_PER_METRE).clamp(SIGN_MIN, SIGN_MAX) * factor;
    let width = height * aspect;
    // Lifted off the point (so the dot stays in sight) and pulled toward the head (so the model
    // does not cover it where the point is on a slope).
    let bottom = add(add(at, scale(up, height * 0.4)), scale(back, (height * 0.5).min(distance * 0.5)));
    let corner = |x: f64, y: f64| add(add(bottom, scale(right, x * width)), scale(up, y * height));
    Some([corner(-0.5, 1.0), corner(0.5, 1.0), corner(0.5, 0.0), corner(-0.5, 0.0)])
}

/// The corners of a board `height` tall (metres) and `aspect` times as wide, its middle at `at`,
/// upright and turned toward `head`: top left, top right, bottom right, bottom left. `None` when
/// the head is at `at` or a number is not one.
pub fn board_corners(at: [f64; 3], head: [f64; 3], height: f64, aspect: f64) -> Option<[[f64; 3]; 4]> {
    use crate::vector::{cross, scale, sub, unit};
    let back = unit(sub(head, at))?;
    if !(aspect.is_finite() && aspect > 0.0 && height.is_finite() && height > 0.0) {
        return None;
    }
    let right = unit(cross([0.0, 1.0, 0.0], back)).or_else(|| unit(cross(back, [0.0, 0.0, 1.0])))?;
    let up = cross(back, right);
    let (w, h) = (height * aspect * 0.5, height * 0.5);
    let corner = |x: f64, y: f64| add(add(at, scale(right, x * w)), scale(up, y * h));
    Some([corner(-1.0, 1.0), corner(1.0, 1.0), corner(1.0, -1.0), corner(-1.0, -1.0)])
}

/// A flat line from `a` to `b` (the room, metres), `half_width` either side, lying across
/// `normal` (the plane it is seen in), seen from both sides: twelve vertices.
pub fn line(out: &mut Vec<Vertex>, a: [f64; 3], b: [f64; 3], normal: [f64; 3], half_width: f64, colour: [f32; 3]) {
    use crate::vector::{cross, scale, sub, unit};
    let Some(along) = unit(sub(b, a)) else { return };
    let Some(across) = unit(cross(normal, along)) else { return };
    let side = scale(across, half_width);
    let quad = [sub(a, side), sub(b, side), add(b, side), add(a, side)];
    let colour = [colour[0], colour[1], colour[2], 1.0];
    for i in [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2] {
        out.push(Vertex { position: quad[i].map(|v| v as f32), colour });
    }
}

/// A box at `offset` from `pose` (in the pose's own directions), `half` its half sides, faces
/// shaded by which way they face so that its shape shows: up brightest, down darkest. Nothing
/// when the pose is not one.
fn cuboid(out: &mut Vec<Vertex>, pose: &Pose, offset: [f64; 3], half: [f64; 3], colour: [f32; 3]) {
    let Some(axes) = pose.axes() else { return };
    let corner = |sx: f64, sy: f64, sz: f64| -> [f32; 3] {
        let local = [offset[0] + sx * half[0], offset[1] + sy * half[1], offset[2] + sz * half[2]];
        add(pose.position, turn(&axes, local)).map(|v| v as f32)
    };
    // Each face: its four corners counter-clockwise seen from outside, and its shade.
    let faces: [([[f64; 3]; 4], f32); 6] = [
        ([[1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0], [1.0, -1.0, 1.0]], 0.8),
        ([[-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0], [-1.0, 1.0, -1.0]], 0.8),
        ([[-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0]], 1.0),
        ([[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [-1.0, -1.0, 1.0]], 0.45),
        ([[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]], 0.65),
        ([[-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [1.0, 1.0, -1.0], [1.0, -1.0, -1.0]], 0.65),
    ];
    for (quad, shade) in faces {
        let [a, b, c, d] = quad.map(|[x, y, z]| corner(x, y, z));
        let colour = [colour[0] * shade, colour[1] * shade, colour[2] * shade, 1.0];
        for position in [a, b, c, a, c, d] {
            out.push(Vertex { position, colour });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE: Style = Style { body: [0.5; 3], body_held: [1.0, 0.5, 0.0], ray: [0.0, 0.5, 1.0], ray_pressed: [1.0; 3] };

    /// A pose at `position`, turned a little about two axes.
    fn pose(position: [f64; 3]) -> Pose {
        // Turned a little about two axes, so that winding is checked off the room's axes too.
        let (s, c) = (0.3_f64.sin(), 0.3_f64.cos());
        let q = [s * 0.6, s * 0.8, 0.0, c];
        Pose { position, orientation: q }
    }

    /// `a` − `b`, in double precision.
    fn sub(a: [f32; 3], b: [f32; 3]) -> [f64; 3] {
        [0, 1, 2].map(|k| f64::from(a[k]) - f64::from(b[k]))
    }

    /// The cross product, right-handed.
    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }

    /// Each hand's ray and body; a hand with neither pose tracked has no marks.
    #[test]
    fn each_tracked_hand_has_a_body_and_a_ray() {
        let left = HandInput { grip: Some(pose([-0.2, 1.0, -0.3])), aim: Some(pose([-0.2, 1.0, -0.35])), ..HandInput::NONE };
        let all = marks(&[left, left], [[false; 2]; 2], [None; 2], RAY_LENGTH, &STYLE);
        assert_eq!(all.len(), 2 * 2 * 36);
        assert_eq!(marks(&[left, HandInput::NONE], [[false; 2]; 2], [None; 2], RAY_LENGTH, &STYLE).len(), 2 * 36);
        assert!(marks(&[HandInput::NONE; 2], [[true; 2]; 2], [Some(1.0); 2], RAY_LENGTH, &STYLE).is_empty());
        let lost = HandInput { grip: Some(Pose { position: [0.0; 3], orientation: [0.0; 4] }), ..HandInput::NONE };
        assert!(marks(&[lost, HandInput::NONE], [[false; 2]; 2], [None; 2], RAY_LENGTH, &STYLE).is_empty());
        assert!(all.iter().all(|v| v.position.iter().chain(&v.colour).all(|x| x.is_finite())));
    }

    /// The ray reaches [`RAY_LENGTH`] down the controller's −Z; the body sits about the grip.
    #[test]
    fn the_ray_points_where_the_controller_points() {
        let aim = Pose { position: [0.0, 1.0, 0.0], orientation: [0.0, 0.0, 0.0, 1.0] };
        let hand = HandInput { aim: Some(aim), ..HandInput::NONE };
        let ray = marks(&[hand, HandInput::NONE], [[false; 2]; 2], [None; 2], RAY_LENGTH, &STYLE);
        let z: Vec<f32> = ray.iter().map(|v| v.position[2]).collect();
        let (near, far) = z.iter().fold((f32::MIN, f32::MAX), |(hi, lo), &v| (hi.max(v), lo.min(v)));
        assert_eq!(near, 0.0);
        assert!((far + RAY_LENGTH as f32).abs() < 1e-6);
        assert!(ray.iter().all(|v| (v.position[1] - 1.0).abs() <= RAY_HALF_WIDTH as f32 + 1e-6));
    }

    /// Every triangle faces out of its box (counter-clockwise seen from outside), so culling
    /// back faces leaves the near side.
    #[test]
    fn triangles_face_out_of_their_box() {
        let hand = HandInput { grip: Some(pose([0.3, 1.1, -0.2])), aim: Some(pose([0.3, 1.1, -0.25])), ..HandInput::NONE };
        let all = marks(&[hand, HandInput::NONE], [[false; 2]; 2], [None; 2], RAY_LENGTH, &STYLE);
        for cuboid in all.chunks(36) {
            let n = cuboid.len() as f64;
            let middle = [0, 1, 2].map(|k| cuboid.iter().map(|v| f64::from(v.position[k])).sum::<f64>() / n);
            for t in cuboid.chunks(3) {
                let normal = cross(sub(t[1].position, t[0].position), sub(t[2].position, t[0].position));
                let centre = [0, 1, 2].map(|k| (0..3).map(|i| f64::from(t[i].position[k])).sum::<f64>() / 3.0);
                let out = [0, 1, 2].map(|k| centre[k] - middle[k]);
                assert!(normal.iter().zip(out).map(|(a, b)| a * b).sum::<f64>() > 0.0, "a triangle faces in");
            }
        }
    }

    /// The grip held and the trigger pressed change the colours.
    #[test]
    fn held_buttons_change_the_colour() {
        let hand = HandInput { grip: Some(pose([0.0; 3])), aim: Some(pose([0.0; 3])), ..HandInput::NONE };
        let colours = |held| {
            let m = marks(&[hand, HandInput::NONE], [held, [false; 2]], [None; 2], RAY_LENGTH, &STYLE);
            (m[0].colour, m[36].colour)
        };
        let (ray, body) = colours([false, false]);
        let (ray_pressed, body_held) = colours([true, true]);
        assert_ne!(ray, ray_pressed);
        assert_ne!(body, body_held);
    }

    /// A controller at `position` pointing down the room's −Z.
    fn ahead(position: [f64; 3]) -> HandInput {
        HandInput { aim: Some(Pose { position, orientation: [0.0, 0.0, 0.0, 1.0] }), ..HandInput::NONE }
    }

    /// The least and greatest Z of `vertices`.
    fn z_range(vertices: &[Vertex]) -> (f32, f32) {
        vertices.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v.position[2]), hi.max(v.position[2])))
    }

    /// Whether two points are the same but for rounding.
    fn near(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
    }

    /// A ray that meets the model stops there, with a dot about where it stops; past the ray's
    /// length it stops at its end; no reach (or one that is no distance) is the whole ray, no dot.
    #[test]
    fn a_ray_stops_where_it_meets_the_model_with_a_dot() {
        let hand = ahead([0.0, 1.0, 0.0]);
        let both = [hand, HandInput::NONE];
        let hit = marks(&both, [[false; 2]; 2], [Some(0.5), None], RAY_LENGTH, &STYLE);
        assert_eq!(hit.len(), 2 * 36, "the ray and its dot");
        let (ray, dot) = hit.split_at(36);
        assert_eq!(z_range(ray), (-0.5, 0.0));
        let (lo, hi) = z_range(dot);
        assert!((lo + 0.5 + DOT_HALF as f32).abs() < 1e-6 && (hi + 0.5 - DOT_HALF as f32).abs() < 1e-6, "{lo} {hi}");
        let beyond = marks(&both, [[false; 2]; 2], [Some(10.0), None], 2.0, &STYLE);
        assert!((z_range(&beyond[..36]).0 + 2.0).abs() < 1e-6, "cut at the ray's own length");
        assert_eq!(beyond.len(), 2 * 36, "with its dot at the end");
        for reach in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let whole = marks(&both, [[false; 2]; 2], [Some(reach), None], RAY_LENGTH, &STYLE);
            assert_eq!(whole.len(), 36, "{reach}: no dot");
            assert!((z_range(&whole).0 + RAY_LENGTH as f32).abs() < 1e-6, "{reach}");
        }
    }

    /// A ray length that is no length draws the ray [`RAY_LENGTH`] long; everything drawn for
    /// both hands fits in [`MAX_VERTICES`].
    #[test]
    fn a_ray_length_that_is_no_length_falls_back() {
        let hand = ahead([0.0, 1.0, 0.0]);
        for length in [0.0, -2.0, f64::NAN, f64::INFINITY] {
            let ray = marks(&[hand, HandInput::NONE], [[false; 2]; 2], [None; 2], length, &STYLE);
            assert!((z_range(&ray).0 + RAY_LENGTH as f32).abs() < 1e-6, "{length}");
        }
        let full = HandInput { grip: Some(pose([0.0, 1.0, 0.0])), ..hand };
        let all = marks(&[full, full], [[true; 2]; 2], [Some(1.0); 2], RAY_LENGTH, &STYLE);
        assert_eq!(all.len(), MAX_VERTICES);
    }

    /// A sign stands upright facing the head, its size growing with the distance within its
    /// limits, its lower edge just above the point and pulled toward the head.
    #[test]
    fn a_sign_stands_over_its_point_facing_the_head() {
        // A metre away: 0.04 tall, twice as wide; lifted 0.4 of its height, 0.02 toward the head.
        let [tl, tr, br, bl] = sign_corners([0.0, 1.0, -1.0], [0.0, 1.0, 0.0], 2.0).unwrap();
        assert!(near(tl, [-0.04, 1.056, -0.98]) && near(tr, [0.04, 1.056, -0.98]), "{tl:?} {tr:?}");
        assert!(near(br, [0.04, 1.016, -0.98]) && near(bl, [-0.04, 1.016, -0.98]), "{br:?} {bl:?}");
        let height = |at: [f64; 3], head: [f64; 3], factor: f64| {
            let [tl, _, _, bl] = sign_corners_sized(at, head, 1.0, factor).unwrap();
            tl[1] - bl[1]
        };
        assert!((height([0.0; 3], [0.0, 0.0, 100.0], 1.0) - SIGN_MAX).abs() < 1e-12, "far: no taller than SIGN_MAX");
        assert!((height([0.0; 3], [0.0, 0.0, 0.1], 1.0) - SIGN_MIN).abs() < 1e-12, "near: no smaller than SIGN_MIN");
        assert!((height([0.0; 3], [0.0, 0.0, 1.0], 2.0) - 0.08).abs() < 1e-12, "twice as big");
        for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!((height([0.0; 3], [0.0, 0.0, 1.0], factor) - 0.04).abs() < 1e-12, "{factor}: as asked for none");
        }
        // Seen from straight above, it lies along the head's line instead, all its numbers numbers.
        let above = sign_corners([0.0, 1.0, -1.0], [0.0, 3.0, -1.0], 1.0).unwrap();
        assert!(above.iter().flatten().all(|v| v.is_finite()));
        assert!((above[1][0] - above[0][0] - 0.08).abs() < 1e-12, "two metres away: 0.08 wide along X");
    }

    /// A sign seen from where it is, or with numbers that are none, is not drawn.
    #[test]
    fn a_sign_from_numbers_that_are_not_any_is_refused() {
        let (at, head) = ([0.0, 1.0, -1.0], [0.0, 1.0, 0.0]);
        assert_eq!(sign_corners(at, at, 1.0), None);
        for aspect in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(sign_corners(at, head, aspect), None, "{aspect}");
        }
        assert_eq!(sign_corners([f64::NAN; 3], head, 1.0), None);
        assert_eq!(sign_corners(at, [0.0, f64::INFINITY, 0.0], 1.0), None);
    }

    /// A board is centred on its point, upright and facing the head, as tall and wide as asked;
    /// one with no size, no shape or no direction is not drawn.
    #[test]
    fn a_board_faces_the_head_centred_on_its_point() {
        let (at, head) = ([0.0, 1.0, -1.0], [0.0, 1.0, 0.0]);
        let [tl, tr, br, bl] = board_corners(at, head, 0.2, 1.5).unwrap();
        assert!(near(tl, [-0.15, 1.1, -1.0]) && near(tr, [0.15, 1.1, -1.0]));
        assert!(near(br, [0.15, 0.9, -1.0]) && near(bl, [-0.15, 0.9, -1.0]));
        assert_eq!(board_corners(at, at, 0.2, 1.5), None);
        for (height, aspect) in [(0.0, 1.0), (-0.2, 1.0), (f64::NAN, 1.0), (0.2, 0.0), (0.2, f64::NAN), (f64::INFINITY, 1.0)] {
            assert_eq!(board_corners(at, head, height, aspect), None, "{height} {aspect}");
        }
    }

    /// A line is a strip along it, `half_width` either side across the plane it is seen in,
    /// seen from both sides; one with no length, or lying along its plane's normal, is not drawn.
    #[test]
    fn a_line_is_a_strip_seen_from_both_sides() {
        let mut out = vec![Vertex { position: [9.0; 3], colour: [1.0; 4] }];
        line(&mut out, [0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.01, [0.5; 3]);
        assert_eq!(out.len(), 1 + 12, "added to what was there");
        assert_eq!(out[0].position, [9.0; 3]);
        let strip = &out[1..];
        assert!(strip.iter().all(|v| v.position[2] == 0.0 && v.position[1].abs() <= 0.01 + 1e-7 && v.colour == [0.5, 0.5, 0.5, 1.0]));
        assert_eq!(strip.iter().map(|v| v.position[0]).fold(f32::MIN, f32::max), 1.0);
        let facing: Vec<f64> = strip.chunks(3).map(|t| cross(sub(t[1].position, t[0].position), sub(t[2].position, t[0].position))[2]).collect();
        assert_eq!((facing.iter().filter(|z| **z > 0.0).count(), facing.iter().filter(|z| **z < 0.0).count()), (2, 2));

        let mut none = Vec::new();
        line(&mut none, [1.0; 3], [1.0; 3], [0.0, 0.0, 1.0], 0.01, [0.5; 3]);
        line(&mut none, [0.0; 3], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], 0.01, [0.5; 3]);
        line(&mut none, [0.0; 3], [f64::NAN, 0.0, 0.0], [0.0, 0.0, 1.0], 0.01, [0.5; 3]);
        line(&mut none, [0.0; 3], [1.0, 0.0, 0.0], [0.0; 3], 0.01, [0.5; 3]);
        assert!(none.is_empty());
    }
}
