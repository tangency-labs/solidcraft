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

/// The most vertices [`marks`] makes: two boxes for each hand.
pub const MAX_VERTICES: usize = 2 * 2 * 36;

/// The marks for both hands. A hand whose pose is not tracked has no mark for it; `held` says,
/// per hand, whether its grip is held and its trigger pressed.
pub fn marks(hands: &[HandInput; 2], held: [[bool; 2]; 2], style: &Style) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(MAX_VERTICES);
    for (hand, [grip, trigger]) in hands.iter().zip(held) {
        if let Some(aim) = &hand.aim {
            // From just in front of the controller, down its −Z.
            let ray = [RAY_HALF_WIDTH, RAY_HALF_WIDTH, RAY_LENGTH * 0.5];
            cuboid(&mut out, aim, [0.0, 0.0, -RAY_LENGTH * 0.5], ray, if trigger { style.ray_pressed } else { style.ray });
        }
        if let Some(body) = &hand.grip {
            cuboid(&mut out, body, [0.0; 3], BODY, if grip { style.body_held } else { style.body });
        }
    }
    out
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
        let all = marks(&[left, left], [[false; 2]; 2], &STYLE);
        assert_eq!(all.len(), MAX_VERTICES);
        assert_eq!(marks(&[left, HandInput::NONE], [[false; 2]; 2], &STYLE).len(), 2 * 36);
        assert!(marks(&[HandInput::NONE; 2], [[true; 2]; 2], &STYLE).is_empty());
        let lost = HandInput { grip: Some(Pose { position: [0.0; 3], orientation: [0.0; 4] }), ..HandInput::NONE };
        assert!(marks(&[lost, HandInput::NONE], [[false; 2]; 2], &STYLE).is_empty());
        assert!(all.iter().all(|v| v.position.iter().chain(&v.colour).all(|x| x.is_finite())));
    }

    /// The ray reaches [`RAY_LENGTH`] down the controller's −Z; the body sits about the grip.
    #[test]
    fn the_ray_points_where_the_controller_points() {
        let aim = Pose { position: [0.0, 1.0, 0.0], orientation: [0.0, 0.0, 0.0, 1.0] };
        let hand = HandInput { aim: Some(aim), ..HandInput::NONE };
        let ray = marks(&[hand, HandInput::NONE], [[false; 2]; 2], &STYLE);
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
        let all = marks(&[hand, HandInput::NONE], [[false; 2]; 2], &STYLE);
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
            let m = marks(&[hand, HandInput::NONE], [held, [false; 2]], &STYLE);
            (m[0].colour, m[36].colour)
        };
        let (ray, body) = colours([false, false]);
        let (ray_pressed, body_held) = colours([true, true]);
        assert_ne!(ray, ray_pressed);
        assert_ne!(body, body_held);
    }
}
