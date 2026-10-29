use serde::{Deserialize, Serialize};
use solidcraft_geom::{Aabb3, Vec3};

/// Column-major 4×4 matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for (c, col) in r.iter_mut().enumerate() {
            for (rr, cell) in col.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.0[k][rr] * o.0[c][k]).sum();
            }
        }
        Mat4(r)
    }
    /// Transform a point; returns (x, y, z, w).
    pub fn apply(&self, p: Vec3) -> [f64; 4] {
        let v = [p.x, p.y, p.z, 1.0];
        let mut out = [0.0; 4];
        for (r, o) in out.iter_mut().enumerate() {
            *o = (0..4).map(|c| self.0[c][r] * v[c]).sum();
        }
        out
    }
    pub fn to_f32(&self) -> [[f32; 4]; 4] {
        self.0.map(|c| c.map(|x| x as f32))
    }
}

/// Named view orientations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StandardView {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
    Iso,
}

impl StandardView {
    pub fn parse(s: &str) -> Option<StandardView> {
        Some(match s.to_ascii_lowercase().as_str() {
            "front" => StandardView::Front,
            "back" => StandardView::Back,
            "top" => StandardView::Top,
            "bottom" => StandardView::Bottom,
            "left" => StandardView::Left,
            "right" => StandardView::Right,
            "iso" | "home" | "isometric" => StandardView::Iso,
            _ => return None,
        })
    }
    /// (yaw, pitch) in radians. Yaw 0 looks along +Y (front view); the iso view looks from the
    /// front-right-top corner. Pitch is the elevation above the XY plane.
    pub fn angles(self) -> (f64, f64) {
        use std::f64::consts::{FRAC_PI_2, PI};
        match self {
            StandardView::Front => (0.0, 0.0),
            StandardView::Back => (PI, 0.0),
            StandardView::Right => (-FRAC_PI_2, 0.0),
            StandardView::Left => (FRAC_PI_2, 0.0),
            StandardView::Top => (0.0, FRAC_PI_2),
            StandardView::Bottom => (0.0, -FRAC_PI_2),
            StandardView::Iso => (-std::f64::consts::FRAC_PI_4, 0.6154797086703874),
        }
    }
}

/// Smooth ease-in-out on [0, 1] (smoothstep: at its fastest, mid-way, 1.5 times the average
/// speed, so long turns stay gentle).
pub fn ease_in_out(t: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 1.0 };
    t * t * (3.0 - 2.0 * t)
}

/// A unit quaternion (w, x, y, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Quat {
    /// From an orthonormal right-handed basis (the columns of a rotation matrix).
    pub fn from_basis(c0: Vec3, c1: Vec3, c2: Vec3) -> Quat {
        let (m00, m11, m22) = (c0.x, c1.y, c2.z);
        let tr = m00 + m11 + m22;
        let q = if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0;
            Quat { w: 0.25 * s, x: (c1.z - c2.y) / s, y: (c2.x - c0.z) / s, z: (c0.y - c1.x) / s }
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            Quat { w: (c1.z - c2.y) / s, x: 0.25 * s, y: (c1.x + c0.y) / s, z: (c2.x + c0.z) / s }
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            Quat { w: (c2.x - c0.z) / s, x: (c1.x + c0.y) / s, y: 0.25 * s, z: (c2.y + c1.z) / s }
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            Quat { w: (c0.y - c1.x) / s, x: (c2.x + c0.z) / s, y: (c2.y + c1.z) / s, z: 0.25 * s }
        };
        q.normalized()
    }
    pub fn dot(self, o: Quat) -> f64 {
        self.w * o.w + self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn scale(self, k: f64) -> Quat {
        Quat { w: self.w * k, x: self.x * k, y: self.y * k, z: self.z * k }
    }
    fn add(self, o: Quat) -> Quat {
        Quat { w: self.w + o.w, x: self.x + o.x, y: self.y + o.y, z: self.z + o.z }
    }
    pub fn normalized(self) -> Quat {
        let l = self.len();
        if l > 1e-300 && l.is_finite() { self.scale(1.0 / l) } else { Quat { w: 1.0, x: 0.0, y: 0.0, z: 0.0 } }
    }
    /// Shortest-arc spherical interpolation.
    pub fn slerp(self, o: Quat, t: f64) -> Quat {
        let mut d = self.dot(o);
        let o = if d < 0.0 {
            d = -d;
            o.scale(-1.0)
        } else {
            o
        };
        if d > 0.9995 {
            return self.scale(1.0 - t).add(o.scale(t)).normalized();
        }
        let th = d.clamp(-1.0, 1.0).acos();
        let s = th.sin();
        self.scale(((1.0 - t) * th).sin() / s).add(o.scale((t * th).sin() / s)).normalized()
    }
    /// The rotated basis vectors (columns of the rotation matrix).
    pub fn basis(self) -> (Vec3, Vec3, Vec3) {
        let Quat { w, x, y, z } = self;
        (
            Vec3::new(1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + w * z), 2.0 * (x * z - w * y)),
            Vec3::new(2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + w * x)),
            Vec3::new(2.0 * (x * z + w * y), 2.0 * (y * z - w * x), 1.0 - 2.0 * (x * x + y * y)),
        )
    }
}

/// A camera move in progress: from one camera to another over `duration` seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraAnim {
    pub from: Camera,
    pub to: Camera,
    pub start: f64,
    pub duration: f64,
}

impl CameraAnim {
    /// The usual length of a view change, in seconds.
    pub const DURATION: f64 = 0.5;
    /// A view change: half a second, longer for turns past a quarter turn (so a big turn, such
    /// as from the home view to the bottom, never sweeps more than about 9 degrees a frame).
    pub fn new(from: Camera, to: Camera, now: f64) -> CameraAnim {
        let dy = (to.yaw - from.yaw + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        let turn = dy.abs().max((to.pitch - from.pitch).abs());
        let k = if turn.is_finite() { (turn / std::f64::consts::FRAC_PI_2).clamp(1.0, 2.0) } else { 1.0 };
        CameraAnim { from, to, start: now, duration: CameraAnim::DURATION * k }
    }
    /// The camera at time `now`, and whether the move has finished.
    pub fn sample(&self, now: f64) -> (Camera, bool) {
        let t = if self.duration > 0.0 { (now - self.start) / self.duration } else { 1.0 };
        if !(t < 1.0) {
            return (self.to, true);
        }
        (Camera::interpolate(&self.from, &self.to, ease_in_out(t)), false)
    }
}

/// Which world axis points up in the view (#3): Z (the default) or Y, as most STEP files from
/// other systems are modelled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpAxis {
    #[default]
    Z,
    Y,
}

