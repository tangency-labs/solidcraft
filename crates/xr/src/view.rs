//! Where things are in the room, and how each eye sees them.
//!
//! The room is the headset's reference space: metres, +X right, +Y up, −Z forward. The model is
//! placed in it by a [`Placement`] (one turn, one scale, one move), and each eye looks at it with
//! its own pose and field of view ([`eye_view`]).
//!
//! Matrices are column-major (`m[column][row]`), as the viewport's own are, with depth 0 at the
//! near plane and 1 at the far one.

/// A column-major 4 × 4 matrix: `m[column][row]`.
pub type Mat4 = [[f64; 4]; 4];

/// The matrix that changes nothing.
pub const IDENTITY: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// The nearest an eye sees (metres): nearer than a hand held to the face.
pub const NEAR: f64 = 0.03;

/// The farthest an eye sees (metres).
pub const FAR: f64 = 200.0;

/// `a` then-applied-after `b`: the matrix that applies `b` first, then `a`.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (c, column) in out.iter_mut().enumerate() {
        for (r, cell) in column.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    out
}

/// The point `p` moved by `m`, as (x, y, z, w).
pub fn apply(m: &Mat4, p: [f64; 3]) -> [f64; 4] {
    let v = [p[0], p[1], p[2], 1.0];
    std::array::from_fn(|r| (0..4).map(|c| m[c][r] * v[c]).sum())
}

/// `m` in the single precision the GPU takes.
pub fn to_f32(m: &Mat4) -> [[f32; 4]; 4] {
    m.map(|column| column.map(|x| x as f32))
}

/// Each column's depth turned round (w − z) for a reversed depth buffer, worked out in f64 as the
/// viewport's own is: with a near plane tiny beside the scene, z and w differ only past an f32's
/// digits.
pub fn turned_depth_f32(m: &Mat4) -> [f32; 4] {
    m.map(|column| (column[3] - column[2]) as f32)
}

/// The dot product.
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `a` − `b`.
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Whether every component of `a` is a finite number.
fn finite(a: [f64; 3]) -> bool {
    a.iter().all(|x| x.is_finite())
}

/// A position and an orientation in the room: what a headset reports for an eye, and what a flat
/// layer is placed with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// Metres.
    pub position: [f64; 3],
    /// A unit quaternion (x, y, z, w).
    pub orientation: [f64; 4],
}

impl Pose {
    /// At the room's origin, looking down −Z.
    pub const IDENTITY: Pose = Pose { position: [0.0; 3], orientation: [0.0, 0.0, 0.0, 1.0] };

    /// The pose's own right, up and back directions in the room, or `None` when the orientation is
    /// not a rotation (a headset that lost tracking may report zeros).
    pub fn axes(&self) -> Option<[[f64; 3]; 3]> {
        let [x, y, z, w] = self.orientation;
        let length = (x * x + y * y + z * z + w * w).sqrt();
        if !(length.is_finite() && length > 1e-6) || !finite(self.position) {
            return None;
        }
        let (x, y, z, w) = (x / length, y / length, z / length, w / length);
        Some([
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)],
            [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)],
            [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)],
        ])
    }

    /// Room to this pose's own space (the view matrix of an eye at this pose).
    pub fn view(&self) -> Option<Mat4> {
        let [r, u, b] = self.axes()?;
        let p = self.position;
        Some([[r[0], u[0], b[0], 0.0], [r[1], u[1], b[1], 0.0], [r[2], u[2], b[2], 0.0], [-dot(r, p), -dot(u, p), -dot(b, p), 1.0]])
    }
}

/// How far an eye sees to each side of where it looks (radians; left and down are negative).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fov {
    pub left: f64,
    pub right: f64,
    pub up: f64,
    pub down: f64,
}

impl Fov {
    /// The tangents of the four angles, or `None` for a field of view no picture can be drawn
    /// for: angles that are not finite, reach a right angle, or leave no width or height.
    fn tangents(&self) -> Option<[f64; 4]> {
        let angles = [self.left, self.right, self.up, self.down];
        if angles.iter().any(|a| !a.is_finite() || a.abs() >= std::f64::consts::FRAC_PI_2 - 1e-3) {
            return None;
        }
        let [left, right, up, down] = angles.map(f64::tan);
        (right - left > 1e-6 && up - down > 1e-6).then_some([left, right, up, down])
    }

