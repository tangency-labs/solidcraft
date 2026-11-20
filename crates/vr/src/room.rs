//! Where the model is in the room.
//!
//! The room is the headset's reference space (metres, +Y up, −Z forward where the user faced at
//! the start). The model is in its own units. [`Transform`] takes a model point to the room:
//! turned, scaled by one number (never stretched), and moved.

use solidcraft_xr::view::{Placement, Pose};

use crate::vector::{add, cross, dot, finite, length, scale, sub, turn, unit};

/// The smallest scale the model is shown at (metres per model unit): a kilometre-long design in
/// millimetres is then a metre long.
pub const MIN_SCALE: f64 = 1e-6;

/// Real size (metres per model unit): the model's units are millimetres.
pub const REAL_SIZE: f64 = 0.001;

/// The largest scale (metres per model unit): a millimetre is then ten metres.
pub const MAX_SCALE: f64 = 10.0;

/// The farthest the model's middle ([`Transform::pivot`]) goes from the room's origin (metres):
/// beyond it a picture's numbers lose their digits, and the model is out of sight anyway.
pub const MAX_DISTANCE: f64 = 1000.0;

/// The size the model first appears at: its largest side this long (metres), a model on a table.
pub const TABLE_SIZE: f64 = 0.4;

/// How far in front of the head the model first appears (metres).
pub const FIRST_DISTANCE: f64 = 0.6;

/// How far below the eyes the model's middle first appears (metres): where hands work.
pub const FIRST_DROP: f64 = 0.3;

/// A model point to a room point: `translation + scale · axes · p`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Where the model's X, Y and Z point in the room: unit and perpendicular, right-handed.
    pub axes: [[f64; 3]; 3],
    /// Metres per model unit, within [`MIN_SCALE`] and [`MAX_SCALE`].
    pub scale: f64,
    /// Where the model's origin is in the room (metres).
    pub translation: [f64; 3],
    /// The model's middle (model units): what is kept within [`MAX_DISTANCE`], wherever the
    /// model's own origin is.
    pub pivot: [f64; 3],
}

impl Transform {
    /// The model as it is, its origin at the room's, at [`REAL_SIZE`].
    pub const MILLIMETRES: Transform =
        Transform { axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], scale: REAL_SIZE, translation: [0.0; 3], pivot: [0.0; 3] };

    /// Where model point `p` is in the room.
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        add(self.translation, scale(turn(&self.axes, p), self.scale))
    }

    /// Where room point `p` is in the model (model units): [`Transform::apply`] undone.
    pub fn to_model(&self, p: [f64; 3]) -> [f64; 3] {
        scale(self.direction_to_model(sub(p, self.translation)), 1.0 / self.scale)
    }

    /// Room direction `d` in the model's own directions, its length kept (the axes only turn).
    pub fn direction_to_model(&self, d: [f64; 3]) -> [f64; 3] {
        self.axes.map(|axis| dot(axis, d))
    }

    /// The transform as the renderer's placement (model units to metres).
    pub fn placement(&self) -> Placement {
        let [x, y, z] = self.axes.map(|a| scale(a, self.scale));
        let t = self.translation;
        Placement { matrix: [[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [t[0], t[1], t[2], 1.0]], scale: self.scale }
    }

    /// The transform kept usable: its axes made unit and perpendicular again (turns add up small
    /// errors) and its scale within [`MIN_SCALE`] and [`MAX_SCALE`]. `None` when the axes or
    /// numbers are past repair, or the model's middle would be farther than [`MAX_DISTANCE`]:
    /// a move that far is refused, not cut short, so a held point never slips out of the hand.
    pub fn tidy(&self) -> Option<Transform> {
        let x = unit(self.axes[0])?;
        let y = unit(sub(self.axes[1], scale(x, dot(x, self.axes[1]))))?;
        let z = cross(x, y);
        // A mirrored set of axes is not a turn.
        if dot(z, self.axes[2]) <= 0.0 || !self.scale.is_finite() || !finite(self.translation) || !finite(self.pivot) {
            return None;
        }
        let tidy = Transform { axes: [x, y, z], scale: self.scale.clamp(MIN_SCALE, MAX_SCALE), ..*self };
        let middle = length(tidy.apply(tidy.pivot));
        (middle.is_finite() && middle <= MAX_DISTANCE).then_some(tidy)
    }
}