impl UpAxis {
    /// The world's up direction.
    pub fn dir(self) -> Vec3 {
        match self {
            UpAxis::Z => Vec3::Z,
            UpAxis::Y => Vec3::Y,
        }
    }
    /// A world vector in the camera's Z-up frame (yaw turns about the frame's Z).
    pub fn to_frame(self, v: Vec3) -> Vec3 {
        match self {
            UpAxis::Z => v,
            UpAxis::Y => Vec3::new(v.x, -v.z, v.y),
        }
    }
    /// A Z-up frame vector in the world.
    pub fn from_frame(self, v: Vec3) -> Vec3 {
        match self {
            UpAxis::Z => v,
            UpAxis::Y => Vec3::new(v.x, v.z, -v.y),
        }
    }
}

/// Orbit camera around `target`. Yaw and pitch are about the up axis (Z, or Y for a Y-up
/// world): with Y up, the front view looks along −Z and the top view down −Y.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub target: Vec3,
    /// Rotation about Z; 0 = looking along +Y (from the front).
    pub yaw: f64,
    /// Elevation above the XY plane, in [−π/2, π/2]. Roll preserves orientation across poles.
    pub pitch: f64,
    /// Rotation about the viewing direction. Zero keeps the world's up axis vertical.
    #[serde(default)]
    pub roll: f64,
    /// Distance from target to eye.
    pub distance: f64,
    /// Vertical field of view (radians); 0 = orthographic.
    pub fov: f64,
    /// The world's up axis.
    #[serde(default)]
    pub up: UpAxis,
}

impl Default for Camera {
    fn default() -> Self {
        let (yaw, pitch) = StandardView::Iso.angles();
        Camera { target: Vec3::ZERO, yaw, pitch, roll: 0.0, distance: 200.0, fov: 0.0, up: UpAxis::Z }
    }
}