    /// The projection for this field of view between `near` and `far` (metres): depth 0 at `near`,
    /// 1 at `far`. `None` for a field of view or a depth range no picture can be drawn for.
    pub fn projection(&self, near: f64, far: f64) -> Option<Mat4> {
        let [left, right, up, down] = self.tangents()?;
        if !(near.is_finite() && far.is_finite() && near > 0.0 && far > near) {
            return None;
        }
        let (width, height) = (right - left, up - down);
        Some([
            [2.0 / width, 0.0, 0.0, 0.0],
            [0.0, 2.0 / height, 0.0, 0.0],
            [(right + left) / width, (up + down) / height, far / (near - far), -1.0],
            [0.0, 0.0, near * far / (near - far), 0.0],
        ])
    }
}

/// One eye at the time a frame is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eye {
    pub pose: Pose,
    pub fov: Fov,
}

/// The model in the room: one turn, one scale and one move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Model point (model units) to room point (metres).
    pub matrix: Mat4,
    /// Metres per model unit.
    pub scale: f64,
}

/// What the renderer needs to draw the placed model for one eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EyeView {
    /// Model point to the eye's clip space.
    pub view_proj: Mat4,
    /// The eye in the model's space.
    pub eye: [f64; 3],
}

/// Room point to `eye`'s clip space, or `None` when the eye's pose or field of view is not one a
/// picture can be drawn for.
pub fn room_view_proj(eye: &Eye) -> Option<Mat4> {
    Some(mul(&eye.fov.projection(NEAR, FAR)?, &eye.pose.view()?))
}