/// The turn about the room's vertical by `angle` (radians; positive turns +X toward −Z), as the
/// columns of its matrix.
pub fn turn_about_vertical(angle: f64) -> [[f64; 3]; 3] {
    let (s, c) = angle.sin_cos();
    [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]]
}

/// Where the model first appears, as the user sees it on the screen: upright (the model's `up`
/// along the room's), the side the desktop camera looks at (`toward_viewer`, from the model to
/// the camera) facing the head, [`TABLE_SIZE`] across, its middle [`FIRST_DISTANCE`] in front of
/// the head and [`FIRST_DROP`] below it.
///
/// `bounds` is the model's box (least and greatest corner, model units); without one (an empty
/// design) the model's origin takes the middle's place at a millimetre to the millimetre. `None`
/// when `up` has no direction or `head` is not a pose.
pub fn first_transform(bounds: Option<[[f64; 3]; 2]>, up: [f64; 3], toward_viewer: [f64; 3], head: &Pose) -> Option<Transform> {
    first_transform_at(bounds, up, toward_viewer, head, [FIRST_DISTANCE, TABLE_SIZE, FIRST_DROP])
}

/// [`first_transform`] with where the model first appears given: `[distance, size, drop]` in
/// place of [`FIRST_DISTANCE`], [`TABLE_SIZE`] and [`FIRST_DROP`] (metres).
pub fn first_transform_at(bounds: Option<[[f64; 3]; 2]>, up: [f64; 3], toward_viewer: [f64; 3], head: &Pose, place: [f64; 3]) -> Option<Transform> {
    let [first_distance, table_size, first_drop] = place;
    let model_up = unit(up)?;
    // The model's side toward the viewer, flattened onto the model's ground. Seen from straight
    // above or below, any side across the up direction does.
    let level = |v: [f64; 3]| unit(sub(v, scale(model_up, dot(v, model_up))));
    let model_front = level(toward_viewer).or_else(|| level([0.0, -1.0, 0.0])).or_else(|| level([0.0, 0.0, 1.0]))?;
    let model_side = cross(model_up, model_front);

    let [_, _, head_back] = head.axes()?;
    let room_up = [0.0, 1.0, 0.0];
    let room_front = unit([head_back[0], 0.0, head_back[2]]).unwrap_or([0.0, 0.0, 1.0]);
    let room_side = cross(room_up, room_front);

    // Model side, up and front to the room's: the turn is room · modelᵀ.
    let model = [model_side, model_up, model_front];
    let room = [room_side, room_up, room_front];
    let axes = [0, 1, 2].map(|k| turn(&room, [model[0][k], model[1][k], model[2][k]]));

    let (middle, extent) = match bounds.filter(|[lo, hi]| finite(*lo) && finite(*hi) && (0..3).all(|k| lo[k] <= hi[k])) {
        Some([lo, hi]) => (scale(add(lo, hi), 0.5), (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max)),
        None => ([0.0; 3], 0.0),
    };
    let s = if extent > 0.0 && table_size > 0.0 { (table_size / extent).clamp(MIN_SCALE, MAX_SCALE) } else { Transform::MILLIMETRES.scale };
    let at = add(add(head.position, scale(room_front, -first_distance)), [0.0, -first_drop, 0.0]);
    Transform { axes, scale: s, translation: sub(at, scale(turn(&axes, middle), s)), pivot: middle }.tidy()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether two points are the same but for rounding.
    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9)
    }

    /// A head at eye height looking down −Z, turned by `yaw` about the vertical.
    fn head(yaw: f64) -> Pose {
        let (s, c) = (yaw * 0.5).sin_cos();
        Pose { position: [0.2, 1.6, -0.1], orientation: [0.0, s, 0.0, c] }
    }

    /// A Z-up model seen from the front (the camera at −Y): its middle in front of the head and
    /// below the eyes, its largest side [`TABLE_SIZE`] long, Z up the room, and its front (−Y)
    /// toward the head.
    #[test]
    fn the_model_first_appears_upright_in_front_of_the_head() {
        let bounds = [[-50.0, -10.0, 0.0], [50.0, 10.0, 20.0]];
        let t = first_transform(Some(bounds), [0.0, 0.0, 1.0], [0.0, -1.0, 0.0], &head(0.0)).unwrap();
        assert!((t.scale - TABLE_SIZE / 100.0).abs() < 1e-12);
        assert!(close(t.apply([0.0, 0.0, 10.0]), [0.2, 1.6 - FIRST_DROP, -0.1 - FIRST_DISTANCE]));
        assert!(close(turn(&t.axes, [0.0, 0.0, 1.0]), [0.0, 1.0, 0.0]), "Z is up");
        assert!(close(turn(&t.axes, [0.0, -1.0, 0.0]), [0.0, 0.0, 1.0]), "the front faces the head");
        assert!(close(turn(&t.axes, [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]), "right stays right");
    }

    /// The head turned a quarter to the left: the model is in front of it there, still facing it.
    /// A Y-up model, seen from above, stands with Y up all the same.
    #[test]
    fn the_model_follows_the_heading_and_the_up_axis() {
        let quarter = std::f64::consts::FRAC_PI_2;
        let t = first_transform(Some([[0.0; 3], [10.0; 3]]), [0.0, 0.0, 1.0], [0.0, -1.0, 0.0], &head(quarter)).unwrap();
        assert!(close(t.apply([5.0; 3]), [0.2 - FIRST_DISTANCE, 1.6 - FIRST_DROP, -0.1]));
        assert!(close(turn(&t.axes, [0.0, -1.0, 0.0]), [1.0, 0.0, 0.0]), "the front faces the head, now to the left");

        let y_up = first_transform(Some([[0.0; 3], [10.0; 3]]), [0.0, 1.0, 0.0], [0.0, 1.0, 0.0], &head(0.0)).unwrap();
        assert!(close(turn(&y_up.axes, [0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]), "Y is up");
    }

    /// No box, a flat box, a box that is not one, an up that is no direction, a head that is no
    /// pose: the model still appears where it can, or not at all; never at a scale past the
    /// limits.
    #[test]
    fn the_first_transform_refuses_what_is_not_a_model_or_a_pose() {
        let up = [0.0, 0.0, 1.0];
        let front = [0.0, -1.0, 0.0];
        let empty = first_transform(None, up, front, &head(0.0)).unwrap();
        assert_eq!(empty.scale, Transform::MILLIMETRES.scale);
        assert!(close(empty.apply([0.0; 3]), [0.2, 1.6 - FIRST_DROP, -0.1 - FIRST_DISTANCE]));
        let point = first_transform(Some([[1.0; 3], [1.0; 3]]), up, front, &head(0.0)).unwrap();
        assert_eq!(point.scale, Transform::MILLIMETRES.scale);
        let turned = first_transform(Some([[1.0; 3], [0.0; 3]]), up, front, &head(0.0)).unwrap();
        assert_eq!(turned.scale, Transform::MILLIMETRES.scale, "a box inside out is none");
        let nan = first_transform(Some([[f64::NAN; 3], [1.0; 3]]), up, front, &head(0.0)).unwrap();
        assert_eq!(nan.scale, Transform::MILLIMETRES.scale);
        // A design a thousand kilometres across: as small as the model gets, its middle still in
        // front of the head.
        let huge = first_transform(Some([[0.0; 3], [1e9; 3]]), up, front, &head(0.0)).unwrap();
        assert_eq!(huge.scale, MIN_SCALE);
        assert!(close(huge.apply(huge.pivot), [0.2, 1.6 - FIRST_DROP, -0.1 - FIRST_DISTANCE]));
        let tiny = first_transform(Some([[0.0; 3], [1e-30; 3]]), up, front, &head(0.0)).unwrap();
        assert_eq!(tiny.scale, MAX_SCALE);
        assert!(first_transform(None, [0.0; 3], front, &head(0.0)).is_none());
        assert!(first_transform(None, [f64::NAN; 3], front, &head(0.0)).is_none());
        let lost = Pose { position: [0.0; 3], orientation: [0.0; 4] };
        assert!(first_transform(None, up, front, &lost).is_none());
        // Looking straight down the up axis: some side faces the head, and the model is upright.
        let above = first_transform(None, up, [0.0, 0.0, 5.0], &head(0.0)).unwrap();
        assert!(close(turn(&above.axes, up), [0.0, 1.0, 0.0]));
    }

    /// A model far from its own origin (a part placed a kilometre out in its assembly) still
    /// appears in front of the head: its middle is placed, not its origin.
    #[test]
    fn a_model_far_from_its_origin_appears_in_front() {
        let bounds = [[1e6, 0.0, 0.0], [1e6 + 100.0, 100.0, 100.0]];
        let t = first_transform(Some(bounds), [0.0, 0.0, 1.0], [0.0, -1.0, 0.0], &head(0.0)).unwrap();
        assert!((t.scale - TABLE_SIZE / 100.0).abs() < 1e-12);
        assert_eq!(t.pivot, [1e6 + 50.0, 50.0, 50.0]);
        let middle = t.apply(t.pivot);
        assert!(close(middle, [0.2, 1.6 - FIRST_DROP, -0.1 - FIRST_DISTANCE]), "{middle:?}");
    }

    /// The placement is the transform as a matrix: the renderer puts every model point where
    /// [`Transform::apply`] does.
    #[test]
    fn the_placement_matches_the_transform() {
        let t = Transform { axes: turn_about_vertical(0.7), scale: 0.002, translation: [1.0, 2.0, 3.0], pivot: [0.0; 3] };
        let p = t.placement();
        assert_eq!(p.scale, 0.002);
        for point in [[0.0; 3], [10.0, -20.0, 30.0], [1e3, 5.0, -7.0]] {
            let [x, y, z, w] = solidcraft_xr::view::apply(&p.matrix, point);
            assert_eq!(w, 1.0);
            assert!(close([x, y, z], t.apply(point)));
        }
    }

    /// Tidying makes turned axes square again, keeps the scale within its limits, refuses axes
    /// that are no turn, and refuses a model whose middle would be out of reach, wherever its
    /// origin is.
    #[test]
    fn tidy_keeps_a_transform_usable() {
        let mut axes = turn_about_vertical(0.3);
        axes[0][1] += 1e-4;
        let t = Transform { axes, scale: 1e9, translation: [1.0, -2.0, 0.5], pivot: [0.0; 3] }.tidy().unwrap();
        for i in 0..3 {
            assert!((dot(t.axes[i], t.axes[i]) - 1.0).abs() < 1e-12);
            assert!(dot(t.axes[i], t.axes[(i + 1) % 3]).abs() < 1e-12);
        }
        assert_eq!(t.scale, MAX_SCALE);
        assert_eq!(t.translation, [1.0, -2.0, 0.5], "the origin is not moved");
        // The origin far away, the middle near: kept.
        let far_origin = Transform { translation: [-5000.0, 0.0, 0.0], pivot: [5e6, 0.0, 0.0], ..Transform::MILLIMETRES };
        assert_eq!(far_origin.tidy(), Some(far_origin));
        // The middle far away: refused.
        assert!(Transform { translation: [MAX_DISTANCE + 1.0, 0.0, 0.0], ..Transform::MILLIMETRES }.tidy().is_none());
        assert!(Transform { pivot: [f64::NAN; 3], ..Transform::MILLIMETRES }.tidy().is_none());
        let mirrored = Transform { axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]], ..Transform::MILLIMETRES };
        assert!(mirrored.tidy().is_none());
        assert!(Transform { axes: [[0.0; 3]; 3], ..Transform::MILLIMETRES }.tidy().is_none());
        assert!(Transform { scale: f64::NAN, ..Transform::MILLIMETRES }.tidy().is_none());
        assert!(Transform { translation: [f64::INFINITY, 0.0, 0.0], ..Transform::MILLIMETRES }.tidy().is_none());
    }

    /// A room point taken into the model's units lands where [`Transform::apply`] took it from,
    /// and a room direction keeps its length.
    #[test]
    fn to_model_undoes_apply() {
        // At real size a metre up: room (0.1, 1.2, 0.3) is model (100, 200, 300).
        let up = Transform { translation: [0.0, 1.0, 0.0], ..Transform::MILLIMETRES };
        assert!(close(up.to_model([0.1, 1.2, 0.3]), [100.0, 200.0, 300.0]));
        // Turned a quarter about the vertical, the model's X points down the room's −Z.
        let turned = Transform { axes: turn_about_vertical(std::f64::consts::FRAC_PI_2), ..up };
        assert!(close(turned.to_model([0.0, 1.0, -0.001]), [1.0, 0.0, 0.0]));
        assert!(close(turned.direction_to_model([0.0, 0.0, -3.0]), [3.0, 0.0, 0.0]));
        let t = Transform { axes: turn_about_vertical(0.7), scale: 0.002, translation: [1.0, 2.0, 3.0], pivot: [0.0; 3] };
        for p in [[0.0; 3], [10.0, -20.0, 30.0], [1e3, 5.0, -7.0]] {
            assert!(close(t.to_model(t.apply(p)), p), "{p:?}");
        }
        let d = t.direction_to_model([3.0, 4.0, 0.0]);
        assert!((length(d) - 5.0).abs() < 1e-12);
    }

    /// Where the model first appears can be set: the defaults give [`first_transform`], and
    /// other numbers place and size it as they say.
    #[test]
    fn the_first_place_can_be_set() {
        let bounds = Some([[-50.0, -10.0, 0.0], [50.0, 10.0, 20.0]]);
        let (up, front) = ([0.0, 0.0, 1.0], [0.0, -1.0, 0.0]);
        let defaults = first_transform_at(bounds, up, front, &head(0.0), [FIRST_DISTANCE, TABLE_SIZE, FIRST_DROP]);
        assert_eq!(defaults, first_transform(bounds, up, front, &head(0.0)));
        let far = first_transform_at(bounds, up, front, &head(0.0), [1.0, 0.2, 0.0]).unwrap();
        assert!((far.scale - 0.2 / 100.0).abs() < 1e-15, "100 mm across shown 0.2 m across");
        assert!(close(far.apply(far.pivot), [0.2, 1.6, -1.1]), "a metre ahead, at eye height");
    }

    /// A size that is no size shows the model at real size; a distance or drop that is no
    /// number, or that puts it out of reach, places nothing.
    #[test]
    fn a_first_place_that_is_no_place_is_refused() {
        let bounds = Some([[0.0; 3], [100.0; 3]]);
        let (up, front) = ([0.0, 0.0, 1.0], [0.0, -1.0, 0.0]);
        let at = |place| first_transform_at(bounds, up, front, &head(0.0), place);
        for size in [0.0, -1.0, f64::NAN] {
            assert_eq!(at([0.6, size, 0.3]).unwrap().scale, Transform::MILLIMETRES.scale, "{size}");
        }
        assert_eq!(at([0.6, f64::INFINITY, 0.3]).unwrap().scale, MAX_SCALE, "endless: as big as it gets");
        assert_eq!(at([f64::NAN, 0.4, 0.3]), None);
        assert_eq!(at([0.6, 0.4, f64::INFINITY]), None);
        assert_eq!(at([MAX_DISTANCE * 2.0, 0.4, 0.3]), None, "out of reach");
        // Behind the head (a negative distance) is still a place.
        assert!(close(at([-1.0, 0.4, 0.0]).unwrap().apply([50.0; 3]), [0.2, 1.6, 0.9]));
    }
}