impl Camera {
    /// Direction from the target toward the eye.
    pub fn back(&self) -> Vec3 {
        let (cp, sp) = (self.pitch.cos(), self.pitch.sin());
        self.up.from_frame(Vec3::new(-self.yaw.sin() * cp, -self.yaw.cos() * cp, sp).normalized().unwrap_or(Vec3::Z))
    }
    /// The world's up direction.
    pub fn up_dir(&self) -> Vec3 {
        self.up.dir()
    }
    /// A world vector in the camera's Z-up frame (where yaw and pitch are measured).
    pub fn to_frame(&self, v: Vec3) -> Vec3 {
        self.up.to_frame(v)
    }
    /// A Z-up frame vector in the world.
    pub fn from_frame(&self, v: Vec3) -> Vec3 {
        self.up.from_frame(v)
    }
    pub fn eye(&self) -> Vec3 {
        self.target + self.back() * self.distance
    }
    /// Camera basis: right, up, back (orthonormal).
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let b = self.back();
        let right = self.up.from_frame(Vec3::new(self.yaw.cos(), -self.yaw.sin(), 0.0));
        let up = b.cross(right).normalized().unwrap_or(self.up.dir());
        let (s, c) = self.roll.sin_cos();
        (right * c + up * s, up * c - right * s, b)
    }
    pub fn view(&self) -> Mat4 {
        let (r, u, b) = self.basis();
        let e = self.eye();
        Mat4([[r.x, u.x, b.x, 0.0], [r.y, u.y, b.y, 0.0], [r.z, u.z, b.z, 0.0], [-r.dot(e), -u.dot(e), -b.dot(e), 1.0]])
    }
    /// Half the visible height at the target distance.
    pub fn half_height(&self) -> f64 {
        if self.fov > 0.0 { self.distance * (self.fov * 0.5).tan() } else { self.distance * 0.4 }
    }
    /// Near/far planes that keep a scene of radius `radius` around the target visible.
    pub fn clip(&self, radius: f64) -> (f64, f64) {
        let r = radius.max(1e-3);
        if self.fov > 0.0 {
            ((self.distance - r * 2.0).max(self.distance * 1e-3).max(1e-3), self.distance + r * 2.0)
        } else {
            (self.distance - r * 4.0 - 1.0, self.distance + r * 4.0 + 1.0)
        }
    }
    /// Clip a known scene, retaining the radius-based margin for grids and overlays.
    /// In perspective the orbit distance is not the nearest geometry's depth: a deep assembly
    /// can reach almost to the eye. Cap the usual near plane at half that nearest depth so the
    /// distance-based precision floor cannot clip its front components after a projection change.
    pub fn clip_bounds(&self, bounds: &Aabb3) -> (f64, f64) {
        let fallback = self.clip((self.half_height() * 2.0).max(50.0));
        if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
            return fallback;
        }
        let radius = bounds.diagonal() * 0.5 + bounds.center().dist(self.target);
        if !radius.is_finite() {
            return fallback;
        }
        let (near, far) = self.clip(radius.max(self.half_height() * 2.0).max(10.0));
        if self.fov <= 0.0 {
            return (near, far);
        }
        let back = self.back();
        let nearest = Vec3::new(
            if back.x >= 0.0 { bounds.max.x } else { bounds.min.x },
            if back.y >= 0.0 { bounds.max.y } else { bounds.min.y },
            if back.z >= 0.0 { bounds.max.z } else { bounds.min.z },
        );
        let depth = self.distance + (self.target - nearest).dot(back);
        if !depth.is_finite() {
            return (near, far);
        }
        // A box crossing the eye uses the minimum positive near plane; points behind the eye
        // still cannot be projected. Keep the existing one-micrometre lower limit.
        (near.min((depth * 0.5).max(1e-3)), far)
    }
    /// Projection to clip space (OpenGL/wgpu style, depth 0..1).
    pub fn proj(&self, aspect: f64, near: f64, far: f64) -> Mat4 {
        let aspect = if aspect.is_finite() && aspect > 1e-6 { aspect } else { 1.0 };
        let (n, f) = (near, far.max(near + 1e-6));
        if self.fov > 0.0 {
            let t = 1.0 / (self.fov * 0.5).tan();
            Mat4([[t / aspect, 0.0, 0.0, 0.0], [0.0, t, 0.0, 0.0], [0.0, 0.0, f / (n - f), -1.0], [0.0, 0.0, n * f / (n - f), 0.0]])
        } else {
            let h = self.half_height();
            let w = h * aspect;
            Mat4([[1.0 / w, 0.0, 0.0, 0.0], [0.0, 1.0 / h, 0.0, 0.0], [0.0, 0.0, 1.0 / (n - f), 0.0], [0.0, 0.0, n / (n - f), 1.0]])
        }
    }
    pub fn view_proj(&self, aspect: f64, radius: f64) -> Mat4 {
        let (n, f) = self.clip(radius);
        self.proj(aspect, n, f).mul(&self.view())
    }
    /// View-projection using the current scene bounds to admit nearby components.
    pub fn view_proj_bounds(&self, aspect: f64, bounds: &Aabb3) -> Mat4 {
        let (n, f) = self.clip_bounds(bounds);
        self.proj(aspect, n, f).mul(&self.view())
    }
    /// World point → pixel coordinates (x right, y down) and depth (0 near … 1 far).
    pub fn to_screen(&self, p: Vec3, w: f64, h: f64, radius: f64) -> Option<(f64, f64, f64)> {
        let c = self.view_proj(w / h.max(1.0), radius).apply(p);
        if c[3].abs() < 1e-12 {
            return None;
        }
        let (x, y, z) = (c[0] / c[3], c[1] / c[3], c[2] / c[3]);
        (c[3] > 0.0).then_some(((x + 1.0) * 0.5 * w, (1.0 - y) * 0.5 * h, z))
    }
    /// Picking ray through pixel (x, y): origin and unit direction.
    pub fn ray(&self, x: f64, y: f64, w: f64, h: f64) -> (Vec3, Vec3) {
        let (r, u, b) = self.basis();
        let hh = self.half_height();
        let aspect = w / h.max(1.0);
        let nx = (x / w.max(1.0)) * 2.0 - 1.0;
        let ny = 1.0 - (y / h.max(1.0)) * 2.0;
        if self.fov > 0.0 {
            let d = (r * (nx * hh * aspect) + u * (ny * hh) - b * self.distance).normalized().unwrap_or(-b);
            (self.eye(), d)
        } else {
            let o = self.target + r * (nx * hh * aspect) + u * (ny * hh) + b * (self.distance * 4.0);
            (o, -b)
        }
    }
    /// Zoom by `factor` (< 1 zooms in) toward a world point that stays put on screen.
    pub fn zoom_at(&mut self, factor: f64, anchor: Option<Vec3>) {
        let f = if factor.is_finite() { factor.clamp(0.05, 20.0) } else { 1.0 };
        let nd = (self.distance * f).clamp(1e-3, 1e7);
        if let Some(a) = anchor {
            self.target = a + (self.target - a) * (nd / self.distance);
        }
        self.distance = nd;
    }
    /// Pan by a screen delta in pixels for a viewport of height `h`.
    pub fn pan(&mut self, dx: f64, dy: f64, h: f64) {
        let (r, u, _) = self.basis();
        let k = 2.0 * self.half_height() / h.max(1.0);
        self.target = self.target - r * (dx * k) + u * (dy * k);
    }
    /// Orbit by pixel deltas. The model follows the cursor: dragging right turns the side
    /// facing the viewer to the right.
    pub fn orbit(&mut self, dx: f64, dy: f64) {
        let lim = std::f64::consts::FRAC_PI_2 - 1e-4;
        self.yaw = (self.yaw + dx * 0.008) % std::f64::consts::TAU;
        self.pitch = (self.pitch + dy * 0.008).clamp(-lim, lim);
    }
    /// Orbit by pixel deltas about `pivot`: the camera turns rigidly about it, so the pivot keeps
    /// its place in the camera's frame and on screen, in perspective as in parallel views.
    pub fn orbit_about(&mut self, dx: f64, dy: f64, pivot: Vec3) {
        let keep = self.frame_coords(pivot);
        self.orbit(dx, dy);
        self.place(pivot, keep);
    }
    /// Where `p` lies from the target along the camera's right, up and back axes.
    pub fn frame_coords(&self, p: Vec3) -> Vec3 {
        let (r, u, b) = self.basis();
        let d = p - self.target;
        Vec3::new(d.dot(r), d.dot(u), d.dot(b))
    }
    /// Move the target so `p` lies at `coords` (from [`Camera::frame_coords`]) in the camera's
    /// frame: with the distance unchanged, `p` is where it was on screen.
    pub fn place(&mut self, p: Vec3, coords: Vec3) {
        let (r, u, b) = self.basis();
        self.target = p - (r * coords.x + u * coords.y + b * coords.z);
    }
    /// Rotate the image about the viewing direction.
    pub fn roll_by(&mut self, radians: f64) {
        if radians.is_finite() {
            self.roll = (self.roll + radians) % std::f64::consts::TAU;
        }
    }
    /// Rotate about the current screen right, up and back axes (radians), without a pole
    /// limit. Reconstruct both heading and roll so crossing a pole keeps the image continuous.
    pub fn rotate_in_view(&mut self, radians: Vec3) {
        if !self.is_valid() || !radians.is_finite() {
            return;
        }
        let angle = radians.x.hypot(radians.y).hypot(radians.z);
        if !(angle > 1e-12) || !angle.is_finite() {
            return;
        }
        let (right, up, back) = self.basis();
        let Some(axis) = (right * (radians.x / angle) + up * (radians.y / angle) + back * (radians.z / angle)).normalized() else { return };
        let (sin, cos) = angle.sin_cos();
        let rotate = |v: Vec3| v * cos + axis.cross(v) * sin + axis * (axis.dot(v) * (1.0 - cos));
        let rotated_right = rotate(right);
        let Some(rotated_back) = rotate(back).normalized() else { return };
        let framed = self.up.to_frame(rotated_back);
        let yaw = if framed.x.hypot(framed.y) < 1e-10 { self.yaw } else { (-framed.x).atan2(-framed.y) };
        let pitch = framed.z.clamp(-1.0, 1.0).asin();
        let base_right = self.up.from_frame(Vec3::new(yaw.cos(), -yaw.sin(), 0.0));
        let Some(base_up) = rotated_back.cross(base_right).normalized() else { return };
        let roll = rotated_right.dot(base_up).atan2(rotated_right.dot(base_right));
        if yaw.is_finite() && pitch.is_finite() && roll.is_finite() {
            self.yaw = yaw;
            self.pitch = pitch;
            self.roll = roll;
        }
    }
    /// Orbit by pixel deltas like [`Camera::orbit`], without its pole limit: sideways turns
    /// about the world's up axis and up/down tilts about the screen's right, on over the top.
    /// Upside down (past a pole) sideways turns the other way, so the model still follows the
    /// cursor.
    pub fn orbit_unlimited(&mut self, dx: f64, dy: f64) {
        if !self.is_valid() || !dx.is_finite() || !dy.is_finite() {
            return;
        }
        let upright = self.basis().1.dot(self.up_dir()) >= 0.0;
        let turn = if upright { dx } else { -dx } * 0.008;
        self.yaw = (self.yaw + turn) % std::f64::consts::TAU;
        self.rotate_in_view(Vec3::new(-dy * 0.008, 0.0, 0.0));
    }
    pub fn set_view(&mut self, v: StandardView) {
        let lim = std::f64::consts::FRAC_PI_2 - 1e-6;
        let (y, p) = v.angles();
        self.yaw = y;
        self.pitch = p.clamp(-lim, lim);
        self.roll = 0.0;
    }
    /// The standard axis view this camera faces, if its view direction is within `tol` radians
    /// of one of the six axis views (Front, Back, Left, Right, Top, Bottom). Iso is never
    /// reported. At the poles (Top/Bottom) the yaw is degenerate, so any yaw counts as aligned.
    /// Used by the "perspective while orbiting, orthographic on faces" view mode (#2682).
    pub fn aligned_standard_view(&self, tol: f64) -> Option<StandardView> {
        use std::f64::consts::FRAC_PI_2;
        if !self.yaw.is_finite() || !self.pitch.is_finite() || !(tol > 0.0) {
            return None;
        }
        // Top / Bottom: aligned when the elevation reaches the pole within the tolerance
        // (`set_view` stops a hair short of pi/2, so the slack covers the exact views too).
        if FRAC_PI_2 - self.pitch.abs() <= tol {
            return Some(if self.pitch >= 0.0 { StandardView::Top } else { StandardView::Bottom });
        }
        // The four side views sit at pitch 0 with yaw on a multiple of pi/2.
        if self.pitch.abs() > tol {
            return None;
        }
        let quarter = (self.yaw / FRAC_PI_2).round().rem_euclid(4.0);
        let off = (self.yaw / FRAC_PI_2).rem_euclid(1.0);
        let off = off.min(1.0 - off) * FRAC_PI_2;
        if off > tol {
            return None;
        }
        Some(match quarter as i32 {
            0 => StandardView::Front,
            1 => StandardView::Left,
            2 => StandardView::Back,
            _ => StandardView::Right,
        })
    }
    /// Frame a bounding box.
    pub fn fit(&mut self, b: &Aabb3) {
        if b.is_empty() {
            return;
        }
        self.target = b.center();
        let r = (b.diagonal() * 0.5).max(1.0);
        self.distance = if self.fov > 0.0 { r / (self.fov * 0.5).sin() * 1.1 } else { r / 0.4 * 1.15 };
    }
    /// The same camera looking from direction `back` (target → eye). Straight up or down
    /// keeps the yaw, rounded to the nearest quarter turn so the view stays square.
    pub fn looking_from(&self, back: Vec3) -> Camera {
        let Some(b) = self.up.to_frame(back).normalized() else { return *self };
        let lim = std::f64::consts::FRAC_PI_2 - 1e-6;
        let pitch = b.z.clamp(-1.0, 1.0).asin().clamp(-lim, lim);
        let yaw = if b.x.hypot(b.y) < 1e-6 {
            let q = std::f64::consts::FRAC_PI_2;
            (self.yaw / q).round() * q
        } else {
            (-b.x).atan2(-b.y)
        };
        Camera { yaw, pitch, roll: 0.0, ..*self }
    }
    /// Orientation as a unit quaternion (camera → world rotation).
    pub fn orientation(&self) -> Quat {
        let (r, u, b) = self.basis();
        Quat::from_basis(r, u, b)
    }
    /// Blend between two cameras at `s` in [0, 1]: heading and roll take the shorter way.
    pub fn interpolate(a: &Camera, b: &Camera, s: f64) -> Camera {
        if !(s > 0.0) {
            return *a;
        }
        if s >= 1.0 {
            return *b;
        }
        // Keep heading and elevation smooth near straight up or down. A quaternion slerp
        // swings the heading sharply there when moving to Top.
        let dy = (b.yaw - a.yaw + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        let yaw = a.yaw + dy * s;
        let dr = (b.roll - a.roll + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        let pitch = a.pitch + (b.pitch - a.pitch) * s;
        Camera {
            target: a.target + (b.target - a.target) * s,
            yaw,
            pitch,
            roll: a.roll + dr * s,
            distance: a.distance + (b.distance - a.distance) * s,
            fov: a.fov + (b.fov - a.fov) * s,
            up: b.up,
        }
    }
    pub fn is_valid(&self) -> bool {
        self.target.is_finite()
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.roll.is_finite()
            && self.distance.is_finite()
            && self.distance > 0.0
    }
    /// The view's orientation in degrees, as the viewport's View angles readout shows it (#6032).
    /// These are the camera's own angles: yaw in (−180, 180], pitch in [−90, 90], roll in
    /// (−180, 180]. A camera whose pitch passed a pole is read from its basis, as the same view
    /// with the angles in range; there, straight up or down, yaw and roll turn the image the same
    /// way, so roll reads 0 and yaw carries the turn. An invalid camera reads all zeros.
    pub fn view_angles(&self) -> ViewAngles {
        use std::f64::consts::FRAC_PI_2;
        let angles = if !self.is_valid() {
            (0.0, 0.0, 0.0)
        } else if self.pitch.abs() <= FRAC_PI_2 {
            (wrap_half_turn(self.yaw), self.pitch, wrap_half_turn(self.roll))
        } else {
            let (right, _, back) = self.basis();
            let (right, back) = (self.up.to_frame(right), self.up.to_frame(back));
            let pitch = back.z.clamp(-1.0, 1.0).asin();
            if back.x.hypot(back.y) < 1e-9 {
                // Straight up or down: the camera's right vector lies in the ground plane.
                ((-right.y).atan2(right.x), pitch, 0.0)
            } else {
                let yaw = (-back.x).atan2(-back.y);
                let base_right = Vec3::new(yaw.cos(), -yaw.sin(), 0.0);
                let base_up = back.cross(base_right).normalized().unwrap_or(Vec3::Z);
                (yaw, pitch, right.dot(base_up).atan2(right.dot(base_right)))
            }
        };
        let degrees = |a: f64| if a.is_finite() { a.to_degrees() + 0.0 } else { 0.0 };
        ViewAngles { yaw: degrees(wrap_half_turn(angles.0)), pitch: degrees(angles.1).clamp(-90.0, 90.0), roll: degrees(wrap_half_turn(angles.2)) }
    }
    /// The same camera turned to `angles` (degrees, as [`Camera::view_angles`] reads them):
    /// target, distance, projection and up axis are kept. Yaw and roll may be any number of
    /// turns; pitch is held to ±90°. `None` when an angle is not finite.
    pub fn with_view_angles(&self, angles: ViewAngles) -> Option<Camera> {
        let ViewAngles { yaw, pitch, roll } = angles;
        if !(yaw.is_finite() && pitch.is_finite() && roll.is_finite()) {
            return None;
        }
        let radians = |a: f64| wrap_half_turn(a.to_radians());
        Some(Camera { yaw: radians(yaw), pitch: pitch.clamp(-90.0, 90.0).to_radians(), roll: radians(roll), ..*self })
    }
}

/// A view's orientation in degrees about the up axis (#6032). Yaw is the heading of the line of
/// sight: 0 looks along +Y (Front), 90 along +X (Left), 180 along −Y (Back), −90 along −X
/// (Right). Pitch is the eye's elevation above the ground plane: 90 looks straight down (Top),
/// −90 straight up (Bottom). Roll turns the image about the line of sight; positive turns the
/// model clockwise on screen. With Y up the same angles turn about Y, the front view looking
/// along −Z.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewAngles {
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
}