/// The view of `placement` from `eye`, or `None` when the eye's pose or field of view is not one
/// a picture can be drawn for (tracking lost, a runtime's zeros).
pub fn eye_view(eye: &Eye, placement: &Placement) -> Option<EyeView> {
    let room = room_view_proj(eye)?;
    let view_proj = mul(&room, &placement.matrix);
    // The eye in the model's space: the placement turned back.
    let m = &placement.matrix;
    let s2 = placement.scale * placement.scale;
    let offset = sub(eye.pose.position, [m[3][0], m[3][1], m[3][2]]);
    let column = |k: usize| [m[k][0], m[k][1], m[k][2]];
    let eye_model = [dot(offset, column(0)) / s2, dot(offset, column(1)) / s2, dot(offset, column(2)) / s2];
    (view_proj.iter().flatten().all(|v| v.is_finite()) && finite(eye_model)).then_some(EyeView { view_proj, eye: eye_model })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An eye at `position` looking down −Z with a square field of view of 45° to each side.
    fn eye_at(position: [f64; 3]) -> Eye {
        let a = std::f64::consts::FRAC_PI_4;
        Eye { pose: Pose { position, ..Pose::IDENTITY }, fov: Fov { left: -a, right: a, up: a, down: -a } }
    }

    /// A model a metre in front of the room's origin at a millimetre to the millimetre, turned a
    /// quarter about the vertical.
    fn placed() -> Placement {
        let s = 0.001;
        Placement { matrix: [[0.0, 0.0, -s, 0.0], [0.0, s, 0.0, 0.0], [s, 0.0, 0.0, 0.0], [0.0, 0.0, -1.0, 1.0]], scale: s }
    }

    /// Whether two points are the same but for rounding.
    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        sub(a, b).iter().all(|d| d.abs() < 1e-9)
    }

    /// An eye sees the point it looks at in the middle of its picture, the right of the room on
    /// the right, up at the top, and nearer points at a smaller depth.
    #[test]
    fn an_eye_sees_the_room_the_right_way_round() {
        let eye = eye_at([0.0, 0.0, 0.0]);
        let room = mul(&eye.fov.projection(NEAR, FAR).unwrap(), &eye.pose.view().unwrap());
        let ndc = |p: [f64; 3]| {
            let [x, y, z, w] = apply(&room, p);
            [x / w, y / w, z / w]
        };
        let ahead = ndc([0.0, 0.0, -2.0]);
        assert!(ahead[0].abs() < 1e-12 && ahead[1].abs() < 1e-12, "{ahead:?}");
        assert!((ndc([2.0, 0.0, -2.0])[0] - 1.0).abs() < 1e-9, "45° to the right is the picture's right edge");
        assert!((ndc([0.0, 2.0, -2.0])[1] - 1.0).abs() < 1e-9, "45° up is the picture's top edge");
        assert!(ndc([0.0, 0.0, -NEAR])[2].abs() < 1e-9 && (ndc([0.0, 0.0, -FAR])[2] - 1.0).abs() < 1e-9);
        assert!(ndc([0.0, 0.0, -1.0])[2] < ndc([0.0, 0.0, -3.0])[2]);
    }

    /// A field of view that is wider to one side puts straight ahead off the picture's middle.
    #[test]
    fn a_lopsided_field_of_view_moves_the_middle() {
        let fov = Fov { left: -(2.0_f64).atan(), right: (1.0_f64).atan(), up: 0.5, down: -0.5 };
        let p = fov.projection(0.1, 100.0).unwrap();
        let [x, _, _, w] = apply(&p, [0.0, 0.0, -1.0]);
        assert!((x / w - 1.0 / 3.0).abs() < 1e-9, "straight ahead is a third right of the middle: {}", x / w);
        let [x, _, _, w] = apply(&p, [-2.0, 0.0, -1.0]);
        assert!((x / w + 1.0).abs() < 1e-9);
    }

    /// An eye that has turned sees what is in front of it then.
    #[test]
    fn a_turned_eye_looks_where_it_faces() {
        // A quarter turn to the left about +Y: the eye now looks down −X.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let eye = Eye { pose: Pose { position: [1.0, 0.0, 0.0], orientation: [0.0, h, 0.0, h] }, ..eye_at([0.0; 3]) };
        let room = mul(&eye.fov.projection(NEAR, FAR).unwrap(), &eye.pose.view().unwrap());
        let [x, y, _, w] = apply(&room, [-4.0, 0.0, 0.0]);
        assert!(w > 0.0 && (x / w).abs() < 1e-9 && (y / w).abs() < 1e-9, "{x} {y} {w}");
        let [_, _, _, w] = apply(&room, [6.0, 0.0, 0.0]);
        assert!(w < 0.0, "behind the eye");
    }

    /// Poses and fields of view a headset reports when it has nothing to report draw no picture.
    #[test]
    fn a_pose_or_a_field_of_view_that_is_not_one_gives_no_view() {
        let placement = placed();
        let good = eye_at([0.0; 3]);
        assert!(eye_view(&good, &placement).is_some());
        let zeros = Pose { position: [0.0; 3], orientation: [0.0; 4] };
        assert_eq!(eye_view(&Eye { pose: zeros, ..good }, &placement), None);
        let nan = Pose { position: [f64::NAN, 0.0, 0.0], ..Pose::IDENTITY };
        assert_eq!(eye_view(&Eye { pose: nan, ..good }, &placement), None);
        for fov in [
            Fov { left: 0.0, right: 0.0, up: 0.5, down: -0.5 },
            Fov { left: 0.5, right: -0.5, up: 0.5, down: -0.5 },
            Fov { left: -0.5, right: 0.5, up: f64::NAN, down: -0.5 },
            Fov { left: -std::f64::consts::FRAC_PI_2, right: 0.5, up: 0.5, down: -0.5 },
        ] {
            assert_eq!(eye_view(&Eye { fov, ..good }, &placement), None, "{fov:?}");
            assert_eq!(room_view_proj(&Eye { fov, ..good }), None, "{fov:?}");
        }
        assert_eq!(good.fov.projection(0.0, 10.0), None);
        assert_eq!(good.fov.projection(1.0, 1.0), None);
    }

    /// The eye's place in the model's space is where the placement puts it back to: the lighting
    /// and the section cap are worked out from there. A model point placed straight ahead of an
    /// eye at the origin is in the middle of its picture.
    #[test]
    fn the_eye_is_found_in_the_models_space() {
        let placement = placed();
        let eye = eye_at([0.3, -0.2, 0.4]);
        let view = eye_view(&eye, &placement).unwrap();
        let [x, y, z, _] = apply(&placement.matrix, view.eye);
        assert!(close([x, y, z], eye.pose.position), "{:?}", [x, y, z]);
        let centred = eye_view(&eye_at([0.0; 3]), &placement).unwrap();
        let [x, y, _, w] = apply(&centred.view_proj, [0.0; 3]);
        assert!(w > 0.0 && (x / w).abs() < 1e-9 && (y / w).abs() < 1e-9);
        let [x, y, z, _] = apply(&placement.matrix, [100.0, 0.0, 0.0]);
        assert!(close([x, y, z], [0.0, 0.0, -1.1]), "the model's X turned to the room's −Z: {:?}", [x, y, z]);
    }
}
