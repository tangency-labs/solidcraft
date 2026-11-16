//! Where things are in the room, and how each eye sees them.
//!
//! The room is the headset's reference space: metres, +X right, +Y up, −Z forward. The window
//! is a flat rectangle in it ([`Screen`]). The model is placed so that the design view on that
//! window is a pane of glass onto it ([`place_model`]): what the desktop camera shows at its
//! target's distance lies in the window's plane, at the size it has there, and what is nearer or
//! farther stands out of the window or back into it. Each eye then looks at the placed model with
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

/// The cross product, right-handed.
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `a` + `b`.
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a` − `b`.
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a` made `s` times as long.
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// Whether every component of `a` is a finite number.
fn finite(a: [f64; 3]) -> bool {
    a.iter().all(|x| x.is_finite())
}

/// `a` at length 1, or `None` when it has no direction.
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 1e-12).then(|| scale(a, 1.0 / length))
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

/// The window in the room: an upright flat rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    /// Its middle (metres).
    pub center: [f64; 3],
    /// Unit directions along its width and its height.
    pub right: [f64; 3],
    pub up: [f64; 3],
    /// Its size (metres).
    pub width: f64,
    pub height: f64,
}

/// The widest and the narrowest a window may be (metres), and the nearest and farthest its middle
/// may be from the room's origin: beyond these a setting is a slip, not a wish.
pub const SCREEN_WIDTH: std::ops::RangeInclusive<f64> = 0.2..=10.0;
pub const SCREEN_DISTANCE: std::ops::RangeInclusive<f64> = 0.3..=20.0;

impl Screen {
    /// A window `width` metres wide and `aspect` times as wide as high, `distance` metres in front
    /// of the room's origin and facing it, its middle `drop` metres below eye level. Sizes outside
    /// [`SCREEN_WIDTH`] and [`SCREEN_DISTANCE`] are brought into them, and an aspect that is not a
    /// shape is taken as 16:10.
    pub fn in_front(width: f64, aspect: f64, distance: f64, drop: f64) -> Screen {
        let within = |x: f64, range: &std::ops::RangeInclusive<f64>, fallback: f64| {
            if x.is_finite() { x.clamp(*range.start(), *range.end()) } else { fallback }
        };
        let width = within(width, &SCREEN_WIDTH, 1.6);
        let distance = within(distance, &SCREEN_DISTANCE, 1.2);
        let aspect = if aspect.is_finite() && (0.1..=10.0).contains(&aspect) { aspect } else { 1.6 };
        let drop = if drop.is_finite() { drop.clamp(-2.0, 2.0) } else { 0.0 };
        Screen { center: [0.0, -drop, -distance], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], width, height: width / aspect }
    }

    /// The direction out of the window toward whoever looks at it.
    pub fn normal(&self) -> [f64; 3] {
        cross(self.right, self.up)
    }

    /// The room point at `(u, v)` of the window: (0, 0) its top left corner, (1, 1) its bottom
    /// right one.
    pub fn point(&self, u: f64, v: f64) -> [f64; 3] {
        add(self.center, add(scale(self.right, (u - 0.5) * self.width), scale(self.up, (0.5 - v) * self.height)))
    }

    /// The pose a flat layer of the window's size is placed with: its middle, its X along the
    /// width and its Y along the height.
    pub fn pose(&self) -> Pose {
        Pose { position: self.center, orientation: quaternion(self.right, self.up, self.normal()) }
    }
}

/// The unit quaternion (x, y, z, w) of the rotation whose columns are `x`, `y` and `z`.
fn quaternion(x: [f64; 3], y: [f64; 3], z: [f64; 3]) -> [f64; 4] {
    let trace = x[0] + y[1] + z[2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [(y[2] - z[1]) / s, (z[0] - x[2]) / s, (x[1] - y[0]) / s, 0.25 * s]
    } else if x[0] > y[1] && x[0] > z[2] {
        let s = (1.0 + x[0] - y[1] - z[2]).sqrt() * 2.0;
        [0.25 * s, (y[0] + x[1]) / s, (z[0] + x[2]) / s, (y[2] - z[1]) / s]
    } else if y[1] > z[2] {
        let s = (1.0 + y[1] - x[0] - z[2]).sqrt() * 2.0;
        [(y[0] + x[1]) / s, 0.25 * s, (z[1] + y[2]) / s, (z[0] - x[2]) / s]
    } else {
        let s = (1.0 + z[2] - x[0] - y[1]).sqrt() * 2.0;
        [(z[0] + x[2]) / s, (z[1] + y[2]) / s, 0.25 * s, (x[1] - y[0]) / s]
    };
    let length = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    if length.is_finite() && length > 1e-9 { q.map(|c| c / length) } else { [0.0, 0.0, 0.0, 1.0] }
}