/// An angle in radians brought into (−π, π]; any finite angle, however many turns.
fn wrap_half_turn(a: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    if a.is_finite() { PI - (PI - a).rem_euclid(TAU) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Orbiting about a point keeps it on its pixel, with and without perspective and roll.
    #[test]
    fn orbit_about_keeps_the_pivot_on_screen() {
        let pivot = Vec3::new(12.0, -7.0, 9.0);
        for fov in [0.0, 0.6] {
            for roll in [0.0, 0.4] {
                let mut cam = Camera { target: Vec3::new(3.0, 4.0, -2.0), distance: 150.0, yaw: 0.3, pitch: 0.5, roll, fov, ..Camera::default() };
                let before = cam.to_screen(pivot, 800.0, 600.0, 100.0).unwrap();
                for _ in 0..10 {
                    cam.orbit_about(25.0, -12.0, pivot);
                }
                let after = cam.to_screen(pivot, 800.0, 600.0, 100.0).unwrap();
                assert!((after.0 - before.0).abs() < 1e-6 && (after.1 - before.1).abs() < 1e-6, "fov {fov}, roll {roll}: {before:?} -> {after:?}");
                assert!((cam.eye() - pivot).len() > 1.0);
            }
        }
    }

    #[test]
    fn screen_rotation_crosses_both_poles_and_returns_after_a_full_turn() {
        for up in [UpAxis::Z, UpAxis::Y] {
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                let mut cam = Camera { yaw: 0.0, pitch: 0.0, up, ..Default::default() };
                let initial = cam;
                let step = std::f64::consts::TAU / 256.0;
                for _ in 0..256 {
                    let before = cam.basis();
                    cam.rotate_in_view(axis * step);
                    let after = cam.basis();
                    assert!(cam.is_valid());
                    // An X or Y turn moves the viewing direction every step, including at a pole.
                    if axis != Vec3::Z {
                        assert!((before.2.dot(after.2) - step.cos()).abs() < 1e-10);
                    }
                    assert!(before.0.dot(after.0) > 0.999 && before.1.dot(after.1) > 0.999);
                    assert!(after.0.cross(after.1).dist(after.2) < 1e-10);
                }
                let (r, u, b) = initial.basis();
                let (rr, uu, bb) = cam.basis();
                assert!(r.dist(rr) < 1e-9 && u.dist(uu) < 1e-9 && b.dist(bb) < 1e-9, "{up:?} {axis:?}: {cam:?}");
                assert_eq!((cam.target, cam.distance, cam.fov, cam.up), (initial.target, initial.distance, initial.fov, initial.up));
            }
        }
    }

    #[test]
    fn screen_rotation_ignores_invalid_motion_and_reverses_combined_axes() {
        let initial = Camera { roll: 1.2, ..Default::default() };
        let mut cam = initial;
        for motion in [Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(0.0, f64::INFINITY, 0.0), Vec3::new(f64::MAX, f64::MAX, 0.0)] {
            cam.rotate_in_view(motion);
            assert_eq!(cam, initial);
        }
        let motion = Vec3::new(0.3, -0.4, 0.2);
        cam.rotate_in_view(motion);
        cam.rotate_in_view(-motion);
        assert!(cam.basis().0.dist(initial.basis().0) < 1e-12 && cam.back().dist(initial.back()) < 1e-12);
    }

    #[test]
    fn front_view_projects_target_to_centre() {
        let mut c = Camera { target: Vec3::new(1.0, 2.0, 3.0), distance: 50.0, ..Default::default() };
        c.set_view(StandardView::Front);
        let (x, y, z) = c.to_screen(c.target, 800.0, 600.0, 10.0).unwrap();
        assert!((x - 400.0).abs() < 1e-6 && (y - 300.0).abs() < 1e-6 && (0.0..=1.0).contains(&z));
        // Front view looks along +Y: +X is to the right, +Z up.
        let (xr, _, _) = c.to_screen(c.target + Vec3::X, 800.0, 600.0, 10.0).unwrap();
        let (_, yu, _) = c.to_screen(c.target + Vec3::Z, 800.0, 600.0, 10.0).unwrap();
        assert!(xr > 400.0 && yu < 300.0);
        let (o, d) = c.ray(400.0, 300.0, 800.0, 600.0);
        assert!(d.dist(Vec3::Y) < 1e-9, "{d:?}");
        assert!((o - c.target).cross(d).len() < 1e-6);
    }

    #[test]
    fn horizontal_orbit_follows_the_cursor() {
        let mut c = Camera { distance: 50.0, ..Default::default() };
        c.set_view(StandardView::Front);
        // Front view looks along +Y, so -Y is the side facing the viewer.
        let near = Vec3::new(0.0, -1.0, 0.0);
        let x0 = c.to_screen(near, 800.0, 600.0, 10.0).unwrap().0;
        c.orbit(20.0, 0.0);
        let x1 = c.to_screen(near, 800.0, 600.0, 10.0).unwrap().0;
        assert!(x1 > x0, "dragging right must move the near side right: {x0} -> {x1}");
    }

    #[test]
    fn roll_rotates_the_picture_and_standard_views_level_it() {
        let mut c = Camera::default();
        c.set_view(StandardView::Front);
        let (_, before_y, _) = c.to_screen(Vec3::X, 800.0, 600.0, 10.0).unwrap();
        c.roll_by(std::f64::consts::FRAC_PI_2);
        let (after_x, after_y, _) = c.to_screen(Vec3::X, 800.0, 600.0, 10.0).unwrap();
        assert!((before_y - 300.0).abs() < 1e-9);
        assert!((after_x - 400.0).abs() < 1e-9 && after_y > 300.0);
        let (r, u, b) = c.basis();
        assert!(r.dot(u).abs() < 1e-12 && r.dot(b).abs() < 1e-12 && u.dot(b).abs() < 1e-12);
        c.set_view(StandardView::Front);
        assert_eq!(c.roll, 0.0);
        let saved = serde_json::to_value(c).unwrap();
        let mut old = saved.as_object().unwrap().clone();
        old.remove("roll");
        let restored: Camera = serde_json::from_value(serde_json::Value::Object(old)).unwrap();
        assert_eq!(restored.roll, 0.0);
    }

    #[test]
    fn view_animation_interpolates() {
        let mut a = Camera { target: Vec3::new(1.0, 2.0, 3.0), distance: 80.0, ..Default::default() };
        a.set_view(StandardView::Iso);
        let mut b = Camera { target: Vec3::new(-5.0, 0.0, 10.0), distance: 120.0, ..Default::default() };
        b.set_view(StandardView::Top);
        let anim = CameraAnim::new(a, b, 10.0);
        // Endpoints are exact.
        assert_eq!(anim.sample(10.0).0, a);
        assert_eq!(anim.sample(10.0 + anim.duration), (b, true));
        assert_eq!(anim.sample(99.0), (b, true));
        // Midpoint: half way in heading, elevation, target and distance.
        let (m, done) = anim.sample(10.0 + anim.duration / 2.0);
        assert!(!done && m.is_valid());
        let dy = (b.yaw - a.yaw + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        assert!((m.yaw - (a.yaw + dy / 2.0)).abs() < 1e-12 && (m.pitch - (a.pitch + b.pitch) / 2.0).abs() < 1e-12);
        assert!(m.target.dist(Vec3::new(-2.0, 1.0, 6.5)) < 1e-9 && (m.distance - 100.0).abs() < 1e-9);
        // Quaternion round trip of a basis.
        let (r, u, bk) = a.basis();
        let (r2, u2, b2) = a.orientation().basis();
        assert!(r.dist(r2) < 1e-12 && u.dist(u2) < 1e-12 && bk.dist(b2) < 1e-12);
        // Ease: fixed ends, symmetric, monotonic.
        assert_eq!((ease_in_out(0.0), ease_in_out(1.0), ease_in_out(0.5)), (0.0, 1.0, 0.5));
        assert!(ease_in_out(0.25) < 0.25 && ease_in_out(0.75) > 0.75);
        // Looking from a corner direction.
        let c = a.looking_from(Vec3::new(1.0, -1.0, 1.0));
        assert!(c.back().dist(Vec3::new(1.0, -1.0, 1.0).normalized().unwrap()) < 1e-9);
    }

    #[test]
    fn top_view_and_perspective() {
        let mut c = Camera { distance: 100.0, fov: 0.8, ..Default::default() };
        c.set_view(StandardView::Top);
        let (xr, _, _) = c.to_screen(Vec3::X, 800.0, 600.0, 10.0).unwrap();
        let (_, yu, _) = c.to_screen(Vec3::Y, 800.0, 600.0, 10.0).unwrap();
        assert!(xr > 400.0 && yu < 300.0, "{xr} {yu}");
        let near = c.to_screen(Vec3::new(0.0, 0.0, 10.0), 800.0, 600.0, 20.0).unwrap().2;
        let far = c.to_screen(Vec3::new(0.0, 0.0, -10.0), 800.0, 600.0, 20.0).unwrap().2;
        assert!(near < far);
        let (o, d) = c.ray(600.0, 300.0, 800.0, 600.0);
        let hit = o + d * (o.z / -d.z);
        let (sx, sy, _) = c.to_screen(hit, 800.0, 600.0, 20.0).unwrap();
        assert!((sx - 600.0).abs() < 1e-6 && (sy - 300.0).abs() < 1e-6);
    }

    /// The #2682 alignment policy: each axis view is detected, off-axis and iso are not, and
    /// the tolerance admits near-aligned cameras (set_view's Top stops a hair short of pi/2).
    #[test]
    fn aligned_standard_view_detects_axis_views_within_tolerance() {
        let tol = 1.0_f64.to_radians();
        for v in [StandardView::Front, StandardView::Back, StandardView::Top, StandardView::Bottom, StandardView::Left, StandardView::Right] {
            let mut cam = Camera::default();
            cam.set_view(v);
            assert_eq!(cam.aligned_standard_view(tol), Some(v), "{v:?}");
        }
        // Iso and off-axis orientations are never aligned.
        let iso = Camera { yaw: StandardView::Iso.angles().0, pitch: StandardView::Iso.angles().1, ..Camera::default() };
        assert_eq!(iso.aligned_standard_view(tol), None);
        let off = Camera { yaw: 0.5, pitch: 0.3, ..Camera::default() };
        assert_eq!(off.aligned_standard_view(tol), None);
        // Within tolerance of Front (yaw and pitch each a half-degree off) it still counts.
        let near = Camera { yaw: 0.5_f64.to_radians(), pitch: 0.5_f64.to_radians(), ..Camera::default() };
        assert_eq!(near.aligned_standard_view(tol), Some(StandardView::Front));
        // Just outside the tolerance it does not.
        let past = Camera { yaw: 2.0_f64.to_radians(), pitch: 0.0, ..Camera::default() };
        assert_eq!(past.aligned_standard_view(tol), None);
        // Yaw wraps: 3*pi/2 is Right, negative yaw works, and at the poles any yaw counts.
        assert_eq!(
            Camera { yaw: 3.0 * std::f64::consts::FRAC_PI_2, pitch: 0.0, ..Camera::default() }.aligned_standard_view(tol),
            Some(StandardView::Right)
        );
        assert_eq!(
            Camera { yaw: -std::f64::consts::FRAC_PI_2, pitch: 0.0, ..Camera::default() }.aligned_standard_view(tol),
            Some(StandardView::Right)
        );
        assert_eq!(
            Camera { yaw: 2.0, pitch: std::f64::consts::FRAC_PI_2 - 1e-6, ..Camera::default() }.aligned_standard_view(tol),
            Some(StandardView::Top)
        );
        assert_eq!(
            Camera { yaw: 2.0, pitch: -std::f64::consts::FRAC_PI_2 + 1e-6, ..Camera::default() }.aligned_standard_view(tol),
            Some(StandardView::Bottom)
        );
        // Non-finite angles and a zero tolerance never align.
        assert_eq!(Camera { yaw: f64::NAN, pitch: 0.0, ..Camera::default() }.aligned_standard_view(tol), None);
        assert_eq!(Camera::default().aligned_standard_view(0.0), None);
    }

    #[test]
    fn large_scene_projection_toggle_keeps_near_and_far_components_in_clip_range() {
        for scale in [1.0, 100.0] {
            let bounds = Aabb3 { min: Vec3::new(-0.25, -10_000.0, -0.25) * scale, max: Vec3::new(0.25, 10_000.0, 0.25) * scale };
            let mut cam = Camera { yaw: 0.0, pitch: 0.0, distance: 10_002.0 * scale, ..Default::default() };
            let original = cam.view_proj_bounds(16.0 / 9.0, &bounds);
            for fov in [0.0, 0.6, 0.0, 0.6, 0.0] {
                cam.fov = fov;
                let vp = cam.view_proj_bounds(16.0 / 9.0, &bounds);
                let gpu = vp.to_f32();
                assert!(gpu.iter().flatten().all(|v| v.is_finite()));
                for x in [bounds.min.x, bounds.max.x] {
                    for y in [bounds.min.y, bounds.max.y] {
                        for z in [bounds.min.z, bounds.max.z] {
                            let p = Vec3::new(x, y, z);
                            let c = vp.apply(p);
                            assert!(c[3] > 0.0 && (0.0..=c[3]).contains(&c[2]), "scale={scale}, fov={fov}, {p:?}: {c:?}");
                            assert!(c[0].abs() <= c[3] && c[1].abs() <= c[3]);
                            let p = p.to_f32();
                            let c: [f32; 4] = std::array::from_fn(|r| gpu[0][r] * p[0] + gpu[1][r] * p[1] + gpu[2][r] * p[2] + gpu[3][r]);
                            assert!(c[3] > 0.0 && (0.0..=c[3]).contains(&c[2]), "GPU scale={scale}, fov={fov}: {c:?}");
                        }
                    }
                }
                if fov == 0.0 {
                    assert_eq!(vp, original);
                }
            }
        }
    }

    #[test]
    fn bounds_clipping_handles_eye_crossing_and_invalid_bounds() {
        let cam = Camera { yaw: 0.0, pitch: 0.0, distance: 10_000.0, fov: 0.6, ..Default::default() };
        let crossing = Aabb3 { min: Vec3::new(-1.0, -10_001.0, -1.0), max: Vec3::new(1.0, 10_001.0, 1.0) };
        let (near, far) = cam.clip_bounds(&crossing);
        assert_eq!(near, 1e-3);
        assert!(far.is_finite() && far > near);
        let c = cam.view_proj_bounds(1.0, &crossing).apply(Vec3::new(0.0, -9_999.0, 0.0));
        assert!(c[3] > 0.0 && (0.0..=c[3]).contains(&c[2]));
        let fallback = cam.clip((cam.half_height() * 2.0).max(50.0));
        for bounds in [
            Aabb3::EMPTY,
            Aabb3 { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::ZERO },
            Aabb3 { min: Vec3::ZERO, max: Vec3::new(f64::INFINITY, 1.0, 1.0) },
            Aabb3 { min: Vec3::new(-f64::MAX, -1.0, -1.0), max: Vec3::new(f64::MAX, 1.0, 1.0) },
        ] {
            assert_eq!(cam.clip_bounds(&bounds), fallback);
            assert!(cam.view_proj_bounds(1.0, &bounds).0.iter().flatten().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn fit_and_hostile() {
        let mut c = Camera::default();
        let b = Aabb3 { min: Vec3::ZERO, max: Vec3::new(40.0, 30.0, 20.0) };
        c.fit(&b);
        assert!(c.target.dist(Vec3::new(20.0, 15.0, 10.0)) < 1e-9);
        c.zoom_at(f64::NAN, None);
        c.orbit(1e9, -1e9);
        assert!(c.is_valid());
        c.fit(&Aabb3::EMPTY);
        assert!(c.is_valid());
    }

    /// A view change sampled every frame (60 fps) moves smoothly to the very end: no frame
    /// turns the view, moves the target or changes the distance by much more than the average
    /// step, and the last frame lands exactly on the target camera.
    #[test]
    fn view_animations_have_no_jumps() {
        let base = Camera { target: Vec3::new(5.0, -3.0, 2.0), distance: 150.0, ..Default::default() };
        let views = [
            StandardView::Iso,
            StandardView::Top,
            StandardView::Front,
            StandardView::Right,
            StandardView::Bottom,
            StandardView::Back,
            StandardView::Left,
        ];
        let mut cams: Vec<Camera> = views
            .iter()
            .map(|v| {
                let mut c = base;
                c.set_view(*v);
                c
            })
            .collect();
        // Corners and edges of the axis view, and a fit (new target and distance).
        cams.push(base.looking_from(Vec3::new(1.0, -1.0, 1.0)));
        cams.push(base.looking_from(Vec3::new(-1.0, 0.0, 1.0)));
        cams.push(base.looking_from(Vec3::new(0.0, 0.0, 1.0)));
        cams.push(Camera { target: Vec3::new(-40.0, 10.0, 0.0), distance: 60.0, ..base });
        for a in &cams {
            for b in &cams {
                let anim = CameraAnim::new(*a, *b, 0.0);
                let frames = (anim.duration * 60.0).round() as usize;
                let mut prev = anim.sample(0.0).0;
                let (mut steps, mut done_at) = (Vec::new(), None);
                for f in 1..=frames + 2 {
                    let (c, done) = anim.sample(f as f64 / 60.0);
                    let turn = c.back().dist(prev.back()) + c.basis().0.dist(prev.basis().0);
                    let moved =
                        c.target.dist(prev.target) / a.distance.max(b.distance) + (c.distance - prev.distance).abs() / a.distance.max(b.distance);
                    steps.push((f, turn, moved));
                    prev = c;
                    if done && done_at.is_none() {
                        done_at = Some(f);
                        assert_eq!(c, *b);
                    }
                }
                assert!(done_at.is_some_and(|f| f >= frames), "ended early at {done_at:?}");
                let total_turn: f64 = steps.iter().map(|s| s.1).sum();
                let total_move: f64 = steps.iter().map(|s| s.2).sum();
                for (f, turn, moved) in &steps {
                    // The eased curve's fastest frame is 1.5x the average: allow 2x.
                    assert!(*turn <= 2.0 * total_turn / frames as f64 + 1e-9, "turn jump at frame {f}: {turn} of {total_turn} ({a:?} -> {b:?})");
                    assert!(*moved <= 2.0 * total_move / frames as f64 + 1e-9, "move jump at frame {f}: {moved} of {total_move}");
                }
            }
        }
    }

    /// With Y up: the front view looks along −Z (eye on +Z), the top view looks down −Y with X
    /// to the right, orbiting turns about Y, and the picture's up is the world's up.
    #[test]
    fn y_up_views() {
        let mut c = Camera { up: UpAxis::Y, distance: 100.0, ..Default::default() };
        c.set_view(StandardView::Front);
        assert!(c.back().dist(Vec3::Z) < 1e-9, "front eye {:?}", c.back());
        let (r, u, b) = c.basis();
        assert!(r.dist(Vec3::X) < 1e-9 && u.dist(Vec3::Y) < 1e-9 && b.dist(Vec3::Z) < 1e-9);
        c.set_view(StandardView::Top);
        assert!(c.back().dist(Vec3::Y) < 1e-5, "top eye {:?}", c.back());
        assert!(c.basis().0.dist(Vec3::X) < 1e-9);
        // Orbiting sideways keeps the eye's height above the XZ ground.
        c.set_view(StandardView::Iso);
        let h = c.back().y;
        c.orbit(120.0, 0.0);
        assert!((c.back().y - h).abs() < 1e-12);
        // Looking from a world direction lands on it.
        let d = Vec3::new(1.0, 1.0, 1.0).normalized().unwrap();
        assert!(c.looking_from(d).back().dist(d) < 1e-9);
        // Frames round-trip.
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert!(UpAxis::Y.from_frame(UpAxis::Y.to_frame(v)).dist(v) < 1e-12);
    }

    fn same_view(a: &Camera, b: &Camera) -> bool {
        let ((ar, au, ab), (br, bu, bb)) = (a.basis(), b.basis());
        ar.dist(br) < 1e-9 && au.dist(bu) < 1e-9 && ab.dist(bb) < 1e-9
    }

    /// The View angles readout (#6032): the named views read round numbers, with Front 0/0/0,
    /// in both up frames.
    #[test]
    fn named_views_read_round_view_angles() {
        let iso_pitch = (1.0_f64 / 3.0_f64.sqrt()).asin().to_degrees();
        for up in [UpAxis::Z, UpAxis::Y] {
            for (view, expected) in [
                (StandardView::Front, (0.0, 0.0, 0.0)),
                (StandardView::Back, (180.0, 0.0, 0.0)),
                (StandardView::Left, (90.0, 0.0, 0.0)),
                (StandardView::Right, (-90.0, 0.0, 0.0)),
                (StandardView::Top, (0.0, 90.0, 0.0)),
                (StandardView::Bottom, (0.0, -90.0, 0.0)),
                (StandardView::Iso, (-45.0, iso_pitch, 0.0)),
            ] {
                let mut cam = Camera { up, roll: 0.7, ..Default::default() };
                cam.set_view(view);
                let a = cam.view_angles();
                assert!(
                    (a.yaw - expected.0).abs() < 1e-3 && (a.pitch - expected.1).abs() < 1e-3 && (a.roll - expected.2).abs() < 1e-9,
                    "{up:?} {view:?}: {a:?}"
                );
            }
        }
        // Yaw is the heading of the line of sight; positive roll turns the model clockwise.
        let cam = Camera::default().with_view_angles(ViewAngles { yaw: 90.0, pitch: 0.0, roll: 0.0 }).unwrap();
        assert!(cam.back().dist(-Vec3::X) < 1e-12, "yaw 90 looks along +X: {:?}", cam.back());
        let cam = Camera::default().with_view_angles(ViewAngles { yaw: 0.0, pitch: 0.0, roll: 30.0 }).unwrap();
        let (x, y, _) = cam.to_screen(cam.target + Vec3::X * 10.0, 800.0, 600.0, 50.0).unwrap();
        assert!(x > 400.0 && y > 300.0, "+X turned clockwise, below the centre: {x}, {y}");
    }

    /// Angles read from a camera turn a camera to the same view, and set angles read back, for
    /// any orientation, including straight up and down and cameras whose stored pitch passed a pole.
    #[test]
    fn view_angles_round_trip_without_nan_at_the_poles() {
        use std::f64::consts::{FRAC_PI_2, TAU};
        for up in [UpAxis::Z, UpAxis::Y] {
            let base = Camera { up, target: Vec3::new(1.0, -2.0, 3.0), distance: 80.0, fov: 0.6, ..Default::default() };
            for yaw in [-180.0, -135.0, -90.0, -12.5, 0.0, 45.0, 90.0, 179.0, 180.0] {
                for pitch in [-90.0, -89.9, -35.0, 0.0, 15.0, 89.9, 90.0] {
                    for roll in [-170.0, -90.0, 0.0, 30.0, 180.0] {
                        let set = ViewAngles { yaw, pitch, roll };
                        let cam = base.with_view_angles(set).unwrap();
                        assert_eq!((cam.target, cam.distance, cam.fov, cam.up), (base.target, base.distance, base.fov, base.up));
                        let read = cam.view_angles();
                        let turn = |a: f64, b: f64| ((a - b + 540.0).rem_euclid(360.0) - 180.0).abs();
                        assert!(
                            turn(read.yaw, yaw) < 1e-9 && (read.pitch - pitch).abs() < 1e-9 && turn(read.roll, roll) < 1e-9,
                            "{set:?} -> {read:?}"
                        );
                        assert!(same_view(&cam, &cam.with_view_angles(read).unwrap()));
                    }
                }
            }
            // Stored angles out of range (a pitch over a pole, many turns) read in range as the same view.
            for (yaw, pitch, roll) in [
                (0.3, 2.0, 0.2),
                (-1.0, -3.0, 1.0),
                (7.0 * TAU + 0.5, 0.4, -9.0 * TAU),
                (0.8, FRAC_PI_2 + TAU, 0.3),
                (-0.6, -FRAC_PI_2 - TAU, 1.1),
                (1e6, 1e6, -1e6),
            ] {
                let cam = Camera { yaw, pitch, roll, ..base };
                let a = cam.view_angles();
                assert!(a.yaw.is_finite() && a.pitch.is_finite() && a.roll.is_finite(), "{cam:?}: {a:?}");
                assert!((-180.0..=180.0).contains(&a.yaw) && (-90.0..=90.0).contains(&a.pitch) && (-180.0..=180.0).contains(&a.roll));
                assert!(same_view(&cam, &cam.with_view_angles(a).unwrap()), "{cam:?}: {a:?}");
            }
        }
    }

    #[test]
    fn hostile_view_angles_are_refused_or_held_in_range() {
        let cam = Camera::default();
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for a in [
                ViewAngles { yaw: bad, ..Default::default() },
                ViewAngles { pitch: bad, ..Default::default() },
                ViewAngles { roll: bad, ..Default::default() },
            ] {
                assert_eq!(cam.with_view_angles(a), None);
            }
            assert_eq!(Camera { yaw: bad, ..cam }.view_angles(), ViewAngles::default());
            assert_eq!(Camera { pitch: bad, ..cam }.view_angles(), ViewAngles::default());
        }
        let huge = cam.with_view_angles(ViewAngles { yaw: f64::MAX, pitch: f64::MAX, roll: -f64::MAX }).unwrap();
        assert!(huge.is_valid() && (huge.pitch - std::f64::consts::FRAC_PI_2).abs() < 1e-12, "{huge:?}");
        let a = huge.view_angles();
        assert!(a.yaw.is_finite() && a.roll.is_finite() && a.pitch == 90.0, "{a:?}");
    }
}