/// A part of the window, as fractions of its width and height from its top left corner: the
/// design view's rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pane {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Pane {
    /// The part of a window `window` points in size that the rectangle from `min` to `max`
    /// (points) covers, or `None` when either has no area or a number is not finite.
    pub fn of(min: [f32; 2], max: [f32; 2], window: [f32; 2]) -> Option<Pane> {
        let [w, h] = window.map(f64::from);
        let (min, max) = (min.map(f64::from), max.map(f64::from));
        if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) || min.iter().chain(&max).any(|x| !x.is_finite()) {
            return None;
        }
        let pane = Pane {
            left: (min[0] / w).clamp(0.0, 1.0),
            top: (min[1] / h).clamp(0.0, 1.0),
            right: (max[0] / w).clamp(0.0, 1.0),
            bottom: (max[1] / h).clamp(0.0, 1.0),
        };
        (pane.right - pane.left > 1e-6 && pane.bottom - pane.top > 1e-6).then_some(pane)
    }

    /// The pane's four corners in the room, on `screen`.
    pub fn corners(&self, screen: &Screen) -> [[f64; 3]; 4] {
        [
            screen.point(self.left, self.top),
            screen.point(self.right, self.top),
            screen.point(self.right, self.bottom),
            screen.point(self.left, self.bottom),
        ]
    }
}

/// The desktop camera, as far as placing the model needs it: where it looks, its own directions
/// in the model's space, and how much of the model it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFrame {
    /// The point the camera turns about (model units).
    pub target: [f64; 3],
    /// Its right, up and back (toward the eye) directions: unit and perpendicular.
    pub right: [f64; 3],
    pub up: [f64; 3],
    pub back: [f64; 3],
    /// Half the height it shows at the target's distance (model units).
    pub half_height: f64,
}

/// The model in the room: one turn, one scale and one move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Model point (model units) to room point (metres).
    pub matrix: Mat4,
    /// Metres per model unit.
    pub scale: f64,
}

/// Place the model so that `pane` of `screen` is a pane of glass onto it, as `camera` shows it:
/// the camera's target in the pane's middle, `depth` metres behind the glass (negative: in front
/// of it), the camera's right and up along the window's, and the height the camera shows at its
/// target filling the pane's height. `None` when the camera or the pane has no size to go by.
pub fn place_model(camera: &CameraFrame, pane: &Pane, screen: &Screen, depth: f64) -> Option<Placement> {
    let (right, up, back) = (unit(camera.right)?, unit(camera.up)?, unit(camera.back)?);
    let pane_height = (pane.bottom - pane.top) * screen.height;
    if !(camera.half_height.is_finite() && camera.half_height > 0.0 && pane_height.is_finite() && pane_height > 0.0)
        || !finite(camera.target)
        || !depth.is_finite()
    {
        return None;
    }
    let s = pane_height / (2.0 * camera.half_height);
    if !(s.is_finite() && s > 0.0) {
        return None;
    }
    let (sr, su, sn) = (unit(screen.right)?, unit(screen.up)?, unit(screen.normal())?);
    let middle = sub(screen.point((pane.left + pane.right) * 0.5, (pane.top + pane.bottom) * 0.5), scale(sn, depth));
    // A model direction `d` goes to s · (sr (d·right) + su (d·up) + sn (d·back)).
    let column = |k: usize| scale(add(add(scale(sr, right[k]), scale(su, up[k])), scale(sn, back[k])), s);
    let (x, y, z) = (column(0), column(1), column(2));
    let t = camera.target;
    let moved = sub(middle, add(add(scale(x, t[0]), scale(y, t[1])), scale(z, t[2])));
    let matrix = [[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [moved[0], moved[1], moved[2], 1.0]];
    matrix.iter().flatten().all(|v| v.is_finite()).then_some(Placement { matrix, scale: s })
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

/// Where the point `p` shows in a picture drawn with `m`: x from −1 at the left edge to 1 at the
/// right, y from −1 at the bottom to 1 at the top. `None` for a point behind the eye or one that
/// has no finite place.
pub fn project(m: &Mat4, p: [f64; 3]) -> Option<[f64; 2]> {
    let [x, y, _, w] = apply(m, p);
    let at = [x / w, y / w];
    (w.is_finite() && w > 1e-6 && at[0].is_finite() && at[1].is_finite()).then_some(at)
}

/// A flat outline in the room, parallel to `screen`: `outline` is in points from the outline's
/// own origin (x to the right, y down, as a UI draws), which is put at `origin`, at `metres` a
/// point. This is how the mouse pointer is drawn on the model's surface: the same arrow the
/// window draws, at the depth of what it points at.
pub fn outline_in_room(origin: [f64; 3], screen: &Screen, outline: &[[f64; 2]], metres: f64) -> Vec<[f64; 3]> {
    outline.iter().map(|[x, y]| add(origin, add(scale(screen.right, x * metres), scale(screen.up, -y * metres)))).collect()
}

/// How large to draw something at `at` so that it looks as large as it does at `reference`, seen
/// from `from`: the ratio of the two distances. 1 when either point is at the viewer or a number
/// is not finite.
pub fn same_apparent_size(from: [f64; 3], reference: [f64; 3], at: [f64; 3]) -> f64 {
    let (near, far) = (dot(sub(reference, from), sub(reference, from)).sqrt(), dot(sub(at, from), sub(at, from)).sqrt());
    let ratio = far / near;
    if ratio.is_finite() && ratio > 1e-6 && near > 1e-6 { ratio } else { 1.0 }
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

/// The rectangle of an eye's picture (x, y, width, height in pixels of a `size` picture) that
/// holds the room points `corners`: what the model is drawn within, so that it shows through the
/// design view and not around the window. `None` when the corners are not all in front of the
/// eye or the rectangle is empty: nothing of the pane can be seen.
pub fn pane_scissor(eye: &Eye, corners: &[[f64; 3]; 4], size: [u32; 2]) -> Option<[u32; 4]> {
    let room = room_view_proj(eye)?;
    let (w, h) = (f64::from(size[0]), f64::from(size[1]));
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for corner in corners {
        let [x, y, _, cw] = apply(&room, *corner);
        if !(cw.is_finite() && cw > 1e-6) {
            return None;
        }
        let px = [(x / cw + 1.0) * 0.5 * w, (1.0 - y / cw) * 0.5 * h];
        if !px[0].is_finite() || !px[1].is_finite() {
            return None;
        }
        lo = [lo[0].min(px[0]), lo[1].min(px[1])];
        hi = [hi[0].max(px[0]), hi[1].max(px[1])];
    }
    let (x0, y0) = (lo[0].floor().clamp(0.0, w), lo[1].floor().clamp(0.0, h));
    let (x1, y1) = (hi[0].ceil().clamp(0.0, w), hi[1].ceil().clamp(0.0, h));
    (x1 > x0 && y1 > y0).then_some([x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An eye at `position` looking down −Z with a square field of view of 45° to each side.
    fn eye_at(position: [f64; 3]) -> Eye {
        let a = std::f64::consts::FRAC_PI_4;
        Eye { pose: Pose { position, ..Pose::IDENTITY }, fov: Fov { left: -a, right: a, up: a, down: -a } }
    }

    /// A camera looking along −Z at the origin, showing 100 units of height.
    fn front_camera() -> CameraFrame {
        CameraFrame { target: [0.0; 3], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0], back: [0.0, 0.0, 1.0], half_height: 50.0 }
    }

    /// The whole of a window as a pane.
    const WHOLE: Pane = Pane { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0 };

    /// Whether two points are the same but for rounding.
    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        sub(a, b).iter().all(|d| d.abs() < 1e-9)
    }

    /// The window sits in front of the room's origin, upright and facing it, at the size asked.
    #[test]
    fn the_window_faces_the_origin_at_the_size_asked() {
        let s = Screen::in_front(2.0, 2.0, 1.5, 0.1);
        assert_eq!((s.width, s.height), (2.0, 1.0));
        assert!(close(s.center, [0.0, -0.1, -1.5]));
        assert!(close(s.normal(), [0.0, 0.0, 1.0]));
        assert!(close(s.point(0.0, 0.0), [-1.0, 0.4, -1.5]));
        assert!(close(s.point(1.0, 1.0), [1.0, -0.6, -1.5]));
        assert_eq!(s.pose().orientation, [0.0, 0.0, 0.0, 1.0]);
    }

    /// Sizes that are slips (zero, negative, huge, not a number) give a window that can be seen.
    #[test]
    fn a_window_size_that_is_a_slip_is_brought_into_range() {
        for (width, aspect, distance, drop) in [
            (0.0, 0.0, 0.0, 0.0),
            (-3.0, -1.0, -2.0, 99.0),
            (1e9, 1e9, 1e9, -1e9),
            (f64::NAN, f64::NAN, f64::NAN, f64::NAN),
            (f64::INFINITY, 1.0, 1.0, 0.0),
        ] {
            let s = Screen::in_front(width, aspect, distance, drop);
            assert!(SCREEN_WIDTH.contains(&s.width), "{width}: {s:?}");
            assert!(s.height.is_finite() && s.height > 0.0, "{aspect}: {s:?}");
            assert!(SCREEN_DISTANCE.contains(&-s.center[2]), "{distance}: {s:?}");
            assert!(s.center[1].abs() <= 2.0, "{drop}: {s:?}");
        }
    }

    /// The flat layer's pose turns with the window: its X along the width, its Y along the height.
    #[test]
    fn a_turned_window_has_a_pose_with_its_own_axes() {
        let turned = Screen { center: [1.0, 2.0, 3.0], right: [0.0, 0.0, -1.0], up: [0.0, 1.0, 0.0], width: 1.0, height: 1.0 };
        let [r, u, b] = turned.pose().axes().unwrap();
        assert!(close(r, turned.right) && close(u, turned.up) && close(b, turned.normal()), "{r:?} {u:?} {b:?}");
        let tilted = Screen { center: [0.0; 3], right: [1.0, 0.0, 0.0], up: [0.0, 0.0, -1.0], width: 1.0, height: 1.0 };
        let [r, u, b] = tilted.pose().axes().unwrap();
        assert!(close(r, tilted.right) && close(u, tilted.up) && close(b, tilted.normal()), "{r:?} {u:?} {b:?}");
    }

    /// The camera's target lands in the pane's middle, what it shows fills the pane's height, and
    /// its directions lie along the window's.
    #[test]
    fn the_model_is_placed_as_the_camera_shows_it() {
        let screen = Screen::in_front(2.0, 2.0, 1.0, 0.0);
        let p = place_model(&front_camera(), &WHOLE, &screen, 0.0).unwrap();
        assert!((p.scale - 0.01).abs() < 1e-12, "100 units fill 1 m: {}", p.scale);
        let at = |m: [f64; 3]| {
            let [x, y, z, _] = apply(&p.matrix, m);
            [x, y, z]
        };
        assert!(close(at([0.0; 3]), screen.center));
        assert!(close(at([0.0, 50.0, 0.0]), screen.point(0.5, 0.0)), "the top of the view is the pane's top edge");
        assert!(close(at([100.0, 0.0, 0.0]), screen.point(1.0, 0.5)));
        assert!(close(at([0.0, 0.0, 10.0]), add(screen.center, [0.0, 0.0, 0.1])), "nearer the camera stands out of the window");
    }

    /// A pane that is part of the window, a turned camera with a target away from the origin, and
    /// a depth: the target is behind the pane's middle and the scale follows the pane's height.
    #[test]
    fn a_pane_and_a_turned_camera_place_the_target_behind_the_pane() {
        let screen = Screen::in_front(2.0, 2.0, 1.0, 0.0);
        let pane = Pane { left: 0.5, top: 0.0, right: 1.0, bottom: 0.5 };
        // Looking down the model's −X, with the model's Z up.
        let camera = CameraFrame { target: [5.0, 6.0, 7.0], right: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], back: [1.0, 0.0, 0.0], half_height: 25.0 };
        let p = place_model(&camera, &pane, &screen, 0.2).unwrap();
        assert!((p.scale - 0.01).abs() < 1e-12, "50 units fill half of 1 m: {}", p.scale);
        let at = |m: [f64; 3]| {
            let [x, y, z, _] = apply(&p.matrix, m);
            [x, y, z]
        };
        assert!(close(at(camera.target), [0.5, 0.25, -1.2]), "{:?}", at(camera.target));
        assert!(close(at([5.0, 6.0, 17.0]), [0.5, 0.35, -1.2]), "the model's up is the window's up");
        assert!(close(at([15.0, 6.0, 7.0]), [0.5, 0.25, -1.1]), "toward the camera is toward the viewer");
    }

    /// A camera or a pane without a size, and numbers that are not finite, place nothing.
    #[test]
    fn nothing_is_placed_without_a_size_to_go_by() {
        let screen = Screen::in_front(2.0, 2.0, 1.0, 0.0);
        let camera = front_camera();
        for half_height in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(place_model(&CameraFrame { half_height, ..camera }, &WHOLE, &screen, 0.0), None, "{half_height}");
        }
        assert_eq!(place_model(&CameraFrame { right: [0.0; 3], ..camera }, &WHOLE, &screen, 0.0), None);
        assert_eq!(place_model(&CameraFrame { target: [f64::NAN, 0.0, 0.0], ..camera }, &WHOLE, &screen, 0.0), None);
        assert_eq!(place_model(&camera, &Pane { left: 0.2, top: 0.5, right: 0.9, bottom: 0.5 }, &screen, 0.0), None);
        assert_eq!(place_model(&camera, &WHOLE, &screen, f64::NAN), None);
        assert_eq!(place_model(&camera, &WHOLE, &Screen { height: 0.0, ..screen }, 0.0), None);
    }

    /// A pane is the design view's share of the window; one without area, or outside numbers, is none.
    #[test]
    fn a_pane_is_a_share_of_the_window() {
        let pane = Pane::of([100.0, 50.0], [900.0, 550.0], [1000.0, 600.0]).unwrap();
        assert!((pane.left - 0.1).abs() < 1e-6 && (pane.right - 0.9).abs() < 1e-6, "{pane:?}");
        assert!((pane.top - 50.0 / 600.0).abs() < 1e-6 && (pane.bottom - 550.0 / 600.0).abs() < 1e-6, "{pane:?}");
        assert_eq!(Pane::of([-50.0, -50.0], [2000.0, 900.0], [1000.0, 600.0]), Some(WHOLE), "clamped to the window");
        assert_eq!(Pane::of([100.0, 50.0], [100.0, 550.0], [1000.0, 600.0]), None);
        assert_eq!(Pane::of([100.0, 50.0], [900.0, 550.0], [0.0, 600.0]), None);
        assert_eq!(Pane::of([f32::NAN, 50.0], [900.0, 550.0], [1000.0, 600.0]), None);
        assert_eq!(Pane::of([100.0, 50.0], [900.0, 550.0], [f32::INFINITY, 600.0]), None);
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
        let placement = place_model(&front_camera(), &WHOLE, &Screen::in_front(2.0, 2.0, 1.0, 0.0), 0.0).unwrap();
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
            assert_eq!(pane_scissor(&Eye { fov, ..good }, &WHOLE.corners(&Screen::in_front(2.0, 2.0, 1.0, 0.0)), [100, 100]), None, "{fov:?}");
        }
        assert_eq!(good.fov.projection(0.0, 10.0), None);
        assert_eq!(good.fov.projection(1.0, 1.0), None);
    }

    /// The eye's place in the model's space is where the placement puts it back to: the lighting
    /// and the section cap are worked out from there.
    #[test]
    fn the_eye_is_found_in_the_models_space() {
        let screen = Screen::in_front(2.0, 2.0, 1.0, 0.0);
        let camera = CameraFrame { target: [5.0, 6.0, 7.0], right: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], back: [1.0, 0.0, 0.0], half_height: 25.0 };
        let placement = place_model(&camera, &WHOLE, &screen, 0.0).unwrap();
        let eye = eye_at([0.3, -0.2, 0.4]);
        let view = eye_view(&eye, &placement).unwrap();
        let [x, y, z, _] = apply(&placement.matrix, view.eye);
        assert!(close([x, y, z], eye.pose.position), "{:?}", [x, y, z]);
        // The target is in the window's middle, straight ahead of an eye at the origin.
        let centred = eye_view(&eye_at([0.0; 3]), &placement).unwrap();
        let [x, y, _, w] = apply(&centred.view_proj, camera.target);
        assert!(w > 0.0 && (x / w).abs() < 1e-9 && (y / w).abs() < 1e-9);
    }

    /// A point shows where the eye sees it: straight ahead in the middle, 45° right at the right
    /// edge; a point behind the eye, or one that is not a number, shows nowhere.
    #[test]
    fn a_point_is_projected_into_an_eyes_picture() {
        let m = room_view_proj(&eye_at([0.0; 3])).unwrap();
        let ahead = project(&m, [0.0, 0.0, -3.0]).unwrap();
        assert!(ahead[0].abs() < 1e-12 && ahead[1].abs() < 1e-12, "{ahead:?}");
        let right_up = project(&m, [3.0, 1.5, -3.0]).unwrap();
        assert!((right_up[0] - 1.0).abs() < 1e-9 && (right_up[1] - 0.5).abs() < 1e-9, "{right_up:?}");
        assert_eq!(project(&m, [0.0, 0.0, 3.0]), None, "behind the eye");
        assert_eq!(project(&m, [0.0; 3]), None, "at the eye");
        assert_eq!(project(&m, [f64::NAN, 0.0, -1.0]), None);
        assert_eq!(room_view_proj(&Eye { pose: Pose { position: [0.0; 3], orientation: [0.0; 4] }, ..eye_at([0.0; 3]) }), None);
    }

    /// The pointer on the model: the window's own arrow, laid parallel to the window at the point
    /// it points at, its x along the window's width and its y down the window's height.
    #[test]
    fn an_outline_is_laid_in_the_room_parallel_to_the_window() {
        let screen = Screen::in_front(2.0, 2.0, 1.0, 0.0);
        let laid = outline_in_room([0.5, 0.25, -1.4], &screen, &[[0.0, 0.0], [10.0, 0.0], [0.0, 20.0]], 0.001);
        assert_eq!(laid.len(), 3);
        assert!(close(laid[0], [0.5, 0.25, -1.4]), "the outline's origin is the point");
        assert!(close(laid[1], [0.51, 0.25, -1.4]), "x runs along the window's width");
        assert!(close(laid[2], [0.5, 0.23, -1.4]), "y runs down the window");
        assert!(outline_in_room([0.0; 3], &screen, &[], 0.001).is_empty());
    }

    /// Something twice as far away is drawn twice as large to look the same size; a point at the
    /// viewer, or numbers that are not finite, leave the size alone.
    #[test]
    fn a_farther_point_is_drawn_larger_to_look_the_same() {
        let (from, window) = ([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        assert!((same_apparent_size(from, window, [0.0, 0.0, -2.0]) - 2.0).abs() < 1e-12);
        assert!((same_apparent_size(from, window, [0.0, 0.0, -0.5]) - 0.5).abs() < 1e-12);
        assert_eq!(same_apparent_size(from, window, window), 1.0);
        assert_eq!(same_apparent_size(from, from, window), 1.0, "the reference is at the viewer");
        assert_eq!(same_apparent_size(from, window, from), 1.0, "the point is at the viewer");
        assert_eq!(same_apparent_size(from, window, [f64::NAN, 0.0, 0.0]), 1.0);
        assert_eq!(same_apparent_size([f64::INFINITY, 0.0, 0.0], window, [0.0, 0.0, -2.0]), 1.0);
    }

    /// The model is drawn within the pane as the eye sees it: its rectangle in the eye's picture.
    #[test]
    fn the_pane_is_a_rectangle_of_the_eyes_picture() {
        // A 1 m square window 1 m ahead: to an eye that sees 45° each way (2 m across there), the
        // middle half of the picture.
        let screen = Screen::in_front(1.0, 1.0, 1.0, 0.0);
        let corners = WHOLE.corners(&screen);
        // The rectangle is rounded outward to whole pixels, so an edge that falls on a pixel's
        // border may take that pixel too: each side within one pixel, and never short.
        let covers = |got: [u32; 4], want: [u32; 4]| {
            let (g, w) = ([got[0], got[1], got[0] + got[2], got[1] + got[3]], [want[0], want[1], want[0] + want[2], want[1] + want[3]]);
            g[0] <= w[0] && g[1] <= w[1] && g[2] >= w[2] && g[3] >= w[3] && g.iter().zip(&w).all(|(a, b)| a.abs_diff(*b) <= 1)
        };
        let centred = pane_scissor(&eye_at([0.0; 3]), &corners, [1000, 800]).unwrap();
        assert!(covers(centred, [250, 200, 500, 400]), "{centred:?}");
        // From the left the window is to the right in the picture, and cut off at its edge.
        let from_left = pane_scissor(&eye_at([-1.0, 0.0, 0.0]), &corners, [1000, 800]).unwrap();
        assert!(covers(from_left, [750, 200, 250, 400]), "{from_left:?}");
        assert_eq!(from_left[0] + from_left[2], 1000, "cut off at the picture's edge");
        // Beside the eye's view altogether, or behind it: nothing to draw in.
        assert_eq!(pane_scissor(&eye_at([5.0, 0.0, 0.0]), &corners, [1000, 800]), None);
        assert_eq!(pane_scissor(&eye_at([0.0, 0.0, -2.0]), &corners, [1000, 800]), None);
        assert_eq!(pane_scissor(&eye_at([0.0; 3]), &corners, [0, 0]), None);
    }
}
